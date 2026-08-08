//! Role-slotted Active Inference **group** coalition-decision policy (features
//! `decision` + `process`) — the `grp-*` arm of pre-registration K4-EQ5b
//! (`docs/prereg-K4-eq5b-typed-two-engine.md`, **including Amendment 1**, which is
//! the spec of record wherever it conflicts with §3/§4).
//!
//! Where [`PersistentAifArm`] decides as *one* agent reading a *flat* capability
//! mask, this arm decides as a **group of role specialists** reading a
//! **workflow's** `(bit, role)` [`Demand`]: an [`aif::GroupAgent`] with a
//! [`CopyAgent`](aif::CopyAgent) sensory slot, one internal agent per demanding
//! role, and a [`VotingAgent`](aif::VotingAgent) active slot in
//! [`CertaintyWeighted`](aif::VotingMode::CertaintyWeighted) mode over
//! `n_actions = 2` (`0` = decline, `1` = act).
//!
//! # The five things worth knowing before using it
//!
//! **1. The internals are arm-E1 queries, built by arm-E1's own constructor.**
//! SP1 pins internal `r` as "arm-E1's existing query construction with
//! `required_r` substituted for `required`", where
//! `required_r = { b : (b, r) ∈ distinct demand }`. That is literally what happens
//! — [`PersistentAifArm::role_query`] is the same `build_query` the single-agent
//! arm calls, coverage-masked `initial_pa` injection and all. Nothing is
//! re-derived here, so the claim "this is arm-E1's mechanism in a group" is true
//! rather than approximated.
//!
//! **2. The specialisation lives in the world models, not in member continuity**
//! (Amendment A1.1). An arm-E1 query bakes the *this-candidate* coverage masks
//! into its matrices and the engine exposes no way to re-parameterize or replace a
//! built member, so members are **single-use on every leg**. What separates the
//! confirmatory cells from the reference cells is instead
//! [`WorldModelTopology`]: `R` role-specialised persistent world models (each
//! learning only from its own role's bits) versus **one shared** model read
//! through three role-restricted views. "A group of role specialists" versus
//! "three views of one model, aggregated".
//!
//! **3. There is no commit.** D4a is **struck** (Amendment A1.1): with members
//! discarded after each decision,
//! [`record_group_action`](aif::GroupAgent::record_group_action) would advance
//! nothing, so this arm never calls it and never calls
//! `group_distribution_recording` either. The MMP same-timestep double-update
//! hazard of koalisi **gotcha 32** is thereby *designed out* rather than gated —
//! a single-use member cannot be read twice. That hazard still applies to any
//! future arm that gives its members continuity.
//!
//! **4. A role with no demand does not vote** (Amendment A1.3). It leaves the
//! roster for that task, so the realised `R` varies 3 → 2 → 1. The alternative —
//! an abstaining member at `[0.5, 0.5]` — is not neutral under
//! `CertaintyWeighted`: it still carries weight `exp(−ln 2) = 0.5` and would drag
//! `p(act)` toward the decision threshold on every affected slot. Realised roster
//! sizes and the empty-slot rate are a **mandatory disclosure**
//! ([`GroupAifCounters`]).
//!
//! **5. Upstream errors are outcomes, not panics.** An [`aif::AifError`] anywhere
//! on the decision path is a decline **and a count**
//! ([`GroupAifCounters::declines_upstream`]) — never a panic, never a silent
//! fallback to some other arm's answer. Per the standing disclosure, a zero score
//! is **not** a decline: read the counters, never the score.
//!
//! # Coverage is role-matched (Amendment A2.1)
//!
//! SP1 substitutes `required_r` and was silent on the coverage masks; A2.1
//! registers them. A step `(b, r)` is covered iff some member **of role `r`** holds
//! bit `b` — the world's own `p9_step_covered` — so internal `r` is built with
//! `cfg0`/`cfg1` restricted to role-`r` participants. A role-blind union would let
//! a role-2 worker "cover" a role-0 step, scoring the query against a different
//! notion of coverage than the world it is evaluated in, and would leave the three
//! internals differing only in `required_r` — specialists in name only.
//!
//! [`CoverageMasks::RoleBlind`] exists as exactly that contrast, and is the
//! non-gating `grp-role-blind` reference leg: it isolates how much of a margin
//! comes from role-matched coverage versus role-restricted demand alone.
//!
//! # Determinism
//!
//! [`group_distribution`](aif::GroupAgent::group_distribution) is the **only**
//! RNG-free path into a group's action distribution; every other entry point
//! samples. This arm calls that one and no other, its members are
//! [`POMDPAgent`](aif::POMDPAgent)s behind a deterministic wrapper, and its
//! sensory slot is a `CopyAgent` — the three conditions gotcha 32 pins RNG-freedom
//! on. Seeds are threaded for hygiene only; nothing draws from them.

use std::collections::HashMap;
use std::sync::Mutex;

use nalgebra::DVector;

use crate::algorithms::AgentCapabilities;
use crate::process::{Demand, Role};

use super::aif_persistent_policy::{
    PersistentAifArm, PersistentAifConfig, PersistentAifState, low_mask, run_replay, set_bits,
    splitmix64,
};
use super::{CoalitionDecisionPolicy, Decision, DecisionContext};

// --- Pinned constants ------------------------------------------------------

/// The group's action space (SP3): `0` = decline, `1` = act.
pub const GROUP_N_ACTIONS: usize = 2;

/// Index of the "act" action within [`GROUP_N_ACTIONS`].
const ACTION_ACT: usize = 1;

/// The registered role count `R` (D2).
pub const DEFAULT_N_ROLES: usize = 3;

/// The scalar handed to [`aif::GroupAgent::group_distribution`].
///
/// **Inert by construction.** It flows through the `CopyAgent` sensory slot into
/// each member's [`InternalAgent::action_probabilities`](aif::InternalAgent),
/// which this module implements as the arm-E1 *multi-modality* replay (Amendment
/// A1.2) and which therefore ignores it. The group surface offers no way to pass
/// one observation per modality; carrying the replay vector on the member is how
/// arm-E1's A1.4 two-task replay survives into the group.
const SENSORY_OBSERVATION: usize = 0;

// --- Configuration ---------------------------------------------------------

/// How many persistent world models the arm keeps, and what each learns from
/// (Amendment A1.1 — this is where D2a's contrast now lives).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldModelTopology {
    /// `R` models, one per role, each observing **only its own role's bits** —
    /// the **confirmatory** legs `grp-role` / `grp-mult`.
    RoleSpecialised,
    /// One model observing the whole task, read through `R` role-restricted
    /// views — the non-gating reference legs `grp-role-fresh` / `grp-mult-fresh`.
    Shared,
}

/// Which precision channel drives the internals (D3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrecisionChannel {
    /// SP1 alone: internal `r` sees only its role's `(b, r)` demand.
    RoleRestricted,
    /// SP1 **plus** SP2: each modality's injected Dirichlet counts are scaled by
    /// the occurrence multiplicity `m(b, r)` of that step in the declared writing.
    ///
    /// Dirichlet counts *are* observation counts, so a step occurring three times
    /// carries three observations' worth of evidence demand — no tuned
    /// coefficient. Because `A = column_normalize(pA)` is invariant under a
    /// positive uniform scale, this moves the **concentration** and nothing else:
    /// the channel acts through the novelty / parameter-info-gain term, which is
    /// exactly v5's validated mechanism. It is consequently inert when learning is
    /// off (no counts are injected then) — the registered base has it on.
    MultiplicityWeighted,
}

/// Whose capabilities count toward internal `r`'s coverage masks `cfg0`/`cfg1`
/// (Amendment **A2.1**).
///
/// SP1 pinned `required_r` and was silent on the masks; A2.1 registers
/// [`RoleMatched`](Self::RoleMatched) and adds [`RoleBlind`](Self::RoleBlind) as a
/// single non-gating reference leg (`grp-role-blind`) at the
/// [`RoleRestricted`](PrecisionChannel::RoleRestricted) channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverageMasks {
    /// **Registered.** Only role-`r` participants can staff a role-`r` step, which
    /// is the world's own predicate (`p9_step_covered` requires the member's role
    /// to equal the step's role *and* the member to hold the bit). Anything else
    /// would score the query against a different notion of coverage than the world
    /// it is evaluated in.
    RoleMatched,
    /// **Reference leg only.** Every participant's capabilities count toward every
    /// internal's masks. Isolates how much of a margin comes from role-matched
    /// coverage versus role-restricted demand alone — under this setting the three
    /// internals differ **only** in `required_r`, specialists in name only.
    RoleBlind,
}

