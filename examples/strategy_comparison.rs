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

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use catgraph_magnitude::{CatgraphError, ChannelCouplings, CoalitionEvaluator, role_grid};
// Part 9 (koalisi #76) EQ5a: the colored-string-diagram surface. `catgraph_applied`
// is a hard dependency; `catgraph_syntax` rides in with the `process` feature, which
// the example now requires.
use catgraph_applied::prop::colored::ColoredExpr;
use catgraph_applied::prop::presentation::content::{
    ContentKey, canonical_key, content_eq, content_of, content_of_colored,
};
use catgraph_applied::prop::presentation::rewrite::RewriteRule;
use catgraph_applied::prop::{Free, PropExpr};
use catgraph_syntax::frobenius::FrobeniusOr;
use koalisi::process::{
    Demand, LabelledRule, Role, Schema, StaffingTable, Step, Workflow, WorkflowGen, chain,
    content_matches, demand, fusion_pairs, optimize_workflow, rule_labels, rule_theory,
    spider_expr, staffing_price, step_expr, uniform_cost, verify_optimization, workflow_cost,
};
use koalisi::algorithms::{
    AgentCapabilities, CoalitionStructure, FeedbackCalculator, FeedbackStore, PopulationConfig,
    SynergisticCalculator, ValueCalculator, search,
};
use koalisi::decision::{
    AifDecisionPolicy, AifMmDecisionPolicy, CoalitionDecisionPolicy, CouplingModel, Decision,
    DecisionContext, MagnitudePolicy, PersistentAifArm, PersistentAifConfig, RoleId,
    RoleModulation, ThresholdPolicy, TrialBoundary,
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
    println!();
    println!("{}", "=".repeat(72));
    println!();
    part8_eq4_typed_roles();
    println!();
    println!("{}", "=".repeat(72));
    println!();
    part9_eq5a_process_structured();
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

    // Part 7 (koalisi #69) reads Part 2's own `mag` latency median back, so its
    // before/after context row cites THIS binary's frozen-battery number rather
    // than a remembered one. Capture only — the value is the same
    // `median_iqr(mag_lat).0` the report prints below, and no printed line of
    // Part 2 changes.
    PART2_MAG_LATENCY_US.set(median_iqr(mag_lat.clone()).0).ok();

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
///
/// **Absolute, and registered for THIS run** — deliberately not scale-relative.
/// The bound is comfortable at the magnitudes this battery reaches, but it is
/// not a universal constant: one ulp at a magnitude of ~16 is already 1.78e-15,
/// so on a larger coalition a genuinely certified flip could carry a margin
/// above the bound and be scored `FALSIFIED (parity)`. That error direction is
/// conservative — the bound can only reject a certified divergence, never admit
/// an uncertified one — so the registered value stands as-is here, and the run
/// prints its observed headroom. Any future reuse should re-register a
/// scale-relative form.
#[cfg(feature = "magnitude-fast")]
const P7_SHAPE_NOISE: f64 = 1e-15;
/// How many shape-PASSing first divergences the table prints before eliding.
/// Rows that FAIL the shape are never elided — they are the gating evidence.
#[cfg(feature = "magnitude-fast")]
const P7_FIRSTDIV_DISPLAY_CAP: usize = 40;
/// The K6 reference latencies (`docs/ab-report-K4-catgraph-evaluator.md`) — v1
/// regime, so every comparison against them is labelled cross-regime.
#[cfg(feature = "magnitude-fast")]
const P7_K6_MAG_US: f64 = 3.552;
#[cfg(feature = "magnitude-fast")]
const P7_K6_AIF_US: f64 = 1.435;
/// Report date for the Part 7 battery, stamped per committed run.
const P7_REPORT_DATE: &str = "2026-08-02";

/// Part 2's measured `mag` latency median (µs) from THIS binary's run, captured
/// so Part 7's before/after context row can cite it next to the pre-change
/// session baselines instead of asking the reader to scroll. Write-once; Part 2
/// runs before Part 7 in [`main`].
static PART2_MAG_LATENCY_US: std::sync::OnceLock<f64> = std::sync::OnceLock::new();

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
/// Classes are disjoint and exhaustive: `non_finite` (NaN or ±∞ — never reaches
/// the index arithmetic), `exact_zero` (no decade exists), `underflow`
/// (`0 < x < 1e-16`, below the registered histogram floor — its OWN row, not
/// folded into the first decade), then `decades[i]` = `[1e(i-16), 1e(i-15))`
/// for `i` in `0..16`, and `decades[16]` = the `>= 1e0` overflow.
#[cfg(feature = "magnitude-fast")]
#[derive(Default)]
struct P7Increments {
    /// Join decisions that reached the incremental branch.
    probed: usize,
    /// NaN / ±∞ increments — impossible on a healthy stream, counted rather
    /// than silently bucketed so a numeric regression is visible in the report.
    non_finite: usize,
    exact_zero: usize,
    /// `0 < |with − base| < 1e-16` — below the registered floor.
    underflow: usize,
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
        if x.is_finite() {
            if x == 0.0 {
                self.exact_zero += 1;
            } else {
                // `log10().floor()` is only meaningful for a positive finite
                // `x`; the guard above is what makes the index arithmetic safe.
                let e = x.log10().floor();
                let idx = e as i64 + 16;
                if idx < 0 {
                    self.underflow += 1;
                } else {
                    self.decades[idx.min(16) as usize] += 1;
                }
            }
        } else {
            self.non_finite += 1;
            tracing::warn!(
                base = probe.base,
                with = probe.with,
                "Part 7 increment histogram: non-finite |with − base|"
            );
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
const P7_BUCKETS: [&str; 7] = [
    "join/empty",
    "join/excluded",
    "join/clear",
    "join/band",
    "join/proof",
    "join/probe-err",
    "leave/fresh",
];

/// Index of the `leave/fresh` bucket in [`P7_BUCKETS`].
#[cfg(feature = "magnitude-fast")]
const P7_BUCKET_LEAVE: usize = 6;
/// Index of the `join/probe-err` bucket: the candidate DID reach the
/// incremental branch (non-empty base, candidate not excluded) but the probe
/// returned `None`, i.e. the evaluator build or the incremental query errored.
/// Its own class — folding it into `join/clear` would attribute an error path's
/// latency to the cheapest decisive one.
#[cfg(feature = "magnitude-fast")]
const P7_BUCKET_PROBE_ERR: usize = 5;

/// One untimed-classification + timed-decision instrumentation pass over the
/// battery for one arm. NOT the pooled latency measurement (that is a clean
/// `stateless_battery_mode` run with no probing anywhere near it) — this pass
/// probes before each decision, so its absolute numbers carry probe-induced
/// cache disturbance and are only read as a decomposition.
///
/// Returns per-bucket latency samples, the increment tally, and the
/// `FactorizationPath` tally of the `f64` fresh evaluations the EQ3 arm
/// performs: `[Cholesky, LBLT, Gauss–Jordan, of-which-errored]`. The route is
/// decided before any solve, so an errored evaluation still contributes its
/// path — dropping those would undercount Gauss–Jordan, the route an exact
/// singularity surfaces on.
#[cfg(feature = "magnitude-fast")]
fn p7_instrumentation_pass(eq3_arm: bool) -> (Vec<Vec<f64>>, P7Increments, [usize; 4]) {
    let policy = if eq3_arm {
        MagnitudePolicy::default().with_eq3_levers(true)
    } else {
        MagnitudePolicy::default()
    };
    let mut buckets: Vec<Vec<f64>> = vec![Vec::new(); P7_BUCKETS.len()];
    let mut inc = P7Increments::default();
    let mut paths = [0usize; 4];

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
                        if let Some((path, mag)) =
                            probe_fresh_factorization(&with_view, task.required)
                        {
                            p7_count_path(&mut paths, path, &mag);
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
                        Some(_) => 2,
                        // Reached the incremental branch but the probe could not
                        // answer — an upstream error, not a decisive margin.
                        None => P7_BUCKET_PROBE_ERR,
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
                        if let Some((path, mag)) = probe_fresh_factorization(v, task.required) {
                            p7_count_path(&mut paths, path, &mag);
                        }
                    }
                }

                let t0 = Instant::now();
                let d = policy.should_leave(agent, &view, &ctx);
                buckets[P7_BUCKET_LEAVE].push(seconds_to_us(t0.elapsed()));
                if d.act {
                    members.remove(pos);
                }
            }
        }
    }

    (buckets, inc, paths)
}

/// Tally one probed factorization: its route, plus whether the magnitude the
/// same handle produced was an `Err` (slot 3, a subset of the route counts).
#[cfg(feature = "magnitude-fast")]
fn p7_count_path(
    paths: &mut [usize; 4],
    path: FactorizationPath,
    mag: &Result<f64, CatgraphError>,
) {
    if mag.is_err() {
        paths[3] += 1;
    }
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
    println!(
        "_Instance convention: ONE policy instance per arm, reused across all {} seeds (`stateless_battery_mode`) — the v1 Part 2 code shape, kept because comparability requires every arm to face the same cache-warmth history. The prereg's \"(per-seed fresh arms)\" parenthetical describes the LEARNING arms' factory pattern (#44/#53), which does not apply to these three stateless policies; a per-seed rebuild would reset the magnitude evaluator cache 30 times and measure a different thing._",
        mag.len()
    );
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
        // Display cap applies to PASSing rows only: every !shape_ok row is a
        // gating failure and is printed unconditionally.
        println!("| seed | task | kind | certificate | mag margin | shape |");
        println!("|-----:|-----:|------|-------------|-----------:|-------|");
        let mut shown_ok = 0usize;
        let mut elided = 0usize;
        for d in &paired.firsts {
            if d.shape_ok {
                if shown_ok >= P7_FIRSTDIV_DISPLAY_CAP {
                    elided += 1;
                    continue;
                }
                shown_ok += 1;
            }
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
        if elided > 0 {
            println!("| … | | | | | _{elided} more PASS rows elided_ |");
        }
    }
    println!();

    // Shape-bound headroom (sem-F2): how close the certified margins ran to the
    // registered absolute bound.
    let max_certified = paired
        .firsts
        .iter()
        .filter(|d| d.shape_ok)
        .map(|d| d.mag_margin.abs())
        .fold(0.0f64, f64::max);
    println!(
        "- **Shape-bound headroom:** the largest certified margin observed is {max_certified:.3e}, i.e. {:.0}% of the registered bound {P7_SHAPE_NOISE:.0e}. The bound is ABSOLUTE and fixed for this run; one ulp at a magnitude of ~16 is already 1.78e-15, so a legitimate certified flip on a larger coalition could exceed it and would be scored `FALSIFIED (parity)`. That direction is conservative (it can only reject, never wave through), so this run stands as scored; a scale-relative bound is a matter for a future registration, not an edit here.",
        100.0 * max_certified / P7_SHAPE_NOISE
    );
    println!(
        "- Cascaded divergent decisions after a task's first (exempt by A1.2, context only): **{}**.",
        paired.cascade
    );
    // Cascade residual (sem-F4): what the exemption actually leaves unchecked.
    let total_tasks = (P7_SEED_END - P7_SEED_START) as usize * TASKS;
    let divergent_tasks = paired.firsts.len();
    println!(
        "- **Cascade residual:** {}/{} tasks ({:.1}%) diverge nowhere and are therefore fully verified decision-for-decision by the walk. The A1.2 exemption leaves exactly one stream unverified: the post-divergence tail of the {divergent_tasks} divergent tasks, where the two arms are no longer scoring the same coalition. Those tails are counted above, never shape-checked. (The library's L3-isolation corpus test covers uncertified agreement decision-by-decision, but on the v1 regime — it is not a substitute for this v2 measurement.)",
        total_tasks - divergent_tasks,
        total_tasks,
        100.0 * (total_tasks - divergent_tasks) as f64 / total_tasks as f64
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
        "- **Gap-shrink vs the K6 reference** (mag {P7_K6_MAG_US:.3} µs vs aif {P7_K6_AIF_US:.3} µs = {:.2}×) — **cross-regime**: K6 is the v1 draw and this battery is v2, so the ratio is the *more* comparable quantity than the absolute µs — not an invariant one, since the ratio itself moves with coalition size and therefore with the regime. This run: {:.2}× (`mag`) and {:.2}× (`mag-eq3`).",
        P7_K6_MAG_US / P7_K6_AIF_US,
        mag_med / scalar_med,
        eq3_med / scalar_med
    );
    println!(
        "- **Quality medians** (v2 regime; #61 Part 5a context rows were mag 0.1286 / scalar 0.1332): `scalar` {scalar_q:.4} · `mag` {mag_q:.4} · `mag-eq3` {eq3_q:.4}."
    );
    println!(
        "- **Frozen-battery before/after** (all v1 regime, all `mag`, latency-only by construction — every Part 2 quality column is byte-identical, the X-A/X-B gate): this binary's Part 2 line **{}**, this session's pre-change baselines 3.566 / 3.793 µs, the K6 report-of-record {P7_K6_MAG_US:.3} µs. L1 (scratch adoption) is the only lever active on that battery.",
        PART2_MAG_LATENCY_US.get().map_or_else(
            || "n/a (Part 2 did not run)".to_owned(),
            |v| format!("{v:.3} µs")
        )
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
    println!("| non-finite (NaN / ±∞) | {} |", mag_inc.non_finite);
    println!("| exact 0 | {} |", mag_inc.exact_zero);
    println!("| < 1e-16 (underflow) | {} |", mag_inc.underflow);
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
        "- The three rows above the decades are disjoint classes, not decade members: non-finite increments never reach the index arithmetic ({} seen — any nonzero count is a numeric regression, also logged via `tracing::warn`), and sub-1e-16 increments get their OWN row rather than being folded into `[1e-16, 1e-15)`.",
        mag_inc.non_finite
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
        "- **Former knife-edge recomputes retired:** the quantity that matters is measured on the **frozen `mag` arm's** stream — those are the recomputes that actually used to be paid. {} of its {} band decisions carry a certificate and are skipped by `mag-eq3` (**{:.1}%**).",
        mag_inc.knife_certified,
        mag_inc.knife,
        100.0 * mag_inc.knife_certified as f64 / mag_inc.knife.max(1) as f64
    );
    println!(
        "- For completeness, the same ratio on `mag-eq3`'s own stream (whose membership has already drifted under L2, so it is a different decision population): {} of {} ({:.1}%).",
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
        "- **DEVIATION from the registered bucket list (ledgered):** the K6 table splits every join bucket by `rebuild` vs `hit` (evaluator reconstructed vs cached). That split is only visible from inside the policy's cache, and the only cheap way to expose it is a counter on the decision path — which would instrument the very code the H-lat measurement times. Protecting the registered measurement won: the rebuild/hit split is NOT reproduced here, while the `empty`/`excluded`/`clear`/`band`/`proof`/`probe-err`/`leave` classes are, via the read-only `probe_join` surface. This deviation stands on that rationale alone — the prereg's \"where cheaply available\" latitude attaches to the `FactorizationPath` row, not to this table."
    );
    println!(
        "- This pass probes before each timed decision, so its absolute µs carry probe-induced cache disturbance. Read the SHAPE (which bucket costs what), not the level; the pooled table at the top is the clean measurement."
    );
    println!();

    let path_total: usize = eq3_paths[..3].iter().sum();
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
        "- **Sites counted** (not a totality claim): both magnitudes of every variant-A leave decision, plus the `with` side of every unprovable-knife-edge join. NOT counted: empty-coalition bootstrap joins, whose fresh evaluation is a trivial 1×1 ζ, and the excluded-candidate branch, which answers from `base_value` without a fresh evaluation at all. One factorization per count — the handle that reports the path is the handle that answers the magnitude (`t = 1` makes the scaling a no-op), so no extra factorization is paid to report this."
    );
    println!(
        "- **{}** of those {path_total} factorizations produced an `Err` magnitude (a singular ζ). Their route is still counted above: the route is chosen before any solve, so dropping errored evaluations would silently undercount Gauss–Jordan — the route an exact singularity surfaces on.",
        eq3_paths[3]
    );
    println!(
        "- **Read the H-lat result against this split.** The `f64` handle only takes Cholesky/LBLT when ζ is *exactly* symmetric; the substitutability coupling is asymmetric whenever two members have different relevant widths, so most of this traffic falls back to Gauss–Jordan — which re-enters the rig-generic computation AFTER paying the dense-matrix build and the symmetry scan. On the fallback share, L3 is net overhead rather than acceleration, and `mag-eq3`'s latency movement comes chiefly from L2."
    );
    println!();
}

// ===========================================================================
// Part 8 — EQ4 typed-roles battery (koalisi #72).
//
// Governed by `docs/prereg-K4-eq4-typed-roles.md`, registered BEFORE this code
// (owner design-lock D1–D9 on #72). The world is the **v2t** regime: the frozen
// `draw_prefix_v2` prefix with per-worker roles and per-required-bit role tags
// APPENDED off the same SplitMix64 stream (the #46/#48 shared-prefix
// discipline), plus a feasibility rejection re-draw. Ground truth is
// role-matched coverage; the confirmatory arm is the library's ρ-modulated
// typed magnitude (`MagnitudePolicy::with_role_modulation`).
//
// Everything here is ADDITIVE — Parts 1–7 above are the byte-identity gate
// (§6 X-battery), so no frozen draw, runner, or print statement is touched.
// ===========================================================================

/// Part 8 seed range (prereg §1, owner D4) — fresh seeds; 90..120 and 150..180
/// stay reserved.
const P8_SEED_START: u64 = 240;
const P8_SEED_END: u64 = 270;
/// `R` — the role count of the registered v2t world (prereg §2).
const P8_ROLES: usize = 3;
/// The identity world's role count: `next() % 1 == 0`, so every worker and every
/// tag lands in role 0 and the typed metric reduces to the untyped one
/// (prereg §2, "Identity world (for §6 X-identity)").
const P8_IDENTITY_ROLES: usize = 1;
/// H-T conjunct 1: median `PRIMARY(mag-typed)` must be at least this multiple of
/// median `PRIMARY(mag)`.
const P8_HT_FACTOR: f64 = 1.25;
/// H-T conjunct 2: `mag-typed` must be strictly superior on at least this many of
/// the 30 seeds (the 60 % consistency bar inherited from K4-v2/v3).
const P8_HT_SUPERIOR_MIN: usize = 18;
/// Rejection-re-draw budget per task (prereg §2). A task still infeasible after
/// this many attempts is a RUN-INVALID condition — expected never at these draws.
const P8_REDRAW_CAP: usize = 1000;
/// E-deg cells: the off-diagonal `ρ` entries the oracle's exact `0` is lifted to.
const P8_RHO_OFF_GRID: [f64; 2] = [0.25, 0.5];
/// E-ρq: the off-diagonal of the planted symmetric world table (diagonal 1.0).
const P8_RHOQ_OFF: f64 = 0.25;
/// E-T3: the channel count `C = R` (one channel per role).
const P8_CHANNELS: usize = P8_ROLES;
/// S-fib: the relative tolerance grid-vs-certificate agreement is checked at.
/// **Adopted from upstream's own documented figure**, not chosen here: catgraph
/// `v0.7.0`'s `coalition_typed` tests compare with
/// `rel_close(a, b) = |a − b| ≤ 1e-9 · max(|a|, |b|, 1)` and `RoleGrid::proof`'s
/// doctest asserts `≤ 1e-9 · max(|expected|, 1)`. Upstream also documents that
/// this tightness is only safe away from near-1 non-merged couplings (adversarial
/// tables at `1 − O(1e-12)` deviate up to ~1e-2), which is why the shapes below
/// keep every off-diagonal well inside `(0, 1)`.
const P8_FIB_REL_TOL: f64 = 1e-9;
/// Report date for the Part 8 battery, stamped per committed run.
const P8_REPORT_DATE: &str = "2026-08-03";

/// One v2t task: the v2 `required` mask and arrival order, plus the role tag of
/// every required bit. `tags[b]` is meaningful exactly for `b ∈ required`; the
/// other entries are never read (they stay at the `0` the array is built with).
struct TypedTask {
    required: u32,
    tags: [RoleId; UNIVERSE],
    order: Vec<usize>,
}

/// One seeded v2t instance: the v2 pool, each worker's role (indexed by
/// `Worker.id`, which is the pool index), the tagged task stream, and how many
/// rejection re-draws the feasibility pass needed.
struct TypedInstance {
    agents: Vec<Worker>,
    roles: Vec<RoleId>,
    tasks: Vec<TypedTask>,
    redraws: usize,
    /// The most attempts any SINGLE task of this instance needed — the distance
    /// to the [`P8_REDRAW_CAP`] RUN-INVALID trigger. The per-seed `redraws` total
    /// does not bound it usefully (it is a sum over 20 tasks), and the cap is a
    /// gate, so the headroom is measured and printed rather than assumed.
    max_attempts: usize,
}

/// Draw the role tag of every required bit in ASCENDING bit order (prereg §2).
/// Non-required bits consume no draw.
fn draw_tags(rng: &mut SplitMix64, required: u32, n_roles: u64) -> [RoleId; UNIVERSE] {
    let mut tags = [0usize; UNIVERSE];
    for (b, tag) in tags.iter_mut().enumerate() {
        if required & (1u32 << b) != 0 {
            *tag = (rng.next_u64() % n_roles) as RoleId;
        }
    }
    tags
}

/// Role-matched feasibility of one task against the pool (prereg §2): for every
/// required bit `b` tagged `r`, some pool worker of role `r` must hold `b`.
///
/// Strictly stronger than the untyped pool coverage the #63 draw guarantees —
/// the same worker no longer covers a bit for every tag, only for its own role.
fn p8_task_feasible(
    agents: &[Worker],
    roles: &[RoleId],
    required: u32,
    tags: &[RoleId; UNIVERSE],
) -> bool {
    (0..UNIVERSE)
        .filter(|&b| required & (1u32 << b) != 0)
        .all(|b| {
            agents.iter().any(|a| {
                roles.get(a.id).is_some_and(|&r| r == tags[b]) && a.caps & (1u32 << b) != 0
            })
        })
}

/// The v2t instance prefix (prereg §2): [`draw_prefix_v2`] verbatim, then the
/// role draws APPENDED off the SAME stream — so the untyped prefix of the stream
/// is bit-identical to a pure-v2 draw of the same seed (the #46/#48 discipline;
/// pinned by `v2t_prefix_matches_v2`).
///
/// Post-prefix draw order (fixed, arm-independent):
/// 1. one `next() % n_roles` per worker, in worker order;
/// 2. per task, one `next() % n_roles` per required bit in ascending bit order,
///    followed — only if the task is role-infeasible — by a rejection re-draw of
///    `(|required|, required, tags)` off the same stream.
///
/// # Panics
///
/// Panics with a RUN-INVALID message if a task is still infeasible after
/// [`P8_REDRAW_CAP`] attempts (expected never at these draws — the run is
/// invalid rather than silently biased if it ever fires).
fn draw_prefix_v2t(
    rng: &mut SplitMix64,
    n_roles: u64,
) -> (Vec<Worker>, Vec<RoleId>, Vec<TypedTask>, usize, usize) {
    let (agents, base_tasks) = draw_prefix_v2(rng);
    let roles: Vec<RoleId> = (0..agents.len())
        .map(|_| (rng.next_u64() % n_roles) as RoleId)
        .collect();

    let mut redraws = 0usize;
    let mut max_attempts = 0usize;
    let mut tasks: Vec<TypedTask> = Vec::with_capacity(base_tasks.len());
    for base in base_tasks {
        let mut required = base.required;
        let mut tags = draw_tags(rng, required, n_roles);
        let mut attempts = 1usize;
        while !p8_task_feasible(&agents, &roles, required, &tags) {
            assert!(
                attempts < P8_REDRAW_CAP,
                "RUN-INVALID: a v2t task stayed role-infeasible after {P8_REDRAW_CAP} rejection re-draws (prereg §2)"
            );
            // The re-draw repeats the v2 task draw for `required` (`r ∈ 2..=8`)
            // and re-tags it; the arrival order is drawn once in the prefix and
            // is NOT re-drawn (it is role-independent).
            let r = 2 + rng.next_u64() % 7;
            required = draw_distinct_bits(rng, r);
            tags = draw_tags(rng, required, n_roles);
            attempts += 1;
            redraws += 1;
        }
        max_attempts = max_attempts.max(attempts);
        tasks.push(TypedTask {
            required,
            tags,
            order: base.order,
        });
    }

    (agents, roles, tasks, redraws, max_attempts)
}

/// One seeded v2t instance at `n_roles` roles.
fn draw_typed_instance(seed: u64, n_roles: usize) -> TypedInstance {
    let mut rng = SplitMix64::new(seed);
    let (agents, roles, tasks, redraws, max_attempts) = draw_prefix_v2t(&mut rng, n_roles as u64);
    TypedInstance {
        agents,
        roles,
        tasks,
        redraws,
        max_attempts,
    }
}

/// The registered seed range's instances, drawn once and shared by every arm
/// (identical instances across arms is the standing battery invariant).
fn p8_instances(n_roles: usize) -> Vec<TypedInstance> {
    (P8_SEED_START..P8_SEED_END)
        .map(|s| draw_typed_instance(s, n_roles))
        .collect()
}

/// Role-matched covered required bits (prereg §2): bit `b` tagged `r` counts iff
/// some member of role `r` holds it.
fn p8_typed_covered(inst: &TypedInstance, members: &[usize], task: &TypedTask) -> u32 {
    (0..UNIVERSE)
        .filter(|&b| task.required & (1u32 << b) != 0)
        .filter(|&b| {
            members.iter().any(|&i| {
                inst.roles.get(i).is_some_and(|&r| r == task.tags[b])
                    && inst.agents[i].caps & (1u32 << b) != 0
            })
        })
        .count() as u32
}

/// Untyped covered required bits — the frozen `run_instance` formula, used by
/// the E-ρq world (whose coverage is deliberately untyped) and by the
/// identity-world reduction check.
fn p8_untyped_covered(inst: &TypedInstance, members: &[usize], required: u32) -> u32 {
    let union = members
        .iter()
        .fold(0u32, |acc, &i| acc | inst.agents[i].caps);
    (union & required).count_ones()
}

