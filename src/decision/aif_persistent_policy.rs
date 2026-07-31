//! Persistent-agent multimodal Active Inference coalition-decision strategy
//! (feature `decision`), the `aif-pers` arm of pre-registration K4-v4
//! (`docs/prereg-K4-v4-persistent-aif.md`, including **Amendment 1**, the spec of
//! record).
//!
//! Where [`AifMmDecisionPolicy`](super::AifMmDecisionPolicy) rebuilds a *stateless*
//! multimodal POMDP per decision from binary union coverage — and is provably
//! decision-equivalent to the scalar bridge (the K4-v3 theorem) — this arm holds a
//! **persistent** 8-factor / 8-modality world model that learns per-bit provider
//! reliability across the task stream (Scope B's `perf` signal). Each per-decision
//! `G` query is a *fresh* POMDP built from the persistent agent's **learned**
//! matrices/beliefs, so the theorem's premises (binary coverage, deterministic B,
//! static G) are all broken by decision-relevant structure earned from outcomes.
//!
//! # Architecture (D1: persistent model + fresh queries)
//!
//! - **Persistent agent** (one per seed, [`PersistentAifArm::new`]): 8 two-state
//!   factors (one per capability bit: providers reliable=0 / unreliable=1), 8
//!   three-outcome modalities `{success=0, failure=1, no_obs=2}`; per-factor
//!   single-control sticky B `[[0.9,0.1],[0.1,0.9]]`; uniform D; MMP window;
//!   `learn_a`/`learn_b`/`learn_d` + both novelty flags. It **observes once per
//!   task** ([`PersistentAifArm::observe_outcome`]) and never `act()`s.
//! - **Query POMDP** (Amendment A1.1, fresh per decision): `r = |required|`
//!   bit-drift factors (2-state, one control, learned per-bit B) + one membership
//!   factor (2-state, two controls, deterministic B). The decision is the
//!   deterministic marginal action posterior `p(control 1)` after replaying the
//!   persistent agent's last two task outcomes (A1.4).
//!
//! The arm never samples — decisions are deterministic belief/EFE arithmetic. See
//! the prereg §Determinism.
//!
//! # Count injection (aif 0.11.0, prereg Amendment 2)
//!
//! An earlier engine gap (aif ≤ 0.10.1) made the registered arm decision-dead:
//! `initial_precision` seeded pA row-uniform per joint column, and the first
//! `learn_a` write-back `A = normalize(pA)` erased any coverage-structured A —
//! `p(control 1) ≈ 0.5` regardless of coverage. aif-v0.11.0 adds
//! `AgentParams::initial_pa` / `initial_pb` (direct Dirichlet-count injection;
//! `A/B ≡ column-normalize(counts)` from step 0, structure preserved under the
//! learn write-back — pinned upstream by `test_structured_pa_learn_a_not_flattened`).
//!
//! Per Amendment A2.2 both models now inject counts exactly:
//! - **Persistent agent** — `initial_pa` = the A1.2 anchor counts, so the
//!   reliable/unreliable asymmetry survives `learn_a` (without this its `pa()` would
//!   flatten and the query's covered blocks could not discriminate). pB keeps the
//!   structure-preserving scale path (`initial_precision_b = s_b·B`).
//! - **Query** — `initial_pa` = coverage-masked persistent pA blocks (covered bit →
//!   the persistent modality's counts; uncovered → the flat block at unit
//!   concentration); `initial_pb` = the persistent per-bit pB verbatim for the bit
//!   factors, plus unit-scaled deterministic counts for the membership factor
//!   (structural zeros ⇒ B-novelty exactly 0 under the 0.10.0 mask). A1.3's
//!   mean-concentration approximations are void; novelty magnitudes are now exact.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use nalgebra::{DMatrix, DVector};

use crate::algorithms::AgentCapabilities;

use super::{CoalitionDecisionPolicy, Decision, DecisionContext};

// --- Fixed model constants -------------------------------------------------

/// Number of capability bits the persistent world model tracks (8-bit universe).
const N_BITS: usize = 8;
/// Persistent joint-state count: `2^N_BITS`.
const N_JOINT_PERSISTENT: usize = 1 << N_BITS;
/// Outcomes per modality: `{success, failure, no_obs}`.
const N_OUTCOMES: usize = 3;
const SUCCESS: usize = 0;
const FAILURE: usize = 1;
const NO_OBS: usize = 2;

/// Persistent initial A anchors (Amendment A1.2): asymmetric so the state-label
/// symmetry a flat A cannot break is broken from construction. Columns are
/// `[success, failure, no_obs]`, no-obs row uniform at `u = 0.5`.
const RELIABLE_COL: [f64; N_OUTCOMES] = [0.45, 0.05, 0.50];
const UNRELIABLE_COL: [f64; N_OUTCOMES] = [0.30, 0.20, 0.50];

/// Sticky self-transition probability of the per-factor persistent B (ε = 0.1).
const STICKY: f64 = 0.9;

/// Query per-modality preference `C` (Amendment A1.1), no-obs at the ln-midpoint
/// of success/failure. Normalized at construction (entries must be in `(0, 1]`).
const C_RAW: [f64; N_OUTCOMES] = [0.9, 0.1, 0.3];

/// Query action precision `α` (prereg §Arms 3): softmax temperature over the
/// marginalized action posterior that the decision reads.
const QUERY_ALPHA: f64 = 8.0;

/// Query MMP window / policy configuration (prereg §Arms 3, Amendment A1.1).
const QUERY_HORIZON: usize = 2;
const QUERY_ITERS: usize = 10;
const QUERY_POLICY_DEPTH: usize = 2;

/// Persistent MMP window (prereg §Arms 3).
const PERSISTENT_HORIZON: usize = 2;
const PERSISTENT_ITERS: usize = 10;

/// Persistent pD Dirichlet concentration scale.
const PERSISTENT_INITIAL_PRECISION_D: f64 = 1.0;

/// Replay window depth (Amendment A1.4): the query replays the last `REPLAY_CAP`
/// task outcomes before reading the posterior.
const REPLAY_CAP: usize = 2;

// --- Configuration ---------------------------------------------------------

/// When the persistent agent's MMP window / precision state is reset within a
/// seed (design note D3; prereg D5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrialBoundary {
    /// Reset after every task ([`PersistentAifArm::observe_outcome`] calls
    /// `reset_window`) — the exploratory ablation E4.
    PerTask,
    /// Never reset within a seed — the **registered** arm (D5): β and the MMP
    /// window persist across the whole 20-task stream.
    PerStream,
}