/// How the group's action is read (D4 + its registered exploratory contrast).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionRead {
    /// **Registered (D4).** [`group_distribution`](aif::GroupAgent::group_distribution) —
    /// no RNG drawn, no `last_action` advanced. `score` is the real margin
    /// `p(act) − 0.5`.
    Deterministic,
    /// **Exploratory `E-seed` only, non-gating.** Decide by the shipped
    /// [`Agent::act`](aif::Agent::act) draw, measuring what sampling costs against
    /// argmax. Seeded, so it stays reproducible.
    ///
    /// **This cell reports no margin.** `act` hands back a drawn action and no
    /// distribution, so `score` encodes the *draw* (`±0.5`) rather than a
    /// confidence — synthesising a margin from a sample would be inventing a
    /// number. Read `act` here, never `score`; PRIMARY, churn and superiority all
    /// depend on `act` alone, which is exactly what E-seed asks about.
    SeededSampling,
}

/// Registered configuration of the group arm. The [`Default`] is the
/// **confirmatory `grp-role`** cell over arm-E1's registered **v5 E1** base.
#[derive(Debug, Clone, Copy)]
pub struct GroupAifConfig {
    /// World-model topology (Amendment A1.1).
    pub topology: WorldModelTopology,
    /// Precision channel (D3).
    pub channel: PrecisionChannel,
    /// Whose capabilities count toward a role internal's coverage masks
    /// (Amendment A2.1). Confirmatory cells run
    /// [`CoverageMasks::RoleMatched`]; only the `grp-role-blind` reference leg
    /// differs.
    pub masks: CoverageMasks,
    /// How the group's action is read (D4). Every gating cell runs
    /// [`DecisionRead::Deterministic`]; only the exploratory `E-seed` cell differs.
    pub read: DecisionRead,
    /// The role count `R` the arm is configured for (D2: 3). Roles are indexed
    /// `0..n_roles`; a demand step naming a role outside that range is a
    /// [`GroupAifError::RoleOutOfRange`] at [`begin_task`](GroupAifPolicy::begin_task).
    pub n_roles: usize,
    /// The per-role world model's own configuration.
    ///
    /// **The default here is the v5 E1 arm, not [`PersistentAifConfig::default`]** —
    /// that default is the FALSIFIED v4 configuration (`query_dynamics: true`), and
    /// `EQ5b` contests the group *shape* over the validated model, not a re-tuned
    /// model (gotcha 24's relabelling discipline).
    pub base: PersistentAifConfig,
}

/// arm-E1's registered v5 configuration: learned per-bit precisions, `MeanField`
/// queries at the engine-default fixed γ = 16, novelty on, no precision dynamics.
#[must_use]
pub fn v5_e1_base() -> PersistentAifConfig {
    PersistentAifConfig {
        persistent_learning: true,
        query_dynamics: false,
        query_novelty: true,
        query_gamma: None,
        ..PersistentAifConfig::default()
    }
}

impl Default for GroupAifConfig {
    fn default() -> Self {
        Self {
            topology: WorldModelTopology::RoleSpecialised,
            channel: PrecisionChannel::RoleRestricted,
            masks: CoverageMasks::RoleMatched,
            read: DecisionRead::Deterministic,
            n_roles: DEFAULT_N_ROLES,
            base: v5_e1_base(),
        }
    }
}

// --- Errors ----------------------------------------------------------------

/// What can go wrong preparing a task for the group arm.
///
/// Hand-rolled `Display`/[`Error`](std::error::Error), in the same style as
/// `process::errors` and `topology::errors` — koalisi carries no `thiserror`
/// dependency.
#[derive(Debug)]
pub enum GroupAifError {
    /// A demand step names a role outside the configured `0..n_roles`.
    RoleOutOfRange {
        /// The offending role index.
        role: u8,
        /// The configured role count.
        n_roles: usize,
    },
    /// No `(bit, role)` of the demand lands inside the world model's `n_bits`
    /// universe, so no role could be staffed and no internal could be built.
    EmptyDemand {
        /// The world model's capability-bit width.
        n_bits: usize,
    },
    /// The engine rejected a model.
    Aif(aif::AifError),
}

impl std::fmt::Display for GroupAifError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RoleOutOfRange { role, n_roles } => write!(
                f,
                "group arm: demand names role r{role}, outside the configured R = {n_roles}"
            ),
            Self::EmptyDemand { n_bits } => write!(
                f,
                "group arm: no demanded (bit, role) lies inside the {n_bits}-bit universe"
            ),
            Self::Aif(inner) => write!(f, "group arm: engine rejection: {inner}"),
        }
    }
}

impl std::error::Error for GroupAifError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Aif(inner) => Some(inner),
            Self::RoleOutOfRange { .. } | Self::EmptyDemand { .. } => None,
        }
    }
}

impl From<aif::AifError> for GroupAifError {
    fn from(inner: aif::AifError) -> Self {
        Self::Aif(inner)
    }
}

// --- Instrumentation (S-learn, A1.3 / A1.4) --------------------------------

/// Which world model an audit row is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelLabel {
    /// A role-specialised model.
    Role(Role),
    /// The single shared model.
    Shared,
}

/// One world model's **counted** update ledger — the S-learn (i) gate.
///
/// The gate is *counted, not inferred from state movement*: under the persistent
/// model's MMP inference a missing action record makes an update apply **twice**
/// rather than not at all, so "did the state move?" passes a double-updating arm.
/// Both `updates < expected` (deficit) and `updates > expected` (surplus) are
/// RUN-INVALID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelUpdateAudit {
    /// The model this row describes.
    pub label: ModelLabel,
    /// Updates the engine actually applied.
    pub updates: u64,
    /// Updates it should have applied: tasks in which this model's role had
    /// demand (all observed tasks, for [`ModelLabel::Shared`]).
    pub expected: u64,
}

impl ModelUpdateAudit {
    /// Whether this model advanced exactly the right number of times.
    #[must_use]
    pub fn is_exact(&self) -> bool {
        self.updates == self.expected
    }
}

/// One decision's **E-agree** sample: how the role internals voted, and how far
/// the group's mixture landed from the threshold.
///
/// The votes are each member's own argmax — exactly what
/// [`group_distribution`](aif::GroupAgent::group_distribution) tallies under the
/// discrete modes and what `CertaintyWeighted` mixes — read back off the roster
/// after the read. It is a *disclosure*, never a decision input.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AgreementSample {
    /// Realised roster size for this decision (A1.3: `R` varies).
    pub roster: usize,
    /// How many internals' own argmax was "act".
    pub votes_for_act: usize,
    /// `|p(act) − 0.5|` — the CW mixture's distance from the SP3 threshold.
    /// Meaningless under [`DecisionRead::SeededSampling`], which reports no
    /// margin; that cell records `None`-shaped samples by simply not recording.
    pub margin: f64,
}

impl AgreementSample {
    /// Whether every internal voted the same way — the E-agree agreement event.
    #[must_use]
    pub fn unanimous(&self) -> bool {
        self.votes_for_act == 0 || self.votes_for_act == self.roster
    }
}

/// Everything the registered gates and disclosures need counted.
///
/// Struck by Amendment A1.4 and deliberately **absent**: member-advance counts and
/// committed-action counts. Members are single-use and there is no commit, so
/// neither has a referent — counting them would be ceremony that reads as a gate.
// `Eq` is deliberately absent: [`AgreementSample::margin`] is an `f64`. The
// counters are a report, not an equality key.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GroupAifCounters {
    /// Tasks whose outcome was observed into the world models.
    pub tasks_observed: u64,
    /// Decisions the arm returned an act/decline for, error declines included.
    pub decisions: u64,
    /// Successful `group_distribution` reads (`decisions` minus every decline
    /// that never reached the engine, minus upstream errors).
    pub reads: u64,
    /// Declines caused by an [`aif::AifError`] on the decision path.
    pub declines_upstream: u64,
    /// Declines caused by a participant absent from the role map.
    pub declines_missing_role: u64,
    /// Declines caused by there being no task in force, or no demanding role.
    pub declines_no_demand: u64,
    /// Realised roster size per task, in task order (Amendment A1.3 disclosure).
    pub roster_sizes: Vec<usize>,
    /// `(task, role)` slots where the role had no demand and did not vote.
    pub empty_role_slots: u64,
    /// Per-model update ledger — the S-learn (i) gate.
    pub model_updates: Vec<ModelUpdateAudit>,
    /// One sample per successful deterministic read — the **E-agree** disclosure.
    /// Empty under [`DecisionRead::SeededSampling`], which exposes no mixture.
    pub agreement: Vec<AgreementSample>,
}