/// Fraction of a task's required bits that are held by workers of EXACTLY ONE
/// role in the pool — the tag-conditioning disclosure (review finding).
///
/// The feasibility re-draw conditions tags on the pool, not just on task size:
/// if only one role holds bit `b`, every feasible tag for `b` IS that role, so
/// the bit is decided before any arm runs. On such a bit role-matched coverage
/// coincides with untyped coverage, and the typed arm has no edge to gain —
/// which makes the direction CONSERVATIVE for H-T (the contest happens only on
/// the remaining bits).
fn p8_single_role_fraction(inst: &TypedInstance, required: u32) -> f64 {
    let bits: Vec<usize> = (0..UNIVERSE)
        .filter(|&b| required & (1u32 << b) != 0)
        .collect();
    if bits.is_empty() {
        return 0.0;
    }
    let single = bits
        .iter()
        .filter(|&&b| {
            let mut roles: Vec<RoleId> = inst
                .agents
                .iter()
                .filter(|a| a.caps & (1u32 << b) != 0)
                .filter_map(|a| inst.roles.get(a.id).copied())
                .collect();
            roles.sort_unstable();
            roles.dedup();
            roles.len() == 1
        })
        .count();
    single as f64 / bits.len() as f64
}

/// Mean pairwise `ρ_world(role_i, role_j)` over the final members (E-ρq, prereg
/// §5): the mean over ordered pairs `i ≠ j`, `1.0` for a singleton (no pair to
/// average) and `0.0` for an empty coalition (which scores `cov_eff = 0` anyway).
fn p8_mean_pairwise_rho(
    inst: &TypedInstance,
    members: &[usize],
    rho: &[[f64; P8_ROLES]; P8_ROLES],
) -> f64 {
    if members.is_empty() {
        return 0.0;
    }
    if members.len() == 1 {
        return 1.0;
    }
    let mut sum = 0.0;
    let mut pairs = 0usize;
    for (a, &i) in members.iter().enumerate() {
        for (b, &j) in members.iter().enumerate() {
            if a == b {
                continue;
            }
            sum += rho[inst.roles[i]][inst.roles[j]];
            pairs += 1;
        }
    }
    sum / pairs as f64
}

/// Which quality model a Part 8 run scores under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum P8Metric {
    /// The registered v2t metric: role-matched coverage (prereg §2).
    Typed,
    /// The E-ρq exploratory world (prereg §5): UNTYPED coverage, with task
    /// quality `cov_eff × mean pairwise ρ_world` over the final members.
    RhoQ,
}