/// Registered-arm configuration plus the exploratory toggles E5–E8. The
/// [`Default`] is the **registered** `aif-pers` arm (prereg §Arms 3).
#[derive(Debug, Clone, Copy)]
pub struct PersistentAifConfig {
    /// Trial boundary (registered: [`TrialBoundary::PerStream`]; E4: `PerTask`).
    pub trial_boundary: TrialBoundary,
    /// Persistent-agent learning (registered: `true`; E5 sets `false` — the agent
    /// accumulates nothing and queries fall back to the unlearned A anchors).
    pub persistent_learning: bool,
    /// Query precision dynamics (registered: `true` — MMP + `PrecisionDynamics`;
    /// E6 sets `false` — MeanField queries with fixed γ).
    pub query_dynamics: bool,
    /// Query novelty terms (registered: `true`; E7 sets `false`).
    pub query_novelty: bool,
    /// Persistent pB Dirichlet concentration scale (registered: `4.0`; E8 sweeps
    /// `1.0 / 4.0 / 16.0`).
    pub initial_precision_b: f64,
    /// Per-task eviction cap (koalisi #56, K4-v6). **Identity default `None`** —
    /// unlimited leaves, the #53 arm bit-for-bit. `Some(c)`: at most `c` leave-acts
    /// per task; once the cap is exhausted (including `c = 0` from the first leave
    /// consult) the leave path returns `Decision { act: false, score: 0.0 }`
    /// WITHOUT constructing a query (no engine call, no `decision_counter`
    /// increment). `Some(0)` = **never-evict** (churn `0` by construction — the
    /// registered v6 point). The arm still learns normally via `observe_outcome`.
    pub eviction_cap: Option<u32>,
    /// Rejoin lockout in tasks (koalisi #56, exploratory). **Identity default `0`**
    /// (off). When `k > 0`, an agent whose leave decision returned `act == true`
    /// during task `t` (i.e. `tasks_observed == t` at that moment) is barred from
    /// JOINING while `tasks_observed > t && tasks_observed <= t + k`: `should_join`
    /// returns `Decision { act: false, score: 0.0 }` without a query (no counter
    /// increment). Within-task rejoin can't occur anyway (the join pass precedes
    /// the leave sweep), so the bar effectively covers the next `k` tasks.
    pub rejoin_lockout_tasks: u64,
    /// Fixed softmax temperature γ over the query's policy EFE (koalisi #61, EQ1).
    /// **Identity default `None`** — the engine default `16.0`, which is the value
    /// the registered arm-E1 / K4-v5 arm runs at.
    ///
    /// Only the MeanField query path reads this: under `query_dynamics: true` the
    /// engine initializes γ from `1/β₀` and updates it live via
    /// [`aif::PrecisionDynamics`], so a fixed γ is ignored there.
    ///
    /// This is the EQ1 de-saturation lever — γ = 16 peaks the policy posterior hard
    /// enough that the join/leave scores saturate at ±0.5 (gotcha 23). The
    /// battery-v2 arm-config labels are `arm-E1g1` / `arm-E1g4` / `arm-E1g16`;
    /// arm-E1 itself stays `None`.
    pub query_gamma: Option<f64>,
}

impl Default for PersistentAifConfig {
    /// The registered `aif-pers` arm.
    fn default() -> Self {
        Self {
            trial_boundary: TrialBoundary::PerStream,
            persistent_learning: true,
            query_dynamics: true,
            query_novelty: true,
            initial_precision_b: 4.0,
            eviction_cap: None,
            rejoin_lockout_tasks: 0,
            query_gamma: None,
        }
    }
}

// --- State snapshot (design note §5 seam) ----------------------------------

/// Serialization seam for the persistent world model (design note §5). Plain
/// matrices/scalars so durable state can later land on the catgraph-surreal
/// reboot without redesign. `aif` ships no serde; this is a koalisi-side struct.
#[derive(Debug, Clone)]
pub struct PersistentAifState {
    /// Current smoothed per-factor beliefs (BMA X at the last window node).
    pub beliefs: Vec<DVector<f64>>,
    /// Dirichlet pA counts (one per modality), `None` when learning is off.
    pub pa: Option<Vec<DMatrix<f64>>>,
    /// Dirichlet pB counts (`[factor][control]`), `None` when learning is off.
    pub pb: Option<Vec<Vec<DMatrix<f64>>>>,
    /// Dirichlet pD counts (one per factor), `None` when learning is off.
    pub pd: Option<Vec<DVector<f64>>>,
    /// Precision hyperparameter β (always `None`: the persistent agent runs no
    /// precision dynamics — single control ⇒ provably inert).
    pub beta: Option<f64>,
    /// The configured trial boundary.
    pub trial_boundary: TrialBoundary,
}

// --- Arm -------------------------------------------------------------------

/// Mutable persistent state, guarded by the arm's mutex.
struct Inner {
    /// The persistent 8-factor / 8-modality world model.
    agent: aif::POMDPAgent,
    /// The last `REPLAY_CAP` task-outcome observation vectors (full 8-length,
    /// oldest first), replayed into each query (A1.4).
    replay: VecDeque<Vec<usize>>,
    /// Number of task outcomes observed so far this seed.
    tasks_observed: usize,
    /// Monotonic per-decision counter — derives the query RNG seed (hygiene; the
    /// arm never samples). Tracked internally because the decision trait does not
    /// surface task/decision indices.
    decision_counter: u64,
    /// The seed this arm was built for (the query seed base).
    battery_seed: u64,
    /// Leave-acts emitted so far in the current task; reset to 0 each
    /// `observe_outcome` (the task boundary). Gates the eviction cap (#56).
    evictions_this_task: usize,
    /// `agent_id` → `tasks_observed` at its last eviction (overwritten on
    /// re-eviction). Never reset within a seed. Gates the rejoin lockout (#56).
    evicted_at: HashMap<usize, usize>,
}

/// Persistent-agent multimodal Active Inference join/leave policy (`aif-pers`,
/// prereg K4-v4). One arm per seed; the harness rebuilds it at each seed boundary
/// and calls [`observe_outcome`](Self::observe_outcome) once per task.
///
/// The decision rule (Amendment A1.1) reads the deterministic marginal action
/// posterior of a fresh membership-factor query: join iff `p(control 1) > 0.5`;
/// leave iff `p(control 1) ≥ 0.5` (ties leave). Engine errors decline the join /
/// keep the member (score `0.0`), mirroring [`AifMmDecisionPolicy`].
pub struct PersistentAifArm {
    inner: Arc<Mutex<Inner>>,
    config: PersistentAifConfig,
}