impl GroupAifCounters {
    /// Whether every world model advanced exactly its expected number of times.
    #[must_use]
    pub fn s_learn_exact(&self) -> bool {
        self.model_updates.iter().all(ModelUpdateAudit::is_exact)
    }
}

// --- The member wrapper (Amendment A1.2) -----------------------------------

/// One role's internal agent: an arm-E1 query POMDP plus the replay window it must
/// see, presented to [`aif::GroupAgent`] as an [`InternalAgent`](aif::InternalAgent).
///
/// # Why a wrapper exists at all
///
/// `GroupAgent` feeds every member one **scalar** observation, but an arm-E1 query
/// carries `|required_r|` modalities; a scalar would reach modality 0 alone and
/// silently discard arm-E1's A1.4 two-task replay — hollowing out the very
/// mechanism `EQ5b` exists to carry into the workflow world. `GroupAgent<S, I, X>` is
/// generic in `I`, so the member can carry its own observation vector instead. RNG
/// freedom is preserved: this is a leaf wrapper over a `POMDPAgent`, not a nested
/// group or a sampling sensory slot (gotcha 32).
struct RoleMember {
    query: aif::POMDPAgent,
    /// Replay sequences from the world model, restricted to this role's bits.
    replay: Vec<Vec<usize>>,
    /// `|required_r|` — the neutral read's width when the replay window is empty.
    modalities: usize,
    /// This member's own argmax from its last read — the **E-agree** vote. Read
    /// back through [`aif::GroupAgent::internal_agents`] after the group read; a
    /// disclosure only, never a decision input.
    last_vote: Option<usize>,
}

impl aif::Agent for RoleMember {
    /// Unsupported on purpose: this member exists to be **read**, never to act.
    ///
    /// `GroupAgent::act` would call it; this arm calls only `group_distribution`,
    /// which polls
    /// [`InternalAgent::action_probabilities`](aif::InternalAgent::action_probabilities)
    /// instead. Reporting [`Unsupported`](aif::AifError::Unsupported) rather than
    /// sampling something is what keeps "this arm never draws" checkable.
    fn act(&mut self, _observation: usize) -> Result<usize, aif::AifError> {
        Err(aif::AifError::Unsupported(
            "koalisi group role member is a deterministic read-only slot; \
             use GroupAgent::group_distribution, not act"
                .to_string(),
        ))
    }
}

impl aif::InternalAgent for RoleMember {
    /// The arm-E1 replay + marginal action posterior, ignoring the group's scalar
    /// observation (see [`SENSORY_OBSERVATION`]).
    ///
    /// The trait has no error channel, so an engine rejection returns an **empty**
    /// distribution, which `group_distribution` turns into
    /// [`AifError::InvalidLength`](aif::AifError::InvalidLength) before anything is
    /// aggregated. The policy then declines and counts. Deterministic, non-panicking,
    /// and visible in the counters rather than smuggled into a score.
    fn action_probabilities(&mut self, _observation: usize) -> DVector<f64> {
        match run_replay(&mut self.query, &self.replay, self.modalities) {
            Ok(dist) => {
                // Ties to the lowest index, matching the engine's own
                // `argmax_index` so the recorded vote is the one the group would
                // have tallied.
                self.last_vote = dist
                    .iter()
                    .enumerate()
                    .fold(None::<(usize, f64)>, |best, (i, &p)| match best {
                        Some((_, b)) if b >= p => best,
                        _ => Some((i, p)),
                    })
                    .map(|(i, _)| i);
                dist
            }
            Err(e) => {
                tracing::warn!(error = %e, "group role member replay failed; declining the read");
                self.last_vote = None;
                DVector::zeros(0)
            }
        }
    }

    fn record_action(&mut self, action: usize) {
        self.query.record_action(action);
    }

    fn reseed(&mut self, seed: u64) {
        self.query.reseed(seed);
    }
}

// --- Per-task state --------------------------------------------------------

/// The workflow demand of the task currently in force, pre-resolved per role.
#[derive(Debug, Clone)]
struct TaskState {
    /// `required_r` per role index, masked into the world model's universe.
    /// `0` ⇒ the role has no demand and does not vote (Amendment A1.3).
    required: Vec<u32>,
    /// `multiplicity[role][bit]` — SP2's `m(b, r)`, `0` outside the demand.
    multiplicity: Vec<Vec<u64>>,
    /// Demanding roles, ascending — the realised roster.
    roster: Vec<Role>,
    /// OR of every `required_r` — what a [`WorldModelTopology::Shared`] model
    /// observes.
    union_required: u32,
}

/// Mutable arm state behind one mutex.
struct Shared {
    task: Option<TaskState>,
    counters: GroupAifCounters,
    /// Per-model applied-update counts, index-aligned with [`Models`].
    updates: Vec<u64>,
    /// Per-model expected-update counts.
    expected: Vec<u64>,
    /// Monotonic decision counter — derives the per-decision seed (hygiene only).
    decision_counter: u64,
}

/// The arm's persistent world models (Amendment A1.1).
enum Models {
    /// One per role, index = role index.
    RoleSpecialised(Vec<PersistentAifArm>),
    /// One for the whole task.
    Shared(PersistentAifArm),
}

impl Models {
    /// The model internal `role` reads from.
    fn view(&self, role: Role) -> Option<&PersistentAifArm> {
        match self {
            Self::RoleSpecialised(models) => models.get(role.index() as usize),
            Self::Shared(model) => Some(model),
        }
    }

    fn labels(&self, n_roles: usize) -> Vec<ModelLabel> {
        match self {
            Self::RoleSpecialised(_) => (0..n_roles)
                .map(|r| {
                    ModelLabel::Role(Role::new(
                        u8::try_from(r).expect("invariant: n_roles is clamped below u8::MAX"),
                    ))
                })
                .collect(),
            Self::Shared(_) => vec![ModelLabel::Shared],
        }
    }

    fn len(&self) -> usize {
        match self {
            Self::RoleSpecialised(models) => models.len(),
            Self::Shared(_) => 1,
        }
    }

    fn at(&self, index: usize) -> Option<&PersistentAifArm> {
        match self {
            Self::RoleSpecialised(models) => models.get(index),
            Self::Shared(model) => (index == 0).then_some(model),
        }
    }
}

// --- The arm ---------------------------------------------------------------

/// Role-slotted Active Inference group join/leave policy (`grp-*`, prereg K4-EQ5b).
///
/// One arm per seed. The harness calls [`begin_task`](Self::begin_task) with the
/// task's workflow [`Demand`] before its decisions, and
/// [`observe_outcome`](Self::observe_outcome) once after its leave sweep. See the
/// `decision::group_policy` module documentation for the five load-bearing
/// properties. (A plain code span, not an intra-doc link: the module is private
/// behind a re-export, like every other `decision` arm, so a `self` link would
/// point at an undocumented item.)
pub struct GroupAifPolicy {
    models: Models,
    config: GroupAifConfig,
    /// `agent_id → Role`. Only ever *looked up*, never iterated — no `HashMap`
    /// ordering reaches a decision.
    agent_roles: HashMap<usize, Role>,
    battery_seed: u64,
    /// The world model width, read once at construction (post-clamp).
    n_bits: usize,
    shared: Mutex<Shared>,
}

impl GroupAifPolicy {
    /// Build a fresh group arm for `battery_seed`.
    ///
    /// `agent_roles` maps every pool worker's `agent_id` to its [`Role`]; a
    /// participant absent from it makes the decision a **decline-and-count**
    /// rather than an invented answer (the gotcha-28 typed-arm contract).
    ///
    /// # Errors
    ///
    /// [`GroupAifError::Aif`] if the engine rejects a world model.
    ///
    /// # Panics
    ///
    /// Never in practice: the only `expect` here asserts that a topology built at
    /// least one model, which both arms of the match above do unconditionally.
    pub fn new(
        battery_seed: u64,
        config: GroupAifConfig,
        agent_roles: HashMap<usize, Role>,
    ) -> Result<Self, GroupAifError> {
        let n_roles = config.n_roles.clamp(1, usize::from(u8::MAX));
        let config = GroupAifConfig { n_roles, ..config };

        let models = match config.topology {
            WorldModelTopology::RoleSpecialised => {
                let mut built = Vec::with_capacity(n_roles);
                for r in 0..n_roles {
                    // Distinct per-model seeds are hygiene: the world model never
                    // samples (single control, no precision dynamics), so this
                    // cannot perturb a decision — it only keeps the models from
                    // sharing an RNG stream by accident.
                    let seed = battery_seed ^ splitmix64(r as u64 + 1);
                    built.push(PersistentAifArm::new(seed, config.base)?);
                }
                Models::RoleSpecialised(built)
            }
            WorldModelTopology::Shared => {
                Models::Shared(PersistentAifArm::new(battery_seed, config.base)?)
            }
        };

        let n_bits = models
            .at(0)
            .expect("invariant: every topology builds at least one model")
            .n_bits();
        let model_count = models.len();

        Ok(Self {
            models,
            config,
            agent_roles,
            battery_seed,
            n_bits,
            shared: Mutex::new(Shared {
                task: None,
                counters: GroupAifCounters::default(),
                updates: vec![0; model_count],
                expected: vec![0; model_count],
                decision_counter: 0,
            }),
        })
    }