/// How a Part 8 arm supplies its policy for a seed.
enum P8Arm<'a> {
    /// One policy for the whole seed (`mag`, `mag-typed`, `scalar`, `arm-E1`).
    Fixed(&'a dyn CoalitionDecisionPolicy),
    /// A policy rebuilt per task, because the value model reads that task's role
    /// tags (the example-side `E-ceil` and `E-T3` arms).
    PerTask(&'a dyn Fn(&TypedTask) -> Box<dyn CoalitionDecisionPolicy>),
}

/// The per-task hook a Part 8 run fires after every leave sweep:
/// `(required, per-bit role-matched success, whole-task success, final members)`
/// — the [`run_seed_b_regime`] hook shape, so the learning arm wires in unchanged.
type P8OutcomeHook<'a> = dyn FnMut(u32, &[bool], bool, &[usize]) + 'a;

/// Per-seed Part 8 result. `scores` carries the raw `Decision::score` BIT
/// PATTERNS in decision order — the X-identity cell-1 conjunct is a bit-identity
/// claim, and `f64` equality would not express it (it also makes the comparison
/// total in the presence of a `NaN` score, which the arms guard against but the
/// gate should not assume away).
struct P8Seed {
    primary: f64,
    churn: usize,
    acts: Vec<bool>,
    scores: Vec<u64>,
    success_rate: f64,
}

/// Run one arm over one v2t instance with the standing battery protocol
/// (bootstrap first arrival, one leave sweep per task in arrival order, every
/// removal counts as churn, `Instant` around every sync decision), scoring under
/// `metric`.
///
/// `on_task` fires once per task AFTER the leave sweep with
/// `(required, per_bit, success, members)` — the same hook shape
/// [`run_seed_b_regime`] uses, so the learning arm wires in unchanged. The
/// per-bit slice is the **role-matched** oracle signal (bit `b` is `true` iff it
/// is role-matched covered by the final coalition); it is `UNIVERSE` wide, and
/// non-required bits are `false` (`observe_outcome` ignores them).
fn p8_run_seed(
    arm: &P8Arm<'_>,
    inst: &TypedInstance,
    metric: P8Metric,
    lat: &mut Vec<f64>,
    on_task: &mut P8OutcomeHook<'_>,
) -> P8Seed {
    let rho_world = p8_rho_table(P8_RHOQ_OFF);

    let mut success_count = 0usize;
    let mut quality_sum = 0.0f64;
    let mut churn = 0usize;
    let mut acts = Vec::new();
    let mut scores = Vec::new();

    for task in &inst.tasks {
        let ctx = DecisionContext {
            required_capabilities: task.required,
        };
        let per_task;
        let policy: &dyn CoalitionDecisionPolicy = match arm {
            P8Arm::Fixed(p) => *p,
            P8Arm::PerTask(make) => {
                per_task = make(task);
                &*per_task
            }
        };

        let mut members: Vec<usize> = vec![task.order[0]];

        for &idx in &task.order[1..] {
            let candidate: &dyn AgentCapabilities = &inst.agents[idx];
            let coalition = coalition_view(&inst.agents, &members);
            let t0 = Instant::now();
            let d = policy.should_join(candidate, &coalition, &ctx);
            lat.push(seconds_to_us(t0.elapsed()));
            acts.push(d.act);
            scores.push(d.score.to_bits());
            if d.act {
                members.push(idx);
            }
        }

        for &idx in &task.order {
            let Some(pos) = members.iter().position(|&m| m == idx) else {
                continue;
            };
            let coalition = coalition_view(&inst.agents, &members);
            let agent: &dyn AgentCapabilities = &inst.agents[idx];
            let t0 = Instant::now();
            let d = policy.should_leave(agent, &coalition, &ctx);
            lat.push(seconds_to_us(t0.elapsed()));
            acts.push(d.act);
            scores.push(d.score.to_bits());
            if d.act {
                members.remove(pos);
                churn += 1;
            }
        }

        let req_bits = f64::from(task.required.count_ones());
        let covered = match metric {
            P8Metric::Typed => p8_typed_covered(inst, &members, task),
            P8Metric::RhoQ => p8_untyped_covered(inst, &members, task.required),
        };
        let completed = covered == task.required.count_ones();
        let cov_eff = if members.is_empty() {
            0.0
        } else {
            (f64::from(covered) / req_bits) / members.len() as f64
        };
        let quality = match metric {
            P8Metric::Typed => cov_eff,
            P8Metric::RhoQ => cov_eff * p8_mean_pairwise_rho(inst, &members, &rho_world),
        };
        if completed {
            success_count += 1;
        }
        quality_sum += quality;

        let mut per_bit = vec![false; UNIVERSE];
        for (b, slot) in per_bit.iter_mut().enumerate() {
            *slot = task.required & (1u32 << b) != 0
                && members.iter().any(|&i| {
                    inst.roles.get(i).is_some_and(|&r| r == task.tags[b])
                        && inst.agents[i].caps & (1u32 << b) != 0
                });
        }
        on_task(task.required, &per_bit, completed, &members);
    }

    let n_tasks = inst.tasks.len() as f64;
    let success_rate = success_count as f64 / n_tasks;
    P8Seed {
        primary: success_rate * (quality_sum / n_tasks),
        churn,
        acts,
        scores,
        success_rate,
    }
}

/// A square `R × R` table with a uniform diagonal and a uniform off-diagonal.
///
/// Every Part 8 table is of this shape: the oracle `ρ = δ` at `(1, 0)`, the
/// X-identity `ρ ≡ 1` at `(1, 1)`, the E-deg cells at `(1, 0.25)` / `(1, 0.5)`,
/// the E-ρq planted world table at `(1, P8_RHOQ_OFF)`, and the E-ρq-inv cell
/// (Amendment 2) at the FLIPPED `(P8_RHOQ_OFF, 1)`.
fn p8_rho_table_at(diag: f64, off: f64) -> [[f64; P8_ROLES]; P8_ROLES] {
    let mut t = [[off; P8_ROLES]; P8_ROLES];
    for (i, row) in t.iter_mut().enumerate() {
        row[i] = diag;
    }
    t
}

/// [`p8_rho_table_at`] with the `1.0` diagonal every registered table carries.
fn p8_rho_table(off: f64) -> [[f64; P8_ROLES]; P8_ROLES] {
    p8_rho_table_at(1.0, off)
}

/// [`p8_rho_table_at`] as a validated upstream [`RoleModulation`].
fn p8_rho_at(diag: f64, off: f64) -> RoleModulation {
    let rows: Vec<Vec<f64>> = p8_rho_table_at(diag, off)
        .iter()
        .map(|r| r.to_vec())
        .collect();
    RoleModulation::new(rows)
        .expect("invariant: a literal square table whose entries are all in [0, 1]")
}

/// [`p8_rho_at`] with the `1.0` diagonal.
fn p8_rho(off: f64) -> RoleModulation {
    p8_rho_at(1.0, off)
}

/// The `mag-typed` arm for one instance: the library typed policy over a role map
/// covering the WHOLE pool.
///
/// # Panics
///
/// Panics if the role map does not cover every pool worker. The library declines
/// (with a warning) for a participating agent that carries no role, which would
/// silently freeze membership rather than fail — so full coverage is asserted
/// here, where a gap is a harness bug and must stop the run.
fn p8_typed_policy(inst: &TypedInstance, rho: &RoleModulation) -> MagnitudePolicy {
    let roles: HashMap<usize, RoleId> = inst
        .agents
        .iter()
        .map(|a| {
            let role = *inst
                .roles
                .get(a.id)
                .expect("invariant: the v2t draw assigns one role per pool worker");
            (a.id, role)
        })
        .collect();
    assert_eq!(
        roles.len(),
        inst.agents.len(),
        "the typed role map must cover every pool worker (agent ids are the pool indices)"
    );
    MagnitudePolicy::default().with_role_modulation(roles, rho.clone())
}

/// Run one fixed-policy arm over the shared instances. A discarded warm-up on the
/// first instance runs first (warm caches / warm allocator, the standing
/// convention); it cannot perturb the seed-derived results.
fn p8_battery<F>(insts: &[TypedInstance], make: F, metric: P8Metric) -> (Vec<P8Seed>, Vec<f64>)
where
    F: Fn(&TypedInstance) -> Box<dyn CoalitionDecisionPolicy>,
{
    if let Some(first) = insts.first() {
        let p = make(first);
        let mut warm = Vec::new();
        let _ = p8_run_seed(
            &P8Arm::Fixed(&*p),
            first,
            metric,
            &mut warm,
            &mut |_, _, _, _| {},
        );
    }
    let mut lat = Vec::new();
    let results = insts
        .iter()
        .map(|inst| {
            let p = make(inst);
            p8_run_seed(
                &P8Arm::Fixed(&*p),
                inst,
                metric,
                &mut lat,
                &mut |_, _, _, _| {},
            )
        })
        .collect();
    (results, lat)
}

/// Run one per-task-policy arm (E-ceil, E-T3) over the shared instances.
fn p8_pertask_battery<F>(
    insts: &[TypedInstance],
    make: F,
    metric: P8Metric,
) -> (Vec<P8Seed>, Vec<f64>)
where
    F: Fn(&TypedInstance, &TypedTask) -> Box<dyn CoalitionDecisionPolicy>,
{
    if let Some(first) = insts.first() {
        let f = |task: &TypedTask| make(first, task);
        let mut warm = Vec::new();
        let _ = p8_run_seed(
            &P8Arm::PerTask(&f),
            first,
            metric,
            &mut warm,
            &mut |_, _, _, _| {},
        );
    }
    let mut lat = Vec::new();
    let results = insts
        .iter()
        .map(|inst| {
            let f = |task: &TypedTask| make(inst, task);
            p8_run_seed(
                &P8Arm::PerTask(&f),
                inst,
                metric,
                &mut lat,
                &mut |_, _, _, _| {},
            )
        })
        .collect();
    (results, lat)
}

/// The `arm-E1` context battery: a FRESH [`PersistentAifArm`] per seed (the
/// #44/#53 factory pattern) fed the role-matched per-bit outcome after every
/// task. Warm-up on the first instance discarded.
fn p8_e1_battery(insts: &[TypedInstance], config: PersistentAifConfig) -> (Vec<P8Seed>, Vec<f64>) {
    if let Some(first) = insts.first() {
        let arm = PersistentAifArm::new(P8_SEED_START, config).expect("persistent arm construction");
        let mut warm = Vec::new();
        let _ = p8_run_seed(
            &P8Arm::Fixed(&arm),
            first,
            P8Metric::Typed,
            &mut warm,
            &mut |req, bits, _success, _members| arm.observe_outcome(req, bits),
        );
    }
    let mut lat = Vec::new();
    let results = insts
        .iter()
        .enumerate()
        .map(|(i, inst)| {
            let seed = P8_SEED_START + i as u64;
            let arm = PersistentAifArm::new(seed, config).expect("persistent arm construction");
            p8_run_seed(
                &P8Arm::Fixed(&arm),
                inst,
                P8Metric::Typed,
                &mut lat,
                &mut |req, bits, _success, _members| arm.observe_outcome(req, bits),
            )
        })
        .collect();
    (results, lat)
}

fn p8_primaries(rs: &[P8Seed]) -> Vec<f64> {
    rs.iter().map(|r| r.primary).collect()
}
fn p8_churns(rs: &[P8Seed]) -> Vec<f64> {
    rs.iter().map(|r| r.churn as f64).collect()
}
/// Seeds on which `a` strictly beats `b` on PRIMARY.
fn p8_superior_count(a: &[P8Seed], b: &[P8Seed]) -> usize {
    (0..a.len().min(b.len()))
        .filter(|&i| a[i].primary > b[i].primary)
        .count()
}

/// One summary row of a Part 8 table: medians plus the paired contrast against
/// the `mag` control.
fn p8_row(label: &str, rs: &[P8Seed], base: &[P8Seed], lat: &[f64]) {
    let med = median(p8_primaries(rs));
    let base_med = median(p8_primaries(base));
    let ratio = if base_med > 0.0 {
        format!("{:.2}×", med / base_med)
    } else {
        "n/a".to_owned()
    };
    println!(
        "| `{label}` | {med:.4} | {ratio} | {}/{} | {:.2} | {:.3} |",
        p8_superior_count(rs, base),
        rs.len(),
        median(p8_churns(rs)),
        median(lat.to_vec())
    );
}

// ---------------------------------------------------------------------------
// E-ceil — the typed-relevance ceiling (registered exploratory, example-side).
//
// The registered `mag-typed` lever keeps relevance masks UNTYPED and lets roles
// in only through coupling modulation. This arm instead re-types the masks
// themselves — `rel_i = caps_i ∩ (required bits tagged role_i)` — and then runs
// the ORDINARY untyped substitutability couplings over them (no ρ). It is the
// arm that fully understands role-matching, so it measures the total convertible
// signal; the gap to `mag-typed` mechanism-scopes any H-T failure.
// ---------------------------------------------------------------------------

/// The mask of required bits carrying tag `role`.
fn p8_tagged_mask(required: u32, tags: &[RoleId; UNIVERSE], role: RoleId) -> u32 {
    (0..UNIVERSE)
        .filter(|&b| required & (1u32 << b) != 0 && tags[b] == role)
        .fold(0u32, |acc, b| acc | (1u32 << b))
}

/// E-ceil policy (exploratory; example-only, one instance per task).
struct TypedRelevanceMag {
    /// Role of each pool worker, indexed by `agent_id`.
    roles: Vec<RoleId>,
    /// This task's per-bit role tags.
    tags: [RoleId; UNIVERSE],
    join_margin: f64,
}

impl TypedRelevanceMag {
    /// Deduplicate by `agent_id` (first wins) and drop agents whose TYPED
    /// relevance is empty, returning the survivors' typed masks.
    ///
    /// The typed mask is a submask of `required`, so
    /// [`CouplingModel::coupling`]'s internal `& required` is a no-op on it and
    /// the coupling it computes is exactly `|rel_i ∩ rel_j| / |rel_i|` over the
    /// TYPED relevance — which is the whole point of this arm.
    ///
    /// Returns `None` (⇒ decline) if an agent id has no role, mirroring the
    /// library's typed decline rather than panicking.
    fn typed_masks(&self, agents: &[&dyn AgentCapabilities], required: u32) -> Option<Vec<u32>> {
        let mut seen = HashSet::new();
        let mut masks = Vec::with_capacity(agents.len());
        for a in agents {
            let id = a.agent_id();
            if !seen.insert(id) {
                continue;
            }
            let &role = self.roles.get(id)?;
            let m = a.capabilities() & p8_tagged_mask(required, &self.tags, role);
            if m != 0 {
                masks.push(m);
            }
        }
        Some(masks)
    }
}

impl CoalitionDecisionPolicy for TypedRelevanceMag {
    fn should_join(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        ctx: &DecisionContext,
    ) -> Decision {
        let required = ctx.required_capabilities;
        let mut with: Vec<&dyn AgentCapabilities> = coalition.to_vec();
        with.push(agent);
        let (Some(masks_without), Some(masks_with)) = (
            self.typed_masks(coalition, required),
            self.typed_masks(&with, required),
        ) else {
            return Decision {
                act: false,
                score: 0.0,
            };
        };
        join_decision(
            magnitude_at_t(&masks_with, required, 1.0),
            magnitude_at_t(&masks_without, required, 1.0),
            self.join_margin,
        )
    }

    fn should_leave(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        ctx: &DecisionContext,
    ) -> Decision {
        let required = ctx.required_capabilities;
        let agent_id = agent.agent_id();
        let without: Vec<&dyn AgentCapabilities> = coalition
            .iter()
            .filter(|a| a.agent_id() != agent_id)
            .copied()
            .collect();
        let (Some(masks_in), Some(masks_out)) = (
            self.typed_masks(coalition, required),
            self.typed_masks(&without, required),
        ) else {
            return Decision {
                act: false,
                score: 0.0,
            };
        };
        leave_decision(
            magnitude_at_t(&masks_in, required, 1.0),
            magnitude_at_t(&masks_out, required, 1.0),
        )
    }
}

// ---------------------------------------------------------------------------
// E-T3 — channel-valued couplings (registered exploratory, example-side).
//
// `C = R = 3` channels; channel `c` carries the role-`c`-restricted
// substitutability, collapsed with the registered uniform θ = (1/3, 1/3, 1/3)
// through the upstream declared homomorphism `ChannelCouplings::collapse`.
// ---------------------------------------------------------------------------

/// Decision-INERT tallies for the E-T3 caveat (review finding: the neutral-`1.0`
/// convention was asserted as the account without ever being measured).
///
/// Counted over every channel entry the leg builds — i.e. over EVALUATIONS
/// (`should_join` pays two, `should_leave` two), not over decisions. Nothing here
/// is read back into a value: the counters are written after the coupling has
/// already been computed.
#[derive(Default)]
struct P8ChannelCounters {
    /// Channel entries built, `(i, j, c)` triples.
    entries: AtomicUsize,
    /// Of those, entries whose denominator `rel_i ∩ tagged(c)` was EMPTY and
    /// therefore took the registered neutral `1.0`.
    neutral: AtomicUsize,
    /// Collapsed couplings that came out exactly `1.0` with EVERY channel
    /// neutral — "no evidence anywhere", the caveat's pure case.
    unit_all_neutral: AtomicUsize,
    /// Collapsed couplings that came out exactly `1.0` with at least one
    /// NON-neutral channel — the upstream `powf` trap (a strictly-sub-1 base can
    /// round up to exactly `1.0`), which manufactures a skeletal merge between
    /// agents no single channel perfectly couples.
    unit_rounded: AtomicUsize,
}

impl P8ChannelCounters {
    fn get(&self) -> (usize, usize, usize, usize) {
        (
            self.entries.load(Ordering::Relaxed),
            self.neutral.load(Ordering::Relaxed),
            self.unit_all_neutral.load(Ordering::Relaxed),
            self.unit_rounded.load(Ordering::Relaxed),
        )
    }
}

/// Channel-collapsed coalition magnitude at the pinned `t = 1`.
///
/// `masks` are the [`relevant_masks`] survivors (untyped relevance). For each
/// ordered pair `i ≠ j` and channel `c`,
/// `A_c(i → j) = |rel_i ∩ rel_j ∩ tagged(c)| / |rel_i ∩ tagged(c)|` with an
/// EMPTY denominator collapsing to the neutral `1.0` (the registered caveat: the
/// product collapse makes "no evidence" neutral-high, biasing cross-role coupling
/// upward — one reason this leg is exploratory).
///
/// `counters`, when supplied, tallies that caveat's incidence. It is written
/// only after each value is already fixed, so it cannot influence a decision.
///
/// # Errors
///
/// Propagates upstream validation (`ChannelCouplings::set` / `collapse`) and any
/// [`CatgraphError`] from the magnitude computation.
fn p8_channel_magnitude(
    masks: &[u32],
    required: u32,
    tags: &[RoleId; UNIVERSE],
    counters: Option<&P8ChannelCounters>,
) -> Result<f64, CatgraphError> {
    if masks.is_empty() {
        return Ok(0.0);
    }
    let agents: Vec<usize> = (0..masks.len()).collect();
    let tagged: Vec<u32> = (0..P8_CHANNELS)
        .map(|c| p8_tagged_mask(required, tags, c))
        .collect();

    let mut channels = ChannelCouplings::new(P8_CHANNELS)?;
    // Pairs whose every channel took the neutral `1.0`, so an exact-`1.0`
    // collapse there is the caveat rather than a `powf` rounding artifact.
    let mut all_neutral: HashSet<(usize, usize)> = HashSet::new();
    for (i, &from) in masks.iter().enumerate() {
        for (j, &to) in masks.iter().enumerate() {
            if i == j {
                continue;
            }
            let rel_i = from & required;
            let rel_j = to & required;
            let mut neutral_here = 0usize;
            let v: Vec<f64> = tagged
                .iter()
                .map(|&t| {
                    let denom = (rel_i & t).count_ones();
                    if denom == 0 {
                        neutral_here += 1;
                        1.0
                    } else {
                        f64::from((rel_i & rel_j & t).count_ones()) / f64::from(denom)
                    }
                })
                .collect();
            if let Some(c) = counters {
                c.entries.fetch_add(P8_CHANNELS, Ordering::Relaxed);
                c.neutral.fetch_add(neutral_here, Ordering::Relaxed);
            }
            if neutral_here == P8_CHANNELS {
                all_neutral.insert((i, j));
            }
            channels.set(i, j, v)?;
        }
    }

    let theta = [1.0 / P8_CHANNELS as f64; P8_CHANNELS];
    let couplings = channels.collapse(&theta)?;
    if let Some(c) = counters {
        for &(from, to, p) in &couplings {
            if p == 1.0 {
                if all_neutral.contains(&(from, to)) {
                    c.unit_all_neutral.fetch_add(1, Ordering::Relaxed);
                } else {
                    c.unit_rounded.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }
    catgraph_magnitude::coalition_magnitude_from_couplings(&agents, &couplings, &agents, 1.0)
}

/// E-T3 policy (exploratory; example-only, one instance per task).
struct ChannelMagnitudePolicy {
    tags: [RoleId; UNIVERSE],
    join_margin: f64,
    /// Shared with every other task's policy of the same leg, so the printed
    /// tallies cover the whole battery. `Arc` because
    /// [`CoalitionDecisionPolicy`] is `Send + Sync`.
    counters: Arc<P8ChannelCounters>,
}

impl CoalitionDecisionPolicy for ChannelMagnitudePolicy {
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
        join_decision(
            p8_channel_magnitude(&masks_with, required, &self.tags, Some(&self.counters)),
            p8_channel_magnitude(&masks_without, required, &self.tags, Some(&self.counters)),
            self.join_margin,
        )
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
        leave_decision(
            p8_channel_magnitude(&masks_in, required, &self.tags, Some(&self.counters)),
            p8_channel_magnitude(&masks_out, required, &self.tags, Some(&self.counters)),
        )
    }
}

// ---------------------------------------------------------------------------
// T1 — role-share instrumentation (non-gating).
// ---------------------------------------------------------------------------

/// The `role_shares` tally over every task's final `mag-typed` coalition.
#[derive(Default)]
struct P8RoleShareStats {
    /// Coalitions the evaluator was successfully built and decomposed on.
    samples: usize,
    /// Samples whose every skeletal class was single-role.
    exact: usize,
    /// Samples carrying at least one role-mixed class.
    mixed_samples: usize,
    /// Total role-mixed classes across all samples.
    mixed_classes: usize,
    /// Tasks whose final coalition had no relevance-surviving member (nothing to
    /// decompose) — counted, not silently dropped.
    empty: usize,
    /// Upstream errors on the instrumentation path (never a decision path).
    errors: usize,
    /// Per-role attributed share samples, `share(r)` for `r ∈ 0..R`.
    per_role: [Vec<f64>; P8_ROLES],
    /// Largest observed relative gap between `Σ_r share(r)` and `base_value()`
    /// on EXACT samples (upstream contracts these as equal up to float
    /// re-association, not bit-identical).
    max_rel_gap: f64,
}

/// Decompose one final coalition's magnitude across its roles (T1), off the
/// decision path: a fresh [`CoalitionEvaluator`] over the SAME ρ-modulated
/// couplings the typed arm evaluated, then `role_shares` with member-local roles.
fn p8_record_role_shares(
    inst: &TypedInstance,
    required: u32,
    members: &[usize],
    rho: &RoleModulation,
    stats: &mut P8RoleShareStats,
) {
    // Relevance filter + role lookup, positionally aligned exactly as the
    // library's typed path builds its side.
    let mut seen = HashSet::new();
    let mut masks: Vec<u32> = Vec::with_capacity(members.len());
    let mut roles: Vec<RoleId> = Vec::with_capacity(members.len());
    for &i in members {
        let caps = inst.agents[i].caps;
        if caps & required == 0 || !seen.insert(i) {
            continue;
        }
        let Some(&role) = inst.roles.get(i) else {
            stats.errors += 1;
            return;
        };
        masks.push(caps);
        roles.push(role);
    }
    if masks.is_empty() {
        stats.empty += 1;
        return;
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

    let Ok(modulated) = catgraph_magnitude::modulate(&couplings, &roles, rho) else {
        stats.errors += 1;
        return;
    };
    let Ok(ev) = CoalitionEvaluator::new(&agents, modulated.couplings(), &agents, 1.0) else {
        stats.errors += 1;
        return;
    };
    // `role_shares` indexes roles by MEMBER-LOCAL position; members were passed
    // as `0..k` in `masks` order, so `roles` is already that alignment.
    let Ok(shares) = ev.role_shares(&roles) else {
        stats.errors += 1;
        return;
    };

    stats.samples += 1;
    if shares.is_exact() {
        stats.exact += 1;
        let sum: f64 = (0..P8_ROLES).filter_map(|r| shares.share(r)).sum();
        let base = ev.base_value();
        stats.max_rel_gap = stats
            .max_rel_gap
            .max((sum - base).abs() / base.abs().max(1.0));
    } else {
        stats.mixed_samples += 1;
        stats.mixed_classes += shares.mixed_classes().len();
    }
    for (r, bucket) in stats.per_role.iter_mut().enumerate() {
        if let Some(v) = shares.share(r) {
            bucket.push(v);
        }
    }
}

/// Re-run the `mag-typed` arm and decompose every task's final coalition (T1).
/// Off the decision path: the instrumentation happens in the post-sweep hook.
fn p8_role_share_pass(insts: &[TypedInstance], rho: &RoleModulation) -> P8RoleShareStats {
    let mut stats = P8RoleShareStats::default();
    let mut lat = Vec::new();
    for inst in insts {
        let policy = p8_typed_policy(inst, rho);
        let _ = p8_run_seed(
            &P8Arm::Fixed(&policy),
            inst,
            P8Metric::Typed,
            &mut lat,
            &mut |required, _bits, _success, members| {
                p8_record_role_shares(inst, required, members, rho, &mut stats);
            },
        );
    }
    stats
}

// ---------------------------------------------------------------------------
// S-fib — the role-grid factorization sanity gate.
// ---------------------------------------------------------------------------

/// One deterministic `role_space × fiber` shape. Off-diagonals are hand-fixed
/// strictly inside `(0, 1)` and well away from `1` (upstream documents the
/// tight relative tolerance as safe only away from near-1 non-merged couplings);
/// diagonals are exactly `1.0`, which `role_grid` requires.
struct P8FibShape {
    label: &'static str,
    role: Vec<Vec<f64>>,
    fiber: Vec<Vec<f64>>,
}

fn p8_fib_shapes() -> Vec<P8FibShape> {
    vec![
        P8FibShape {
            label: "2 × 2 (symmetric role, asymmetric fiber)",
            role: vec![vec![1.0, 0.5], vec![0.5, 1.0]],
            fiber: vec![vec![1.0, 0.6], vec![0.3, 1.0]],
        },
        P8FibShape {
            label: "3 × 2 (asymmetric role)",
            role: vec![
                vec![1.0, 0.4, 0.2],
                vec![0.25, 1.0, 0.5],
                vec![0.1, 0.35, 1.0],
            ],
            fiber: vec![vec![1.0, 0.7], vec![0.45, 1.0]],
        },
        P8FibShape {
            label: "3 × 3 (both non-trivial)",
            role: vec![
                vec![1.0, 0.5, 0.25],
                vec![0.5, 1.0, 0.5],
                vec![0.25, 0.5, 1.0],
            ],
            fiber: vec![
                vec![1.0, 0.3, 0.6],
                vec![0.2, 1.0, 0.4],
                vec![0.55, 0.15, 1.0],
            ],
        },
    ]
}

/// Evaluate every S-fib shape and print its row; returns `true` iff all agree.
fn p8_sfib_gate() -> bool {
    println!("| shape | agents | harness magnitude | `expected_magnitude()` | rel Δ | gate |");
    println!("|-------|-------:|------------------:|-----------------------:|------:|------|");
    let mut all_ok = true;
    for shape in p8_fib_shapes() {
        let role = RoleModulation::new(shape.role.clone())
            .expect("invariant: literal square table with entries in [0, 1]");
        let fiber = RoleModulation::new(shape.fiber.clone())
            .expect("invariant: literal square table with entries in [0, 1]");
        let grid = role_grid(&role, &fiber)
            .expect("invariant: both S-fib factors carry an exact 1.0 diagonal");
        let agents: Vec<usize> = (0..grid.n_agents()).collect();
        // The example's own fresh route — the same public entry point
        // `magnitude_at_t` calls, on the grid's product couplings.
        let actual =
            catgraph_magnitude::coalition_magnitude_from_couplings(&agents, grid.couplings(), &agents, 1.0);
        let proof = grid.proof(1.0);
        match (actual, proof) {
            (Ok(a), Ok(p)) => {
                let e = p.expected_magnitude();
                let rel = (a - e).abs() / a.abs().max(e.abs()).max(1.0);
                let ok = rel <= P8_FIB_REL_TOL;
                all_ok &= ok;
                println!(
                    "| {} | {} | {a:.12} | {e:.12} | {rel:.2e} | {} |",
                    shape.label,
                    grid.n_agents(),
                    pass(ok)
                );
            }
            (a, p) => {
                all_ok = false;
                println!(
                    "| {} | {} | {} | {} | — | {} |",
                    shape.label,
                    grid.n_agents(),
                    a.map_or_else(|e| format!("Err({e})"), |v| format!("{v:.12}")),
                    p.map_or_else(
                        |e| format!("Err({e})"),
                        |v| format!("{:.12}", v.expected_magnitude())
                    ),
                    pass(false)
                );
            }
        }
    }
    all_ok
}

#[allow(clippy::too_many_lines)]
fn part8_eq4_typed_roles() {
    println!("# koalisi #72 — Part 8: EQ4 typed-roles battery (REGISTERED)");
    println!();
    println!(
        "_governed by `docs/prereg-K4-eq4-typed-roles.md` (registered BEFORE this code; owner design-lock D1–D9 on #72). Report date {P8_REPORT_DATE}. World **v2t** (prereg §2): the frozen `draw_prefix_v2` prefix, then `R = {P8_ROLES}` worker roles and per-required-bit role tags APPENDED off the same SplitMix64 stream, with a role-feasibility rejection re-draw (≤ {P8_REDRAW_CAP} attempts per task). Ground truth is **role-matched coverage** — a required bit `b` tagged `r` counts only if a member of role `r` holds it — and `PRIMARY = success_rate × mean cov_eff` keeps the standing stream-level shape. Seeds **{P8_SEED_START}..{P8_SEED_END}** (fresh; 90..120 and 150..180 stay reserved). Arms: `mag` = frozen `MagnitudePolicy::default()` (control, sees bit masks only), `mag-typed` = the same policy `.with_role_modulation(pool roles, ρ)` at the **oracle `ρ = δ`** (identity matrix: cross-role substitutability exactly 0), `scalar` = `AifDecisionPolicy::default()` and `arm-E1` = the v5 E1 `PersistentAifArm` (untyped context, non-gating). Latency is recorded and NEVER gating (prereg §7)._"
    );
    println!();
    println!(
        "_Instance convention: a FRESH policy per seed for every arm. `mag-typed`'s role map keys on the pool, so a per-seed rebuild is structural there; applying the same convention to every arm keeps the arms comparable. It differs from Part 7's one-instance-per-arm choice and costs the untyped control its cross-seed evaluator-cache warmth — which moves latency only: the cache is decision-frozen by the knife-edge fallback (gotcha 15), and latency is non-gating in this registration._"
    );
    println!();
    println!(
        "_Metric note: `success ≡ completed` under the typed ground truth (prereg §2 defines `completed(task)` and `PRIMARY = success_rate × mean_cov_eff`, and the v2t draw carries no per-member performance matrix). The Scope-B reliability gate of Parts 3–7 is deliberately NOT layered on top — the registered question is whether role structure converts, not whether reliability does._"
    );
    println!();

    // --- Instances (shared by every arm) ------------------------------------
    let insts = p8_instances(P8_ROLES);
    let n_seeds = insts.len();
    let total_redraws: usize = insts.iter().map(|i| i.redraws).sum();
    let seeds_with_redraws = insts.iter().filter(|i| i.redraws > 0).count();
    let max_redraws = insts.iter().map(|i| i.redraws).max().unwrap_or(0);
    let worst_attempts = insts.iter().map(|i| i.max_attempts).max().unwrap_or(0);
    // What the rejection re-draw does to the task-size distribution: the v2t
    // requirements against the pure-v2 requirements of the SAME seeds.
    let v2t_req: Vec<f64> = insts
        .iter()
        .flat_map(|i| i.tasks.iter().map(|t| f64::from(t.required.count_ones())))
        .collect();
    let v2_req: Vec<f64> = (P8_SEED_START..P8_SEED_END)
        .flat_map(|s| {
            let mut rng = SplitMix64::new(s);
            let (_, tasks) = draw_prefix_v2(&mut rng);
            tasks
                .into_iter()
                .map(|t| f64::from(t.required.count_ones()))
                .collect::<Vec<f64>>()
        })
        .collect();
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;

    // --- The registered arms -------------------------------------------------
    let oracle = p8_rho(0.0);
    let (mag, mag_lat) = p8_battery(
        &insts,
        |_| Box::new(MagnitudePolicy::default()) as Box<dyn CoalitionDecisionPolicy>,
        P8Metric::Typed,
    );
    let (typed, typed_lat) = p8_battery(
        &insts,
        |inst| Box::new(p8_typed_policy(inst, &oracle)) as Box<dyn CoalitionDecisionPolicy>,
        P8Metric::Typed,
    );
    let (scalar, scalar_lat) = p8_battery(
        &insts,
        |_| Box::new(AifDecisionPolicy::default()) as Box<dyn CoalitionDecisionPolicy>,
        P8Metric::Typed,
    );
    let (e1, e1_lat) = p8_e1_battery(&insts, e1_config());

    // --- §6 gates (run and asserted BEFORE any leg is read) ------------------
    println!("## Gates (prereg §6 — any failure ⇒ RUN-INVALID)");
    println!();

    // X-identity cell 1 — the identity configuration IS the untyped path.
    let (mag_again, _) = p8_battery(
        &insts,
        |_| Box::new(MagnitudePolicy::default()) as Box<dyn CoalitionDecisionPolicy>,
        P8Metric::Typed,
    );
    for (i, (a, b)) in mag.iter().zip(mag_again.iter()).enumerate() {
        assert_eq!(
            a.acts, b.acts,
            "X-identity cell 1: identity-configuration acts must match `mag` on seed {}",
            P8_SEED_START + i as u64
        );
        assert_eq!(
            a.scores, b.scores,
            "X-identity cell 1: identity-configuration score bits must match `mag` on seed {}",
            P8_SEED_START + i as u64
        );
    }

    // The identity WORLD (R = 1): every worker and every tag in role 0, so
    // role-matched coverage IS untyped coverage — the reduction prereg §2 claims,
    // checked per task on the whole pool (the strongest member set available).
    let id_insts = p8_instances(P8_IDENTITY_ROLES);
    let mut id_tasks = 0usize;
    for inst in &id_insts {
        let all: Vec<usize> = (0..inst.agents.len()).collect();
        for task in &inst.tasks {
            assert_eq!(
                p8_typed_covered(inst, &all, task),
                p8_untyped_covered(inst, &all, task.required),
                "X-identity cell 1: at R = 1 the typed metric must reduce to the untyped one"
            );
            id_tasks += 1;
        }
    }
    println!(
        "- **X-identity cell 1 — {}.** The identity configuration (no roles supplied) routes STRUCTURALLY to the untyped path, so this is identity by construction, not by measurement; the run asserts it anyway as a determinism check — an independently constructed `MagnitudePolicy::default()` reproduces the `mag` arm's acts AND raw score BIT PATTERNS on all {n_seeds} seeds ({} decisions). Alongside it, the R = 1 identity world (`next() % 1`, same code path) is checked for the metric reduction prereg §2 claims: role-matched coverage equals untyped coverage on all {id_tasks} of its tasks.",
        pass(true),
        mag.iter().map(|r| r.acts.len()).sum::<usize>()
    );

    // X-identity cell 2 — the typed path at ρ ≡ 1 is the untyped arm's decisions.
    let ones = p8_rho(1.0);
    let (typed_ones, _) = p8_battery(
        &insts,
        |inst| Box::new(p8_typed_policy(inst, &ones)) as Box<dyn CoalitionDecisionPolicy>,
        P8Metric::Typed,
    );
    for (i, (a, b)) in mag.iter().zip(typed_ones.iter()).enumerate() {
        let seed = P8_SEED_START + i as u64;
        assert_eq!(a.acts, b.acts, "X-identity cell 2: acts on seed {seed}");
        assert_eq!(
            a.primary.to_bits(),
            b.primary.to_bits(),
            "X-identity cell 2: PRIMARY on seed {seed}"
        );
        assert_eq!(a.churn, b.churn, "X-identity cell 2: churn on seed {seed}");
    }
    println!(
        "- **X-identity cell 2 — {}.** The typed path at `ρ ≡ 1` (exact `1.0` entries; `1.0·π == π` in IEEE) over the R = {P8_ROLES} world reproduces `mag` on **acts + per-seed PRIMARY (bit-identical) + churn**, all {n_seeds} seeds. Raw scores are NOT compared: the typed route evaluates both sides fresh where the untyped arm answers a cached incremental query, which re-associates the low bits — the registered gate is on decisions, not float bit patterns (the EQ3 H-par′ lesson).",
        pass(true)
    );
    println!();

    // S-fib.
    println!("### S-fib — role-grid factorization certificate");
    println!();
    println!(
        "_Three deterministic `role_space × fiber` shapes built with `role_grid`; each grid coalition is evaluated through the example's own fresh route (`coalition_magnitude_from_couplings` at the pinned `t = 1`, the entry point `magnitude_at_t` uses) and compared against `RoleGrid::proof(1.0).expected_magnitude()` within the **upstream-documented** relative tolerance `{P8_FIB_REL_TOL:.0e} · max(|a|, |b|, 1)` — the bound catgraph `v0.7.0`'s own `coalition_typed` tests (`rel_close`) and the `proof` doctest use. Upstream documents that tightness as safe only away from near-1 non-merged couplings, so every off-diagonal here sits well inside `(0, 1)`._"
    );
    println!();
    let sfib_ok = p8_sfib_gate();
    println!();
    assert!(
        sfib_ok,
        "S-fib: every role-grid instance must match its factorization certificate"
    );
    println!("- **S-fib — {}** (all shapes agree).", pass(sfib_ok));
    println!();
    println!(
        "- **X-battery** (frozen Parts 1–7 byte-identical on every quality/churn/verdict line) is checked OUTSIDE this binary, by diffing this run's Parts 1–7 against a pre-change baseline; latency-only diffs are the standing exclusion."
    );
    println!();

    // --- Draw / feasibility --------------------------------------------------
    println!("## Draw and feasibility (prereg §2)");
    println!();
    println!(
        "Rejection re-draws over the {n_seeds} seeds: **{total_redraws}** in total, on **{seeds_with_redraws}** seeds, worst seed **{max_redraws}** (a per-SEED sum over its {TASKS} tasks). Per-seed counts are the `redraws` column of the H-T table below. Role-matched feasibility is strictly stronger than the untyped pool coverage of the #63 draw — the same worker no longer covers a bit for every tag, only for its own role — so this regime rejects heavily where the #63 draw barely did."
    );
    println!();
    println!(
        "- **What a re-draw replaces:** the FULL v2 task draw — the size `r ∈ 2..=8`, the `r` distinct required bits, and then all `r` role tags, all off the same stream. It does NOT re-tag a kept requirement, and it never re-draws the arrival order (which is role-independent and is drawn once in the v2 prefix)."
    );
    println!(
        "- **Cap headroom:** the budget is {P8_REDRAW_CAP} **total attempts per TASK** — the initial draw is attempt 1, so the budget allows {} re-draws — and the worst single task in this run needed **{worst_attempts}** attempts ({:.1}% of the budget). Exceeding it panics the run as RUN-INVALID rather than silently biasing the draw.",
        P8_REDRAW_CAP - 1,
        100.0 * worst_attempts as f64 / P8_REDRAW_CAP as f64
    );
    println!(
        "- **Induced task-size shift (disclosure, not a deviation):** rejection sampling is not size-neutral — a large requirement is far likelier to contain a bit no worker of that bit's role holds, so it is rejected more often. Realized `|required|` over the {} v2t tasks: mean **{:.2}**, median **{:.1}**; the pure-v2 draw of the SAME seeds has mean {:.2}, median {:.1}. The registered v2t world IS the post-rejection distribution (prereg §2 fixes the mechanism), so this is the world both arms face — but any cross-part comparison against a v2 number must read it as a different task-size mix, not only a different metric.",
        v2t_req.len(),
        mean(&v2t_req),
        median(v2t_req.clone()),
        mean(&v2_req),
        median(v2_req.clone())
    );
    // Tag conditioning: the re-draw also conditions the TAGS on the pool, not
    // only the task size.
    let per_seed_single: Vec<f64> = insts
        .iter()
        .map(|inst| {
            let f: Vec<f64> = inst
                .tasks
                .iter()
                .map(|t| p8_single_role_fraction(inst, t.required))
                .collect();
            mean(&f)
        })
        .collect();
    let single_all: Vec<f64> = insts
        .iter()
        .flat_map(|inst| {
            inst.tasks
                .iter()
                .map(|t| p8_single_role_fraction(inst, t.required))
                .collect::<Vec<f64>>()
        })
        .collect();
    println!(
        "- **Tag conditioning (disclosure):** the re-draw conditions TAGS on the pool as well. A required bit held by workers of exactly ONE role has its tag forced — every feasible tag for it is that role — so on such a bit role-matched coverage coincides with untyped coverage and the bit is contest-dead. Measured here: **{:.1}%** of required bits across all {} tasks (per-seed mean {:.1}% … {:.1}%). The direction is CONSERVATIVE for H-T — the typed arm can only earn its margin on the remaining bits — and re-draw intensity anti-correlates with pool size, so the small-pool seeds carry the most conditioning.",
        100.0 * mean(&single_all),
        single_all.len(),
        100.0 * per_seed_single.iter().copied().fold(f64::INFINITY, f64::min),
        100.0 * per_seed_single.iter().copied().fold(f64::NEG_INFINITY, f64::max)
    );
    println!();

    // --- Arms ----------------------------------------------------------------
    println!("## Arms (pooled over {n_seeds} seeds, v2t world)");
    println!();
    println!(
        "| arm | median PRIMARY | vs `mag` | superior seeds | median churn | median µs/decision |"
    );
    println!(
        "|-----|---------------:|---------:|---------------:|-------------:|-------------------:|"
    );
    p8_row("mag", &mag, &mag, &mag_lat);
    p8_row("mag-typed", &typed, &mag, &typed_lat);
    p8_row("scalar", &scalar, &mag, &scalar_lat);
    p8_row("arm-E1", &e1, &mag, &e1_lat);
    println!();
    println!(
        "_`scalar` and `arm-E1` are untyped context baselines, non-gating (prereg §3). `arm-E1` is the v5 E1 configuration (learned per-bit precisions + fixed-γ = 16 MeanField queries) fed the ROLE-MATCHED per-bit outcome after every task — the typed analogue of the oracle signal it was registered on._"
    );
    println!();

    // --- H-T -----------------------------------------------------------------
    let mag_med = median(p8_primaries(&mag));
    let typed_med = median(p8_primaries(&typed));
    let bar = P8_HT_FACTOR * mag_med;
    let superior = p8_superior_count(&typed, &mag);
    let h_t_median = typed_med >= bar;
    let h_t_consistency = superior >= P8_HT_SUPERIOR_MIN;
    let h_t = h_t_median && h_t_consistency;

    println!("## H-T (confirmatory) — typed valuation beats untyped");
    println!();
    println!(
        "| seed | n | redraws | mag | mag-typed | Δ | churn mag | churn typed |"
    );
    println!(
        "|-----:|--:|--------:|----:|----------:|--:|----------:|------------:|"
    );
    for (i, ((m, t), inst)) in mag.iter().zip(typed.iter()).zip(insts.iter()).enumerate() {
        println!(
            "| {} | {} | {} | {:.4} | {:.4} | {:+.4} | {} | {} |",
            P8_SEED_START + i as u64,
            inst.agents.len(),
            inst.redraws,
            m.primary,
            t.primary,
            t.primary - m.primary,
            m.churn,
            t.churn
        );
    }
    println!();
    println!(
        "- Conjunct 1 (median): `mag-typed` **{typed_med:.4}** vs bar {P8_HT_FACTOR} × {mag_med:.4} = **{bar:.4}** ⇒ **{}**.",
        pass(h_t_median)
    );
    println!(
        "- Conjunct 2 (consistency): strictly superior on **{superior}/{n_seeds}** seeds, bar {P8_HT_SUPERIOR_MIN} ⇒ **{}**.",
        pass(h_t_consistency)
    );
    println!(
        "- Success-rate context (never gated): `mag` {:.4} · `mag-typed` {:.4} (mean over seeds).",
        mag.iter().map(|r| r.success_rate).sum::<f64>() / n_seeds as f64,
        typed.iter().map(|r| r.success_rate).sum::<f64>() / n_seeds as f64
    );
    println!(
        "- **Mechanism (what the arm can and cannot do):** `mag-typed` never sees a tag — its relevance masks stay untyped by registration (prereg §4), so it cannot route coverage toward the role a bit asks for. Its ONE lever is refusing to treat cross-role members as substitutes: `ρ = δ` zeroes their coupling, so they never skeletalize into a single effective agent and each keeps its own diversity weight. The arm therefore RETAINS role-diverse redundancy that the untyped arm collapses away, and the win shows up as role-matched completion (mean success {:.2} → {:.2}) rather than as smarter coverage selection.",
        mag.iter().map(|r| r.success_rate).sum::<f64>() / n_seeds as f64,
        typed.iter().map(|r| r.success_rate).sum::<f64>() / n_seeds as f64
    );
    println!();

    let verdict = if h_t {
        "VALIDATED (typed roles)"
    } else {
        "FALSIFIED (typed roles)"
    };
    println!("## VERDICT: **{verdict}**");
    println!();
    println!(
        "_Grammar (prereg §7): `VALIDATED (typed roles)` = both H-T conjuncts with every §6 gate holding · `FALSIFIED (typed roles)` = gates hold, either conjunct fails · `RUN-INVALID` = any §6 gate fails. This run: conjunct 1 {} · conjunct 2 {} · gates X-identity {} / S-fib {}. No bar moves either way, and the v1/v2 K4 verdicts, EQ3's verdict, and the #54 arm question (mag = demonstrated default, FINAL) are untouched regardless of outcome (prereg §7 pre-commitments)._",
        pass(h_t_median),
        pass(h_t_consistency),
        pass(true),
        pass(sfib_ok)
    );
    println!();

    // --- E-deg ---------------------------------------------------------------
    println!("## E-deg (registered context, non-gating) — ρ mis-specification");
    println!();
    println!(
        "_The oracle table's exact-`0` off-diagonal is lifted to `ρ_off ∈ {{{}, {}}}` (diagonal stays `1.0`), same {n_seeds} seeds, same world: how much of any H-T margin survives a mis-specified table (#54 oracle-vs-degraded discipline)._",
        P8_RHO_OFF_GRID[0], P8_RHO_OFF_GRID[1]
    );
    println!();
    println!(
        "| arm | median PRIMARY | vs `mag` | superior seeds | median churn | median µs/decision |"
    );
    println!(
        "|-----|---------------:|---------:|---------------:|-------------:|-------------------:|"
    );
    p8_row("mag-typed (ρ_off = 0, oracle)", &typed, &mag, &typed_lat);
    for off in P8_RHO_OFF_GRID {
        let rho = p8_rho(off);
        let (cell, cell_lat) = p8_battery(
            &insts,
            |inst| Box::new(p8_typed_policy(inst, &rho)) as Box<dyn CoalitionDecisionPolicy>,
            P8Metric::Typed,
        );
        p8_row(&format!("mag-typed (ρ_off = {off})"), &cell, &mag, &cell_lat);
    }
    p8_row("mag (control)", &mag, &mag, &mag_lat);
    println!();

    // --- E-ceil --------------------------------------------------------------
    let (ceil, ceil_lat) = p8_pertask_battery(
        &insts,
        |inst, task| {
            Box::new(TypedRelevanceMag {
                roles: inst.roles.clone(),
                tags: task.tags,
                join_margin: 0.0,
            }) as Box<dyn CoalitionDecisionPolicy>
        },
        P8Metric::Typed,
    );
    println!("## E-ceil (registered exploratory, non-gating) — typed-relevance reference arm");
    println!();
    println!(
        "_An example-side arm that re-types the relevance masks themselves — `rel_i = caps_i ∩ (required bits tagged role_i)` — and then runs the ORDINARY untyped substitutability couplings over those masks (no ρ), evaluated fresh through the example's `magnitude_at_t` at the pinned `t = 1`. It is the **fully-informed reference arm within the magnitude family**: it reads the tags the registered lever deliberately withholds. It is NOT a supremum — no oracle anchor exists at these pool sizes, and nothing rules out a non-magnitude arm scoring higher — so read it as a reference margin, not a ceiling on what is achievable._"
    );
    println!();
    println!(
        "| arm | median PRIMARY | vs `mag` | superior seeds | median churn | median µs/decision |"
    );
    println!(
        "|-----|---------------:|---------:|---------------:|-------------:|-------------------:|"
    );
    p8_row("mag", &mag, &mag, &mag_lat);
    p8_row("mag-typed", &typed, &mag, &typed_lat);
    p8_row("E-ceil", &ceil, &mag, &ceil_lat);
    println!();
    let ceil_med = median(p8_primaries(&ceil));
    let reference_margin = ceil_med - mag_med;
    let converted = if reference_margin.abs() > 0.0 {
        format!("{:.1}%", 100.0 * (typed_med - mag_med) / reference_margin)
    } else {
        "n/a (reference margin is zero)".to_owned()
    };
    println!(
        "- **Conversion fraction:** `(mag-typed − mag) / (E-ceil − mag)` = ({typed_med:.4} − {mag_med:.4}) / ({ceil_med:.4} − {mag_med:.4}) = **{converted}**. The ρ-modulation lever converts that fraction of the tag-informed reference margin; the remainder requires tag knowledge the registered lever deliberately withholds (prereg §4 keeps the relevance masks untyped precisely so the test can fail). Medians, so the ratio is a summary contrast, not a per-seed decomposition."
    );
    println!();

    // --- E-ρq ----------------------------------------------------------------
    let rho_q = p8_rho(P8_RHOQ_OFF);
    let (rq_mag, rq_mag_lat) = p8_battery(
        &insts,
        |_| Box::new(MagnitudePolicy::default()) as Box<dyn CoalitionDecisionPolicy>,
        P8Metric::RhoQ,
    );
    let (rq_typed, rq_typed_lat) = p8_battery(
        &insts,
        |inst| Box::new(p8_typed_policy(inst, &rho_q)) as Box<dyn CoalitionDecisionPolicy>,
        P8Metric::RhoQ,
    );
    let rho_inv = p8_rho_at(P8_RHOQ_OFF, 1.0);
    let (rq_inv, rq_inv_lat) = p8_battery(
        &insts,
        |inst| Box::new(p8_typed_policy(inst, &rho_inv)) as Box<dyn CoalitionDecisionPolicy>,
        P8Metric::RhoQ,
    );
    println!("## E-ρq (registered exploratory, non-gating) — the ρ-structured quality world");
    println!();
    println!(
        "_Same pool and same tasks, a different WORLD (prereg §5, D1c): coverage is UNTYPED and task quality is `cov_eff × mean pairwise ρ_world(role_i, role_j)` over the final members (singleton ⇒ `1.0`), with `ρ_world` the planted symmetric table (`ρ(r,r) = 1`, `ρ_off = {P8_RHOQ_OFF}`). `mag-typed` here carries THAT table as its oracle — and per **Amendment A1.2** that configuration is structurally **ANTI-aligned**, not friendly: a `ρ < 1` entry WEAKENS the cross-role coupling, magnitude rewards diversity, so weakly-coupled cross-role candidates look MORE additive and are MORE likely to be admitted — while this world's quality term rewards role COHESION. The leg measures that anti-alignment (the gotcha-23 lesson restated: magnitude scores diversity, not dependability, and a table that reads as 'these two are unlike' is an argument to keep both)._"
    );
    println!();
    println!(
        "_The **E-ρq-inv** cell (Amendment 2, owner-approved) flips the modulation direction to test whether alignment is recoverable INSIDE T2: `ρ(r,r) = {P8_RHOQ_OFF}` on the diagonal and `ρ(r ≠ r′) = 1.0` off it, so cross-role members now look fully substitutable and same-role members look only partly so — magnitude should then favour role-cohesive coalitions, which is what this world pays for. The world's quality table is unchanged; only the arm's table is inverted._"
    );
    println!();
    println!(
        "| arm | median PRIMARY_ρq | vs `mag` | superior seeds | median churn | median µs/decision |"
    );
    println!(
        "|-----|------------------:|---------:|---------------:|-------------:|-------------------:|"
    );
    p8_row("mag", &rq_mag, &rq_mag, &rq_mag_lat);
    p8_row("mag-typed (ρ_world)", &rq_typed, &rq_mag, &rq_typed_lat);
    p8_row("mag-typed-inv (ρ flipped)", &rq_inv, &rq_mag, &rq_inv_lat);
    println!();

    // --- E-T3 ----------------------------------------------------------------
    let t3_counters = Arc::new(P8ChannelCounters::default());
    let (t3, t3_lat) = p8_pertask_battery(
        &insts,
        |_inst, task| {
            Box::new(ChannelMagnitudePolicy {
                tags: task.tags,
                join_margin: 0.0,
                counters: Arc::clone(&t3_counters),
            }) as Box<dyn CoalitionDecisionPolicy>
        },
        P8Metric::Typed,
    );
    let (t3_entries, t3_neutral, t3_unit_neutral, t3_unit_rounded) = t3_counters.get();
    println!("## E-T3 (registered exploratory, non-gating) — channel-valued couplings");
    println!();
    println!(
        "_`C = R = {P8_CHANNELS}` channels; channel `c` carries the role-`c`-restricted substitutability `A_c(i → j) = |rel_i ∩ rel_j ∩ tagged(c)| / |rel_i ∩ tagged(c)|` over UNTYPED relevance, collapsed with the registered uniform `θ = (1/3, 1/3, 1/3)` (D3) through `ChannelCouplings::collapse`, then evaluated fresh at `t = 1`. **REGISTERED CAVEAT** (prereg §5): an empty denominator collapses to the neutral `1.0`, so the product collapse makes 'no evidence' neutral-HIGH and biases cross-role coupling upward — one reason this leg is exploratory. A second float trap upstream names: `powf` can round up to exactly `1.0`, so a collapsed table can carry exact-`1.0` couplings and hence skeletal merges between agents no single channel perfectly couples._"
    );
    println!();
    println!(
        "| arm | median PRIMARY | vs `mag` | superior seeds | median churn | median µs/decision |"
    );
    println!(
        "|-----|---------------:|---------:|---------------:|-------------:|-------------------:|"
    );
    p8_row("mag", &mag, &mag, &mag_lat);
    p8_row("E-T3", &t3, &mag, &t3_lat);
    println!();
    println!(
        "- **Caveat incidence (measured, decision-inert counters):** of **{t3_entries}** channel entries built across the leg's evaluations, **{t3_neutral}** ({:.1}%) took the neutral `1.0` for an empty `rel_i ∩ tagged(c)` denominator. Exactly-`1.0` collapsed couplings — each of which forces a skeletal merge — split into **{t3_unit_neutral}** where EVERY channel was neutral (the registered caveat's pure 'no evidence anywhere' case) and **{t3_unit_rounded}** where at least one channel was non-neutral, i.e. `powf` rounded a strictly-sub-1 product up to exactly `1.0` (the upstream trap). Counted per EVALUATION, not per decision (`should_join` pays two, `should_leave` two), and written only after each value was already fixed — the counters cannot move a decision.",
        100.0 * t3_neutral as f64 / t3_entries.max(1) as f64
    );
    println!();

    // --- T1 instrumentation --------------------------------------------------
    let shares = p8_role_share_pass(&insts, &oracle);
    println!("## T1 instrumentation (non-gating) — role shares of the final coalitions");
    println!();
    println!(
        "_`CoalitionEvaluator::role_shares` (upstream T1) on every task's final `mag-typed` coalition, constructed fresh OFF the decision path over the same ρ-modulated couplings the arm evaluated, with roles indexed by member-local position. Weights are read, never moved: `Mag = Σ_c w_c` (Leinster 2013 Lemma 1.1.4) bucketed by role, and a role-mixed skeletal class is reported rather than split._"
    );
    println!();
    println!(
        "- Samples: **{}** decomposed coalitions ({} tasks skipped — no relevance-surviving member; {} upstream errors).",
        shares.samples, shares.empty, shares.errors
    );
    println!(
        "- **Mixed classes: {}** across {} samples ({} samples exact, i.e. every skeletal class single-role). Expected 0 under the oracle `ρ = δ`: a cross-role coupling is modulated to exactly `0`, so two agents of different roles can never reach mutual closure `1.0` and never share a class — the only merges left are same-role clones, which are single-role by construction. A nonzero count would mean the modulation is not reaching the closure, and is printed either way.",
        shares.mixed_classes, shares.samples, shares.exact
    );
    println!(
        "- `Σ_r share(r)` vs `base_value()` on exact samples: largest relative gap **{:.2e}** (upstream contracts these as equal up to float re-association, not bit-identity).",
        shares.max_rel_gap
    );
    println!();
    println!("| role | samples | p25 | median | p75 |");
    println!("|-----:|--------:|----:|-------:|----:|");
    for (r, bucket) in shares.per_role.iter().enumerate() {
        if bucket.is_empty() {
            println!("| {r} | 0 | — | — | — |");
            continue;
        }
        let mut sorted = bucket.clone();
        sorted.sort_by(f64::total_cmp);
        println!(
            "| {r} | {} | {:.4} | {:.4} | {:.4} |",
            sorted.len(),
            percentile(&sorted, 0.25),
            percentile(&sorted, 0.5),
            percentile(&sorted, 0.75)
        );
    }
    println!();
    println!(
        "_A role present only inside mixed classes still appears with share `0.0`; a role that never appeared in a coalition's partition contributes no sample at all (upstream's `Some(0.0)` vs `None` distinction)._"
    );
    println!();
}

// ===========================================================================
// Part 9 — EQ5a process-structured battery (koalisi #76). REGISTERED.
//
// Governed by `docs/prereg-K4-eq5a-process-structured.md` (registered BEFORE
// this code; owner design-lock D1–D11 on #76, Amendment 1 pinning the rule
// theory / fuel / staffing price / λ / draw parameters, Amendment 2 voiding
// §4's "3–5 rules" numeral in favour of the schema closure, Amendment 3
// re-reading valuation-only as the unstaffable residual (A3.1) and widening the
// fusion schema to every same-role ordered pair — 174 instances (A3.2)).
//
// The world is the **v2w** regime: the Part 8 `v2t` prefix VERBATIM — so the
// prefix of the stream stays bit-identical to a pure-v2t draw of the same seed
// (`v2w_prefix_matches_v2t`) — with the workflow SHAPE draw appended off the
// same SplitMix64. Every task becomes a colored string diagram over
// `FrobeniusOr<Step>`; an arm declares the writing it staffs, and the scorer
// scores each arm's DECLARED writing after verifying it.
//
// Everything here is ADDITIVE — Parts 1–8 above are the byte-identity gate
// (§6 X-battery), so no frozen draw, runner, arm, or print statement is
// touched.
// ===========================================================================

/// Part 9 seed range (prereg §1, owner D7) — fresh seeds; 90..120 and 150..180
/// stay reserved.
const P9_SEED_START: u64 = 270;
const P9_SEED_END: u64 = 300;
/// Confirmatory fuel budget, in rewrite APPLICATIONS (Amendment A1.2).
const P9_FUEL: usize = 256;
/// The registered E-fuel sweep (Amendment A1.2; exploratory, non-gating).
const P9_FUEL_GRID: [usize; 4] = [32, 128, 512, 2048];
/// The valuation-only coefficient λ (Amendment A1.4).
const P9_LAMBDA: f64 = 0.05;
/// The registered exploratory λ grid (Amendment A1.4; non-gating).
const P9_LAMBDA_GRID: [f64; 3] = [0.01, 0.05, 0.25];
/// Spider fan-out probability per same-role adjacent pair (Amendment A1.5) is
/// `0.25`, drawn as `next_u64() % 4 == 0` — one draw per pair, exactly a quarter
/// of the 2^64 range, no float comparison anywhere in a seeded draw.
const P9_FANOUT_DENOM: u64 = 4;
/// H-P conjunct 1: a cell's PRIMARY median must be at least this multiple of the
/// control's. Raised from the lineage's standing 1.25× to pay for four looks
/// (prereg §5), pinned pre-run.
const P9_HP_FACTOR: f64 = 1.4;
/// H-P conjunct 2: strictly superior on at least this many of the 30 seeds (70 %).
const P9_HP_SUPERIOR_MIN: usize = 21;
/// E-ceil leg (ii): the high fuel its harness-side search runs at.
const P9_ECEIL_FUEL: usize = 2048;
/// E-ceil leg (ii): the pinned subsample, `P9_SEED_START .. +P9_ECEIL_SEEDS`.
const P9_ECEIL_SEEDS: u64 = 6;
/// E-ceil leg (ii): the weight a singled-out step carries in the per-step
/// objective family. Finite and modest so no cost sum can overflow or saturate.
const P9_ECEIL_HEAVY: u64 = 1000;
/// Report date for the Part 9 battery, stamped per committed run.
const P9_REPORT_DATE: &str = "2026-08-05";

/// One seeded v2w instance: the Part 8 `TypedInstance` verbatim, plus each
/// task's as-written workflow and the task-constant pool staffing table.
struct WorkflowInstance {
    /// The v2t instance — pool, roles, tagged tasks, re-draw bookkeeping.
    base: TypedInstance,
    /// As-written workflow per task, index-aligned with `base.tasks`.
    written: Vec<Workflow>,
    /// `1 + scarcity` prices, built ONCE from the drawn pool before any decision
    /// (Amendment A1.3) and constant for every task of the seed.
    table: StaffingTable,
}

/// Whether the pinned FUSION schema can match this task at all — the WIDENED
/// schema of prereg Amendment A3.2.
///
/// Fusion is instantiated for every role and every ordered pair of distinct bits
/// `(b, b')` whose target `b'' = (b + b' + 4) mod bits` is neither of them, so a
/// task is eligible iff some role's tagged required bits contain both members of
/// one surviving pair. The pair set is read off the library's own
/// [`fusion_pairs`] rather than re-derived here: a disclosure that restated the
/// formula could drift from the theory it is reporting on.
///
/// Why it is worth counting: coverage is per DISTINCT `(bit, role)`, and
/// idempotence and spider absorption change only OCCURRENCE count. Fusion is
/// therefore the ONLY schema that can move a staffing decision, and this
/// predicate is the structural ceiling on how many tasks the confirmatory leg can
/// possibly act on. It is a NECESSARY condition, not a sufficient one — the two
/// steps must also become adjacent in the diagram for the rule to match convexly
/// (a fan-out between them has to be absorbed first).
fn p9_fusion_eligible(pairs: &[(u8, u8, u8)], task: &TypedTask) -> bool {
    let tagged = |bit: u8, role: usize| {
        let b = usize::from(bit);
        b < UNIVERSE && task.required & (1u32 << b) != 0 && task.tags[b] == role
    };
    (0..P8_ROLES).any(|r| {
        pairs
            .iter()
            .any(|&(first, second, _)| tagged(first, r) && tagged(second, r))
    })
}

/// The NARROW schema's eligibility, as measured at Stage 2 and quoted by prereg
/// Amendment A3.2: **43 of 600 tasks (7.2 %)** on this seed block under
/// Amendment 1's one-designated-pair-per-role fusion.
///
/// Hard-coded as a RECORDED PRIOR MEASUREMENT — the narrow schema no longer
/// exists in the theory, so the run cannot re-measure it; A3.2 makes the
/// comparison a mandatory report line because it is the evidence that the
/// confirmatory leg is now powered.
const P9_NARROW_ELIGIBLE_PRIOR: usize = 43;
const P9_NARROW_ELIGIBLE_TOTAL_PRIOR: usize = 600;

/// The `(bit, role)` steps of a task, in ascending bit order.
///
/// Every required bit carries exactly one tag, so this list is already distinct
/// — the shape draw is what introduces multiplicity, never the tagging.
fn p9_steps(task: &TypedTask) -> Vec<Step> {
    (0..UNIVERSE)
        .filter(|&b| task.required & (1u32 << b) != 0)
        .map(|b| Step::new(b as u8, Role::new(task.tags[b] as u8)))
        .collect()
}

/// A task's steps grouped by role, roles ascending, empty roles dropped.
fn p9_role_groups(task: &TypedTask) -> Vec<(Role, Vec<Step>)> {
    let steps = p9_steps(task);
    (0..P8_ROLES)
        .map(|r| Role::new(r as u8))
        .filter_map(|role| {
            let group: Vec<Step> = steps.iter().copied().filter(|s| s.role == role).collect();
            (!group.is_empty()).then_some((role, group))
        })
        .collect()
}

/// Pin a per-role leg list into one colored diagram: roles combined with
/// [`Free::tensor`] in ascending order, then the whole thing pinned through
/// [`ColoredExpr::new`].
///
/// # Panics
///
/// Panics on a color or arity rejection. Both are harness bugs rather than
/// runtime conditions: every leg is a chain of `r → r` boxes of ONE role, so the
/// pinned source word matches by construction. Amendment A2.3 is why the pin is
/// not optional — `Free::compose` checks widths, not colors, so a diagram that is
/// never pinned is never role-checked at all.
fn p9_pin(legs: Vec<(Role, PropExpr<WorkflowGen>)>) -> Workflow {
    let mut source: Vec<Role> = Vec::with_capacity(legs.len());
    let mut exprs: Vec<PropExpr<WorkflowGen>> = Vec::with_capacity(legs.len());
    for (role, expr) in legs {
        source.push(role);
        exprs.push(expr);
    }
    let expr = exprs
        .into_iter()
        .reduce(Free::tensor)
        .expect("invariant: |required| >= 2, so at least one role carries a step");
    ColoredExpr::new(source, expr)
        .expect("invariant: every leg is a single-role r -> r chain, so the pin matches")
}

/// Draw one task's as-written workflow off the APPENDED stream (prereg §2,
/// Amendment A1.5).
///
/// Per role, a sequential chain whose length is that role's tagged-bit count (no
/// free parameter). For each same-role ADJACENT pair one `next_u64()` is drawn;
/// on a `1/4` hit the later step is written as the fan-out-and-rejoin
/// `δ_r ; (s ⊗ s) ; μ_r` instead of the bare `s`.
///
/// # Why that fan-out shape and not a generic two-successor split
///
/// `δ_r ; (s ⊗ s) ; μ_r` is EXACTLY the left-hand side of the pinned
/// spider-absorption schema (Amendment A1.1 rule 3). A generic fan-out feeding
/// two *different* same-role successors would leave that schema dead on drawn
/// traffic — 24 of the theory's 174 instances would never match anything, and the
/// dependency they justify would be decorative. This is a draw-shape choice the
/// registration did not spell out; it is made so the registered schema is
/// reachable, and it is disclosed with the run.
fn p9_draw_shape(rng: &mut SplitMix64, task: &TypedTask) -> Workflow {
    let legs = p9_role_groups(task)
        .into_iter()
        .map(|(role, steps)| {
            let mut parts: Vec<PropExpr<WorkflowGen>> = Vec::with_capacity(steps.len());
            for (i, &s) in steps.iter().enumerate() {
                if i > 0 && rng.next_u64().is_multiple_of(P9_FANOUT_DENOM) {
                    parts.push(spider_expr(FrobeniusOr::Delta(role)));
                    parts.push(Free::tensor(step_expr(s), step_expr(s)));
                    parts.push(spider_expr(FrobeniusOr::Mu(role)));
                } else {
                    parts.push(step_expr(s));
                }
            }
            let expr = chain(parts)
                .expect("invariant: a non-empty list of 1 -> 1 same-role segments composes");
            (role, expr)
        })
        .collect();
    p9_pin(legs)
}

/// The degenerate world's shape (prereg §2, X-reduce): the all-parallel tensor of
/// the task's distinct steps, fan-out 0.
///
/// **Consumes ZERO stream draws** — load-bearing for the gate. Its instances are
/// therefore bit-for-bit the v2t instances of the same seed, so X-reduce compares
/// two code paths on ONE world rather than two worlds.
fn p9_degenerate_shape(task: &TypedTask) -> Workflow {
    let legs = p9_steps(task)
        .into_iter()
        .map(|s| (s.role, step_expr(s)))
        .collect();
    p9_pin(legs)
}

/// One seeded v2w instance. `degenerate` selects the X-reduce world.
fn p9_draw_instance(seed: u64, degenerate: bool) -> WorkflowInstance {
    let mut rng = SplitMix64::new(seed);
    // The v2t prefix VERBATIM — including its own role-feasibility rejection
    // re-draw, which is the "full task draw" re-draw prereg §2 registers. The
    // shape draw is appended strictly after it.
    let (agents, roles, tasks, redraws, max_attempts) = draw_prefix_v2t(&mut rng, P8_ROLES as u64);
    let base = TypedInstance {
        agents,
        roles,
        tasks,
        redraws,
        max_attempts,
    };
    let written: Vec<Workflow> = base
        .tasks
        .iter()
        .map(|t| {
            if degenerate {
                p9_degenerate_shape(t)
            } else {
                p9_draw_shape(&mut rng, t)
            }
        })
        .collect();
    let table = StaffingTable::from_pool(base.agents.iter().map(|a| {
        let role = *base
            .roles
            .get(a.id)
            .expect("invariant: the v2t draw assigns one role per pool worker");
        (a.caps, Role::new(role as u8))
    }));
    WorkflowInstance {
        base,
        written,
        table,
    }
}

/// The registered seed range's instances, drawn once and shared by every arm.
fn p9_instances(degenerate: bool) -> Vec<WorkflowInstance> {
    (P9_SEED_START..P9_SEED_END)
        .map(|s| p9_draw_instance(s, degenerate))
        .collect()
}

/// Whether a coalition covers one `(bit, role)` step: some member of that role
/// holds that bit. The role-matched question of prereg §2, asked per step.
fn p9_step_covered(inst: &WorkflowInstance, members: &[usize], step: Step) -> bool {
    let Some(mask) = step.capability_mask() else {
        return false;
    };
    members.iter().any(|&i| {
        inst.base
            .roles
            .get(i)
            .is_some_and(|&r| r as u8 == step.role.index())
            && inst.base.agents[i].caps & mask != 0
    })
}

/// Role-matched feasibility of a DEMAND against the whole pool — the E-conc
/// question. `StaffingTable` already tabulates `(bit, role) → holders`, so this
/// is a lookup per distinct pair, not a pool scan.
fn p9_demand_feasible(inst: &WorkflowInstance, d: &Demand) -> bool {
    d.distinct().all(|s| inst.table.is_staffable(s))
}

/// The OR-mask a policy sees: the union of the declared demand's bits.
///
/// **Documented consequence** (prereg §2): coverage is per DISTINCT `(bit, role)`
/// pair, so if one bit appears under two roles the policy's mask carries it ONCE
/// while the scorer counts TWO demands. The arm never sees a tag — exactly as in
/// EQ4 — so the flattening is the whole of what it is told.
fn p9_required_mask(d: &Demand) -> u32 {
    d.distinct()
        .filter_map(Step::capability_mask)
        .fold(0u32, |acc, m| acc | m)
}

// ---------------------------------------------------------------------------
// Declared writings — what each arm staffs, and the S-sound gate over them.
// ---------------------------------------------------------------------------

/// Which cost model a cell prices the process with (prereg §4 D5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum P9Cost {
    /// `per_gen = |_| 1` — generator count. The arm is metric-blind.
    Uniform,
    /// `1 + scarcity(b, r)` per step, 1 per spider (Amendment A1.3).
    Priced,
}

impl P9Cost {
    fn label(self) -> &'static str {
        match self {
            Self::Uniform => "uniform",
            Self::Priced => "priced",
        }
    }
}

/// Which mechanism a cell runs (prereg §4 D3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum P9Mechanism {
    /// The control: staff the workflow as written.
    AsWritten,
    /// Requirement rewriting: declare `optimize(...).best()` and staff THAT.
    Rewrite,
    /// Valuation-only: declared writing unchanged, `−λ · cost_of(...)` enters
    /// the score.
    Valuation,
}

/// What one arm declares for one task.
///
/// Per-task optimizer telemetry (`initial_cost`, `best_cost`, trace length,
/// `states_explored`, `fuel_exhausted`, schema fires) is aggregated into
/// [`P9DeclareStats`] as the writings are declared, so none of it is duplicated
/// here. Since Amendment A3.1 the valuation term is a function of the DEMAND and
/// the coalition rather than of a per-task cost constant, so the cost no longer
/// needs carrying per declaration either.
///
/// Every writing that is not the as-written one has ALREADY been `replay`-verified
/// and `content_eq`-checked by the time it lands in this struct (prereg §4
/// fairness clause) — verification is a precondition of declaring, not a step the
/// scorer could skip.
struct P9Declared {
    /// The writing this arm staffs. Read by the E-dedup pass over declared
    /// corpora; the scorer itself only needs `demand` and `required`.
    writing: Workflow,
    /// Its `(bit, role)` demand.
    demand: Demand,
    /// The OR-mask the policy is handed.
    required: u32,
    /// The optimizer rejected the task. **Decline-and-count** (prereg §4
    /// placement): the task forms no coalition and scores zero, never a silent
    /// as-written fallback.
    failed: bool,
}

/// Per-cell telemetry accumulated while declaring writings.
#[derive(Default)]
struct P9DeclareStats {
    /// Tasks whose declared writing was verified (`replay` + `content_eq`).
    verified: usize,
    /// S-sound failures — any nonzero value is RUN-INVALID.
    unsound: usize,
    /// `optimize` rejections (decline-and-count).
    failed: usize,
    /// Tasks whose search hit the fuel budget (mandatory disclosure, A1.2).
    fuel_exhausted: usize,
    /// Fires per schema, in [`Schema`] order.
    fired: [usize; 3],
    /// Tasks whose declared writing is content-equal to the as-written one.
    unchanged: usize,
    /// Tasks whose declared DISTINCT demand differs from the as-written one.
    ///
    /// **The number that decides how to read this leg.** Coverage is per distinct
    /// `(bit, role)`, so a rewrite that changes only OCCURRENCE count leaves the
    /// score bit-identical to the control: idempotence and spider absorption are
    /// staffing-invisible by construction (that is the inertness trap prereg §4
    /// names), and FUSION is the only schema that can move a decision. This
    /// counts the tasks where anything could have happened at all.
    demand_moved: usize,
    /// `initial_cost` / `best_cost` / step count / `states_explored` samples.
    initial: Vec<f64>,
    best: Vec<f64>,
    trace_len: Vec<f64>,
    explored: Vec<f64>,
    /// Distinct-demand reduction `as_written − declared`, per task.
    reduction: Vec<f64>,
}

fn p9_schema_slot(schema: Schema) -> usize {
    match schema {
        Schema::Idempotence => 0,
        Schema::Fusion => 1,
        Schema::SpiderAbsorption => 2,
    }
}

/// Declare one cell's writings over every task of every instance, verifying
/// every non-as-written writing before it is ever scored (prereg §4 fairness
/// clause + §6 S-sound).
fn p9_declare(
    insts: &[WorkflowInstance],
    rules: &[RewriteRule<WorkflowGen>],
    labels: &[LabelledRule],
    mechanism: P9Mechanism,
    cost: P9Cost,
    fuel: usize,
) -> (Vec<Vec<P9Declared>>, P9DeclareStats) {
    let mut stats = P9DeclareStats::default();
    let declared = insts
        .iter()
        .map(|inst| {
            inst.written
                .iter()
                .map(|written| match mechanism {
                    P9Mechanism::AsWritten | P9Mechanism::Valuation => {
                        let d = demand(written);
                        let price = match cost {
                            P9Cost::Uniform => workflow_cost(written, uniform_cost()),
                            P9Cost::Priced => workflow_cost(written, staffing_price(&inst.table)),
                        };
                        stats.unchanged += 1;
                        stats.initial.push(price as f64);
                        stats.best.push(price as f64);
                        stats.trace_len.push(0.0);
                        stats.explored.push(1.0);
                        stats.reduction.push(0.0);
                        P9Declared {
                            required: p9_required_mask(&d),
                            writing: written.clone(),
                            demand: d,
                            failed: false,
                        }
                    }
                    P9Mechanism::Rewrite => {
                        let outcome = match cost {
                            P9Cost::Uniform => {
                                optimize_workflow(written, rules, fuel, uniform_cost())
                            }
                            P9Cost::Priced => {
                                optimize_workflow(written, rules, fuel, staffing_price(&inst.table))
                            }
                        };
                        let Ok(outcome) = outcome else {
                            // Decline-and-count. The `Err` carries an upstream
                            // rejection; the run reports the count and the task
                            // scores zero rather than falling back silently.
                            stats.failed += 1;
                            let d = demand(written);
                            return P9Declared {
                                required: p9_required_mask(&d),
                                writing: written.clone(),
                                demand: d,
                                failed: true,
                            };
                        };
                        // S-sound, and it GATES the declaration rather than
                        // merely annotating it: an unverified writing is
                        // declined here, so it can never reach the scorer. The
                        // §6 assert on `unsound` still fails the whole run — the
                        // decline is what makes "verified before it is scored"
                        // true in the code and not just in the reporting order.
                        if verify_optimization(written, rules, &outcome).is_err() {
                            stats.unsound += 1;
                            let d = demand(written);
                            return P9Declared {
                                required: p9_required_mask(&d),
                                writing: written.clone(),
                                demand: d,
                                failed: true,
                            };
                        }
                        stats.verified += 1;
                        for step in outcome.steps() {
                            if let Some(label) = labels.get(step.rule()) {
                                stats.fired[p9_schema_slot(label.schema)] += 1;
                            }
                        }
                        if outcome.fuel_exhausted() {
                            stats.fuel_exhausted += 1;
                        }
                        let best = outcome.best();
                        if content_matches(written, best) {
                            stats.unchanged += 1;
                        }
                        let d = demand(best);
                        let before = demand(written).distinct_len();
                        if d.distinct_len() != before {
                            stats.demand_moved += 1;
                        }
                        stats.initial.push(outcome.initial_cost() as f64);
                        stats.best.push(outcome.best_cost() as f64);
                        stats.trace_len.push(outcome.steps().len() as f64);
                        stats.explored.push(outcome.states_explored() as f64);
                        stats
                            .reduction
                            .push(before as f64 - d.distinct_len() as f64);
                        P9Declared {
                            required: p9_required_mask(&d),
                            writing: best.clone(),
                            demand: d,
                            failed: false,
                        }
                    }
                })
                .collect()
        })
        .collect();
    (declared, stats)
}

// ---------------------------------------------------------------------------
// Arms and the scorer.
// ---------------------------------------------------------------------------

/// The valuation-only arm (prereg §4 D3b as **re-read by Amendment A3.1**): the
/// unstaffable residual.
///
/// ```text
/// value(S) = Mag(S) − λ · Σ per_gen(g)   over generator occurrences g of the
///                          declared writing whose (bit, role) is NOT covered by S
/// ```
///
/// # Why the re-read exists
///
/// §4 D3b originally scored `Mag(S) − λ · cost_of(writing, per_gen)` while also
/// fixing the declared writing to be independent of the coalition. That term is a
/// per-task CONSTANT, so it cancelled exactly from every join/leave margin and
/// both valuation cells measured bit-identical to the control at every registered
/// λ. Amendment A3.1 prices the residual instead: the penalty depends on `S`, so
/// admitting an agent that covers previously-unstaffable steps improves the score
/// by `λ` times those steps' price.
///
/// # Why it is not a rescaling of the magnitude signal
///
/// The penalty is weighted by `per_gen`, so **occurrence multiplicity and step
/// scarcity enter a decision for the first time** — neither is visible to
/// magnitude, which sees only the OR-mask of distinct demand. Demand and the
/// declared writing are unchanged; this is still valuation-only, not rewriting.
///
/// # Spiders
///
/// `μ`/`η`/`δ`/`ε` occurrences are **excluded from the residual**. They are
/// priced by `cost_of` (they are hyperedges like any other), but they name no
/// `(bit, role)`, so "uncovered" is not defined for them and no coalition could
/// ever discharge them. Counting them would add a constant to every side of every
/// margin — exactly the cancelling term A3.1 removed.
struct P9ValuationPolicy {
    inner: MagnitudePolicy,
    /// The coefficient λ (Amendment A1.4, pinned at [`P9_LAMBDA`]).
    lambda: f64,
    /// One entry per step OCCURRENCE of the declared writing, with its `per_gen`
    /// price under the cell's cost model. Multiplicity is deliberately NOT
    /// collapsed: two occurrences of an unstaffable step are twice the penalty.
    residual: Vec<(Step, u64)>,
    /// Pool worker id → role index (the v2t draw's map, indexed by agent id).
    /// Coverage is role-MATCHED, so the wrapper needs the same map the typed
    /// policy carries. Owned rather than borrowed because a
    /// `Box<dyn CoalitionDecisionPolicy>` is `'static`; the pool is ≤ 16 workers,
    /// so the per-task clone is noise next to one magnitude evaluation.
    roles: Vec<u8>,
}

impl P9ValuationPolicy {
    /// The `(role, capabilities)` of one participant, or `None` when the harness
    /// has no role for it — the same condition the library declines on, so the
    /// wrapper forwards the library's decision rather than inventing one.
    fn staff_of(&self, agent: &dyn AgentCapabilities) -> Option<(u8, u32)> {
        Some((
            *self.roles.get(agent.agent_id())?,
            agent.capabilities(),
        ))
    }

    fn staff(&self, members: &[&dyn AgentCapabilities]) -> Option<Vec<(u8, u32)>> {
        members.iter().map(|a| self.staff_of(*a)).collect()
    }

    /// `Σ per_gen(g)` over residual occurrences no member of `staff` can perform.
    fn uncovered(&self, staff: &[(u8, u32)]) -> u64 {
        self.residual
            .iter()
            .filter(|&&(step, _)| !p9_step_staffed(staff, step))
            .map(|&(_, price)| price)
            .sum()
    }

    /// The full residual at an EMPTY coalition — `λ ·` the price of the whole
    /// declared writing's step occurrences, i.e. the largest the term can be.
    /// Reported in the instrumentation section.
    fn full_term(&self) -> f64 {
        self.lambda * self.uncovered(&[]) as f64
    }

    /// Fold a non-negative residual correction into a base decision.
    ///
    /// A **zero** correction forwards the base decision VERBATIM, which is not
    /// merely an optimization: the library signals a decline (upstream error,
    /// non-finite margin, missing role) as `Decision { act: false, score: 0.0 }`,
    /// indistinguishable at this seam from a legitimate zero margin. Forwarding
    /// on zero keeps every decline exactly as the library made it. A decline
    /// coinciding with a NONZERO correction would still be read as a zero margin
    /// — an unavoidable ambiguity of the `Decision` surface, bounded here by the
    /// harness asserting full role-map coverage (`p8_typed_policy`), which is the
    /// only decline reachable on this world short of an upstream failure that
    /// would equally break the control arm.
    fn fold(base: Decision, correction: f64, act: impl Fn(f64) -> bool) -> Decision {
        if !base.score.is_finite() || correction == 0.0 {
            return base;
        }
        let score = base.score + correction;
        Decision {
            act: act(score),
            score,
        }
    }
}

/// Whether any member of `staff` can perform `step` — role-matched, the same
/// question [`p9_step_covered`] asks of a member index list.
fn p9_step_staffed(staff: &[(u8, u32)], step: Step) -> bool {
    step.capability_mask().is_some_and(|mask| {
        staff
            .iter()
            .any(|&(role, caps)| role == step.role.index() && caps & mask != 0)
    })
}

impl CoalitionDecisionPolicy for P9ValuationPolicy {
    /// `Δvalue = ΔMag + λ · (pen(S) − pen(S ∪ {x}))`.
    ///
    /// The correction is non-negative — admitting an agent can only cover more
    /// steps — so it can turn a declined join into an accepted one, never the
    /// reverse.
    fn should_join(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        ctx: &DecisionContext,
    ) -> Decision {
        let base = self.inner.should_join(agent, coalition, ctx);
        let (Some(without), Some(joined)) = (self.staff(coalition), self.staff_of(agent)) else {
            return base;
        };
        let mut with = without.clone();
        with.push(joined);
        // `saturating_sub` states the monotonicity rather than trusting it: a
        // larger staff can never leave more uncovered.
        let correction =
            self.lambda * self.uncovered(&without).saturating_sub(self.uncovered(&with)) as f64;
        // The library's own `margin > join_margin` rule, read off the very policy
        // being wrapped rather than mirrored as a constant — the score changed,
        // so the predicate has to be restated, but the threshold does not.
        let margin = self.inner.join_margin;
        Self::fold(base, correction, |score| score > margin)
    }

    /// `Δvalue = ΔMag + λ · (pen(S \ {x}) − pen(S))`, leaving iff `Δvalue ≤ 0`
    /// (the library's leave rule, restated over value instead of magnitude).
    ///
    /// The correction is again non-negative, so a member holding otherwise
    /// unstaffable steps becomes LESS likely to be swept out.
    fn should_leave(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        ctx: &DecisionContext,
    ) -> Decision {
        let base = self.inner.should_leave(agent, coalition, ctx);
        // `coalition` INCLUDES `agent` on the leave path (library convention).
        let id = agent.agent_id();
        let remaining: Vec<&dyn AgentCapabilities> = coalition
            .iter()
            .filter(|a| a.agent_id() != id)
            .copied()
            .collect();
        let (Some(inside), Some(outside)) = (self.staff(coalition), self.staff(&remaining)) else {
            return base;
        };
        let correction =
            self.lambda * self.uncovered(&outside).saturating_sub(self.uncovered(&inside)) as f64;
        Self::fold(base, correction, |score| score <= 0.0)
    }
}

/// How a Part 9 arm supplies its policy for a seed — the [`P8Arm`] shape, with
/// the per-task variant keyed by task index (the valuation term is a task
/// constant).
enum P9Arm<'a> {
    Fixed(&'a dyn CoalitionDecisionPolicy),
    PerTask(&'a dyn Fn(usize) -> Box<dyn CoalitionDecisionPolicy>),
}

/// Per-seed Part 9 result. `scores` carries raw `Decision::score` BIT PATTERNS,
/// for the same reason [`P8Seed`] does — the identity gates are bit-identity
/// claims and `f64` equality would not express them.
struct P9Seed {
    primary: f64,
    churn: usize,
    acts: Vec<bool>,
    scores: Vec<u64>,
    success_rate: f64,
    /// `cov_eff` per task, index-aligned with the task stream (E-conc reads it).
    quality: Vec<f64>,
    /// Whether the DECLARED demand was role-matched feasible in the pool (E-conc).
    declared_feasible: Vec<bool>,
    /// Tasks declined because `optimize` failed.
    declined: usize,
}

/// The per-task hook a Part 9 run fires after every leave sweep:
/// `(required, per-bit role-matched success over the declared demand)`.
type P9OutcomeHook<'a> = dyn FnMut(u32, &[bool]) + 'a;

/// [`p9_run_seed_hooked`] with no outcome hook — every arm but `arm-E1`.
fn p9_run_seed(
    arm: &P9Arm<'_>,
    inst: &WorkflowInstance,
    declared: &[P9Declared],
    lat: &mut Vec<f64>,
) -> P9Seed {
    p9_run_seed_hooked(arm, inst, declared, lat, &mut |_, _| {})
}

/// Run one arm over one v2w instance under the standing battery protocol
/// (bootstrap first arrival, one leave sweep per task in arrival order, every
/// removal counts as churn, `Instant` around every sync decision), scoring the
/// arm's DECLARED writing.
fn p9_run_seed_hooked(
    arm: &P9Arm<'_>,
    inst: &WorkflowInstance,
    declared: &[P9Declared],
    lat: &mut Vec<f64>,
    on_task: &mut P9OutcomeHook<'_>,
) -> P9Seed {
    let mut success_count = 0usize;
    let mut quality_sum = 0.0f64;
    let mut churn = 0usize;
    let mut acts = Vec::new();
    let mut scores = Vec::new();
    let mut quality = Vec::with_capacity(declared.len());
    let mut declared_feasible = Vec::with_capacity(declared.len());
    let mut declined = 0usize;

    for (t, (task, decl)) in inst.base.tasks.iter().zip(declared.iter()).enumerate() {
        if decl.failed {
            // Decline-and-count: no coalition forms, the task scores zero, and
            // nothing falls back to the as-written writing.
            declined += 1;
            quality.push(0.0);
            declared_feasible.push(false);
            continue;
        }

        let ctx = DecisionContext {
            required_capabilities: decl.required,
        };
        let per_task;
        let policy: &dyn CoalitionDecisionPolicy = match arm {
            P9Arm::Fixed(p) => *p,
            P9Arm::PerTask(make) => {
                per_task = make(t);
                &*per_task
            }
        };

        let mut members: Vec<usize> = vec![task.order[0]];

        for &idx in &task.order[1..] {
            let candidate: &dyn AgentCapabilities = &inst.base.agents[idx];
            let coalition = coalition_view(&inst.base.agents, &members);
            let t0 = Instant::now();
            let d = policy.should_join(candidate, &coalition, &ctx);
            lat.push(seconds_to_us(t0.elapsed()));
            acts.push(d.act);
            scores.push(d.score.to_bits());
            if d.act {
                members.push(idx);
            }
        }

        for &idx in &task.order {
            let Some(pos) = members.iter().position(|&m| m == idx) else {
                continue;
            };
            let coalition = coalition_view(&inst.base.agents, &members);
            let agent: &dyn AgentCapabilities = &inst.base.agents[idx];
            let t0 = Instant::now();
            let d = policy.should_leave(agent, &coalition, &ctx);
            lat.push(seconds_to_us(t0.elapsed()));
            acts.push(d.act);
            scores.push(d.score.to_bits());
            if d.act {
                members.remove(pos);
                churn += 1;
            }
        }

        // Coverage is per DISTINCT (bit, role): multiplicity prices the process,
        // it does not add coverage demand.
        let d_len = decl.demand.distinct_len();
        let covered = decl
            .demand
            .distinct()
            .filter(|&s| p9_step_covered(inst, &members, s))
            .count();
        // An empty declared demand is structurally unreachable (no registered
        // rule deletes a step outright), but it is scored as a non-completion
        // rather than a vacuous success so it can never hand out a free point.
        let completed = d_len > 0 && covered == d_len;
        let cov_eff = if members.is_empty() || d_len == 0 {
            0.0
        } else {
            (covered as f64 / d_len as f64) / members.len() as f64
        };
        if completed {
            success_count += 1;
        }
        quality_sum += cov_eff;
        quality.push(cov_eff);
        declared_feasible.push(p9_demand_feasible(inst, &decl.demand));

        // Role-matched per-bit signal over the DECLARED demand: bit `b` is
        // `true` iff some declared step on `b` is covered by a member of that
        // step's role. A bit demanded under two roles counts as satisfied when
        // either is staffed; `observe_outcome` ignores non-required bits.
        let mut per_bit = vec![false; UNIVERSE];
        for step in decl.demand.distinct() {
            let b = step.bit as usize;
            if b < UNIVERSE && p9_step_covered(inst, &members, step) {
                per_bit[b] = true;
            }
        }
        on_task(decl.required, &per_bit);
    }

    let n_tasks = inst.base.tasks.len() as f64;
    let success_rate = success_count as f64 / n_tasks;
    P9Seed {
        primary: success_rate * (quality_sum / n_tasks),
        churn,
        acts,
        scores,
        success_rate,
        quality,
        declared_feasible,
        declined,
    }
}

/// Run one fixed-policy arm over the shared instances against a cell's declared
/// writings. A discarded warm-up on the first instance runs first (the standing
/// convention); it cannot perturb the seed-derived results.
fn p9_battery<F>(
    insts: &[WorkflowInstance],
    declared: &[Vec<P9Declared>],
    make: F,
) -> (Vec<P9Seed>, Vec<f64>)
where
    F: Fn(&WorkflowInstance) -> Box<dyn CoalitionDecisionPolicy>,
{
    if let (Some(first), Some(first_decl)) = (insts.first(), declared.first()) {
        let p = make(first);
        let mut warm = Vec::new();
        let _ = p9_run_seed(&P9Arm::Fixed(&*p), first, first_decl, &mut warm);
    }
    let mut lat = Vec::new();
    let results = insts
        .iter()
        .zip(declared.iter())
        .map(|(inst, decl)| {
            let p = make(inst);
            p9_run_seed(&P9Arm::Fixed(&*p), inst, decl, &mut lat)
        })
        .collect();
    (results, lat)
}

/// The pool's role map as a `Vec<u8>` indexed by agent id — the same map
/// [`p8_typed_policy`] hands the library, in the shape the valuation wrapper's
/// role-matched coverage check wants.
///
/// # Panics
///
/// Panics if a pool worker carries no role or a role index does not fit a `u8`.
/// Both are harness bugs: `p8_typed_policy` already asserts full coverage, and
/// `R = 3`.
fn p9_role_map(inst: &WorkflowInstance) -> Vec<u8> {
    inst.base
        .roles
        .iter()
        .map(|&r| {
            u8::try_from(r).expect("invariant: the v2t draw assigns role indices below R = 3")
        })
        .collect()
}

/// The per-occurrence residual of one declared writing under a cost model: every
/// step occurrence paired with its `per_gen` price (Amendment A3.1).
///
/// Spiders never appear — [`Demand::occurrences`] carries `User` generators only,
/// which is exactly the exclusion A3.1 requires (a spider names no `(bit, role)`,
/// so it can never be uncovered).
fn p9_residual(inst: &WorkflowInstance, decl: &P9Declared, cost: P9Cost) -> Vec<(Step, u64)> {
    decl.demand
        .occurrences()
        .iter()
        .map(|&step| {
            let price = match cost {
                P9Cost::Uniform => 1,
                P9Cost::Priced => inst.table.price(step),
            };
            (step, price)
        })
        .collect()
}

/// The valuation-only battery: a per-task [`P9ValuationPolicy`] carrying that
/// task's residual over the shared typed control.
///
/// The third return is the per-task **full residual** `λ · Σ per_gen` at an empty
/// coalition — the largest the term can be, read off the policy the arm actually
/// carries so the reported figure cannot drift from the one in the score.
fn p9_valuation_battery(
    insts: &[WorkflowInstance],
    declared: &[Vec<P9Declared>],
    rho: &RoleModulation,
    lambda: f64,
    cost: P9Cost,
) -> (Vec<P9Seed>, Vec<f64>, Vec<f64>) {
    let mut terms: Vec<f64> = Vec::new();
    let mut lat = Vec::new();
    let mut results = Vec::with_capacity(insts.len());
    // Discarded warm-up on the first instance, as in `p9_battery` — the arms stay
    // latency-comparable even though latency is never gating here.
    if let (Some(first), Some(first_decl)) = (insts.first(), declared.first()) {
        let inner = p8_typed_policy(&first.base, rho);
        let role_map = p9_role_map(first);
        let make = |t: usize| {
            Box::new(P9ValuationPolicy {
                inner: inner.clone(),
                lambda,
                residual: p9_residual(first, &first_decl[t], cost),
                roles: role_map.clone(),
            }) as Box<dyn CoalitionDecisionPolicy>
        };
        let mut warm = Vec::new();
        let _ = p9_run_seed(&P9Arm::PerTask(&make), first, first_decl, &mut warm);
    }
    for (inst, decl) in insts.iter().zip(declared.iter()) {
        let inner = p8_typed_policy(&inst.base, rho);
        let role_map = p9_role_map(inst);
        let build = |t: usize| P9ValuationPolicy {
            inner: inner.clone(),
            lambda,
            residual: p9_residual(inst, &decl[t], cost),
            roles: role_map.clone(),
        };
        let make = |t: usize| Box::new(build(t)) as Box<dyn CoalitionDecisionPolicy>;
        terms.extend((0..decl.len()).map(|t| build(t).full_term()));
        results.push(p9_run_seed(&P9Arm::PerTask(&make), inst, decl, &mut lat));
    }
    (results, lat, terms)
}

/// The `arm-E1` context battery over the v2w world: a FRESH [`PersistentAifArm`]
/// per seed (the #44/#53 factory pattern), fed the role-matched per-bit outcome
/// over the DECLARED demand after every task. Warm-up on the first instance
/// discarded, as everywhere else.
fn p9_e1_battery(
    insts: &[WorkflowInstance],
    declared: &[Vec<P9Declared>],
    config: PersistentAifConfig,
) -> (Vec<P9Seed>, Vec<f64>) {
    if let (Some(first), Some(first_decl)) = (insts.first(), declared.first()) {
        let arm = PersistentAifArm::new(P9_SEED_START, config).expect("persistent arm construction");
        let mut warm = Vec::new();
        let _ = p9_run_seed_hooked(
            &P9Arm::Fixed(&arm),
            first,
            first_decl,
            &mut warm,
            &mut |req, bits| arm.observe_outcome(req, bits),
        );
    }
    let mut lat = Vec::new();
    let results = insts
        .iter()
        .zip(declared.iter())
        .enumerate()
        .map(|(i, (inst, decl))| {
            let seed = P9_SEED_START + i as u64;
            let arm = PersistentAifArm::new(seed, config).expect("persistent arm construction");
            p9_run_seed_hooked(&P9Arm::Fixed(&arm), inst, decl, &mut lat, &mut |req, bits| {
                arm.observe_outcome(req, bits);
            })
        })
        .collect();
    (results, lat)
}

fn p9_primaries(rs: &[P9Seed]) -> Vec<f64> {
    rs.iter().map(|r| r.primary).collect()
}
fn p9_churns(rs: &[P9Seed]) -> Vec<f64> {
    rs.iter().map(|r| r.churn as f64).collect()
}
fn p9_superior_count(a: &[P9Seed], b: &[P9Seed]) -> usize {
    (0..a.len().min(b.len()))
        .filter(|&i| a[i].primary > b[i].primary)
        .count()
}

/// One summary row of a Part 9 table: medians plus the paired contrast against
/// the `wf-asis` control.
fn p9_row(label: &str, rs: &[P9Seed], base: &[P9Seed], lat: &[f64]) {
    let med = median(p9_primaries(rs));
    let base_med = median(p9_primaries(base));
    let ratio = if base_med > 0.0 {
        format!("{:.2}×", med / base_med)
    } else {
        "n/a".to_owned()
    };
    println!(
        "| `{label}` | {med:.4} | {ratio} | {}/{} | {:.2} | {:.3} |",
        p9_superior_count(rs, base),
        rs.len(),
        median(p9_churns(rs)),
        median(lat.to_vec())
    );
}

fn p9_table_head() {
    println!(
        "| arm | median PRIMARY | vs `wf-asis` | superior seeds | median churn | median µs/decision |"
    );
    println!(
        "|-----|---------------:|-------------:|---------------:|-------------:|-------------------:|"
    );
}

/// Assert one cell is identical to another on **acts + PRIMARY bits + churn** —
/// the registered X-reduce triple. Raw score bits are deliberately excluded: the
/// gate is on DECISIONS, not float bit patterns (the EQ3 H-par′ lesson).
fn p9_assert_identical(a: &[P9Seed], b: &[P9Seed], what: &str) {
    for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
        let seed = P9_SEED_START + i as u64;
        assert_eq!(x.acts, y.acts, "{what}: acts on seed {seed}");
        assert_eq!(
            x.primary.to_bits(),
            y.primary.to_bits(),
            "{what}: PRIMARY bits on seed {seed}"
        );
        assert_eq!(x.churn, y.churn, "{what}: churn on seed {seed}");
    }
}

/// [`p9_assert_identical`] plus raw score BIT PATTERNS.
///
/// Only claimed where identity is exact by construction rather than by
/// reduction: the valuation wrapper forwards to the very same policy instance on
/// the very same declared writing, so every `Decision::score` must reproduce bit
/// for bit — nothing re-associates. Returns whether it held, so the run can print
/// the finding as a measurement.
fn p9_bit_identical(a: &[P9Seed], b: &[P9Seed]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b.iter()).all(|(x, y)| {
            x.acts == y.acts
                && x.scores == y.scores
                && x.primary.to_bits() == y.primary.to_bits()
                && x.churn == y.churn
        })
}