impl PersistentAifArm {
    /// Build a fresh persistent arm for `battery_seed` under `config`.
    ///
    /// # Errors
    ///
    /// Returns [`aif::AifError`] if the persistent generative model or params are
    /// rejected by the engine.
    pub fn new(battery_seed: u64, config: PersistentAifConfig) -> Result<Self, aif::AifError> {
        let agent = build_persistent_agent(config, battery_seed)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(Inner {
                agent,
                replay: VecDeque::with_capacity(REPLAY_CAP),
                tasks_observed: 0,
                decision_counter: 0,
                battery_seed,
                evictions_this_task: 0,
                evicted_at: HashMap::new(),
            })),
            config,
        })
    }

    /// Observe one task outcome into the persistent world model (the harness calls
    /// this once per task, after the leave sweep — the `record_outcome` site).
    ///
    /// `required` is the task capability mask; `per_bit_success[b]` is `true` iff
    /// some final member providing bit `b` performed on this task (the harness
    /// computes it and passes it in, keeping the arm decoupled from `perf`). Bits
    /// `∉ required` observe `no_obs` (likelihood-neutral padding). Under
    /// [`TrialBoundary::PerTask`] the window is reset afterwards.
    ///
    /// The 8-length observation is also pushed into the replay deque (cap
    /// [`REPLAY_CAP`], oldest evicted) for the query replay (A1.4).
    pub fn observe_outcome(&self, required: u32, per_bit_success: &[bool; N_BITS]) {
        let obs: Vec<usize> = (0..N_BITS)
            .map(|b| {
                if required & (1u32 << b) == 0 {
                    NO_OBS
                } else if per_bit_success[b] {
                    SUCCESS
                } else {
                    FAILURE
                }
            })
            .collect();

        let mut inner = self.inner.lock().expect("persistent arm mutex poisoned");
        // Record-then-observe (brief-pinned; numerically identical to the canonical
        // observe-then-record here because the single-control agent's every action
        // is 0, so `mmp_act_hist` is all-zero regardless of alignment).
        inner.agent.record_action(0);
        if let Err(e) = inner.agent.action_probabilities_multi(&obs) {
            // Structurally impossible (obs has n_modalities entries), but never
            // panic inside a battery: log and skip the update.
            tracing::warn!(error = %e, "persistent observe_outcome failed");
        }
        if self.config.trial_boundary == TrialBoundary::PerTask {
            inner.agent.reset_window();
        }
        if inner.replay.len() == REPLAY_CAP {
            inner.replay.pop_front();
        }
        inner.replay.push_back(obs);
        inner.tasks_observed += 1;
        // Task boundary: reset the per-task eviction counter (#56). `evicted_at`
        // persists across tasks (it drives the cross-task rejoin lockout).
        inner.evictions_this_task = 0;
    }

    /// Snapshot the persistent world model (design note §5 serialization seam).
    #[must_use]
    pub fn state_snapshot(&self) -> PersistentAifState {
        let inner = self.inner.lock().expect("persistent arm mutex poisoned");
        PersistentAifState {
            beliefs: current_beliefs(&inner.agent),
            pa: inner.agent.pa().map(<[_]>::to_vec),
            pb: inner.agent.pb().map(<[_]>::to_vec),
            pd: inner.agent.pd().map(<[_]>::to_vec),
            beta: inner.agent.beta(),
            trial_boundary: self.config.trial_boundary,
        }
    }

    /// Shared decision core: build the fresh membership-factor query for
    /// `(required, cfg0, cfg1)`, replay the recent outcome window, and read the
    /// marginal action posterior. `leave` selects the tie rule (`≥` vs `>`).
    fn decide(&self, required: u32, cfg0: u32, cfg1: u32, leave: bool) -> Decision {
        // required == 0 ⇒ nothing to cover ⇒ no-op (mirrors the mm/scalar arms).
        if required == 0 {
            return Decision { act: false, score: 0.0 };
        }

        let mut inner = self.inner.lock().expect("persistent arm mutex poisoned");
        inner.decision_counter = inner.decision_counter.wrapping_add(1);
        let seed = inner.battery_seed ^ splitmix64(inner.decision_counter);

        let (mut query, replay_seqs) =
            match build_query(&inner, &self.config, seed, required, cfg0, cfg1) {
                Ok(q) => q,
                Err(e) => {
                    tracing::warn!(error = %e, "persistent query construction failed");
                    return Decision { act: false, score: 0.0 };
                }
            };

        let r = required.count_ones() as usize;
        let dist = match run_replay(&mut query, &replay_seqs, r) {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!(error = %e, "persistent query replay failed");
                return Decision { act: false, score: 0.0 };
            }
        };

        // p(control 1) — switch to config1 (join / leave).
        let p1 = dist.get(1).copied().unwrap_or(f64::NAN);
        if !p1.is_finite() {
            return Decision { act: false, score: 0.0 };
        }
        let act = if leave { p1 >= 0.5 } else { p1 > 0.5 };
        Decision { act, score: p1 - 0.5 }
    }
}

impl CoalitionDecisionPolicy for PersistentAifArm {
    /// Join iff `p(control 1) > 0.5` — config0 = alone (the agent's own caps),
    /// config1 = coalition ∪ {agent}. `coalition` excludes `agent`.
    fn should_join(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        ctx: &DecisionContext,
    ) -> Decision {
        let alone = agent.capabilities();
        // Rejoin lockout (#56, exploratory): bar an agent evicted during task `t`
        // from rejoining while `t < tasks_observed <= t + k` — skip the query, no
        // decision-counter increment. Off (identity) when `rejoin_lockout_tasks == 0`.
        if self.config.rejoin_lockout_tasks > 0 {
            let inner = self.inner.lock().expect("persistent arm mutex poisoned");
            if let Some(&t) = inner.evicted_at.get(&agent.agent_id()) {
                let now = inner.tasks_observed;
                if now > t && (now as u64) <= t as u64 + self.config.rejoin_lockout_tasks {
                    return Decision { act: false, score: 0.0 };
                }
            }
        }
        let cfg1 = union_caps(coalition) | alone;
        self.decide(ctx.required_capabilities, alone, cfg1, false)
    }