    /// Put a task's workflow demand in force and return the **realised roster
    /// size** — how many roles actually vote on it (Amendment A1.3).
    ///
    /// Call exactly once per task, before that task's decisions. Bits outside the
    /// world model's `n_bits` universe carry no factor and are dropped, exactly as
    /// arm-E1 drops them from `required`.
    ///
    /// # Errors
    ///
    /// [`GroupAifError::RoleOutOfRange`] if the demand names a role outside
    /// `0..n_roles`, and [`GroupAifError::EmptyDemand`] if nothing survives the
    /// universe mask. Either clears the task, so subsequent decisions decline and
    /// count rather than silently reusing the previous task's demand.
    ///
    /// # Panics
    ///
    /// Only on a poisoned arm mutex — i.e. only after another thread already
    /// panicked while holding it.
    pub fn begin_task(&self, demand: &Demand) -> Result<usize, GroupAifError> {
        let n_roles = self.config.n_roles;
        let mut required = vec![0u32; n_roles];
        let mut multiplicity = vec![vec![0u64; self.n_bits]; n_roles];

        let mut shared = self.shared.lock().expect("group arm mutex poisoned");
        shared.task = None;

        for step in demand.distinct() {
            let role = usize::from(step.role.index());
            if role >= n_roles {
                return Err(GroupAifError::RoleOutOfRange {
                    role: step.role.index(),
                    n_roles,
                });
            }
            let bit = usize::from(step.bit);
            if bit >= self.n_bits {
                // Outside the universe: no factor exists for it, so it cannot enter
                // a query. Dropped rather than errored — arm-E1 masks `required` the
                // same way, and a workflow wider than the world model is a
                // configuration choice, not a fault.
                continue;
            }
            required[role] |= 1u32 << bit;
            multiplicity[role][bit] = demand.multiplicity(step) as u64;
        }

        let roster: Vec<Role> = (0..n_roles)
            .filter(|&r| required[r] != 0)
            .map(|r| {
                Role::new(u8::try_from(r).expect("invariant: n_roles is clamped below u8::MAX"))
            })
            .collect();
        if roster.is_empty() {
            return Err(GroupAifError::EmptyDemand {
                n_bits: self.n_bits,
            });
        }

        let union_required = required.iter().fold(0u32, |acc, &m| acc | m);
        let size = roster.len();
        shared.counters.roster_sizes.push(size);
        shared.counters.empty_role_slots += (n_roles - size) as u64;
        shared.task = Some(TaskState {
            required,
            multiplicity,
            roster,
            union_required,
        });
        Ok(size)
    }

    /// Observe one task outcome into the world models — call once per task, after
    /// the leave sweep, exactly as arm-E1's
    /// [`observe_outcome`](PersistentAifArm::observe_outcome) is called.
    ///
    /// Under [`WorldModelTopology::RoleSpecialised`] each **demanding** role's model
    /// observes its own `required_r`; roles that left the roster observe nothing and
    /// their expected-update count does not advance. Under
    /// [`WorldModelTopology::Shared`] the one model observes the union — arm-E1's
    /// own call, verbatim.
    ///
    /// Returns how many models the engine actually advanced. A width mismatch is
    /// warned and the whole task is skipped (nothing is counted), so the S-learn
    /// ledger can never be inflated by an update that did not happen.
    ///
    /// # Panics
    ///
    /// Only on a poisoned arm mutex.
    pub fn observe_outcome(&self, per_bit_success: &[bool]) -> usize {
        if per_bit_success.len() != self.n_bits {
            tracing::warn!(
                got = per_bit_success.len(),
                expected = self.n_bits,
                "group arm observe_outcome width mismatch, skipping the task"
            );
            return 0;
        }
        let mut shared = self.shared.lock().expect("group arm mutex poisoned");
        let Some(task) = shared.task.clone() else {
            tracing::warn!("group arm observe_outcome with no task in force, skipping");
            return 0;
        };

        let mut applied = 0usize;
        match &self.models {
            Models::RoleSpecialised(models) => {
                for &role in &task.roster {
                    let index = usize::from(role.index());
                    let Some(model) = models.get(index) else {
                        continue;
                    };
                    shared.expected[index] += 1;
                    if model.observe_outcome_checked(task.required[index], per_bit_success) {
                        shared.updates[index] += 1;
                        applied += 1;
                    }
                }
            }
            Models::Shared(model) => {
                shared.expected[0] += 1;
                if model.observe_outcome_checked(task.union_required, per_bit_success) {
                    shared.updates[0] += 1;
                    applied = 1;
                }
            }
        }
        shared.counters.tasks_observed += 1;
        applied
    }

    /// The counted gate + disclosure ledger (S-learn (i), Amendment A1.3/A1.4).
    ///
    /// # Panics
    ///
    /// Only on a poisoned arm mutex.
    #[must_use]
    pub fn counters(&self) -> GroupAifCounters {
        let shared = self.shared.lock().expect("group arm mutex poisoned");
        let labels = self.models.labels(self.config.n_roles);
        let mut counters = shared.counters.clone();
        counters.model_updates = labels
            .into_iter()
            .enumerate()
            .map(|(i, label)| ModelUpdateAudit {
                label,
                updates: shared.updates.get(i).copied().unwrap_or(0),
                expected: shared.expected.get(i).copied().unwrap_or(0),
            })
            .collect();
        counters
    }

    /// Snapshot every world model — the S-learn (i) **non-vacuity guard** (end of
    /// stream must differ from initialization). Necessary, explicitly not
    /// sufficient; the counted ledger in [`counters`](Self::counters) is the gate.
    #[must_use]
    pub fn model_snapshots(&self) -> Vec<(ModelLabel, PersistentAifState)> {
        self.models
            .labels(self.config.n_roles)
            .into_iter()
            .enumerate()
            .filter_map(|(i, label)| Some((label, self.models.at(i)?.state_snapshot())))
            .collect()
    }

    /// The configuration in force (post-clamp).
    #[must_use]
    pub fn config(&self) -> GroupAifConfig {
        self.config
    }

    /// The role of one participant, or `None` when the map has no entry.
    fn role_of(&self, agent: &dyn AgentCapabilities) -> Option<Role> {
        self.agent_roles.get(&agent.agent_id()).copied()
    }

    /// A decline in the arm's own shape. A zero score is **not** a decline — the
    /// caller reads [`counters`](Self::counters) for that (standing disclosure).
    fn declined() -> Decision {
        Decision {
            act: false,
            score: 0.0,
        }
    }