/// How far a cell diverges from another: `(seeds with any differing act,
/// decisions with a differing act, decisions with differing raw score BITS)`.
///
/// The A3.1 liveness measurement. A valuation cell that scored identically to the
/// control on every decision would be the exact defect Amendment A3.1 removed —
/// a term that cancels out of every margin — so this is reported as a number and
/// asserted, never assumed.
fn p9_divergence(cell: &[P9Seed], base: &[P9Seed]) -> (usize, usize, usize) {
    let mut seeds = 0usize;
    let mut acts = 0usize;
    let mut scores = 0usize;
    for (x, y) in cell.iter().zip(base.iter()) {
        let act_diff = x
            .acts
            .iter()
            .zip(y.acts.iter())
            .filter(|(a, b)| a != b)
            .count()
            + x.acts.len().abs_diff(y.acts.len());
        if act_diff > 0 {
            seeds += 1;
        }
        acts += act_diff;
        scores += x
            .scores
            .iter()
            .zip(y.scores.iter())
            .filter(|(a, b)| a != b)
            .count()
            + x.scores.len().abs_diff(y.scores.len());
    }
    (seeds, acts, scores)
}

// ---------------------------------------------------------------------------
// The pinned rule theory, printed in full (prereg §4 / §5 instrumentation).
// ---------------------------------------------------------------------------