    /// Leave iff `p(control 1) ≥ 0.5` (ties leave) — config0 = coalition with the
    /// member, config1 = coalition without it. `coalition` includes `agent`.
    fn should_leave(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        ctx: &DecisionContext,
    ) -> Decision {
        let cfg0 = union_caps(coalition);
        let agent_id = agent.agent_id();
        let cfg1 = coalition
            .iter()
            .filter(|a| a.agent_id() != agent_id)
            .fold(0u32, |acc, a| acc | a.capabilities());

        // Eviction cap (#56): once the per-task cap is exhausted (incl. `c = 0` from
        // the first consult), decline the leave WITHOUT building a query — no engine
        // call, no `decision_counter` increment. Skipped entirely (identity) when
        // `eviction_cap` is `None`.
        if let Some(cap) = self.config.eviction_cap {
            let inner = self.inner.lock().expect("persistent arm mutex poisoned");
            if inner.evictions_this_task >= cap as usize {
                return Decision { act: false, score: 0.0 };
            }
        }

        let d = self.decide(ctx.required_capabilities, cfg0, cfg1, true);

        // Count an actual eviction and record its task for the rejoin lockout. This
        // assumes callers honor `act` (true for the battery sweep and the
        // `CoalitionService` seam). Both counters are inert at the identity default
        // (`eviction_cap: None`, `rejoin_lockout_tasks: 0`) — nothing reads them.
        if d.act {
            let mut inner = self.inner.lock().expect("persistent arm mutex poisoned");
            inner.evictions_this_task += 1;
            let t = inner.tasks_observed;
            inner.evicted_at.insert(agent_id, t);
        }
        d
    }
}

// --- Persistent-agent construction -----------------------------------------

/// Build the persistent 8-factor / 8-modality world model (design note §3.1,
/// Amendment A1.2).
fn build_persistent_agent(
    config: PersistentAifConfig,
    seed: u64,
) -> Result<aif::POMDPAgent, aif::AifError> {
    // pA counts (A1.2 anchors): one modality per bit; A_m depends only on factor m,
    // replicated across the 256-column joint (little-endian, factor 0 fastest ⇒
    // state = (j >> m) & 1). Injected as `initial_pa` so the asymmetric anchors
    // survive `learn_a` (aif 0.11.0 count injection — Amendment 2): under the old
    // `initial_precision` scale path the first write-back `A = normalize(pA)`
    // flattened them, defeating A1.2's "a symmetric A never learns to discriminate".
    let pa_counts: Vec<DMatrix<f64>> = (0..N_BITS)
        .map(|m| {
            DMatrix::from_fn(N_OUTCOMES, N_JOINT_PERSISTENT, |row, j| {
                if (j >> m) & 1 == 0 {
                    RELIABLE_COL[row]
                } else {
                    UNRELIABLE_COL[row]
                }
            })
        })
        .collect();
    // A = column-normalize(counts) — the anchor columns already sum to 1, so this is
    // the anchors themselves. Ignored under injection (shape-only), passed for a valid
    // model and for the learning-off (E5) path.
    let a: Vec<DMatrix<f64>> = pa_counts.clone();

    // B: per-factor single-control sticky [[0.9,0.1],[0.1,0.9]]. The scale path
    // (`initial_precision_b`) seeds pB = s_b·B, which is structure-preserving, so B
    // does NOT need count injection.
    let sticky = DMatrix::from_row_slice(2, 2, &[STICKY, 1.0 - STICKY, 1.0 - STICKY, STICKY]);
    let b: Vec<Vec<DMatrix<f64>>> = (0..N_BITS).map(|_| vec![sticky.clone()]).collect();

    // C: neutral (uniform) — the persistent agent never drives a decision, so C is
    // inert for its belief/learning path.
    let c: Vec<Vec<f64>> = (0..N_BITS).map(|_| vec![0.5, 0.5, 0.5]).collect();

    // D: uniform per factor.
    let d: Vec<Vec<f64>> = (0..N_BITS).map(|_| vec![0.5, 0.5]).collect();

    let learn = config.persistent_learning;
    let params = aif::AgentParams {
        alpha: 1.0, // engine default; never sampled.
        policy_depth: 1,
        learn_a: learn,
        learn_b: learn,
        learn_d: learn,
        use_param_info_gain: learn,
        use_b_info_gain: learn,
        initial_pa: learn.then(|| pa_counts.clone()),
        initial_precision_b: learn.then_some(config.initial_precision_b),
        initial_precision_d: learn.then_some(PERSISTENT_INITIAL_PRECISION_D),
        state_inference: aif::StateInference::MarginalMessagePassing {
            horizon: PERSISTENT_HORIZON,
            iters: PERSISTENT_ITERS,
        },
        precision_dynamics: None, // single control ⇒ inert (design note §3.1).
        seed: Some(seed),
        ..Default::default()
    };

    aif::POMDPAgent::from_model(aif::GenerativeModel { a, b, c, d }, params)
}

// --- Query construction (Amendment A1.1/A1.2/A1.3/A1.4) --------------------