    /// Shared decision core.
    ///
    /// `leave` selects the coverage configuration only; the SP3 rule is applied
    /// uniformly (`act` iff `p(act) > 0.5`, ties decline) because SP3 pins one
    /// criterion for the arm rather than arm-E1's join/leave tie split.
    fn decide(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        leave: bool,
    ) -> Decision {
        let mut shared = self.shared.lock().expect("group arm mutex poisoned");
        shared.counters.decisions += 1;

        let Some(task) = shared.task.clone() else {
            shared.counters.declines_no_demand += 1;
            return Self::declined();
        };

        // Role resolution first: an unmapped participant is a decline, never a
        // guess (gotcha 28). The candidate is resolved even on the leave path,
        // where it is also a coalition member, so a partial map cannot slip through.
        let Some(agent_role) = self.role_of(agent) else {
            shared.counters.declines_missing_role += 1;
            return Self::declined();
        };
        let mut members: Vec<(Role, usize, u32)> = Vec::with_capacity(coalition.len());
        for m in coalition {
            let Some(role) = self.role_of(*m) else {
                shared.counters.declines_missing_role += 1;
                return Self::declined();
            };
            members.push((role, m.agent_id(), m.capabilities()));
        }

        shared.decision_counter = shared.decision_counter.wrapping_add(1);
        let seed = self.battery_seed ^ splitmix64(shared.decision_counter);
        drop(shared);

        let agent_id = agent.agent_id();
        let agent_caps = agent.capabilities();

        let mut internals: Vec<RoleMember> = Vec::with_capacity(task.roster.len());
        for &role in &task.roster {
            let required_r = task.required[usize::from(role.index())] & low_mask(self.n_bits);
            let (cfg0, cfg1) = self.coverage_masks(
                role,
                (agent_role, agent_id, agent_caps),
                &members,
                leave,
            );

            let scale = match self.config.channel {
                PrecisionChannel::RoleRestricted => None,
                PrecisionChannel::MultiplicityWeighted => Some(self.scale_for(&task, role)),
            };

            let Some(model) = self.models.view(role) else {
                self.count_upstream_decline();
                tracing::warn!(role = role.index(), "group arm has no world model for the role");
                return Self::declined();
            };
            let member_seed = seed ^ splitmix64(u64::from(role.index()) + 1);
            match model.role_query(required_r, cfg0, cfg1, member_seed, scale.as_deref()) {
                Ok((query, replay)) => internals.push(RoleMember {
                    query,
                    replay,
                    modalities: required_r.count_ones() as usize,
                    last_vote: None,
                }),
                Err(e) => {
                    tracing::warn!(error = %e, role = role.index(), "group role query construction failed");
                    self.count_upstream_decline();
                    return Self::declined();
                }
            }
        }

        // D2: `CertaintyWeighted` is forced, not chosen — with ≤ 3 voters over 2
        // actions `Deterministic` is always a delta and `Probabilistic` is
        // supported on {0, ⅓, ⅔, 1}; neither carries a usable margin. The seeds are
        // hygiene: `group_distribution` draws nothing.
        let roster = internals.len();
        let mut group = aif::GroupAgent::with_slots_seeded(
            aif::CopyAgent,
            internals,
            aif::VotingAgent::with_seed(GROUP_N_ACTIONS, aif::VotingMode::CertaintyWeighted, seed),
            GROUP_N_ACTIONS,
            seed,
        );

        if self.config.read == DecisionRead::SeededSampling {
            return self.sampled_decision(&mut group);
        }

        // D4/D4a-as-amended: the pure read, and NOTHING else. No
        // `record_group_action`, no `group_distribution_recording`.
        let dist = match group.group_distribution(SENSORY_OBSERVATION) {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!(error = %e, "group distribution read failed");
                self.count_upstream_decline();
                return Self::declined();
            }
        };

        let p_act = dist.get(ACTION_ACT).copied().unwrap_or(f64::NAN);
        if !p_act.is_finite() {
            tracing::warn!(p_act, "group distribution returned a non-finite p(act)");
            self.count_upstream_decline();
            return Self::declined();
        }

        // E-agree: read the members' own argmaxes back off the roster. Purely a
        // disclosure — nothing above consulted them.
        let votes_for_act = group
            .internal_agents()
            .iter()
            .filter(|m| m.last_vote == Some(ACTION_ACT))
            .count();

        let mut shared = self.shared.lock().expect("group arm mutex poisoned");
        shared.counters.reads += 1;
        shared.counters.agreement.push(AgreementSample {
            roster,
            votes_for_act,
            margin: (p_act - 0.5).abs(),
        });
        drop(shared);

        // SP3: act iff p(act) > 0.5; ties decline.
        Decision {
            act: p_act > 0.5,
            score: p_act - 0.5,
        }
    }

    /// Internal `role`'s coverage masks `(cfg0, cfg1)` for one decision
    /// (Amendment **A2.1**).
    ///
    /// Under the registered [`CoverageMasks::RoleMatched`] only role-`role`
    /// participants count — the world's own predicate. [`CoverageMasks::RoleBlind`]
    /// is the `grp-role-blind` reference leg and counts everyone.
    ///
    /// `candidate` is `(role, id, capabilities)` of the agent being decided about;
    /// `members` is the coalition as `(role, id, capabilities)`, including the
    /// candidate on the leave path.
    fn coverage_masks(
        &self,
        role: Role,
        candidate: (Role, usize, u32),
        members: &[(Role, usize, u32)],
        leave: bool,
    ) -> (u32, u32) {
        let (agent_role, agent_id, agent_caps) = candidate;
        let role_matched = self.config.masks == CoverageMasks::RoleMatched;
        let member_union = members
            .iter()
            .filter(|&&(r, id, _)| (!role_matched || r == role) && !(leave && id == agent_id))
            .fold(0u32, |acc, &(_, _, caps)| acc | caps);
        let own = if role_matched && agent_role != role {
            0
        } else {
            agent_caps
        };
        if leave {
            // cfg0 = the coalition WITH the member, cfg1 = without it.
            (member_union | own, member_union)
        } else {
            // cfg0 = the candidate alone, cfg1 = coalition ∪ {candidate}.
            (own, own | member_union)
        }
    }

    /// The `E-seed` branch: decide by the shipped draw (exploratory, non-gating).
    ///
    /// Under `CertaintyWeighted`, [`Agent::act`](aif::Agent::act) polls members
    /// through `action_probabilities` and samples with the group's own seeded RNG
    /// — it never calls a member's `act`, which is why the read-only wrapper can
    /// refuse that method and this cell still runs.
    fn sampled_decision<I: aif::InternalAgent>(
        &self,
        group: &mut aif::GroupAgent<aif::CopyAgent, I, aif::VotingAgent>,
    ) -> Decision {
        match aif::Agent::act(group, SENSORY_OBSERVATION) {
            Ok(action) => {
                let mut shared = self.shared.lock().expect("group arm mutex poisoned");
                shared.counters.reads += 1;
                drop(shared);
                let act = action == ACTION_ACT;
                Decision {
                    // NOT a margin — see `DecisionRead::SeededSampling`.
                    score: if act { 0.5 } else { -0.5 },
                    act,
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "group sampled act failed");
                self.count_upstream_decline();
                Self::declined()
            }
        }
    }

    /// SP2's per-modality count scale for `role`, index-aligned with the query's
    /// modalities (`set_bits(required_r)`).
    fn scale_for(&self, task: &TaskState, role: Role) -> Vec<f64> {
        let index = usize::from(role.index());
        let required_r = task.required[index] & low_mask(self.n_bits);
        set_bits(required_r, self.n_bits)
            .into_iter()
            .map(|bit| {
                let m = task
                    .multiplicity
                    .get(index)
                    .and_then(|row| row.get(bit))
                    .copied()
                    .unwrap_or(1)
                    .max(1);
                multiplicity_as_f64(m)
            })
            .collect()
    }

    fn count_upstream_decline(&self) {
        let mut shared = self.shared.lock().expect("group arm mutex poisoned");
        shared.counters.declines_upstream += 1;
    }
}

/// An occurrence multiplicity as an `f64`, so the one lossy conversion in this
/// module has one site and one justification: a step's occurrence count in a
/// drawn workflow is bounded by the task's step count (single digits here, and
/// far below `2^53` for anything a caller could plausibly build).
fn multiplicity_as_f64(m: u64) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    {
        m as f64
    }
}

impl CoalitionDecisionPolicy for GroupAifPolicy {
    /// Join iff the group's `p(act) > 0.5` — `coalition` excludes `agent`.
    fn should_join(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        _ctx: &DecisionContext,
    ) -> Decision {
        self.decide(agent, coalition, false)
    }

    /// Leave iff the group's `p(act) > 0.5` — `coalition` includes `agent`.
    ///
    /// Note the tie rule: SP3 pins `>` with **ties declining** for the whole arm,
    /// where arm-E1 leaves on a tie. One criterion, applied to both questions, is
    /// what makes "`EQ5b` changes the engine, not the criterion" checkable.
    fn should_leave(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        _ctx: &DecisionContext,
    ) -> Decision {
        self.decide(agent, coalition, true)
    }
}

#[cfg(test)]
mod tests {
    use catgraph_applied::prop::colored::ColoredExpr;

    use super::super::aif_persistent_policy::NO_OBS;
    use super::*;
    use crate::process::{Step, chain, demand, step_expr};

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

    /// A single-role chain of `steps`, as a `Workflow`.
    fn leg(role: Role, bits: &[u8]) -> catgraph_applied::prop::PropExpr<crate::process::WorkflowGen>
    {
        chain(
            bits.iter()
                .map(|&b| step_expr(Step::new(b, role)))
                .collect(),
        )
        .expect("a non-empty chain of same-role 1->1 steps composes")
    }