/// Print every instance of the pinned schema closure, so a trace's rule index
/// reports as the rule that fired (Amendment A2.1: §4's "3–5 rules" numeral is
/// void — the count "3–5" refers to SCHEMAS, and the closure governs; Amendment
/// A3.2 widens the fusion schema to 174 instances in total).
///
/// The consumed steps come from [`LabelledRule::sources`], not from the role:
/// since A3.2 the fusion instances no longer follow from the role alone, so a
/// printer that re-derived them would be printing a different theory than the one
/// the trace indexes into.
fn p9_print_rule_theory(labels: &[LabelledRule]) {
    println!("| # | schema | rule |");
    println!("|--:|--------|------|");
    for (i, label) in labels.iter().enumerate() {
        let t = label.target;
        let r = t.role.index();
        let [first, second] = label.sources;
        let rule = match label.schema {
            Schema::Idempotence => format!("`s{b}_r{r} ; s{b}_r{r} ⇒ s{b}_r{r}`", b = t.bit),
            Schema::Fusion => format!(
                "`s{b1}_r{r} ; s{b2}_r{r} ⇒ s{b3}_r{r}`",
                b1 = first.bit,
                b2 = second.bit,
                b3 = t.bit
            ),
            Schema::SpiderAbsorption => format!(
                "`delta@r{r} ; (s{b}_r{r} ⊗ s{b}_r{r}) ; mu@r{r} ⇒ s{b}_r{r}`",
                b = t.bit
            ),
        };
        println!("| {i} | {:?} | {rule} |", label.schema);
    }
}