/// Build the fresh membership-factor query POMDP and the replay observation
/// sequences (r-length, restricted to `required`). Returns the query agent ready
/// for replay + posterior read.
fn build_query(
    inner: &Inner,
    config: &PersistentAifConfig,
    seed: u64,
    required: u32,
    cfg0: u32,
    cfg1: u32,
) -> Result<(aif::POMDPAgent, Vec<Vec<usize>>), aif::AifError> {
    let required_bits = set_bits(required);
    let r = required_bits.len();
    let n_joint = 1usize << (r + 1); // 2^r bit-factor states × 2 membership states.

    let obs_model = inner.agent.observation_model();
    let trans_model = inner.agent.transition_model();
    let pa_persist = inner.agent.pa();
    let pb_persist = inner.agent.pb();
    let beliefs = current_beliefs(&inner.agent);

    // pA counts (Amendment A2.2): A_m conditions on (bit-m factor, membership factor).
    // Covered under the membership config ⇒ the persistent modality's Dirichlet counts
    // for that bit state (the exact learned reliability model); uncovered ⇒ the flat
    // block at unit concentration `[1, 1, 1]` (uninformative). `A ≡ column-normalize(pA)`.
    let pa_counts: Vec<DMatrix<f64>> = (0..r)
        .map(|m| {
            let bit = required_bits[m];
            DMatrix::from_fn(N_OUTCOMES, n_joint, |row, j| {
                let s_bit = (j >> m) & 1;
                let s_mem = (j >> r) & 1;
                let cover_mask = if s_mem == 0 { cfg0 } else { cfg1 };
                if cover_mask & (1u32 << bit) != 0 {
                    persistent_bit_count(pa_persist, obs_model, bit, s_bit, row)
                } else {
                    1.0
                }
            })
        })
        .collect();
    let a: Vec<DMatrix<f64>> = pa_counts.iter().map(column_normalize).collect();

    // pB counts (Amendment A2.2): r bit-drift factors carry the persistent per-bit pB
    // verbatim (learned stochastic drift ⇒ state info-gain live) + a membership factor
    // LAST with unit-scaled deterministic counts (control u ⇒ all mass to state u;
    // structural zeros ⇒ B-novelty exactly 0 under the 0.10.0 mask). `B ≡ normalize(pB)`.
    let mut pb_counts: Vec<Vec<DMatrix<f64>>> = required_bits
        .iter()
        .map(|&bit| match pb_persist {
            Some(pb) => pb[bit].clone(),
            None => vec![trans_model[bit][0].clone()], // E5 fallback: unlearned B as counts.
        })
        .collect();
    let membership_pb: Vec<DMatrix<f64>> = (0..2)
        .map(|u| DMatrix::from_fn(2, 2, |row, _col| if row == u { 1.0 } else { 0.0 }))
        .collect();
    pb_counts.push(membership_pb);
    let b: Vec<Vec<DMatrix<f64>>> = pb_counts
        .iter()
        .map(|bf| bf.iter().map(column_normalize).collect())
        .collect();

    // C: normalize([0.9, 0.1, 0.3]) per modality (Amendment A1.1).
    let c_query = normalize(&C_RAW);
    let c: Vec<Vec<f64>> = vec![c_query; r];

    // D: bit factors = persistent smoothed beliefs; membership = δ(config0).
    let mut d: Vec<Vec<f64>> = required_bits
        .iter()
        .map(|&bit| {
            let belief = &beliefs[bit];
            let s = (belief[0] + belief[1]).max(1e-12);
            vec![belief[0] / s, belief[1] / s]
        })
        .collect();
    d.push(vec![1.0, 0.0]);

    // Effective query learning/novelty flags (E5 ties query learning to persistent
    // learning; novelty requires learning).
    let query_learn = config.persistent_learning;
    let query_novelty = query_learn && config.query_novelty;

    // Exact count injection (Amendment A2.2) — no more initial_precision approximations.
    let (initial_pa, initial_pb) = if query_learn {
        (Some(pa_counts), Some(pb_counts))
    } else {
        (None, None)
    };

    let (state_inference, precision_dynamics) = if config.query_dynamics {
        (
            aif::StateInference::MarginalMessagePassing {
                horizon: QUERY_HORIZON,
                iters: QUERY_ITERS,
            },
            Some(aif::PrecisionDynamics::default()),
        )
    } else {
        (aif::StateInference::MeanField, None)
    };

    let mut params = aif::AgentParams {
        alpha: QUERY_ALPHA,
        policy_depth: QUERY_POLICY_DEPTH,
        learn_a: query_learn,
        learn_b: query_learn,
        learn_d: false, // query learn_d is off (not registered).
        use_param_info_gain: query_novelty,
        use_b_info_gain: query_novelty,
        initial_pa,
        initial_pb,
        state_inference,
        precision_dynamics,
        seed: Some(seed),
        ..Default::default()
    };
    // EQ1 (#61): overriding γ is opt-in, so `None` leaves the engine default in
    // place rather than re-stating it — identity by construction.
    if let Some(g) = config.query_gamma {
        params.gamma = g;
    }

    let query = aif::POMDPAgent::from_model(aif::GenerativeModel { a, b, c, d }, params)?;

    // Replay sequences (A1.4): each stored 8-length outcome restricted to this
    // task's required bits (no-obs padding is already encoded in the stored vector
    // for bits that were not required when it was observed).
    let replay_seqs: Vec<Vec<usize>> = inner
        .replay
        .iter()
        .map(|full| required_bits.iter().map(|&b| full[b]).collect())
        .collect();

    Ok((query, replay_seqs))
}

/// One entry of a covered query pA column (Amendment A2.2): the persistent
/// modality's Dirichlet count for bit `bit` in state `s_bit`, outcome row `row`.
///
/// Reads the persistent pA counts (`pa()`) at the representative column
/// `s_bit << bit` (factor `bit` in state `s_bit`, all other factors 0 — the
/// persistent A_bit is factor-`bit`-only by construction). When `pa()` is absent
/// (E5, persistent learning off) it falls back to the persistent observation model
/// (the unlearned anchor), which is the natural "binary-coverage" reliability model.
fn persistent_bit_count(
    pa: Option<&[DMatrix<f64>]>,
    obs_model: &[DMatrix<f64>],
    bit: usize,
    s_bit: usize,
    row: usize,
) -> f64 {
    let col = s_bit << bit;
    match pa {
        Some(pa) => pa[bit][(row, col)],
        None => obs_model[bit][(row, col)],
    }
}

/// Column-normalize a Dirichlet count matrix into a column-stochastic matrix
/// (`out[(r,j)] = m[(r,j)] / Σ_r m[(r,j)]`), leaving sub-floor columns untouched —
/// mirrors the engine's construction-time `A ← normalize(pA)` sync so the supplied
/// `GenerativeModel` `a`/`b` (shape-checked, else used verbatim under E5) agree with
/// the injected counts.
fn column_normalize(m: &DMatrix<f64>) -> DMatrix<f64> {
    let mut out = m.clone();
    for j in 0..m.ncols() {
        let s: f64 = m.column(j).iter().sum();
        if s > 1e-10 {
            for r in 0..m.nrows() {
                out[(r, j)] = m[(r, j)] / s;
            }
        }
    }
    out
}

/// Replay the recent outcome window into `query` and return the marginal action
/// posterior. With no replay history the neutral all-no-obs read is used (A1.4,
/// likelihood-neutral by construction).
fn run_replay(
    query: &mut aif::POMDPAgent,
    seqs: &[Vec<usize>],
    r: usize,
) -> Result<DVector<f64>, aif::AifError> {
    let mut last: Option<DVector<f64>> = None;
    for obs in seqs {
        query.record_action(0);
        last = Some(query.action_probabilities_multi(obs)?);
    }
    match last {
        Some(d) => Ok(d),
        None => {
            query.record_action(0);
            query.action_probabilities_multi(&vec![NO_OBS; r])
        }
    }
}

// --- Small helpers ---------------------------------------------------------

/// Current smoothed per-factor beliefs: the BMA X at the last MMP window node,
/// falling back to `state_beliefs()` when the window is empty (first decision of a
/// seed, or after a PerTask reset) or under MeanField.
fn current_beliefs(agent: &aif::POMDPAgent) -> Vec<DVector<f64>> {
    let mut tau = 1;
    let mut last = None;
    while let Some(b) = agent.bma_state_belief(tau) {
        last = Some(b);
        tau += 1;
    }
    last.unwrap_or_else(|| agent.state_beliefs().to_vec())
}