    /// A workflow over `legs = [(role, bits)]`, each role a sequential chain.
    fn workflow(legs: &[(Role, &[u8])]) -> crate::process::Workflow {
        let mut source = Vec::new();
        let mut exprs = Vec::new();
        for &(role, bits) in legs {
            source.push(role);
            exprs.push(leg(role, bits));
        }
        let expr = exprs
            .into_iter()
            .reduce(catgraph_applied::prop::Free::tensor)
            .expect("at least one leg");
        ColoredExpr::new(source, expr).expect("every leg is a single-role r -> r chain")
    }

    /// The three-role roster used by most tests: r0 needs bit 0, r1 bit 1, r2 bit 2.
    fn three_role_demand() -> Demand {
        demand(&workflow(&[
            (Role::new(0), &[0]),
            (Role::new(1), &[1]),
            (Role::new(2), &[2]),
        ]))
    }

    fn roles_map(pairs: &[(usize, u8)]) -> HashMap<usize, Role> {
        pairs
            .iter()
            .map(|&(id, r)| (id, Role::new(r)))
            .collect()
    }

    fn policy(config: GroupAifConfig, roles: &[(usize, u8)]) -> GroupAifPolicy {
        GroupAifPolicy::new(11, config, roles_map(roles)).unwrap()
    }

    /// The default config is the **v5 E1** base, not the FALSIFIED v4 default
    /// (gotcha 24's relabelling discipline), and the registered `grp-role` cell.
    #[test]
    fn default_config_is_the_registered_cell() {
        let c = GroupAifConfig::default();
        assert_eq!(c.topology, WorldModelTopology::RoleSpecialised);
        assert_eq!(c.channel, PrecisionChannel::RoleRestricted);
        assert_eq!(c.n_roles, DEFAULT_N_ROLES);
        assert!(c.base.persistent_learning);
        assert!(c.base.query_novelty);
        assert!(
            !c.base.query_dynamics,
            "v5 E1 runs MeanField queries at fixed gamma, NOT the v4 precision dynamics"
        );
        assert!(c.base.query_gamma.is_none(), "engine-default gamma = 16");
    }

    /// The `CertaintyWeighted` read is RNG-free: two reads from identical state
    /// agree bit-for-bit, and a whole decision stream is a pure function of the
    /// seed (the S-determinism gate, library half).
    #[test]
    fn read_is_rng_free_and_the_stream_is_deterministic() {
        let a0 = TestAgent { id: 0, caps: 0b001, trust: 50 };
        let a1 = TestAgent { id: 1, caps: 0b010, trust: 50 };
        let a2 = TestAgent { id: 2, caps: 0b100, trust: 50 };
        let coalition: [&dyn AgentCapabilities; 2] = [&a1, &a2];
        let full: [&dyn AgentCapabilities; 3] = [&a0, &a1, &a2];
        let ctx = DecisionContext { required_capabilities: 0b111 };
        let map = [(0usize, 0u8), (1, 1), (2, 2)];

        // Two reads from identical state: build two arms, run one read each.
        let one = policy(GroupAifConfig::default(), &map);
        let two = policy(GroupAifConfig::default(), &map);
        one.begin_task(&three_role_demand()).unwrap();
        two.begin_task(&three_role_demand()).unwrap();
        let d1 = one.should_join(&a0, &coalition, &ctx);
        let d2 = two.should_join(&a0, &coalition, &ctx);
        assert_eq!(
            d1.score.to_bits(),
            d2.score.to_bits(),
            "identical state must produce an identical mixture"
        );
        assert_eq!(d1.act, d2.act);

        // A whole stream, twice.
        let run = || {
            let p = policy(GroupAifConfig::default(), &map);
            let mut out = Vec::new();
            for _ in 0..3 {
                p.begin_task(&three_role_demand()).unwrap();
                let j = p.should_join(&a0, &coalition, &ctx);
                out.push((j.act, j.score.to_bits()));
                let l = p.should_leave(&a0, &full, &ctx);
                out.push((l.act, l.score.to_bits()));
                p.observe_outcome(&[true, false, true, false, false, false, false, false]);
            }
            out
        };
        assert_eq!(run(), run(), "same seed ⇒ same result");
    }

    /// SP3: `act` iff `p(act) > 0.5`, **ties decline**. Exercised directly on the
    /// rule, because a tie is not reachable on demand through the engine.
    #[test]
    fn sp3_ties_decline() {
        for (p_act, expected) in [(0.5_f64, false), (0.5 + f64::EPSILON, true), (0.49, false)] {
            let d = Decision {
                act: p_act > 0.5,
                score: p_act - 0.5,
            };
            assert_eq!(d.act, expected, "p(act) = {p_act} must act = {expected}");
        }

        // And the live arm never acts on an exact-0.0 score.
        let a0 = TestAgent { id: 0, caps: 0b001, trust: 50 };
        let a1 = TestAgent { id: 1, caps: 0b010, trust: 50 };
        let coalition: [&dyn AgentCapabilities; 1] = [&a1];
        let ctx = DecisionContext::default();
        let p = policy(GroupAifConfig::default(), &[(0, 0), (1, 1)]);
        p.begin_task(&demand(&workflow(&[
            (Role::new(0), &[0]),
            (Role::new(1), &[1]),
        ])))
        .unwrap();
        let d = p.should_join(&a0, &coalition, &ctx);
        assert!(
            !(d.score == 0.0 && d.act),
            "a zero margin must never act (SP3 ties decline)"
        );
    }

    /// A participant absent from the role map declines and counts — it never
    /// invents a decision (gotcha 28's typed-arm contract).
    #[test]
    fn missing_role_map_entry_declines_and_counts() {
        let a0 = TestAgent { id: 0, caps: 0b001, trust: 50 };
        let stranger = TestAgent { id: 9, caps: 0b010, trust: 50 };
        let ctx = DecisionContext::default();

        // The candidate is unmapped.
        let p = policy(GroupAifConfig::default(), &[(1, 1), (2, 2)]);
        p.begin_task(&three_role_demand()).unwrap();
        let d = p.should_join(&a0, &[], &ctx);
        assert!(!d.act && d.score == 0.0);
        assert_eq!(p.counters().declines_missing_role, 1);

        // A coalition MEMBER is unmapped.
        let q = policy(GroupAifConfig::default(), &[(0, 0)]);
        q.begin_task(&three_role_demand()).unwrap();
        let coalition: [&dyn AgentCapabilities; 1] = [&stranger];
        let d = q.should_join(&a0, &coalition, &ctx);
        assert!(!d.act && d.score == 0.0);
        assert_eq!(q.counters().declines_missing_role, 1);
        assert_eq!(q.counters().reads, 0, "no read happened");
    }

    /// Amendment A1.3: a role with no demand leaves the roster, `R` varies, and
    /// both the realised sizes and the empty-slot rate are disclosed.
    #[test]
    fn empty_demand_roles_leave_the_roster() {
        let p = policy(GroupAifConfig::default(), &[(0, 0), (1, 1), (2, 2)]);
        assert_eq!(p.begin_task(&three_role_demand()).unwrap(), 3);
        assert_eq!(
            p.begin_task(&demand(&workflow(&[(Role::new(1), &[1, 3])])))
                .unwrap(),
            1,
            "only r1 has demand"
        );
        assert_eq!(
            p.begin_task(&demand(&workflow(&[
                (Role::new(0), &[0]),
                (Role::new(2), &[2]),
            ])))
            .unwrap(),
            2
        );

        let c = p.counters();
        assert_eq!(c.roster_sizes, vec![3, 1, 2]);
        // (3 - 3) + (3 - 1) + (3 - 2) — the summed empty slots.
        assert_eq!(c.empty_role_slots, 3);

        // A role outside the configured R is an error, and it clears the task so
        // later decisions decline rather than reuse stale demand.
        let err = p.begin_task(&demand(&workflow(&[(Role::new(7), &[0])])));
        assert!(matches!(
            err,
            Err(GroupAifError::RoleOutOfRange { role: 7, n_roles: 3 })
        ));
        let a0 = TestAgent { id: 0, caps: 0b001, trust: 50 };
        let d = p.should_join(&a0, &[], &DecisionContext::default());
        assert!(!d.act);
        assert_eq!(p.counters().declines_no_demand, 1);
    }