// ---------------------------------------------------------------------------
// E-dedup / S-dedup — the content-vs-writing corpus facts (prereg §5, §6).
// ---------------------------------------------------------------------------

/// What the dedup pass measured over the drawn corpus.
struct P9DedupStats {
    /// Workflows in the corpus.
    corpus: usize,
    /// Distinct as WRITTEN (expression tree + pinned source word).
    writings: usize,
    /// Distinct as CONTENT (`canonical_key` buckets).
    contents: usize,
    /// Bucket sizes, descending.
    buckets: Vec<usize>,
    /// Arrivals that are distinct as written but equal as content.
    collapsed: usize,
    /// S-dedup: pairs where `canonical_key` equality and `content_eq` disagree.
    /// Any nonzero value is RUN-INVALID.
    biconditional_violations: usize,
    /// Pairs compared.
    pairs: usize,
    /// Workflows whose MONO key (`content_of` on the bare expression) differs
    /// from their COLORED key (`content_of_colored`) — the cg like-with-like
    /// seam, measured rather than assumed.
    mono_differs: usize,
}

/// Run the dedup pass over every as-written workflow of the corpus.
///
/// **One content entry point per table**: every key below comes from
/// `content_of_colored`. `content_of` appears exactly once, in the `mono_differs`
/// measurement, and never feeds a table — mixing the two would silently split or
/// merge buckets on any wire no generator touches (cg seam note).
///
/// `ContentKey` is deliberately NOT `Ord`, so the tables are `HashMap`s and every
/// reported figure is order-free: counts, and a bucket-size histogram sorted by
/// SIZE (a `usize`), never by key.
fn p9_dedup_pass(corpus: &[&Workflow]) -> P9DedupStats {
    let contents: Vec<_> = corpus.iter().map(|w| content_of_colored(w)).collect();

    let mut by_writing: HashSet<String> = HashSet::new();
    let mut by_content: HashMap<ContentKey<WorkflowGen>, usize> = HashMap::new();
    let mut mono_differs = 0usize;
    for (w, c) in corpus.iter().zip(contents.iter()) {
        by_writing.insert(format!(
            "{:?}|{}",
            w.source_word(),
            catgraph_syntax::text::print(w.expr())
        ));
        *by_content.entry(canonical_key(c)).or_insert(0) += 1;
        if canonical_key(&content_of(w.expr())) != canonical_key(c) {
            mono_differs += 1;
        }
    }

    let mut buckets: Vec<usize> = by_content.values().copied().collect();
    buckets.sort_unstable_by(|a, b| b.cmp(a));

    // S-dedup: `canonical_key(a) == canonical_key(b)` iff `content_eq(a, b)`,
    // over every unordered pair of the corpus.
    let keys: Vec<_> = contents.iter().map(canonical_key).collect();
    let mut biconditional_violations = 0usize;
    let mut pairs = 0usize;
    for i in 0..contents.len() {
        for j in (i + 1)..contents.len() {
            pairs += 1;
            if (keys[i] == keys[j]) != content_eq(&contents[i], &contents[j]) {
                biconditional_violations += 1;
            }
        }
    }

    P9DedupStats {
        corpus: corpus.len(),
        writings: by_writing.len(),
        contents: by_content.len(),
        collapsed: by_writing.len().saturating_sub(by_content.len()),
        buckets,
        biconditional_violations,
        pairs,
        mono_differs,
    }
}

// ---------------------------------------------------------------------------
// E-ceil (ii) — the harness-side minimum-distinct-demand reference.
// ---------------------------------------------------------------------------

/// The smallest DISTINCT demand reachable for one task through a pinned family
/// of high-fuel `optimize` objectives.
///
/// prereg §5 registers "a harness-side brute-force minimum-distinct-demand search
/// over the class `optimize` enumerates at high fuel". `optimize` returns only
/// `best()` — the visited set is private — so the class it enumerates is not
/// directly readable through the public API. What runs instead is a pinned
/// multi-objective sweep over that class: the uniform and priced optima, plus one
/// optimum per distinct as-written step under an objective that makes THAT step
/// expensive (`P9_ECEIL_HEAVY`, others 1), which is how a per-generator weight
/// can ask the search to route around a specific `(bit, role)`. The minimum over
/// those optima is a **reference within the rewriting family and an upper bound
/// on the true minimum** — never a supremum (the #72 A2.5 correction), and
/// disclosed as such.
fn p9_eceil_min_distinct(
    inst: &WorkflowInstance,
    written: &Workflow,
    rules: &[RewriteRule<WorkflowGen>],
) -> usize {
    let as_written = demand(written);
    let mut best = as_written.distinct_len();
    let mut consider = |outcome: Result<_, CatgraphError>| {
        if let Ok(o) = outcome {
            let o: catgraph_applied::prop::presentation::rewrite::RewriteOutcome<WorkflowGen> = o;
            best = best.min(demand(o.best()).distinct_len());
        }
    };
    consider(optimize_workflow(
        written,
        rules,
        P9_ECEIL_FUEL,
        uniform_cost(),
    ));
    consider(optimize_workflow(
        written,
        rules,
        P9_ECEIL_FUEL,
        staffing_price(&inst.table),
    ));
    for target in as_written.distinct() {
        consider(optimize_workflow(written, rules, P9_ECEIL_FUEL, |g| {
            match g {
                FrobeniusOr::User(s) if *s == target => P9_ECEIL_HEAVY,
                _ => 1,
            }
        }));
    }
    best
}