/// Union of the capability masks of `agents`.
fn union_caps(agents: &[&dyn AgentCapabilities]) -> u32 {
    agents.iter().fold(0u32, |acc, a| acc | a.capabilities())
}

/// Ascending indices of the set bits of `mask` (within the 8-bit universe).
fn set_bits(mask: u32) -> Vec<usize> {
    (0..N_BITS).filter(|&b| mask & (1u32 << b) != 0).collect()
}

/// L1-normalize a slice into a probability vector (defensive against a zero sum).
fn normalize(v: &[f64]) -> Vec<f64> {
    let s = v.iter().sum::<f64>();
    let s = if s > 0.0 { s } else { 1.0 };
    v.iter().map(|&x| x / s).collect()
}

/// SplitMix64 finalizer — derives a deterministic per-decision query seed
/// (hygiene only; the arm never samples).
fn splitmix64(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy)]
    struct TestAgent {
        id: usize,
        caps: u32,
        trust: u32,
    }

    impl AgentCapabilities for TestAgent {
        fn agent_id(&self) -> usize {
            self.id
        }
        fn capabilities(&self) -> u32 {
            self.caps
        }
        fn trust_level(&self) -> u32 {
            self.trust
        }
    }

    /// A well-formed persistent agent must build under the registered config.
    #[test]
    fn persistent_agent_builds() {
        let arm = PersistentAifArm::new(0, PersistentAifConfig::default()).unwrap();
        let snap = arm.state_snapshot();
        assert_eq!(snap.beliefs.len(), N_BITS);
        // Uniform D before any observation.
        for b in &snap.beliefs {
            assert!((b[0] - 0.5).abs() < 1e-12 && (b[1] - 0.5).abs() < 1e-12);
        }
        assert!(snap.pa.is_some(), "learning on ⇒ pa present");
        assert!(snap.beta.is_none(), "no precision dynamics on the persistent agent");
    }

    /// Query-construction shape (r = 2): joint sizes, A dims 3×8, membership B
    /// deterministic (Amendment A1.1).
    #[test]
    fn query_shape_r2() {
        let arm = PersistentAifArm::new(7, PersistentAifConfig::default()).unwrap();
        let inner = arm.inner.lock().unwrap();
        let required = 0b0000_0011u32; // bits 0 and 1
        let (query, replay) = build_query(&inner, &arm.config, 1234, required, 0b01, 0b11).unwrap();

        // r = 2 bit factors + 1 membership factor; n_actions = 1·1·2 = 2.
        assert_eq!(query.n_factors(), 3);
        assert_eq!(query.n_modalities(), 2);
        assert_eq!(query.n_actions(), 2);

        // n_joint = 2^2 · 2 = 8; each A is 3 × 8.
        for am in query.observation_model() {
            assert_eq!(am.shape(), (N_OUTCOMES, 8));
        }

        // Membership factor is last, with two deterministic controls.
        let b = query.transition_model();
        assert_eq!(b[2].len(), 2, "membership factor has two controls");
        // control 0 ⇒ all mass to state 0; control 1 ⇒ all mass to state 1.
        assert_eq!(b[2][0][(0, 0)], 1.0);
        assert_eq!(b[2][0][(0, 1)], 1.0);
        assert_eq!(b[2][1][(1, 0)], 1.0);
        assert_eq!(b[2][1][(1, 1)], 1.0);

        // No replay observed yet ⇒ empty replay sequences.
        assert!(replay.is_empty());
    }

    /// Determinism: the same seed + the same outcome sequence ⇒ identical acts.
    #[test]
    fn deterministic_acts() {
        let run = || {
            let arm = PersistentAifArm::new(42, PersistentAifConfig::default()).unwrap();
            let ctx = DecisionContext { required_capabilities: 0b0111 };
            let a0 = TestAgent { id: 0, caps: 0b0001, trust: 50 };
            let a1 = TestAgent { id: 1, caps: 0b0010, trust: 50 };
            let a2 = TestAgent { id: 2, caps: 0b0100, trust: 50 };
            let coalition: [&dyn AgentCapabilities; 2] = [&a1, &a2];

            let mut acts = Vec::new();
            for task in 0..3 {
                acts.push(arm.should_join(&a0, &coalition, &ctx).act);
                let full: [&dyn AgentCapabilities; 3] = [&a0, &a1, &a2];
                acts.push(arm.should_leave(&a0, &full, &ctx).act);
                // Observe a fixed outcome (bit 0 fails, bits 1/2 succeed).
                let succ = [false, true, true, false, false, false, false, false];
                arm.observe_outcome(0b0111, &succ);
                let _ = task;
            }
            acts
        };
        assert_eq!(run(), run(), "same seed + outcomes ⇒ identical acts");
    }

    /// PerTask vs PerStream produce an observable window difference: PerStream
    /// retains a smoothed window node after a task, PerTask resets it.
    #[test]
    fn per_task_vs_per_stream_window() {
        let succ = [true, false, false, false, false, false, false, false];

        let stream = PersistentAifArm::new(1, PersistentAifConfig::default()).unwrap();
        stream.observe_outcome(0b0001, &succ);
        {
            let inner = stream.inner.lock().unwrap();
            assert!(
                inner.agent.bma_state_belief(1).is_some(),
                "PerStream keeps the window node"
            );
        }

        let per_task = PersistentAifArm::new(
            1,
            PersistentAifConfig { trial_boundary: TrialBoundary::PerTask, ..Default::default() },
        )
        .unwrap();
        per_task.observe_outcome(0b0001, &succ);
        {
            let inner = per_task.inner.lock().unwrap();
            assert!(
                inner.agent.bma_state_belief(1).is_none(),
                "PerTask resets the window"
            );
        }
    }

    /// The outcome hook is real learning: it changes the persistent pA counts.
    #[test]
    fn outcome_updates_pa() {
        let arm = PersistentAifArm::new(3, PersistentAifConfig::default()).unwrap();
        let before = arm.state_snapshot().pa.unwrap();
        // Bit 0 required, observed success.
        let succ = [true, false, false, false, false, false, false, false];
        arm.observe_outcome(0b0001, &succ);
        let after = arm.state_snapshot().pa.unwrap();
        let changed = before
            .iter()
            .zip(&after)
            .any(|(x, y)| (x - y).iter().any(|d| d.abs() > 1e-9));
        assert!(changed, "observing an outcome must update pA (learning is real)");
    }

    /// Neutral-read (A1.2 likelihood-neutrality): an all-no-obs observation leaves
    /// the persistent beliefs unchanged (the no-obs row is uniform across states).
    #[test]
    fn neutral_read_leaves_beliefs_unchanged() {
        let arm = PersistentAifArm::new(9, PersistentAifConfig::default()).unwrap();
        {
            let mut inner = arm.inner.lock().unwrap();
            inner.agent.record_action(0);
            inner
                .agent
                .action_probabilities_multi(&[NO_OBS; N_BITS])
                .unwrap();
        }
        for b in arm.state_snapshot().beliefs {
            assert!(
                (b[0] - 0.5).abs() < 1e-9 && (b[1] - 0.5).abs() < 1e-9,
                "all-no-obs must leave beliefs at the uniform prior, got {b:?}"
            );
        }
    }

    /// The **registered** config discriminates coverage (aif 0.11.0 count injection,
    /// Amendment 2). A join lifting coverage 1/3 → 3/3 fires and scores strictly
    /// higher than a redundant clone (same coverage both sides) — the gain-vs-clone
    /// posteriors genuinely differ, which the aif ≤ 0.10.1 write-back made impossible.
    /// A learning-OFF (E5) arm is checked alongside as a construction sanity: it too
    /// must favour real coverage over a clone.
    #[test]
    fn registered_config_discriminates_coverage() {
        let ctx = DecisionContext { required_capabilities: 0b0111 };
        let a0 = TestAgent { id: 0, caps: 0b0001, trust: 50 };
        let a1 = TestAgent { id: 1, caps: 0b0010, trust: 50 };
        let a2 = TestAgent { id: 2, caps: 0b0100, trust: 50 };
        let coalition: [&dyn AgentCapabilities; 2] = [&a1, &a2]; // join → 3/3
        let clone_agent = TestAgent { id: 3, caps: 0b0001, trust: 50 };
        let redundant: [&dyn AgentCapabilities; 1] = [&clone_agent]; // join → still 1/3

        // Registered config (learn_a + novelty + dynamics ON): counts are injected,
        // so coverage reaches the decision.
        let arm = PersistentAifArm::new(11, PersistentAifConfig::default()).unwrap();
        let gain = arm.should_join(&a0, &coalition, &ctx);
        let clone = arm.should_join(&a0, &redundant, &ctx);
        assert!(gain.act, "coverage-improving join must fire (score={})", gain.score);
        assert!(
            gain.score > clone.score + 1e-6,
            "registered config must favour coverage gain over a redundant clone: {} vs {}",
            gain.score,
            clone.score
        );

        // Learning-OFF (E5) construction sanity: still coverage-discriminating.
        let no_learn = PersistentAifConfig {
            persistent_learning: false,
            query_dynamics: false,
            ..Default::default()
        };
        let arm_e5 = PersistentAifArm::new(11, no_learn).unwrap();
        let gain_e5 = arm_e5.should_join(&a0, &coalition, &ctx);
        let clone_e5 = arm_e5.should_join(&a0, &redundant, &ctx);
        assert!(
            gain_e5.score > clone_e5.score + 0.1,
            "E5 construction must clearly favour coverage gain: {} vs {}",
            gain_e5.score,
            clone_e5.score
        );
    }

    /// required == 0 is a no-op (mirrors the mm/scalar arms).
    #[test]
    fn required_zero_is_noop() {
        let arm = PersistentAifArm::new(5, PersistentAifConfig::default()).unwrap();
        let ctx = DecisionContext { required_capabilities: 0 };
        let a0 = TestAgent { id: 0, caps: 0b0001, trust: 50 };
        let a1 = TestAgent { id: 1, caps: 0b0010, trust: 50 };
        let coalition: [&dyn AgentCapabilities; 1] = [&a1];
        let d = arm.should_join(&a0, &coalition, &ctx);
        assert!(!d.act && d.score == 0.0);
    }

    /// The exploratory toggles all build and decide without error.
    #[test]
    fn exploratory_toggles_run() {
        let ctx = DecisionContext { required_capabilities: 0b0011 };
        let a0 = TestAgent { id: 0, caps: 0b0001, trust: 50 };
        let a1 = TestAgent { id: 1, caps: 0b0010, trust: 50 };
        let coalition: [&dyn AgentCapabilities; 1] = [&a1];
        let succ = [true, true, false, false, false, false, false, false];

        let configs = [
            PersistentAifConfig { trial_boundary: TrialBoundary::PerTask, ..Default::default() },
            PersistentAifConfig { persistent_learning: false, ..Default::default() },
            PersistentAifConfig { query_dynamics: false, ..Default::default() },
            PersistentAifConfig { query_novelty: false, ..Default::default() },
            PersistentAifConfig { initial_precision_b: 16.0, ..Default::default() },
        ];
        for cfg in configs {
            let arm = PersistentAifArm::new(2, cfg).unwrap();
            arm.observe_outcome(0b0011, &succ);
            let _ = arm.should_join(&a0, &coalition, &ctx);
            let full: [&dyn AgentCapabilities; 2] = [&a0, &a1];
            let _ = arm.should_leave(&a0, &full, &ctx);
        }
    }

    /// #56: `eviction_cap: Some(0)` never evicts — in a scenario where the default
    /// arm removes a redundant clone (coverage unchanged ⇒ `p(control 1) = 0.5`,
    /// leave tie removes), the capped arm declines with score `0.0` (no query built).
    #[test]
    fn eviction_cap_zero_never_evicts() {
        let ctx = DecisionContext { required_capabilities: 0b0011 };
        let a0 = TestAgent { id: 0, caps: 0b0001, trust: 50 };
        let a1 = TestAgent { id: 1, caps: 0b0010, trust: 50 };
        let clone = TestAgent { id: 2, caps: 0b0001, trust: 50 }; // redundant with a0
        let full: [&dyn AgentCapabilities; 3] = [&a0, &a1, &clone];

        let def = PersistentAifArm::new(0, PersistentAifConfig::default()).unwrap();
        assert!(
            def.should_leave(&clone, &full, &ctx).act,
            "control: the default arm must evict the redundant clone"
        );

        let ne = PersistentAifArm::new(
            0,
            PersistentAifConfig { eviction_cap: Some(0), ..Default::default() },
        )
        .unwrap();
        let d = ne.should_leave(&clone, &full, &ctx);
        assert!(!d.act && d.score == 0.0, "eviction_cap Some(0) must decline with score 0.0");
    }

    /// #56: the rejoin lockout bars an evicted agent from rejoining for the next `k`
    /// tasks, then consults it normally again. The barred agent is otherwise a
    /// coverage-improving (would-fire) join, so barred = declined, unbarred = fires.
    #[test]
    fn rejoin_lockout_bars_then_reallows() {
        let cfg = PersistentAifConfig { rejoin_lockout_tasks: 2, ..Default::default() };
        let arm = PersistentAifArm::new(0, cfg).unwrap();
        let ctx = DecisionContext { required_capabilities: 0b0011 };
        let a0 = TestAgent { id: 0, caps: 0b0001, trust: 50 };
        let a1 = TestAgent { id: 1, caps: 0b0010, trust: 50 };
        let x = TestAgent { id: 2, caps: 0b0001, trust: 50 };
        let full: [&dyn AgentCapabilities; 3] = [&a0, &a1, &x]; // x redundant here
        let solo: [&dyn AgentCapabilities; 1] = [&a1]; // x is coverage-improving vs a1
        let succ = [true, true, false, false, false, false, false, false];

        // Task 0 (tasks_observed == 0): evict the redundant x → recorded at t = 0.
        assert!(arm.should_leave(&x, &full, &ctx).act, "x evicted in task 0");
        arm.observe_outcome(0b0011, &succ); // tasks_observed → 1

        // Tasks 1 and 2 (0 < now <= 0 + 2): x's coverage-improving join is barred.
        let d1 = arm.should_join(&x, &solo, &ctx);
        assert!(!d1.act && d1.score == 0.0, "barred at task 1");
        arm.observe_outcome(0b0011, &succ); // → 2
        let d2 = arm.should_join(&x, &solo, &ctx);
        assert!(!d2.act && d2.score == 0.0, "barred at task 2");
        arm.observe_outcome(0b0011, &succ); // → 3

        // Task 3 (now = 3 > 0 + 2): consulted normally again. The query runs on the
        // asymmetric coverage (cfg0 = x alone covers 1/2, cfg1 = x ∪ a1 covers 2/2),
        // so `p(control 1) != 0.5` ⇒ a non-zero score — the observable signature that
        // the lockout no longer forces the score-0.0 decline (learning drift makes
        // the raw `act` value across tasks engine-dependent, so we gate on the query
        // having run, not on its sign).
        let d3 = arm.should_join(&x, &solo, &ctx);
        assert!(
            d3.score != 0.0,
            "lockout lifted: the join is consulted (query ran, non-zero score), got {}",
            d3.score
        );
    }

    /// #56 X-B: the new levers at their defaults are identity — a config with
    /// explicit `eviction_cap: None, rejoin_lockout_tasks: 0` decides identically to
    /// `..Default::default()` over a fixed join/leave/observe stream.
    #[test]
    fn defaults_are_identity() {
        let ctx = DecisionContext { required_capabilities: 0b0111 };
        let a0 = TestAgent { id: 0, caps: 0b0001, trust: 50 };
        let a1 = TestAgent { id: 1, caps: 0b0010, trust: 50 };
        let a2 = TestAgent { id: 2, caps: 0b0100, trust: 50 };
        let coalition: [&dyn AgentCapabilities; 2] = [&a1, &a2];
        let full: [&dyn AgentCapabilities; 3] = [&a0, &a1, &a2];

        let run = |cfg| {
            let arm = PersistentAifArm::new(42, cfg).unwrap();
            let mut out = Vec::new();
            for _ in 0..3 {
                let j = arm.should_join(&a0, &coalition, &ctx);
                out.push((j.act, j.score.to_bits()));
                let l = arm.should_leave(&a0, &full, &ctx);
                out.push((l.act, l.score.to_bits()));
                let succ = [false, true, true, false, false, false, false, false];
                arm.observe_outcome(0b0111, &succ);
            }
            out
        };

        let explicit = PersistentAifConfig {
            eviction_cap: None,
            rejoin_lockout_tasks: 0,
            ..Default::default()
        };
        assert_eq!(
            run(explicit),
            run(PersistentAifConfig::default()),
            "explicit None/0 must decide identically to the defaults"
        );
    }

    /// A fixed join/leave/observe stream under `cfg`, as `(act, score bits)` pairs.
    /// The base config is arm-E1 (K4-v5): MeanField queries, where γ is live.
    fn e1_stream(query_gamma: Option<f64>) -> Vec<(bool, u64)> {
        let ctx = DecisionContext { required_capabilities: 0b0111 };
        let a0 = TestAgent { id: 0, caps: 0b0001, trust: 50 };
        let a1 = TestAgent { id: 1, caps: 0b0010, trust: 50 };
        let a2 = TestAgent { id: 2, caps: 0b0100, trust: 50 };
        let coalition: [&dyn AgentCapabilities; 2] = [&a1, &a2];
        let full: [&dyn AgentCapabilities; 3] = [&a0, &a1, &a2];

        let cfg = PersistentAifConfig {
            query_dynamics: false,
            query_gamma,
            ..Default::default()
        };
        let arm = PersistentAifArm::new(42, cfg).unwrap();
        let mut out = Vec::new();
        for _ in 0..3 {
            let j = arm.should_join(&a0, &coalition, &ctx);
            out.push((j.act, j.score.to_bits()));
            let l = arm.should_leave(&a0, &full, &ctx);
            out.push((l.act, l.score.to_bits()));
            let succ = [false, true, true, false, false, false, false, false];
            arm.observe_outcome(0b0111, &succ);
        }
        out
    }

    /// #61 EQ1: `query_gamma: Some(16.0)` restates the engine default, so it must
    /// decide bit-identically to the identity default `None` (arm-E1g16 == arm-E1).
    #[test]
    fn query_gamma_sixteen_is_identity() {
        assert_eq!(
            e1_stream(Some(16.0)),
            e1_stream(None),
            "Some(16.0) must reproduce the engine default exactly"
        );
    }

    /// #61 EQ1: the lever is live on the MeanField path — γ = 1 flattens the policy
    /// softmax, so at least one decision differs from the saturated γ = 16 default.
    #[test]
    fn query_gamma_one_changes_decisions() {
        let flat = e1_stream(Some(1.0));
        let base = e1_stream(None);
        assert_ne!(flat, base, "γ = 1 must de-saturate at least one decision");
    }

    /// Object-safety: the arm is usable behind the decision trait object.
    #[test]
    fn object_safe() {
        let arm = PersistentAifArm::new(0, PersistentAifConfig::default()).unwrap();
        let _: Box<dyn CoalitionDecisionPolicy> = Box::new(arm);
    }
}