    /// SP2 identity: at all-unit multiplicity the multiplicity-weighted channel
    /// reproduces the role-restricted one **bit for bit** — the library half of
    /// X-identity, and the pin that keeps the channel from being a free parameter.
    #[test]
    fn unit_multiplicity_is_bit_identical_to_the_role_restricted_channel() {
        let a0 = TestAgent { id: 0, caps: 0b001, trust: 50 };
        let a1 = TestAgent { id: 1, caps: 0b010, trust: 50 };
        let a2 = TestAgent { id: 2, caps: 0b100, trust: 50 };
        let coalition: [&dyn AgentCapabilities; 2] = [&a1, &a2];
        let ctx = DecisionContext { required_capabilities: 0b111 };
        let map = [(0usize, 0u8), (1, 1), (2, 2)];

        // Every step occurs exactly once ⇒ m(b, r) == 1 everywhere.
        let flat = three_role_demand();
        for step in flat.distinct() {
            assert_eq!(flat.multiplicity(step), 1);
        }

        let run = |channel| {
            let p = policy(
                GroupAifConfig {
                    channel,
                    ..GroupAifConfig::default()
                },
                &map,
            );
            let mut out = Vec::new();
            for _ in 0..3 {
                p.begin_task(&three_role_demand()).unwrap();
                let j = p.should_join(&a0, &coalition, &ctx);
                out.push((j.act, j.score.to_bits()));
                p.observe_outcome(&[true, true, false, false, false, false, false, false]);
            }
            out
        };
        assert_eq!(
            run(PrecisionChannel::MultiplicityWeighted),
            run(PrecisionChannel::RoleRestricted),
            "m == 1 everywhere must be the identity configuration"
        );
    }

    /// SP2 is **live**: repeating a step changes the decision stream, even though
    /// repetition adds no coverage demand (the whole point of the channel).
    #[test]
    fn repeated_steps_move_the_multiplicity_channel() {
        let a0 = TestAgent { id: 0, caps: 0b001, trust: 50 };
        let a1 = TestAgent { id: 1, caps: 0b010, trust: 50 };
        let coalition: [&dyn AgentCapabilities; 1] = [&a1];
        let ctx = DecisionContext { required_capabilities: 0b011 };
        let map = [(0usize, 0u8), (1, 1)];

        // Same DISTINCT demand, different multiplicity.
        let once = demand(&workflow(&[(Role::new(0), &[0]), (Role::new(1), &[1])]));
        let thrice = demand(&workflow(&[
            (Role::new(0), &[0, 0, 0]),
            (Role::new(1), &[1]),
        ]));
        assert_eq!(once.distinct_len(), thrice.distinct_len());
        assert_eq!(thrice.multiplicity(Step::new(0, Role::new(0))), 3);

        let run = |d: &Demand| {
            let p = policy(
                GroupAifConfig {
                    channel: PrecisionChannel::MultiplicityWeighted,
                    ..GroupAifConfig::default()
                },
                &map,
            );
            p.begin_task(d).unwrap();
            p.should_join(&a0, &coalition, &ctx).score.to_bits()
        };
        assert_ne!(
            run(&once),
            run(&thrice),
            "multiplicity must reach the score; an inert channel is the failure \
             class this lineage keeps shipping"
        );
    }

    /// Amendment A1.1: the two topologies are genuinely different arms — the
    /// role-specialised models learn from disjoint bit sets and diverge from the
    /// one shared model over a task stream.
    #[test]
    fn topologies_diverge_over_a_stream() {
        let a0 = TestAgent { id: 0, caps: 0b001, trust: 50 };
        let a1 = TestAgent { id: 1, caps: 0b010, trust: 50 };
        let a2 = TestAgent { id: 2, caps: 0b100, trust: 50 };
        let coalition: [&dyn AgentCapabilities; 2] = [&a1, &a2];
        let ctx = DecisionContext { required_capabilities: 0b111 };
        let map = [(0usize, 0u8), (1, 1), (2, 2)];

        let run = |topology| {
            let p = policy(
                GroupAifConfig {
                    topology,
                    ..GroupAifConfig::default()
                },
                &map,
            );
            let mut out = Vec::new();
            for t in 0..4 {
                p.begin_task(&three_role_demand()).unwrap();
                out.push(p.should_join(&a0, &coalition, &ctx).score.to_bits());
                let succ = [t % 2 == 0, true, false, false, false, false, false, false];
                p.observe_outcome(&succ);
            }
            out
        };
        assert_ne!(
            run(WorldModelTopology::RoleSpecialised),
            run(WorldModelTopology::Shared),
            "R role-specialised models must not decide identically to one shared model"
        );
    }

    /// S-learn (i): the per-model update ledger is EXACT, and both a deficit and a
    /// surplus are detectable. Roles that left the roster do not accrue an expected
    /// update, so the gate is not vacuously satisfiable by skipping work.
    #[test]
    fn s_learn_counts_are_exact_and_role_scoped() {
        let p = policy(GroupAifConfig::default(), &[(0, 0), (1, 1), (2, 2)]);
        let succ = [true, true, true, false, false, false, false, false];

        // Task 1: all three roles demand.
        p.begin_task(&three_role_demand()).unwrap();
        assert_eq!(p.observe_outcome(&succ), 3);
        // Task 2: only r1 demands.
        p.begin_task(&demand(&workflow(&[(Role::new(1), &[1])])))
            .unwrap();
        assert_eq!(p.observe_outcome(&succ), 1);

        let c = p.counters();
        assert_eq!(c.tasks_observed, 2);
        assert!(c.s_learn_exact(), "every model advanced exactly as expected");
        let by_role: Vec<(ModelLabel, u64, u64)> = c
            .model_updates
            .iter()
            .map(|a| (a.label, a.updates, a.expected))
            .collect();
        assert_eq!(
            by_role,
            vec![
                (ModelLabel::Role(Role::new(0)), 1, 1),
                (ModelLabel::Role(Role::new(1)), 2, 2),
                (ModelLabel::Role(Role::new(2)), 1, 1),
            ],
            "r1 demanded twice, r0 and r2 once each"
        );

        // A width-mismatched outcome is skipped WHOLE — it can neither inflate the
        // ledger (surplus) nor be silently absorbed.
        p.begin_task(&three_role_demand()).unwrap();
        assert_eq!(p.observe_outcome(&[true; 3]), 0);
        let c = p.counters();
        assert_eq!(c.tasks_observed, 2, "a skipped task is not observed");
        assert!(c.s_learn_exact());

        // Non-vacuity guard: end-of-stream state differs from initialization.
        let fresh = policy(GroupAifConfig::default(), &[(0, 0), (1, 1), (2, 2)]);
        let moved = p
            .model_snapshots()
            .into_iter()
            .zip(fresh.model_snapshots())
            .any(|((_, after), (_, before))| {
                let (Some(a), Some(b)) = (after.pa, before.pa) else {
                    return false;
                };
                a.iter()
                    .zip(&b)
                    .any(|(x, y)| (x - y).iter().any(|d| d.abs() > 1e-9))
            });
        assert!(moved, "the world models must actually have learned");
    }

    /// The shared topology's ledger is per-task, not per-role — stated as a test so
    /// the two topologies' expected counts can never drift apart silently.
    #[test]
    fn shared_topology_updates_once_per_task() {
        let p = policy(
            GroupAifConfig {
                topology: WorldModelTopology::Shared,
                ..GroupAifConfig::default()
            },
            &[(0, 0), (1, 1), (2, 2)],
        );
        let succ = [true, true, true, false, false, false, false, false];
        p.begin_task(&three_role_demand()).unwrap();
        assert_eq!(p.observe_outcome(&succ), 1);
        p.begin_task(&demand(&workflow(&[(Role::new(1), &[1])])))
            .unwrap();
        assert_eq!(p.observe_outcome(&succ), 1);

        let c = p.counters();
        assert_eq!(c.model_updates.len(), 1);
        assert_eq!(c.model_updates[0].label, ModelLabel::Shared);
        assert!(c.s_learn_exact());
        assert_eq!(c.model_updates[0].updates, 2);
    }

