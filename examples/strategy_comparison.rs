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
    AgentCapabilities, FeedbackCalculator, FeedbackStore, SynergisticCalculator,
};
use koalisi::decision::{
    AifDecisionPolicy, AifMmDecisionPolicy, CoalitionDecisionPolicy, CouplingModel, Decision,
    DecisionContext, MagnitudePolicy, PersistentAifArm, PersistentAifConfig, ThresholdPolicy,
    TrialBoundary,
};

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
    let mut mask = 0u32;
    let mut count = 0u64;
    while count < k {
        let bit = (rng.next_u64() % UNIVERSE_BITS) as u32;
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
    let mut rng = SplitMix64::new(seed);
    let (agents, tasks) = draw_prefix(&mut rng);
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
    mut on_task_outcome: impl FnMut(u32, &[bool; 8], bool),
) -> SeedResultB {
    let (agents, tasks, _rho, perf) = generate_instance_b(seed);

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
        let mut per_bit = [false; 8];
        for (b, slot) in per_bit.iter_mut().enumerate() {
            *slot = members
                .iter()
                .any(|&i| (agents[i].caps >> b) & 1 == 1 && perf[t][i]);
        }
        on_task_outcome(task.required, &per_bit, success);
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
    {
        let arm = PersistentAifArm::new(start, config).expect("persistent arm construction");
        let mut warm = Vec::new();
        let _ = run_seed_b(&arm, start, &mut warm, |req, succ, _| arm.observe_outcome(req, succ));
    }
    let mut lat = Vec::new();
    let results = (start..end)
        .map(|s| {
            let arm = PersistentAifArm::new(s, config).expect("persistent arm construction");
            run_seed_b(&arm, s, &mut lat, |req, succ, _| arm.observe_outcome(req, succ))
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
    {
        let arm = PersistentAifArm::new(start, config).expect("persistent arm construction");
        let mut warm = Vec::new();
        let _ = run_seed_b(&arm, start, &mut warm, |req, _bits, success| {
            arm.observe_outcome(req, &[success; 8]);
        });
    }
    let mut lat = Vec::new();
    let results = (start..end)
        .map(|s| {
            let arm = PersistentAifArm::new(s, config).expect("persistent arm construction");
            run_seed_b(&arm, s, &mut lat, |req, _bits, success| {
                arm.observe_outcome(req, &[success; 8]);
            })
        })
        .collect();
    (results, lat)
}

/// Run a stateless arm (scalar / magnitude) over the Scope-B seed range
/// `start..end` with no outcome hook, capturing act streams. Warm-up (on `start`)
/// discarded.
fn stateless_battery_range(
    make: impl Fn() -> Box<dyn CoalitionDecisionPolicy>,
    start: u64,
    end: u64,
) -> (Vec<SeedResultB>, Vec<f64>) {
    {
        let p = make();
        let mut warm = Vec::new();
        let _ = run_seed_b(&*p, start, &mut warm, |_, _, _| {});
    }
    let mut lat = Vec::new();
    let p = make();
    let results = (start..end)
        .map(|s| run_seed_b(&*p, s, &mut lat, |_, _, _| {}))
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
    {
        let arm = PersistentAifArm::new(start, config).expect("persistent arm construction");
        let wrapper = MarginE1 {
            arm: &arm,
            join_delta: jd,
            leave_delta: ld,
            tap: None,
        };
        let mut warm = Vec::new();
        if degraded {
            let _ = run_seed_b(&wrapper, start, &mut warm, |req, _bits, success| {
                arm.observe_outcome(req, &[success; 8]);
            });
        } else {
            let _ = run_seed_b(&wrapper, start, &mut warm, |req, bits, _| {
                arm.observe_outcome(req, bits);
            });
        }
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
            if degraded {
                run_seed_b(&wrapper, s, &mut lat, |req, _bits, success| {
                    arm.observe_outcome(req, &[success; 8]);
                })
            } else {
                run_seed_b(&wrapper, s, &mut lat, |req, bits, _| {
                    arm.observe_outcome(req, bits);
                })
            }
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
}