#[allow(clippy::too_many_lines)]
fn part9_eq5a_process_structured() {
    println!("# koalisi #76 — Part 9: EQ5a process-structured battery (REGISTERED)");
    println!();
    println!(
        "_governed by `docs/prereg-K4-eq5a-process-structured.md` (registered BEFORE this code; owner design-lock D1–D11 on #76, Amendment 1 pinning the rule theory / fuel / staffing price / λ / draw parameters, Amendment 2 voiding §4's \"3–5 rules\" numeral in favour of the schema closure, **Amendment 3** re-reading valuation-only as the unstaffable residual (A3.1) and widening the fusion schema to every same-role ordered pair (A3.2)). Report date {P9_REPORT_DATE}. World **v2w** (prereg §2): the Part 8 `v2t` prefix VERBATIM — `draw_prefix_v2`, then `R = {P8_ROLES}` worker roles and per-required-bit role tags with the role-feasibility rejection re-draw — and the workflow SHAPE draw APPENDED off the same SplitMix64 stream. Each tagged required bit `(b, r)` is a step generator `s_{{b,r}} : r → r`; steps chain per role (`Free::compose`), roles combine with `Free::tensor`, and every diagram is pinned through `ColoredExpr::new` (Amendment A2.3 — `Free::compose` checks WIDTHS, not colors, so the pin is the only color gate). Seeds **{P9_SEED_START}..{P9_SEED_END}** (fresh; 90..120 and 150..180 stay reserved). Latency is recorded and NEVER gating (prereg §7)._"
    );
    println!();
    println!(
        "_**The declared-writing mechanic** (prereg §4 fairness clause): each arm declares the writing it staffs — the control declares the as-written workflow, a rewriting arm declares `optimize(...).best()` — and the scorer scores each arm's DECLARED writing. Every declared writing that is not the as-written one is `replay`-verified against the registered rules and `content_eq`-checked against the reported representative BEFORE it is scored (§6 S-sound), so an arm cannot grade its own homework and the control is not penalised for a legitimate alternative it did not take. **Consequence, stated because it is by design:** `cov_eff` denominators differ across arms — when both complete, the ratio term is `1.0` for both and the contrast comes through **member count**, which is the claimed advantage. The policy itself only ever sees a `required: u32` mask (the OR over the declared writing's distinct demand) plus the worker role map; the arm never sees a tag, exactly as in EQ4._"
    );
    println!();
    println!(
        "_**Coverage is per DISTINCT `(bit, role)` pair** — multiplicity prices the process, it does not add coverage demand. So if one bit is demanded under two roles the OR-mask the policy sees carries it once while the scorer counts two demands; that asymmetry is what the rewrite theory acts on (prereg §2)._"
    );
    println!();

    // --- The pinned theory ---------------------------------------------------
    let bits = u8::try_from(UNIVERSE).expect("invariant: the universe is 8 bits");
    let roles = u8::try_from(P8_ROLES).expect("invariant: R = 3");
    let rules = rule_theory(bits, roles).expect("invariant: the registered (8, 3) theory constructs");
    let labels = rule_labels(bits, roles).expect("invariant: labels mirror the theory");
    assert_eq!(rules.len(), labels.len());
    let pairs = fusion_pairs(bits);
    let fusion_instances = pairs.len() * P8_ROLES;
    assert_eq!(
        rules.len(),
        2 * UNIVERSE * P8_ROLES + fusion_instances,
        "the printed theory size must be the schema closure's, not a restated constant"
    );

    println!("## The pinned rule theory (Amendment A1.1, widened by A3.2 — printed in full)");
    println!();
    println!(
        "_Three SCHEMAS closed over the `(bit, role)` index set; at the registered `bits = {UNIVERSE}, roles = {P8_ROLES}` the closure is **{} instances** ({} idempotence + {fusion_instances} fusion + {} spider absorption). Amendment A2.1: §4's \"3–5 rules\" is an ERRATUM — the numeral refers to schemas and is void; the closure governs, and every instance constructs through `RewriteRule::new` so nothing is silently dropped. **Amendment A3.2** widened fusion from one designated pair per role to every role × every ordered pair of distinct bits `(b, b')`, target `b'' = (b + b' + 4) mod bits`, the instance NOT built when `b'' ∈ {{b, b'}}` — at `bits = 8` that skip excludes exactly the pairs involving bit 4, leaving {} per role. The skip is the two-sidedness guarantee A1.1 stated as a global condition, now enforced per instance: had a target been a consumed bit, every application would strictly shrink distinct demand and the rewriting cells would win BY CONSTRUCTION. A trace binds rule INDICES, so this order is part of the registered theory._",
        rules.len(),
        UNIVERSE * P8_ROLES,
        UNIVERSE * P8_ROLES,
        pairs.len()
    );
    println!();
    p9_print_rule_theory(&labels);
    println!();

    // --- Instances (shared by every arm) ------------------------------------
    let insts = p9_instances(false);
    let n_seeds = insts.len();
    let oracle = p8_rho(0.0);

    // --- Declared writings, one set per cell ---------------------------------
    let (d_ctl, s_ctl) = p9_declare(
        &insts,
        &rules,
        &labels,
        P9Mechanism::AsWritten,
        P9Cost::Uniform,
        P9_FUEL,
    );
    // Wall time of the two confirmatory declares — the A3.2 (ii) search-cost
    // disclosure reports them next to the full fuel sweep.
    let t0 = Instant::now();
    let (d_rw_u, s_rw_u) = p9_declare(
        &insts,
        &rules,
        &labels,
        P9Mechanism::Rewrite,
        P9Cost::Uniform,
        P9_FUEL,
    );
    let confirm_wall_u = t0.elapsed().as_secs_f64();
    let t0 = Instant::now();
    let (d_rw_p, s_rw_p) = p9_declare(
        &insts,
        &rules,
        &labels,
        P9Mechanism::Rewrite,
        P9Cost::Priced,
        P9_FUEL,
    );
    let confirm_wall_p = t0.elapsed().as_secs_f64();
    let (d_val_u, s_val_u) = p9_declare(
        &insts,
        &rules,
        &labels,
        P9Mechanism::Valuation,
        P9Cost::Uniform,
        P9_FUEL,
    );
    let (d_val_p, s_val_p) = p9_declare(
        &insts,
        &rules,
        &labels,
        P9Mechanism::Valuation,
        P9Cost::Priced,
        P9_FUEL,
    );

    // --- The registered arms -------------------------------------------------
    let (asis, asis_lat) = p9_battery(&insts, &d_ctl, |inst| {
        Box::new(p8_typed_policy(&inst.base, &oracle)) as Box<dyn CoalitionDecisionPolicy>
    });
    let (rw_u, rw_u_lat) = p9_battery(&insts, &d_rw_u, |inst| {
        Box::new(p8_typed_policy(&inst.base, &oracle)) as Box<dyn CoalitionDecisionPolicy>
    });
    let (rw_p, rw_p_lat) = p9_battery(&insts, &d_rw_p, |inst| {
        Box::new(p8_typed_policy(&inst.base, &oracle)) as Box<dyn CoalitionDecisionPolicy>
    });
    let (val_u, val_u_lat, val_u_terms) =
        p9_valuation_battery(&insts, &d_val_u, &oracle, P9_LAMBDA, P9Cost::Uniform);
    let (val_p, val_p_lat, val_p_terms) =
        p9_valuation_battery(&insts, &d_val_p, &oracle, P9_LAMBDA, P9Cost::Priced);
    let (mag, mag_lat) = p9_battery(&insts, &d_ctl, |_| {
        Box::new(MagnitudePolicy::default()) as Box<dyn CoalitionDecisionPolicy>
    });
    let (scalar, scalar_lat) = p9_battery(&insts, &d_ctl, |_| {
        Box::new(AifDecisionPolicy::default()) as Box<dyn CoalitionDecisionPolicy>
    });
    let (e1, e1_lat) = p9_e1_battery(&insts, &d_ctl, e1_config());

    // --- §6 gates (run and asserted BEFORE any leg is read) ------------------
    println!("## Gates (prereg §6 — any failure ⇒ RUN-INVALID)");
    println!();

    // S-sound.
    let sound_cells = [
        ("wf-rw-u", &s_rw_u),
        ("wf-rw-p", &s_rw_p),
    ];
    let mut sound_ok = true;
    let mut verified_total = 0usize;
    for (label, s) in sound_cells {
        sound_ok &= s.unsound == 0;
        verified_total += s.verified;
        assert_eq!(
            s.unsound, 0,
            "S-sound: {label} declared {} writing(s) its own trace does not derive",
            s.unsound
        );
    }
    println!(
        "- **S-sound — {}.** Every declared writing of the two rewriting cells `replay`s under the registered {} rules and the replayed content `content_eq`s the reported representative: **{verified_total}** verified ({n_seeds} seeds × {TASKS} tasks × 2), **0** unsound. This is strictly stronger than the registration asks — the writings the search left content-equal to the as-written one are verified too, rather than exempted on the grounds that nothing changed. The as-written control and the valuation cells declare the writing the world drew, so no verification is owed there (prereg §4). Amendment A2.4: `RewriteStep` has no public constructor, so the negative direction is exercised upstream and by the library's own tamper tests (empty rules slice, mismatched `start`, reordered rules) rather than by a forged step here.",
        pass(sound_ok),
        rules.len()
    );

    // S-dedup, over the drawn (as-written) corpus.
    let as_written_corpus: Vec<&Workflow> = insts.iter().flat_map(|i| i.written.iter()).collect();
    let dedup = p9_dedup_pass(&as_written_corpus);
    let dedup_ok = dedup.biconditional_violations == 0;
    assert!(
        dedup_ok,
        "S-dedup: canonical_key equality and content_eq disagreed on {} pair(s)",
        dedup.biconditional_violations
    );
    println!(
        "- **S-dedup — {}.** `canonical_key(a) == canonical_key(b)` ⟺ `content_eq(a, b)` on all **{}** unordered pairs of the **{}**-workflow drawn corpus: **{}** disagreements. Like-with-like holds by construction — every key in every table comes from `content_of_colored`, and `content_of` appears exactly once, in the mono-vs-colored measurement below, where it feeds no table. `ContentKey` is deliberately NOT `Ord`, so the tables are hash-keyed and every reported figure is order-free (counts, and a histogram sorted by SIZE, never by key).",
        pass(dedup_ok),
        dedup.pairs,
        dedup.corpus,
        dedup.biconditional_violations
    );
    println!(
        "  - Mono-vs-colored seam, MEASURED not assumed: **{}** of {} workflows have `canonical_key(content_of(expr)) != canonical_key(content_of_colored(w))`. Mixing the two entry points in one table would split or merge exactly those buckets.",
        dedup.mono_differs, dedup.corpus
    );

    // X-reduce.
    let deg = p9_instances(true);
    let (deg_ctl, _) = p9_declare(
        &deg,
        &rules,
        &labels,
        P9Mechanism::AsWritten,
        P9Cost::Uniform,
        P9_FUEL,
    );
    let (deg_asis, _) = p9_battery(&deg, &deg_ctl, |inst| {
        Box::new(p8_typed_policy(&inst.base, &oracle)) as Box<dyn CoalitionDecisionPolicy>
    });
    // The EQ4 typed arm on the SAME seeds, through the frozen Part 8 runner.
    let eq4_ref: Vec<P8Seed> = {
        let mut lat = Vec::new();
        deg.iter()
            .map(|inst| {
                let p = p8_typed_policy(&inst.base, &oracle);
                p8_run_seed(
                    &P8Arm::Fixed(&p),
                    &inst.base,
                    P8Metric::Typed,
                    &mut lat,
                    &mut |_, _, _, _| {},
                )
            })
            .collect()
    };
    for (i, (a, b)) in deg_asis.iter().zip(eq4_ref.iter()).enumerate() {
        let seed = P9_SEED_START + i as u64;
        assert_eq!(a.acts, b.acts, "X-reduce: acts vs the EQ4 typed arm, seed {seed}");
        assert_eq!(
            a.primary.to_bits(),
            b.primary.to_bits(),
            "X-reduce: PRIMARY bits vs the EQ4 typed arm, seed {seed}"
        );
        assert_eq!(
            a.churn, b.churn,
            "X-reduce: churn vs the EQ4 typed arm, seed {seed}"
        );
    }
    // The rewriting cells with an EMPTY rule set must reproduce `wf-asis`.
    let (deg_rw_u, _) = p9_declare(
        &deg,
        &[],
        &labels,
        P9Mechanism::Rewrite,
        P9Cost::Uniform,
        P9_FUEL,
    );
    let (deg_rw_p, _) = p9_declare(
        &deg,
        &[],
        &labels,
        P9Mechanism::Rewrite,
        P9Cost::Priced,
        P9_FUEL,
    );
    let (deg_rw_u_rs, _) = p9_battery(&deg, &deg_rw_u, |inst| {
        Box::new(p8_typed_policy(&inst.base, &oracle)) as Box<dyn CoalitionDecisionPolicy>
    });
    let (deg_rw_p_rs, _) = p9_battery(&deg, &deg_rw_p, |inst| {
        Box::new(p8_typed_policy(&inst.base, &oracle)) as Box<dyn CoalitionDecisionPolicy>
    });
    p9_assert_identical(&deg_rw_u_rs, &deg_asis, "X-reduce: empty-rule `wf-rw-u`");
    p9_assert_identical(&deg_rw_p_rs, &deg_asis, "X-reduce: empty-rule `wf-rw-p`");
    println!(
        "- **X-reduce — {}.** On the degenerate world (every task the all-parallel `tensor` of its distinct steps, fan-out 0, and — load-bearing — **zero stream draws**, so its instances are bit-for-bit the v2t instances of the same seed), `wf-asis` reproduces the EQ4 typed arm's **acts + per-seed PRIMARY (bit-identical) + churn** on all {n_seeds} seeds, and both rewriting cells with an EMPTY rule set reproduce `wf-asis` bit-identically on the same three. Raw score bits are NOT compared: the gate is on DECISIONS, not float bit patterns (the EQ3 H-par′ lesson).",
        pass(true)
    );
    println!(
        "- **X-battery** (frozen Parts 1–8 byte-identical on every quality/ratio/superiority/churn/verdict line) is checked OUTSIDE this binary, by diffing this run's Parts 1–8 against a pre-change baseline; latency-only diffs are the standing exclusion."
    );
    println!();

    // --- Draw, shape, and feasibility ---------------------------------------
    let total_redraws: usize = insts.iter().map(|i| i.base.redraws).sum();
    let worst_attempts = insts.iter().map(|i| i.base.max_attempts).max().unwrap_or(0);
    let occurrences: Vec<f64> = insts
        .iter()
        .flat_map(|i| i.written.iter().map(|w| demand(w).total() as f64))
        .collect();
    let distincts: Vec<f64> = insts
        .iter()
        .flat_map(|i| i.written.iter().map(|w| demand(w).distinct_len() as f64))
        .collect();
    let fanned = occurrences
        .iter()
        .zip(distincts.iter())
        .filter(|(o, d)| o > d)
        .count();
    // The as-written demand is re-checked for role-matched feasibility: it must
    // hold by construction (the shape draw repeats steps, it never introduces a
    // new `(bit, role)`), and a violation is RUN-INVALID.
    let mut as_written_infeasible = 0usize;
    for inst in &insts {
        for w in &inst.written {
            if !p9_demand_feasible(inst, &demand(w)) {
                as_written_infeasible += 1;
            }
        }
    }
    assert_eq!(
        as_written_infeasible, 0,
        "RUN-INVALID: an as-written demand is role-matched infeasible (prereg §2)"
    );
    println!("## Draw, shape, and feasibility (prereg §2, Amendment A1.5)");
    println!();
    println!(
        "- **Shape.** Per-role chain length is that role's tagged-bit count (A1.5 — no free parameter); the spider fan-out probability is **0.25** per same-role ADJACENT pair, drawn as `next_u64() % {P9_FANOUT_DENOM} == 0` (one draw per pair, exactly a quarter of the range, no float comparison inside a seeded draw). Over the {} tasks: median step **occurrences** {:.1}, median **distinct** `(bit, role)` {:.1}, and **{fanned}** tasks ({:.1} %) carry at least one fan-out.",
        occurrences.len(),
        median(occurrences.clone()),
        median(distincts.clone()),
        100.0 * fanned as f64 / occurrences.len() as f64
    );
    println!(
        "- **Fan-out SHAPE (disclosure — a draw choice the registration did not spell out).** A firing pair writes the later step as `δ_r ; (s ⊗ s) ; μ_r`, which is EXACTLY the left-hand side of the pinned spider-absorption schema. prereg §2 phrases fan-out as \"a step may feed two same-role successors\"; a generic split feeding two DIFFERENT successors would leave the absorption schema dead on drawn traffic — {} of the theory's {} instances would never match anything and the `catgraph-syntax` dependency A1.1 justifies would be decorative. The reachable shape is used so the registered schema is testable.",
        UNIVERSE * P8_ROLES,
        rules.len()
    );
    let fusion_eligible: usize = insts
        .iter()
        .map(|i| {
            i.base
                .tasks
                .iter()
                .filter(|t| p9_fusion_eligible(&pairs, t))
                .count()
        })
        .sum();
    let narrow_rate = 100.0 * P9_NARROW_ELIGIBLE_PRIOR as f64 / P9_NARROW_ELIGIBLE_TOTAL_PRIOR as f64;
    println!(
        "- **Fusion eligibility — the structural ceiling on the confirmatory lever (MANDATORY disclosure, Amendment A3.2).** Coverage is per DISTINCT `(bit, role)`, so idempotence and spider absorption — {} of the theory's {} instances — change OCCURRENCE count only and are staffing-INVISIBLE by construction: a task they alone touch scores bit-identically to the control. **Fusion is the only schema that can move a decision.** Under the WIDENED schema (every role × every ordered pair of distinct bits, target `(b + b' + 4) mod bits ∉ {{b, b'}}`) a task is eligible iff some role's tagged bits contain both members of one surviving pair: **{fusion_eligible}** of {} tasks (**{:.1} %**). Under the NARROW Amendment 1 schema (one designated pair per role) the recorded Stage-2 measurement on this same seed block was **{P9_NARROW_ELIGIBLE_PRIOR}/{P9_NARROW_ELIGIBLE_TOTAL_PRIOR} ({narrow_rate:.1} %)** — quoted as a recorded prior, since the narrow schema no longer exists in the theory and this run cannot re-measure it. That comparison is the evidence the leg is now powered: a stream-level bar over a stream where ~93 % of tasks were identical across arms was not reachable on merit. The eligibility figure is a NECESSARY condition and so an upper bound — the two steps must also become adjacent for the rule to match convexly — and every H-P number below should be read against it.",
        2 * UNIVERSE * P8_ROLES,
        rules.len(),
        occurrences.len(),
        100.0 * fusion_eligible as f64 / occurrences.len() as f64
    );
    println!(
        "- **Feasibility (gotcha 25 / #63).** The v2t prefix carries the registered role-matched rejection re-draw ({total_redraws} re-draws over the {n_seeds} seeds; worst single task **{worst_attempts}** of the {P8_REDRAW_CAP}-attempt budget). The AS-WRITTEN demand is then re-checked in full: **{as_written_infeasible}** infeasible, which is structural — the shape draw repeats and fans steps but never introduces a new `(bit, role)`, so as-written feasibility is exactly the v2t feasibility the prefix already guarantees. **The OPTIMIZED demand is NOT re-drawn** (prereg §2): an optimizer that concentrates demand on an absent `(bit, role)` is the registered failure mode, counted in E-conc below and never a draw condition."
    );
    println!();

    // --- Arms ----------------------------------------------------------------
    println!("## Arms (pooled over {n_seeds} seeds, v2w world)");
    println!();
    p9_table_head();
    p9_row("wf-asis", &asis, &asis, &asis_lat);
    p9_row("wf-rw-u", &rw_u, &asis, &rw_u_lat);
    p9_row("wf-rw-p", &rw_p, &asis, &rw_p_lat);
    p9_row("wf-val-u", &val_u, &asis, &val_u_lat);
    p9_row("wf-val-p", &val_p, &asis, &val_p_lat);
    p9_row("mag", &mag, &asis, &mag_lat);
    p9_row("scalar", &scalar, &asis, &scalar_lat);
    p9_row("arm-E1", &e1, &asis, &e1_lat);
    println!();
    println!(
        "_`wf-asis` is the control: the **EQ4-validated typed arm** (`with_role_modulation`, oracle `ρ = δ`) staffing the workflow as written. EQ5a therefore measures process signal BEYOND role signal and cannot re-harvest EQ4's margin (prereg §1). `mag` (frozen untyped `MagnitudePolicy` over the flattened demand), `scalar` and `arm-E1` are context, non-gating (prereg §3); all three staff the as-written writing._"
    );
    println!();

    // --- A3.1: the valuation-only cells are LIVE (measured, not assumed) ------
    let (val_u_seeds, val_u_acts, val_u_scores) = p9_divergence(&val_u, &asis);
    let (val_p_seeds, val_p_acts, val_p_scores) = p9_divergence(&val_p, &asis);
    let val_live = !p9_bit_identical(&val_u, &asis) || !p9_bit_identical(&val_p, &asis);
    println!("### A3.1 liveness: the valuation-only cells price the UNSTAFFABLE RESIDUAL");
    println!();
    println!(
        "_prereg §4 D3b originally scored `value(S) = Mag(S) − λ · cost_of(writing, per_gen)` while ALSO fixing the declared writing to be independent of `S`. That term is a per-task CONSTANT, so it cancelled exactly from every join/leave margin, and Stage 2 measured both valuation cells bit-identical to the control at every registered λ — two of the four confirmatory cells could not move. **Amendment A3.1** re-reads the mechanism as the residual process the coalition cannot execute:_"
    );
    println!();
    println!("```");
    println!(
        "value(S) = Mag(S) − λ · Σ per_gen(g)   over step occurrences g of the declared"
    );
    println!(
        "                         writing whose (bit, role) is NOT covered by S"
    );
    println!("```");
    println!();
    println!(
        "_This depends on `S`, so it no longer cancels: admitting an agent that covers previously-unstaffable steps improves the score by `λ` times those steps' price, and a member holding otherwise-unstaffable steps becomes less likely to be swept out. It is not a rescaling of the magnitude signal either — the penalty is weighted by `per_gen`, so **occurrence multiplicity and step scarcity enter a decision for the first time**, neither of which magnitude can see (it gets only the OR-mask of distinct demand). **Spiders are excluded** from the residual: they are priced by `cost_of` but name no `(bit, role)`, so \"uncovered\" is undefined for them and counting them would re-introduce exactly the cancelling constant A3.1 removed. Demand and the declared writing are unchanged — this is still valuation-only, not rewriting. λ stays {P9_LAMBDA} with the exploratory grid `{P9_LAMBDA_GRID:?}`._"
    );
    println!();
    println!(
        "- **Liveness, MEASURED against the control** (the regression the original formulation would have failed silently): `wf-val-u` diverges on **{val_u_seeds}/{n_seeds}** seeds, **{val_u_acts}** decisions by ACT and **{val_u_scores}** by raw score bits; `wf-val-p` on **{val_p_seeds}/{n_seeds}** seeds, **{val_p_acts}** acts and **{val_p_scores}** score bits. Cell is live ⇒ **{}**.",
        pass(val_live)
    );
    assert!(
        val_live,
        "A3.1: the valuation-only cells must not be bit-identical to the control — a cancelling term is the defect the amendment removed"
    );
    println!();
    // The registered λ grid, RUN: under A3.1 the term no longer cancels, so the
    // grid is a genuine sensitivity sweep rather than three cells a proof says
    // must coincide.
    let grid_row = P9_LAMBDA_GRID
        .iter()
        .map(|&lambda| {
            let (cell, _, terms) =
                p9_valuation_battery(&insts, &d_val_p, &oracle, lambda, P9Cost::Priced);
            let (seeds, acts, _) = p9_divergence(&cell, &asis);
            format!(
                "λ = {lambda}: median PRIMARY {:.4}, median full term {:.3}, diverging {seeds}/{n_seeds} seeds ({acts} acts)",
                median(p9_primaries(&cell)),
                median(terms)
            )
        })
        .collect::<Vec<String>>()
        .join(" · ");
    println!(
        "- **λ grid** `{P9_LAMBDA_GRID:?}` (§5, exploratory and non-gating), on the priced cell — {grid_row}. A1.4 pinned λ so the process term would be \"a tiebreaker between otherwise close candidates rather than the dominant term\" against O(1) magnitude margins; the sweep is the check on that reasoning."
    );
    println!();

    // --- H-P -----------------------------------------------------------------
    let asis_med = median(p9_primaries(&asis));
    let bar = P9_HP_FACTOR * asis_med;
    let cells: [(&str, &Vec<P9Seed>); 4] = [
        ("wf-rw-u", &rw_u),
        ("wf-rw-p", &rw_p),
        ("wf-val-u", &val_u),
        ("wf-val-p", &val_p),
    ];

    println!("## H-P (confirmatory, family-wise) — optimizing the process before staffing it");
    println!();
    println!(
        "_Four cells against `wf-asis`, seeds {P9_SEED_START}..{P9_SEED_END}: PRIMARY median ≥ **{P9_HP_FACTOR}×** the control's AND strictly superior on ≥ **{P9_HP_SUPERIOR_MIN}/{n_seeds}** seeds (70 %). Both conjuncts, in the SAME cell. The bar is raised from the lineage's standing 1.25× / 60 % to pay for the four looks and was pinned pre-run. Any cell may carry the verdict; **all four report regardless** — no cell-shopping, no post-hoc bar movement._"
    );
    println!();
    println!(
        "| seed | n | wf-asis | wf-rw-u | wf-rw-p | wf-val-u | wf-val-p | churn asis | churn rw-u |"
    );
    println!(
        "|-----:|--:|--------:|--------:|--------:|---------:|---------:|-----------:|-----------:|"
    );
    for i in 0..n_seeds {
        println!(
            "| {} | {} | {:.4} | {:.4} | {:.4} | {:.4} | {:.4} | {} | {} |",
            P9_SEED_START + i as u64,
            insts[i].base.agents.len(),
            asis[i].primary,
            rw_u[i].primary,
            rw_p[i].primary,
            val_u[i].primary,
            val_p[i].primary,
            asis[i].churn,
            rw_u[i].churn
        );
    }
    println!();
    let mut carried: Option<&str> = None;
    for (label, rs) in cells {
        let med = median(p9_primaries(rs));
        let superior = p9_superior_count(rs, &asis);
        let c1 = med >= bar;
        let c2 = superior >= P9_HP_SUPERIOR_MIN;
        if c1 && c2 && carried.is_none() {
            carried = Some(label);
        }
        println!(
            "- `{label}`: median **{med:.4}** vs bar {P9_HP_FACTOR} × {asis_med:.4} = **{bar:.4}** ⇒ **{}** · strictly superior **{superior}/{n_seeds}**, bar {P9_HP_SUPERIOR_MIN} ⇒ **{}** · cell ⇒ **{}**.",
            pass(c1),
            pass(c2),
            pass(c1 && c2)
        );
    }
    println!();
    let declined_total: usize = rw_u.iter().map(|r| r.declined).sum::<usize>()
        + rw_p.iter().map(|r| r.declined).sum::<usize>();
    println!(
        "- Success-rate context (never gated): `wf-asis` {:.4} · `wf-rw-u` {:.4} · `wf-rw-p` {:.4} (mean over seeds). Optimizer declines across both rewriting cells: **{declined_total}** (a declined task forms no coalition and scores zero — never a silent as-written fallback).",
        asis.iter().map(|r| r.success_rate).sum::<f64>() / n_seeds as f64,
        rw_u.iter().map(|r| r.success_rate).sum::<f64>() / n_seeds as f64,
        rw_p.iter().map(|r| r.success_rate).sum::<f64>() / n_seeds as f64
    );
    println!();

    let verdict = if carried.is_some() {
        "VALIDATED (process structure)"
    } else {
        "FALSIFIED (process structure)"
    };
    println!("## VERDICT: **{verdict}**");
    println!();
    println!(
        "_Grammar (prereg §7): `VALIDATED (process structure)` = at least one H-P cell passes BOTH conjuncts at the family-wise bar with every §6 gate holding · `FALSIFIED (process structure)` = gates hold, no cell clears the bar · `RUN-INVALID` = any §6 gate fails. This run: carrying cell {} · gates X-reduce {} / S-sound {} / S-dedup {}. E-ceil / E-conc / E-fuel mechanism-scoping is reported below but cannot upgrade the verdict. The v1/v2 K4 verdicts, EQ3's and EQ4's verdicts, and the #54 arm question (mag = demonstrated default, FINAL) are UNTOUCHED regardless of outcome, and a `VALIDATED` result would speak to the process-vs-as-written contrast within the typed magnitude family — not to the mag-vs-aif arm question, which is EQ5b's (prereg §7 pre-commitments)._",
        carried.map_or_else(|| "none".to_owned(), |c| format!("`{c}`")),
        pass(true),
        pass(sound_ok),
        pass(dedup_ok)
    );
    println!();

    // --- E-fuel --------------------------------------------------------------
    println!("## E-fuel (registered exploratory, non-gating) — how much of any margin is fuel-bought");
    println!();
    println!(
        "_The registered sweep `{P9_FUEL_GRID:?}` on the rewriting cells (confirmatory fuel is F = {P9_FUEL}), a direct probe of catgraph's registered no-termination posture. `fuel_exhausted()` counts are a MANDATORY disclosure (A1.2), never a RUN-INVALID condition._"
    );
    println!();
    println!(
        "| cell | fuel | median PRIMARY | vs `wf-asis` | superior seeds | fuel-exhausted tasks | median `states_explored` | median `best_cost` | declare wall s |"
    );
    println!(
        "|------|-----:|---------------:|-------------:|---------------:|---------------------:|-------------------------:|-------------------:|---------------:|"
    );
    let mut sweep_wall = 0.0f64;
    for cost in [P9Cost::Uniform, P9Cost::Priced] {
        for fuel in P9_FUEL_GRID {
            let t0 = Instant::now();
            let (d, s) = p9_declare(&insts, &rules, &labels, P9Mechanism::Rewrite, cost, fuel);
            let declare_s = t0.elapsed().as_secs_f64();
            sweep_wall += declare_s;
            let (rs, _) = p9_battery(&insts, &d, |inst| {
                Box::new(p8_typed_policy(&inst.base, &oracle)) as Box<dyn CoalitionDecisionPolicy>
            });
            let med = median(p9_primaries(&rs));
            println!(
                "| `wf-rw-{}` ({}) | {fuel} | {med:.4} | {:.2}× | {}/{n_seeds} | {} | {:.1} | {:.1} | {declare_s:.1} |",
                if cost == P9Cost::Uniform { "u" } else { "p" },
                cost.label(),
                if asis_med > 0.0 { med / asis_med } else { f64::NAN },
                p9_superior_count(&rs, &asis),
                s.fuel_exhausted,
                median(s.explored.clone()),
                median(s.best.clone())
            );
        }
    }
    println!();
    println!(
        "- **Search-cost disclosure (Amendment A3.2 (ii), MANDATORY).** `optimize` matches every rule against every state, so widening the theory from 51 to {} instances ({:.1}×) raises search cost directly. Measured on this run: the confirmatory declare (F = {P9_FUEL}, {n_seeds} seeds × {TASKS} tasks) took **{:.1} s** uniform / **{:.1} s** priced, and the whole registered sweep `{P9_FUEL_GRID:?}` × 2 cost models took **{sweep_wall:.1} s**. **The sweep ran IN FULL — no fuel point was dropped, so there is no reduction to disclose.** Had runtime forced one, A3.2 requires it be reported here rather than silently trimmed.",
        rules.len(),
        rules.len() as f64 / 51.0,
        confirm_wall_u,
        confirm_wall_p
    );
    println!();

    // --- E-conc --------------------------------------------------------------
    println!("## E-conc (registered disclosure, non-gating) — demand concentration onto absent `(bit, role)`");
    println!();
    println!(
        "_The registered failure mode, MEASURED: a cheaper writing may concentrate demand on a `(bit, role)` no pool worker holds. Such a task is COUNTED, never re-drawn (prereg §2). `PRIMARY = success_rate × mean cov_eff` is not additive over tasks, so the contribution below decomposes the **mean-quality** factor: `Δ_infeasible` is the part of the per-seed `mean cov_eff` gap that comes from tasks whose declared demand is infeasible, and `Δ_feasible` the rest._"
    );
    println!();
    println!(
        "| cell | infeasible tasks | rate | mean `cov_eff` there (cell / control) | Δ_infeasible | Δ_feasible |"
    );
    println!(
        "|------|-----------------:|-----:|--------------------------------------:|-------------:|-----------:|"
    );
    for (label, rs) in [("wf-rw-u", &rw_u), ("wf-rw-p", &rw_p)] {
        let mut infeasible = 0usize;
        let mut total = 0usize;
        let (mut q_cell, mut q_ctrl) = (0.0f64, 0.0f64);
        let (mut d_inf, mut d_feas) = (0.0f64, 0.0f64);
        for (r, c) in rs.iter().zip(asis.iter()) {
            let n = r.quality.len().min(c.quality.len()) as f64;
            for i in 0..r.quality.len().min(c.quality.len()) {
                total += 1;
                let delta = (r.quality[i] - c.quality[i]) / n;
                if r.declared_feasible[i] {
                    d_feas += delta;
                } else {
                    infeasible += 1;
                    q_cell += r.quality[i];
                    q_ctrl += c.quality[i];
                    d_inf += delta;
                }
            }
        }
        let denom = infeasible.max(1) as f64;
        println!(
            "| `{label}` | {infeasible} | {:.1} % | {:.4} / {:.4} | {:+.4} | {:+.4} |",
            100.0 * infeasible as f64 / total.max(1) as f64,
            q_cell / denom,
            q_ctrl / denom,
            d_inf / n_seeds as f64,
            d_feas / n_seeds as f64
        );
    }
    println!();

    // --- E-dedup -------------------------------------------------------------
    println!("## E-dedup (registered exploratory, non-gating) — content vs writing over the corpus");
    println!();
    println!(
        "_Reported as a corpus fact and a latency fact; it cannot carry the verdict (EQ3 spent this lineage's appetite for latency legs)._"
    );
    println!();
    println!(
        "- Corpus: **{}** as-written workflows ({n_seeds} seeds × {TASKS} tasks). Distinct as **written** (expression tree + pinned source word): **{}**. Distinct as **content** (`canonical_key`): **{}**. So **{}** arrivals are distinct as written but equal as content — what a content-keyed table buys over a writing-keyed one, at these draws.",
        dedup.corpus, dedup.writings, dedup.contents, dedup.collapsed
    );
    let singletons = dedup.buckets.iter().filter(|&&b| b == 1).count();
    println!(
        "- Bucket-size distribution: largest **{}**, median **{:.1}**, **{singletons}** singleton buckets of {} (sorted by SIZE — `ContentKey` is not `Ord`, so nothing here depends on key order).",
        dedup.buckets.first().copied().unwrap_or(0),
        median(dedup.buckets.iter().map(|&b| b as f64).collect::<Vec<f64>>()),
        dedup.buckets.len()
    );
    // The same question over a DECLARED corpus: rewriting is a normalizing pass,
    // so the writing-vs-content gap should close after it.
    let rw_corpus: Vec<&Workflow> = d_rw_u.iter().flat_map(|s| s.iter().map(|d| &d.writing)).collect();
    let rw_dedup = p9_dedup_pass(&rw_corpus);
    assert_eq!(
        rw_dedup.biconditional_violations, 0,
        "S-dedup must also hold on the declared corpus"
    );
    println!(
        "- Same pass over the **`wf-rw-u` DECLARED** corpus ({} writings): distinct as written **{}**, distinct as content **{}**, collapsed **{}**. Rewriting is a normalizing pass, so the writing-vs-content gap is expected to close relative to the as-written corpus above; the two numbers side by side are what the leg reports.",
        rw_dedup.corpus, rw_dedup.writings, rw_dedup.contents, rw_dedup.collapsed
    );
    println!(
        "- **Latency fact:** the median `states_explored` of the confirmatory rewriting cells — {:.1} (uniform) / {:.1} (priced) — is the number of DISTINCT contents the search kept, which is exactly what `canonical_key` dedup bought inside `optimize`; a writing-keyed visited set would have revisited every re-association of each of them.",
        median(s_rw_u.explored.clone()),
        median(s_rw_p.explored.clone())
    );
    println!();

    // --- E-ceil --------------------------------------------------------------
    println!("## E-ceil (registered exploratory, non-gating) — a reference arm WITHIN the rewriting family");
    println!();
    println!(
        "_NOT a supremum (the #72 A2.5 correction). `cost_of` sums per-generator weights over OCCURRENCES, so \"minimise DISTINCT `(bit, role)` demand\" — the staffing question — is not expressible as a `per_gen` at all; E-ceil therefore runs as (i) a large-fuel scarcity-priced cell and (ii) a harness-side minimum-distinct-demand search on a pinned subsample._"
    );
    println!();
    let (d_ceil, s_ceil) = p9_declare(
        &insts,
        &rules,
        &labels,
        P9Mechanism::Rewrite,
        P9Cost::Priced,
        P9_ECEIL_FUEL,
    );
    let (ceil_rs, ceil_lat) = p9_battery(&insts, &d_ceil, |inst| {
        Box::new(p8_typed_policy(&inst.base, &oracle)) as Box<dyn CoalitionDecisionPolicy>
    });
    p9_table_head();
    p9_row("wf-asis", &asis, &asis, &asis_lat);
    p9_row("wf-rw-p", &rw_p, &asis, &rw_p_lat);
    p9_row(
        &format!("E-ceil (i): priced, fuel {P9_ECEIL_FUEL}"),
        &ceil_rs,
        &asis,
        &ceil_lat,
    );
    println!();
    let ceil_med = median(p9_primaries(&ceil_rs));
    let reference_margin = ceil_med - asis_med;
    let best_rw = median(p9_primaries(&rw_u)).max(median(p9_primaries(&rw_p)));
    // A conversion fraction is only meaningful against a POSITIVE reference
    // margin. If E-ceil does not beat the control there is no margin to convert,
    // and a ratio of two negatives would print a confident-looking percentage
    // that means the opposite of what it reads — so it is refused rather than
    // formatted.
    let converted = if reference_margin > 0.0 {
        format!(
            "**{:.1} %**",
            100.0 * (best_rw - asis_med) / reference_margin
        )
    } else {
        format!(
            "**n/a** — the reference margin is not positive ({ceil_med:.4} − {asis_med:.4} = {reference_margin:+.4}), so there is no achievable margin for the confirmatory cells to have converted a fraction of. A ratio here would divide two same-signed numbers and read as a high conversion rate while describing a reference arm that does not beat the control"
        )
    };
    println!(
        "- **Conversion fraction (leg i):** `(best confirmatory cell − wf-asis) / (E-ceil − wf-asis)` = {converted}. Medians, so this is a summary contrast, not a per-seed decomposition."
    );
    println!(
        "  Fuel-exhausted tasks at fuel {P9_ECEIL_FUEL}: **{}**; median `states_explored` **{:.1}**.",
        s_ceil.fuel_exhausted,
        median(s_ceil.explored.clone())
    );
    // Leg (ii): the pinned subsample.
    let sub = &insts[..(P9_ECEIL_SEEDS as usize).min(insts.len())];
    let mut as_written_d: Vec<f64> = Vec::new();
    let mut rw_d: Vec<f64> = Vec::new();
    let mut ref_d: Vec<f64> = Vec::new();
    for (s, inst) in sub.iter().enumerate() {
        for (t, w) in inst.written.iter().enumerate() {
            as_written_d.push(demand(w).distinct_len() as f64);
            rw_d.push(d_rw_u[s][t].demand.distinct_len() as f64);
            ref_d.push(p9_eceil_min_distinct(inst, w, &rules) as f64);
        }
    }
    println!(
        "- **Leg (ii), pinned subsample (seeds {P9_SEED_START}..{}, {} tasks):** median DISTINCT `(bit, role)` demand — as written **{:.1}**, `wf-rw-u` at F = {P9_FUEL} **{:.1}**, reference **{:.1}**.",
        P9_SEED_START + P9_ECEIL_SEEDS,
        as_written_d.len(),
        median(as_written_d.clone()),
        median(rw_d.clone()),
        median(ref_d.clone())
    );
    println!(
        "  _What the reference actually searches, stated plainly: `optimize` returns only `best()` — its visited set is private — so the class it enumerates is not readable through the public API. The reference is a pinned MULTI-OBJECTIVE sweep over that class at fuel {P9_ECEIL_FUEL}: the uniform and priced optima, plus one optimum per distinct as-written step under an objective that prices THAT step at {P9_ECEIL_HEAVY} and everything else at 1 — which is how a per-generator weight can ask the search to route around a specific `(bit, role)`. The minimum over those optima is an UPPER bound on the true minimum-distinct demand, and a reference within the rewriting family; it is not a supremum on what any arm could achieve._"
    );
    println!();

    // --- Instrumentation -----------------------------------------------------
    println!("## Instrumentation (non-gating)");
    println!();
    println!(
        "| cell | median `initial_cost` | median `best_cost` | median steps | median `states_explored` | unchanged writings | **demand moved** | idem / fusion / absorption fires |"
    );
    println!(
        "|------|----------------------:|-------------------:|-------------:|-------------------------:|-------------------:|-----------------:|--------------------------------:|"
    );
    for (label, s) in [
        ("wf-asis", &s_ctl),
        ("wf-rw-u", &s_rw_u),
        ("wf-rw-p", &s_rw_p),
        ("wf-val-u", &s_val_u),
        ("wf-val-p", &s_val_p),
    ] {
        println!(
            "| `{label}` | {:.1} | {:.1} | {:.1} | {:.1} | {} | {} | {} / {} / {} |",
            median(s.initial.clone()),
            median(s.best.clone()),
            median(s.trace_len.clone()),
            median(s.explored.clone()),
            s.unchanged,
            s.demand_moved,
            s.fired[0],
            s.fired[1],
            s.fired[2]
        );
    }
    println!();
    println!(
        "- **`demand moved`** counts tasks whose declared DISTINCT `(bit, role)` demand differs from the as-written one — the only way a rewrite can change a score, since coverage is per distinct pair. Every other task in a rewriting cell scores bit-identically to `wf-asis` no matter how much cheaper its writing got. Against the fusion-eligibility bound printed in the draw section, this is the leg's whole causal budget: `wf-rw-u` **{}**/{}, `wf-rw-p` **{}**/{}.",
        s_rw_u.demand_moved,
        occurrences.len(),
        s_rw_p.demand_moved,
        occurrences.len()
    );
    println!();
    let mut red_u = s_rw_u.reduction.clone();
    red_u.sort_by(f64::total_cmp);
    let mut red_p = s_rw_p.reduction.clone();
    red_p.sort_by(f64::total_cmp);
    println!(
        "- **Distinct-demand reduction** (`as written − declared`, per task): `wf-rw-u` p25 {:.1} / median {:.1} / p75 {:.1} / max {:.1}; `wf-rw-p` p25 {:.1} / median {:.1} / p75 {:.1} / max {:.1}. A NEGATIVE value is the two-sided fusion lever working against the arm — the fused step demands a bit neither consumed step required.",
        percentile(&red_u, 0.25),
        percentile(&red_u, 0.5),
        percentile(&red_u, 0.75),
        red_u.last().copied().unwrap_or(0.0),
        percentile(&red_p, 0.25),
        percentile(&red_p, 0.5),
        percentile(&red_p, 0.75),
        red_p.last().copied().unwrap_or(0.0)
    );
    println!(
        "- **The residual term** (λ = {P9_LAMBDA}, Amendment A3.1): median FULL residual `λ · Σ per_gen` at an empty coalition — the largest the penalty can be — is **{:.3}** uniform / **{:.3}** staffing-priced; it falls to zero as the coalition covers the writing. A1.4's rationale pins λ so the process term is \"a tiebreaker between otherwise close candidates rather than the dominant term\" against O(1) magnitude margins; these magnitudes plus the liveness divergence counts above are the check on that reasoning.",
        median(val_u_terms.clone()),
        median(val_p_terms.clone())
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
    /// own class (they have no decade), sub-`1e-16` magnitudes get their own
    /// underflow row rather than being folded into the first decade, non-finite
    /// increments are counted without ever reaching the index arithmetic, every
    /// other magnitude lands in the half-open decade `[1e(i-16), 1e(i-15))`, and
    /// anything `≥ 1e0` saturates the overflow bucket. Driven through synthetic
    /// probes so the arithmetic is checked independently of any battery.
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
        inc.record(&probe(12.0, false, None)); // overflow (saturates)
        inc.record(&probe(1e-30, false, None)); // BELOW the floor ⇒ underflow row
        inc.record(&probe(f64::INFINITY, false, None)); // non-finite
        inc.record(&probe(f64::NEG_INFINITY, false, None)); // non-finite
        inc.record(&probe(f64::NAN, false, None)); // non-finite

        assert_eq!(inc.probed, 12);
        assert_eq!(inc.exact_zero, 2, "±0.0 are the exact-zero class");
        assert_eq!(
            inc.non_finite, 3,
            "NaN and ±∞ are their own class and never index a decade"
        );
        assert_eq!(
            inc.underflow, 1,
            "sub-1e-16 gets its own row, not the first decade"
        );
        assert_eq!(inc.decades[0], 2, "[1e-16, 1e-15) only");
        assert_eq!(inc.decades[1], 1);
        assert_eq!(inc.decades[15], 1);
        assert_eq!(inc.decades[16], 2, "≥ 1e0 saturates the overflow bucket");
        assert_eq!(
            inc.decades.iter().sum::<usize>()
                + inc.exact_zero
                + inc.underflow
                + inc.non_finite,
            inc.probed,
            "the classes are disjoint and exhaustive"
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

    /// Part 8 (#72): the v2t draw APPENDS to the v2 stream — the untyped prefix
    /// (pool, caps, trust, arrival orders) is bit-identical to a pure-v2 draw of
    /// the same seed, which is the #46/#48 shared-prefix discipline the prereg
    /// invokes. `required` is compared only on tasks the feasibility pass did NOT
    /// re-draw (a re-draw legitimately replaces it); the run prints the re-draw
    /// count so that exemption is never silent.
    #[test]
    fn v2t_prefix_matches_v2() {
        for seed in P8_SEED_START..P8_SEED_START + 4 {
            let mut rng_v2 = SplitMix64::new(seed);
            let (base_agents, base_tasks) = draw_prefix_v2(&mut rng_v2);
            let inst = draw_typed_instance(seed, P8_ROLES);

            assert_eq!(base_agents.len(), inst.agents.len(), "pool size unchanged");
            for (b, a) in base_agents.iter().zip(inst.agents.iter()) {
                assert_eq!((b.id, b.caps, b.trust), (a.id, a.caps, a.trust));
            }
            assert_eq!(base_tasks.len(), inst.tasks.len());
            for (b, t) in base_tasks.iter().zip(inst.tasks.iter()) {
                assert_eq!(b.order, t.order, "arrival orders are never re-drawn");
            }
            if inst.redraws == 0 {
                for (b, t) in base_tasks.iter().zip(inst.tasks.iter()) {
                    assert_eq!(b.required, t.required, "no re-draw ⇒ v2 `required` kept");
                }
            }
            assert_eq!(inst.roles.len(), inst.agents.len(), "one role per worker");
            assert!(inst.roles.iter().all(|&r| r < P8_ROLES), "roles are 0..R");
        }
    }

    /// Part 8 (#72): the feasibility guarantee HOLDS after the rejection re-draw —
    /// every required bit of every task is coverable by a pool worker of that
    /// bit's tag role — and the identity world (R = 1) collapses roles and tags
    /// to 0 while keeping the same guarantee.
    #[test]
    fn v2t_tasks_are_role_feasible() {
        for n_roles in [P8_ROLES, P8_IDENTITY_ROLES] {
            for seed in P8_SEED_START..P8_SEED_START + 4 {
                let inst = draw_typed_instance(seed, n_roles);
                assert!(inst.roles.iter().all(|&r| r < n_roles));
                for task in &inst.tasks {
                    assert!(
                        (2..=8).contains(&task.required.count_ones()),
                        "the re-draw keeps the v2 |required| shape"
                    );
                    assert!(
                        p8_task_feasible(&inst.agents, &inst.roles, task.required, &task.tags),
                        "seed {seed}: a task survived the feasibility pass infeasible"
                    );
                    for b in 0..UNIVERSE {
                        if task.required & (1u32 << b) != 0 {
                            assert!(task.tags[b] < n_roles, "tags are 0..R");
                        }
                    }
                }
            }
        }
    }

    /// Part 8 (#72): the typed scorer on a hand-computed mini case. Two required
    /// bits with different tags; the member of role 0 holds both, so only the
    /// bit tagged 0 is role-matched — the untyped scorer would count both.
    #[test]
    fn p8_typed_metric_hand_case() {
        let agents = vec![
            Worker {
                id: 0,
                caps: 0b011,
                trust: 50,
            },
            Worker {
                id: 1,
                caps: 0b100,
                trust: 50,
            },
        ];
        let mut tags = [0usize; UNIVERSE];
        tags[0] = 0; // bit 0 wants role 0 — agent 0 (role 0) holds it ⇒ matched
        tags[1] = 1; // bit 1 wants role 1 — only agent 0 holds it, role 0 ⇒ unmatched
        let inst = TypedInstance {
            agents,
            roles: vec![0, 1],
            tasks: Vec::new(),
            redraws: 0,
            max_attempts: 0,
        };
        let task = TypedTask {
            required: 0b011,
            tags,
            order: vec![0, 1],
        };

        assert_eq!(p8_typed_covered(&inst, &[0], &task), 1, "only bit 0 matches");
        assert_eq!(
            p8_untyped_covered(&inst, &[0], task.required),
            2,
            "untyped coverage sees both bits"
        );
        // Agent 1 is role 1 but holds neither required bit, so it adds nothing.
        assert_eq!(p8_typed_covered(&inst, &[0, 1], &task), 1);
        // Re-tag bit 1 to role 0: now the same member matches both.
        let mut tags0 = tags;
        tags0[1] = 0;
        let task0 = TypedTask {
            required: 0b011,
            tags: tags0,
            order: vec![0, 1],
        };
        assert_eq!(p8_typed_covered(&inst, &[0], &task0), 2);

        // Mean pairwise ρ: singleton is 1.0 by convention; the 0/1 cross pair
        // reads the off-diagonal in both directions.
        let rho = p8_rho_table(0.25);
        assert!((p8_mean_pairwise_rho(&inst, &[0], &rho) - 1.0).abs() < 1e-12);
        assert!((p8_mean_pairwise_rho(&inst, &[0, 1], &rho) - 0.25).abs() < 1e-12);
        assert!((p8_mean_pairwise_rho(&inst, &[0, 0], &rho) - 1.0).abs() < 1e-12);
    }

    /// Part 8 (#72): the E-T3 channel formula on a hand case, including the
    /// registered neutral-`1.0` convention for an empty denominator, and the
    /// uniform-θ geometric-mean collapse.
    #[test]
    fn p8_channel_formula_hand_case() {
        // required = bits 0,1,2; tags: bit0 → channel 0, bits 1,2 → channel 1.
        let required = 0b111u32;
        let mut tags = [0usize; UNIVERSE];
        tags[0] = 0;
        tags[1] = 1;
        tags[2] = 1;

        let masks = [0b011u32, 0b110u32];
        // rel_0 = {0,1}, rel_1 = {1,2}; tagged(0) = {0}, tagged(1) = {1,2},
        // tagged(2) = {} (no bit carries tag 2).
        // A_0(0→1) = |{} ∩ {0}| / |{0}| = 0
        // A_1(0→1) = |{1}| / |{1}| = 1
        // A_2(0→1) = empty denominator ⇒ neutral 1.0
        // collapse θ = (1/3,1/3,1/3) ⇒ 0^(1/3) · 1 · 1 = 0
        // A_0(1→0): rel_1 ∩ tagged(0) = {} ⇒ neutral 1.0
        // A_1(1→0) = |{1}| / |{1,2}| = 0.5 ; A_2 ⇒ 1.0
        // collapse ⇒ 0.5^(1/3)
        let mut cc = ChannelCouplings::new(P8_CHANNELS).expect("3 channels");
        cc.set(0, 1, vec![0.0, 1.0, 1.0]).expect("valid vector");
        cc.set(1, 0, vec![1.0, 0.5, 1.0]).expect("valid vector");
        let theta = [1.0 / 3.0; P8_CHANNELS];
        let expected = cc.collapse(&theta).expect("valid theta");
        assert_eq!(expected.len(), 2);
        assert!((expected[0].2 - 0.0).abs() < 1e-12);
        assert!((expected[1].2 - 0.5f64.powf(1.0 / 3.0)).abs() < 1e-12);

        // The policy's own magnitude must be finite and match a hand-built
        // evaluation of the very same collapsed table.
        let counters = P8ChannelCounters::default();
        let got =
            p8_channel_magnitude(&masks, required, &tags, Some(&counters)).expect("channel magnitude");
        let agents = [0usize, 1];
        let want =
            catgraph_magnitude::coalition_magnitude_from_couplings(&agents, &expected, &agents, 1.0)
                .expect("hand-built magnitude");
        assert!(
            (got - want).abs() <= 1e-12 * want.abs().max(1.0),
            "channel magnitude {got} vs hand-built {want}"
        );

        // The decision-inert caveat counters on the same hand case: 2 ordered
        // pairs × 3 channels = 6 entries; the neutral ones are channel 2 in both
        // directions (no bit carries tag 2) plus channel 0 in the 1 → 0
        // direction (rel_1 ∩ tagged(0) is empty) = 3. Neither collapsed coupling
        // is exactly 1.0 (0 and 0.5^(1/3)), and no pair is all-neutral.
        let (entries, neutral, unit_all_neutral, unit_rounded) = counters.get();
        assert_eq!((entries, neutral), (6, 3));
        assert_eq!((unit_all_neutral, unit_rounded), (0, 0));

        // A pair with NO tagged evidence at all is the caveat's pure case: every
        // channel neutral ⇒ collapsed exactly 1.0, counted as all-neutral.
        let mut lone = [0usize; UNIVERSE];
        lone[3] = 0; // bit 3 tagged role 0; the masks below hold none of it
        let blind = P8ChannelCounters::default();
        let _ = p8_channel_magnitude(&[0b011, 0b110], 0b1000, &lone, Some(&blind))
            .expect("no relevant bits");
        let (_, blind_neutral, blind_all, blind_rounded) = blind.get();
        assert_eq!(blind_neutral, 6, "every channel entry is neutral");
        assert_eq!((blind_all, blind_rounded), (2, 0));

        // Empty and singleton guards (no counters — `None` must be accepted).
        assert_eq!(
            p8_channel_magnitude(&[], required, &tags, None).expect("empty is 0"),
            0.0
        );
        assert!(
            (p8_channel_magnitude(&[0b011], required, &tags, None).expect("singleton") - 1.0).abs()
                < 1e-12
        );
    }

    /// Part 8 (#72), S-fib: one deterministic role-grid shape agrees with its
    /// construction-carried certificate within the upstream-documented relative
    /// tolerance. The full three-shape gate runs in the battery.
    #[test]
    fn p8_sfib_one_shape() {
        let shape = p8_fib_shapes().remove(0);
        let role = RoleModulation::new(shape.role).expect("square [0,1] table");
        let fiber = RoleModulation::new(shape.fiber).expect("square [0,1] table");
        let grid = role_grid(&role, &fiber).expect("unit diagonals");
        assert_eq!(grid.n_agents(), 4);
        let agents: Vec<usize> = (0..grid.n_agents()).collect();
        let actual = catgraph_magnitude::coalition_magnitude_from_couplings(
            &agents,
            grid.couplings(),
            &agents,
            1.0,
        )
        .expect("grid evaluates");
        let expected = grid.proof(1.0).expect("certificate").expected_magnitude();
        assert!(
            (actual - expected).abs() <= P8_FIB_REL_TOL * actual.abs().max(expected.abs()).max(1.0),
            "grid {actual} vs certificate {expected}"
        );
    }

    /// Part 8 (#72), 2-seed smoke: the registered arms run end-to-end on the v2t
    /// world, the ρ ≡ 1 typed path reproduces `mag`'s decisions (the X-identity
    /// cell-2 property at 2-seed scale), and the exploratory example-side arms
    /// produce finite in-range metrics. Does NOT run the 30-seed battery.
    #[test]
    fn p8_two_seed_smoke() {
        let insts: Vec<TypedInstance> = (P8_SEED_START..P8_SEED_START + 2)
            .map(|s| draw_typed_instance(s, P8_ROLES))
            .collect();

        let (mag, mag_lat) = p8_battery(
            &insts,
            |_| Box::new(MagnitudePolicy::default()) as Box<dyn CoalitionDecisionPolicy>,
            P8Metric::Typed,
        );
        let ones = p8_rho(1.0);
        let (typed_ones, _) = p8_battery(
            &insts,
            |inst| Box::new(p8_typed_policy(inst, &ones)) as Box<dyn CoalitionDecisionPolicy>,
            P8Metric::Typed,
        );
        assert_eq!(mag.len(), 2);
        assert!(!mag_lat.is_empty(), "latencies recorded");
        for (m, t) in mag.iter().zip(typed_ones.iter()) {
            assert_eq!(m.acts, t.acts, "ρ ≡ 1 must reproduce the untyped acts");
            assert_eq!(m.primary.to_bits(), t.primary.to_bits());
            assert_eq!(m.churn, t.churn);
        }

        let oracle = p8_rho(0.0);
        let (typed, _) = p8_battery(
            &insts,
            |inst| Box::new(p8_typed_policy(inst, &oracle)) as Box<dyn CoalitionDecisionPolicy>,
            P8Metric::Typed,
        );
        let (ceil, _) = p8_pertask_battery(
            &insts,
            |inst, task| {
                Box::new(TypedRelevanceMag {
                    roles: inst.roles.clone(),
                    tags: task.tags,
                    join_margin: 0.0,
                }) as Box<dyn CoalitionDecisionPolicy>
            },
            P8Metric::Typed,
        );
        let counters = Arc::new(P8ChannelCounters::default());
        let (t3, _) = p8_pertask_battery(
            &insts,
            |_inst, task| {
                Box::new(ChannelMagnitudePolicy {
                    tags: task.tags,
                    join_margin: 0.0,
                    counters: Arc::clone(&counters),
                }) as Box<dyn CoalitionDecisionPolicy>
            },
            P8Metric::Typed,
        );
        let (entries, neutral, _, _) = counters.get();
        assert!(entries > 0, "the E-T3 leg must build channel entries");
        assert!(neutral <= entries, "neutral entries are a subset");
        let (rq, _) = p8_battery(
            &insts,
            |_| Box::new(MagnitudePolicy::default()) as Box<dyn CoalitionDecisionPolicy>,
            P8Metric::RhoQ,
        );
        for r in typed.iter().chain(&ceil).chain(&t3).chain(&rq) {
            assert!(r.primary.is_finite() && (0.0..=1.0).contains(&r.primary));
        }

        // T1: the instrumentation pass decomposes real coalitions, and under the
        // oracle ρ = δ no skeletal class can span two roles.
        let stats = p8_role_share_pass(&insts, &oracle);
        assert!(stats.samples > 0, "some coalition must be decomposable");
        assert_eq!(stats.errors, 0, "no upstream error on the T1 path");
        assert_eq!(
            stats.mixed_classes, 0,
            "ρ = δ zeroes cross-role couplings, so no class can be role-mixed"
        );
    }

    // -- Part 9 (#76, EQ5a) ---------------------------------------------------

    /// Part 9 (#76): the v2w prefix is the v2t prefix VERBATIM — the workflow
    /// shape draw is APPENDED, so pool, roles, tags, arrival orders and re-draw
    /// bookkeeping are bit-identical to a pure-v2t draw of the same seed.
    #[test]
    fn v2w_prefix_matches_v2t() {
        for seed in P9_SEED_START..P9_SEED_START + 4 {
            let v2t = draw_typed_instance(seed, P8_ROLES);
            for degenerate in [false, true] {
                let v2w = p9_draw_instance(seed, degenerate);
                assert_eq!(v2t.agents.len(), v2w.base.agents.len());
                for (a, b) in v2t.agents.iter().zip(v2w.base.agents.iter()) {
                    assert_eq!((a.id, a.caps, a.trust), (b.id, b.caps, b.trust));
                }
                assert_eq!(v2t.roles, v2w.base.roles);
                assert_eq!(v2t.redraws, v2w.base.redraws);
                assert_eq!(v2t.max_attempts, v2w.base.max_attempts);
                assert_eq!(v2t.tasks.len(), v2w.base.tasks.len());
                for (a, b) in v2t.tasks.iter().zip(v2w.base.tasks.iter()) {
                    assert_eq!(a.required, b.required);
                    assert_eq!(a.tags, b.tags);
                    assert_eq!(a.order, b.order);
                }
                assert_eq!(v2w.written.len(), v2w.base.tasks.len());
            }
        }
    }

    /// Part 9 (#76): the degenerate world consumes ZERO stream draws, which is
    /// what makes X-reduce a two-code-paths-one-world comparison. Checked by
    /// continuing the SAME stream past the prefix: the degenerate build must
    /// leave the generator exactly where the v2t prefix did, while the real
    /// shape draw must move it.
    #[test]
    fn degenerate_shape_consumes_no_draws() {
        for seed in P9_SEED_START..P9_SEED_START + 4 {
            let mut a = SplitMix64::new(seed);
            let (_, _, tasks, _, _) = draw_prefix_v2t(&mut a, P8_ROLES as u64);
            let after_prefix = a.next_u64();

            let mut b = SplitMix64::new(seed);
            let (_, _, tasks_b, _, _) = draw_prefix_v2t(&mut b, P8_ROLES as u64);
            for t in &tasks_b {
                let _ = p9_degenerate_shape(t);
            }
            assert_eq!(
                b.next_u64(),
                after_prefix,
                "the degenerate shape must consume no draws"
            );

            // And the real shape draw DOES consume draws — otherwise the check
            // above would be vacuous.
            let mut c = SplitMix64::new(seed);
            let (_, _, tasks_c, _, _) = draw_prefix_v2t(&mut c, P8_ROLES as u64);
            for t in &tasks_c {
                let _ = p9_draw_shape(&mut c, t);
            }
            assert_ne!(
                c.next_u64(),
                after_prefix,
                "the drawn shape must consume stream draws"
            );

            // The degenerate demand IS the v2t tagged required set.
            let inst = p9_draw_instance(seed, true);
            for (task, w) in tasks.iter().zip(inst.written.iter()) {
                let d = demand(w);
                assert_eq!(d.distinct_len(), task.required.count_ones() as usize);
                assert_eq!(
                    d.total(),
                    d.distinct_len(),
                    "no multiplicity when fan-out is 0"
                );
                assert_eq!(p9_required_mask(&d), task.required);
            }
        }
    }

    /// Part 9 (#76): the drawn world is well-formed — every diagram pins, its
    /// demand is role-matched feasible in the pool, its distinct demand is
    /// exactly the tagged required set, and multiplicity only ever comes from
    /// the fan-out.
    #[test]
    fn v2w_shape_is_well_formed() {
        for seed in P9_SEED_START..P9_SEED_START + 3 {
            let inst = p9_draw_instance(seed, false);
            for (task, w) in inst.base.tasks.iter().zip(inst.written.iter()) {
                let d = demand(w);
                assert_eq!(
                    d.distinct_len(),
                    task.required.count_ones() as usize,
                    "the shape draw never introduces a new (bit, role)"
                );
                assert_eq!(p9_required_mask(&d), task.required);
                assert!(d.total() >= d.distinct_len());
                assert!(
                    p9_demand_feasible(&inst, &d),
                    "as-written demand must be role-matched feasible"
                );
                for s in d.distinct() {
                    assert_eq!(usize::from(s.role.index()), task.tags[s.bit as usize]);
                }
            }
        }
    }

    /// Part 9 (#76) 2-seed smoke: the whole declare → verify → score pipeline
    /// runs end-to-end on the registered seeds, the rewriting cell is S-sound,
    /// and the empty-rule cell reproduces the control bit-identically (the
    /// X-reduce property at 2-seed scale). Does NOT run the 30-seed registered
    /// battery. Valuation liveness has its own test below.
    #[test]
    fn part9_two_seed_smoke() {
        let bits = u8::try_from(UNIVERSE).unwrap();
        let roles = u8::try_from(P8_ROLES).unwrap();
        let rules = rule_theory(bits, roles).unwrap();
        let labels = rule_labels(bits, roles).unwrap();
        // Amendment A3.2's widened closure: 24 idempotence + 126 fusion + 24
        // absorption. The battery prints the theory in full, so a drift here
        // would silently change what every rewriting cell searches over.
        assert_eq!(rules.len(), 174);
        assert_eq!(fusion_pairs(bits).len() * P8_ROLES, 126);

        let insts: Vec<WorkflowInstance> = (P9_SEED_START..P9_SEED_START + 2)
            .map(|s| p9_draw_instance(s, false))
            .collect();
        let oracle = p8_rho(0.0);

        let (d_ctl, _) = p9_declare(
            &insts,
            &rules,
            &labels,
            P9Mechanism::AsWritten,
            P9Cost::Uniform,
            P9_FUEL,
        );
        let (d_rw, s_rw) = p9_declare(
            &insts,
            &rules,
            &labels,
            P9Mechanism::Rewrite,
            P9Cost::Uniform,
            P9_FUEL,
        );
        assert_eq!(s_rw.unsound, 0, "S-sound at 2-seed scale");
        assert_eq!(s_rw.failed, 0, "no optimizer declines expected");
        assert!(s_rw.verified > 0, "the rewriting cell must verify writings");

        let (asis, lat) = p9_battery(&insts, &d_ctl, |inst| {
            Box::new(p8_typed_policy(&inst.base, &oracle)) as Box<dyn CoalitionDecisionPolicy>
        });
        let (rw, _) = p9_battery(&insts, &d_rw, |inst| {
            Box::new(p8_typed_policy(&inst.base, &oracle)) as Box<dyn CoalitionDecisionPolicy>
        });
        assert_eq!(asis.len(), 2);
        assert!(!lat.is_empty(), "latencies recorded");
        for r in asis.iter().chain(&rw) {
            assert!(r.primary.is_finite() && (0.0..=1.0).contains(&r.primary));
            assert!(!r.acts.is_empty(), "some join/leave decisions must have run");
        }

        // Empty rule set ⇒ the rewriting cell IS the control (X-reduce's second
        // conjunct, at 2-seed scale).
        let (d_empty, _) = p9_declare(
            &insts,
            &[],
            &labels,
            P9Mechanism::Rewrite,
            P9Cost::Uniform,
            P9_FUEL,
        );
        let (empty_rs, _) = p9_battery(&insts, &d_empty, |inst| {
            Box::new(p8_typed_policy(&inst.base, &oracle)) as Box<dyn CoalitionDecisionPolicy>
        });
        p9_assert_identical(&empty_rs, &asis, "empty-rule rewriting cell");

        // The valuation cell runs end-to-end and reports a finite, non-negative
        // full residual per task (liveness is asserted separately).
        let (d_val, _) = p9_declare(
            &insts,
            &rules,
            &labels,
            P9Mechanism::Valuation,
            P9Cost::Priced,
            P9_FUEL,
        );
        let (val, _, terms) =
            p9_valuation_battery(&insts, &d_val, &oracle, P9_LAMBDA, P9Cost::Priced);
        assert_eq!(val.len(), 2);
        assert_eq!(terms.len(), 2 * TASKS);
        assert!(terms.iter().all(|t| t.is_finite() && *t >= 0.0));
        assert!(
            terms.iter().any(|&t| t > 0.0),
            "a declared writing with steps must carry a nonzero residual at an empty coalition"
        );

        // Context arms run over the same declared writings.
        let (mag, _) = p9_battery(&insts, &d_ctl, |_| {
            Box::new(MagnitudePolicy::default()) as Box<dyn CoalitionDecisionPolicy>
        });
        assert_eq!(mag.len(), 2);
        let (e1, _) = p9_e1_battery(&insts, &d_ctl, e1_config());
        assert_eq!(e1.len(), 2);

        // The dedup pass answers, and its biconditional holds.
        let corpus: Vec<&Workflow> = insts.iter().flat_map(|i| i.written.iter()).collect();
        let dedup = p9_dedup_pass(&corpus);
        assert_eq!(dedup.biconditional_violations, 0, "S-dedup at 2-seed scale");
        assert_eq!(dedup.corpus, 2 * TASKS);
        assert!(dedup.contents <= dedup.writings);
    }

    /// Part 9 (#76), Amendment **A3.1**: the valuation-only arm must NOT be
    /// bit-identical to the control.
    ///
    /// This is the regression the original D3b formulation would have failed
    /// silently. `Mag(S) − λ · cost_of(writing)` with a coalition-independent
    /// writing is a per-task CONSTANT: it cancels exactly from every join/leave
    /// margin, so the cell reproduced `wf-asis` on acts, raw score bits, PRIMARY
    /// and churn — every test it had passed, because it was a no-op. The
    /// unstaffable-residual re-read depends on `S`, and this test proves the
    /// difference is observable rather than argued.
    #[test]
    fn part9_valuation_is_live_a3_1() {
        let bits = u8::try_from(UNIVERSE).unwrap();
        let roles = u8::try_from(P8_ROLES).unwrap();
        let rules = rule_theory(bits, roles).unwrap();
        let labels = rule_labels(bits, roles).unwrap();
        let oracle = p8_rho(0.0);
        let insts: Vec<WorkflowInstance> = (P9_SEED_START..P9_SEED_START + 4)
            .map(|s| p9_draw_instance(s, false))
            .collect();

        let (d_ctl, _) = p9_declare(
            &insts,
            &rules,
            &labels,
            P9Mechanism::AsWritten,
            P9Cost::Uniform,
            P9_FUEL,
        );
        let (asis, _) = p9_battery(&insts, &d_ctl, |inst| {
            Box::new(p8_typed_policy(&inst.base, &oracle)) as Box<dyn CoalitionDecisionPolicy>
        });

        let mut live = false;
        for cost in [P9Cost::Uniform, P9Cost::Priced] {
            let (d_val, _) = p9_declare(
                &insts,
                &rules,
                &labels,
                P9Mechanism::Valuation,
                cost,
                P9_FUEL,
            );
            let (val, _, _) =
                p9_valuation_battery(&insts, &d_val, &oracle, P9_LAMBDA, cost);
            let (_, acts, scores) = p9_divergence(&val, &asis);
            live |= acts > 0 || scores > 0;
        }
        assert!(
            live,
            "A3.1: the valuation-only cells must diverge from the control on acts or score bits — \
             a cell that cannot move is the inert formulation the amendment replaced"
        );

        // …and the divergence is caused by the residual, not by the wrapper:
        // λ = 0 zeroes every correction, so the cell must collapse back onto the
        // control exactly.
        let (d_zero, _) = p9_declare(
            &insts,
            &rules,
            &labels,
            P9Mechanism::Valuation,
            P9Cost::Priced,
            P9_FUEL,
        );
        let (zero, _, terms) = p9_valuation_battery(&insts, &d_zero, &oracle, 0.0, P9Cost::Priced);
        assert!(terms.iter().all(|&t| t == 0.0));
        for (a, b) in zero.iter().zip(asis.iter()) {
            assert_eq!(a.acts, b.acts, "λ = 0 must reproduce the control's acts");
            assert_eq!(a.scores, b.scores, "λ = 0 must reproduce the control's scores");
        }
    }

    /// Part 9 (#76): X-reduce's first conjunct at 2-seed scale — on the
    /// degenerate world `wf-asis` reproduces the EQ4 typed arm's acts, per-seed
    /// PRIMARY bits and churn through the FROZEN Part 8 runner.
    #[test]
    fn part9_x_reduce_two_seed() {
        let labels = rule_labels(
            u8::try_from(UNIVERSE).unwrap(),
            u8::try_from(P8_ROLES).unwrap(),
        )
        .unwrap();
        let oracle = p8_rho(0.0);
        let deg: Vec<WorkflowInstance> = (P9_SEED_START..P9_SEED_START + 2)
            .map(|s| p9_draw_instance(s, true))
            .collect();
        let (d, _) = p9_declare(
            &deg,
            &[],
            &labels,
            P9Mechanism::AsWritten,
            P9Cost::Uniform,
            P9_FUEL,
        );
        let (mine, _) = p9_battery(&deg, &d, |inst| {
            Box::new(p8_typed_policy(&inst.base, &oracle)) as Box<dyn CoalitionDecisionPolicy>
        });
        let mut lat = Vec::new();
        let reference: Vec<P8Seed> = deg
            .iter()
            .map(|inst| {
                let p = p8_typed_policy(&inst.base, &oracle);
                p8_run_seed(
                    &P8Arm::Fixed(&p),
                    &inst.base,
                    P8Metric::Typed,
                    &mut lat,
                    &mut |_, _, _, _| {},
                )
            })
            .collect();
        for (a, b) in mine.iter().zip(reference.iter()) {
            assert_eq!(a.acts, b.acts, "X-reduce acts");
            assert_eq!(a.primary.to_bits(), b.primary.to_bits(), "X-reduce PRIMARY");
            assert_eq!(a.churn, b.churn, "X-reduce churn");
        }
    }
}