    /// An `AifError` on the decision path declines and increments the upstream
    /// decline counter — never a panic, never a silent fallback. Provoked by a
    /// member whose replay window is the wrong width for its query, the one
    /// engine rejection reachable without mutating upstream state.
    #[test]
    fn upstream_error_declines_and_counts() {
        let mut member = {
            let arm = PersistentAifArm::new(3, v5_e1_base()).unwrap();
            let (query, _) = arm.role_query(0b011, 0b001, 0b011, 7, None).unwrap();
            RoleMember {
                query,
                // Deliberately the wrong width: the query has 2 modalities.
                replay: vec![vec![NO_OBS; 5]],
                modalities: 2,
                last_vote: None,
            }
        };
        let probs = <RoleMember as aif::InternalAgent>::action_probabilities(&mut member, 0);
        assert_eq!(probs.len(), 0, "a failed replay reports an empty distribution");

        // …and that empty distribution becomes a counted decline at the group.
        let mut group = aif::GroupAgent::with_slots_seeded(
            aif::CopyAgent,
            vec![member],
            aif::VotingAgent::with_seed(
                GROUP_N_ACTIONS,
                aif::VotingMode::CertaintyWeighted,
                1,
            ),
            GROUP_N_ACTIONS,
            1,
        );
        assert!(matches!(
            group.group_distribution(SENSORY_OBSERVATION),
            Err(aif::AifError::InvalidLength { expected: 2, got: 0 })
        ));

        // The read-only slot refuses to act rather than sampling something.
        let arm = PersistentAifArm::new(3, v5_e1_base()).unwrap();
        let (query, replay) = arm.role_query(0b011, 0b001, 0b011, 7, None).unwrap();
        let mut ok = RoleMember {
            query,
            replay,
            modalities: 2,
            last_vote: None,
        };
        assert!(matches!(
            <RoleMember as aif::Agent>::act(&mut ok, 0),
            Err(aif::AifError::Unsupported(_))
        ));
    }

    /// A decision before any `begin_task` declines and counts, rather than reading
    /// an empty roster into the engine.
    #[test]
    fn no_task_in_force_declines_and_counts() {
        let p = policy(GroupAifConfig::default(), &[(0, 0)]);
        let a0 = TestAgent { id: 0, caps: 0b001, trust: 50 };
        let d = p.should_join(&a0, &[], &DecisionContext::default());
        assert!(!d.act && d.score == 0.0);
        let c = p.counters();
        assert_eq!(c.declines_no_demand, 1);
        assert_eq!(c.decisions, 1);
        assert_eq!(c.reads, 0);
    }

    /// Coverage is role-matched: a worker of the wrong role does not staff a step,
    /// so joining with a role-mismatched capability must not score like the
    /// role-matched one.
    #[test]
    fn coverage_is_role_matched() {
        let ctx = DecisionContext { required_capabilities: 0b011 };
        let d2 = demand(&workflow(&[(Role::new(0), &[0]), (Role::new(1), &[1])]));

        // `matched` holds bit 1 and IS role 1; `mismatched` holds bit 1 but is role 2.
        let candidate = TestAgent { id: 0, caps: 0b010, trust: 50 };
        let anchor = TestAgent { id: 1, caps: 0b001, trust: 50 };
        let coalition: [&dyn AgentCapabilities; 1] = [&anchor];

        let run = |candidate_role: u8| {
            let p = policy(
                GroupAifConfig::default(),
                &[(0, candidate_role), (1, 0)],
            );
            p.begin_task(&d2).unwrap();
            p.should_join(&candidate, &coalition, &ctx).score.to_bits()
        };
        assert_ne!(
            run(1),
            run(2),
            "the same capability under a different role must not read the same"
        );
    }

    /// Amendment A2.1: the `grp-role-blind` reference leg is a genuinely different
    /// arm from the registered role-matched one, and the difference is exactly the
    /// cross-role capability leaking into a role's coverage masks.
    #[test]
    fn role_blind_masks_diverge_from_role_matched() {
        let ctx = DecisionContext { required_capabilities: 0b011 };
        let d2 = demand(&workflow(&[(Role::new(0), &[0]), (Role::new(1), &[1])]));

        // The candidate is role 0 and holds bit 1 — which role 1 needs and role 0
        // does not. Role-matched: it contributes nothing to r1's masks. Role-blind:
        // it "covers" r1's step.
        let candidate = TestAgent { id: 0, caps: 0b010, trust: 50 };
        let anchor = TestAgent { id: 1, caps: 0b001, trust: 50 };
        let coalition: [&dyn AgentCapabilities; 1] = [&anchor];
        let map = [(0usize, 0u8), (1, 0)];

        let run = |masks| {
            let p = policy(
                GroupAifConfig {
                    masks,
                    ..GroupAifConfig::default()
                },
                &map,
            );
            let mut out = Vec::new();
            for _ in 0..2 {
                p.begin_task(&d2).unwrap();
                let j = p.should_join(&candidate, &coalition, &ctx);
                out.push((j.act, j.score.to_bits()));
                p.observe_outcome(&[true, false, false, false, false, false, false, false]);
            }
            out
        };
        assert_ne!(
            run(CoverageMasks::RoleBlind),
            run(CoverageMasks::RoleMatched),
            "role-blind masks must be a different arm, not a relabelling"
        );

        // …and the registered default is the role-matched one.
        assert_eq!(GroupAifConfig::default().masks, CoverageMasks::RoleMatched);
    }

    /// E-agree: one sample per successful deterministic read, carrying the
    /// realised roster, the internals' own votes, and the mixture margin.
    #[test]
    fn agreement_samples_track_every_read() {
        let a0 = TestAgent { id: 0, caps: 0b001, trust: 50 };
        let a1 = TestAgent { id: 1, caps: 0b010, trust: 50 };
        let a2 = TestAgent { id: 2, caps: 0b100, trust: 50 };
        let coalition: [&dyn AgentCapabilities; 2] = [&a1, &a2];
        let ctx = DecisionContext { required_capabilities: 0b111 };

        let p = policy(GroupAifConfig::default(), &[(0, 0), (1, 1), (2, 2)]);
        p.begin_task(&three_role_demand()).unwrap();
        let d = p.should_join(&a0, &coalition, &ctx);

        let c = p.counters();
        assert_eq!(c.agreement.len(), 1, "one sample per successful read");
        assert_eq!(c.agreement.len() as u64, c.reads);
        let s = c.agreement[0];
        assert_eq!(s.roster, 3, "all three roles demanded");
        assert!(s.votes_for_act <= s.roster);
        assert!(
            (s.margin - d.score.abs()).abs() < 1e-15,
            "the sample's margin is |p(act) − 0.5|"
        );
        assert_eq!(s.unanimous(), s.votes_for_act == 0 || s.votes_for_act == 3);

        // A single-role task shrinks the roster, and the sample says so.
        p.begin_task(&demand(&workflow(&[(Role::new(1), &[1])])))
            .unwrap();
        let _ = p.should_join(&a0, &coalition, &ctx);
        assert_eq!(p.counters().agreement[1].roster, 1);
    }

    /// `E-seed`: the seeded-sampling cell runs (upstream never calls a member's
    /// `act` under `CertaintyWeighted`), stays deterministic under its seed, and
    /// reports the draw rather than a fabricated margin.
    #[test]
    fn seeded_sampling_cell_runs_and_reports_the_draw_not_a_margin() {
        let a0 = TestAgent { id: 0, caps: 0b001, trust: 50 };
        let a1 = TestAgent { id: 1, caps: 0b010, trust: 50 };
        let a2 = TestAgent { id: 2, caps: 0b100, trust: 50 };
        let coalition: [&dyn AgentCapabilities; 2] = [&a1, &a2];
        let ctx = DecisionContext { required_capabilities: 0b111 };
        let map = [(0usize, 0u8), (1, 1), (2, 2)];
        let cfg = GroupAifConfig {
            read: DecisionRead::SeededSampling,
            ..GroupAifConfig::default()
        };

        let run = || {
            let p = policy(cfg, &map);
            let mut out = Vec::new();
            for _ in 0..3 {
                p.begin_task(&three_role_demand()).unwrap();
                let d = p.should_join(&a0, &coalition, &ctx);
                out.push((d.act, d.score.to_bits()));
                p.observe_outcome(&[true, false, true, false, false, false, false, false]);
            }
            (out, p.counters())
        };
        let (a, ca) = run();
        let (b, _) = run();
        assert_eq!(a, b, "the seeded draw must still be a function of the seed");
        for (act, bits) in &a {
            let expected = if *act { 0.5f64 } else { -0.5f64 };
            assert_eq!(*bits, expected.to_bits(), "score encodes the draw, not a margin");
        }
        assert_eq!(ca.declines_upstream, 0, "the sampled path must not error");
        assert!(
            ca.agreement.is_empty(),
            "the sampling cell exposes no mixture, so it records no E-agree sample"
        );
        assert_eq!(ca.reads, 3);
    }

    /// Object-safety: the arm is usable behind the decision trait object, like
    /// every other koalisi policy.
    #[test]
    fn object_safe() {
        let p = policy(GroupAifConfig::default(), &[(0, 0)]);
        let _: Box<dyn CoalitionDecisionPolicy> = Box::new(p);
    }
}
