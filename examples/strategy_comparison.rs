//! A/B comparison of coalition-decision strategies.
//!
//! Run with:
//! ```sh
//! cargo run --release --features decision,magnitude --example strategy_comparison
//! ```
//!
//! **Why `--release`:** the latency criterion (Part 2) is only meaningful on
//! optimized builds. (Historical note: `catgraph-magnitude v0.1.0` also had a
//! debug-only over-strict triangle-inequality `debug_assert` that panicked on
//! the battery's non-dyadic couplings — catgraph #29, fixed in `v0.1.1`;
//! koalisi pins `v0.2.0` since #14, and debug builds run clean.)
//!
//! # Part 1 — single-scenario divergence demo (Threshold vs AIF)
//!
//! Evaluates the *same* join decision under:
//!   * `ThresholdPolicy<SynergisticCalculator>` — marginal-value heuristic
//!   * `AifDecisionPolicy` — Active Inference expected-free-energy bridge
//!
//! and prints a side-by-side table. The scenario is chosen so the two policies
//! DIVERGE: the existing coalition already fully covers `required_capabilities`,
//! so the AIF margin is ~0 (no information gain ⇒ don't join), while the
//! Synergistic calculator still sees positive marginal value from the extra
//! member (more size/capability/trust ⇒ join). This part is unchanged.
//!
//! # Part 2 — pre-registered A/B harness (koalisi #7)
//!
//! Benchmarks the Active Inference expected-free-energy arm (`AifDecisionPolicy`,
//! feature `decision`) against the categorical-magnitude arm (`MagnitudePolicy`,
//! feature `magnitude`) over a seeded scenario battery, then prints a markdown
//! report to stdout. The protocol is **pre-registered** (koalisi #7 comment
//! 2026-07-02): the harness reports whatever the data says — falsification of the
//! magnitude arm is a legitimate outcome and nothing is tuned to flip it. The
//! report prints BOTH the original (v1) and amended (v2 — #7 amendment
//! 2026-07-02) verdicts for cross-run comparability.
//!
//! ## Protocol (fixed)
//!
//! - **30 seeded instances** (seeds `0..30`). All randomness comes from an inline
//!   **`SplitMix64`** PRNG seeded with the scenario seed — no `rand` dependency, so
//!   results are version-stable and reproducible.
//! - **Agent pool** of `n = 4 + (next() % 13)` agents (`n ∈ [4, 16]`); each agent
//!   draws a capability mask of `k ∈ [1, 4]` distinct bits from an 8-bit universe
//!   (bits `0..8`) and trust `20 + (next() % 80)`. Agent ids are `0..n`.
//! - **Task stream** of `T = 20` tasks; each task requires `r ∈ [1, 5]` distinct
//!   bits from the universe.
//! - **Decision stream per task** (identical inputs for both arms; only the
//!   coalition state legitimately diverges with the arms' decisions):
//!   1. Arrival order = a seeded Fisher–Yates permutation of the pool, drawn ONCE
//!      per task before the arms run.
//!   2. **Bootstrap**: the FIRST arrival joins unconditionally (protocol decision
//!      — AIF cannot self-start from an empty coalition since its margin from
//!      empty is 0; the seed is identical for both arms so it biases neither).
//!   3. Each subsequent arrival calls `should_join` (sync); join iff `act`.
//!   4. One leave sweep in arrival order over the final membership calls
//!      `should_leave`; remove iff `act` (membership updates as the sweep
//!      proceeds). Every removal counts toward churn — the seeded member too.
//!
//! ## Protocol decisions (where the pre-registration left an implementation
//! choice open, these are the fixed instantiations)
//!
//! - **Distinct-bit draw**: rejection sampling — repeatedly draw `next() % 8` and
//!   accept the bit if not already chosen, until `k` distinct bits are held.
//! - **Fisher–Yates**: the standard inside-out-free variant — for `i` from
//!   `n-1` down to `1`, swap element `i` with element `next() % (i + 1)`.
//! - **PRNG draw schedule per instance**: all agents first (mask then trust per
//!   agent), then per task its `required` mask, then its arrival order. Fixed and
//!   arm-independent so both arms see byte-identical instances.
//!
//! ## Metrics, aggregation, and the t-sweep are documented at their call sites.

// This is a numeric statistics harness: `usize`/`u64` counts become `f64` rates
// and percentiles, and `f64` ranks index sorted slices. Those casts are
// intentional and lossless in the value ranges here (counts ≤ a few thousand,
// bit widths ≤ 8). We opt out of the pedantic cast/naming lints for the example
// rather than sprinkle per-expression allows; the repo gate (default clippy
// `-D warnings`) stays clean regardless.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::similar_names
)]

use std::collections::HashSet;
use std::time::{Duration, Instant};

use catgraph_magnitude::CatgraphError;
use koalisi::algorithms::{
    AgentCapabilities, CoalitionStructure, FeedbackCalculator, FeedbackStore, PopulationConfig,
    SynergisticCalculator, ValueCalculator, search,
};
use koalisi::decision::{
    AifDecisionPolicy, AifMmDecisionPolicy, CoalitionDecisionPolicy, CouplingModel, Decision,
    DecisionContext, MagnitudePolicy, PersistentAifArm, PersistentAifConfig, ThresholdPolicy,
    TrialBoundary,
};
// Part 7 (koalisi #69) EQ3 instrumentation surface + the upstream certificate /
// factorization enums it reports. Feature-gated exactly like the `mag-eq3` arm.
#[cfg(feature = "magnitude-fast")]
use catgraph_magnitude::ZeroDiversityProof;
#[cfg(feature = "magnitude-fast")]
use catgraph_magnitude::magnitude_f64::FactorizationPath;
#[cfg(feature = "magnitude-fast")]
use koalisi::decision::{JoinProbe, probe_fresh_factorization};

/// Hardcoded report date — deterministic, never read from the clock. Bumped
/// manually per committed run (2026-07-02 = K4 initial + K1 backend parity;
/// 2026-07-03 = K6 post-optimization re-run, koalisi #14).
const REPORT_DATE: &str = "2026-07-03";
/// Report date for the Part 3 feedback-arm battery (koalisi #46), separate from
/// the frozen Part 2 date above so a re-run stamps its own committed run.
const FEEDBACK_REPORT_DATE: &str = "2026-07-16";
/// Number of seeded instances (seeds `0..SEEDS`).
const SEEDS: u64 = 30;
/// Tasks per instance.
const TASKS: usize = 20;
/// Capability-universe width (bits `0..UNIVERSE_BITS`).
const UNIVERSE_BITS: u64 = 8;
/// Capability-universe width of the Part 5c widened slice (koalisi #61, #61
/// Part 5c — exploratory). Everything registered before Part 5c runs at
/// [`UNIVERSE_BITS`].
const W12_UNIVERSE_BITS: u64 = 12;
/// Instances with `n <= ORACLE_MAX_N` are oracle-eligible (brute force ≤ 255
/// non-empty subsets).
const ORACLE_MAX_N: usize = 8;

// --- Part 3 (feedback arm, koalisi #46) reliability-structure constants ------
/// Scope B: probability an agent is *reliable* (bimodal hidden reliability).
const RELIABLE_PROB: f64 = 0.7;
/// Scope B: per-task failure probability of a *reliable* agent.
const RHO_RELIABLE: f64 = 0.05;
/// Scope B: per-task failure probability of a *flaky* agent.
const RHO_FLAKY: f64 = 0.40;
/// Part 3 confirmatory: `fb` must beat `thr` on at least this many of `SEEDS`
/// seeds for H2 (≥ 18/30, the 60% consistency bar inherited from K4-v2/v3).
const FB_SUPERIOR_MIN: usize = 18;

// --- Part 4 (selective-base feedback arm, koalisi #48) constants -------------
/// Part 4 confirmatory `join_threshold` — a *selective* base (positive threshold
/// keeps the coalition small, unlike the falsified #46 `join = 0` full-join base).
const JOIN_THRESHOLD_SELECTIVE: f64 = 100.0;
/// Part 4 exploratory E1 `join_threshold` grid (Scope B, non-gating).
const SELECTIVE_THRESHOLD_GRID: [f64; 5] = [50.0, 75.0, 100.0, 125.0, 150.0];
/// Part 4 selective-base feedback weights: failure-only (`hw = 0, fw = 1`);
/// absorbs the #49 failure-weighted point.
const HW_SELECTIVE: f64 = 0.0;
const FW_SELECTIVE: f64 = 1.0;
/// Report date for the Part 4 selective-base battery (koalisi #48), separate from
/// the frozen Part 2/3 dates so a re-run stamps its own committed run.
const SELECTIVE_REPORT_DATE: &str = "2026-07-17";

/// Which battery an instance run belongs to.
///
/// - [`Scope::A`] — the i.i.d. null control: fitness = `completed` (0/1); the
///   PRIMARY is `completion_rate × mean_cov_eff`, exactly the committed Part 2
///   metric (the regression gate reproduces `mag` seed-for-seed here).
/// - [`Scope::B`] — the reliability-structured contest: a task succeeds iff it
///   is `completed` **and** every final member performed (per-`(task, agent)`
///   `perf` matrix); `PRIMARY_B` = `success_rate × mean_cov_eff`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    A,
    B,
}

/// Concrete agent for the demo.
#[derive(Debug, Clone, Copy)]
struct Worker {
    id: usize,
    caps: u32,
    trust: u32,
}

impl AgentCapabilities for Worker {
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

fn main() {
    part1_divergence_demo();
    println!();
    println!("{}", "=".repeat(72));
    println!();
    part2_ab_harness();
    println!();
    println!("{}", "=".repeat(72));
    println!();
    part3_feedback_arm();
    println!();
    println!("{}", "=".repeat(72));
    println!();
    part4_selective_feedback();
    println!();
    println!("{}", "=".repeat(72));
    println!();
    part3b_scalar_aif_scope_b_baseline();
    println!();
    println!("{}", "=".repeat(72));
    println!();
    part4c_persistent_aif();
    println!();
    println!("{}", "=".repeat(72));
    println!();
    part4d_e1_persistent_aif();
    println!();
    println!("{}", "=".repeat(72));
    println!();
    part4e_arm_choice_addendum();
    println!();
    println!("{}", "=".repeat(72));
    println!();
    part4f_churn_frontier();
    println!();
    println!("{}", "=".repeat(72));
    println!();
    part4g_reliability_filtered_mag();
    println!();
    println!("{}", "=".repeat(72));
    println!();
    part4h_v6_never_evict();
    println!();
    println!("{}", "=".repeat(72));
    println!();
    part5a_battery_v2();
    println!();
    println!("{}", "=".repeat(72));
    println!();
    part5b_reliability_routing();
    println!();
    println!("{}", "=".repeat(72));
    println!();
    part5c_addendum();
    println!();
    println!("{}", "=".repeat(72));
    println!();
    part6_corrected_routing();
    println!();
    println!("{}", "=".repeat(72));
    println!();
    part7_eq3_latency_rematch();
}

// ===========================================================================
// Part 1 — the original single-scenario divergence demo (behavior unchanged).
// ===========================================================================

fn row(name: &str, d: Decision) {
    let act = d.act;
    let score = d.score;
    println!("{name:<28} | act = {act:<5} | score = {score:>10.6}");
}

fn part1_divergence_demo() {
    // Task requires capabilities {bit0, bit1}.
    let required_capabilities = 0b011;
    let ctx = DecisionContext {
        required_capabilities,
    };

    // Existing coalition ALREADY fully covers the required capabilities
    // (a1 covers bit0, a2 covers bit1 ⇒ union 0b011 == required).
    let a1 = Worker {
        id: 1,
        caps: 0b001,
        trust: 80,
    };
    let a2 = Worker {
        id: 2,
        caps: 0b010,
        trust: 80,
    };
    let coalition: [&dyn AgentCapabilities; 2] = [&a1, &a2];

    // Candidate ALSO covers all required capabilities on its own (bits 0+1) plus
    // an extra unrequired bit2. Crucially it adds NO new required-capability
    // coverage to the coalition: coverage is 1.0 whether or not it joins, so the
    // AIF expected-free-energy margin is ≈ 0 (don't join). The Synergistic
    // calculator, however, still rewards the extra member's size/capability/
    // trust (positive marginal value ⇒ join). This is the divergence.
    let candidate = Worker {
        id: 0,
        caps: 0b111,
        trust: 80,
    };

    // Strategy A: marginal-value threshold over the Synergistic calculator.
    // join_threshold 0.0 ⇒ join on any positive marginal value.
    let threshold = ThresholdPolicy::new(SynergisticCalculator, 0.0, 0.0);
    // Strategy B: Active Inference expected free energy.
    let aif = AifDecisionPolicy::default();

    let d_threshold = threshold.should_join(&candidate, &coalition, &ctx);
    let d_aif = aif.should_join(&candidate, &coalition, &ctx);

    let candidate_caps = candidate.caps;
    let union = a1.caps | a2.caps;
    println!(
        "Scenario: candidate caps=0b{candidate_caps:03b}, coalition union=0b{union:03b}, required=0b{required_capabilities:03b}"
    );
    println!(
        "(Coalition already covers all required capabilities; candidate adds only an unrequired bit ⇒ no coverage gain.)\n"
    );

    let header = format!("{:<28} | {:<11} | {}", "policy", "decision", "score");
    println!("{header}");
    let separator = "-".repeat(58);
    println!("{separator}");
    row("ThresholdPolicy(Synergistic)", d_threshold);
    row("AifDecisionPolicy (EFE)", d_aif);
    println!();

    if d_threshold.act == d_aif.act {
        println!("No divergence on this scenario (both policies agree).");
    } else {
        let threshold_verdict = if d_threshold.act {
            "JOIN"
        } else {
            "DON'T JOIN"
        };
        let aif_verdict = if d_aif.act { "JOIN" } else { "DON'T JOIN" };
        println!("DIVERGENCE: Threshold says {threshold_verdict} but AIF says {aif_verdict}.");
        println!(
            "  The Synergistic calculator rewards the extra member's raw size/capability/trust"
        );
        println!("  (positive marginal value => JOIN), while the AIF bridge sees no gain in");
        println!("  required-capability coverage (G unchanged => margin ~ 0 => DON'T JOIN).");
    }
}

// ===========================================================================
// Part 2 — the pre-registered A/B harness.
// ===========================================================================

/// `SplitMix64` — the reference constant-schedule PRNG. Seeded with the scenario
/// seed; every draw advances the state by the golden-ratio gamma and mixes.
struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// Rejection-sample `k` distinct bits from the low `UNIVERSE_BITS` bits (see the
/// module doc's "Protocol decisions").
fn draw_distinct_bits(rng: &mut SplitMix64, k: u64) -> u32 {
    draw_distinct_bits_in(rng, k, UNIVERSE_BITS)
}

/// [`draw_distinct_bits`] over an arbitrary universe width (koalisi #61 Part 5c,
/// the 12-bit widened slice). The draw schedule is the frozen one — one
/// `next_u64` per rejection round, `% universe` — so at `universe =
/// UNIVERSE_BITS` this IS the frozen helper, byte-for-byte.
fn draw_distinct_bits_in(rng: &mut SplitMix64, k: u64, universe: u64) -> u32 {
    let mut mask = 0u32;
    let mut count = 0u64;
    while count < k {
        let bit = (rng.next_u64() % universe) as u32;
        let b = 1u32 << bit;
        if mask & b == 0 {
            mask |= b;
            count += 1;
        }
    }
    mask
}

/// Standard Fisher–Yates shuffle of `0..n` (see the module doc's "Protocol
/// decisions").
fn fisher_yates(rng: &mut SplitMix64, n: usize) -> Vec<usize> {
    let mut order: Vec<usize> = (0..n).collect();
    for i in (1..n).rev() {
        let j = (rng.next_u64() % (i as u64 + 1)) as usize;
        order.swap(i, j);
    }
    order
}

/// A single task: which capabilities it needs and the (fixed) arrival order.
struct Task {
    required: u32,
    order: Vec<usize>,
}

/// Draw the shared instance *prefix* — the agent pool then the task stream —
/// off `rng`. This is the exact draw sequence both scopes share; Scope B
/// continues drawing its reliability structure off the SAME `rng` afterwards, so
/// the prefix stays byte-identical to Scope A (and to the committed Part 2
/// battery). Do not reorder draws here.
fn draw_prefix(rng: &mut SplitMix64) -> (Vec<Worker>, Vec<Task>) {
    let n = (4 + rng.next_u64() % 13) as usize;
    let agents: Vec<Worker> = (0..n)
        .map(|id| {
            let k = 1 + rng.next_u64() % 4;
            let caps = draw_distinct_bits(rng, k);
            let trust = (20 + rng.next_u64() % 80) as u32;
            Worker { id, caps, trust }
        })
        .collect();

    let tasks: Vec<Task> = (0..TASKS)
        .map(|_| {
            let r = 1 + rng.next_u64() % 5;
            let required = draw_distinct_bits(rng, r);
            let order = fisher_yates(rng, n);
            Task { required, order }
        })
        .collect();

    (agents, tasks)
}

/// Which task-draw regime an instance is generated under (koalisi #61, EQ1).
///
/// The regimes differ ONLY in the per-task `|required|` draw; the pool draw
/// (`n ∈ 4..=16`, caps `k ∈ 1..=4`) and the arrival-order shuffle are identical.
/// Everything registered before battery v2 runs under [`Regime::V1`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Regime {
    /// The v1 draw: `|required| ∈ 1..=5` (Parts 2–4h, frozen).
    V1,
    /// The battery-v2 draw: `|required| ∈ 2..=8` — the de-saturated regime the
    /// `docs/prereg-K4-battery-v2.md` registration fixes.
    V2,
    /// The Part 5c widened draw (EXPLORATORY): a **12-bit** universe,
    /// `|required| ∈ 2..=12`, worker caps `1..=6`. Requires an arm at
    /// `n_bits = 12`; the 8-bit regimes are untouched.
    W12,
}

/// Capability-universe width of a regime, in bits. Also the outcome-slice width
/// a `PersistentAifArm` must be configured for to run that regime.
fn regime_universe(regime: Regime) -> usize {
    match regime {
        Regime::V1 | Regime::V2 => UNIVERSE_BITS as usize,
        Regime::W12 => W12_UNIVERSE_BITS as usize,
    }
}

/// The battery-v2 instance prefix (koalisi #61, EQ1): identical to
/// [`draw_prefix`] in shape and draw ORDER, but each task requires `r ∈ [2, 8]`
/// distinct bits instead of `r ∈ [1, 5]`. Deliberately a separate function
/// rather than a parameterization of [`draw_prefix`] — the v1 draw is frozen and
/// must stay literally untouched for the byte-identity gate.
///
/// The streams diverge from the first task draw onward (`draw_distinct_bits`
/// consumes a variable number of `next_u64`s), so v2 instances are genuinely
/// new instances, not re-labelled v1 ones.
fn draw_prefix_v2(rng: &mut SplitMix64) -> (Vec<Worker>, Vec<Task>) {
    let n = (4 + rng.next_u64() % 13) as usize;
    let agents: Vec<Worker> = (0..n)
        .map(|id| {
            let k = 1 + rng.next_u64() % 4;
            let caps = draw_distinct_bits(rng, k);
            let trust = (20 + rng.next_u64() % 80) as u32;
            Worker { id, caps, trust }
        })
        .collect();

    let tasks: Vec<Task> = (0..TASKS)
        .map(|_| {
            let r = 2 + rng.next_u64() % 7;
            let required = draw_distinct_bits(rng, r);
            let order = fisher_yates(rng, n);
            Task { required, order }
        })
        .collect();

    (agents, tasks)
}

/// The Part 5c widened instance prefix (koalisi #61, EXPLORATORY): the v2 shape
/// lifted onto a **12-bit** universe — pool `n ∈ 4..=16` unchanged, worker caps
/// `k ∈ 1..=6` distinct bits, `|required| ∈ 2..=12`. A separate function for the
/// same reason [`draw_prefix_v2`] is one: the registered draws are frozen.
fn draw_prefix_w12(rng: &mut SplitMix64) -> (Vec<Worker>, Vec<Task>) {
    let n = (4 + rng.next_u64() % 13) as usize;
    let agents: Vec<Worker> = (0..n)
        .map(|id| {
            let k = 1 + rng.next_u64() % 6;
            let caps = draw_distinct_bits_in(rng, k, W12_UNIVERSE_BITS);
            let trust = (20 + rng.next_u64() % 80) as u32;
            Worker { id, caps, trust }
        })
        .collect();

    let tasks: Vec<Task> = (0..TASKS)
        .map(|_| {
            let r = 2 + rng.next_u64() % 11;
            let required = draw_distinct_bits_in(rng, r, W12_UNIVERSE_BITS);
            let order = fisher_yates(rng, n);
            Task { required, order }
        })
        .collect();

    (agents, tasks)
}

/// Generate one seeded instance: the agent pool and the task stream. Called by
/// BOTH the arm runners and the oracle, guaranteeing byte-identical instances.
fn generate_instance(seed: u64) -> (Vec<Worker>, Vec<Task>) {
    let mut rng = SplitMix64::new(seed);
    draw_prefix(&mut rng)
}

/// Uniform `f64` in `[0, 1)` from the top 53 bits of one `next_u64` draw (the
/// standard IEEE-754 double construction). Part 3 uses this for the reliability
/// draws; the integer `% N` draws in `draw_prefix` are untouched.
fn next_unit(rng: &mut SplitMix64) -> f64 {
    (rng.next_u64() >> 11) as f64 / (1u64 << 53) as f64
}

/// Generate one Scope-B instance: the byte-identical Scope-A prefix, then the
/// reliability structure drawn off the SAME stream (so the prefix is unchanged).
///
/// Draw order after the prefix (fixed, arm-independent):
/// 1. per-agent hidden reliability `ρ_i` — one `next_unit` per agent: reliable
///    (`RHO_RELIABLE`) with prob `RELIABLE_PROB`, else flaky (`RHO_FLAKY`);
/// 2. the `perf[t][i]` matrix — `t` outer, `i` inner — one `next_unit` each:
///    `perf[t][i] = (next_unit < 1 − ρ_i)` (the agent "performs" on that task).
///
/// `perf` is pre-drawn once per instance and identical for every arm — the
/// prereg's core invariant that arms only differ in their value model.
fn generate_instance_b(seed: u64) -> (Vec<Worker>, Vec<Task>, Vec<f64>, Vec<Vec<bool>>) {
    generate_instance_b_regime(seed, Regime::V1)
}

/// [`generate_instance_b`] under a chosen [`Regime`] (koalisi #61, EQ1). The
/// post-prefix draw order (`ρ` per agent, then the `perf[t][i]` matrix) is the
/// same in both regimes; only the prefix draw differs, so `Regime::V1` is
/// bit-identical to the frozen generator by construction.
fn generate_instance_b_regime(
    seed: u64,
    regime: Regime,
) -> (Vec<Worker>, Vec<Task>, Vec<f64>, Vec<Vec<bool>>) {
    let mut rng = SplitMix64::new(seed);
    let (agents, tasks) = match regime {
        Regime::V1 => draw_prefix(&mut rng),
        Regime::V2 => draw_prefix_v2(&mut rng),
        Regime::W12 => draw_prefix_w12(&mut rng),
    };
    let n = agents.len();

    let rho: Vec<f64> = (0..n)
        .map(|_| {
            if next_unit(&mut rng) < RELIABLE_PROB {
                RHO_RELIABLE
            } else {
                RHO_FLAKY
            }
        })
        .collect();

    let perf: Vec<Vec<bool>> = (0..TASKS)
        .map(|_| (0..n).map(|i| next_unit(&mut rng) < 1.0 - rho[i]).collect())
        .collect();

    (agents, tasks, rho, perf)
}

/// Per-seed result for one arm.
struct InstanceMetrics {
    seed: u64,
    n: usize,
    /// PRIMARY: `success_rate × mean_cov_eff` (stream-level product). Under
    /// [`Scope::A`], `success ≡ completed`, so this is exactly the committed
    /// Part 2 metric; under [`Scope::B`], `success` additionally requires every
    /// final member to have performed.
    primary: f64,
    /// Total leave-sweep removals over the stream.
    churn: usize,
    /// Fraction of tasks that succeeded (Scope A: completed; Scope B: completed
    /// AND all final members performed). Record-only; for Scope A it equals the
    /// completion rate.
    success_rate: f64,
}

fn seconds_to_us(d: Duration) -> f64 {
    d.as_secs_f64() * 1.0e6
}

/// Build a `&dyn` coalition view from member indices into `agents`.
fn coalition_view<'a>(agents: &'a [Worker], members: &[usize]) -> Vec<&'a dyn AgentCapabilities> {
    members
        .iter()
        .map(|&i| &agents[i] as &dyn AgentCapabilities)
        .collect()
}

/// Run one arm over one seeded instance, timing every sync `should_join` /
/// `should_leave` call into `latencies` (µs).
///
/// Metrics (per the pre-registered protocol):
/// - `completed(task)` = union of members' caps covers `required` fully.
/// - `coverage_eff(task)` = (covered required bits / required bits) / member
///   count, `0.0` if empty.
/// - `success(task)` = `completed` (Scope A) or `completed AND ∀ i∈members:
///   perf[t][i]` (Scope B, the reliability contest).
/// - PRIMARY = `success_rate × mean_cov_eff` (stream-level product).
///
/// `store` (the feedback arm's counter table) is written **once per task, after
/// the leave sweep**, with the final coalition's `success` (0/1) as the fitness —
/// so every within-task join/leave decision saw a *constant* store. Member
/// indices are their `agent_id`s (`Worker.id == index`), so they are recorded
/// directly. Passing `None` (the `mag`/`thr`/Part-2 arms) records nothing and
/// reproduces the committed behaviour exactly.
fn run_instance(
    policy: &dyn CoalitionDecisionPolicy,
    seed: u64,
    scope: Scope,
    store: Option<&FeedbackStore>,
    latencies: &mut Vec<f64>,
) -> InstanceMetrics {
    // Scope A draws only the shared prefix; Scope B additionally draws the
    // arm-independent `perf` matrix. Both share a byte-identical prefix.
    let (agents, tasks, perf) = match scope {
        Scope::A => {
            let (agents, tasks) = generate_instance(seed);
            (agents, tasks, Vec::new())
        }
        Scope::B => {
            let (agents, tasks, _rho, perf) = generate_instance_b(seed);
            (agents, tasks, perf)
        }
    };
    let n = agents.len();

    let mut success_count = 0usize;
    let mut cov_eff_sum = 0.0f64;
    let mut churn = 0usize;

    for (t, task) in tasks.iter().enumerate() {
        let ctx = DecisionContext {
            required_capabilities: task.required,
        };

        // Bootstrap: the first arrival joins unconditionally.
        let mut members: Vec<usize> = vec![task.order[0]];

        // Subsequent arrivals consult the policy (sync path).
        for &idx in &task.order[1..] {
            let candidate: &dyn AgentCapabilities = &agents[idx];
            let coalition = coalition_view(&agents, &members);
            let t0 = Instant::now();
            let d = policy.should_join(candidate, &coalition, &ctx);
            latencies.push(seconds_to_us(t0.elapsed()));
            if d.act {
                members.push(idx);
            }
        }

        // One leave sweep in arrival order over the final membership.
        for &idx in &task.order {
            let Some(pos) = members.iter().position(|&m| m == idx) else {
                continue;
            };
            let coalition = coalition_view(&agents, &members);
            let agent: &dyn AgentCapabilities = &agents[idx];
            let t0 = Instant::now();
            let d = policy.should_leave(agent, &coalition, &ctx);
            latencies.push(seconds_to_us(t0.elapsed()));
            if d.act {
                members.remove(pos);
                churn += 1;
            }
        }

        // Task metrics on the formed (final) coalition.
        let union = members.iter().fold(0u32, |acc, &i| acc | agents[i].caps);
        let covered = (union & task.required).count_ones();
        let completed = covered == task.required.count_ones();

        // Scope B: a covered task still fails unless every final member performed
        // on this task (the reliability signal `mag`/EFE are blind to). Scope A:
        // success ≡ completed.
        let success = match scope {
            Scope::A => completed,
            Scope::B => completed && members.iter().all(|&i| perf[t][i]),
        };
        if success {
            success_count += 1;
        }

        let cov_eff = if members.is_empty() {
            0.0
        } else {
            (f64::from(covered) / f64::from(task.required.count_ones())) / members.len() as f64
        };
        cov_eff_sum += cov_eff;

        // Feedback write-back: once per task, AFTER the leave sweep, so all of
        // this task's decisions saw a constant store. `FeedbackStore::new(1.0)`
        // ⇒ any non-success (fitness < 1.0) is a failure.
        if let Some(store) = store {
            let fitness = if success { 1.0 } else { 0.0 };
            store.record_outcome(&members, fitness);
        }
    }

    let success_rate = success_count as f64 / tasks.len() as f64;
    let mean_cov_eff = cov_eff_sum / tasks.len() as f64;
    InstanceMetrics {
        seed,
        n,
        primary: success_rate * mean_cov_eff,
        churn,
        success_rate,
    }
}

/// Run the full 30-seed battery for one arm, with a discarded seed-0 warm-up
/// first (so the measured latencies see warm caches and a warm allocator).
/// Returns the per-seed metrics and every measured per-decision latency (µs).
fn run_battery(policy: &dyn CoalitionDecisionPolicy) -> (Vec<InstanceMetrics>, Vec<f64>) {
    // Warm-up: full seed-0 instance, latencies discarded.
    let mut warm = Vec::new();
    let _ = run_instance(policy, 0, Scope::A, None, &mut warm);

    let mut latencies = Vec::new();
    let per_seed: Vec<InstanceMetrics> = (0..SEEDS)
        .map(|s| run_instance(policy, s, Scope::A, None, &mut latencies))
        .collect();
    (per_seed, latencies)
}

// ---------------------------------------------------------------------------
// Oracle (only for instances with n <= 8): per task, brute-force all non-empty
// member subsets and pick the lexicographic max of (completed, coverage_eff).
// ---------------------------------------------------------------------------

/// Best `(completed, coverage_eff)` over all non-empty subsets of `agents` for a
/// task requiring `required`. Lexicographic: prefer completing, then higher
/// coverage efficiency (which rewards the smallest completing subset).
fn best_subset(agents: &[Worker], required: u32) -> (bool, f64) {
    let n = agents.len();
    let mut best: (bool, f64) = (false, f64::NEG_INFINITY);
    let req_bits = f64::from(required.count_ones());
    for bits in 1u32..(1u32 << n) {
        let mut union = 0u32;
        let mut count = 0usize;
        for (i, agent) in agents.iter().enumerate() {
            if bits & (1u32 << i) != 0 {
                union |= agent.caps;
                count += 1;
            }
        }
        let covered = (union & required).count_ones();
        let completed = covered == required.count_ones();
        let cov_eff = (f64::from(covered) / req_bits) / count as f64;
        let cand = (completed, cov_eff);
        let better = match (cand.0, best.0) {
            (true, false) => true,
            (false, true) => false,
            _ => cand.1 > best.1,
        };
        if better {
            best = cand;
        }
    }
    best
}

/// Oracle PRIMARY for a seed via the same stream-level formula.
fn oracle_primary(agents: &[Worker], tasks: &[Task]) -> f64 {
    let mut completed_count = 0usize;
    let mut cov_eff_sum = 0.0f64;
    for task in tasks {
        let (completed, cov_eff) = best_subset(agents, task.required);
        if completed {
            completed_count += 1;
        }
        cov_eff_sum += cov_eff;
    }
    let completion_rate = completed_count as f64 / tasks.len() as f64;
    let mean_cov_eff = cov_eff_sum / tasks.len() as f64;
    completion_rate * mean_cov_eff
}

// ---------------------------------------------------------------------------
// Exploratory t-sweep policy — lives IN THE EXAMPLE, never in the library.
//
// Replicates `MagnitudePolicy`'s decision logic exactly, but scores via
// `coalition_magnitude_from_couplings` at an arbitrary scale `t` (the library
// arm is pinned to t = 1, catgraph #22). Exploratory only: no latency, no
// verdict. t = 1.0 sanity-checks that this reproduces the stable arm.
// ---------------------------------------------------------------------------

/// Deduplicate `agents` by `agent_id` (first wins) and drop task-irrelevant
/// agents (`caps & required == 0`), returning survivors' capability masks. Exact
/// mirror of the library's `relevant_masks`.
fn relevant_masks(agents: &[&dyn AgentCapabilities], required: u32) -> Vec<u32> {
    let mut seen = HashSet::new();
    let mut masks = Vec::with_capacity(agents.len());
    for a in agents {
        let caps = a.capabilities();
        if caps & required != 0 && seen.insert(a.agent_id()) {
            masks.push(caps);
        }
    }
    masks
}

/// Coalition magnitude of `masks` at scale `t` under the substitutability
/// coupling; `Ok(0.0)` when empty (upstream errors on an empty member set).
fn magnitude_at_t(masks: &[u32], required: u32, t: f64) -> Result<f64, CatgraphError> {
    if masks.is_empty() {
        return Ok(0.0);
    }
    let agents: Vec<usize> = (0..masks.len()).collect();
    let mut couplings: Vec<(usize, usize, f64)> = Vec::new();
    for (i, &from) in masks.iter().enumerate() {
        for (j, &to) in masks.iter().enumerate() {
            if i == j {
                continue;
            }
            let p = CouplingModel::coupling(from, to, required);
            if p > 0.0 {
                couplings.push((i, j, p));
            }
        }
    }
    catgraph_magnitude::coalition_magnitude_from_couplings(&agents, &couplings, &agents, t)
}

fn join_decision(
    with: Result<f64, CatgraphError>,
    without: Result<f64, CatgraphError>,
    margin: f64,
) -> Decision {
    match (with, without) {
        (Ok(w), Ok(wo)) => {
            let m = w - wo;
            if m.is_finite() {
                Decision {
                    act: m > margin,
                    score: m,
                }
            } else {
                Decision {
                    act: false,
                    score: 0.0,
                }
            }
        }
        _ => Decision {
            act: false,
            score: 0.0,
        },
    }
}

fn leave_decision(
    mag_in: Result<f64, CatgraphError>,
    mag_out: Result<f64, CatgraphError>,
) -> Decision {
    match (mag_in, mag_out) {
        (Ok(i), Ok(o)) => {
            let d = i - o;
            if d.is_finite() {
                Decision {
                    act: d <= 0.0,
                    score: d,
                }
            } else {
                Decision {
                    act: false,
                    score: 0.0,
                }
            }
        }
        _ => Decision {
            act: false,
            score: 0.0,
        },
    }
}

/// t-parameterized magnitude policy (exploratory; example-only).
struct TSweepMagnitudePolicy {
    t: f64,
    join_margin: f64,
}

impl CoalitionDecisionPolicy for TSweepMagnitudePolicy {
    fn should_join(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        ctx: &DecisionContext,
    ) -> Decision {
        let required = ctx.required_capabilities;
        let masks_without = relevant_masks(coalition, required);
        let mut with: Vec<&dyn AgentCapabilities> = coalition.to_vec();
        with.push(agent);
        let masks_with = relevant_masks(&with, required);
        let mag_with = magnitude_at_t(&masks_with, required, self.t);
        let mag_without = magnitude_at_t(&masks_without, required, self.t);
        join_decision(mag_with, mag_without, self.join_margin)
    }

    fn should_leave(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        ctx: &DecisionContext,
    ) -> Decision {
        let required = ctx.required_capabilities;
        let masks_in = relevant_masks(coalition, required);
        let agent_id = agent.agent_id();
        let without: Vec<&dyn AgentCapabilities> = coalition
            .iter()
            .filter(|a| a.agent_id() != agent_id)
            .copied()
            .collect();
        let masks_out = relevant_masks(&without, required);
        let mag_in = magnitude_at_t(&masks_in, required, self.t);
        let mag_out = magnitude_at_t(&masks_out, required, self.t);
        leave_decision(mag_in, mag_out)
    }
}

// ---------------------------------------------------------------------------
// Statistics helpers.
// ---------------------------------------------------------------------------

/// Linear-interpolated percentile of a pre-sorted slice.
fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    if sorted.len() == 1 {
        return sorted[0];
    }
    let rank = p * (sorted.len() - 1) as f64;
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    let frac = rank - lo as f64;
    sorted[lo] + (sorted[hi] - sorted[lo]) * frac
}

/// `(median, IQR)` of `values` (consumed and sorted).
fn median_iqr(mut values: Vec<f64>) -> (f64, f64) {
    values.sort_by(f64::total_cmp);
    let med = percentile(&values, 0.5);
    let iqr = percentile(&values, 0.75) - percentile(&values, 0.25);
    (med, iqr)
}

/// Median of `values` (consumed and sorted).
fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    percentile(&values, 0.5)
}

// ---------------------------------------------------------------------------
// Harness driver + markdown report.
// ---------------------------------------------------------------------------

fn part2_ab_harness() {
    // Three measured arms (prereg K4-v3): the incumbent quality winner `mag`, the
    // shipped scalar AIF bridge `aif-scalar`, and the new multi-modality bridge
    // `aif-mm`. Instances are byte-identical across arms; only the value model
    // differs. `aif` / `mag` run first and in their original order so their
    // per-seed rows reproduce the committed baseline (the regression gate).
    let aif = AifDecisionPolicy::default();
    let mag = MagnitudePolicy::default();
    let mm = AifMmDecisionPolicy::default();
    let (aif_seeds, aif_lat) = run_battery(&aif);
    let (mag_seeds, mag_lat) = run_battery(&mag);
    let (mm_seeds, mm_lat) = run_battery(&mm);

    // Oracle (n <= 8 seeds only).
    let oracle: Vec<Option<f64>> = (0..SEEDS)
        .map(|s| {
            let (agents, tasks) = generate_instance(s);
            (agents.len() <= ORACLE_MAX_N).then(|| oracle_primary(&agents, &tasks))
        })
        .collect();

    // t-sweep (exploratory, non-gating).
    let t_values = [0.5, 1.0, 2.0, 10.0];
    let sweep: Vec<(f64, f64)> = t_values
        .iter()
        .map(|&t| {
            let policy = TSweepMagnitudePolicy {
                t,
                join_margin: 0.0,
            };
            let mut scratch = Vec::new();
            let primaries: Vec<f64> = (0..SEEDS)
                .map(|s| run_instance(&policy, s, Scope::A, None, &mut scratch).primary)
                .collect();
            (t, median(primaries))
        })
        .collect();

    print_report(
        &aif_seeds, &aif_lat, &mag_seeds, &mag_lat, &mm_seeds, &mm_lat, &oracle, &sweep,
    );
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn print_report(
    aif_seeds: &[InstanceMetrics],
    aif_lat: &[f64],
    mag_seeds: &[InstanceMetrics],
    mag_lat: &[f64],
    mm_seeds: &[InstanceMetrics],
    mm_lat: &[f64],
    oracle: &[Option<f64>],
    sweep: &[(f64, f64)],
) {
    // Aggregates.
    let aif_primaries: Vec<f64> = aif_seeds.iter().map(|m| m.primary).collect();
    let mag_primaries: Vec<f64> = mag_seeds.iter().map(|m| m.primary).collect();
    let mm_primaries: Vec<f64> = mm_seeds.iter().map(|m| m.primary).collect();
    let aif_primary_med = median(aif_primaries.clone());
    let mag_primary_med = median(mag_primaries.clone());
    let mm_primary_med = median(mm_primaries.clone());
    let (aif_p_med, aif_p_iqr) = median_iqr(aif_primaries);
    let (mag_p_med, mag_p_iqr) = median_iqr(mag_primaries);
    let (mm_p_med, mm_p_iqr) = median_iqr(mm_primaries);
    let (aif_c_med, aif_c_iqr) = median_iqr(aif_seeds.iter().map(|m| m.churn as f64).collect());
    let (mag_c_med, mag_c_iqr) = median_iqr(mag_seeds.iter().map(|m| m.churn as f64).collect());
    let mm_churn_med = median(mm_seeds.iter().map(|m| m.churn as f64).collect());
    let scalar_churn_med = median(aif_seeds.iter().map(|m| m.churn as f64).collect());
    let (mm_c_med, mm_c_iqr) = median_iqr(mm_seeds.iter().map(|m| m.churn as f64).collect());
    let (aif_l_med, aif_l_iqr) = median_iqr(aif_lat.to_vec());
    let (mag_l_med, mag_l_iqr) = median_iqr(mag_lat.to_vec());
    let (mm_l_med, mm_l_iqr) = median_iqr(mm_lat.to_vec());

    // Verdict criteria (pre-committed).
    let non_inferior_median = mag_primary_med >= 0.95 * aif_primary_med;
    let inferior_count = (0..aif_seeds.len())
        .filter(|&i| mag_seeds[i].primary < aif_seeds[i].primary)
        .count();
    let inferior_ok = inferior_count as f64 <= 0.40 * SEEDS as f64;
    let crit1 = non_inferior_median && inferior_ok;
    let crit2 = mag_l_med < aif_l_med;
    let verdict = match (crit1, crit2) {
        (true, true) => "VALIDATED",
        (false, false) => "FALSIFIED (both)",
        (false, true) => "FALSIFIED (non-inferiority)",
        (true, false) => "FALSIFIED (latency)",
    };

    // v2 amended criterion (#7 amendment, 2026-07-02): dual-path validation.
    // Path A is the v1 route (unchanged); Path B is the new quality-dominance
    // route with a bounded-latency-overhead constraint instead of a strict race.
    let path_a = crit1 && crit2;
    let b1 = mag_primary_med >= 1.25 * aif_primary_med;
    let superior_count = (0..aif_seeds.len())
        .filter(|&i| mag_seeds[i].primary > aif_seeds[i].primary)
        .count();
    let b2 = superior_count as f64 >= 0.60 * SEEDS as f64;
    let b3 = mag_l_med <= 10.0 * aif_l_med;
    let path_b = b1 && b2 && b3;
    let v2_verdict = match (path_a, path_b) {
        (true, true) => "VALIDATED (A+B)",
        (true, false) => "VALIDATED (A)",
        (false, true) => "VALIDATED (B)",
        (false, false) => "FALSIFIED",
    };

    // K4-v3 confirmatory criteria (prereg `docs/prereg-K4v3-multimodal-aif.md`).
    // `aif` is the scalar arm; `mag` the incumbent; `mm` the new multi-modality arm.
    // H1 — gap closed: magnitude is no longer clearly superior to mm.
    let h1 = mag_primary_med < 1.25 * mm_primary_med;
    // H2 — mechanism: mm clearly beats the scalar bridge (median + per-seed).
    let mm_superior_count = (0..mm_seeds.len())
        .filter(|&i| mm_seeds[i].primary > aif_seeds[i].primary)
        .count();
    let h2a = mm_primary_med >= 1.25 * aif_primary_med;
    let h2b = mm_superior_count as f64 >= 0.60 * SEEDS as f64;
    let h2 = h2a && h2b;
    // S1 — churn (secondary, reported alongside the verdict, non-gating).
    let s1 = mm_churn_med <= 0.5 * scalar_churn_med;
    let k4v3_verdict = match (h1, h2) {
        (true, true) => "VALIDATED (gap closed)",
        (_, true) => "PARTIAL (mechanism only)",
        (_, false) => "FALSIFIED (multimodality)",
    };
    // Regression gate: mm ≡ scalar per-seed indicates the registered bridge is
    // decision-equivalent to the scalar arm (both monotone in covered-bit count).
    let mm_equals_scalar = (0..mm_seeds.len()).all(|i| {
        (mm_seeds[i].primary - aif_seeds[i].primary).abs() < 1e-12
            && mm_seeds[i].churn == aif_seeds[i].churn
    });

    // Oracle regret over eligible seeds.
    let aif_regrets: Vec<f64> = (0..aif_seeds.len())
        .filter_map(|i| oracle[i].map(|o| o - aif_seeds[i].primary))
        .collect();
    let mag_regrets: Vec<f64> = (0..mag_seeds.len())
        .filter_map(|i| oracle[i].map(|o| o - mag_seeds[i].primary))
        .collect();
    let eligible = aif_regrets.len();
    let aif_regret_med = median(aif_regrets);
    let mag_regret_med = median(mag_regrets);

    println!("# koalisi #7 — categorical-magnitude vs Active-Inference A/B report");
    println!();
    println!(
        "_{REPORT_DATE} · catgraph backend · `CoalitionEvaluator` hot path (post-#14) · release build_"
    );
    println!();
    println!("Pre-registered A/B harness (koalisi #7). AIF expected-free-energy arm");
    println!("(`AifDecisionPolicy`) vs categorical-magnitude arm (`MagnitudePolicy`, t = 1).");
    println!();

    // Protocol summary.
    println!("## Protocol");
    println!();
    println!("- **Seeds:** {SEEDS} instances, seeds `0..{SEEDS}`, inline SplitMix64 (no `rand`).");
    println!(
        "- **Pool:** `n = 4 + next()%13` agents (n ∈ [4,16]); caps = k ∈ [1,4] distinct bits of an 8-bit universe; trust = 20 + next()%80."
    );
    println!(
        "- **Task stream:** T = {TASKS} tasks; required = r ∈ [1,5] distinct bits of the universe."
    );
    println!(
        "- **Decision stream:** seeded Fisher–Yates arrival order (drawn once per task); first arrival joins unconditionally (bootstrap); subsequent arrivals via `should_join`; one leave sweep in arrival order via `should_leave`."
    );
    println!("- **completed(task):** union of members' caps covers `required` fully.");
    println!(
        "- **coverage_eff(task):** (covered bits / required bits) / member_count, 0 if empty."
    );
    println!("- **PRIMARY(seed):** completion_rate × mean_cov_eff (stream-level product).");
    println!("- **Churn(seed):** total leave-sweep removals over the stream.");
    println!();

    // Per-seed table.
    println!("## Per-seed results");
    println!();
    println!(
        "| seed | n | mag_primary | scalar_primary | mm_primary | mag_churn | scalar_churn | mm_churn | oracle_primary |"
    );
    println!(
        "|-----:|--:|------------:|---------------:|-----------:|----------:|-------------:|---------:|---------------:|"
    );
    for i in 0..aif_seeds.len() {
        let a = &aif_seeds[i];
        let m = &mag_seeds[i];
        let mm = &mm_seeds[i];
        let oracle_cell = match oracle[i] {
            Some(o) => format!("{o:.4}"),
            None => "—".to_string(),
        };
        println!(
            "| {} | {} | {:.4} | {:.4} | {:.4} | {} | {} | {} | {} |",
            a.seed, a.n, m.primary, a.primary, mm.primary, m.churn, a.churn, mm.churn, oracle_cell
        );
    }
    println!();

    // Aggregate table.
    println!("## Aggregates (median · IQR)");
    println!();
    println!("| metric | Magnitude | AIF-scalar | AIF-mm |");
    println!("|--------|----------:|-----------:|-------:|");
    println!(
        "| primary | {mag_p_med:.4} · {mag_p_iqr:.4} | {aif_p_med:.4} · {aif_p_iqr:.4} | {mm_p_med:.4} · {mm_p_iqr:.4} |"
    );
    println!(
        "| churn | {mag_c_med:.2} · {mag_c_iqr:.2} | {aif_c_med:.2} · {aif_c_iqr:.2} | {mm_c_med:.2} · {mm_c_iqr:.2} |"
    );
    println!(
        "| latency µs | {mag_l_med:.3} · {mag_l_iqr:.3} | {aif_l_med:.3} · {aif_l_iqr:.3} | {mm_l_med:.3} · {mm_l_iqr:.3} |"
    );
    println!();
    println!(
        "_Latency: same hardware, both arms warm, sync path — the only machine-varying numbers in this report._"
    );
    println!();
    println!(
        "**Oracle regret** (n ≤ {ORACLE_MAX_N}, {eligible} eligible seeds): AIF median {aif_regret_med:.4}, Magnitude median {mag_regret_med:.4}."
    );
    println!();

    // Verdict.
    println!("## Verdict");
    println!();
    println!("### Original criterion (v1)");
    println!();
    println!(
        "- Criterion 1 (non-inferiority): mag median {mag_primary_med:.4} ≥ 0.95 × aif median {aif_primary_med:.4} ({}); mag strictly inferior in {inferior_count}/{SEEDS} seeds ≤ 40% ({}). → {}",
        pass(non_inferior_median),
        pass(inferior_ok),
        pass(crit1)
    );
    println!(
        "- Criterion 2 (latency): mag median {mag_l_med:.3} µs < aif median {aif_l_med:.3} µs → {}",
        pass(crit2)
    );
    println!();
    println!("**VERDICT (v1): {verdict}**");
    println!();
    println!("### Amended criterion (v2 — #7 amendment, 2026-07-02)");
    println!();
    println!(
        "- Path B.1 (clear superiority): mag median {mag_primary_med:.4} ≥ 1.25 × aif median {aif_primary_med:.4} → {}",
        pass(b1)
    );
    println!(
        "- Path B.2 (consistency): mag strictly superior in {superior_count}/{SEEDS} seeds ≥ 60% → {}",
        pass(b2)
    );
    println!(
        "- Path B.3 (bounded latency overhead): mag median {mag_l_med:.3} µs ≤ 10 × aif median {aif_l_med:.3} µs → {}",
        pass(b3)
    );
    println!("- Path A (v1 speed route): equals the v1 result → {}", pass(path_a));
    println!();
    println!("**VERDICT (v2): {v2_verdict}**");
    println!();
    println!(
        "_Criterion history: run 1 (2026-07-02) was scored under v1 — FALSIFIED (latency) — and that recorded outcome stands; the v2 amendment (quality-dominance Path B with bounded ≤10× latency overhead, OR the original Path A) was posted on #7 before any subsequent run and governs re-runs (K1 backend parity, post-optimization)._"
    );
    println!();
    println!("_Falsification is a legitimate result; nothing was tuned to flip it (koalisi #7)._");
    println!();

    // K4-v3 confirmatory verdict (multi-modality AIF arm; koalisi #43 Part 2).
    let lat_ratio = if aif_l_med > 0.0 { mm_l_med / aif_l_med } else { f64::NAN };
    println!("### K4-v3 confirmatory criteria (multi-modality AIF arm, #43 Part 2)");
    println!();
    println!(
        "- **H1 (gap closed):** mag median {mag_primary_med:.4} < 1.25 × mm median {mm_primary_med:.4} → {}",
        pass(h1)
    );
    println!(
        "- **H2 (mechanism):** mm median {mm_primary_med:.4} ≥ 1.25 × scalar median {aif_primary_med:.4} ({}) AND mm strictly superior in {mm_superior_count}/{SEEDS} seeds ≥ 60% ({}) → {}",
        pass(h2a),
        pass(h2b),
        pass(h2)
    );
    println!(
        "- **S1 (churn, secondary/non-gating):** mm churn median {mm_churn_med:.2} ≤ 0.5 × scalar churn median {scalar_churn_med:.2} → {}",
        pass(s1)
    );
    println!(
        "- **Latency (record-only):** mm median {mm_l_med:.3} µs, scalar median {aif_l_med:.3} µs, mm/scalar ratio {lat_ratio:.2}×."
    );
    println!();
    println!("**VERDICT (K4-v3): {k4v3_verdict}**");
    println!();
    if mm_equals_scalar {
        println!(
            "_Note: the registered `aif-mm` arm is **decision-equivalent** to `aif-scalar` — per-seed primary and churn match seed-for-seed. With binary union coverage and symmetric per-modality preferences the multi-modality `G` is a strictly monotone function of the covered-bit COUNT, the same information the scalar coverage fraction carries; both decision rules depend only on the sign of ΔG, so they make identical join/leave acts. The structure enters the *value* (`G` magnitudes differ) but not the *decision*. This is the mechanism the H2 test probes; it is reported, not tuned._"
        );
        println!();
    }

    // t-sweep.
    println!("## t-sweep (exploratory, non-gating)");
    println!();
    println!(
        "Magnitude at scales t ∈ {{0.5, 1.0, 2.0, 10.0}}. t = 1.0 sanity-checks the stable arm (median {mag_primary_med:.4}). Example-only policy — the library arm is pinned to t = 1 (catgraph #22)."
    );
    println!();
    println!("| t | magnitude primary median |");
    println!("|----:|-------------------------:|");
    for &(t, med) in sweep {
        println!("| {t:.1} | {med:.4} |");
    }
    println!();

    // Reproduce.
    println!("## Reproduce");
    println!();
    println!("```sh");
    println!("cargo run --release --manifest-path Cargo.toml --target-dir /tmp/koalisi-target \\");
    println!("  --features decision,magnitude --example strategy_comparison");
    println!("```");
    println!();
    println!(
        "_Release build required for the latency criterion (optimized code). \
         Debug builds run clean since `catgraph-magnitude v0.1.1` (catgraph #29 \
         fixed the over-strict triangle `debug_assert` that v0.1.0 tripped on \
         this battery's non-dyadic couplings; the pinned dep is `v0.2.0` since \
         koalisi #14)._"
    );
}

fn pass(b: bool) -> &'static str {
    if b { "PASS" } else { "FAIL" }
}

// ===========================================================================
// Part 3 — feedback-weighted arm vs magnitude (koalisi #46; #41 follow-up).
//
// Confirmatory battery over TWO scopes (prereg `docs/prereg-feedback-arm-k4.md`):
//   Scope A (i.i.d.)      — the null control: feedback ≈ its feedback-off base.
//   Scope B (reliability) — the contest: agents have a hidden bimodal
//                           reliability `ρ`; a covered task still fails unless
//                           every final member performed. `mag` (diversity) and
//                           EFE (coverage) are blind to `ρ`; feedback can learn
//                           it, so H-main predicts fb > mag on realized quality.
//
// Arms (base = SynergisticCalculator; ThresholdPolicy thresholds = 0.0):
//   mag — MagnitudePolicy::default()                        (frozen incumbent)
//   thr — ThresholdPolicy(Synergistic)                      (feedback-OFF control)
//   fb  — ThresholdPolicy(FeedbackCalculator(Synergistic, hw=0.5, fw=0.5, store))
//         with a FRESH FeedbackStore::new(1.0) per seed; outcomes written back
//         once per task after the leave sweep (see `run_instance`).
//
// The instance prefix (pool + tasks) is byte-identical across arms and scopes;
// only the value model + feedback differ. The `mag` arm reproduces the committed
// Part 2 `mag` column on Scope A seed-for-seed (the regression gate).
// ===========================================================================

/// Per-arm battery result: the 30 measured seeds and every per-decision latency.
struct ArmRun {
    seeds: Vec<InstanceMetrics>,
    lat: Vec<f64>,
}

/// The three arms of one scope's battery.
struct ScopeRun {
    mag: ArmRun,
    thr: ArmRun,
    fb: ArmRun,
}

/// Build the `fb` arm's policy + its write-back store handle (a FRESH store per
/// call, per the prereg — the two clones share one `Arc`, so the store the
/// calculator reads is the store `run_instance` records into).
fn make_fb(
    join_threshold: f64,
    hw: f64,
    fw: f64,
) -> (Box<dyn CoalitionDecisionPolicy>, Option<FeedbackStore>) {
    let store = FeedbackStore::new(1.0);
    let calc = FeedbackCalculator::new(SynergisticCalculator, hw, fw, store.clone());
    (
        Box::new(ThresholdPolicy::new(calc, join_threshold, 0.0)) as Box<dyn CoalitionDecisionPolicy>,
        Some(store),
    )
}

/// Run one arm's full 30-seed battery (discarded seed-0 warm-up first), building
/// a fresh arm — and thus a fresh feedback store — per seed via `make`. A fresh
/// store per seed keeps the 30 instances independent even though `fb` decisions
/// are path-dependent on within-seed task order by design.
fn run_fb_arm<F>(scope: Scope, make: F) -> ArmRun
where
    F: Fn(u64) -> (Box<dyn CoalitionDecisionPolicy>, Option<FeedbackStore>),
{
    let (warm_policy, warm_store) = make(0);
    let mut warm = Vec::new();
    let _ = run_instance(&*warm_policy, 0, scope, warm_store.as_ref(), &mut warm);

    let mut lat = Vec::new();
    let seeds: Vec<InstanceMetrics> = (0..SEEDS)
        .map(|s| {
            let (policy, store) = make(s);
            run_instance(&*policy, s, scope, store.as_ref(), &mut lat)
        })
        .collect();
    ArmRun { seeds, lat }
}

/// Run all three arms for one scope. `mag` is constructed once and cloned per
/// seed (`MagnitudePolicy::clone` SHARES its evaluator cache — gotcha 15 — so the
/// cache behaves exactly as the committed single-instance battery); `thr`/`fb`
/// build fresh per seed.
fn run_feedback_scope(scope: Scope, join_threshold: f64, hw: f64, fw: f64) -> ScopeRun {
    let mag = MagnitudePolicy::default();
    let mag_run = run_fb_arm(scope, |_| {
        (
            Box::new(mag.clone()) as Box<dyn CoalitionDecisionPolicy>,
            None,
        )
    });
    let thr_run = run_fb_arm(scope, |_| {
        (
            Box::new(ThresholdPolicy::new(SynergisticCalculator, join_threshold, 0.0))
                as Box<dyn CoalitionDecisionPolicy>,
            None,
        )
    });
    let fb_run = run_fb_arm(scope, |_| make_fb(join_threshold, hw, fw));
    ScopeRun {
        mag: mag_run,
        thr: thr_run,
        fb: fb_run,
    }
}

fn primaries(run: &ArmRun) -> Vec<f64> {
    run.seeds.iter().map(|m| m.primary).collect()
}

fn churns(run: &ArmRun) -> Vec<f64> {
    run.seeds.iter().map(|m| m.churn as f64).collect()
}

fn success_rates(run: &ArmRun) -> Vec<f64> {
    run.seeds.iter().map(|m| m.success_rate).collect()
}

/// Number of seeds on which `a` strictly beats `b` on PRIMARY.
fn superior_count(a: &ArmRun, b: &ArmRun) -> usize {
    (0..a.seeds.len())
        .filter(|&i| a.seeds[i].primary > b.seeds[i].primary)
        .count()
}

/// Print a scope's per-seed table: `mag`/`thr`/`fb` primary + churn per seed.
fn print_scope_table(run: &ScopeRun) {
    println!(
        "| seed | n | mag_primary | thr_primary | fb_primary | mag_churn | thr_churn | fb_churn |"
    );
    println!(
        "|-----:|--:|------------:|------------:|-----------:|----------:|----------:|---------:|"
    );
    for i in 0..run.mag.seeds.len() {
        let m = &run.mag.seeds[i];
        let t = &run.thr.seeds[i];
        let f = &run.fb.seeds[i];
        println!(
            "| {} | {} | {:.4} | {:.4} | {:.4} | {} | {} | {} |",
            m.seed, m.n, m.primary, t.primary, f.primary, m.churn, t.churn, f.churn
        );
    }
}

fn part3_feedback_arm() {
    let scope_a = run_feedback_scope(Scope::A, 0.0, 0.5, 0.5);
    let scope_b = run_feedback_scope(Scope::B, 0.0, 0.5, 0.5);
    print_feedback_report(&scope_a, &scope_b);
    print_weight_sweep();
}

#[allow(clippy::too_many_lines)]
fn print_feedback_report(scope_a: &ScopeRun, scope_b: &ScopeRun) {
    // Medians (Scope A = null control; Scope B = contest).
    let a_mag_med = median(primaries(&scope_a.mag));
    let a_thr_med = median(primaries(&scope_a.thr));
    let a_fb_med = median(primaries(&scope_a.fb));
    let b_mag_med = median(primaries(&scope_b.mag));
    let b_thr_med = median(primaries(&scope_b.thr));
    let b_fb_med = median(primaries(&scope_b.fb));

    // Confirmatory verdict — evaluated on Scope B.
    let h1 = b_mag_med < 1.25 * b_fb_med;
    let fb_sup_thr_b = superior_count(&scope_b.fb, &scope_b.thr);
    let h2a = b_fb_med >= 1.25 * b_thr_med;
    let h2b = fb_sup_thr_b >= FB_SUPERIOR_MIN;
    let h2 = h2a && h2b;
    let verdict = match (h1, h2) {
        (true, true) => "VALIDATED (feedback arm)",
        (false, true) => "PARTIAL (mechanism only)",
        (_, false) => "FALSIFIED (feedback)",
    };

    // Scope A red-flag: the registered prediction is fb ≈ thr and fb does NOT
    // clear H1. A Scope-A fb win points at a metric/leakage bug, not a success.
    let a_h1 = a_mag_med < 1.25 * a_fb_med;
    let fb_sup_thr_a = superior_count(&scope_a.fb, &scope_a.thr);
    let a_redflag = a_h1 || fb_sup_thr_a >= FB_SUPERIOR_MIN;

    println!("# koalisi #46 — feedback-weighted arm vs magnitude (K4 battery)");
    println!();
    println!(
        "_{FEEDBACK_REPORT_DATE} · catgraph backend · release build · base calculator `SynergisticCalculator` · `ThresholdPolicy` thresholds 0.0_"
    );
    println!();
    println!(
        "Confirmatory battery (prereg `docs/prereg-feedback-arm-k4.md`). Three arms — `mag` (`MagnitudePolicy`, frozen incumbent), `thr` (feedback-OFF `ThresholdPolicy<Synergistic>`), `fb` (`ThresholdPolicy<FeedbackCalculator<Synergistic>>`, `hw = fw = 0.5`, fresh store per seed) — over two scopes."
    );
    println!();
    println!("## Protocol");
    println!();
    println!(
        "- **Shared grammar:** {SEEDS} seeds `0..{SEEDS}`, inline SplitMix64; pool `n ∈ [4,16]`, caps `k ∈ [1,4]` bits of an 8-bit universe, trust `20–99`; `T = {TASKS}` tasks, required `r ∈ [1,5]` bits; seeded Fisher–Yates arrival; bootstrap-first-arrival; one leave sweep; seed-0 warm-up discarded."
    );
    println!(
        "- **Scope A (null control):** i.i.d.; success ≡ `completed` (union of member caps covers `required`); PRIMARY = completion_rate × mean_cov_eff (the committed Part 2 metric)."
    );
    println!(
        "- **Scope B (contest):** per-agent hidden reliability `ρ_i` (bimodal: reliable `ρ={RHO_RELIABLE}` w.p. {RELIABLE_PROB}, else flaky `ρ={RHO_FLAKY}`) + a pre-drawn arm-independent `perf[t][i]` matrix (`perform` w.p. `1−ρ_i`); success ≡ `completed AND all final members performed`; PRIMARY_B = success_rate × mean_cov_eff."
    );
    println!(
        "- **Feedback write-back:** `fb` records `success` (0/1) for the final coalition once per task, AFTER the leave sweep; `FeedbackStore::new(1.0)` ⇒ any non-success is a failure. `mag`/`thr` record nothing."
    );
    println!();

    // Scope A section.
    println!("## Scope A — i.i.d. null control");
    println!();
    print_scope_table(scope_a);
    println!();
    println!(
        "**Scope A medians:** mag {a_mag_med:.4} · thr {a_thr_med:.4} · fb {a_fb_med:.4}. fb strictly beats thr in {fb_sup_thr_a}/{SEEDS} seeds."
    );
    println!(
        "_Registered prediction: fb ≈ thr and fb does NOT clear H1 (mag {a_mag_med:.4} < 1.25 × fb {a_fb_med:.4} is {}). A Scope-A fb win is a RED FLAG to investigate (metric/leakage bug), not a success._",
        pass(a_h1)
    );
    if a_redflag {
        println!();
        println!(
            "> **⚠ RED FLAG:** Scope A shows a feedback advantage the null control did not predict — investigate before trusting the Scope-B contest."
        );
    }
    println!();

    // Scope B section.
    println!("## Scope B — reliability-structured contest");
    println!();
    print_scope_table(scope_b);
    println!();
    println!(
        "**Scope B medians:** mag {b_mag_med:.4} · thr {b_thr_med:.4} · fb {b_fb_med:.4}."
    );
    println!();

    // Record-only secondaries.
    let b_mag_succ = median(success_rates(&scope_b.mag));
    let b_thr_succ = median(success_rates(&scope_b.thr));
    let b_fb_succ = median(success_rates(&scope_b.fb));
    let b_mag_churn = median(churns(&scope_b.mag));
    let b_thr_churn = median(churns(&scope_b.thr));
    let b_fb_churn = median(churns(&scope_b.fb));
    let b_mag_lat = median(scope_b.mag.lat.clone());
    let b_thr_lat = median(scope_b.thr.lat.clone());
    let b_fb_lat = median(scope_b.fb.lat.clone());
    println!("### Scope B secondaries (record-only, non-gating)");
    println!();
    println!("| metric (median) | mag | thr | fb |");
    println!("|-----------------|----:|----:|---:|");
    println!("| success_rate | {b_mag_succ:.4} | {b_thr_succ:.4} | {b_fb_succ:.4} |");
    println!("| churn | {b_mag_churn:.2} | {b_thr_churn:.2} | {b_fb_churn:.2} |");
    println!("| latency µs | {b_mag_lat:.3} | {b_thr_lat:.3} | {b_fb_lat:.3} |");
    println!();
    println!(
        "_Expected if H-main holds: fb success_rate > thr ≈ mag (feedback learns to avoid flaky members)._"
    );
    println!();

    // Confirmatory verdict.
    println!("## Confirmatory verdict (Scope B)");
    println!();
    println!(
        "- **H1 (beats magnitude):** mag median {b_mag_med:.4} < 1.25 × fb median {b_fb_med:.4} → {}",
        pass(h1)
    );
    println!(
        "- **H2 (mechanism):** fb median {b_fb_med:.4} ≥ 1.25 × thr median {b_thr_med:.4} ({}) AND fb strictly superior to thr in {fb_sup_thr_b}/{SEEDS} seeds ≥ {FB_SUPERIOR_MIN} ({}) → {}",
        pass(h2a),
        pass(h2b),
        pass(h2)
    );
    println!();
    println!("**VERDICT (feedback arm, #46): {verdict}**");
    println!();
    println!(
        "_VALIDATED = H1 ∧ H2 · PARTIAL (mechanism only) = H2 ∧ ¬H1 · FALSIFIED = ¬H2. Thresholds (1.25×, {FB_SUPERIOR_MIN}/{SEEDS}) inherited from the K4-v2/v3 amendments; falsification is a legitimate result and nothing is tuned to flip it (koalisi #46)._"
    );
    println!();
}

/// E1 — weight sweep (exploratory, non-gating): `fb` `PRIMARY_B` median on Scope
/// B over `(hw, fw) ∈ {0, 0.5, 1, 2}²`. `(0, 0)` is the feedback-off control
/// (≡ `thr`); `(0.5, 0.5)` reproduces the confirmatory `fb` cell.
fn print_weight_sweep() {
    let weights = [0.0_f64, 0.5, 1.0, 2.0];
    println!("## E1 — weight sweep (exploratory, non-gating, Scope B)");
    println!();
    println!(
        "`fb` `PRIMARY_B` median over {SEEDS} seeds by (history_weight `hw`, failure_weight `fw`). `(0, 0)` ≡ the feedback-off `thr` control; `(0.5, 0.5)` = the confirmatory arm."
    );
    println!();
    print!("| hw \\ fw ");
    for fw in weights {
        print!("| fw={fw:.1} ");
    }
    println!("|");
    print!("|--------:");
    for _ in weights {
        print!("|-------:");
    }
    println!("|");
    for hw in weights {
        print!("| hw={hw:.1} ");
        for fw in weights {
            let run = run_fb_arm(Scope::B, |_| make_fb(0.0, hw, fw));
            let med = median(primaries(&run));
            print!("| {med:.4} ");
        }
        println!("|");
    }
    println!();
}

// ===========================================================================
// Part 4 — selective-base feedback arm vs magnitude (koalisi #48; absorbs #49).
//
// #46 falsified the feedback arm on a full-join base (`join_threshold = 0`):
// `ThresholdPolicy<Synergistic>` joins the whole pool, never leaves (churn 0),
// and the balanced `hw=fw=0.5` cancelled (`history ≈ failures` per member). This
// part re-runs the contest on a *selective* base (`join_threshold = 100.0`) with
// a *failure-only* signal (`hw = 0, fw = 1`), decomposing magnitude's edge into:
//   * selectivity        — isolated by `thr-selective` (positive threshold, no feedback);
//   * reliability-gating  — the increment `fb-selective` adds via failure-weighting.
//
// Arms (base = SynergisticCalculator; leave_threshold = 0.0):
//   mag           — MagnitudePolicy::default()                     (frozen incumbent)
//   thr-selective — ThresholdPolicy(Synergistic, 100.0, 0.0)       (feedback-OFF control)
//   fb-selective  — ThresholdPolicy(FeedbackCalculator(Synergistic, hw=0, fw=1, store),
//                                    100.0, 0.0), fresh store per seed.
//
// The `mag` arm reproduces the frozen #46 rows seed-for-seed (regression gate);
// verdict is 4-way (H1 × H2), distinct from Part 3's 3-way verdict.
// ===========================================================================

fn part4_selective_feedback() {
    let scope_a = run_feedback_scope(Scope::A, JOIN_THRESHOLD_SELECTIVE, HW_SELECTIVE, FW_SELECTIVE);
    let scope_b = run_feedback_scope(Scope::B, JOIN_THRESHOLD_SELECTIVE, HW_SELECTIVE, FW_SELECTIVE);
    print_selective_report(&scope_a, &scope_b);
    print_selective_threshold_sweep();
}

#[allow(clippy::too_many_lines)]
fn print_selective_report(scope_a: &ScopeRun, scope_b: &ScopeRun) {
    // Medians (Scope A = null control; Scope B = contest).
    let a_mag_med = median(primaries(&scope_a.mag));
    let a_thr_med = median(primaries(&scope_a.thr));
    let a_fb_med = median(primaries(&scope_a.fb));
    let b_mag_med = median(primaries(&scope_b.mag));
    let b_thr_med = median(primaries(&scope_b.thr));
    let b_fb_med = median(primaries(&scope_b.fb));

    // 4-way confirmatory verdict — evaluated on Scope B.
    let h1 = b_mag_med < 1.25 * b_fb_med;
    let fb_sup_thr_b = superior_count(&scope_b.fb, &scope_b.thr);
    let h2a = b_fb_med >= 1.25 * b_thr_med;
    let h2b = fb_sup_thr_b >= FB_SUPERIOR_MIN;
    let h2 = h2a && h2b;
    let verdict = match (h1, h2) {
        (true, true) => "VALIDATED (selective-feedback arm)",
        (true, false) => "PARTIAL (selectivity only)",
        (false, true) => "PARTIAL (mechanism only)",
        (false, false) => "FALSIFIED (selective feedback)",
    };

    // Scope A red-flag: the registered prediction is thr-selective ≈ fb-selective
    // and neither clears H1. A Scope-A fb win points at a metric/leakage bug.
    let a_h1 = a_mag_med < 1.25 * a_fb_med;
    let fb_sup_thr_a = superior_count(&scope_a.fb, &scope_a.thr);
    let a_redflag = a_h1 || fb_sup_thr_a >= FB_SUPERIOR_MIN;

    println!("# koalisi #48 — selective-base feedback arm vs magnitude (K4 battery v2)");
    println!();
    println!(
        "_{SELECTIVE_REPORT_DATE} · catgraph backend · release build · base calculator `SynergisticCalculator` · `join_threshold = 100.0` · `leave_threshold = 0.0` · weights `hw = 0, fw = 1` · prereg `docs/prereg-feedback-arm-k4-v2.md`_"
    );
    println!();
    println!(
        "Confirmatory battery decomposing magnitude's edge into **selectivity** (`thr-selective`) and **reliability-gating** (the `fb-selective` increment). Three arms — `mag` (`MagnitudePolicy`, frozen incumbent), `thr-selective` (feedback-OFF `ThresholdPolicy<Synergistic>` at `join_threshold = 100.0`, isolating selectivity), `fb-selective` (`ThresholdPolicy<FeedbackCalculator<Synergistic>>` at `join_threshold = 100.0`, `hw = 0`, `fw = 1`, fresh store per seed) — over two scopes. In the tables below the `thr`/`fb` columns are the `thr-selective`/`fb-selective` arms."
    );
    println!();
    println!("## Protocol");
    println!();
    println!(
        "- **Shared grammar:** {SEEDS} seeds `0..{SEEDS}`, inline SplitMix64; pool `n ∈ [4,16]`, caps `k ∈ [1,4]` bits of an 8-bit universe, trust `20–99`; `T = {TASKS}` tasks, required `r ∈ [1,5]` bits; seeded Fisher–Yates arrival; bootstrap-first-arrival; one leave sweep; seed-0 warm-up discarded."
    );
    println!(
        "- **Scope A (null control):** i.i.d.; success ≡ `completed` (union of member caps covers `required`); PRIMARY = completion_rate × mean_cov_eff."
    );
    println!(
        "- **Scope B (contest):** per-agent hidden reliability `ρ_i` (bimodal: reliable `ρ={RHO_RELIABLE}` w.p. {RELIABLE_PROB}, else flaky `ρ={RHO_FLAKY}`) + a pre-drawn arm-independent `perf[t][i]` matrix (`perform` w.p. `1−ρ_i`); success ≡ `completed AND all final members performed`; PRIMARY_B = success_rate × mean_cov_eff."
    );
    println!(
        "- **Feedback write-back:** `fb-selective` records `success` (0/1) for the final coalition once per task, AFTER the leave sweep; `FeedbackStore::new(1.0)` ⇒ any non-success is a failure. `mag`/`thr-selective` record nothing."
    );
    println!();

    // Regression gate (run validity, not hypothesis).
    let gate_a_ok = (a_mag_med - 0.4469).abs() < 5e-4;
    let gate_b_ok = (b_mag_med - 0.2818).abs() < 5e-4;
    println!("## Regression gate (run validity)");
    println!();
    println!(
        "- **`mag` Scope-A median** {a_mag_med:.4} must equal 0.4469 (`docs/ab-report-feedback-arm-k4.md`) → {}",
        pass(gate_a_ok)
    );
    println!(
        "- **`mag` Scope-B median** {b_mag_med:.4} must equal 0.2818 (`docs/ab-report-feedback-arm-k4.md`) → {}",
        pass(gate_b_ok)
    );
    if !(gate_a_ok && gate_b_ok) {
        println!();
        println!(
            "> **⚠ INVALID RUN:** the `mag` regression gate did not reproduce the frozen #46 medians — fix the harness, never the criteria (prereg)."
        );
    }
    println!();

    // Scope A section.
    println!("## Scope A — i.i.d. null control");
    println!();
    print_scope_table(scope_a);
    println!();
    println!(
        "**Scope A medians:** mag {a_mag_med:.4} · thr-selective {a_thr_med:.4} · fb-selective {a_fb_med:.4}. fb-selective strictly beats thr-selective in {fb_sup_thr_a}/{SEEDS} seeds."
    );
    println!(
        "_Registered prediction: thr-selective ≈ fb-selective and neither clears H1 (mag {a_mag_med:.4} < 1.25 × fb {a_fb_med:.4} is {}). A Scope-A fb win is a RED FLAG (metric/leakage bug), not a success._",
        pass(a_h1)
    );
    if a_redflag {
        println!();
        println!(
            "> **⚠ RED FLAG:** Scope A shows a feedback advantage the null control did not predict — investigate before trusting the Scope-B contest."
        );
    }
    println!();

    // Scope B section.
    println!("## Scope B — reliability-structured contest");
    println!();
    print_scope_table(scope_b);
    println!();
    println!("**Scope B medians:** mag {b_mag_med:.4} · thr-selective {b_thr_med:.4} · fb-selective {b_fb_med:.4}.");
    println!();

    // Record-only secondaries.
    let b_mag_succ = median(success_rates(&scope_b.mag));
    let b_thr_succ = median(success_rates(&scope_b.thr));
    let b_fb_succ = median(success_rates(&scope_b.fb));
    let b_mag_churn = median(churns(&scope_b.mag));
    let b_thr_churn = median(churns(&scope_b.thr));
    let b_fb_churn = median(churns(&scope_b.fb));
    let b_mag_lat = median(scope_b.mag.lat.clone());
    let b_thr_lat = median(scope_b.thr.lat.clone());
    let b_fb_lat = median(scope_b.fb.lat.clone());
    println!("### Scope B secondaries (record-only, non-gating)");
    println!();
    println!("| metric (median) | mag | thr-selective | fb-selective |");
    println!("|-----------------|----:|--------------:|-------------:|");
    println!("| success_rate | {b_mag_succ:.4} | {b_thr_succ:.4} | {b_fb_succ:.4} |");
    println!("| churn | {b_mag_churn:.2} | {b_thr_churn:.2} | {b_fb_churn:.2} |");
    println!("| latency µs | {b_mag_lat:.3} | {b_thr_lat:.3} | {b_fb_lat:.3} |");
    println!();
    println!(
        "_Expected: fb-selective churn ↑ vs the falsified `join = 0` arms; fb-selective success_rate > thr-selective if H2 holds._"
    );
    println!();

    // Confirmatory verdict.
    println!("## Confirmatory verdict (Scope B)");
    println!();
    println!(
        "- **H1 (beats magnitude):** mag median {b_mag_med:.4} < 1.25 × fb-selective median {b_fb_med:.4} → {}",
        pass(h1)
    );
    println!(
        "- **H2 (mechanism beyond selectivity):** fb-selective median {b_fb_med:.4} ≥ 1.25 × thr-selective median {b_thr_med:.4} ({}) AND fb-selective strictly superior to thr-selective in {fb_sup_thr_b}/{SEEDS} seeds ≥ {FB_SUPERIOR_MIN} ({}) → {}",
        pass(h2a),
        pass(h2b),
        pass(h2)
    );
    println!();
    println!("**VERDICT (selective-feedback arm, #48): {verdict}**");
    println!();
    println!(
        "_VALIDATED = H1 ∧ H2 · PARTIAL (selectivity only) = H1 ∧ ¬H2 · PARTIAL (mechanism only) = H2 ∧ ¬H1 · FALSIFIED = ¬H1 ∧ ¬H2. Thresholds (1.25×, {FB_SUPERIOR_MIN}/{SEEDS}) inherited from the K4-v2/v3/#46 amendments; falsification is a legitimate result and nothing is tuned to flip it (koalisi #48)._"
    );
    println!();
}

/// E1 — selectivity threshold sweep (exploratory, non-gating): `thr-selective`
/// and `fb-selective` `PRIMARY_B` + churn medians on Scope B over
/// `join_threshold ∈ SELECTIVE_THRESHOLD_GRID`. The `join = 100.0` row must match
/// the confirmatory medians (sanity).
fn print_selective_threshold_sweep() {
    println!("## E1 — selectivity threshold sweep (exploratory, non-gating, Scope B)");
    println!();
    println!(
        "`thr-selective` (feedback-off, `hw = fw = 0`) and `fb-selective` (`hw = 0, fw = 1`) `PRIMARY_B` + churn medians over {SEEDS} seeds by `join_threshold`. The `join = 100.0` row matches the confirmatory arms (sanity)."
    );
    println!();
    println!(
        "| join_threshold | thr-selective PRIMARY_B | fb-selective PRIMARY_B | thr churn (med) | fb churn (med) |"
    );
    println!(
        "|---------------:|------------------------:|-----------------------:|----------------:|---------------:|"
    );
    for join in SELECTIVE_THRESHOLD_GRID {
        // One battery yields both arms: the `thr` arm is a bare
        // `ThresholdPolicy` that never reads `hw`/`fw`, so its result is
        // independent of the `(0.0, 1.0)` weights the `fb` arm uses.
        let scope = run_feedback_scope(Scope::B, join, 0.0, 1.0);
        let thr_primary = median(primaries(&scope.thr));
        let fb_primary = median(primaries(&scope.fb));
        let thr_churn = median(churns(&scope.thr));
        let fb_churn = median(churns(&scope.fb));
        println!(
            "| {join:.1} | {thr_primary:.4} | {fb_primary:.4} | {thr_churn:.2} | {fb_churn:.2} |"
        );
    }
    println!();
}

// ===========================================================================
// Part 3b — scalar-AIF Scope-B baseline (koalisi #44, K4-v4 prereg).
//
// Purely additive, non-gating: runs the shipped scalar bridge
// `AifDecisionPolicy::default()` through the Scope-B reliability battery so its
// per-seed PRIMARY_B + churn rows can be frozen as the K4-v4 baseline. The
// scalar policy is stateless per call (store = None), so it runs all 30 seeds
// directly, exactly as the `mag` arm does. Scope-B instances are byte-identical
// to every other Scope-B arm — they come from `generate_instance_b(seed)`, not
// shared state. Alters no existing section, ordering, or printed value.
// ===========================================================================

fn part3b_scalar_aif_scope_b_baseline() {
    // Warm-up: full seed-0 Scope-B instance, latencies discarded (matches the
    // other batteries' warm-cache convention). Rows are seed-derived, so the
    // warm-up cannot perturb any printed value.
    let aif = AifDecisionPolicy::default();
    let mut warm = Vec::new();
    let _ = run_instance(&aif, 0, Scope::B, None, &mut warm);

    let mut lat = Vec::new();
    let seeds: Vec<InstanceMetrics> = (0..SEEDS)
        .map(|s| run_instance(&aif, s, Scope::B, None, &mut lat))
        .collect();

    let primary_med = median(seeds.iter().map(|m| m.primary).collect());
    let churn_med = median(seeds.iter().map(|m| m.churn as f64).collect());

    println!("# koalisi #44 — scalar-AIF Scope-B baseline (K4-v4 prereg)");
    println!();
    println!(
        "_scalar bridge `AifDecisionPolicy::default()` · Scope B (reliability contest) · store = None · additive baseline, non-gating_"
    );
    println!();
    println!(
        "Per-seed `PRIMARY_B` + churn for the shipped scalar AIF arm over {SEEDS} seeds `0..{SEEDS}`, byte-identical Scope-B instances (`generate_instance_b(seed)`), seed-0 warm-up discarded. Frozen as the K4-v4 baseline (koalisi #44)."
    );
    println!();
    println!("| seed | n | primary_B | churn |");
    println!("|-----:|--:|----------:|------:|");
    for m in &seeds {
        println!("| {} | {} | {:.4} | {} |", m.seed, m.n, m.primary, m.churn);
    }
    println!();
    println!("**Medians:** primary_B {primary_med:.4} · churn {churn_med:.2}.");
    println!();
}

// ===========================================================================
// Part 4c — persistent-agent AIF arm vs magnitude/scalar (koalisi #44, K4-v4
// prereg + Amendment 2). ADDITIVE ONLY — all existing Parts print unchanged.
//
// Confirmatory: `aif-pers` (registered PersistentAifConfig) over Scope B, 30
// seeds, per-seed factory (fresh persistent arm per seed). After each task's
// leave sweep the harness computes per-bit outcome success (bit b succeeds iff
// any FINAL member providing b performed) and calls the arm's outcome hook — the
// arm learns per-bit reliability across the stream. `scalar`/`mag` Scope-B rows
// are reused for the strict-superiority (H2) and act-divergence (S1) tests and as
// regression gates (mag 0.2818 / scalar 0.1035). Then exploratory E4–E8 tables.
// ===========================================================================

/// Per-seed Scope-B result for an instrumented arm: PRIMARY_B, churn, and the
/// ordered join/leave act stream (for S1 act divergence).
struct SeedResultB {
    primary: f64,
    churn: usize,
    acts: Vec<bool>,
}

/// Run one arm over one Scope-B seed, mirroring [`run_instance`]'s Scope::B metric
/// path byte-for-byte (so `scalar`/`mag` reproduce their frozen medians), while
/// additionally capturing the act stream and invoking `on_task_outcome` once per
/// task after the leave sweep with `(required, per_bit_success)`.
fn run_seed_b(
    policy: &dyn CoalitionDecisionPolicy,
    seed: u64,
    lat: &mut Vec<f64>,
    on_task_outcome: impl FnMut(u32, &[bool], bool, &[usize]),
) -> SeedResultB {
    run_seed_b_regime(policy, seed, Regime::V1, lat, on_task_outcome)
}

/// [`run_seed_b`] under a chosen [`Regime`] (koalisi #61, EQ1) — the instance
/// draw is the ONLY difference; the metric path, the decision stream, and the
/// outcome hook are shared verbatim, so `Regime::V1` reproduces the frozen
/// batteries exactly.
///
/// The per-bit outcome slice handed to `on_task_outcome` is
/// [`regime_universe`]-wide (koalisi #61 Part 5c) — 8 entries in both registered
/// regimes, so the frozen values are unchanged; only its Rust type widened from
/// `&[bool; 8]` to `&[bool]`.
fn run_seed_b_regime(
    policy: &dyn CoalitionDecisionPolicy,
    seed: u64,
    regime: Regime,
    lat: &mut Vec<f64>,
    mut on_task_outcome: impl FnMut(u32, &[bool], bool, &[usize]),
) -> SeedResultB {
    let (agents, tasks, _rho, perf) = generate_instance_b_regime(seed, regime);
    let universe = regime_universe(regime);

    let mut success_count = 0usize;
    let mut cov_eff_sum = 0.0f64;
    let mut churn = 0usize;
    let mut acts = Vec::new();

    for (t, task) in tasks.iter().enumerate() {
        let ctx = DecisionContext {
            required_capabilities: task.required,
        };
        let mut members: Vec<usize> = vec![task.order[0]];

        for &idx in &task.order[1..] {
            let candidate: &dyn AgentCapabilities = &agents[idx];
            let coalition = coalition_view(&agents, &members);
            let t0 = Instant::now();
            let d = policy.should_join(candidate, &coalition, &ctx);
            lat.push(seconds_to_us(t0.elapsed()));
            acts.push(d.act);
            if d.act {
                members.push(idx);
            }
        }

        for &idx in &task.order {
            let Some(pos) = members.iter().position(|&m| m == idx) else {
                continue;
            };
            let coalition = coalition_view(&agents, &members);
            let agent: &dyn AgentCapabilities = &agents[idx];
            let t0 = Instant::now();
            let d = policy.should_leave(agent, &coalition, &ctx);
            lat.push(seconds_to_us(t0.elapsed()));
            acts.push(d.act);
            if d.act {
                members.remove(pos);
                churn += 1;
            }
        }

        let union = members.iter().fold(0u32, |acc, &i| acc | agents[i].caps);
        let covered = (union & task.required).count_ones();
        let completed = covered == task.required.count_ones();
        let success = completed && members.iter().all(|&i| perf[t][i]);
        if success {
            success_count += 1;
        }
        let cov_eff = if members.is_empty() {
            0.0
        } else {
            (f64::from(covered) / f64::from(task.required.count_ones())) / members.len() as f64
        };
        cov_eff_sum += cov_eff;

        // Per-bit outcome success (harness-computed; the arm stays decoupled from
        // `perf`): bit b succeeds iff any final member providing b performed.
        let mut per_bit = vec![false; universe];
        for (b, slot) in per_bit.iter_mut().enumerate() {
            *slot = members
                .iter()
                .any(|&i| (agents[i].caps >> b) & 1 == 1 && perf[t][i]);
        }
        on_task_outcome(task.required, &per_bit, success, &members);
    }

    let success_rate = success_count as f64 / tasks.len() as f64;
    let mean_cov_eff = cov_eff_sum / tasks.len() as f64;
    SeedResultB {
        primary: success_rate * mean_cov_eff,
        churn,
        acts,
    }
}

/// Run the `aif-pers` battery over the Scope-B seed range `start..end`: a FRESH
/// persistent arm per seed (the `run_fb_arm` factory pattern), with the per-task
/// outcome hook wired into the arm. Discards a warm-up (on `start`) first (warm
/// caches; warm-up cannot perturb the seed-derived results). Returns the per-seed
/// results and every measured latency (µs).
fn persistent_battery_range(
    config: PersistentAifConfig,
    start: u64,
    end: u64,
) -> (Vec<SeedResultB>, Vec<f64>) {
    persistent_battery_mode(config, RunMode::V1_ORACLE, start, end)
}

/// How a Scope-B battery draws its instances and feeds outcomes back to a
/// learning arm (koalisi #61, EQ1). Factors the two axes the battery-v2
/// factorial varies over the frozen runners' fixed choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RunMode {
    /// Instance draw (see [`Regime`]).
    regime: Regime,
    /// `true` = the degraded/L2 signal (whole-task `success` smeared across the
    /// required bits — the runtime-feasible #55 contract); `false` = the per-bit
    /// oracle signal.
    degraded: bool,
}

impl RunMode {
    /// The frozen Parts 2–4h mode: v1 draw, per-bit oracle signal.
    const V1_ORACLE: Self = Self {
        regime: Regime::V1,
        degraded: false,
    };
    /// The frozen Part 4e/4f/4h degraded mode: v1 draw, whole-task signal.
    const V1_DEGRADED: Self = Self {
        regime: Regime::V1,
        degraded: true,
    };
}

/// The generalized persistent battery: [`persistent_battery_range`] (oracle) and
/// [`persistent_battery_range_degraded`] are its `RunMode::V1_*` instantiations,
/// so both keep their frozen behaviour by construction. Fresh arm per seed,
/// warm-up on `start` discarded.
fn persistent_battery_mode(
    config: PersistentAifConfig,
    mode: RunMode,
    start: u64,
    end: u64,
) -> (Vec<SeedResultB>, Vec<f64>) {
    let degraded = mode.degraded;
    // The degraded smear is one entry per universe bit — `[success; 8]` in both
    // registered regimes, so the frozen batteries are unchanged (koalisi #61
    // Part 5c widened this from a fixed-8 array).
    let width = regime_universe(mode.regime);
    {
        let arm = PersistentAifArm::new(start, config).expect("persistent arm construction");
        let mut warm = Vec::new();
        let _ = run_seed_b_regime(&arm, start, mode.regime, &mut warm, |req, bits, success, _| {
            if degraded {
                arm.observe_outcome(req, &vec![success; width]);
            } else {
                arm.observe_outcome(req, bits);
            }
        });
    }
    let mut lat = Vec::new();
    let results = (start..end)
        .map(|s| {
            let arm = PersistentAifArm::new(s, config).expect("persistent arm construction");
            run_seed_b_regime(&arm, s, mode.regime, &mut lat, |req, bits, success, _| {
                if degraded {
                    arm.observe_outcome(req, &vec![success; width]);
                } else {
                    arm.observe_outcome(req, bits);
                }
            })
        })
        .collect();
    (results, lat)
}

/// `persistent_battery_range` over `0..seeds` (Part 4c uses this; byte-identical
/// to the pre-range code — warm-up seed 0, seeds `0..seeds`).
fn persistent_battery(config: PersistentAifConfig, seeds: u64) -> (Vec<SeedResultB>, Vec<f64>) {
    persistent_battery_range(config, 0, seeds)
}

/// Like [`persistent_battery_range`], but feeds the arm the DEGRADED outcome
/// signal (koalisi #54 Step 2): the whole-coalition `success` bool smeared across
/// every required bit — i.e. the signal a runtime can produce from a single
/// task-completion event with *no* per-member performance telemetry. Because
/// `PersistentAifArm::observe_outcome` ignores entries for non-required bits (they
/// become no-obs), passing `&[success; 8]` means exactly "every required bit
/// observes the coalition-level outcome" and nothing else. Same warm-up discipline
/// and fresh-arm-per-seed factory as the oracle-signal battery.
fn persistent_battery_range_degraded(
    config: PersistentAifConfig,
    start: u64,
    end: u64,
) -> (Vec<SeedResultB>, Vec<f64>) {
    persistent_battery_mode(config, RunMode::V1_DEGRADED, start, end)
}

/// Run a stateless arm (scalar / magnitude) over the Scope-B seed range
/// `start..end` with no outcome hook, capturing act streams. Warm-up (on `start`)
/// discarded.
fn stateless_battery_range(
    make: impl Fn() -> Box<dyn CoalitionDecisionPolicy>,
    start: u64,
    end: u64,
) -> (Vec<SeedResultB>, Vec<f64>) {
    stateless_battery_mode(make, Regime::V1, start, end)
}

/// [`stateless_battery_range`] under a chosen [`Regime`] (koalisi #61, EQ1) —
/// the battery-v2 context rows for `mag` and `scalar`.
fn stateless_battery_mode(
    make: impl Fn() -> Box<dyn CoalitionDecisionPolicy>,
    regime: Regime,
    start: u64,
    end: u64,
) -> (Vec<SeedResultB>, Vec<f64>) {
    {
        let p = make();
        let mut warm = Vec::new();
        let _ = run_seed_b_regime(&*p, start, regime, &mut warm, |_, _, _, _| {});
    }
    let mut lat = Vec::new();
    let p = make();
    let results = (start..end)
        .map(|s| run_seed_b_regime(&*p, s, regime, &mut lat, |_, _, _, _| {}))
        .collect();
    (results, lat)
}

/// `stateless_battery_range` over `0..seeds` (Part 4c uses this).
fn stateless_battery_b(
    make: impl Fn() -> Box<dyn CoalitionDecisionPolicy>,
    seeds: u64,
) -> (Vec<SeedResultB>, Vec<f64>) {
    stateless_battery_range(make, 0, seeds)
}

fn primaries_b(rs: &[SeedResultB]) -> Vec<f64> {
    rs.iter().map(|r| r.primary).collect()
}
fn churns_b(rs: &[SeedResultB]) -> Vec<f64> {
    rs.iter().map(|r| r.churn as f64).collect()
}
/// Seeds on which `a` strictly beats `b` on PRIMARY_B.
fn superior_count_b(a: &[SeedResultB], b: &[SeedResultB]) -> usize {
    (0..a.len()).filter(|&i| a[i].primary > b[i].primary).count()
}
/// Seeds on which the act streams differ (S1 divergence vs the scalar theorem).
fn act_divergence(a: &[SeedResultB], b: &[SeedResultB]) -> usize {
    (0..a.len()).filter(|&i| a[i].acts != b[i].acts).count()
}

#[allow(clippy::too_many_lines)]
fn part4c_persistent_aif() {
    // Confirmatory batteries — Scope B, 30 seeds.
    let (pers, pers_lat) = persistent_battery(PersistentAifConfig::default(), SEEDS);
    let (scalar, _scalar_lat) = stateless_battery_b(
        || Box::new(AifDecisionPolicy::default()) as Box<dyn CoalitionDecisionPolicy>,
        SEEDS,
    );
    let mag_policy = MagnitudePolicy::default();
    let (mag, _mag_lat) = stateless_battery_b(
        || Box::new(mag_policy.clone()) as Box<dyn CoalitionDecisionPolicy>,
        SEEDS,
    );

    let pers_med = median(primaries_b(&pers));
    let scalar_med = median(primaries_b(&scalar));
    let mag_med = median(primaries_b(&mag));
    let pers_churn_med = median(churns_b(&pers));
    let scalar_churn_med = median(churns_b(&scalar));
    let pers_lat_med = median(pers_lat.clone());

    // Regression gates (run validity, not hypothesis): frozen Scope-B medians.
    assert_eq!(
        format!("{scalar_med:.4}"),
        "0.1035",
        "regression gate: scalar Scope-B median must reproduce 0.1035 (baseline-aif-scalar-scope-b.md)"
    );
    assert_eq!(
        format!("{mag_med:.4}"),
        "0.2818",
        "regression gate: mag Scope-B median must reproduce 0.2818 (ab-report-feedback-arm-k4-v2.md)"
    );

    // Confirmatory criteria (prereg §Confirmatory criteria v4).
    let h1 = mag_med < 1.25 * pers_med;
    let h2a = pers_med >= 1.25 * scalar_med;
    let pers_sup_scalar = superior_count_b(&pers, &scalar);
    let h2b = pers_sup_scalar >= 18;
    let h2 = h2a && h2b;
    let s1 = act_divergence(&pers, &scalar);
    let verdict = match (h1, h2) {
        (true, true) => "VALIDATED (gap closed)",
        (false, true) => "PARTIAL (mechanism only)",
        (_, false) => "FALSIFIED (persistence)",
    };

    println!("# koalisi #44 — persistent-agent AIF arm (K4-v4, Amendment 2)");
    println!();
    println!(
        "_persistent multimodal AIF (`PersistentAifConfig::default()`, aif-v0.11.0 count injection) · Scope B (reliability contest) · 30 seeds `0..{SEEDS}` · per-seed factory + per-bit outcome hook · confirmatory_"
    );
    println!();
    println!(
        "Three arms — `aif-pers` (registered persistent arm), `aif-scalar` (`AifDecisionPolicy::default()`, frozen), `mag` (`MagnitudePolicy`, frozen) — over Scope B. Regression gate: `scalar` median {scalar_med:.4} ≡ 0.1035, `mag` median {mag_med:.4} ≡ 0.2818."
    );
    println!();
    println!("## Per-seed PRIMARY_B + churn");
    println!();
    println!("| seed | pers_primary | scalar_primary | mag_primary | pers_churn | scalar_churn | acts_differ |");
    println!("|-----:|-------------:|---------------:|------------:|-----------:|-------------:|:-----------:|");
    for i in 0..pers.len() {
        let differ = if pers[i].acts != scalar[i].acts { "yes" } else { "no" };
        println!(
            "| {} | {:.4} | {:.4} | {:.4} | {} | {} | {} |",
            i, pers[i].primary, scalar[i].primary, mag[i].primary, pers[i].churn, scalar[i].churn, differ
        );
    }
    println!();
    println!(
        "**Medians:** pers {pers_med:.4} · scalar {scalar_med:.4} · mag {mag_med:.4}. Churn: pers {pers_churn_med:.2} · scalar {scalar_churn_med:.2}. Latency pers {pers_lat_med:.3} µs (record-only)."
    );
    println!();

    // Confirmatory verdict.
    println!("## Confirmatory verdict (Scope B)");
    println!();
    println!(
        "- **H1 (gap closed):** mag median {mag_med:.4} < 1.25 × pers median {pers_med:.4} ({:.4}) → {}",
        1.25 * pers_med,
        pass(h1)
    );
    println!(
        "- **H2 (mechanism):** pers median {pers_med:.4} ≥ 1.25 × scalar median {scalar_med:.4} (= {:.6}) ({}) AND pers strictly superior to scalar in {pers_sup_scalar}/{SEEDS} seeds ≥ 18 ({}) → {}",
        1.25 * scalar_med,
        pass(h2a),
        pass(h2b),
        pass(h2)
    );
    println!(
        "- **S1 (act divergence, non-gating):** pers act stream differs from scalar on {s1}/{SEEDS} seeds. {}",
        if s1 == 0 {
            "divergence = 0 ⇒ the arm collapsed back to the K4-v3 theorem."
        } else {
            "divergence > 0 ⇒ the arm genuinely escapes the theorem."
        }
    );
    println!(
        "- **S2 (churn, non-gating):** pers churn median {pers_churn_med:.2} vs scalar 113.00."
    );
    println!();
    println!("**VERDICT (persistent arm, #44): {verdict}**");
    println!();
    println!(
        "_VALIDATED (gap closed) = H1 ∧ H2 · PARTIAL (mechanism only) = H2 ∧ ¬H1 · FALSIFIED (persistence) = ¬H2. Thresholds (1.25×, 18/{SEEDS}) inherited from v2/v3; falsification is a legitimate result and nothing is tuned to flip it (koalisi #44)._"
    );
    println!();

    // Exploratory E4–E8 (non-gating, single-toggle tables).
    print_persistent_exploratory();
}

/// E4–E8 exploratory conditions (non-gating): one 30-seed Scope-B battery each
/// under a single toggle off the registered arm, reported as PRIMARY_B + churn
/// medians. No verdicts (prereg §Exploratory conditions).
fn print_persistent_exploratory() {
    let base = PersistentAifConfig::default();
    let rows: Vec<(String, PersistentAifConfig)> = vec![
        (
            "E4 PerTask (reset each task)".to_owned(),
            PersistentAifConfig { trial_boundary: TrialBoundary::PerTask, ..base },
        ),
        (
            "E5 learning off".to_owned(),
            PersistentAifConfig { persistent_learning: false, ..base },
        ),
        (
            "E6 dynamics off (MeanField query)".to_owned(),
            PersistentAifConfig { query_dynamics: false, ..base },
        ),
        (
            "E7 novelty off".to_owned(),
            PersistentAifConfig { query_novelty: false, ..base },
        ),
        (
            "E8 initial_precision_b = 1.0".to_owned(),
            PersistentAifConfig { initial_precision_b: 1.0, ..base },
        ),
        (
            "E8 initial_precision_b = 4.0 (registered)".to_owned(),
            base,
        ),
        (
            "E8 initial_precision_b = 16.0".to_owned(),
            PersistentAifConfig { initial_precision_b: 16.0, ..base },
        ),
    ];

    println!("## Exploratory E4–E8 (non-gating, Scope B, {SEEDS} seeds)");
    println!();
    println!("| condition | median PRIMARY_B | churn median |");
    println!("|-----------|----------------:|-------------:|");
    for (label, cfg) in rows {
        let (rs, _lat) = persistent_battery(cfg, SEEDS);
        let med = median(primaries_b(&rs));
        let churn = median(churns_b(&rs));
        println!("| {label} | {med:.4} | {churn:.2} |");
    }
    println!();
    println!("_Single-toggle ablations off the registered arm; no verdicts (prereg §Exploratory conditions)._");
    println!();
}

// ===========================================================================
// Part 4d — E1-only persistent AIF arm, out-of-sample (koalisi #53, K4-v5
// prereg `docs/prereg-K4-v5-e1-persistent-aif.md`). ADDITIVE ONLY.
//
// Registered arm `aif-e1` = the #44 PersistentAifArm (943d139, NO code changes)
// with the v4 E6 configuration: MeanField queries at fixed γ = 16, no
// PrecisionDynamics (`query_dynamics: false`); everything else the v4 registered
// config. Confirmatory over the FRESH seed range 30..60 (out-of-sample: the
// motivating 0..30 E6 win cannot self-confirm on a deterministic battery).
// Thresholds are computed from THIS run's 30..60 medians; 0..30 numbers are cited
// as context strings only, never scored.
// ===========================================================================

/// The registered `aif-e1` configuration (v4 E6 branch): MeanField queries, fixed
/// γ = 16, no precision dynamics; all other levers at the v4 registered defaults.
fn e1_config() -> PersistentAifConfig {
    PersistentAifConfig {
        query_dynamics: false,
        ..PersistentAifConfig::default()
    }
}

#[allow(clippy::too_many_lines)]
fn part4d_e1_persistent_aif() {
    // Confirmatory batteries — Scope B, seeds 30..60 (out-of-sample).
    let (e1, e1_lat) = persistent_battery_range(e1_config(), 30, 60);
    let (scalar, _scalar_lat) = stateless_battery_range(
        || Box::new(AifDecisionPolicy::default()) as Box<dyn CoalitionDecisionPolicy>,
        30,
        60,
    );
    let mag_policy = MagnitudePolicy::default();
    let (mag, _mag_lat) = stateless_battery_range(
        || Box::new(mag_policy.clone()) as Box<dyn CoalitionDecisionPolicy>,
        30,
        60,
    );

    let e1_med = median(primaries_b(&e1));
    let scalar_med = median(primaries_b(&scalar));
    let mag_med = median(primaries_b(&mag));
    let e1_churn_med = median(churns_b(&e1));
    let scalar_churn_med = median(churns_b(&scalar));
    let e1_lat_med = median(e1_lat.clone());

    // Confirmatory criteria (prereg §Confirmatory criteria v5) — computed from THIS
    // run's 30..60 medians only. No cross-seed-range scoring.
    let h1 = mag_med < 1.25 * e1_med;
    let h2a = e1_med >= 1.25 * scalar_med;
    let e1_sup_scalar = superior_count_b(&e1, &scalar);
    let h2b = e1_sup_scalar >= 18;
    let h2 = h2a && h2b;
    let s1 = act_divergence(&e1, &scalar);
    let verdict = match (h1, h2) {
        (true, true) => "VALIDATED (gap closed)",
        (false, true) => "PARTIAL (mechanism only)",
        (_, false) => "FALSIFIED (E1)",
    };

    println!("# koalisi #53 — E1-only persistent AIF arm, out-of-sample (K4-v5)");
    println!();
    println!(
        "_registered `aif-e1` = #44 `PersistentAifArm` (943d139, no code changes) in the v4 E6 configuration (MeanField queries, fixed γ = 16, no `PrecisionDynamics`) · Scope B · seeds **30..60** (out-of-sample) · confirmatory_"
    );
    println!();
    println!(
        "Three arms — `aif-e1` (registered), `aif-scalar` (`AifDecisionPolicy::default()`, frozen), `mag` (`MagnitudePolicy`, frozen) — on 30 fresh instances. All thresholds are this run's own 30..60 medians; cross-range numbers (v4 0..30 E6 0.4042, magnitude 0.2818) are context only, never scored."
    );
    println!();
    println!("## Per-seed PRIMARY_B + churn (seeds 30..60)");
    println!();
    println!("| seed | e1_primary | scalar_primary | mag_primary | e1_churn | scalar_churn | acts_differ |");
    println!("|-----:|-----------:|---------------:|------------:|---------:|-------------:|:-----------:|");
    for i in 0..e1.len() {
        let seed = 30 + i as u64;
        let differ = if e1[i].acts != scalar[i].acts { "yes" } else { "no" };
        println!(
            "| {} | {:.4} | {:.4} | {:.4} | {} | {} | {} |",
            seed, e1[i].primary, scalar[i].primary, mag[i].primary, e1[i].churn, scalar[i].churn, differ
        );
    }
    println!();
    println!(
        "**Medians (30..60):** e1 {e1_med:.4} · scalar {scalar_med:.4} · mag {mag_med:.4}. Churn: e1 {e1_churn_med:.2} · scalar {scalar_churn_med:.2}. Latency e1 {e1_lat_med:.3} µs (record-only)."
    );
    println!();

    // Confirmatory verdict.
    println!("## Confirmatory verdict (Scope B, seeds 30..60)");
    println!();
    println!(
        "- **H1 (gap closed):** mag median {mag_med:.4} < 1.25 × e1 median {e1_med:.4} (= {:.4}) → {}",
        1.25 * e1_med,
        pass(h1)
    );
    println!(
        "- **H2 (mechanism):** e1 median {e1_med:.4} ≥ 1.25 × scalar median {scalar_med:.4} (= {:.6}) ({}) AND e1 strictly superior to scalar in {e1_sup_scalar}/30 seeds ≥ 18 ({}) → {}",
        1.25 * scalar_med,
        pass(h2a),
        pass(h2b),
        pass(h2)
    );
    println!(
        "- **S1 (act divergence, non-gating):** e1 act stream differs from scalar on {s1}/30 seeds."
    );
    println!(
        "- **S2 (churn, non-gating):** e1 churn median {e1_churn_med:.2} vs scalar {scalar_churn_med:.2} (the 0..30 E6 churn 210 was flagged high — see whether the pattern persists)."
    );
    println!();
    println!("**VERDICT (E1 arm, #53): {verdict}**");
    println!();
    println!(
        "_VALIDATED (gap closed) = H1 ∧ H2 · PARTIAL (mechanism only) = H2 ∧ ¬H1 · FALSIFIED (E1) = ¬H2. Thresholds (1.25×, 18/30) inherit the v2→v4 family; baselines are this run's own 30..60 rows; nothing is tuned to flip the verdict (koalisi #53)._"
    );
    println!();

    // Exploratory X1 / X2.
    print_e1_exploratory();
}

/// X1 (novelty off on 30..60) + X2 (0..30 re-score determinism check). Non-gating
/// except the X2 assertion, which is a run-invalidating determinism gate.
fn print_e1_exploratory() {
    println!("## Exploratory X1–X2 (non-gating; X2 is a determinism gate)");
    println!();

    // X1 — novelty off (the v4 E7 analog) on 30..60.
    let x1_cfg = PersistentAifConfig {
        query_novelty: false,
        ..e1_config()
    };
    let (x1, _) = persistent_battery_range(x1_cfg, 30, 60);
    let x1_med = median(primaries_b(&x1));
    let x1_churn = median(churns_b(&x1));

    // X2 — re-score the registered arm on 0..30; MUST reproduce the v4 E6 numbers
    // (0.4042 / 210.00) exactly — a determinism + comparability gate. A mismatch
    // invalidates the run.
    let (x2, _) = persistent_battery(e1_config(), SEEDS);
    let x2_med = median(primaries_b(&x2));
    let x2_churn = median(churns_b(&x2));
    assert_eq!(
        format!("{x2_med:.4}"),
        "0.4042",
        "X2 determinism gate: registered arm on seeds 0..30 must reproduce the v4 E6 median 0.4042"
    );
    assert_eq!(
        format!("{x2_churn:.2}"),
        "210.00",
        "X2 determinism gate: registered arm on seeds 0..30 must reproduce the v4 E6 churn 210.00"
    );

    println!("| condition | median PRIMARY_B | churn median |");
    println!("|-----------|----------------:|-------------:|");
    println!("| X1 novelty off (30..60) | {x1_med:.4} | {x1_churn:.2} |");
    println!("| X2 registered arm re-score (0..30, ≡ v4 E6) | {x2_med:.4} | {x2_churn:.2} |");
    println!();
    println!(
        "_X1 isolates the novelty lever on the out-of-sample seeds. X2 re-scores the registered arm on 0..30 as a determinism/comparability check — it reproduces the v4 E6 numbers 0.4042 / 210.00 exactly (asserted in-code)._"
    );
    println!();
}

// ===========================================================================
// Part 4e — arm-choice addendum (koalisi #54). UNREGISTERED, EXPLORATORY.
//
// Additive to the frozen Parts 1–4d; no verdict is derived here. Two questions
// for the #54 cost-quality decision memo: (1) what churn does the frozen `mag`
// arm incur on the out-of-sample seeds (the quality winner's cost side); and
// (2) does the E1 persistent arm still work when fed only the runtime-feasible
// DEGRADED outcome signal (whole-coalition success) instead of the per-bit
// oracle signal the battery hands it. Everything prints strictly AFTER Part 4d.
// ===========================================================================

fn part4e_arm_choice_addendum() {
    println!("# koalisi #54 — arm-choice addendum (unregistered, exploratory)");
    println!();
    println!(
        "_additive to the frozen Parts 1–4d; unregistered and exploratory — informs the #54 cost-quality decision memo; no verdict is derived from this section._"
    );
    println!();

    // --- mag churn (the quality winner's cost side) ------------------------
    let mag_policy = MagnitudePolicy::default();
    let (mag, _) = stateless_battery_range(
        || Box::new(mag_policy.clone()) as Box<dyn CoalitionDecisionPolicy>,
        30,
        60,
    );
    let mag_prim_med = median(primaries_b(&mag));
    let mag_churn_med = median(churns_b(&mag));

    println!("## mag churn (seeds 30..60)");
    println!();
    println!("| seed | mag_primary | mag_churn |");
    println!("|-----:|------------:|----------:|");
    for (i, r) in mag.iter().enumerate() {
        let seed = 30 + i as u64;
        println!("| {} | {:.4} | {} |", seed, r.primary, r.churn);
    }
    println!();
    println!("**Medians:** mag primary_B {mag_prim_med:.4} · mag churn {mag_churn_med:.2}.");
    println!();
    println!(
        "_mag_primary deterministically reproduces the Part 4d `mag_primary` column; it is reprinted here only so this churn table is self-contained._"
    );
    println!();

    // --- degraded outcome signal — e1 (oracle vs runtime-feasible signal) ---
    let (e1_oracle, _) = persistent_battery_range(e1_config(), 30, 60);
    let (e1_deg, _) = persistent_battery_range_degraded(e1_config(), 30, 60);
    let (scalar, _) = stateless_battery_range(
        || Box::new(AifDecisionPolicy::default()) as Box<dyn CoalitionDecisionPolicy>,
        30,
        60,
    );

    let e1_oracle_med = median(primaries_b(&e1_oracle));
    let e1_deg_med = median(primaries_b(&e1_deg));
    let scalar_med = median(primaries_b(&scalar));
    let e1_deg_churn_med = median(churns_b(&e1_deg));

    println!("## degraded outcome signal — e1 (seeds 30..60)");
    println!();
    println!("| seed | e1_oracle_primary | e1_degraded_primary | e1_degraded_churn |");
    println!("|-----:|------------------:|--------------------:|------------------:|");
    for i in 0..e1_oracle.len() {
        let seed = 30 + i as u64;
        println!(
            "| {} | {:.4} | {:.4} | {} |",
            seed, e1_oracle[i].primary, e1_deg[i].primary, e1_deg[i].churn
        );
    }
    println!();
    println!(
        "**Medians:** e1 oracle {e1_oracle_med:.4} · e1 degraded {e1_deg_med:.4} · scalar {scalar_med:.4}. Degraded churn {e1_deg_churn_med:.2}."
    );
    println!();
    println!(
        "_degraded = whole-coalition `success` smeared across the required bits (the runtime-feasible signal). If degraded ≈ oracle, the runtime needs only a task-completion event; if degraded ≈ scalar, the per-bit oracle signal is load-bearing and e1 stays battery-only pending finer telemetry. Assessed in the #54 design note, not here._"
    );
    println!();
}

// ===========================================================================
// Part 4f — churn-mitigation frontier (koalisi #54 Step 3). UNREGISTERED,
// EXPLORATORY. Additive to the frozen Parts 1–4e; no verdict is derived here.
//
// Sweeps a join-margin threshold (δ) × a leave-hysteresis threshold (h) over the
// registered `aif-e1` arm, entirely example-side (no `src/` change). The wrapper
// is EXACT at (0, 0): `PersistentAifArm::decide` returns `score = p1 - 0.5` and
// decides join on `score > 0`, leave on `score >= 0` (its `required == 0` /
// engine-error paths return `act = false`). So `act && score > δ` / `act && score
// >= h` is the bare arm at (0, 0) and monotone-tightens for positive δ/h — an
// identity gate asserts the (0, 0) reproduction, mirroring the X2 determinism gate.
// ===========================================================================

/// Decision-score tap: the raw `p1 − 0.5` margins the wrapped arm produced, split
/// by join vs leave. Filled only for the (0, 0) identity cell (Part 4f score
/// distribution); every other cell passes `tap: None`.
#[derive(Default)]
struct ScoreTap {
    join_scores: Vec<f64>,
    leave_scores: Vec<f64>,
}

/// Margin / hysteresis wrapper over a [`PersistentAifArm`] (koalisi #54 Step 3,
/// example-only). Tightens the arm's own decision rule: join iff `act && score >
/// join_delta`, leave iff `act && score >= leave_delta`. At `(0, 0)` this is the
/// bare arm bit-for-bit — the arm's finite path already decides join on `score >
/// 0` and leave on `score >= 0`, and its `required == 0` / engine-error paths
/// return `act = false` (which stays false under the AND). See `decide()` in
/// `src/decision/aif_persistent_policy.rs`.
struct MarginE1<'a> {
    arm: &'a PersistentAifArm,
    join_delta: f64,
    leave_delta: f64,
    tap: Option<&'a std::sync::Mutex<ScoreTap>>,
}

impl CoalitionDecisionPolicy for MarginE1<'_> {
    fn should_join(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        ctx: &DecisionContext,
    ) -> Decision {
        let d = self.arm.should_join(agent, coalition, ctx);
        if let Some(tap) = self.tap {
            tap.lock().expect("score tap poisoned").join_scores.push(d.score);
        }
        Decision {
            act: d.act && d.score > self.join_delta,
            score: d.score,
        }
    }

    fn should_leave(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        ctx: &DecisionContext,
    ) -> Decision {
        let d = self.arm.should_leave(agent, coalition, ctx);
        if let Some(tap) = self.tap {
            tap.lock().expect("score tap poisoned").leave_scores.push(d.score);
        }
        Decision {
            act: d.act && d.score >= self.leave_delta,
            score: d.score,
        }
    }
}

/// Run the margin/hysteresis-wrapped `aif-e1` arm over the Scope-B seed range
/// `start..end`: fresh `PersistentAifArm` per seed, wrapped by a per-seed
/// [`MarginE1`] at `(jd, ld)`. `degraded` selects the outcome signal fed back to
/// the arm (oracle per-bit vs whole-coalition success smeared across the required
/// bits — see [`persistent_battery_range_degraded`]). Same warm-up discipline as
/// the other persistent batteries; the warm-up wrapper always gets `tap: None`, so
/// a supplied `tap` collects scores only from the scored seeds.
fn margin_battery_range(
    config: PersistentAifConfig,
    jd: f64,
    ld: f64,
    degraded: bool,
    start: u64,
    end: u64,
    tap: Option<&std::sync::Mutex<ScoreTap>>,
) -> (Vec<SeedResultB>, Vec<f64>) {
    let mode = RunMode {
        regime: Regime::V1,
        degraded,
    };
    margin_battery_mode(config, jd, ld, mode, start, end, tap)
}

/// [`margin_battery_range`] under a chosen [`RunMode`] (koalisi #61, EQ1) — the
/// Part 5a factorial runs every cell through this one path, δ = 0 included, so
/// the wrapper is common to the cell and its own baseline.
fn margin_battery_mode(
    config: PersistentAifConfig,
    jd: f64,
    ld: f64,
    mode: RunMode,
    start: u64,
    end: u64,
    tap: Option<&std::sync::Mutex<ScoreTap>>,
) -> (Vec<SeedResultB>, Vec<f64>) {
    let degraded = mode.degraded;
    // See `persistent_battery_mode`: 8 entries in both registered regimes.
    let width = regime_universe(mode.regime);
    {
        let arm = PersistentAifArm::new(start, config).expect("persistent arm construction");
        let wrapper = MarginE1 {
            arm: &arm,
            join_delta: jd,
            leave_delta: ld,
            tap: None,
        };
        let mut warm = Vec::new();
        let _ = run_seed_b_regime(
            &wrapper,
            start,
            mode.regime,
            &mut warm,
            |req, bits, success, _| {
                if degraded {
                    arm.observe_outcome(req, &vec![success; width]);
                } else {
                    arm.observe_outcome(req, bits);
                }
            },
        );
    }
    let mut lat = Vec::new();
    let results = (start..end)
        .map(|s| {
            let arm = PersistentAifArm::new(s, config).expect("persistent arm construction");
            let wrapper = MarginE1 {
                arm: &arm,
                join_delta: jd,
                leave_delta: ld,
                tap,
            };
            run_seed_b_regime(&wrapper, s, mode.regime, &mut lat, |req, bits, success, _| {
                if degraded {
                    arm.observe_outcome(req, &vec![success; width]);
                } else {
                    arm.observe_outcome(req, bits);
                }
            })
        })
        .collect();
    (results, lat)
}

#[allow(clippy::too_many_lines)]
fn part4f_churn_frontier() {
    println!("# koalisi #54 — Part 4f: churn-mitigation frontier (unregistered, exploratory)");
    println!();
    println!(
        "_join margin `p > 0.5 + δ` (wrapper `score > δ`) × leave hysteresis (`score ≥ h` to evict), swept over seeds 30..60 under BOTH the oracle and degraded outcome signals. Unregistered and exploratory: a promising point here is a v6 registration CANDIDATE only — a fresh prereg on seeds 60..90 must precede any quality claim._"
    );
    println!();

    // --- Identity gate (run-invalidating, like X2) -------------------------
    let tap = std::sync::Mutex::new(ScoreTap::default());
    let (base, _base_lat) = margin_battery_range(e1_config(), 0.0, 0.0, false, 30, 60, Some(&tap));
    let (ref_e1, _ref_lat) = persistent_battery_range(e1_config(), 30, 60);
    assert_eq!(
        base.len(),
        ref_e1.len(),
        "Part 4f identity gate: MarginE1(0,0) must reproduce the bare arm"
    );
    for i in 0..base.len() {
        assert!(
            base[i].primary.to_bits() == ref_e1[i].primary.to_bits()
                && base[i].churn == ref_e1[i].churn,
            "Part 4f identity gate: MarginE1(0,0) must reproduce the bare arm"
        );
    }
    println!(
        "**Identity gate:** `MarginE1(δ=0, h=0)` reproduces the bare `aif-e1` arm on all {} seeds (primary + churn bit-identical; asserted in-code).",
        base.len()
    );
    println!();

    // --- Decision-score distribution (identity cell, oracle) ---------------
    let (mut js, mut ls) = {
        let g = tap.lock().expect("score tap poisoned");
        (g.join_scores.clone(), g.leave_scores.clone())
    };
    js.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    ls.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let qtl = |s: &[f64], p: f64| -> f64 {
        if s.is_empty() {
            return f64::NAN;
        }
        let idx = (p * (s.len() as f64 - 1.0)).round() as usize;
        s[idx.min(s.len() - 1)]
    };
    println!("## decision-score distribution (identity cell, oracle)");
    println!();
    println!("| stream | n | min | p25 | p50 | p75 | max |");
    println!("|--------|--:|----:|----:|----:|----:|----:|");
    println!(
        "| join | {} | {:.4} | {:.4} | {:.4} | {:.4} | {:.4} |",
        js.len(),
        qtl(&js, 0.0),
        qtl(&js, 0.25),
        qtl(&js, 0.5),
        qtl(&js, 0.75),
        qtl(&js, 1.0)
    );
    println!(
        "| leave | {} | {:.4} | {:.4} | {:.4} | {:.4} | {:.4} |",
        ls.len(),
        qtl(&ls, 0.0),
        qtl(&ls, 0.25),
        qtl(&ls, 0.5),
        qtl(&ls, 0.75),
        qtl(&ls, 1.0)
    );
    println!();
    println!(
        "_quantiles by nearest-rank on the sorted scores (`score = p(control 1) − 0.5`). Shows where posterior mass sits: with fixed γ = 16 the query posteriors may saturate near ±0.5, which would bound how much a margin/hysteresis threshold can move._"
    );
    println!();

    // --- Frontier grids ----------------------------------------------------
    let jd_grid = [0.0, 0.05, 0.15, 0.30, 0.45];
    let ld_grid = [0.0, 0.05, 0.15, 0.30];

    // Oracle: reuse the identity-cell `base` for (0, 0); run every other cell.
    let mut oracle_cells: Vec<(f64, f64, f64, f64)> = Vec::new();
    for (ji, &jd) in jd_grid.iter().enumerate() {
        for (li, &ld) in ld_grid.iter().enumerate() {
            let (prim, churn) = if ji == 0 && li == 0 {
                (median(primaries_b(&base)), median(churns_b(&base)))
            } else {
                let (rs, _) = margin_battery_range(e1_config(), jd, ld, false, 30, 60, None);
                (median(primaries_b(&rs)), median(churns_b(&rs)))
            };
            oracle_cells.push((jd, ld, prim, churn));
        }
    }

    // Degraded: every cell runs (including its own (0, 0)).
    let mut deg_cells: Vec<(f64, f64, f64, f64)> = Vec::new();
    for &jd in &jd_grid {
        for &ld in &ld_grid {
            let (rs, _) = margin_battery_range(e1_config(), jd, ld, true, 30, 60, None);
            deg_cells.push((jd, ld, median(primaries_b(&rs)), median(churns_b(&rs))));
        }
    }

    let print_grid = |title: &str, cells: &[(f64, f64, f64, f64)]| {
        println!("{title}");
        println!();
        print!("| δ\\h |");
        for &ld in &ld_grid {
            print!(" h={ld:.2} |");
        }
        println!();
        print!("|---|");
        for _ in &ld_grid {
            print!("---|");
        }
        println!();
        for (ji, &jd) in jd_grid.iter().enumerate() {
            print!("| δ={jd:.2} |");
            for li in 0..ld_grid.len() {
                let cell = &cells[ji * ld_grid.len() + li];
                print!(" {:.4}/{:.0} |", cell.2, cell.3);
            }
            println!();
        }
        println!();
        println!("_cell = median PRIMARY_B / median churn over seeds 30..60._");
        println!();
    };

    print_grid("## frontier — oracle signal (seeds 30..60)", &oracle_cells);
    print_grid("## frontier — degraded signal (seeds 30..60)", &deg_cells);

    // --- Pareto summary ----------------------------------------------------
    println!("## Pareto summary");
    println!();
    println!(
        "_Reference (this run's Part 4d/4e values, stable under the identity gate; not recomputed here): mag 0.2720 / churn 8 · e1 baseline 0.4406 / churn 136._"
    );
    println!();
    let print_pareto = |signal: &str, cells: &[(f64, f64, f64, f64)]| {
        let mut qualifying: Vec<&(f64, f64, f64, f64)> =
            cells.iter().filter(|c| c.2 >= 0.40).collect();
        qualifying.sort_by(|a, b| a.3.partial_cmp(&b.3).unwrap_or(std::cmp::Ordering::Equal));
        println!("**{signal}** — cells with median PRIMARY_B ≥ 0.40, by churn ascending:");
        println!();
        if qualifying.is_empty() {
            println!("- none.");
        } else {
            for (rank, c) in qualifying.iter().enumerate() {
                if rank == 0 {
                    println!(
                        "- **(δ={:.2}, h={:.2}) {:.4}/{:.0} — v6 candidate ({signal})**",
                        c.0, c.1, c.2, c.3
                    );
                } else {
                    println!("- (δ={:.2}, h={:.2}) {:.4}/{:.0}", c.0, c.1, c.2, c.3);
                }
            }
        }
        println!();
    };
    print_pareto("oracle", &oracle_cells);
    print_pareto("degraded", &deg_cells);

    println!(
        "_Unregistered exploratory sweep — nothing here is a verdict. Any v6 arm must be pre-registered on fresh seeds 60..90 before a quality claim; the (δ, h) grid was fixed before this run, not tuned on its results._"
    );
    println!();
}

// ===========================================================================
// Part 4g — reliability-filtered magnitude (koalisi #54 option-C probe).
// UNREGISTERED, EXPLORATORY. Additive to the frozen Parts 1–4f; no verdict here.
//
// Magnitude stays PURELY STRUCTURAL; reliability gates SEPARATELY. Injecting
// reliability into the couplings backfires: scaling `A(i→j)` down for an
// unreliable agent makes it LESS substitutable, which RAISES its Möbius weight
// (magnitude measures diversity, not dependability). So this composes the two
// shipped mechanisms — `MagnitudePolicy` for structure + the #41 `FeedbackStore`
// as a reliability VETO — fed by the whole-task success signal (the same single
// task-completion event #54 Step 2 established as runtime-feasible).
// ===========================================================================

/// Reliability-veto wrapper (koalisi #54 option-C, example-only): the structural
/// `mag` decision, overridden only by a reliability gate sourced from a
/// [`FeedbackStore`]. See the section comment for why reliability is composed with
/// magnitude rather than folded into its couplings.
struct RelFilteredMag<'a> {
    mag: &'a MagnitudePolicy,
    store: &'a FeedbackStore,
    tau: f64,
    n_min: u64,
    filter_leave: bool,
}

impl RelFilteredMag<'_> {
    /// Reliability estimate for `id`: `1 − failures/history`, or `None` (cold
    /// start) when fewer than `n_min` episodes are recorded. `None` never vetoes —
    /// optimistic, mirroring the X1 lesson that epistemic joining is load-bearing.
    fn rel(&self, id: usize) -> Option<f64> {
        let h = self.store.history(id);
        if h < self.n_min {
            None
        } else {
            Some(1.0 - self.store.failures(id) as f64 / h as f64)
        }
    }
}

impl CoalitionDecisionPolicy for RelFilteredMag<'_> {
    fn should_join(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        ctx: &DecisionContext,
    ) -> Decision {
        let d = self.mag.should_join(agent, coalition, ctx);
        // Veto a structural join when the agent's estimated reliability is below τ.
        // Score passthrough (exploratory semantics — the score stays the structural
        // magnitude margin; only `act` is gated).
        if d.act && matches!(self.rel(agent.agent_id()), Some(r) if r < self.tau) {
            return Decision {
                act: false,
                score: d.score,
            };
        }
        d
    }

    fn should_leave(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        ctx: &DecisionContext,
    ) -> Decision {
        let d = self.mag.should_leave(agent, coalition, ctx);
        // Reliability eviction: force a leave that mag would have kept, when the
        // member's estimated reliability is below τ (score passthrough as above).
        if self.filter_leave
            && !d.act
            && matches!(self.rel(agent.agent_id()), Some(r) if r < self.tau)
        {
            return Decision {
                act: true,
                score: d.score,
            };
        }
        d
    }
}

/// Run the reliability-filtered `mag` arm over the Scope-B seed range
/// `start..end`: one shared-cache [`MagnitudePolicy`] (cache is decision-neutral,
/// same as Part 4d), a FRESH per-seed [`FeedbackStore`] (the #46 Arm-factory
/// contract), and a per-seed [`RelFilteredMag`] wrapper at `(tau, n_min,
/// filter_leave)`. The outcome hook records the whole-task success (`1.0` / `0.0`)
/// against the final `members` once per task after the leave sweep;
/// `FeedbackStore::new(0.5)` counts a failure iff the recorded value `< 0.5`.
/// Warm-up on `start` discarded (fresh store).
fn rel_mag_battery_range(
    tau: f64,
    n_min: u64,
    filter_leave: bool,
    start: u64,
    end: u64,
) -> (Vec<SeedResultB>, Vec<f64>) {
    let mag_policy = MagnitudePolicy::default();
    {
        let store = FeedbackStore::new(0.5);
        let policy = RelFilteredMag {
            mag: &mag_policy,
            store: &store,
            tau,
            n_min,
            filter_leave,
        };
        let mut warm = Vec::new();
        let _ = run_seed_b(&policy, start, &mut warm, |_req, _bits, success, members| {
            store.record_outcome(members, if success { 1.0 } else { 0.0 });
        });
    }
    let mut lat = Vec::new();
    let results = (start..end)
        .map(|s| {
            let store = FeedbackStore::new(0.5);
            let policy = RelFilteredMag {
                mag: &mag_policy,
                store: &store,
                tau,
                n_min,
                filter_leave,
            };
            run_seed_b(&policy, s, &mut lat, |_req, _bits, success, members| {
                store.record_outcome(members, if success { 1.0 } else { 0.0 });
            })
        })
        .collect();
    (results, lat)
}

#[allow(clippy::too_many_lines)]
fn part4g_reliability_filtered_mag() {
    println!(
        "# koalisi #54 — Part 4g: reliability-filtered magnitude (unregistered, exploratory)"
    );
    println!();
    println!(
        "_the option-C probe: magnitude stays purely STRUCTURAL, reliability gates SEPARATELY via the #41 `FeedbackStore` (a veto), fed ONLY the whole-task success signal (the runtime-feasible L2 task-completion event, #54 Step 2). Folding reliability into the couplings backfires — down-scaling `A(i→j)` for an unreliable agent makes it LESS substitutable, RAISING its Möbius weight — so the two mechanisms are composed, not merged. Unregistered and exploratory; the grid was fixed before the run._"
    );
    println!();

    // --- Identity gate (run-invalidating) ----------------------------------
    // τ = 0 can never veto (`r < 0.0` is impossible for `r = 1 − failures/h ≥ 0`),
    // so RelFilteredMag(0, 1, false) must reproduce the bare `mag` arm seed-for-seed.
    let (idg, _) = rel_mag_battery_range(0.0, 1, false, 30, 60);
    let mag_policy = MagnitudePolicy::default();
    let (bare, _) = stateless_battery_range(
        || Box::new(mag_policy.clone()) as Box<dyn CoalitionDecisionPolicy>,
        30,
        60,
    );
    assert_eq!(
        idg.len(),
        bare.len(),
        "Part 4g identity gate: RelFilteredMag(τ=0) must reproduce bare mag"
    );
    for i in 0..idg.len() {
        assert!(
            idg[i].primary.to_bits() == bare[i].primary.to_bits() && idg[i].churn == bare[i].churn,
            "Part 4g identity gate: RelFilteredMag(τ=0) must reproduce bare mag"
        );
    }
    println!(
        "**Identity gate:** `RelFilteredMag(τ=0, n_min=1, filter_leave=false)` reproduces the bare `mag` arm on all {} seeds (primary + churn bit-identical; asserted in-code).",
        idg.len()
    );
    println!();

    // --- Grid --------------------------------------------------------------
    let tau_grid = [0.3, 0.5, 0.7];
    let nmin_grid = [1u64, 2];

    let run_grid = |filter_leave: bool| -> Vec<(f64, u64, f64, f64)> {
        let mut cells = Vec::new();
        for &tau in &tau_grid {
            for &nmin in &nmin_grid {
                let (rs, _) = rel_mag_battery_range(tau, nmin, filter_leave, 30, 60);
                cells.push((tau, nmin, median(primaries_b(&rs)), median(churns_b(&rs))));
            }
        }
        cells
    };
    let cells_noleave = run_grid(false);
    let cells_leave = run_grid(true);

    let print_grid = |title: &str, cells: &[(f64, u64, f64, f64)]| {
        println!("{title}");
        println!();
        print!("| τ\\n_min |");
        for &nmin in &nmin_grid {
            print!(" n={nmin} |");
        }
        println!();
        print!("|---|");
        for _ in &nmin_grid {
            print!("---|");
        }
        println!();
        for (ti, &tau) in tau_grid.iter().enumerate() {
            print!("| τ={tau:.1} |");
            for ni in 0..nmin_grid.len() {
                let c = &cells[ti * nmin_grid.len() + ni];
                print!(" {:.4}/{:.0} |", c.2, c.3);
            }
            println!();
        }
        println!();
        println!("_cell = median PRIMARY_B / median churn over seeds 30..60._");
        println!();
    };
    print_grid("## grid — filter_leave = false (join veto only)", &cells_noleave);
    print_grid(
        "## grid — filter_leave = true (join veto + reliability eviction)",
        &cells_leave,
    );

    // --- Summary -----------------------------------------------------------
    println!("## Summary");
    println!();
    println!(
        "_Reference (cited, not recomputed): mag 0.2720/8 · e1 oracle 0.4406/136 · e1 degraded 0.4381/143 · scalar 0.1267/79.5._"
    );
    println!();
    let mut all: Vec<(f64, u64, bool, f64, f64)> = Vec::new();
    for c in &cells_noleave {
        all.push((c.0, c.1, false, c.2, c.3));
    }
    for c in &cells_leave {
        all.push((c.0, c.1, true, c.2, c.3));
    }
    all.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal));
    println!("Top cells by median PRIMARY_B (descending):");
    println!();
    for c in all.iter().take(5) {
        println!(
            "- (τ={:.1}, n_min={}, leave={}) {:.4}/{:.0}",
            c.0, c.1, c.2, c.3, c.4
        );
    }
    println!();
    println!(
        "_Unregistered exploratory probe — nothing here is a verdict. If a cell approaches e1's ~0.44 at mag-like churn, option C becomes the v6-registration candidate (fresh prereg, seeds 60..90); otherwise it is ruled out._"
    );
    println!();
}

// ===========================================================================
// Part 4h — K4-v6 never-evict E1 arm, dual-signal, out-of-sample (koalisi #56).
// THE REGISTERED RUN. Governed by `docs/prereg-K4-v6-never-evict.md` (committed +
// posted to #56 before implementation); seeds 60..90; both signals gating;
// thresholds from THIS run's own 60..90 medians. Additive; every existing printed
// line byte-identical.
// ===========================================================================

#[allow(clippy::too_many_lines)]
fn part4h_v6_never_evict() {
    // The registered lever: never-evict (eviction cap 0) atop the #53 E6 config.
    let ne_cfg = PersistentAifConfig {
        eviction_cap: Some(0),
        ..e1_config()
    };

    println!(
        "# koalisi #56 — K4-v6: never-evict E1 arm, dual-signal, out-of-sample (REGISTERED)"
    );
    println!();
    println!(
        "_governed by `docs/prereg-K4-v6-never-evict.md` (committed + posted pre-implementation); registered lever = `eviction_cap: Some(0)` (churn 0 by construction) atop the #53 E6 `aif-e1` config; Scope B · seeds **60..90** (out-of-sample, never used by v1–v5 or #54 Parts 4d–4g); BOTH signals gating; all thresholds are THIS run's own 60..90 medians._"
    );
    println!();

    // --- X-A run-validity gate (run-invalidating, like X2) -----------------
    // e1-k0 (cap None, oracle) on 30..60 must reproduce the #53 registered numbers.
    let (xa, _) = persistent_battery_range(e1_config(), 30, 60);
    let xa_med = median(primaries_b(&xa));
    let xa_churn = median(churns_b(&xa));
    assert_eq!(
        format!("{xa_med:.4}"),
        "0.4406",
        "X-A gate: e1-k0 (cap None, oracle) on 30..60 must reproduce the #53 median 0.4406"
    );
    assert_eq!(
        format!("{xa_churn:.2}"),
        "136.00",
        "X-A gate: e1-k0 (cap None, oracle) on 30..60 must reproduce the #53 churn 136.00"
    );
    println!(
        "**X-A gate:** `e1-k0` (cap `None`, oracle) on seeds 30..60 reproduces the #53 registered numbers 0.4406 / 136.00 exactly (asserted in-code)."
    );
    println!();

    // --- Confirmatory batteries — Scope B, seeds 60..90 --------------------
    let (ne_oracle, ne_o_lat) = persistent_battery_range(ne_cfg, 60, 90);
    let (ne_deg, _ne_d_lat) = persistent_battery_range_degraded(ne_cfg, 60, 90);
    let (e1k0, _e1k0_lat) = persistent_battery_range(e1_config(), 60, 90);
    let (scalar, _scalar_lat) = stateless_battery_range(
        || Box::new(AifDecisionPolicy::default()) as Box<dyn CoalitionDecisionPolicy>,
        60,
        90,
    );
    let mag_policy = MagnitudePolicy::default();
    let (mag, _mag_lat) = stateless_battery_range(
        || Box::new(mag_policy.clone()) as Box<dyn CoalitionDecisionPolicy>,
        60,
        90,
    );

    let ne_o_med = median(primaries_b(&ne_oracle));
    let ne_d_med = median(primaries_b(&ne_deg));
    let e1k0_med = median(primaries_b(&e1k0));
    let scalar_med = median(primaries_b(&scalar));
    let mag_med = median(primaries_b(&mag));
    let ne_o_churn = median(churns_b(&ne_oracle));
    let ne_d_churn = median(churns_b(&ne_deg));
    let e1k0_churn = median(churns_b(&e1k0));
    let ne_o_lat_med = median(ne_o_lat.clone());

    println!("## Per-seed PRIMARY_B + churn (seeds 60..90)");
    println!();
    println!(
        "| seed | ne_oracle | ne_degraded | e1k0_oracle | scalar | mag | ne_o_churn | ne_d_churn |"
    );
    println!(
        "|-----:|----------:|------------:|------------:|-------:|----:|-----------:|-----------:|"
    );
    for i in 0..ne_oracle.len() {
        let seed = 60 + i as u64;
        println!(
            "| {} | {:.4} | {:.4} | {:.4} | {:.4} | {:.4} | {} | {} |",
            seed,
            ne_oracle[i].primary,
            ne_deg[i].primary,
            e1k0[i].primary,
            scalar[i].primary,
            mag[i].primary,
            ne_oracle[i].churn,
            ne_deg[i].churn
        );
    }
    println!();
    println!(
        "**Medians (60..90):** ne-oracle {ne_o_med:.4} · ne-degraded {ne_d_med:.4} · e1-k0 {e1k0_med:.4} · scalar {scalar_med:.4} · mag {mag_med:.4}. Churn: ne-oracle {ne_o_churn:.2} · ne-degraded {ne_d_churn:.2} · e1-k0 {e1k0_churn:.2}. Latency ne-oracle {ne_o_lat_med:.3} µs (record-only)."
    );
    println!();

    // --- Confirmatory verdict (prereg rule, from THIS run's 60..90 medians) -
    let sup_o = superior_count_b(&ne_oracle, &scalar);
    let sup_d = superior_count_b(&ne_deg, &scalar);
    let h1_o = ne_o_med >= 1.25 * mag_med;
    let h1_d = ne_d_med >= 1.25 * mag_med;
    let h3_o = sup_o >= 18;
    let h3_d = sup_d >= 18;
    let h2_o = ne_o_churn <= 68.0;
    let h2_d = ne_d_churn <= 68.0;
    let h2 = h2_o && h2_d;
    let oracle_pass = h1_o && h3_o;
    let degraded_pass = h1_d && h3_d;
    // Never-evict must give churn 0 (H2 by construction); a >68 median would be a
    // run-invalidating implementation bug, so it blocks VALIDATED.
    let verdict = if h2 {
        match (oracle_pass, degraded_pass) {
            (true, true) => "VALIDATED (v6)",
            (true, false) => "PARTIAL (signal-limited: oracle only)",
            (false, true) => "PARTIAL (signal-limited: degraded only)",
            (false, false) => "FALSIFIED (never-evict)",
        }
    } else {
        "FALSIFIED (never-evict)"
    };

    println!("## Confirmatory verdict (Scope B, seeds 60..90)");
    println!();
    println!(
        "- **H1(oracle) — quality vs mag:** ne-oracle {ne_o_med:.4} ≥ 1.25 × mag {mag_med:.4} (= {:.4}) → {}",
        1.25 * mag_med,
        pass(h1_o)
    );
    println!(
        "- **H1(degraded) — quality vs mag:** ne-degraded {ne_d_med:.4} ≥ 1.25 × mag {mag_med:.4} (= {:.4}) → {}",
        1.25 * mag_med,
        pass(h1_d)
    );
    println!(
        "- **H3(oracle) — mechanism vs scalar:** ne-oracle strictly superior to scalar in {sup_o}/30 ≥ 18 → {}",
        pass(h3_o)
    );
    println!(
        "- **H3(degraded) — mechanism vs scalar:** ne-degraded strictly superior to scalar in {sup_d}/30 ≥ 18 → {}",
        pass(h3_d)
    );
    println!(
        "- **H2 — churn ceiling ≤ 68 (absolute; by construction at c = 0):** ne-oracle {ne_o_churn:.2} ({}) · ne-degraded {ne_d_churn:.2} ({})",
        pass(h2_o),
        pass(h2_d)
    );
    println!();
    println!("**VERDICT (K4-v6, #56): {verdict}**");
    println!();
    println!(
        "_VALIDATED (v6) = H1 ∧ H3 under BOTH signals (∧ H2); PARTIAL (signal-limited) = H1 ∧ H3 under exactly one signal; FALSIFIED (never-evict) = anything less. Thresholds (1.25×, 18/30) inherit the v2→v5 family; churn ceiling 68 is absolute; nothing is tuned — lever, signal set, and bar were locked before implementation (prereg `docs/prereg-K4-v6-never-evict.md`)._"
    );
    println!();

    // --- Exploratory (non-gating) ------------------------------------------
    println!(
        "## Exploratory (non-gating): eviction-cap interpolation + rejoin lockout (degraded, 60..90)"
    );
    println!();
    println!("| condition | median PRIMARY_B | churn median |");
    println!("|-----------|----------------:|-------------:|");
    for c in [1u32, 2, 4] {
        let cfg = PersistentAifConfig {
            eviction_cap: Some(c),
            ..e1_config()
        };
        let (rs, _) = persistent_battery_range_degraded(cfg, 60, 90);
        println!(
            "| cap c={c} | {:.4} | {:.2} |",
            median(primaries_b(&rs)),
            median(churns_b(&rs))
        );
    }
    for k in [1u64, 2] {
        let cfg = PersistentAifConfig {
            rejoin_lockout_tasks: k,
            ..e1_config()
        };
        let (rs, _) = persistent_battery_range_degraded(cfg, 60, 90);
        println!(
            "| lockout k={k} | {:.4} | {:.2} |",
            median(primaries_b(&rs)),
            median(churns_b(&rs))
        );
    }
    println!();
    println!(
        "_non-gating; caps interpolate between never-evict (c = 0) and the k0 arm (unlimited), lockouts are the across-task state alternative (unlimited evictions). Informs whether any interior point merits a future registration — none is implied by this run._"
    );
    println!();
}

// ===========================================================================
// Part 5a — battery v2 core: γ × regime × margin factorial (koalisi #61, EQ1).
// THE REGISTERED RUN for lever 2 (de-saturation). Governed by
// `docs/prereg-K4-battery-v2.md` (committed + posted to #61 before
// implementation); seeds 120..150, degraded/L2 signal confirmatory; every
// threshold is THIS run's own 120..150 medians. Additive; every existing
// printed line byte-identical.
//
// The lever is `PersistentAifConfig::query_gamma` — the fixed softmax
// temperature over the query's policy EFE, live only on the MeanField path the
// registered `aif-e1` arm runs on. γ = 16 restates the engine default, so
// arm-E1g16 IS arm-E1 (asserted); γ ∈ {1, 4} flatten the policy posterior.
// ===========================================================================

/// Part 5a confirmatory seed range (150..180 stays held out for the
/// pre-committed replication).
const V2_SEED_START: u64 = 120;
const V2_SEED_END: u64 = 150;
/// Registered query-γ grid (`query_gamma`); 16 = the engine default = arm-E1.
const V2_GAMMA_GRID: [f64; 3] = [1.0, 4.0, 16.0];
/// Registered join-margin grid (join requires `p > 0.5 + δ`). Leave hysteresis
/// is h = 0 in every registered cell — h is exploratory-only in battery v2.
const V2_DELTA_GRID: [f64; 3] = [0.0, 0.15, 0.30];
/// H-S churn leg: a passing cell's churn median ≤ this × its δ = 0 baseline.
const HS_CHURN_FACTOR: f64 = 0.5;
/// H-S quality leg: a passing cell's PRIMARY_B median ≥ this × its baseline.
const HS_PRIMARY_FACTOR: f64 = 0.9;
/// H-S consistency leg: paired per-seed churn reductions needed (of 30).
const HS_PAIRED_MIN: usize = 18;
/// A score quantile at least this far from zero counts as saturated at ±0.5 —
/// the recorded (non-gating) mechanism observable behind the Part 4f NULL.
const SATURATION_BAND: f64 = 0.4999;

/// One factorial cell of the Part 5a grid: an arm config (γ), an instance
/// regime, and a join margin, scored over the confirmatory seed range.
struct V2Cell {
    gamma: f64,
    regime: Regime,
    delta: f64,
    primaries: Vec<f64>,
    churns: Vec<f64>,
}

impl V2Cell {
    fn primary_med(&self) -> f64 {
        median(self.primaries.clone())
    }
    fn churn_med(&self) -> f64 {
        median(self.churns.clone())
    }
}

/// The Part 5a arm config: the frozen `aif-e1` (K4-v5) shape with the EQ1
/// `query_gamma` lever set. The registered `aif-e1` config itself is untouched —
/// it keeps `query_gamma: None`.
fn e1_gamma_config(gamma: f64) -> PersistentAifConfig {
    PersistentAifConfig {
        query_gamma: Some(gamma),
        ..e1_config()
    }
}

/// Registered arm-config label for a γ cell (`arm-E1g1` / `arm-E1g4` /
/// `arm-E1g16`).
fn arm_label(gamma: f64) -> String {
    format!("arm-E1g{gamma:.0}")
}

fn regime_label(regime: Regime) -> &'static str {
    match regime {
        Regime::V1 => "v1-draw",
        Regime::V2 => "v2-draw",
        Regime::W12 => "w12-draw",
    }
}

/// Seeds on which `cell` churned strictly less than `base` (paired, same seed) —
/// the H-S consistency leg.
fn paired_churn_reduced(cell: &[f64], base: &[f64]) -> usize {
    (0..cell.len()).filter(|&i| cell[i] < base[i]).count()
}

/// Assert that two batteries are bit-identical on PRIMARY_B and churn per seed
/// (the Part 5a X-B identity gates).
fn assert_battery_identical(a: &[SeedResultB], b: &[SeedResultB], what: &str) {
    assert_eq!(a.len(), b.len(), "{what}");
    for i in 0..a.len() {
        assert!(
            a[i].primary.to_bits() == b[i].primary.to_bits() && a[i].churn == b[i].churn,
            "{what} (seed index {i})"
        );
    }
}

#[allow(clippy::too_many_lines)]
fn part5a_battery_v2() {
    println!("# koalisi #61 — Part 5a: battery v2 core, γ × regime × margin factorial (REGISTERED)");
    println!();
    println!(
        "_governed by `docs/prereg-K4-battery-v2.md` (committed + posted to #61 pre-implementation); lever 2 = de-saturation, CONFIRMATORY. Factorial γ ∈ {{1, 4, 16}} × regime ∈ {{v1-draw, v2-draw}} × join margin δ ∈ {{0, 0.15, 0.30}} (hysteresis h = 0 everywhere) over the registered `aif-e1` arm, Scope B · seeds **120..150** (out-of-sample; 150..180 held out for replication) · **degraded/L2 signal** confirmatory. The v2 draw is `|required| ∈ 2..=8` (v1: 1..=5) over the same 8-bit universe and the same pool draw. All bars are THIS run's own 120..150 medians._"
    );
    println!();

    // --- X-B(a) identity gate (run-invalidating) ---------------------------
    // `query_gamma: Some(16.0)` restates the engine default, so arm-E1g16 must
    // reproduce the frozen arm-E1 on the X-A cell bit-for-bit.
    let (g16, _) = persistent_battery_range(e1_gamma_config(16.0), 30, 60);
    let (g_none, _) = persistent_battery_range(e1_config(), 30, 60);
    assert_battery_identical(
        &g16,
        &g_none,
        "X-B(a) gate: query_gamma Some(16.0) must reproduce the engine default None",
    );
    println!(
        "**X-B(a) gate:** `query_gamma: Some(16.0)` reproduces the identity default `None` on the X-A cell (`aif-e1`, oracle, seeds 30..60) — PRIMARY_B + churn bit-identical on all {} seeds (asserted in-code).",
        g16.len()
    );
    println!();

    // --- Factorial cells (degraded signal, confirmatory) -------------------
    // δ = 0 cells additionally tap the raw decision scores (the saturation
    // observable) and are checked against the unwrapped arm — gate X-B(b).
    let mut cells: Vec<V2Cell> = Vec::new();
    let mut score_rows: Vec<(Regime, f64, Vec<f64>, Vec<f64>)> = Vec::new();
    for &regime in &[Regime::V1, Regime::V2] {
        for &gamma in &V2_GAMMA_GRID {
            let cfg = e1_gamma_config(gamma);
            let mode = RunMode {
                regime,
                degraded: true,
            };
            for (di, &delta) in V2_DELTA_GRID.iter().enumerate() {
                let tap = (di == 0).then(|| std::sync::Mutex::new(ScoreTap::default()));
                let (rs, _) = margin_battery_mode(
                    cfg,
                    delta,
                    0.0,
                    mode,
                    V2_SEED_START,
                    V2_SEED_END,
                    tap.as_ref(),
                );
                if let Some(t) = tap {
                    let (bare, _) =
                        persistent_battery_mode(cfg, mode, V2_SEED_START, V2_SEED_END);
                    assert_battery_identical(
                        &rs,
                        &bare,
                        "X-B(b) gate: the delta = 0 margin wrapper must reproduce the unwrapped arm",
                    );
                    let g = t.into_inner().expect("score tap poisoned");
                    score_rows.push((regime, gamma, g.join_scores, g.leave_scores));
                }
                cells.push(V2Cell {
                    gamma,
                    regime,
                    delta,
                    primaries: primaries_b(&rs),
                    churns: churns_b(&rs),
                });
            }
        }
    }
    println!(
        "**X-B(b) gate:** the `MarginE1(δ = 0, h = 0)` wrapper reproduces the unwrapped arm per seed in all {} (γ, regime) cells (PRIMARY_B + churn bit-identical; asserted in-code).",
        score_rows.len()
    );
    println!();

    let cell_at = |gamma: f64, regime: Regime, delta: f64| -> &V2Cell {
        cells
            .iter()
            .find(|c| {
                c.gamma.to_bits() == gamma.to_bits()
                    && c.regime == regime
                    && c.delta.to_bits() == delta.to_bits()
            })
            .expect("every factorial cell was run")
    };

    println!("## Factorial cells — degraded signal, seeds 120..150 (confirmatory)");
    println!();
    println!("| regime | arm | δ | median PRIMARY_B | churn median |");
    println!("|--------|-----|--:|-----------------:|-------------:|");
    for c in &cells {
        println!(
            "| {} | {} | {:.2} | {:.4} | {:.2} |",
            regime_label(c.regime),
            arm_label(c.gamma),
            c.delta,
            c.primary_med(),
            c.churn_med()
        );
    }
    println!();

    // --- In-run baselines (context rows, non-gating) -----------------------
    println!("## In-run baselines on the same instances (context, non-gating)");
    println!();
    println!("| regime | arm | median PRIMARY_B | churn median |");
    println!("|--------|-----|-----------------:|-------------:|");
    for &regime in &[Regime::V1, Regime::V2] {
        let (scalar, _) = stateless_battery_mode(
            || Box::new(AifDecisionPolicy::default()) as Box<dyn CoalitionDecisionPolicy>,
            regime,
            V2_SEED_START,
            V2_SEED_END,
        );
        let mag_policy = MagnitudePolicy::default();
        let (mag, _) = stateless_battery_mode(
            || Box::new(mag_policy.clone()) as Box<dyn CoalitionDecisionPolicy>,
            regime,
            V2_SEED_START,
            V2_SEED_END,
        );
        println!(
            "| {} | mag | {:.4} | {:.2} |",
            regime_label(regime),
            median(primaries_b(&mag)),
            median(churns_b(&mag))
        );
        println!(
            "| {} | scalar | {:.4} | {:.2} |",
            regime_label(regime),
            median(primaries_b(&scalar)),
            median(churns_b(&scalar))
        );
    }
    println!();

    // --- Saturation observable (record-only, non-gating) -------------------
    println!("## Decision-score quantiles at δ = 0 (record-only, non-gating)");
    println!();
    println!("| regime | arm | stream | n | p25 | p50 | p75 |");
    println!("|--------|-----|--------|--:|----:|----:|----:|");
    let mut desaturated_any = false;
    for (regime, gamma, join, leave) in &score_rows {
        for (stream, raw) in [("join", join), ("leave", leave)] {
            let mut s = raw.clone();
            s.sort_by(f64::total_cmp);
            let (q25, q50, q75) = (
                percentile(&s, 0.25),
                percentile(&s, 0.5),
                percentile(&s, 0.75),
            );
            if *regime == Regime::V2
                && gamma.to_bits() != 16.0f64.to_bits()
                && [q25, q50, q75].iter().any(|q| q.abs() < SATURATION_BAND)
            {
                desaturated_any = true;
            }
            println!(
                "| {} | {} | {} | {} | {:.4} | {:.4} | {:.4} |",
                regime_label(*regime),
                arm_label(*gamma),
                stream,
                s.len(),
                q25,
                q50,
                q75
            );
        }
    }
    println!();
    println!(
        "_linear-interpolated quantiles of the arm's raw `score = p(control 1) − 0.5`, collected on the δ = 0 cells. Part 4f measured every v1 γ = 16 quantile from p25 up at exactly 0.5000 (saturated), which is why no sub-0.5 margin could separate decisions there. A |quantile| < {SATURATION_BAND} in a (γ ∈ {{1, 4}}, v2-draw) row is the de-saturation this battery set out to create._"
    );
    println!();

    // --- H-S evaluation (prereg rule, from THIS run's 120..150 medians) ----
    println!("## H-S evaluation — lever 2 (γ ∈ {{1, 4}}, v2-draw, δ > 0 vs its own δ = 0)");
    println!();
    println!(
        "| arm | δ | churn med | base churn | ≤ 0.5× | PRIMARY_B med | base | ≥ 0.9× | paired churn↓ | ≥ 18/30 | cell |"
    );
    println!(
        "|-----|--:|----------:|-----------:|:------:|--------------:|-----:|:------:|--------------:|:-------:|:----:|"
    );
    let mut hs_pass = false;
    for &gamma in &[1.0f64, 4.0] {
        let base = cell_at(gamma, Regime::V2, 0.0);
        for &delta in &V2_DELTA_GRID[1..] {
            let cell = cell_at(gamma, Regime::V2, delta);
            let churn_leg = cell.churn_med() <= HS_CHURN_FACTOR * base.churn_med();
            let quality_leg = cell.primary_med() >= HS_PRIMARY_FACTOR * base.primary_med();
            let paired = paired_churn_reduced(&cell.churns, &base.churns);
            let paired_leg = paired >= HS_PAIRED_MIN;
            let cell_pass = churn_leg && quality_leg && paired_leg;
            hs_pass |= cell_pass;
            println!(
                "| {} | {:.2} | {:.2} | {:.2} | {} | {:.4} | {:.4} | {} | {}/30 | {} | {} |",
                arm_label(gamma),
                delta,
                cell.churn_med(),
                base.churn_med(),
                pass(churn_leg),
                cell.primary_med(),
                base.primary_med(),
                pass(quality_leg),
                paired,
                pass(paired_leg),
                pass(cell_pass)
            );
        }
    }
    println!();

    let verdict = if hs_pass {
        "VALIDATED (de-saturation)"
    } else {
        "FALSIFIED (de-saturation)"
    };
    println!("**VERDICT (K4 battery v2 — lever 2, #61): {verdict}**");
    println!();
    println!(
        "_VALIDATED (de-saturation) = there EXISTS a (γ ∈ {{1, 4}}, v2-draw, δ > 0) cell whose churn median ≤ {HS_CHURN_FACTOR:.1} × its own δ = 0 baseline AND whose PRIMARY_B median ≥ {HS_PRIMARY_FACTOR:.1} × that baseline AND whose per-seed churn reduction holds on ≥ {HS_PAIRED_MIN}/30 paired seeds; FALSIFIED (de-saturation) = anything less. Thresholds (0.5×, 0.9×, 18/30) inherit the v2→v6 family conventions; the grids, draw, signal, and bars were locked in the prereg before implementation._"
    );
    if !hs_pass {
        let scope = if desaturated_any {
            "the δ = 0 scores DID de-saturate in at least one (γ ∈ {1, 4}, v2-draw) row, so the falsification is about the margin lever itself, not about reaching a de-saturated regime"
        } else {
            "the (γ ∈ {1, 4}, v2-draw, δ = 0) scores are STILL saturated at ±0.5, so this falsification reads \"γ is not the de-saturation lever\" and is scoped accordingly"
        };
        println!();
        println!("_Scoping (prereg verdict rule): {scope}._");
    }
    println!();

    print_v2_oracle_twins();
}

/// Exploratory (non-gating, printed after the verdict): the Part 5a factorial
/// re-run under the per-bit ORACLE signal — the lever-3 pricing rows. Medians
/// only; nothing here is scored and no verdict depends on it.
fn print_v2_oracle_twins() {
    println!("## Exploratory (non-gating): oracle-signal twins of the Part 5a cells");
    println!();
    println!("| regime | arm | δ | median PRIMARY_B | churn median |");
    println!("|--------|-----|--:|-----------------:|-------------:|");
    for &regime in &[Regime::V1, Regime::V2] {
        for &gamma in &V2_GAMMA_GRID {
            let cfg = e1_gamma_config(gamma);
            let mode = RunMode {
                regime,
                degraded: false,
            };
            for &delta in &V2_DELTA_GRID {
                let (rs, _) =
                    margin_battery_mode(cfg, delta, 0.0, mode, V2_SEED_START, V2_SEED_END, None);
                println!(
                    "| {} | {} | {:.2} | {:.4} | {:.2} |",
                    regime_label(regime),
                    arm_label(gamma),
                    delta,
                    median(primaries_b(&rs)),
                    median(churns_b(&rs))
                );
            }
        }
    }
    println!();
    println!(
        "_lever 3 (oracle-vs-degraded signal fidelity) is EXPLORATORY by registration: these rows price the confirmatory cells against the per-bit oracle signal a runtime cannot emit. Any confirmatory claim about signal fidelity needs its own registration._"
    );
    println!();
}

// ===========================================================================
// Part 5b — reliability-routing test (koalisi #61, EQ1). THE REGISTERED RUN for
// lever 1 (routing). Governed by `docs/prereg-K4-battery-v2.md` §"Part 5b —
// reliability-routing test" + §H-R; seeds 120..150; PLANTED per-bit
// reliabilities (confirmatory cells use no learned beliefs — this is the
// mechanism test). Additive; every existing printed line byte-identical.
//
// The question: does reliability weighting make the population search route
// AROUND a weak required bit, or does it only rescale the same optimum
// (gotcha 24)? v1's fixed 15-per-bit partial weight inverted against the
// full-coverage weight at |required| ≥ 7 (15 > 100/7), so the prereg re-derives
// the model as size-normalized `w(m) = 80/m`.
// ===========================================================================

/// Part 5b full-coverage bonus (unchanged from the v1 `TaskCoverage`).
const V2B_FULL_BONUS: f64 = 100.0;
/// Part 5b partial-coverage budget: the per-bit weight is `w(m) = 80/m`.
const V2B_PARTIAL_BUDGET: f64 = 80.0;
/// Part 5b per-member cost (unchanged from v1).
const V2B_MEMBER_COST: f64 = 8.0;
/// Planted reliability of the single weak required bit `b*`.
const V2B_WEAK_R: f64 = 0.15;
/// Planted reliability of every other required bit.
const V2B_STRONG_R: f64 = 0.9;
/// H-R sanity leg: the unweighted argmax must cover all required bits on at
/// least this many of the 30 seeds, else the run is INVALID.
const HR_SANITY_MIN: usize = 27;
/// H-R skip / REAL-superiority legs — the family's 60% consistency bar.
const HR_CONSISTENCY_MIN: usize = 18;
/// H-R median bar: weighted median `REAL` ≥ this × the unweighted median.
const HR_REAL_FACTOR: f64 = 1.05;

/// Universe width as a `usize` (the reliability vector's length).
const UNIVERSE: usize = UNIVERSE_BITS as usize;

/// The Part 5b value model (prereg §5b): **size-normalized** coverage.
///
/// For a block `S` against `required` (`m = |required|`), per-bit reliabilities `r`:
///
/// - full coverage (`union(S) ⊇ required`): `100 · mean(r_b : b ∈ required)`
/// - partial coverage: `w(m) · Σ(r_b : b ∈ union(S) ∩ required)`, `w(m) = 80/m`
/// - minus `8` per member
///
/// The **unweighted** confirmatory model is this same type at `r ≡ 1`
/// ([`unweighted`](Self::unweighted)), so the two argmaxes the H-R legs compare
/// differ ONLY in the reliability vector — the prereg's requirement.
///
/// This is the `ReliabilityCoverage` SHAPE (`src/decision/reliability_value.rs`)
/// re-based on the v2 coefficients; the library type is untouched and keeps its
/// registered v1 constants (100 / 15 / 8).
struct TaskCoverageV2 {
    required: u32,
    reliability: [f64; UNIVERSE],
}

impl TaskCoverageV2 {
    /// The confirmatory unweighted model — `r ≡ 1`.
    fn unweighted(required: u32) -> Self {
        Self {
            required,
            reliability: [1.0; UNIVERSE],
        }
    }

    /// The reliability-weighted twin. Identical in every respect but `r`.
    fn weighted(required: u32, reliability: [f64; UNIVERSE]) -> Self {
        Self {
            required,
            reliability,
        }
    }

    /// `w(m) = 80/m`. `m = 0` cannot reach the partial branch (an empty
    /// requirement is trivially fully covered), so the `max(1)` is only a guard.
    fn partial_weight(&self) -> f64 {
        V2B_PARTIAL_BUDGET / f64::from(self.required.count_ones().max(1))
    }

    fn sum_reliability(&self, mask: u32) -> f64 {
        sum_reliability_of(mask, &self.reliability)
    }
}

/// Σ of `reliability[b]` over the bits set in `mask` (low [`UNIVERSE`] bits).
fn sum_reliability_of(mask: u32, reliability: &[f64; UNIVERSE]) -> f64 {
    (0..UNIVERSE)
        .filter(|b| mask & (1u32 << b) != 0)
        .map(|b| reliability[b])
        .sum()
}

impl ValueCalculator for TaskCoverageV2 {
    fn calculate_value(&self, agents: &[&dyn AgentCapabilities]) -> f64 {
        if agents.is_empty() {
            return 0.0;
        }
        let union = agents.iter().fold(0u32, |acc, a| acc | a.capabilities());
        let covered = union & self.required;

        let coverage = if covered == self.required {
            let m = self.required.count_ones();
            let mean = if m == 0 {
                1.0
            } else {
                self.sum_reliability(self.required) / f64::from(m)
            };
            V2B_FULL_BONUS * mean
        } else {
            self.partial_weight() * self.sum_reliability(covered)
        };

        coverage - agents.len() as f64 * V2B_MEMBER_COST
    }
}

/// One Part 5b instance: the pool, the requirement, the planted weak bit, and
/// the planted per-bit reliability vector.
struct RoutingInstance {
    agents: Vec<Worker>,
    required: u32,
    b_star: usize,
    reliability: [f64; UNIVERSE],
}

/// Draw one Part 5b instance off a FRESH `SplitMix64` stream (prereg §5b): pool
/// `n ∈ 8..=16` with caps `k ∈ 1..=4` distinct bits, `m = |required|` uniform on
/// `{7, 8}`, then `b*` uniform among the required bits. Reliabilities are
/// PLANTED — `r[b*] = 0.15`, every other required bit `0.9`.
///
/// This is its own stream (a new `SplitMix64` per seed), so it cannot perturb any
/// existing battery's draw schedule; the shared draw helpers are reused unchanged.
/// Entries for non-required bits are never read by the model (it only sums over
/// `required` and over subsets of it), and are left at the strong value.
fn draw_routing_instance(seed: u64) -> RoutingInstance {
    let mut rng = SplitMix64::new(seed);

    let n = (8 + rng.next_u64() % 9) as usize;
    let agents: Vec<Worker> = (0..n)
        .map(|id| {
            let k = 1 + rng.next_u64() % 4;
            let caps = draw_distinct_bits(&mut rng, k);
            let trust = (20 + rng.next_u64() % 80) as u32;
            Worker { id, caps, trust }
        })
        .collect();

    let m = 7 + rng.next_u64() % 2;
    let required = draw_distinct_bits(&mut rng, m);

    let req_bits: Vec<usize> = (0..UNIVERSE).filter(|b| required & (1u32 << b) != 0).collect();
    let b_star = req_bits[(rng.next_u64() % req_bits.len() as u64) as usize];

    let mut reliability = [V2B_STRONG_R; UNIVERSE];
    reliability[b_star] = V2B_WEAK_R;

    RoutingInstance {
        agents,
        required,
        b_star,
        reliability,
    }
}

/// `REAL(structure)` — the expected realized payoff at the UNWEIGHTED
/// `TaskCoverageV2` coefficients when each covered required bit `b` independently
/// succeeds with probability `r_b` and a failed bit counts as uncovered. This is
/// the yardstick that is NOT tautological w.r.t. either argmax: neither
/// calculator computes it.
///
/// Closed form, per block with covered set `C = union(S) ∩ required`:
///
/// ```text
/// E[block] = w(m)·Σ_{b∈C} r_b − 8·|S| + (C == required ? (100 − 80)·Π_{b∈required} r_b : 0)
/// ```
///
/// Derivation: the realized covered set `C' ⊆ C` has `E|C'| = Σ_{b∈C} r_b`, and
/// the block earns the full bonus only when `C' = required`, which requires
/// `C = required` AND every required bit to succeed — probability
/// `P = Π_{b∈required} r_b`. Splitting the expectation on that event gives
/// `E = 100·P + w(m)·(Σ_{b∈C} r_b − m·P) − 8|S|`; since `w(m)·m = 80`, the two
/// `P` terms collapse to the `(100 − 80)·P` residual above.
fn real_payoff(
    structure: &CoalitionStructure,
    agents: &[Worker],
    required: u32,
    reliability: &[f64; UNIVERSE],
) -> f64 {
    let m = required.count_ones();
    let w = V2B_PARTIAL_BUDGET / f64::from(m.max(1));
    let p_full: f64 = (0..UNIVERSE)
        .filter(|b| required & (1u32 << b) != 0)
        .map(|b| reliability[b])
        .product();

    structure
        .blocks()
        .iter()
        .map(|blk| {
            let union = blk.iter().fold(0u32, |acc, &i| acc | agents[i].caps);
            let covered = union & required;
            let full_term = if covered == required {
                (V2B_FULL_BONUS - V2B_PARTIAL_BUDGET) * p_full
            } else {
                0.0
            };
            w * sum_reliability_of(covered, reliability) - blk.len() as f64 * V2B_MEMBER_COST
                + full_term
        })
        .sum()
}

/// Union of the required bits any block of `structure` covers. Because
/// `search` returns a **partition of the whole pool**, this is identically the
/// pool's own coverage — see the structural note printed by Part 5b.
fn structure_required_coverage(
    structure: &CoalitionStructure,
    agents: &[Worker],
    required: u32,
) -> u32 {
    structure
        .blocks()
        .iter()
        .fold(0u32, |acc, blk| {
            acc | blk.iter().fold(0u32, |b, &i| b | agents[i].caps)
        })
        & required
}

/// Capability mask of `structure`'s **top block** — the block maximizing `calc`'s
/// own `calculate_value`.
///
/// This is the free-function form of the Part 5b exploratory diagnostic's local
/// closure (which is left untouched, so Part 5b's printed lines cannot move), and
/// it is the operational definition of "top block" the #63 registration amended
/// itself to: `max_by` with `partial_cmp`, whose tie-break keeps the **last**
/// maximal element in the structure's canonical block order — deterministic for a
/// fixed structure. Non-finite values compare `Equal` rather than panicking.
///
/// The returned mask is the block's raw capability union (NOT masked to
/// `required`), so callers test coverage with `mask & required` / `mask & bit`.
fn top_block_mask<C: ValueCalculator>(
    structure: &CoalitionStructure,
    agents: &[Worker],
    calc: &C,
) -> u32 {
    top_block(structure, agents, calc)
        .iter()
        .fold(0u32, |acc, &i| acc | agents[i].caps)
}

/// The top block itself (agent indices), under the same rule
/// [`top_block_mask`] uses. Empty for an empty structure.
fn top_block<C: ValueCalculator>(
    structure: &CoalitionStructure,
    agents: &[Worker],
    calc: &C,
) -> Vec<usize> {
    structure
        .blocks()
        .into_iter()
        .max_by(|a, b| {
            let va = calc.calculate_value(&coalition_view(agents, a));
            let vb = calc.calculate_value(&coalition_view(agents, b));
            va.partial_cmp(&vb).unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or_default()
}

/// The two coefficient properties the prereg requires ASSERTED before the run.
///
/// 1. At `r ≡ 1` the optimum covers all required bits for every `m ∈ 2..=8`
///    (`w(m) = 80/m < 100/m` strictly, so full coverage always outscores partial
///    at equal member count) — verified through the real `search` on a pool of
///    `m` pure specialists.
/// 2. A sufficiently weak bit flips the optimum to skipping it. This holds at the
///    **block** level — comparing a full-coverage block against the
///    one-member-smaller block that omits `b*` — which is the only level at which
///    it can hold: over a *partition* of a fixed pool the `8·N` member cost is
///    constant, so "skipping" never buys back a member. Asserted as the exact
///    algebraic condition `8m > 20·Σ_{b≠b*} r_b + 100·r[b*]`.
fn assert_v2b_coefficient_properties() {
    for m in 2u32..=8 {
        let required = (1u32 << m) - 1;
        let agents: Vec<Worker> = (0..m as usize)
            .map(|b| Worker {
                id: b,
                caps: 1u32 << b,
                trust: 50,
            })
            .collect();
        let calc = TaskCoverageV2::unweighted(required);
        let cfg = PopulationConfig::default().with_seed(u64::from(m));
        let best = search(&agents, &calc, &cfg).best;
        assert_eq!(
            structure_required_coverage(&best, &agents, required),
            required,
            "r = 1: the optimum must cover every required bit at m = {m}"
        );
        assert_eq!(
            best.blocks().len(),
            1,
            "r = 1: the optimum must be a single full-coverage block at m = {m}"
        );
    }

    // Property 2 — the block-level flip, at m = 8 with a uniformly weak pool.
    let m = 8.0f64;
    let r_star = 0.0f64;
    let others = 0.4f64;
    let s: f64 = 7.0 * others;
    assert!(
        8.0 * m > 20.0 * s + 100.0 * r_star,
        "a sufficiently weak bit must flip the block-level optimum to skipping it"
    );
    // ...and the planted (0.9 / 0.15) values do NOT satisfy it — recorded, not a
    // failure: it is the measured content of the H-R result.
    let planted_s = 7.0 * V2B_STRONG_R;
    assert!(
        8.0 * m <= 20.0 * planted_s + 100.0 * V2B_WEAK_R,
        "the planted reliabilities are expected NOT to flip the block-level optimum"
    );
}

#[allow(clippy::too_many_lines)]
fn part5b_reliability_routing() {
    println!("# koalisi #61 — Part 5b: reliability-routing test (REGISTERED)");
    println!();
    println!(
        "_governed by `docs/prereg-K4-battery-v2.md` §\"Part 5b\" + §H-R; lever 1 = routing, CONFIRMATORY. Value model `TaskCoverageV2` — full-coverage bonus 100, partial per-bit weight `w(m) = 80/m`, member cost 8·N — and its reliability-weighted twin (the `ReliabilityCoverage` shape re-based on these coefficients; the library type is untouched). Per seed: pool `n ∈ 8..=16` (caps 1..=4 bits), `m = |required|` uniform on {{7, 8}}, one uniformly-chosen required bit `b*` PLANTED at r = 0.15 with every other required bit at 0.9. Both argmaxes come from `search()` at the same pinned `PopulationConfig` and the same seed, differing ONLY in the calculator. Seeds **120..150**._"
    );
    println!();

    assert_v2b_coefficient_properties();
    println!(
        "**Coefficient gate:** at `r ≡ 1` the `search()` optimum is a single full-coverage block for every `m ∈ 2..=8` (`w(m) = 80/m < 100/m`), and the block-level skip condition `8m > 20·Σ_{{b≠b*}} r_b + 100·r[b*]` is satisfiable for a sufficiently weak pool — both asserted in-code before the run."
    );
    println!();

    // --- Per-seed confirmatory run ----------------------------------------
    let mut rows: Vec<(u64, u32, usize, bool, f64, f64)> = Vec::new();
    let mut sanity_ok = 0usize;
    let mut skipped = 0usize;
    let mut real_superior = 0usize;
    let mut reals_w: Vec<f64> = Vec::new();
    let mut reals_u: Vec<f64> = Vec::new();
    // Exploratory (non-gating): does the argmax's single best-valued block omit b*?
    let mut best_block_routed = 0usize;

    for seed in V2_SEED_START..V2_SEED_END {
        let inst = draw_routing_instance(seed);
        let cfg = PopulationConfig::default().with_seed(seed);

        let unweighted = search(
            &inst.agents,
            &TaskCoverageV2::unweighted(inst.required),
            &cfg,
        )
        .best;
        let weighted = search(
            &inst.agents,
            &TaskCoverageV2::weighted(inst.required, inst.reliability),
            &cfg,
        )
        .best;

        let u_cov = structure_required_coverage(&unweighted, &inst.agents, inst.required);
        if u_cov == inst.required {
            sanity_ok += 1;
        }

        let w_cov = structure_required_coverage(&weighted, &inst.agents, inst.required);
        let b_star_bit = 1u32 << inst.b_star;
        let skips = w_cov & b_star_bit == 0;
        if skips {
            skipped += 1;
        }

        let real_w = real_payoff(&weighted, &inst.agents, inst.required, &inst.reliability);
        let real_u = real_payoff(&unweighted, &inst.agents, inst.required, &inst.reliability);
        if real_w > real_u {
            real_superior += 1;
        }
        reals_w.push(real_w);
        reals_u.push(real_u);

        // Exploratory: the highest-value block under each model.
        let top_block = |s: &CoalitionStructure, calc: &TaskCoverageV2| -> u32 {
            s.blocks()
                .iter()
                .max_by(|a, b| {
                    let va = calc.calculate_value(&coalition_view(&inst.agents, a));
                    let vb = calc.calculate_value(&coalition_view(&inst.agents, b));
                    va.partial_cmp(&vb).unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|blk| blk.iter().fold(0u32, |acc, &i| acc | inst.agents[i].caps))
                .unwrap_or(0)
        };
        let w_top = top_block(
            &weighted,
            &TaskCoverageV2::weighted(inst.required, inst.reliability),
        );
        let u_top = top_block(&unweighted, &TaskCoverageV2::unweighted(inst.required));
        if w_top & b_star_bit == 0 && u_top & b_star_bit != 0 {
            best_block_routed += 1;
        }

        rows.push((
            seed,
            inst.required.count_ones(),
            inst.b_star,
            skips,
            real_w,
            real_u,
        ));
    }

    println!("## Per-seed argmax comparison (seeds 120..150)");
    println!();
    println!("| seed | m | b* | weighted skips b* | REAL_w | REAL_u |");
    println!("|-----:|--:|---:|:-----------------:|-------:|-------:|");
    for (seed, m, b_star, skips, real_w, real_u) in &rows {
        println!(
            "| {} | {} | {} | {} | {:.4} | {:.4} |",
            seed,
            m,
            b_star,
            if *skips { "yes" } else { "no" },
            real_w,
            real_u
        );
    }
    println!();

    let med_w = median(reals_w.clone());
    let med_u = median(reals_u.clone());
    println!("**Medians (120..150):** REAL_w {med_w:.4} · REAL_u {med_u:.4}.");
    println!();

    // --- H-R evaluation ----------------------------------------------------
    let sanity_leg = sanity_ok >= HR_SANITY_MIN;
    let skip_leg = skipped >= HR_CONSISTENCY_MIN;
    let real_sup_leg = real_superior >= HR_CONSISTENCY_MIN;
    let real_med_leg = med_w >= HR_REAL_FACTOR * med_u;
    let real_leg = real_sup_leg && real_med_leg;

    println!("## H-R evaluation — lever 1 (routing)");
    println!();
    println!(
        "- **Sanity leg (run-invalidating):** the unweighted argmax covers every required bit (incl. `b*`) on {sanity_ok}/30 ≥ {HR_SANITY_MIN} → {}",
        pass(sanity_leg)
    );
    println!(
        "- **Skip leg:** the weighted argmax skips `b*` (no block covers it) on {skipped}/30 ≥ {HR_CONSISTENCY_MIN} → {}",
        pass(skip_leg)
    );
    println!(
        "- **REAL leg:** weighted `REAL` strictly greater on {real_superior}/30 ≥ {HR_CONSISTENCY_MIN} ({}) AND median REAL_w {med_w:.4} ≥ {HR_REAL_FACTOR} × REAL_u {med_u:.4} (= {:.4}) ({}) → {}",
        pass(real_sup_leg),
        HR_REAL_FACTOR * med_u,
        pass(real_med_leg),
        pass(real_leg)
    );
    println!();

    if sanity_leg {
        let verdict = if skip_leg && real_leg {
            "VALIDATED (routing)"
        } else {
            "FALSIFIED (routing)"
        };
        println!("**VERDICT (K4 battery v2 — lever 1, #61): {verdict}**");
    } else {
        println!("**VERDICT (K4 battery v2 — lever 1, #61): RUN-INVALID (sanity leg)**");
    }
    println!();
    println!(
        "_VALIDATED (routing) = the skip leg AND the REAL leg, with the sanity leg intact; FALSIFIED (routing) = anything less; a sanity-leg failure invalidates the run rather than producing a verdict. Bars (27/30, 18/30, 1.05×) were locked in the prereg before implementation._"
    );
    println!();

    // --- Structural note (observed mechanism; non-gating) ------------------
    println!("## Structural note on the skip leg (observed, non-gating)");
    println!();
    println!(
        "`search()` returns a **partition of the entire pool** (`assignment[i]` is defined for every agent index `i`), so the union of the blocks' capabilities is identically the union of the whole pool. \"No block covers `b*`\" is therefore equivalent to \"no agent in the pool provides `b*`\" — it cannot depend on the calculator at all. The same identity makes the sanity leg a statement about pool coverage rather than about the argmax, and it makes the two legs **mutually exclusive**: whenever the sanity leg holds (the pool covers every required bit, `b*` included), the skip leg is 0/30 by construction. Measured this run: sanity {sanity_ok}/30, skip {skipped}/30."
    );
    println!();
    println!(
        "Related: over a partition of a fixed pool the `8·N` member-cost term sums to the same constant for every structure, so skipping a bit never buys back a member — the block-level flip condition asserted by the coefficient gate cannot express itself at the partition level. Reliability re-ranks structures only through *which* bits the partial blocks cover."
    );
    println!();
    println!(
        "**Exploratory (non-gating, not a registered criterion):** the weaker block-level reading of the same question — the weighted argmax's single highest-value block omits `b*` while the unweighted argmax's does not — holds on {best_block_routed}/30 seeds."
    );
    println!();
}

// ===========================================================================
// Part 5c — registered exploratory addendum (koalisi #61, EQ1). Registered in
// `docs/prereg-K4-battery-v2.md` §"Part 5c — exploratory only", which fixes the
// SCOPE of these four items and nothing about their outcome: everything printed
// below is EXPLORATORY and NON-GATING — no verdict is derived here, no
// confirmatory threshold is evaluated, and no registered section is edited.
// Additive: every Part 1–5b printed line is unchanged.
//
//   Item 1 — the 12-bit widened slice (`w12-draw`), on the koalisi-side
//            `PersistentAifConfig::n_bits` parameterization.
//   Item 2 — leave-side hysteresis h ∈ {0.15, 0.30}.
//   Item 3 — the expected-outcome value model, gated on its own gotcha-21
//            degeneracy analysis.
//   Item 4 — learned-posterior twins of the Part 5b routing comparison.
//
// (Lever 3, oracle-vs-degraded pricing, is the fifth registered Part 5c item and
// already ran with Part 5a — see `print_v2_oracle_twins`.)
// ===========================================================================

/// Item 1: γ of the registered widened cell (`arm-E1g4`, δ = 0, degraded).
const P5C_W12_GAMMA: f64 = 4.0;
/// Item 2: the registered leave-side hysteresis grid.
const P5C_H_GRID: [f64; 2] = [0.15, 0.30];
/// Item 2: γ of the cell the hysteresis sweep runs at — see the printed
/// cell-selection rationale (the most de-saturated LEAVE stream, which is the
/// stream this lever acts on).
const P5C_HYSTERESIS_GAMMA: f64 = 1.0;
/// Item 4: tasks of synthetic outcome stream fed to each twin arm before its
/// posterior is read. Matches the batteries' own [`TASKS`] stream length.
const P5C_TWIN_TASKS: usize = TASKS;
/// Item 4: salt for the twin outcome stream's seed. The stream is a SEPARATE
/// `SplitMix64` (`SplitMix64::new(seed ^ salt)`), so `draw_routing_instance`'s
/// own draw schedule is untouched and Part 5b stays byte-identical.
const P5C_TWIN_SEED_SALT: u64 = 0x5C17_0000_0000_0000;

/// The Part 5c item-1 arm config: the frozen `aif-e1` shape at the EQ1 γ lever,
/// widened to the 12-bit world model the `w12-draw` regime needs. `n_bits` is the
/// only field beyond `e1_gamma_config`'s, and at its identity default (8) that
/// function is unchanged — so every Part 5a cell is untouched.
fn e1_w12_config(gamma: f64) -> PersistentAifConfig {
    PersistentAifConfig {
        n_bits: W12_UNIVERSE_BITS as usize,
        ..e1_gamma_config(gamma)
    }
}

/// Linear-interpolated (p25, p50, p75) of a raw score sample.
fn score_quartiles(raw: &[f64]) -> (f64, f64, f64) {
    let mut s = raw.to_vec();
    s.sort_by(f64::total_cmp);
    (
        percentile(&s, 0.25),
        percentile(&s, 0.5),
        percentile(&s, 0.75),
    )
}

/// Item 1 — the 12-bit widened slice.
fn part5c_item1_w12_slice() {
    println!("## Item 1 — 12-bit widened slice (`w12-draw`)");
    println!();
    println!(
        "_registered cell: `arm-E1g4` (γ = {P5C_W12_GAMMA:.0}), δ = 0, **degraded** signal, seeds {V2_SEED_START}..{V2_SEED_END}, on a **12-bit** universe — `|required|` uniform 2..=12, worker caps 1..=6, pool `n ∈ 4..=16` as in the v2 draw. The arm runs at `PersistentAifConfig::n_bits = 12` (persistent joint space 4096); at the identity default 8 the same code path is the registered arm bit-for-bit. `mag` / `scalar` context rows run on the SAME 12-bit instances. Exploratory: no bar, no verdict._"
    );
    println!();

    let cfg = e1_w12_config(P5C_W12_GAMMA);
    let mode = RunMode {
        regime: Regime::W12,
        degraded: true,
    };
    let tap = std::sync::Mutex::new(ScoreTap::default());
    let (e1, e1_lat) = margin_battery_mode(
        cfg,
        0.0,
        0.0,
        mode,
        V2_SEED_START,
        V2_SEED_END,
        Some(&tap),
    );

    let (scalar, _) = stateless_battery_mode(
        || Box::new(AifDecisionPolicy::default()) as Box<dyn CoalitionDecisionPolicy>,
        Regime::W12,
        V2_SEED_START,
        V2_SEED_END,
    );
    let mag_policy = MagnitudePolicy::default();
    let (mag, _) = stateless_battery_mode(
        || Box::new(mag_policy.clone()) as Box<dyn CoalitionDecisionPolicy>,
        Regime::W12,
        V2_SEED_START,
        V2_SEED_END,
    );

    println!("| regime | arm | δ | median PRIMARY_B | churn median |");
    println!("|--------|-----|--:|-----------------:|-------------:|");
    for (label, delta, rs) in [
        (arm_label(P5C_W12_GAMMA), "0.00", &e1),
        ("mag".to_owned(), "—", &mag),
        ("scalar".to_owned(), "—", &scalar),
    ] {
        println!(
            "| {} | {} | {} | {:.4} | {:.2} |",
            regime_label(Regime::W12),
            label,
            delta,
            median(primaries_b(rs)),
            median(churns_b(rs))
        );
    }
    println!();
    println!(
        "Latency `arm-E1g4` on the 12-bit slice: {:.3} µs/decision median (record-only — the query joint space is `2^(|required|+1)`, up to 8192 here vs 512 at the v2 draw).",
        median(e1_lat)
    );
    println!();

    let (join, leave) = {
        let g = tap.lock().expect("score tap poisoned");
        (g.join_scores.clone(), g.leave_scores.clone())
    };
    println!("### Decision-score quantiles on the same cell (mechanism observable)");
    println!();
    println!("| stream | n | p25 | p50 | p75 |");
    println!("|--------|--:|----:|----:|----:|");
    for (stream, raw) in [("join", &join), ("leave", &leave)] {
        let (q25, q50, q75) = score_quartiles(raw);
        println!(
            "| {} | {} | {:.4} | {:.4} | {:.4} |",
            stream,
            raw.len(),
            q25,
            q50,
            q75
        );
    }
    println!();
    println!(
        "_same `score = p(control 1) − 0.5` tap as Part 5a's δ = 0 rows. Gotcha 25 records the 8-bit finding — the JOIN rail sits at exactly +0.5 (p = 1.0) in every measured (γ, regime) cell while γ de-saturates only the LEAVE stream — so these rows say whether a wider universe changes that, which no 8-bit cell could answer._"
    );
    println!();
}

/// Item 2 — leave-side hysteresis at the most de-saturated cell.
fn part5c_item2_hysteresis() {
    println!("## Item 2 — leave-side hysteresis h ∈ {{0.15, 0.30}}");
    println!();
    println!(
        "_**Cell selection** (stated because it lands in the report — BOTH restrictions are interpretations of the registered \"at the best-performing (γ, δ) cell\"): first, the search is restricted to the **v2-draw** regime — v1-draw cells score higher in absolute `PRIMARY_B` (≈ 0.386), but lever 2 (which this lever follows on) was registered and judged on v2-draw, and the v1-draw leave streams barely de-saturate at any γ — and second, at δ = 0 all three γ tie on v2-draw `PRIMARY_B` in the Part 5a table, so the tie is broken by the mechanism this lever acts on: hysteresis raises the bar a LEAVE score must clear, and the most de-saturated LEAVE stream is **γ = {P5C_HYSTERESIS_GAMMA:.0}** (leave p25 well inside ±0.5 while γ = 16 sits on the rail). Cell: `MarginE1(δ = 0, h)` over `arm-E1g1`, v2-draw, degraded, seeds {V2_SEED_START}..{V2_SEED_END}. The h = 0 row is re-run in-line as this sweep's own paired baseline; it is asserted in-code to reproduce the unwrapped arm, which gate X-B(b) pinned equal to the Part 5a γ = 1, δ = 0 cell. Exploratory: no bar, no verdict._"
    );
    println!();

    let cfg = e1_gamma_config(P5C_HYSTERESIS_GAMMA);
    let mode = RunMode {
        regime: Regime::V2,
        degraded: true,
    };
    let (base, _) = margin_battery_mode(cfg, 0.0, 0.0, mode, V2_SEED_START, V2_SEED_END, None);
    // The baseline-reproduction claim in the rationale above, made a fact: the
    // h = 0 wrapper equals the unwrapped arm per seed (X-B(b) pinned that arm
    // equal to the Part 5a γ = 1, δ = 0 cell, so equality is transitive).
    let (bare, _) = persistent_battery_mode(cfg, mode, V2_SEED_START, V2_SEED_END);
    assert_battery_identical(
        &base,
        &bare,
        "item 2 h = 0 baseline must reproduce the unwrapped arm",
    );
    let base_churns = churns_b(&base);
    let base_churn_med = median(base_churns.clone());
    let base_primary_med = median(primaries_b(&base));

    println!("| arm | h | median PRIMARY_B | churn median | vs base churn | paired churn↓ |");
    println!("|-----|--:|-----------------:|-------------:|--------------:|--------------:|");
    println!(
        "| {} | 0.00 | {:.4} | {:.2} | 1.00× | — |",
        arm_label(P5C_HYSTERESIS_GAMMA),
        base_primary_med,
        base_churn_med
    );
    for &h in &P5C_H_GRID {
        let (rs, _) = margin_battery_mode(cfg, 0.0, h, mode, V2_SEED_START, V2_SEED_END, None);
        let churns = churns_b(&rs);
        let churn_med = median(churns.clone());
        let ratio = if base_churn_med > 0.0 {
            churn_med / base_churn_med
        } else {
            f64::NAN
        };
        println!(
            "| {} | {:.2} | {:.4} | {:.2} | {:.2}× | {}/{} |",
            arm_label(P5C_HYSTERESIS_GAMMA),
            h,
            median(primaries_b(&rs)),
            churn_med,
            ratio,
            paired_churn_reduced(&churns, &base_churns),
            base_churns.len()
        );
    }
    println!();
    println!(
        "_`MarginE1` evicts iff the arm evicts AND `score ≥ h`, so h tightens the leave rail at each individual consult — but churn is NOT monotone in h across the run: a suppressed eviction changes membership, which changes every later decision and the outcome the arm learns from. What the rows measure is the net effect and its PRICE in `PRIMARY_B`. The H-S bars (0.5× churn at ≥ 0.9× quality on ≥ 18/30 paired seeds) are NOT applied here: h was registered as exploratory, and lever 2's verdict is already closed on the join-margin grid._"
    );
    println!();
}

/// The Part 5c item-3 value model (EXPLORATORY): a block's fitness IS its
/// closed-form expected realized payoff — literally the per-block term of
/// [`real_payoff`] at the `TaskCoverageV2` coefficients:
///
/// ```text
/// E[block] = w(m)·Σ_{b∈C} r_b − 8·|S| + (C == required ? (100 − 80)·Π_{b∈required} r_b : 0)
/// ```
///
/// So `search()` under this calculator optimizes the REAL yardstick directly,
/// where `TaskCoverageV2::weighted` optimizes a nominal proxy for it. The
/// identity `Σ over blocks of calculate_value == real_payoff` is asserted before
/// the run (`assert_p5c_expected_outcome_identity`).
///
/// `required == 0` is out of contract (shared by the whole `TaskCoverageV2` /
/// [`real_payoff`] family): an empty requirement makes every block vacuously
/// full-covered, so each block earns the flat residual and the "value" grows
/// with the block COUNT. Unreachable here — `draw_routing_instance` draws
/// `m ∈ {7, 8}` — but a future caller must guard it.
struct ExpectedOutcomeV2 {
    required: u32,
    reliability: [f64; UNIVERSE],
    /// `Π_{b∈required} r_b`, precomputed (constant per instance).
    p_full: f64,
}

impl ExpectedOutcomeV2 {
    fn new(required: u32, reliability: [f64; UNIVERSE]) -> Self {
        let p_full: f64 = (0..UNIVERSE)
            .filter(|b| required & (1u32 << b) != 0)
            .map(|b| reliability[b])
            .product();
        Self {
            required,
            reliability,
            p_full,
        }
    }
}

impl ValueCalculator for ExpectedOutcomeV2 {
    fn calculate_value(&self, agents: &[&dyn AgentCapabilities]) -> f64 {
        if agents.is_empty() {
            return 0.0;
        }
        let union = agents.iter().fold(0u32, |acc, a| acc | a.capabilities());
        let covered = union & self.required;
        let w = V2B_PARTIAL_BUDGET / f64::from(self.required.count_ones().max(1));
        let full_term = if covered == self.required {
            (V2B_FULL_BONUS - V2B_PARTIAL_BUDGET) * self.p_full
        } else {
            0.0
        };
        w * sum_reliability_of(covered, &self.reliability) - agents.len() as f64 * V2B_MEMBER_COST
            + full_term
    }
}

/// Σ of `calc` over a partition given as explicit blocks.
fn blocks_fitness<C: ValueCalculator>(blocks: &[Vec<usize>], agents: &[Worker], calc: &C) -> f64 {
    blocks
        .iter()
        .map(|blk| calc.calculate_value(&coalition_view(agents, blk)))
        .sum()
}

/// [`ExpectedOutcomeV2`] summed over a structure's blocks must equal
/// [`real_payoff`] on that structure exactly — the two are the same closed form,
/// and this is what makes "the search optimizes REAL directly" a fact rather than
/// a claim. Asserted on the confirmatory seeds before item 3 runs.
fn assert_p5c_expected_outcome_identity() {
    for seed in V2_SEED_START..V2_SEED_END {
        let inst = draw_routing_instance(seed);
        let cfg = PopulationConfig::default().with_seed(seed);
        let calc = ExpectedOutcomeV2::new(inst.required, inst.reliability);
        let best = search(&inst.agents, &calc, &cfg).best;
        let via_blocks = blocks_fitness(&best.blocks(), &inst.agents, &calc);
        let via_real = real_payoff(&best, &inst.agents, inst.required, &inst.reliability);
        assert!(
            (via_blocks - via_real).abs() < 1e-9,
            "ExpectedOutcomeV2 must sum to real_payoff (seed {seed}): {via_blocks} vs {via_real}"
        );
    }
}

/// Item 3 — expected-outcome value model: degeneracy analysis, then the Part 5b
/// re-run gated on it.
#[allow(clippy::too_many_lines)]
fn part5c_item3_expected_outcome() {
    println!("## Item 3 — expected-outcome value model (degeneracy analysis, then re-run)");
    println!();
    println!(
        "_the registered item: re-run the Part 5b comparison with success-probability semantics — a block's fitness IS its expected realized payoff (`w(m)·Σ_{{b∈C}} r_b − 8|S| + [C = required]·20·Π_{{b∈required}} r_b`, the closed form the L2 `TaskOutcome` event's success probability induces), so `search()` optimizes REAL directly. **Gated on its own gotcha-21 degeneracy analysis**, which runs first. Same `draw_routing_instance` seeds {V2_SEED_START}..{V2_SEED_END} as the registered Part 5b — the pool-coverage gap is deliberately NOT fixed here (that belongs to #63); this is a like-for-like calculator swap._"
    );
    println!();

    assert_p5c_expected_outcome_identity();
    println!(
        "**Identity gate:** `Σ over blocks of ExpectedOutcomeV2 == real_payoff(structure)` on all 30 argmaxes (asserted in-code) — the search really is maximizing the REAL yardstick, not a proxy for it."
    );
    println!();

    // --- Degeneracy analysis (gotcha 21) ----------------------------------
    println!("### Degeneracy analysis (gotcha 21, runs BEFORE the comparison)");
    println!();
    println!(
        "Structure of the model over a partition of a fixed pool: the member term sums to the partition-constant `−8·N`; the partial term `w(m)·Σ_{{b∈C}} r_b` is earned **per block**, so merging two blocks with covered sets `C₁, C₂` LOSES `w(m)·Σ_{{b∈C₁∩C₂}} r_b`; the only counterweight is the full-coverage residual `20·Π_{{b∈required}} r_b`, earned per full-coverage block. Splitting is therefore weakly optimal unless a merge creates full coverage worth more than the overlap it destroys. At the planted reliabilities `Π r` is tiny, so the prediction is **all-singletons**."
    );
    println!();

    let mut singleton_argmax = 0usize;
    let mut singleton_ge_search = 0usize;
    let mut deg_rows: Vec<(u64, usize, usize, f64, f64, f64)> = Vec::new();
    for seed in V2_SEED_START..V2_SEED_END {
        let inst = draw_routing_instance(seed);
        let n = inst.agents.len();
        let cfg = PopulationConfig::default().with_seed(seed);
        let calc = ExpectedOutcomeV2::new(inst.required, inst.reliability);
        let best = search(&inst.agents, &calc, &cfg).best;

        let singletons: Vec<Vec<usize>> = (0..n).map(|i| vec![i]).collect();
        let one_block: Vec<Vec<usize>> = vec![(0..n).collect()];
        let f_singletons = blocks_fitness(&singletons, &inst.agents, &calc);
        let f_one = blocks_fitness(&one_block, &inst.agents, &calc);
        let f_best = blocks_fitness(&best.blocks(), &inst.agents, &calc);

        if best.blocks().len() == n {
            singleton_argmax += 1;
        }
        if f_singletons >= f_best - 1e-9 {
            singleton_ge_search += 1;
        }
        deg_rows.push((seed, n, best.blocks().len(), f_singletons, f_one, f_best));
    }

    println!("| seed | n | argmax blocks | fitness(all-singletons) | fitness(one block) | fitness(argmax) |");
    println!("|-----:|--:|--------------:|------------------------:|-------------------:|----------------:|");
    for (seed, n, blocks, f_s, f_1, f_b) in &deg_rows {
        println!("| {seed} | {n} | {blocks} | {f_s:.4} | {f_1:.4} | {f_b:.4} |");
    }
    println!();
    let n_seeds = deg_rows.len();
    let analysis = if singleton_ge_search == n_seeds {
        "DEGENERATE — all-singletons is optimal on every seed, so the model has no interior optimum over set-partitions of a fixed pool"
    } else if 2 * singleton_ge_search > n_seeds {
        "MOSTLY DEGENERATE — all-singletons is optimal on a strict majority of seeds; the exceptions are the merges whose full-coverage residual outweighs the overlap they destroy"
    } else {
        "NON-degenerate on most seeds — an interior optimum exists here"
    };
    println!(
        "**Analysis result:** the `search()` argmax is all-singletons on **{singleton_argmax}/{n_seeds}** seeds, and all-singletons matches or beats the argmax on **{singleton_ge_search}/{n_seeds}**. Verdict of the analysis: **{analysis}**."
    );
    println!();
    if 2 * singleton_ge_search > n_seeds {
        println!(
            "_Per the registered gating (\"gated on its own gotcha-21 degeneracy analysis\"), the comparison below is therefore reported as **degenerate-by-analysis**: the numbers are CONTEXT, not evidence about routing. This model joins `Additive` (constant across partitions) and `Synergistic`/`Multiplicative` (split-favouring) on the gotcha-21 list, by a third mechanism — the per-block partial term double-counts a bit that two blocks both cover, so splitting pays, and the full-coverage residual `20·Π r` is far too small at the planted reliabilities to buy the overlap back. The productive response is a value model with an interior optimum, not a re-run of this one._"
        );
        println!();
    }

    // --- Part 5b re-run under the expected-outcome model -------------------
    println!("### Re-run of the Part 5b comparison (context only)");
    println!();

    let mut rows: Vec<(u64, u32, usize, bool, f64, f64)> = Vec::new();
    let mut skipped = 0usize;
    let mut real_superior = 0usize;
    let mut reals_e: Vec<f64> = Vec::new();
    let mut reals_u: Vec<f64> = Vec::new();
    for seed in V2_SEED_START..V2_SEED_END {
        let inst = draw_routing_instance(seed);
        let cfg = PopulationConfig::default().with_seed(seed);

        let unweighted = search(
            &inst.agents,
            &TaskCoverageV2::unweighted(inst.required),
            &cfg,
        )
        .best;
        let expected = search(
            &inst.agents,
            &ExpectedOutcomeV2::new(inst.required, inst.reliability),
            &cfg,
        )
        .best;

        let e_cov = structure_required_coverage(&expected, &inst.agents, inst.required);
        let b_star_bit = 1u32 << inst.b_star;
        let skips = e_cov & b_star_bit == 0;
        if skips {
            skipped += 1;
        }

        let real_e = real_payoff(&expected, &inst.agents, inst.required, &inst.reliability);
        let real_u = real_payoff(&unweighted, &inst.agents, inst.required, &inst.reliability);
        if real_e > real_u {
            real_superior += 1;
        }
        reals_e.push(real_e);
        reals_u.push(real_u);
        rows.push((
            seed,
            inst.required.count_ones(),
            inst.b_star,
            skips,
            real_e,
            real_u,
        ));
    }

    println!("| seed | m | b* | expected-outcome skips b* | REAL_e | REAL_u |");
    println!("|-----:|--:|---:|:-------------------------:|-------:|-------:|");
    for (seed, m, b_star, skips, real_e, real_u) in &rows {
        println!(
            "| {} | {} | {} | {} | {:.4} | {:.4} |",
            seed,
            m,
            b_star,
            if *skips { "yes" } else { "no" },
            real_e,
            real_u
        );
    }
    println!();
    let med_e = median(reals_e);
    let med_u = median(reals_u);
    println!(
        "**Medians ({V2_SEED_START}..{V2_SEED_END}):** REAL_e {med_e:.4} · REAL_u {med_u:.4}. Skips `b*` on {skipped}/30; REAL strictly greater than the unweighted argmax on {real_superior}/30."
    );
    println!();
    println!(
        "_the skip column is the same structurally-vacuous quantity Part 5b's structural note describes (`search()` partitions the WHOLE pool, so \"no block covers `b*`\" ⟺ \"no pool agent provides `b*`\"), reproduced here only so the two tables read alike. Non-gating; the corrected block-level formulation is #63's._"
    );
    println!();
}

/// Item 4 — the learned per-bit reliability posterior for one routing instance.
///
/// Feeds a fresh 8-bit `PersistentAifArm` (the registered `aif-e1` config —
/// query settings are inert here, the arm is only ever observed into, never
/// consulted) a deterministic outcome stream of [`P5C_TWIN_TASKS`] tasks: for
/// each required bit `b`, an independent Bernoulli(`r_b`) draw at the PLANTED
/// reliability. Bits outside `required` observe `no_obs` and keep the 0.5 prior.
///
/// **Stream discipline:** the draws come from a fresh `SplitMix64` seeded
/// `seed ^ P5C_TWIN_SEED_SALT`, never from the instance's own stream, so
/// `draw_routing_instance` is bit-for-bit the Part 5b draw.
///
/// Returns `r̂[b] = beliefs[b][0]` (state 0 = reliable — the same read as
/// `ReliabilityCoverage::from_state`). Gotcha 24 applies: this posterior is
/// recency-dominated, so cross-bit ORDERING is the signal, not the level.
fn p5c_learned_reliability(inst: &RoutingInstance, seed: u64) -> [f64; UNIVERSE] {
    learned_reliability(inst, seed, P5C_TWIN_SEED_SALT, P5C_TWIN_TASKS)
}

/// The learned-posterior pipeline itself, parameterized by the stream `salt` and
/// the task count. [`p5c_learned_reliability`] is the item-4 instantiation;
/// Part 6's leg L is the same pipeline on its own salt, so the two streams have
/// independent starting states and no draws are shared between them (the
/// `run_seed_b` thin-wrapper precedent).
fn learned_reliability(
    inst: &RoutingInstance,
    seed: u64,
    salt: u64,
    tasks: usize,
) -> [f64; UNIVERSE] {
    let arm = PersistentAifArm::new(seed, e1_config()).expect("persistent arm construction");
    let mut rng = SplitMix64::new(seed ^ salt);
    for _ in 0..tasks {
        let mut per_bit = [false; UNIVERSE];
        for (b, slot) in per_bit.iter_mut().enumerate() {
            if inst.required & (1u32 << b) != 0 {
                *slot = next_unit(&mut rng) < inst.reliability[b];
            }
        }
        arm.observe_outcome(inst.required, &per_bit);
    }
    let snapshot = arm.state_snapshot();
    let mut r_hat = [0.5f64; UNIVERSE];
    for (b, slot) in r_hat.iter_mut().enumerate() {
        if let Some(belief) = snapshot.beliefs.get(b) {
            *slot = belief[0];
        }
    }
    r_hat
}

/// One printed row of the item-4 twin table.
struct TwinRow {
    seed: u64,
    b_star: usize,
    /// The reference strong bit: the lowest-indexed required bit other than `b*`.
    strong: usize,
    r_hat_weak: f64,
    r_hat_strong: f64,
    skips_b_star: bool,
    real_learned: f64,
    real_unweighted: f64,
}

/// Item 4 — learned-posterior routing twins of Part 5b.
#[allow(clippy::too_many_lines)]
fn part5c_item4_learned_twins() {
    println!("## Item 4 — learned-posterior routing twins");
    println!();
    println!(
        "_the registered item: Part 5b's reliability-weighted argmax, but with `r̂` LEARNED from an outcome stream (the #57 `from_state` path) instead of read off the planted vector — a pipeline test of \"could a runtime actually get these weights?\", not a second routing test. Per seed: a fresh `aif-e1` `PersistentAifArm` observes {P5C_TWIN_TASKS} tasks of independent per-bit Bernoulli(`r_b`) outcomes at the PLANTED reliabilities (own `SplitMix64` stream, salted off the instance seed, so Part 5b's draw is untouched), then `r̂[b] = beliefs[b][0]`. The weighted twin is `TaskCoverageV2::weighted(required, r̂)` — the example-side v2-coefficient model; the library `ReliabilityCoverage` keeps its registered v1 constants and is not used. The REAL yardstick still uses the PLANTED `r` (ground truth). Exploratory: no bar, no verdict._"
    );
    println!();

    let mut rows: Vec<TwinRow> = Vec::new();
    let mut ordering_ok = 0usize;
    let mut skipped = 0usize;
    let mut real_superior = 0usize;
    let mut reals_l: Vec<f64> = Vec::new();
    let mut reals_u: Vec<f64> = Vec::new();

    for seed in V2_SEED_START..V2_SEED_END {
        let inst = draw_routing_instance(seed);
        let cfg = PopulationConfig::default().with_seed(seed);
        let r_hat = p5c_learned_reliability(&inst, seed);

        // Ordering check (gotcha 24): does the weak bit read below a strong one?
        // The reference strong bit is the lowest-indexed required bit != b*.
        let strong = (0..UNIVERSE)
            .find(|&b| inst.required & (1u32 << b) != 0 && b != inst.b_star)
            .expect("m >= 7 ⇒ a required bit other than b* always exists");
        if r_hat[inst.b_star] < r_hat[strong] {
            ordering_ok += 1;
        }

        let unweighted = search(
            &inst.agents,
            &TaskCoverageV2::unweighted(inst.required),
            &cfg,
        )
        .best;
        let learned = search(
            &inst.agents,
            &TaskCoverageV2::weighted(inst.required, r_hat),
            &cfg,
        )
        .best;

        let l_cov = structure_required_coverage(&learned, &inst.agents, inst.required);
        let skips = l_cov & (1u32 << inst.b_star) == 0;
        if skips {
            skipped += 1;
        }

        let real_l = real_payoff(&learned, &inst.agents, inst.required, &inst.reliability);
        let real_u = real_payoff(&unweighted, &inst.agents, inst.required, &inst.reliability);
        if real_l > real_u {
            real_superior += 1;
        }
        reals_l.push(real_l);
        reals_u.push(real_u);

        rows.push(TwinRow {
            seed,
            b_star: inst.b_star,
            strong,
            r_hat_weak: r_hat[inst.b_star],
            r_hat_strong: r_hat[strong],
            skips_b_star: skips,
            real_learned: real_l,
            real_unweighted: real_u,
        });
    }

    println!("| seed | b* | strong bit | r̂[b*] | r̂[strong] | ordered | learned skips b* | REAL_l | REAL_u |");
    println!("|-----:|---:|-----------:|------:|----------:|:-------:|:----------------:|-------:|-------:|");
    for r in &rows {
        println!(
            "| {} | {} | {} | {:.4} | {:.4} | {} | {} | {:.4} | {:.4} |",
            r.seed,
            r.b_star,
            r.strong,
            r.r_hat_weak,
            r.r_hat_strong,
            if r.r_hat_weak < r.r_hat_strong { "yes" } else { "no" },
            if r.skips_b_star { "yes" } else { "no" },
            r.real_learned,
            r.real_unweighted
        );
    }
    println!();
    let med_l = median(reals_l);
    let med_u = median(reals_u);
    println!(
        "**Medians ({V2_SEED_START}..{V2_SEED_END}):** REAL_l {med_l:.4} · REAL_u {med_u:.4}. Planted `r[b*] = {V2B_WEAK_R}` vs every other required bit `{V2B_STRONG_R}`; the posterior ranks `b*` below the reference strong bit on **{ordering_ok}/30** seeds — the gotcha-24 ordering check, which is the claim the recency-dominated posterior can actually support. Learned-weighted argmax skips `b*` on {skipped}/30 (structurally vacuous, see Item 3); REAL strictly greater on {real_superior}/30."
    );
    println!();
}

fn part5c_addendum() {
    println!("# koalisi #61 — Part 5c: registered exploratory addendum");
    println!();
    println!(
        "_registered in `docs/prereg-K4-battery-v2.md` §\"Part 5c — exploratory only\", which fixes the SCOPE of these items and nothing about their outcome. **Everything below is exploratory and non-gating**: no confirmatory criterion is evaluated, no verdict is derived, and the registered Parts 5a/5b above are untouched (their printed lines are byte-identical to the committed run). Lever 3 — oracle-vs-degraded pricing — is the fifth registered Part 5c item and already ran with Part 5a._"
    );
    println!();
    part5c_item1_w12_slice();
    part5c_item2_hysteresis();
    part5c_item3_expected_outcome();
    part5c_item4_learned_twins();
}

// ===========================================================================
// Part 6 — corrected block-level routing test (koalisi #63, EQ1 tail).
//
// Registered in `docs/prereg-K4-routing-corrected.md` (2026-07-31, posted to #63
// BEFORE any implementation). The #61 lever-1 formulation (Part 5b) landed
// `RUN-INVALID (sanity leg)` for three structural reasons, all of which this part
// corrects: its skip predicate was partition-level and therefore vacuous under
// `search()` (which partitions the WHOLE pool), member-cost savings cannot
// express across partitions of a fixed pool, and its instance draw missed pool
// coverage on 5/30 seeds.
//
//   Leg A — CONFIRMATORY: flip-region planting at the registered v2 coefficients,
//           a BLOCK-level skip predicate, and an attribution counterfactual.
//   Leg C — EXPLORATORY: the product-form full bonus, gated on its own degeneracy
//           analysis (which runs FIRST, per the registration's order).
//   Leg L — EXPLORATORY: the learned-posterior twin of leg A.
//
// Additive: every Part 1–5c printed line is unchanged. Part 5b's own draw,
// closure, and constants are untouched.
// ===========================================================================

/// Part 6 seed range (registration §Registered design) — fresh seeds; 90..120
/// (lockout) and 150..180 (replication) both stay reserved.
const P6_SEED_START: u64 = 180;
/// Exclusive end of the Part 6 seed range.
const P6_SEED_END: u64 = 210;
/// Leg-A planting: the weak bit `b*`, chosen inside the algebraic flip region.
const P6A_WEAK_R: f64 = 0.02;
/// Leg-A planting: every other required bit. The attribution counterfactual is
/// this same value at `b*` too (uniform), which must NOT flip.
const P6A_OTHERS_R: f64 = 0.35;
/// Leg-C planting: the weak bit under the product-form full bonus.
const P6C_WEAK_R: f64 = 0.15;
/// Leg-C planting: every other required bit. Near 1 by necessity — the
/// product-form bonus collapses under a 0.35 counterfactual too, so the
/// attribution conjunct could never fire at the leg-A values (registration
/// §"Design-lock refinement").
const P6C_OTHERS_R: f64 = 0.98;
/// Leg-L outcome-stream salt. Distinct from [`P5C_TWIN_SEED_SALT`], so the two
/// learned twins start from independent starting states; no draws are shared with
/// item 4's stream.
const P6_TWIN_SEED_SALT: u64 = 0x6300_0000_0000_0000;
/// Leg-C degeneracy gate: all-singletons tying-or-beating the argmax on at least
/// this many seeds labels leg C `DEGENERATE (context only)`.
const P6C_DEGEN_MIN: usize = 15;

/// Build a planted reliability vector: `others` at every index, `weak` at `b_star`.
///
/// All four Part 6 plantings (leg A, leg A counterfactual, leg C, leg C
/// counterfactual) are instances of this shape; entries outside `required` are
/// never read by any of the value models.
fn plant(b_star: usize, weak: f64, others: f64) -> [f64; UNIVERSE] {
    let mut reliability = [others; UNIVERSE];
    reliability[b_star] = weak;
    reliability
}

/// The corrected Part 6 draw (registration §"Instance draw"): [`draw_routing_instance`]'s
/// draw logic with a **coverage guarantee** — the whole instance is re-drawn off
/// the SAME per-seed `SplitMix64` stream until the pool union covers `required`.
///
/// Returns the accepted instance plus the number of rejected draws (recorded as
/// context — the #61 draw missed coverage on 5/30 seeds, which is what made its
/// sanity leg a statement about the pool rather than about the argmax).
///
/// The returned `reliability` carries the **leg-A** planting (`r[b*] = 0.02`,
/// every other bit `0.35`); the other three plantings are built with [`plant`]
/// from the same `b_star`. Deterministic per seed, on its own stream, so Part 5b's
/// draw is untouched.
fn draw_routing_instance_corrected(seed: u64) -> (RoutingInstance, usize) {
    let mut rng = SplitMix64::new(seed);
    let mut rejections = 0usize;

    loop {
        let n = (8 + rng.next_u64() % 9) as usize;
        let agents: Vec<Worker> = (0..n)
            .map(|id| {
                let k = 1 + rng.next_u64() % 4;
                let caps = draw_distinct_bits(&mut rng, k);
                let trust = (20 + rng.next_u64() % 80) as u32;
                Worker { id, caps, trust }
            })
            .collect();

        let m = 7 + rng.next_u64() % 2;
        let required = draw_distinct_bits(&mut rng, m);

        let req_bits: Vec<usize> = (0..UNIVERSE).filter(|b| required & (1u32 << b) != 0).collect();
        let b_star = req_bits[(rng.next_u64() % req_bits.len() as u64) as usize];

        let pool_union = agents.iter().fold(0u32, |acc, a| acc | a.caps);
        if pool_union & required != required {
            rejections += 1;
            continue;
        }

        return (
            RoutingInstance {
                agents,
                required,
                b_star,
                reliability: plant(b_star, P6A_WEAK_R, P6A_OTHERS_R),
            },
            rejections,
        );
    }
}

/// Leg C's value model: [`TaskCoverageV2`] with a **product-form** full-coverage
/// bonus — `100 · Π_{b∈required} r_b`, the probability that a fully-covering block
/// actually delivers every required bit. The partial branch (`w(m) = 80/m` per
/// covered bit) and the `8` per-member cost are identical.
///
/// At `r ≡ 1` the product is 1 and this coincides EXACTLY with
/// `TaskCoverageV2::unweighted`, which is why leg C reuses argmax `U` as its
/// unweighted basis rather than drawing a sixth one.
///
/// **Degeneracy risk (gotcha 21 / 25):** the full bonus decays geometrically in
/// `m`, so at any planting materially below 1 it stops being able to pay for the
/// overlap a merge destroys — exactly the mechanism that made
/// [`ExpectedOutcomeV2`] degenerate. The registration therefore gates leg C on an
/// explicit degeneracy analysis that runs BEFORE the comparison.
struct TaskCoverageV2P {
    required: u32,
    reliability: [f64; UNIVERSE],
}

impl TaskCoverageV2P {
    fn new(required: u32, reliability: [f64; UNIVERSE]) -> Self {
        Self {
            required,
            reliability,
        }
    }
}

impl ValueCalculator for TaskCoverageV2P {
    fn calculate_value(&self, agents: &[&dyn AgentCapabilities]) -> f64 {
        if agents.is_empty() {
            return 0.0;
        }
        let union = agents.iter().fold(0u32, |acc, a| acc | a.capabilities());
        let covered = union & self.required;

        let coverage = if covered == self.required {
            let p_full: f64 = (0..UNIVERSE)
                .filter(|b| self.required & (1u32 << b) != 0)
                .map(|b| self.reliability[b])
                .product();
            V2B_FULL_BONUS * p_full
        } else {
            let w = V2B_PARTIAL_BUDGET / f64::from(self.required.count_ones().max(1));
            w * sum_reliability_of(covered, &self.reliability)
        };

        coverage - agents.len() as f64 * V2B_MEMBER_COST
    }
}

/// Run-validity gate X-B (registration §"Run-validity gates"): the leg-A and
/// leg-C coefficient conditions, asserted before the battery loop.
///
/// Every bound is written as the algebraic condition over the registered
/// constants, not as a transcribed decimal, so a constant change breaks the gate
/// rather than silently invalidating the reading.
///
/// **Leg A** — the block-level flip compares a full-coverage block of `m`
/// specialists against the one-member-smaller block that omits `b*`:
/// `8m > 100·r[b*] + 20·Σ_{b≠b*} r_b` (the `20` is the full/partial budget gap).
/// It must HOLD at the planting for both `m ∈ {7, 8}` and FAIL at the uniform
/// counterfactual for every bit — the attribution property leg A's conjunct 3
/// tests empirically.
///
/// **Leg C** — three conditions on the product form: full coverage is strictly
/// optimal at `r ≡ 1` with margin for every `m ∈ 2..=8`; the equal-size flip holds
/// at the leg-C planting (a partial block that skips `b*` outscores the
/// full-coverage block of the same size); and the counterfactual does NOT flip,
/// including against the one-member-smaller comparison that gets the `8` back.
fn assert_p6_coefficient_gates() {
    let budget_gap = V2B_FULL_BONUS - V2B_PARTIAL_BUDGET;

    // Leg A: flip at the planting, no flip at the counterfactual.
    for m in 7u32..=8 {
        let mf = f64::from(m);
        let flip_rhs = V2B_FULL_BONUS * P6A_WEAK_R + budget_gap * (mf - 1.0) * P6A_OTHERS_R;
        assert!(
            V2B_MEMBER_COST * mf > flip_rhs,
            "leg A: the planting must sit inside the block-level flip region at m = {m} \
             ({} vs {flip_rhs})",
            V2B_MEMBER_COST * mf
        );
        let cf_rhs = V2B_FULL_BONUS * P6A_OTHERS_R + budget_gap * (mf - 1.0) * P6A_OTHERS_R;
        assert!(
            V2B_MEMBER_COST * mf <= cf_rhs,
            "leg A: the uniform counterfactual must NOT flip at m = {m} ({} vs {cf_rhs})",
            V2B_MEMBER_COST * mf
        );
    }

    // Leg C, property 1: full coverage strictly optimal at r = 1, with margin.
    for m in 2u32..=8 {
        let mf = f64::from(m);
        let skip = (V2B_PARTIAL_BUDGET / mf) * (mf - 1.0) + V2B_MEMBER_COST;
        assert!(
            V2B_FULL_BONUS > skip,
            "leg C: at r = 1 the product form must prefer full coverage at m = {m} \
             ({V2B_FULL_BONUS} vs {skip})"
        );
    }

    // Leg C, properties 2 and 3: the equal-size flip at the planting, and the
    // counterfactual non-flip against the one-member-smaller comparison.
    for m in 7u32..=8 {
        let mf = f64::from(m);
        let partial_skip = (V2B_PARTIAL_BUDGET / mf) * (mf - 1.0) * P6C_OTHERS_R;
        let full_planted = V2B_FULL_BONUS * P6C_WEAK_R * P6C_OTHERS_R.powi(m as i32 - 1);
        assert!(
            partial_skip > full_planted,
            "leg C: the planting must flip the equal-size comparison at m = {m} \
             ({partial_skip} vs {full_planted})"
        );
        let full_cf = V2B_FULL_BONUS * P6C_OTHERS_R.powi(m as i32);
        assert!(
            full_cf > partial_skip + V2B_MEMBER_COST,
            "leg C: the counterfactual must NOT flip, even one member smaller, at m = {m} \
             ({full_cf} vs {})",
            partial_skip + V2B_MEMBER_COST
        );
    }
}

/// Minimum total capability multiplicity `Σ_i |caps_i ∩ required|` over pool
/// subsets whose union covers `required` — a weighted set cover, solved exactly by
/// a DP over the `2^m ≤ 256` submasks of `required` × the ≤ 16 pool agents
/// (microseconds per instance). `None` iff the pool cannot cover `required` at
/// all, which the corrected draw rules out.
///
/// Diagnostic role: this is how much REDUNDANT capability any covering block is
/// forced to carry on this pool. It is printed beside the analytic feasibility
/// bound `1.25·m`, so a reader can see whether a seed's geometry admits the
/// leg-A flip at all before reading its conjunct columns.
fn min_cover_multiplicity(agents: &[Worker], required: u32) -> Option<u32> {
    let req_bits: Vec<usize> = (0..UNIVERSE).filter(|b| required & (1u32 << b) != 0).collect();
    let m = req_bits.len();
    let compact = |caps: u32| -> u32 {
        req_bits
            .iter()
            .enumerate()
            .filter(|&(_, &b)| caps & (1u32 << b) != 0)
            .fold(0u32, |acc, (k, _)| acc | (1u32 << k))
    };

    let full = (1usize << m) - 1;
    let mut dp = vec![u32::MAX; full + 1];
    dp[0] = 0;
    // Ascending states are a valid order: adding an agent only ever SETS bits, so
    // every relaxation moves strictly forward through the mask lattice.
    for s in 0..=full {
        if dp[s] == u32::MAX {
            continue;
        }
        for a in agents {
            let am = compact(a.caps);
            if am == 0 {
                continue;
            }
            let ns = s | am as usize;
            let cost = dp[s] + am.count_ones();
            if cost < dp[ns] {
                dp[ns] = cost;
            }
        }
    }

    (dp[full] != u32::MAX).then_some(dp[full])
}

/// Per-seed prepass: the corrected draw plus the leg-C argmax `P`.
///
/// The degeneracy gate runs FIRST (the registration's order) and the main loop
/// reads the same instances and the same `P`, so both are computed once per seed
/// here. Both are pure functions of the seed, so recomputing them in the second
/// loop would give identical values — this only avoids a second `search()`.
struct P6Prepass {
    seed: u64,
    inst: RoutingInstance,
    rejections: usize,
    /// The leg-C planting (`r[b*] = 0.15`, every other bit `0.98`).
    r_leg_c: [f64; UNIVERSE],
    /// Argmax under [`TaskCoverageV2P`] at [`Self::r_leg_c`].
    p_best: CoalitionStructure,
}

/// One printed row of the leg-A (confirmatory) table.
struct LegARow {
    seed: u64,
    m: u32,
    b_star: usize,
    rejections: usize,
    /// Conjunct 1: the weighted argmax's top block omits `b*`.
    c1_weighted_omits: bool,
    /// Conjunct 2: the unweighted argmax's top block covers `b*`.
    c2_unweighted_covers: bool,
    /// Conjunct 3: the counterfactual argmax's top block covers `b*` again.
    c3_counterfactual_covers: bool,
    fired: bool,
    real_w: f64,
    real_u: f64,
}

/// One printed row of the leg-A **mechanism-diagnostics** table (context only).
///
/// These columns exist because a bare firing count cannot distinguish "reliability
/// weighting does not route" from "the b\*-window is narrower than the spacing of
/// the competing block values" — see the mechanism-scope paragraph printed below
/// the table.
struct LegADiagRow {
    seed: u64,
    m: u32,
    /// Member count of `U`'s top block.
    s: usize,
    /// Leg-A-weighted value of `U`'s top block — the LOW edge of the b\* window.
    win_lo: f64,
    /// Counterfactual-weighted value of the SAME block — the HIGH edge.
    win_hi: f64,
    /// Best leg-A-weighted singleton value among agents OUTSIDE `U`'s top block,
    /// with the `|caps ∩ required|` of the agent achieving it. `None` when the top
    /// block is the whole pool.
    left: Option<(f64, u32)>,
    /// Minimum `Σ|caps ∩ required|` over pool subsets covering `required`.
    min_mult: Option<u32>,
    /// Does `W`'s argmax contain ANY full-coverage block? Separates the RANKING
    /// channel (formed but ranked below a leftover) from the FORMATION channel.
    w_has_full: bool,
}

/// One printed row of the leg-C (exploratory, product-form) table.
struct LegCRow {
    seed: u64,
    p_omits: bool,
    p_bar_covers: bool,
    u_covers: bool,
    real_p: f64,
    real_u: f64,
}

/// One printed row of the leg-L (exploratory, learned-posterior) table.
struct LegLRow {
    seed: u64,
    b_star: usize,
    /// The reference strong bit: the lowest-indexed required bit other than `b*`.
    strong: usize,
    r_hat_weak: f64,
    r_hat_strong: f64,
    ordered: bool,
    l_omits: bool,
    /// Leg A's conjunct 2, repeated here so the skip column has its control in
    /// the same row.
    u_covers: bool,
    /// `max − min` of `r̂` over the required bits. A near-zero spread collapses
    /// `L`'s partition ranking onto `U`'s, which is what makes the skip column
    /// unreadable on those seeds.
    spread: f64,
    real_l: f64,
    real_u: f64,
}

/// Part 6 — the corrected block-level routing test (#63).
#[allow(clippy::too_many_lines)]
fn part6_corrected_routing() {
    println!("# koalisi #63 — Part 6: corrected block-level routing test (REGISTERED)");
    println!();
    println!(
        "_governed by `docs/prereg-K4-routing-corrected.md` (registered 2026-07-31, BEFORE any implementation; owner design-lock D1–D6 on #63). The corrected re-formulation of the #61 lever-1 test, which landed `RUN-INVALID (sanity leg)`: the draw now GUARANTEES pool coverage by rejection re-draw, the skip predicate is **block-level** (the highest-value block under each calculator's own value, `max_by`/`partial_cmp`, last-maximal on ties) rather than the structurally-vacuous partition-level one, and the planting sits inside the algebraic flip region. Three legs: **A** (CONFIRMATORY) — `TaskCoverageV2` at the registered v2 coefficients, `r[b*] = {P6A_WEAK_R}` against `{P6A_OTHERS_R}` elsewhere, with a uniform-`{P6A_OTHERS_R}` attribution counterfactual; **C** (EXPLORATORY) — the product-form full bonus `100·Π r`, planting `{P6C_WEAK_R}`/`{P6C_OTHERS_R}`, gated on its own degeneracy analysis; **L** (EXPLORATORY) — leg A with `r̂` LEARNED from an outcome stream. Seeds **{P6_SEED_START}..{P6_SEED_END}**; structure-level `REAL` is context ONLY (member costs are partition-constant, so no partition-level flip can be bought by dropping a member; the registered flip condition is derived and tested at BLOCK level)._"
    );
    println!();

    assert_p6_coefficient_gates();
    println!(
        "**Coefficient gate (X-B):** asserted in-code before the loop — leg A's block-level flip `8m > 100·r[b*] + 20·Σ_{{b≠b*}} r_b`, **on the stylized `m`-specialist configuration** (a full-coverage block of `m` one-bit specialists against the one-member-smaller block that omits `b*`), HOLDS at the planting for both `m ∈ {{7, 8}}` and FAILS at the uniform counterfactual for every bit; leg C's product form prefers full coverage at `r ≡ 1` with margin for every `m ∈ 2..=8`, flips at the leg-C planting on the equal-size comparison, and does NOT flip at its counterfactual even against the one-member-smaller block."
    );
    println!();

    // --- Prepass: the corrected draws + the leg-C argmax --------------------
    let mut prepass: Vec<P6Prepass> = Vec::new();
    for seed in P6_SEED_START..P6_SEED_END {
        let (inst, rejections) = draw_routing_instance_corrected(seed);
        let cfg = PopulationConfig::default().with_seed(seed);
        let r_leg_c = plant(inst.b_star, P6C_WEAK_R, P6C_OTHERS_R);
        let p_best = search(
            &inst.agents,
            &TaskCoverageV2P::new(inst.required, r_leg_c),
            &cfg,
        )
        .best;
        prepass.push(P6Prepass {
            seed,
            inst,
            rejections,
            r_leg_c,
            p_best,
        });
    }
    let n_seeds = prepass.len();

    // --- Leg C degeneracy gate (runs FIRST, per the registration) ----------
    println!("## Leg C degeneracy gate (runs FIRST, run-and-label-context)");
    println!();
    println!(
        "_the registered gate: on the {n_seeds} leg-C-planted instances, compare the argmax `P` under `TaskCoverageV2P` against the all-singletons structure under the same calculator. All-singletons tying-or-beating `P` on ≥ {P6C_DEGEN_MIN}/{n_seeds} labels leg C `DEGENERATE (context only)` — its rows then carry no reading beyond the degeneracy mechanism. Either way every leg-C row below is measured and printed._"
    );
    println!();

    let mut singleton_ge_search = 0usize;
    let mut singleton_argmax = 0usize;
    let mut grand_ge_search = 0usize;
    for pre in &prepass {
        let n = pre.inst.agents.len();
        let calc = TaskCoverageV2P::new(pre.inst.required, pre.r_leg_c);
        let singletons: Vec<Vec<usize>> = (0..n).map(|i| vec![i]).collect();
        let one_block: Vec<Vec<usize>> = vec![(0..n).collect()];
        let f_singletons = blocks_fitness(&singletons, &pre.inst.agents, &calc);
        let f_one = blocks_fitness(&one_block, &pre.inst.agents, &calc);
        let f_best = blocks_fitness(&pre.p_best.blocks(), &pre.inst.agents, &calc);
        if f_singletons >= f_best - 1e-9 {
            singleton_ge_search += 1;
        }
        if pre.p_best.blocks().len() == n {
            singleton_argmax += 1;
        }
        if f_one >= f_best - 1e-9 {
            grand_ge_search += 1;
        }
    }
    let leg_c_degenerate = singleton_ge_search >= P6C_DEGEN_MIN;
    let leg_c_label = if leg_c_degenerate {
        "DEGENERATE (context only)"
    } else {
        "not degenerate by the registered gate"
    };
    println!(
        "**Gate result:** all-singletons matches or beats the `P` argmax on **{singleton_ge_search}/{n_seeds}** seeds (bar {P6C_DEGEN_MIN}) → leg C is **{leg_c_label}**."
    );
    println!();
    println!(
        "_Two boundary counts alongside it (item 3's pair, context): the `search()` argmax IS all-singletons on **{singleton_argmax}/{n_seeds}** seeds — the gap between this and the {singleton_ge_search} above is PSO shortfall rather than model degeneracy — and the single grand-coalition block ties or beats the argmax on **{grand_ge_search}/{n_seeds}**._"
    );
    println!();

    // --- Main loop ---------------------------------------------------------
    let mut a_rows: Vec<LegARow> = Vec::new();
    let mut diag_rows: Vec<LegADiagRow> = Vec::new();
    let mut c_rows: Vec<LegCRow> = Vec::new();
    let mut l_rows: Vec<LegLRow> = Vec::new();

    let mut sanity_ok = 0usize;
    let mut c1_count = 0usize;
    let mut c2_count = 0usize;
    let mut c3_count = 0usize;
    let mut fired_count = 0usize;
    let mut control_failed = 0usize;
    let mut partition_differs = 0usize;
    let mut counterfactual_partition_differs = 0usize;
    let mut ordering_ok = 0usize;
    let mut total_rejections = 0usize;

    let mut reals_w: Vec<f64> = Vec::new();
    let mut reals_u_a: Vec<f64> = Vec::new();
    let mut reals_p: Vec<f64> = Vec::new();
    let mut reals_u_c: Vec<f64> = Vec::new();
    let mut reals_l: Vec<f64> = Vec::new();
    let mut spreads: Vec<f64> = Vec::new();

    for pre in &prepass {
        let inst = &pre.inst;
        let cfg = PopulationConfig::default().with_seed(pre.seed);
        let b_star_bit = 1u32 << inst.b_star;
        total_rejections += pre.rejections;

        // The four plantings + the learned twin. `inst.reliability` IS the leg-A
        // planting (the corrected draw returns it); the counterfactuals re-plant
        // `b*` strong on the same draw, which is the attribution control.
        let r_a = inst.reliability;
        let r_a_bar = plant(inst.b_star, P6A_OTHERS_R, P6A_OTHERS_R);
        let r_c_bar = plant(inst.b_star, P6C_OTHERS_R, P6C_OTHERS_R);
        let r_hat = learned_reliability(inst, pre.seed, P6_TWIN_SEED_SALT, P5C_TWIN_TASKS);

        let u_calc = TaskCoverageV2::unweighted(inst.required);
        let w_calc = TaskCoverageV2::weighted(inst.required, r_a);
        let w_bar_calc = TaskCoverageV2::weighted(inst.required, r_a_bar);
        let p_calc = TaskCoverageV2P::new(inst.required, pre.r_leg_c);
        let p_bar_calc = TaskCoverageV2P::new(inst.required, r_c_bar);
        let l_calc = TaskCoverageV2::weighted(inst.required, r_hat);

        let u_best = search(&inst.agents, &u_calc, &cfg).best;
        let w_best = search(&inst.agents, &w_calc, &cfg).best;
        let w_bar_best = search(&inst.agents, &w_bar_calc, &cfg).best;
        let p_bar_best = search(&inst.agents, &p_bar_calc, &cfg).best;
        let l_best = search(&inst.agents, &l_calc, &cfg).best;

        // Every top block is scored under its OWN argmax's calculator.
        let u_top = top_block_mask(&u_best, &inst.agents, &u_calc);
        let w_top = top_block_mask(&w_best, &inst.agents, &w_calc);
        let w_bar_top = top_block_mask(&w_bar_best, &inst.agents, &w_bar_calc);
        let p_top = top_block_mask(&pre.p_best, &inst.agents, &p_calc);
        let p_bar_top = top_block_mask(&p_bar_best, &inst.agents, &p_bar_calc);
        let l_top = top_block_mask(&l_best, &inst.agents, &l_calc);

        // Leg A — the confirmatory conjuncts.
        let sanity = u_top & inst.required == inst.required;
        if sanity {
            sanity_ok += 1;
        }
        let c1 = w_top & b_star_bit == 0;
        let c2 = u_top & b_star_bit != 0;
        let c3 = w_bar_top & b_star_bit != 0;
        c1_count += usize::from(c1);
        c2_count += usize::from(c2);
        c3_count += usize::from(c3);
        let fired = c1 && c2 && c3;
        if fired {
            fired_count += 1;
        }
        if c1 && c2 && !c3 {
            control_failed += 1;
        }
        if w_best.assignment != u_best.assignment {
            partition_differs += 1;
        }
        // Attribution-validity diagnostic. At a UNIFORM reliability the weighted
        // total is an increasing affine map of the unweighted total, so W̄'s and
        // U's argmax partitions should coincide up to float ties — expected 0.
        // Counted, never asserted: a float tie must not panic the registered run.
        if w_bar_best.assignment != u_best.assignment {
            counterfactual_partition_differs += 1;
        }

        let real_w = real_payoff(&w_best, &inst.agents, inst.required, &r_a);
        let real_u_a = real_payoff(&u_best, &inst.agents, inst.required, &r_a);
        let real_p = real_payoff(&pre.p_best, &inst.agents, inst.required, &pre.r_leg_c);
        let real_u_c = real_payoff(&u_best, &inst.agents, inst.required, &pre.r_leg_c);
        let real_l = real_payoff(&l_best, &inst.agents, inst.required, &r_a);
        reals_w.push(real_w);
        reals_u_a.push(real_u_a);
        reals_p.push(real_p);
        reals_u_c.push(real_u_c);
        reals_l.push(real_l);

        // Leg L — the gotcha-24 ordering check, against the lowest-indexed
        // required bit other than `b*` (the item-4 reference).
        let strong = (0..UNIVERSE)
            .find(|&b| inst.required & (1u32 << b) != 0 && b != inst.b_star)
            .expect("m >= 7 ⇒ a required bit other than b* always exists");
        let ordered = r_hat[inst.b_star] < r_hat[strong];
        if ordered {
            ordering_ok += 1;
        }
        let (lo, hi) = (0..UNIVERSE)
            .filter(|b| inst.required & (1u32 << b) != 0)
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), b| {
                (lo.min(r_hat[b]), hi.max(r_hat[b]))
            });
        let spread = hi - lo;
        spreads.push(spread);

        // --- Mechanism diagnostics (context only) --------------------------
        // The b* window: the same block (U's top) valued under the leg-A planting
        // and under the counterfactual. A competing block can only flip the
        // ranking if its value lands strictly between these two edges.
        let u_top_block = top_block(&u_best, &inst.agents, &u_calc);
        let u_top_view = coalition_view(&inst.agents, &u_top_block);
        let win_lo = w_calc.calculate_value(&u_top_view);
        let win_hi = w_bar_calc.calculate_value(&u_top_view);
        let left = inst
            .agents
            .iter()
            .enumerate()
            .filter(|(i, _)| !u_top_block.contains(i))
            .map(|(i, a)| {
                (
                    w_calc.calculate_value(&coalition_view(&inst.agents, &[i])),
                    (a.caps & inst.required).count_ones(),
                )
            })
            .max_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal));
        let w_has_full = w_best.blocks().iter().any(|blk| {
            blk.iter().fold(0u32, |acc, &i| acc | inst.agents[i].caps) & inst.required
                == inst.required
        });
        diag_rows.push(LegADiagRow {
            seed: pre.seed,
            m: inst.required.count_ones(),
            s: u_top_block.len(),
            win_lo,
            win_hi,
            left,
            min_mult: min_cover_multiplicity(&inst.agents, inst.required),
            w_has_full,
        });

        a_rows.push(LegARow {
            seed: pre.seed,
            m: inst.required.count_ones(),
            b_star: inst.b_star,
            rejections: pre.rejections,
            c1_weighted_omits: c1,
            c2_unweighted_covers: c2,
            c3_counterfactual_covers: c3,
            fired,
            real_w,
            real_u: real_u_a,
        });
        c_rows.push(LegCRow {
            seed: pre.seed,
            p_omits: p_top & b_star_bit == 0,
            p_bar_covers: p_bar_top & b_star_bit != 0,
            u_covers: c2,
            real_p,
            real_u: real_u_c,
        });
        l_rows.push(LegLRow {
            seed: pre.seed,
            b_star: inst.b_star,
            strong,
            r_hat_weak: r_hat[inst.b_star],
            r_hat_strong: r_hat[strong],
            ordered,
            l_omits: l_top & b_star_bit == 0,
            u_covers: c2,
            spread,
            real_l,
            real_u: real_u_a,
        });
    }

    // --- Leg A table -------------------------------------------------------
    println!("## Leg A — confirmatory block-level routing (seeds {P6_SEED_START}..{P6_SEED_END})");
    println!();
    println!(
        "| seed | m | b* | rejects | C1 W-top omits b* | C2 U-top covers b* | C3 W̄-top covers b* | fired | REAL_w | REAL_u | ΔREAL |"
    );
    println!(
        "|-----:|--:|---:|--------:|:-----------------:|:------------------:|:-------------------:|:-----:|-------:|-------:|------:|"
    );
    for r in &a_rows {
        println!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {:.4} | {:.4} | {:+.4} |",
            r.seed,
            r.m,
            r.b_star,
            r.rejections,
            if r.c1_weighted_omits { "yes" } else { "no" },
            if r.c2_unweighted_covers { "yes" } else { "no" },
            if r.c3_counterfactual_covers { "yes" } else { "no" },
            if r.fired { "yes" } else { "no" },
            r.real_w,
            r.real_u,
            r.real_w - r.real_u
        );
    }
    println!();
    let med_w = median(reals_w);
    let med_u_a = median(reals_u_a.clone());
    println!(
        "**Medians ({P6_SEED_START}..{P6_SEED_END}, leg-A planting):** REAL_w {med_w:.4} · REAL_u {med_u_a:.4}. Structure-level `REAL` is CONTEXT (registration structural note 1) — never gated. Note also that `W` is not a `REAL`-maximizer: `TaskCoverageV2::weighted` credits a full-coverage block `(20/m)·Σ_required r` on top of the partial term where `REAL` credits `20·Π r` — 6.06 vs 0.0007 at this planting (m = 7), so the two disagree about how much full coverage is worth. At this planting `REAL` itself prefers full coverage over the `b*`-skip by only ~0.23 per block at equal size."
    );
    println!();

    // --- Leg A mechanism diagnostics (context only) ------------------------
    println!("## Leg A mechanism diagnostics (context)");
    println!();
    println!(
        "| seed | m | s | win_lo | win_hi | v_left | j | min_mult | 1.25·m | W full? |"
    );
    println!(
        "|-----:|--:|--:|-------:|-------:|-------:|--:|---------:|-------:|:-------:|"
    );
    for r in &diag_rows {
        let (v_left, j) = match r.left {
            Some((v, j)) => (format!("{v:.4}"), j.to_string()),
            None => ("—".to_string(), "—".to_string()),
        };
        let min_mult = r
            .min_mult
            .map_or_else(|| "—".to_string(), |x| x.to_string());
        println!(
            "| {} | {} | {} | {:.4} | {:.4} | {} | {} | {} | {:.2} | {} |",
            r.seed,
            r.m,
            r.s,
            r.win_lo,
            r.win_hi,
            v_left,
            j,
            min_mult,
            1.25 * f64::from(r.m),
            if r.w_has_full { "yes" } else { "no" }
        );
    }
    println!();
    println!(
        "_Mechanism scope: firing requires the best competing block's value to fall inside a window of width `100·(0.35 − 0.02)/m` (4.71 at m = 7, 4.13 at m = 8) that the b* planting opens between the weighted and counterfactual valuations of the full-coverage block. Competing blocks are singletons whose values sit on a lattice of spacing `w(m)·0.35` (4.00 / 3.50), so at most one leftover capability count can land inside for a given cover size. A low fired count is therefore consistent with \"the b*-window is narrower than the resolution of the competing block values\" as well as with \"no routing\"; the per-seed (s, j) columns above are what distinguishes them._"
    );
    println!();

    // --- Leg C table -------------------------------------------------------
    println!("## Leg C — product-form full bonus (EXPLORATORY, {leg_c_label})");
    println!();
    println!("| seed | P-top omits b* | P̄-top covers b* | U-top covers b* | REAL_p | REAL_u |");
    println!("|-----:|:--------------:|:----------------:|:---------------:|-------:|-------:|");
    for r in &c_rows {
        println!(
            "| {} | {} | {} | {} | {:.4} | {:.4} |",
            r.seed,
            if r.p_omits { "yes" } else { "no" },
            if r.p_bar_covers { "yes" } else { "no" },
            if r.u_covers { "yes" } else { "no" },
            r.real_p,
            r.real_u
        );
    }
    println!();
    let c_fired = c_rows
        .iter()
        .filter(|r| r.p_omits && r.u_covers && r.p_bar_covers)
        .count();
    let med_p = median(reals_p);
    let med_u_c = median(reals_u_c);
    println!(
        "**Medians ({P6_SEED_START}..{P6_SEED_END}, leg-C planting):** REAL_p {med_p:.4} · REAL_u {med_u_c:.4}. The three-conjunct skip predicate fires on **{c_fired}/{n_seeds}** seeds. No bar and no verdict — leg C is exploratory in either degeneracy state. `REAL` is the expected payoff at the ORIGINAL v2 coefficients (partial credit per covered bit); `TaskCoverageV2P` maximizes an all-or-nothing success probability instead, and at this planting the two disagree in direction (per block at equal size, REAL prefers full coverage 71.57 vs 67.20 while the product form prefers the skip 67.20 vs 13.29). `REAL_p` below `REAL_u` is therefore expected by construction and carries no reading about routing."
    );
    println!();

    // --- Leg L table -------------------------------------------------------
    println!("## Leg L — learned-posterior twin of leg A (EXPLORATORY)");
    println!();
    println!(
        "_the same leg-A comparison with `r̂` LEARNED instead of read off the planted vector: a fresh `aif-e1` `PersistentAifArm` observes {P5C_TWIN_TASKS} tasks of independent per-bit Bernoulli(`r_b`) outcomes at the leg-A planting (own `SplitMix64` stream, salted off the seed with a Part 6 salt so item 4's stream is untouched), then `r̂[b] = beliefs[b][0]`. The leg-A planting is a harder ordering problem than item 4's for a reason specific to the recency-dominated read (gotcha 24): the absolute gap narrows (0.33 vs 0.75) while the ratio widens (17.5× vs 6.0×), and — the operative change — the reference strong bit is itself only Bernoulli({P6A_OTHERS_R}), so it fails on most recent tasks and reads low too. Whether that degrades the ordering is what this leg measures. REAL still uses the PLANTED `r` (ground truth). No bar, no verdict._"
    );
    println!();
    println!(
        "| seed | b* | strong bit | r̂[b*] | r̂[strong] | r̂ spread | ordered | L-top omits b* | U-top covers b* | REAL_l | REAL_u |"
    );
    println!(
        "|-----:|---:|-----------:|------:|----------:|---------:|:-------:|:--------------:|:---------------:|-------:|-------:|"
    );
    for r in &l_rows {
        println!(
            "| {} | {} | {} | {:.4} | {:.4} | {:.4} | {} | {} | {} | {:.4} | {:.4} |",
            r.seed,
            r.b_star,
            r.strong,
            r.r_hat_weak,
            r.r_hat_strong,
            r.spread,
            if r.ordered { "yes" } else { "no" },
            if r.l_omits { "yes" } else { "no" },
            if r.u_covers { "yes" } else { "no" },
            r.real_l,
            r.real_u
        );
    }
    println!();
    let l_omits_count = l_rows.iter().filter(|r| r.l_omits).count();
    let l_joint_count = l_rows.iter().filter(|r| r.l_omits && r.u_covers).count();
    let med_l = median(reals_l);
    let med_spread = median(spreads);
    println!(
        "**Medians ({P6_SEED_START}..{P6_SEED_END}):** REAL_l {med_l:.4} · REAL_u {med_u_a:.4} · `r̂` spread {med_spread:.4}. The posterior ranks `b*` below the reference strong bit on **{ordering_ok}/{n_seeds}** seeds (the gotcha-24 ordering check) — that ORDERING is the claim this leg supports. The learned-weighted argmax's top block omits `b*` on {l_omits_count}/{n_seeds}, and omits it while `U`'s covers it on {l_joint_count}/{n_seeds}; the skip column is readable only where the spread is material, since a near-uniform `r̂` collapses `L`'s partition ranking onto `U`'s and leaves only the member-cost-driven top-block shift."
    );
    println!();

    // --- H-BR evaluation (leg A only) --------------------------------------
    let sanity_leg = sanity_ok >= HR_SANITY_MIN;
    let skip_leg = fired_count >= HR_CONSISTENCY_MIN;

    println!("## H-BR evaluation — block-level routing (leg A only)");
    println!();
    println!(
        "- **Sanity leg (run-invalidating):** the top block of `U` achieves full required coverage on {sanity_ok}/{n_seeds} ≥ {HR_SANITY_MIN} → {}",
        pass(sanity_leg)
    );
    println!(
        "- **Skip leg:** all three conjuncts hold on {fired_count}/{n_seeds} ≥ {HR_CONSISTENCY_MIN} → {}",
        pass(skip_leg)
    );
    println!();

    if sanity_leg {
        let verdict = if skip_leg {
            "VALIDATED (block-routing)"
        } else {
            "FALSIFIED (block-routing)"
        };
        println!("**VERDICT (corrected routing — #63): {verdict}**");
    } else {
        println!("**VERDICT (corrected routing — #63): RUN-INVALID (sanity leg)**");
    }
    println!();
    println!(
        "_VALIDATED (block-routing) = sanity ∧ skip; FALSIFIED (block-routing) = sanity ∧ ¬skip; a sanity-leg failure invalidates the run rather than producing a verdict. Bars ({HR_SANITY_MIN}/{n_seeds} sanity, {HR_CONSISTENCY_MIN}/{n_seeds} consistency) inherit the family's conventions and were locked in the prereg before implementation. The headline is BLOCK-level only — no structure-level routing claim is available from this design._"
    );
    println!();
    println!(
        "_Read this verdict against the mechanism-scope paragraph under the leg-A diagnostics table: a low fired count is consistent with the b*-window being narrower than the spacing of the competing block values, not only with the absence of routing._"
    );
    println!();

    // --- Context (never gated) ---------------------------------------------
    println!("## Context (recorded, never gated)");
    println!();
    println!(
        "- Per-conjunct raw counts: C1 (weighted top block omits `b*`) {c1_count}/{n_seeds} · C2 (unweighted top block covers `b*`) {c2_count}/{n_seeds} · C3 (counterfactual top block covers `b*`) {c3_count}/{n_seeds}; all three together {fired_count}/{n_seeds}."
    );
    println!(
        "- C2 is entailed by the sanity leg (`b*` is a required bit, so a top block covering ALL of `required` covers `b*`), so on a valid run the skip leg is effectively C1 ∧ C3."
    );
    println!(
        "- A skip-leg shortfall decomposes into \"no flip\" (¬C1) and \"control failed\" (C1 ∧ C3 missing, i.e. C1 ∧ C2 ∧ ¬C3), the latter measured on {control_failed}/{n_seeds} seeds. A control failure is a SIZE effect of the uniform counterfactual — scaling every reliability to {P6A_OTHERS_R} scales the coverage terms but not the `8`-per-member cost — and is not evidence about routing."
    );
    println!(
        "- **Attribution validity:** `W̄`'s and `U`'s argmax partitions differ on {counterfactual_partition_differs}/{n_seeds} seeds. Expected 0 — at a uniform reliability the weighted total is an increasing affine map of the unweighted total, so the two argmaxes coincide up to float ties. Any nonzero count means C1-vs-C3 was an intent-to-treat contrast across DIFFERENT partitions on those seeds. Counted, never asserted, so a float tie cannot panic the registered run."
    );
    println!(
        "- The weighted and unweighted argmax **partitions** differ at all on {partition_differs}/{n_seeds} seeds. Member costs sum to the partition-constant −8·N under every calculator here, so member savings can never motivate a partition change; any difference counted here comes from the coverage terms alone."
    );
    println!(
        "- Coverage-guaranteed draw: {total_rejections} instance re-draws across the {n_seeds} seeds (the #61 draw had no guarantee and missed coverage on 5/30)."
    );
    println!(
        "- Leg C degeneracy: all-singletons ties or beats the argmax on {singleton_ge_search}/{n_seeds} (bar {P6C_DEGEN_MIN}) → **{leg_c_label}**; the argmax IS all-singletons on {singleton_argmax}/{n_seeds} and the grand coalition ties-or-beats it on {grand_ge_search}/{n_seeds}."
    );
    println!();
}

// ===========================================================================
// Part 7 — EQ3 latency re-match (koalisi #69). REGISTERED.
//
// Governed by `docs/prereg-K4-eq3-latency-rematch.md` + its Amendment 1
// (both committed before this code). The registration fixes: the v2-draw Scope-B
// regime, seeds 210..240, the three arms, the v1 latency protocol, the H-par′ /
// H-lat legs and their verdict grammar, the context rows, and the
// instrumentation. Nothing here is tuned to flip an outcome.
//
// Arms (Amendment 1 / A1.1):
//   scalar  — AifDecisionPolicy::default()          (the v1 latency comparator)
//   mag     — MagnitudePolicy::default()            (L1 only — the frozen arm)
//   mag-eq3 — MagnitudePolicy::default()
//               .with_eq3_levers(true)              (L2 + L3 — the challenger)
//
// `mag-eq3` needs the off-by-default `magnitude-fast` feature, so the whole
// battery is feature-gated and prints a labelled SKIPPED line when it is off.
// The official run is
//   cargo run --release --features decision,magnitude,magnitude-fast \
//     --example strategy_comparison
// ===========================================================================

/// Part 7 seed range (registration §Part 7, owner D4) — fresh seeds; 90..120 and
/// 150..180 stay reserved.
const P7_SEED_START: u64 = 210;
const P7_SEED_END: u64 = 240;
/// H-par′ (ii): median `PRIMARY_B(mag-eq3)` must be at least this multiple of
/// median `PRIMARY_B(mag)` (Amendment 1 / A1.2).
#[cfg(feature = "magnitude-fast")]
const P7_NONINFERIORITY: f64 = 0.98;
/// H-par′ (i): at a first divergence, `mag`'s own margin must be float noise
/// around the certified exact zero. Same order as the library's corpus gate.
#[cfg(feature = "magnitude-fast")]
const P7_SHAPE_NOISE: f64 = 1e-15;
/// The K6 reference latencies (`docs/ab-report-K4-catgraph-evaluator.md`) — v1
/// regime, so every comparison against them is labelled cross-regime.
#[cfg(feature = "magnitude-fast")]
const P7_K6_MAG_US: f64 = 3.552;
#[cfg(feature = "magnitude-fast")]
const P7_K6_AIF_US: f64 = 1.435;
/// Report date for the Part 7 battery, stamped per committed run.
const P7_REPORT_DATE: &str = "2026-08-02";

/// One task's FIRST `mag` vs `mag-eq3` act divergence — the object H-par′ (i)
/// tests. Recorded on the decision where both arms still hold identical
/// coalition state (every earlier act in the task agreed), so the shape check is
/// well posed.
#[cfg(feature = "magnitude-fast")]
#[derive(Debug, Clone, Copy)]
struct P7FirstDiv {
    seed: u64,
    task: usize,
    /// `true` = the divergence happened in the leave sweep.
    leave: bool,
    /// The certificate `mag-eq3` had at that decision, if any. Always `None` on
    /// the leave sweep: the default leave variant is the FRESH two-evaluation
    /// path, which never consults the evaluator, so L2 cannot act there and a
    /// leave divergence can only be L3 arithmetic.
    proof: Option<ZeroDiversityProof>,
    /// `mag`'s own margin/delta at that decision (its `Decision::score`).
    mag_margin: f64,
    /// Conjunct (i): a proof fired AND `|mag_margin| <= P7_SHAPE_NOISE`.
    shape_ok: bool,
}

/// The paired-walk tally for the whole battery.
#[cfg(feature = "magnitude-fast")]
#[derive(Default)]
struct P7Paired {
    /// Decisions where both arms were evaluated on the same index.
    compared: usize,
    /// Per-task first divergences (at most one per task).
    firsts: Vec<P7FirstDiv>,
    /// Divergent decisions AFTER a task's first one — membership cascade,
    /// exempt from the shape check by A1.2 and counted as context.
    cascade: usize,
    /// Leave steps where only one arm still held the member (a structural
    /// cascade — the two coalitions had already drifted apart).
    structural: usize,
}

/// Walk `mag` and `mag-eq3` through one seed in lockstep — same instance, same
/// arrival order, each arm carrying its OWN membership so a divergence is
/// allowed to cascade exactly as it would in a solo run.
///
/// Both arms are scored on identical state up to and including a task's first
/// divergence (all earlier acts agreed ⇒ identical membership), which is what
/// makes the H-par′ (i) shape check meaningful. The certificate is read with the
/// library's `probe_join` instrumentation surface on a private cache, so it
/// cannot perturb either arm.
#[cfg(feature = "magnitude-fast")]
fn p7_paired_seed(seed: u64, out: &mut P7Paired) {
    let (agents, tasks, _rho, _perf) = generate_instance_b_regime(seed, Regime::V2);
    let mag = MagnitudePolicy::default();
    let eq3 = MagnitudePolicy::default().with_eq3_levers(true);

    for (t, task) in tasks.iter().enumerate() {
        let ctx = DecisionContext {
            required_capabilities: task.required,
        };
        let mut m_members: Vec<usize> = vec![task.order[0]];
        let mut e_members: Vec<usize> = vec![task.order[0]];
        let mut diverged = false;

        for &idx in &task.order[1..] {
            let candidate: &dyn AgentCapabilities = &agents[idx];
            let m_view = coalition_view(&agents, &m_members);
            let e_view = coalition_view(&agents, &e_members);
            let dm = mag.should_join(candidate, &m_view, &ctx);
            let de = eq3.should_join(candidate, &e_view, &ctx);
            out.compared += 1;

            if dm.act != de.act {
                if diverged {
                    out.cascade += 1;
                } else {
                    diverged = true;
                    // Identical state here, so probing `m_view` observes exactly
                    // what `mag-eq3` saw.
                    let proof = eq3
                        .probe_join(candidate, &m_view, &ctx)
                        .and_then(|p| p.zero_proof);
                    out.firsts.push(P7FirstDiv {
                        seed,
                        task: t,
                        leave: false,
                        proof,
                        mag_margin: dm.score,
                        shape_ok: proof.is_some() && dm.score.abs() <= P7_SHAPE_NOISE,
                    });
                }
            }

            if dm.act {
                m_members.push(idx);
            }
            if de.act {
                e_members.push(idx);
            }
        }

        for &idx in &task.order {
            let m_pos = m_members.iter().position(|&m| m == idx);
            let e_pos = e_members.iter().position(|&m| m == idx);
            let agent: &dyn AgentCapabilities = &agents[idx];

            match (m_pos, e_pos) {
                (Some(mp), Some(ep)) => {
                    let m_view = coalition_view(&agents, &m_members);
                    let e_view = coalition_view(&agents, &e_members);
                    let dm = mag.should_leave(agent, &m_view, &ctx);
                    let de = eq3.should_leave(agent, &e_view, &ctx);
                    out.compared += 1;

                    if dm.act != de.act {
                        if diverged {
                            out.cascade += 1;
                        } else {
                            diverged = true;
                            out.firsts.push(P7FirstDiv {
                                seed,
                                task: t,
                                leave: true,
                                // Variant A leaves never reach the evaluator, so
                                // no certificate exists to fire (see the field
                                // docs) — the shape check fails by construction,
                                // which is the registered meaning of an L3-caused
                                // act change.
                                proof: None,
                                mag_margin: dm.score,
                                shape_ok: false,
                            });
                        }
                    }

                    if dm.act {
                        m_members.remove(mp);
                    }
                    if de.act {
                        e_members.remove(ep);
                    }
                }
                (Some(mp), None) => {
                    out.structural += 1;
                    let m_view = coalition_view(&agents, &m_members);
                    if mag.should_leave(agent, &m_view, &ctx).act {
                        m_members.remove(mp);
                    }
                }
                (None, Some(ep)) => {
                    out.structural += 1;
                    let e_view = coalition_view(&agents, &e_members);
                    if eq3.should_leave(agent, &e_view, &ctx).act {
                        e_members.remove(ep);
                    }
                }
                (None, None) => {}
            }
        }
    }
}

/// Log-decade histogram of the incremental increment `|with − base|` plus the
/// certificate / knife-edge tallies — the registered instrumentation block.
///
/// Bucket `i` holds `[1e(i-16), 1e(i-15))`; bucket 16 is the `>= 1e0` overflow.
/// Exact zeros are counted separately (they have no decade).
#[cfg(feature = "magnitude-fast")]
#[derive(Default)]
struct P7Increments {
    /// Join decisions that reached the incremental branch.
    probed: usize,
    exact_zero: usize,
    decades: [usize; 17],
    knife: usize,
    knife_certified: usize,
    /// SkeletalMerge / incoming-dup / outgoing-dup.
    by_class: [usize; 3],
}

#[cfg(feature = "magnitude-fast")]
impl P7Increments {
    fn record(&mut self, probe: &JoinProbe) {
        self.probed += 1;
        let x = (probe.with - probe.base).abs();
        if x == 0.0 {
            self.exact_zero += 1;
        } else {
            let e = x.log10().floor();
            let idx = (e as i64 + 16).clamp(0, 16) as usize;
            self.decades[idx] += 1;
        }
        if probe.knife_edge {
            self.knife += 1;
            if probe.zero_proof.is_some() {
                self.knife_certified += 1;
            }
        }
        match probe.zero_proof {
            Some(ZeroDiversityProof::SkeletalMerge { .. }) => self.by_class[0] += 1,
            Some(ZeroDiversityProof::IncomingProfileDuplicate { .. }) => self.by_class[1] += 1,
            Some(ZeroDiversityProof::OutgoingProfileDuplicate { .. }) => self.by_class[2] += 1,
            None => {}
        }
    }

    fn proofs(&self) -> usize {
        self.by_class.iter().sum()
    }
}

/// Latency buckets mirroring the K6 profile table, restricted to the classes a
/// probe can decide without reading cache internals (see the deviation note in
/// the printed report).
#[cfg(feature = "magnitude-fast")]
const P7_BUCKETS: [&str; 6] = [
    "join/empty",
    "join/excluded",
    "join/clear",
    "join/band",
    "join/proof",
    "leave/fresh",
];

/// One untimed-classification + timed-decision instrumentation pass over the
/// battery for one arm. NOT the pooled latency measurement (that is a clean
/// `stateless_battery_mode` run with no probing anywhere near it) — this pass
/// probes before each decision, so its absolute numbers carry probe-induced
/// cache disturbance and are only read as a decomposition.
///
/// Returns per-bucket latency samples, the increment tally, and the
/// `FactorizationPath` counts of the `f64` fresh evaluations the EQ3 arm
/// performs (Cholesky / LBLT / Gauss–Jordan).
#[cfg(feature = "magnitude-fast")]
fn p7_instrumentation_pass(eq3_arm: bool) -> (Vec<Vec<f64>>, P7Increments, [usize; 3]) {
    let policy = if eq3_arm {
        MagnitudePolicy::default().with_eq3_levers(true)
    } else {
        MagnitudePolicy::default()
    };
    let mut buckets: Vec<Vec<f64>> = vec![Vec::new(); P7_BUCKETS.len()];
    let mut inc = P7Increments::default();
    let mut paths = [0usize; 3];

    for seed in P7_SEED_START..P7_SEED_END {
        let (agents, tasks, _rho, _perf) = generate_instance_b_regime(seed, Regime::V2);
        for task in &tasks {
            let ctx = DecisionContext {
                required_capabilities: task.required,
            };
            let mut members: Vec<usize> = vec![task.order[0]];

            for &idx in &task.order[1..] {
                let candidate: &dyn AgentCapabilities = &agents[idx];
                let view = coalition_view(&agents, &members);

                // Classification (untimed).
                let masks_without = relevant_masks(&view, task.required);
                let mut with_view = view.clone();
                with_view.push(candidate);
                let masks_with = relevant_masks(&with_view, task.required);
                let probe = policy.probe_join(candidate, &view, &ctx);
                if let Some(p) = &probe {
                    inc.record(p);
                    if eq3_arm && p.knife_edge && p.zero_proof.is_none() {
                        // The one join site that still evaluates fresh on the
                        // EQ3 arm.
                        if let Some((path, _)) =
                            probe_fresh_factorization(&with_view, task.required)
                        {
                            p7_count_path(&mut paths, path);
                        }
                    }
                }
                let bucket = if masks_without.is_empty() {
                    0
                } else if masks_with.len() == masks_without.len() {
                    1
                } else {
                    match &probe {
                        Some(p) if p.zero_proof.is_some() && eq3_arm => 4,
                        Some(p) if p.knife_edge => 3,
                        _ => 2,
                    }
                };

                let t0 = Instant::now();
                let d = policy.should_join(candidate, &view, &ctx);
                buckets[bucket].push(seconds_to_us(t0.elapsed()));
                if d.act {
                    members.push(idx);
                }
            }

            for &idx in &task.order {
                let Some(pos) = members.iter().position(|&m| m == idx) else {
                    continue;
                };
                let view = coalition_view(&agents, &members);
                let agent: &dyn AgentCapabilities = &agents[idx];

                if eq3_arm {
                    // Variant-A leaves pay two fresh evaluations per decision.
                    let without: Vec<&dyn AgentCapabilities> = view
                        .iter()
                        .filter(|a| a.agent_id() != agent.agent_id())
                        .copied()
                        .collect();
                    for v in [&view, &without] {
                        if let Some((path, _)) = probe_fresh_factorization(v, task.required) {
                            p7_count_path(&mut paths, path);
                        }
                    }
                }

                let t0 = Instant::now();
                let d = policy.should_leave(agent, &view, &ctx);
                buckets[5].push(seconds_to_us(t0.elapsed()));
                if d.act {
                    members.remove(pos);
                }
            }
        }
    }

    (buckets, inc, paths)
}

#[cfg(feature = "magnitude-fast")]
fn p7_count_path(paths: &mut [usize; 3], path: FactorizationPath) {
    match path {
        FactorizationPath::Cholesky => paths[0] += 1,
        FactorizationPath::Lblt => paths[1] += 1,
        FactorizationPath::GaussJordan => paths[2] += 1,
    }
}

fn part7_eq3_latency_rematch() {
    println!("# koalisi #69 — Part 7: EQ3 latency re-match (REGISTERED)");
    println!();
    println!(
        "_governed by `docs/prereg-K4-eq3-latency-rematch.md` + its **Amendment 1** (both committed before this code). Report date {P7_REPORT_DATE}. Regime v2-draw · Scope B · seeds **{P7_SEED_START}..{P7_SEED_END}** (fresh; 90..120 and 150..180 stay reserved) · release build. Arms: `scalar` = `AifDecisionPolicy::default()` (the v1 latency comparator, quality context-only), `mag` = `MagnitudePolicy::default()` (**L1 only** — A1.1 parked L2 behind the toggle), `mag-eq3` = the same policy with `.with_eq3_levers(true)` (**L2 + L3**). Latency = the v1 protocol verbatim: `Instant` around every sync `should_join`/`should_leave`, pooled per arm across all seeds._"
    );
    println!();

    #[cfg(feature = "magnitude-fast")]
    part7_run();

    #[cfg(not(feature = "magnitude-fast"))]
    println!(
        "**SKIPPED — cargo feature `magnitude-fast` is OFF.** The `mag-eq3` arm is the toggle-ON policy, which only exists under that feature, so no leg of this registered battery can run. Re-run with `--features decision,magnitude,magnitude-fast` (the official run's build) to produce the Part 7 rows. Parts 1–6 above are unaffected either way."
    );
    println!();
}

#[cfg(feature = "magnitude-fast")]
#[allow(clippy::too_many_lines)]
fn part7_run() {
    // --- The three arms, identical per-seed instances ----------------------
    let (scalar, scalar_lat) = stateless_battery_mode(
        || Box::new(AifDecisionPolicy::default()) as Box<dyn CoalitionDecisionPolicy>,
        Regime::V2,
        P7_SEED_START,
        P7_SEED_END,
    );
    let (mag, mag_lat) = stateless_battery_mode(
        || Box::new(MagnitudePolicy::default()) as Box<dyn CoalitionDecisionPolicy>,
        Regime::V2,
        P7_SEED_START,
        P7_SEED_END,
    );
    let (eq3, eq3_lat) = stateless_battery_mode(
        || {
            Box::new(MagnitudePolicy::default().with_eq3_levers(true))
                as Box<dyn CoalitionDecisionPolicy>
        },
        Regime::V2,
        P7_SEED_START,
        P7_SEED_END,
    );

    let (scalar_med, scalar_iqr) = median_iqr(scalar_lat.clone());
    let (mag_med, mag_iqr) = median_iqr(mag_lat.clone());
    let (eq3_med, eq3_iqr) = median_iqr(eq3_lat.clone());

    let scalar_q = median(primaries_b(&scalar));
    let mag_q = median(primaries_b(&mag));
    let eq3_q = median(primaries_b(&eq3));

    println!("## Arms (pooled over {} seeds)", mag.len());
    println!();
    println!("| arm | median PRIMARY_B | median churn | median µs/decision | IQR µs | decisions |");
    println!("|-----|-----------------:|-------------:|-------------------:|-------:|----------:|");
    for (name, rs, q, med, iqr, lat) in [
        (
            "scalar",
            &scalar,
            scalar_q,
            scalar_med,
            scalar_iqr,
            &scalar_lat,
        ),
        ("mag", &mag, mag_q, mag_med, mag_iqr, &mag_lat),
        ("mag-eq3", &eq3, eq3_q, eq3_med, eq3_iqr, &eq3_lat),
    ] {
        println!(
            "| `{name}` | {q:.4} | {:.2} | {med:.3} | {iqr:.3} | {} |",
            median(churns_b(rs)),
            lat.len()
        );
    }
    println!();

    // --- H-par′ (i): first-divergence shape -------------------------------
    let mut paired = P7Paired::default();
    for seed in P7_SEED_START..P7_SEED_END {
        p7_paired_seed(seed, &mut paired);
    }
    let bad_shape = paired.firsts.iter().filter(|d| !d.shape_ok).count();
    let h_par_i = bad_shape == 0;

    println!("## H-par′ (i) — first-divergence shape (Amendment 1 / A1.2)");
    println!();
    println!(
        "Paired walk: `mag` and `mag-eq3` over identical instances and arrival orders, each carrying its own membership. **{} compared decisions** ({} leave steps skipped as structural cascade, where only one arm still held the member). **{} task-level first divergences**; each must carry a fired `ZeroDiversityProof` for `mag-eq3` AND `|mag margin| ≤ {P7_SHAPE_NOISE:.0e}`.",
        paired.compared,
        paired.structural,
        paired.firsts.len()
    );
    println!();
    if paired.firsts.is_empty() {
        println!("No divergence anywhere — the shape conjunct holds vacuously.");
    } else {
        println!("| seed | task | kind | certificate | mag margin | shape |");
        println!("|-----:|-----:|------|-------------|-----------:|-------|");
        for d in paired.firsts.iter().take(40) {
            println!(
                "| {} | {} | {} | {} | {:.3e} | {} |",
                d.seed,
                d.task,
                if d.leave { "leave" } else { "join" },
                match d.proof {
                    Some(ZeroDiversityProof::SkeletalMerge { .. }) => "SkeletalMerge",
                    Some(ZeroDiversityProof::IncomingProfileDuplicate { .. }) => "incoming-dup",
                    Some(ZeroDiversityProof::OutgoingProfileDuplicate { .. }) => "outgoing-dup",
                    None => "— (none fired)",
                },
                d.mag_margin,
                pass(d.shape_ok)
            );
        }
        if paired.firsts.len() > 40 {
            println!("| … | | | | | _{} more_ |", paired.firsts.len() - 40);
        }
    }
    println!();
    println!(
        "- Cascaded divergent decisions after a task's first (exempt by A1.2, context only): **{}**.",
        paired.cascade
    );
    println!(
        "- Leave-sweep note: the default leave variant is the FRESH two-evaluation path, which never consults the evaluator — so L2 cannot fire on a leave and any leave first-divergence is L3 arithmetic, failing the shape by construction."
    );
    println!(
        "- **H-par′ (i): {}** ({} of {} first divergences carry the certified shape).",
        pass(h_par_i),
        paired.firsts.len() - bad_shape,
        paired.firsts.len()
    );
    println!();

    // --- H-par′ (ii): quality non-inferiority ------------------------------
    let bar = P7_NONINFERIORITY * mag_q;
    let h_par_ii = eq3_q >= bar;
    println!("## H-par′ (ii) — quality non-inferiority");
    println!();
    println!(
        "median PRIMARY_B: `mag-eq3` **{eq3_q:.4}** vs `mag` {mag_q:.4} · bar = {P7_NONINFERIORITY} × {mag_q:.4} = **{bar:.4}** ⇒ **{}**.",
        pass(h_par_ii)
    );
    println!();
    println!("| seed | mag | mag-eq3 | Δ |");
    println!("|-----:|----:|--------:|--:|");
    for (i, (m, e)) in mag.iter().zip(eq3.iter()).enumerate() {
        println!(
            "| {} | {:.4} | {:.4} | {:+.4} |",
            P7_SEED_START + i as u64,
            m.primary,
            e.primary,
            e.primary - m.primary
        );
    }
    println!();

    // --- H-lat --------------------------------------------------------------
    let h_lat = eq3_med < scalar_med;
    println!("## H-lat — strict Path-A analogue");
    println!();
    println!(
        "pooled median per-decision latency: `mag-eq3` **{eq3_med:.3} µs** vs `scalar` {scalar_med:.3} µs ⇒ **{}** (strict `<`).",
        pass(h_lat)
    );
    println!();

    // --- Verdict ------------------------------------------------------------
    let h_par = h_par_i && h_par_ii;
    let verdict = if !h_par {
        "FALSIFIED (parity)"
    } else if h_lat {
        "VALIDATED (latency re-match)"
    } else {
        "FALSIFIED (latency re-match)"
    };
    println!("## VERDICT: **{verdict}**");
    println!();
    println!(
        "_Grammar (Amendment 1): `VALIDATED (latency re-match)` = H-par′ ∧ H-lat · `FALSIFIED (latency re-match)` = H-par′ ∧ ¬H-lat · `FALSIFIED (parity)` = ¬H-par′ (either conjunct), regardless of H-lat. This run: H-par′(i) {} · H-par′(ii) {} · H-lat {}._",
        pass(h_par_i),
        pass(h_par_ii),
        pass(h_lat)
    );
    println!();

    // --- Registered context rows (never gated) -----------------------------
    println!("## Context (registered, never gated)");
    println!();
    println!(
        "- **Lever decomposition:** `mag` (toggle OFF, L1 only) median {mag_med:.3} µs vs `mag-eq3` (L2+L3) {eq3_med:.3} µs — the difference is what L2+L3 buy on top of the scratch adoption."
    );
    println!(
        "- **Ratios:** `mag-eq3`/`scalar` = {:.2}× · `mag`/`scalar` = {:.2}×.",
        eq3_med / scalar_med,
        mag_med / scalar_med
    );
    println!(
        "- **Gap-shrink vs the K6 reference** (mag {P7_K6_MAG_US:.3} µs vs aif {P7_K6_AIF_US:.3} µs = {:.2}×) — **cross-regime**: K6 is the v1 draw and this battery is v2, so the ratio, not the absolute µs, is the comparable quantity. This run: {:.2}× (`mag`) and {:.2}× (`mag-eq3`).",
        P7_K6_MAG_US / P7_K6_AIF_US,
        mag_med / scalar_med,
        eq3_med / scalar_med
    );
    println!(
        "- **Quality medians** (v2 regime; #61 Part 5a context rows were mag 0.1286 / scalar 0.1332): `scalar` {scalar_q:.4} · `mag` {mag_q:.4} · `mag-eq3` {eq3_q:.4}."
    );
    println!(
        "- The frozen Part 2 `mag` latency line in THIS run is printed above in Part 2 (v1 regime, L1 adopted); the pre-change session baselines were 3.566 / 3.793 µs. Latency-only by construction — every Part 2 quality column is byte-identical (the X-A/X-B gate)."
    );
    println!();

    // --- Registered instrumentation (non-gating) ---------------------------
    let (mag_buckets, mag_inc, _) = p7_instrumentation_pass(false);
    let (eq3_buckets, eq3_inc, eq3_paths) = p7_instrumentation_pass(true);

    println!("## Instrumentation (registered, non-gating)");
    println!();
    println!(
        "### Increment distribution — `|with − base|` on `mag`'s join stream ({} probed decisions)",
        mag_inc.probed
    );
    println!();
    println!("| decade | count |");
    println!("|--------|------:|");
    println!("| exact 0 | {} |", mag_inc.exact_zero);
    for (i, c) in mag_inc.decades.iter().enumerate() {
        if *c > 0 {
            let lo = i as i32 - 16;
            if i == 16 {
                println!("| ≥ 1e0 | {c} |");
            } else {
                println!("| [1e{lo}, 1e{}) | {c} |", lo + 1);
            }
        }
    }
    println!();
    println!(
        "- The cg#153 hypothesis is an EMPTY `[1e-13, 1e-6)` band: measured here at **{}** decisions in that range. Non-gating; no band change ships in EQ3 (prereg non-goal).",
        mag_inc.decades[3..10].iter().sum::<usize>()
    );
    println!(
        "- Knife-edge population on the same stream: **{}** of {} probed joins ({:.1}%).",
        mag_inc.knife,
        mag_inc.probed,
        100.0 * mag_inc.knife as f64 / mag_inc.probed.max(1) as f64
    );
    println!();

    println!("### Proof fire-rate by class — `mag-eq3` arm");
    println!();
    println!(
        "| class | count | share of probed joins |\n|-------|------:|----------------------:|"
    );
    for (label, n) in [
        ("SkeletalMerge", eq3_inc.by_class[0]),
        ("incoming-dup", eq3_inc.by_class[1]),
        ("outgoing-dup", eq3_inc.by_class[2]),
    ] {
        println!(
            "| {label} | {n} | {:.1}% |",
            100.0 * n as f64 / eq3_inc.probed.max(1) as f64
        );
    }
    println!(
        "| **all** | **{}** | **{:.1}%** |",
        eq3_inc.proofs(),
        100.0 * eq3_inc.proofs() as f64 / eq3_inc.probed.max(1) as f64
    );
    println!();
    println!(
        "- **Former knife-edge recomputes retired:** {} of {} band decisions now carry a certificate and skip the fresh `O(m³)` recompute (**{:.1}%**).",
        eq3_inc.knife_certified,
        eq3_inc.knife,
        100.0 * eq3_inc.knife_certified as f64 / eq3_inc.knife.max(1) as f64
    );
    println!();

    println!(
        "### Latency decomposition by bucket (instrumentation pass — NOT the pooled measurement)"
    );
    println!();
    println!("| bucket | mag count | mag median µs | eq3 count | eq3 median µs |");
    println!("|--------|----------:|--------------:|----------:|--------------:|");
    for (i, label) in P7_BUCKETS.iter().enumerate() {
        let m = &mag_buckets[i];
        let e = &eq3_buckets[i];
        println!(
            "| {label} | {} | {} | {} | {} |",
            m.len(),
            if m.is_empty() {
                "—".to_owned()
            } else {
                format!("{:.3}", median(m.clone()))
            },
            e.len(),
            if e.is_empty() {
                "—".to_owned()
            } else {
                format!("{:.3}", median(e.clone()))
            }
        );
    }
    println!();
    println!(
        "- **Deviation from the registered bucket list:** the K6 table splits every join bucket by `rebuild` vs `hit` (evaluator reconstructed vs cached). That split is only visible from inside the policy's cache, and the only cheap way to expose it is a counter on the decision path — which would instrument the very code the H-lat measurement times. Protecting the registered measurement won; the rebuild/hit split is NOT reproduced here, and the `band`/`clear`/`excluded`/`empty`/`leave` classes are, via the read-only `probe_join` surface. The prereg's \"where cheaply available\" wording covers this; recorded as a deviation either way."
    );
    println!(
        "- This pass probes before each timed decision, so its absolute µs carry probe-induced cache disturbance. Read the SHAPE (which bucket costs what), not the level; the pooled table at the top is the clean measurement."
    );
    println!();

    let path_total: usize = eq3_paths.iter().sum();
    println!("### `FactorizationPath` counts — `mag-eq3` fresh evaluations");
    println!();
    println!(
        "| path | count | share |\n|------|------:|------:|\n| Cholesky | {} | {:.1}% |\n| LBLT (Bunch–Kaufman) | {} | {:.1}% |\n| Gauss–Jordan fallback | {} | {:.1}% |",
        eq3_paths[0],
        100.0 * eq3_paths[0] as f64 / path_total.max(1) as f64,
        eq3_paths[1],
        100.0 * eq3_paths[1] as f64 / path_total.max(1) as f64,
        eq3_paths[2],
        100.0 * eq3_paths[2] as f64 / path_total.max(1) as f64
    );
    println!();
    println!(
        "- Counted over the sites the EQ3 arm actually evaluates fresh: both magnitudes of every variant-A leave decision, plus the `with` side of every unprovable-knife-edge join. One factorization per count — the handle that reports the path is the handle that answers the magnitude (`t = 1` makes the scaling a no-op), so no extra factorization is paid to report this."
    );
    println!();
}

#[cfg(test)]
mod part4c_tests {
    use super::*;

    /// 2-seed smoke: the persistent arm pipeline (fresh arm per seed + per-bit
    /// outcome hook + join/leave decisions) executes end-to-end and produces finite
    /// metrics, and the scalar comparison arm runs alongside. Does NOT run the full
    /// 30-seed registered battery.
    #[test]
    fn part4c_two_seed_smoke() {
        let (pers, lat) = persistent_battery(PersistentAifConfig::default(), 2);
        assert_eq!(pers.len(), 2);
        for r in &pers {
            assert!(r.primary.is_finite() && (0.0..=1.0).contains(&r.primary));
            assert!(!r.acts.is_empty(), "some join/leave decisions must have run");
        }
        assert!(!lat.is_empty(), "latencies recorded");

        let (scalar, _) = stateless_battery_b(
            || Box::new(AifDecisionPolicy::default()) as Box<dyn CoalitionDecisionPolicy>,
            2,
        );
        assert_eq!(scalar.len(), 2);

        // An exploratory toggle also runs end-to-end on 2 seeds.
        let (e5, _) = persistent_battery(
            PersistentAifConfig { persistent_learning: false, ..PersistentAifConfig::default() },
            2,
        );
        assert_eq!(e5.len(), 2);
    }

    /// 2-seed smoke for Part 4d: the registered `aif-e1` (E6/MeanField) arm runs
    /// end-to-end on the out-of-sample seeds 30..32 alongside the scalar/mag arms.
    /// Does NOT run the full 30-seed registered battery.
    #[test]
    fn part4d_two_seed_smoke() {
        let (e1, lat) = persistent_battery_range(e1_config(), 30, 32);
        assert_eq!(e1.len(), 2);
        for r in &e1 {
            assert!(r.primary.is_finite() && (0.0..=1.0).contains(&r.primary));
            assert!(!r.acts.is_empty(), "some join/leave decisions must have run");
        }
        assert!(!lat.is_empty(), "latencies recorded");

        let (scalar, _) = stateless_battery_range(
            || Box::new(AifDecisionPolicy::default()) as Box<dyn CoalitionDecisionPolicy>,
            30,
            32,
        );
        assert_eq!(scalar.len(), 2);
        let mag = MagnitudePolicy::default();
        let (mag_rs, _) = stateless_battery_range(
            || Box::new(mag.clone()) as Box<dyn CoalitionDecisionPolicy>,
            30,
            32,
        );
        assert_eq!(mag_rs.len(), 2);
    }

    /// 2-seed smoke for Part 4e: the DEGRADED-signal persistent battery (E1 arm
    /// fed only whole-coalition success, smeared across the required bits) runs
    /// end-to-end on the out-of-sample seeds 30..32 and produces finite metrics.
    #[test]
    fn part4e_two_seed_smoke() {
        let (deg, lat) = persistent_battery_range_degraded(e1_config(), 30, 32);
        assert_eq!(deg.len(), 2);
        for r in &deg {
            assert!(r.primary.is_finite() && (0.0..=1.0).contains(&r.primary));
            assert!(!r.acts.is_empty(), "some join/leave decisions must have run");
        }
        assert!(!lat.is_empty(), "latencies recorded");
    }

    /// 2-seed smoke for Part 4f: (a) `MarginE1(0, 0)` reproduces the bare arm
    /// (the identity property at 2-seed scale), and (b) a tightened degraded cell
    /// runs end-to-end with finite in-range metrics.
    #[test]
    fn part4f_two_seed_smoke() {
        let (m, _) = margin_battery_range(e1_config(), 0.0, 0.0, false, 30, 32, None);
        let (r, _) = persistent_battery_range(e1_config(), 30, 32);
        assert_eq!(m.len(), 2);
        assert_eq!(r.len(), 2);
        for i in 0..m.len() {
            assert!(
                m[i].primary.to_bits() == r[i].primary.to_bits() && m[i].churn == r[i].churn,
                "MarginE1(0,0) must reproduce the bare arm"
            );
        }

        let (deg, _) = margin_battery_range(e1_config(), 0.15, 0.05, true, 30, 32, None);
        assert_eq!(deg.len(), 2);
        for x in &deg {
            assert!(x.primary.is_finite() && (0.0..=1.0).contains(&x.primary));
        }
    }

    /// 2-seed smoke for Part 4g: (a) `RelFilteredMag(τ=0)` reproduces the bare mag
    /// arm (the identity property at 2-seed scale), and (b) one active cell runs
    /// end-to-end with finite in-range metrics.
    #[test]
    fn part4g_two_seed_smoke() {
        let (idg, _) = rel_mag_battery_range(0.0, 1, false, 30, 32);
        let mag = MagnitudePolicy::default();
        let (bare, _) = stateless_battery_range(
            || Box::new(mag.clone()) as Box<dyn CoalitionDecisionPolicy>,
            30,
            32,
        );
        assert_eq!(idg.len(), 2);
        assert_eq!(bare.len(), 2);
        for i in 0..idg.len() {
            assert!(
                idg[i].primary.to_bits() == bare[i].primary.to_bits()
                    && idg[i].churn == bare[i].churn,
                "RelFilteredMag(τ=0) must reproduce bare mag"
            );
        }

        let (active, _) = rel_mag_battery_range(0.5, 1, true, 30, 32);
        assert_eq!(active.len(), 2);
        for x in &active {
            assert!(x.primary.is_finite() && (0.0..=1.0).contains(&x.primary));
        }
    }

    /// 2-seed smoke for Part 4h (#56): the never-evict arm (`eviction_cap: Some(0)`)
    /// runs end-to-end on the registered seeds 60..62 with finite metrics AND zero
    /// churn (the never-evict structural guarantee); one lockout config also runs.
    #[test]
    fn part4h_two_seed_smoke() {
        let ne_cfg = PersistentAifConfig { eviction_cap: Some(0), ..e1_config() };
        let (ne, lat) = persistent_battery_range(ne_cfg, 60, 62);
        assert_eq!(ne.len(), 2);
        for r in &ne {
            assert!(r.primary.is_finite() && (0.0..=1.0).contains(&r.primary));
            assert_eq!(r.churn, 0, "never-evict ⇒ zero churn by construction");
        }
        assert!(!lat.is_empty(), "latencies recorded");

        let lk_cfg = PersistentAifConfig { rejoin_lockout_tasks: 1, ..e1_config() };
        let (lk, _) = persistent_battery_range_degraded(lk_cfg, 60, 62);
        assert_eq!(lk.len(), 2);
        for r in &lk {
            assert!(r.primary.is_finite() && (0.0..=1.0).contains(&r.primary));
        }
    }

    /// The battery-v2 draw (#61) has the registered shape: `|required| ∈ 2..=8`
    /// distinct bits of the 8-bit universe, pool and cap draws unchanged.
    #[test]
    fn v2_draw_shape() {
        for seed in 120..126 {
            let mut rng = SplitMix64::new(seed);
            let (agents, tasks) = draw_prefix_v2(&mut rng);
            assert!((4..=16).contains(&agents.len()), "pool draw unchanged");
            for a in &agents {
                assert!((1..=4).contains(&a.caps.count_ones()), "cap draw unchanged");
            }
            assert_eq!(tasks.len(), TASKS);
            for t in &tasks {
                let r = t.required.count_ones();
                assert!((2..=8).contains(&r), "v2 draw is |required| in 2..=8, got {r}");
                assert_eq!(t.order.len(), agents.len());
            }
        }
    }

    /// 2-seed smoke for Part 5a (#61): (a) the δ = 0 margin wrapper reproduces
    /// the unwrapped arm in the v2 regime (the X-B(b) property at 2-seed scale),
    /// and (b) a de-saturated cell (γ = 1, δ = 0.15) runs end-to-end with finite
    /// in-range metrics. Does NOT run the 18-cell registered factorial.
    #[test]
    fn part5a_two_seed_smoke() {
        let cfg = e1_gamma_config(1.0);
        let mode = RunMode { regime: Regime::V2, degraded: true };
        let (wrapped, _) = margin_battery_mode(cfg, 0.0, 0.0, mode, 120, 122, None);
        let (bare, _) = persistent_battery_mode(cfg, mode, 120, 122);
        assert_battery_identical(&wrapped, &bare, "MarginE1(0,0) must reproduce the bare arm");

        let (active, lat) = margin_battery_mode(cfg, 0.15, 0.0, mode, 120, 122, None);
        assert_eq!(active.len(), 2);
        for r in &active {
            assert!(r.primary.is_finite() && (0.0..=1.0).contains(&r.primary));
        }
        assert!(!lat.is_empty(), "latencies recorded");
    }

    /// Part 5b (#61) instance draw has the registered shape.
    #[test]
    fn v2b_routing_instance_shape() {
        for seed in 120..126 {
            let inst = draw_routing_instance(seed);
            assert!((8..=16).contains(&inst.agents.len()), "pool n in 8..=16");
            for a in &inst.agents {
                assert!((1..=4).contains(&a.caps.count_ones()), "caps 1..=4 bits");
            }
            let m = inst.required.count_ones();
            assert!(m == 7 || m == 8, "m must be 7 or 8, got {m}");
            assert!(
                inst.required & (1u32 << inst.b_star) != 0,
                "b* must be a required bit"
            );
            assert!((inst.reliability[inst.b_star] - V2B_WEAK_R).abs() < 1e-12);
            for b in (0..UNIVERSE).filter(|&b| b != inst.b_star) {
                assert!((inst.reliability[b] - V2B_STRONG_R).abs() < 1e-12);
            }
        }
    }

    /// At `r ≡ 1` every bit succeeds with certainty, so the expected realized
    /// payoff must equal the unweighted model's own fitness exactly — a closed-form
    /// self-check on [`real_payoff`] independent of how it is derived.
    #[test]
    fn v2b_real_reduces_to_fitness_at_r_one() {
        let inst = draw_routing_instance(123);
        let calc = TaskCoverageV2::unweighted(inst.required);
        let cfg = PopulationConfig::default().with_seed(123);
        let best = search(&inst.agents, &calc, &cfg).best;

        let direct: f64 = best
            .blocks()
            .iter()
            .map(|blk| calc.calculate_value(&coalition_view(&inst.agents, blk)))
            .sum();
        let real = real_payoff(&best, &inst.agents, inst.required, &[1.0; UNIVERSE]);
        assert!(
            (real - direct).abs() < 1e-9,
            "REAL at r = 1 must equal the unweighted fitness: {real} vs {direct}"
        );
    }

    /// The prereg's coefficient properties hold (also asserted in the run path).
    #[test]
    fn v2b_coefficient_properties() {
        assert_v2b_coefficient_properties();
    }

    /// `query_gamma: Some(16.0)` restates the engine default, so arm-E1g16 must
    /// reproduce the frozen arm-E1 — the X-B(a) identity property at 2-seed scale.
    #[test]
    fn part5a_gamma16_identity() {
        let (g16, _) = persistent_battery_range(e1_gamma_config(16.0), 30, 32);
        let (g_none, _) = persistent_battery_range(e1_config(), 30, 32);
        assert_battery_identical(&g16, &g_none, "Some(16.0) must reproduce None");
    }

    /// Part 5c (#61) item 1: the widened draw has the registered shape — a 12-bit
    /// universe, `|required| ∈ 2..=12`, worker caps `1..=6`, pool draw unchanged.
    #[test]
    fn w12_draw_shape() {
        for seed in 120..126 {
            let mut rng = SplitMix64::new(seed);
            let (agents, tasks) = draw_prefix_w12(&mut rng);
            assert!((4..=16).contains(&agents.len()), "pool draw unchanged");
            for a in &agents {
                assert!((1..=6).contains(&a.caps.count_ones()), "w12 caps are 1..=6 bits");
                assert_eq!(
                    a.caps >> W12_UNIVERSE_BITS,
                    0,
                    "caps stay inside the 12-bit universe"
                );
            }
            assert_eq!(tasks.len(), TASKS);
            for t in &tasks {
                let r = t.required.count_ones();
                assert!((2..=12).contains(&r), "w12 draw is |required| in 2..=12, got {r}");
                assert_eq!(t.required >> W12_UNIVERSE_BITS, 0, "required stays in 12 bits");
                assert_eq!(t.order.len(), agents.len());
            }
        }
    }

    /// Part 5c (#61) item 1, 2-seed smoke: the `w12-draw` regime threads through
    /// the shared runner end-to-end — instances draw, the decision stream runs,
    /// and the per-task outcome slice handed to the hook is 12 wide (the widening
    /// that lets a 12-bit arm learn at all). Driven by the STATELESS arms, which
    /// are universe-agnostic and cheap; the 12-bit persistent arm itself is
    /// exercised by `part5c_item1_w12_arm_decides` (a bounded number of decisions)
    /// and, at full battery scale, by the `#[ignore]`d smoke below.
    #[test]
    fn part5c_item1_two_seed_smoke() {
        let mut widths: Vec<usize> = Vec::new();
        let mut lat = Vec::new();
        let r = run_seed_b_regime(
            &AifDecisionPolicy::default(),
            120,
            Regime::W12,
            &mut lat,
            |_req, bits, _success, _members| widths.push(bits.len()),
        );
        assert!(r.primary.is_finite() && (0.0..=1.0).contains(&r.primary));
        assert!(!r.acts.is_empty(), "some join/leave decisions must have run");
        assert_eq!(widths.len(), TASKS, "one outcome per task");
        assert!(
            widths.iter().all(|&w| w == W12_UNIVERSE_BITS as usize),
            "the w12 outcome slice must be 12 wide, got {widths:?}"
        );

        let mag = MagnitudePolicy::default();
        let (mag_rs, _) = stateless_battery_mode(
            || Box::new(mag.clone()) as Box<dyn CoalitionDecisionPolicy>,
            Regime::W12,
            120,
            122,
        );
        assert_eq!(mag_rs.len(), 2);
        for x in &mag_rs {
            assert!(x.primary.is_finite() && (0.0..=1.0).contains(&x.primary));
        }
    }

    /// Part 5c (#61) item 1: the 12-bit arm decides on REAL `w12-draw` masks (up
    /// to 12 required bits — a query joint of `2^13`, unreachable at the 8-bit
    /// default) and learns from a 12-wide outcome. Bounded to one task's worth of
    /// decisions so the suite stays fast; the arm's own construction/observation
    /// contract is pinned library-side by
    /// `decision::aif_persistent_policy::tests::twelve_bit_arm_observes_and_decides`.
    #[test]
    fn part5c_item1_w12_arm_decides() {
        let (agents, tasks, _rho, _perf) = generate_instance_b_regime(120, Regime::W12);
        let task = &tasks[0];
        let arm = PersistentAifArm::new(120, e1_w12_config(P5C_W12_GAMMA))
            .expect("12-bit persistent arm construction");
        let ctx = DecisionContext { required_capabilities: task.required };

        let mut members: Vec<usize> = vec![task.order[0]];
        for &idx in task.order[1..].iter().take(3) {
            let candidate: &dyn AgentCapabilities = &agents[idx];
            let coalition = coalition_view(&agents, &members);
            let d = arm.should_join(candidate, &coalition, &ctx);
            assert!(d.score.is_finite() && (-0.5..=0.5).contains(&d.score));
            if d.act {
                members.push(idx);
            }
        }
        let coalition = coalition_view(&agents, &members);
        let member: &dyn AgentCapabilities = &agents[members[0]];
        let d = arm.should_leave(member, &coalition, &ctx);
        assert!(d.score.is_finite() && (-0.5..=0.5).contains(&d.score));

        arm.observe_outcome(task.required, &vec![true; W12_UNIVERSE_BITS as usize]);
        assert_eq!(
            arm.state_snapshot().beliefs.len(),
            W12_UNIVERSE_BITS as usize
        );
    }

    /// Part 5c (#61) item 1, full 2-seed battery smoke — `#[ignore]`d because the
    /// 12-bit slice is prohibitively slow in a DEBUG build (`|required|` reaches
    /// 12, so a query carries 12 modalities over a `2^13` joint, and unoptimized
    /// `nalgebra` turns a 2-seed run into many minutes). The printed slice itself
    /// runs `--release`, per the example's own header. Run it with
    /// `cargo test --release ... -- --ignored`.
    #[test]
    #[ignore = "12-bit battery is debug-prohibitive; run --release with --ignored"]
    fn part5c_item1_w12_battery_smoke() {
        let cfg = e1_w12_config(P5C_W12_GAMMA);
        let mode = RunMode { regime: Regime::W12, degraded: true };
        let (wrapped, lat) = margin_battery_mode(cfg, 0.0, 0.0, mode, 120, 122, None);
        let (bare, _) = persistent_battery_mode(cfg, mode, 120, 122);
        assert_battery_identical(
            &wrapped,
            &bare,
            "MarginE1(0,0) must reproduce the bare arm on the 12-bit slice",
        );
        assert_eq!(wrapped.len(), 2);
        for r in &wrapped {
            assert!(r.primary.is_finite() && (0.0..=1.0).contains(&r.primary));
        }
        assert!(!lat.is_empty(), "latencies recorded");
    }

    /// Part 5c (#61) item 2 smoke: each registered hysteresis cell runs end-to-end
    /// with finite in-range metrics on the sweep's own (γ = 1, v2-draw, degraded)
    /// cell. The `h = 0` baseline's identity with the unwrapped arm is already
    /// pinned by `part5a_two_seed_smoke` (same config, same regime), so it is not
    /// re-run here — this cell is expensive in a debug build. Churn is deliberately
    /// NOT asserted monotone in `h`: a suppressed eviction changes membership and
    /// every later decision, so the run-level effect is measured, not guaranteed.
    #[test]
    fn part5c_item2_smoke() {
        let cfg = e1_gamma_config(P5C_HYSTERESIS_GAMMA);
        let mode = RunMode { regime: Regime::V2, degraded: true };
        for &h in &P5C_H_GRID {
            let (rs, lat) = margin_battery_mode(cfg, 0.0, h, mode, 120, 121, None);
            assert_eq!(rs.len(), 1);
            for r in &rs {
                assert!(r.primary.is_finite() && (0.0..=1.0).contains(&r.primary));
                assert!(!r.acts.is_empty(), "some join/leave decisions must have run");
            }
            assert!(!lat.is_empty(), "latencies recorded");
        }
    }

    /// Part 5c (#61) item 3: the expected-outcome model is exactly the per-block
    /// decomposition of [`real_payoff`], on a 2-seed slice of the identity gate.
    #[test]
    fn part5c_item3_model_matches_real_payoff() {
        for seed in 120..122 {
            let inst = draw_routing_instance(seed);
            let cfg = PopulationConfig::default().with_seed(seed);
            let calc = ExpectedOutcomeV2::new(inst.required, inst.reliability);
            let best = search(&inst.agents, &calc, &cfg).best;
            let via_blocks = blocks_fitness(&best.blocks(), &inst.agents, &calc);
            let via_real = real_payoff(&best, &inst.agents, inst.required, &inst.reliability);
            assert!(
                (via_blocks - via_real).abs() < 1e-9,
                "seed {seed}: {via_blocks} vs {via_real}"
            );

            // The member term is partition-constant, which is the algebraic half of
            // the degeneracy argument the printed analysis rests on: two partitions
            // of the same pool differ only in their coverage terms. (Whether
            // all-singletons actually wins is MEASURED in the run, not asserted —
            // a merge that creates full coverage can pay for the overlap it
            // destroys, so the direction is data, not a theorem.)
            let n = inst.agents.len();
            let singletons: Vec<Vec<usize>> = (0..n).map(|i| vec![i]).collect();
            let member_cost_of = |blocks: &[Vec<usize>]| -> f64 {
                blocks.iter().map(|b| b.len() as f64 * V2B_MEMBER_COST).sum()
            };
            assert!(
                (member_cost_of(&singletons) - member_cost_of(&best.blocks())).abs() < 1e-9,
                "seed {seed}: the member term must be constant across partitions"
            );
        }
    }

    /// Part 5c (#61) item 4, 2-seed smoke: the learned-posterior pipeline runs and
    /// yields probabilities; the unobserved (non-required) bits stay at the 0.5
    /// prior, which is the gotcha-24 caveat the printed table rests on.
    #[test]
    fn part5c_item4_two_seed_smoke() {
        for seed in 120..122 {
            let inst = draw_routing_instance(seed);
            let r_hat = p5c_learned_reliability(&inst, seed);
            for (b, &r) in r_hat.iter().enumerate() {
                assert!(r.is_finite() && (0.0..=1.0).contains(&r), "r̂[{b}] = {r}");
                if inst.required & (1u32 << b) == 0 {
                    assert!(
                        (r - 0.5).abs() < 1e-9,
                        "an unobserved bit must keep the 0.5 prior, got r̂[{b}] = {r}"
                    );
                }
            }
            // The twin is a plain calculator swap — the search runs end-to-end.
            let cfg = PopulationConfig::default().with_seed(seed);
            let learned =
                search(&inst.agents, &TaskCoverageV2::weighted(inst.required, r_hat), &cfg).best;
            assert_eq!(learned.assignment.len(), inst.agents.len());
        }
    }

    /// Part 5c (#61): items 3 and 4 are search-only (no persistent battery), so
    /// their PRINTED sections are cheap enough to execute end-to-end here — which
    /// is what pins the `assert_p5c_expected_outcome_identity` gate over the whole
    /// confirmatory seed range and every median/format path in between. Items 1
    /// and 2 run batteries and are covered by their own smokes instead.
    #[test]
    fn part5c_items3_and_4_print_paths() {
        part5c_item3_expected_outcome();
        part5c_item4_learned_twins();
    }

    /// Part 5c (#61): the twin outcome stream must not perturb the Part 5b draw —
    /// `draw_routing_instance` uses its own `SplitMix64`, so running the learned
    /// pipeline leaves a re-drawn instance bit-identical.
    #[test]
    fn part5c_twin_stream_does_not_perturb_5b() {
        for seed in 120..123 {
            let before = draw_routing_instance(seed);
            let _ = p5c_learned_reliability(&before, seed);
            let after = draw_routing_instance(seed);
            assert_eq!(before.required, after.required);
            assert_eq!(before.b_star, after.b_star);
            assert_eq!(before.agents.len(), after.agents.len());
            for (x, y) in before.agents.iter().zip(&after.agents) {
                assert_eq!((x.id, x.caps, x.trust), (y.id, y.caps, y.trust));
            }
        }
    }

    /// Part 6 (#63) run-validity gate X-B (also asserted in the run path).
    #[test]
    fn p6_coefficient_gates_hold() {
        assert_p6_coefficient_gates();
    }

    /// Part 6 (#63): the product-form model coincides with the unweighted
    /// `TaskCoverageV2` at `r ≡ 1` — the registration's "leg C reuses argmax `U`"
    /// basis. Checked on all three branches (full coverage, partial, empty).
    #[test]
    fn p6_product_form_matches_unweighted_at_r1() {
        let required = 0b0111_1111u32; // m = 7
        let specialists: Vec<Worker> = (0..7)
            .map(|b| Worker {
                id: b,
                caps: 1u32 << b,
                trust: 50,
            })
            .collect();
        let unweighted = TaskCoverageV2::unweighted(required);
        let product = TaskCoverageV2P::new(required, [1.0; UNIVERSE]);

        let full: Vec<usize> = (0..7).collect();
        let partial: Vec<usize> = vec![0, 2, 5];
        for members in [full, partial, Vec::new()] {
            let view = coalition_view(&specialists, &members);
            let a = unweighted.calculate_value(&view);
            let b = product.calculate_value(&view);
            assert!(
                (a - b).abs() < 1e-9,
                "r = 1 must coincide on {members:?}: {a} vs {b}"
            );
        }
    }

    /// Part 6 (#63): the corrected draw honours its coverage guarantee, and it is
    /// a pure function of the seed (instance AND rejection count). Run over the
    /// WHOLE registered seed range — the draw carries no `search()`, so this is
    /// cheap, and it is what pins the rejection loop as terminating on every seed
    /// the battery will actually use.
    #[test]
    fn p6_corrected_draw_covers_required() {
        for seed in P6_SEED_START..P6_SEED_END {
            let (inst, rejections) = draw_routing_instance_corrected(seed);
            let pool_union = inst.agents.iter().fold(0u32, |acc, a| acc | a.caps);
            assert_eq!(
                pool_union & inst.required,
                inst.required,
                "seed {seed}: the pool union must cover every required bit"
            );
            let m = inst.required.count_ones();
            assert!(m == 7 || m == 8, "m must be 7 or 8, got {m}");
            assert!(
                inst.required & (1u32 << inst.b_star) != 0,
                "b* must be a required bit"
            );
            assert!((inst.reliability[inst.b_star] - P6A_WEAK_R).abs() < 1e-12);
            for b in (0..UNIVERSE).filter(|&b| b != inst.b_star) {
                assert!((inst.reliability[b] - P6A_OTHERS_R).abs() < 1e-12);
            }

            let (again, rejections_again) = draw_routing_instance_corrected(seed);
            assert_eq!(rejections, rejections_again);
            assert_eq!((again.required, again.b_star), (inst.required, inst.b_star));
            assert_eq!(again.agents.len(), inst.agents.len());
            for (x, y) in inst.agents.iter().zip(&again.agents) {
                assert_eq!((x.id, x.caps, x.trust), (y.id, y.caps, y.trust));
            }
        }
    }

    /// Part 6 (#63), 2-seed smoke: the per-seed leg-A machinery (corrected draw →
    /// three argmaxes → block-level conjuncts → diagnostics) runs end-to-end.
    ///
    /// The value here is EXECUTION, not logic: the conjuncts are one-line
    /// restatements of their own definitions, so asserting them back would test
    /// nothing. What can actually break is the pipeline — a panicking search, an
    /// out-of-range index, a non-finite payoff — and that is what this covers.
    /// Does NOT run the 30-seed battery.
    #[test]
    fn p6_two_seed_smoke() {
        for seed in P6_SEED_START..P6_SEED_START + 2 {
            let (inst, _) = draw_routing_instance_corrected(seed);
            let cfg = PopulationConfig::default().with_seed(seed);
            let b_star_bit = 1u32 << inst.b_star;

            let u_calc = TaskCoverageV2::unweighted(inst.required);
            let w_calc = TaskCoverageV2::weighted(inst.required, inst.reliability);
            let w_bar_calc = TaskCoverageV2::weighted(
                inst.required,
                plant(inst.b_star, P6A_OTHERS_R, P6A_OTHERS_R),
            );

            let u_best = search(&inst.agents, &u_calc, &cfg).best;
            let w_best = search(&inst.agents, &w_calc, &cfg).best;
            let w_bar_best = search(&inst.agents, &w_bar_calc, &cfg).best;

            let u_top = top_block_mask(&u_best, &inst.agents, &u_calc);
            let w_top = top_block_mask(&w_best, &inst.agents, &w_calc);
            let w_bar_top = top_block_mask(&w_bar_best, &inst.agents, &w_bar_calc);

            let _conjuncts = (
                w_top & b_star_bit == 0,
                u_top & b_star_bit != 0,
                w_bar_top & b_star_bit != 0,
            );

            // The diagnostics half of the per-seed work, exercised for the same
            // reason: these are the paths that can panic or index out of range.
            let u_top_block = top_block(&u_best, &inst.agents, &u_calc);
            let u_top_view = coalition_view(&inst.agents, &u_top_block);
            assert!(w_calc.calculate_value(&u_top_view).is_finite());
            assert!(w_bar_calc.calculate_value(&u_top_view).is_finite());
            assert_eq!(
                u_top_block
                    .iter()
                    .fold(0u32, |acc, &i| acc | inst.agents[i].caps),
                u_top,
                "top_block and top_block_mask must agree"
            );

            let min_mult = min_cover_multiplicity(&inst.agents, inst.required)
                .expect("the corrected draw guarantees pool coverage");
            assert!(
                min_mult >= inst.required.count_ones(),
                "a cover must carry at least one unit of multiplicity per required bit"
            );

            let real = real_payoff(&w_best, &inst.agents, inst.required, &inst.reliability);
            assert!(real.is_finite(), "REAL must be finite");
        }
    }

    /// Part 6 (#63): the [`p5c_learned_reliability`] wrapper is behaviour-identical
    /// to the parameterized [`learned_reliability`] at the P5C salt and task count —
    /// the refactor that let leg L reuse the pipeline on its own salt.
    #[test]
    fn p6_learned_wrapper_unchanged() {
        for seed in 120..122 {
            let inst = draw_routing_instance(seed);
            let via_wrapper = p5c_learned_reliability(&inst, seed);
            let via_inner = learned_reliability(&inst, seed, P5C_TWIN_SEED_SALT, P5C_TWIN_TASKS);
            assert_eq!(via_wrapper, via_inner, "seed {seed}");
            let salted = learned_reliability(&inst, seed, P6_TWIN_SEED_SALT, P5C_TWIN_TASKS);
            assert!(
                salted.iter().all(|r| r.is_finite() && (0.0..=1.0).contains(r)),
                "the Part 6 salt must still yield probabilities"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Part 7 (koalisi #69) — EQ3 latency re-match.
    // -----------------------------------------------------------------------

    /// Part 7 (#69): the v2 draw is pinned on the registered seed range. A
    /// determinism pin, not a property test — if `draw_prefix_v2` or the Scope-B
    /// suffix ever shifts, the registered instances shift with it and this
    /// catches it before a re-run silently reports different numbers.
    #[test]
    fn p7_v2_draw_determinism_pin() {
        for seed in P7_SEED_START..P7_SEED_START + 3 {
            let (a1, t1, r1, p1) = generate_instance_b_regime(seed, Regime::V2);
            let (a2, t2, r2, p2) = generate_instance_b_regime(seed, Regime::V2);

            // Same seed ⇒ byte-identical instance.
            assert_eq!(a1.len(), a2.len(), "seed {seed}: pool size");
            for (x, y) in a1.iter().zip(a2.iter()) {
                assert_eq!((x.id, x.caps, x.trust), (y.id, y.caps, y.trust));
            }
            for (x, y) in t1.iter().zip(t2.iter()) {
                assert_eq!(x.required, y.required);
                assert_eq!(x.order, y.order);
            }
            assert_eq!(r1, r2);
            assert_eq!(p1, p2);

            // The registered v2 envelope: pool 4..=16, caps 1..=4 distinct bits
            // from the 8-bit universe, |required| ∈ 2..=8 distinct bits.
            assert!(
                (4..=16).contains(&a1.len()),
                "seed {seed}: pool size envelope"
            );
            for w in &a1 {
                let k = w.caps.count_ones();
                assert!((1..=4).contains(&k), "seed {seed}: caps width {k}");
                assert_eq!(
                    w.caps >> UNIVERSE_BITS,
                    0,
                    "seed {seed}: caps outside universe"
                );
            }
            assert_eq!(t1.len(), TASKS, "seed {seed}: task count");
            for task in &t1 {
                let r = task.required.count_ones();
                assert!((2..=8).contains(&r), "seed {seed}: |required| = {r}");
                assert_eq!(
                    task.order.len(),
                    a1.len(),
                    "seed {seed}: arrival order length"
                );
            }
            assert_eq!(p1.len(), TASKS);
            assert_eq!(p1[0].len(), a1.len());
        }
    }

    /// Part 7 (#69): the increment histogram's bucketing — exact zeros are their
    /// own class (they have no decade), every other magnitude lands in the
    /// half-open decade `[1e(i-16), 1e(i-15))`, and anything `≥ 1e0` saturates
    /// the overflow bucket. Driven through synthetic probes so the arithmetic is
    /// checked independently of any battery.
    #[cfg(feature = "magnitude-fast")]
    #[test]
    fn p7_increment_histogram_bucketing() {
        // `base = 0` so `with − base` is EXACTLY `delta` — anchoring at 1.0
        // would silently round `1e-16` away (`1.0 + 1e-16 == 1.0`).
        fn probe(delta: f64, knife: bool, proof: Option<ZeroDiversityProof>) -> JoinProbe {
            JoinProbe {
                base: 0.0,
                with: delta,
                zero_proof: proof,
                knife_edge: knife,
            }
        }

        let mut inc = P7Increments::default();
        inc.record(&probe(0.0, true, None)); // exact zero
        inc.record(&probe(-0.0, false, None)); // negative zero is still zero
        inc.record(&probe(1e-16, false, None)); // bucket 0
        inc.record(&probe(5e-16, false, None)); // bucket 0 (same decade)
        inc.record(&probe(1e-15, false, None)); // bucket 1
        inc.record(&probe(1e-1, false, None)); // bucket 15
        inc.record(&probe(1.0, false, None)); // overflow
        inc.record(&probe(12.0, false, None)); // overflow (clamped)
        inc.record(&probe(1e-30, false, None)); // clamped into bucket 0

        assert_eq!(inc.probed, 9);
        assert_eq!(inc.exact_zero, 2, "±0.0 are the exact-zero class");
        assert_eq!(
            inc.decades[0], 3,
            "[1e-16,1e-15) plus the clamped underflow"
        );
        assert_eq!(inc.decades[1], 1);
        assert_eq!(inc.decades[15], 1);
        assert_eq!(inc.decades[16], 2, "≥ 1e0 saturates the overflow bucket");
        assert_eq!(
            inc.decades.iter().sum::<usize>() + inc.exact_zero,
            inc.probed,
            "every probe lands in exactly one class"
        );

        // Knife-edge and certificate tallies are independent of the decade.
        let mut inc = P7Increments::default();
        inc.record(&probe(
            1e-16,
            true,
            Some(ZeroDiversityProof::SkeletalMerge { member: 0 }),
        ));
        inc.record(&probe(
            1e-16,
            true,
            Some(ZeroDiversityProof::IncomingProfileDuplicate { member: 1 }),
        ));
        inc.record(&probe(
            1e-16,
            false,
            Some(ZeroDiversityProof::OutgoingProfileDuplicate { member: 2 }),
        ));
        inc.record(&probe(0.5, true, None));
        assert_eq!(inc.knife, 3);
        assert_eq!(
            inc.knife_certified, 2,
            "only certified band decisions retire"
        );
        assert_eq!(inc.by_class, [1, 1, 1]);
        assert_eq!(inc.proofs(), 3);
    }

    /// Part 7 (#69): the H-par′ (i) shape predicate. A first divergence passes
    /// only when a certificate fired AND `mag`'s own margin there is float noise;
    /// a leave-side divergence can never pass (variant-A leaves never consult the
    /// evaluator, so no certificate exists to fire).
    #[cfg(feature = "magnitude-fast")]
    #[test]
    fn p7_shape_predicate() {
        let shape = |proof: Option<ZeroDiversityProof>, margin: f64| {
            proof.is_some() && margin.abs() <= P7_SHAPE_NOISE
        };
        let merge = Some(ZeroDiversityProof::SkeletalMerge { member: 0 });

        assert!(
            shape(merge, 2.22e-16),
            "certified + noise margin ⇒ admissible"
        );
        assert!(
            shape(merge, 0.0),
            "an exact-zero margin is inside the noise band"
        );
        assert!(
            !shape(None, 2.22e-16),
            "no certificate ⇒ FALSIFIED (parity)"
        );
        assert!(!shape(merge, 1e-9), "a genuine margin ⇒ FALSIFIED (parity)");
        assert!(!shape(None, 0.5), "neither conjunct");
        // The leave case as the walker builds it.
        let leave = P7FirstDiv {
            seed: P7_SEED_START,
            task: 0,
            leave: true,
            proof: None,
            mag_margin: 0.0,
            shape_ok: false,
        };
        assert!(!leave.shape_ok && leave.proof.is_none());
    }

    /// Part 7 (#69): the paired walk's first-divergence bookkeeping, driven on
    /// synthetic act streams so the control flow is checked without depending on
    /// what the real arms happen to do.
    ///
    /// The invariants: at most ONE first divergence per task; every later
    /// divergence in that task counts as cascade; and a task whose acts agree
    /// throughout contributes nothing.
    #[cfg(feature = "magnitude-fast")]
    #[test]
    fn p7_paired_first_divergence_bookkeeping() {
        /// The walker's per-task rule, extracted verbatim.
        fn walk(pairs: &[(bool, bool)]) -> (usize, usize) {
            let (mut firsts, mut cascade, mut diverged) = (0usize, 0usize, false);
            for &(a, b) in pairs {
                if a != b {
                    if diverged {
                        cascade += 1;
                    } else {
                        diverged = true;
                        firsts += 1;
                    }
                }
            }
            (firsts, cascade)
        }

        assert_eq!(
            walk(&[(true, true), (false, false)]),
            (0, 0),
            "no divergence"
        );
        assert_eq!(walk(&[(true, false)]), (1, 0), "single divergence");
        assert_eq!(
            walk(&[(true, true), (true, false), (false, true), (true, false)]),
            (1, 2),
            "one first + two cascaded"
        );
        assert_eq!(walk(&[]), (0, 0), "empty task");
    }

    /// Part 7 (#69), 2-seed smoke: the paired walk and the instrumentation pass
    /// execute end-to-end on the registered seed range and produce coherent
    /// tallies. Does NOT run the 30-seed battery.
    #[cfg(feature = "magnitude-fast")]
    #[test]
    fn p7_two_seed_smoke() {
        let mut paired = P7Paired::default();
        for seed in P7_SEED_START..P7_SEED_START + 2 {
            p7_paired_seed(seed, &mut paired);
        }
        assert!(
            paired.compared > 0,
            "the paired walk must compare decisions"
        );
        assert!(
            paired.firsts.len() <= paired.compared,
            "at most one first divergence per task"
        );
        for d in &paired.firsts {
            assert!(d.mag_margin.is_finite());
            assert!(
                d.shape_ok == (d.proof.is_some() && d.mag_margin.abs() <= P7_SHAPE_NOISE),
                "shape_ok must be exactly the registered conjunction"
            );
        }

        // Both arms run the registered battery machinery on the v2 regime.
        let (mag, mag_lat) = stateless_battery_mode(
            || Box::new(MagnitudePolicy::default()) as Box<dyn CoalitionDecisionPolicy>,
            Regime::V2,
            P7_SEED_START,
            P7_SEED_START + 2,
        );
        let (eq3, eq3_lat) = stateless_battery_mode(
            || {
                Box::new(MagnitudePolicy::default().with_eq3_levers(true))
                    as Box<dyn CoalitionDecisionPolicy>
            },
            Regime::V2,
            P7_SEED_START,
            P7_SEED_START + 2,
        );
        assert_eq!(mag.len(), 2);
        assert_eq!(eq3.len(), 2);
        assert!(
            !mag_lat.is_empty() && !eq3_lat.is_empty(),
            "latencies recorded"
        );
        for r in mag.iter().chain(eq3.iter()) {
            assert!(r.primary.is_finite() && (0.0..=1.0).contains(&r.primary));
        }
    }

    /// Part 7 (#69): the library's `probe_join` instrumentation agrees with the
    /// arm it describes — a certified candidate is one the EQ3 arm declines at an
    /// exact-zero margin, and probing never changes what either arm decides.
    #[cfg(feature = "magnitude-fast")]
    #[test]
    fn p7_probe_agrees_with_the_arm() {
        let mag = MagnitudePolicy::default();
        let eq3 = MagnitudePolicy::default().with_eq3_levers(true);
        let (agents, tasks, _rho, _perf) = generate_instance_b_regime(P7_SEED_START, Regime::V2);
        let task = &tasks[0];
        let ctx = DecisionContext {
            required_capabilities: task.required,
        };

        let mut members: Vec<usize> = vec![task.order[0]];
        let mut certified = 0usize;
        for &idx in &task.order[1..] {
            let candidate: &dyn AgentCapabilities = &agents[idx];
            let view = coalition_view(&agents, &members);
            let probe = eq3.probe_join(candidate, &view, &ctx);
            let d_eq3 = eq3.should_join(candidate, &view, &ctx);
            let d_mag = mag.should_join(candidate, &view, &ctx);
            if let Some(p) = probe {
                if p.zero_proof.is_some() {
                    certified += 1;
                    assert!(
                        !d_eq3.act && d_eq3.score == 0.0,
                        "a certified candidate is an exact-zero decline on the EQ3 arm"
                    );
                }
                assert!(p.base.is_finite() && p.with.is_finite());
            }
            // Probing is state-free: the frozen arm still answers as it would.
            assert_eq!(
                d_mag.act,
                mag.should_join(candidate, &view, &ctx).act,
                "probing must not perturb the frozen arm"
            );
            if d_mag.act {
                members.push(idx);
            }
        }
        assert!(
            certified > 0,
            "the registered stream must exercise the certificate at least once"
        );
    }
}
