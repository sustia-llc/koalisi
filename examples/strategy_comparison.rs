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
//! the battery's non-dyadic couplings — catgraph #29, fixed in `v0.1.1`, which
//! koalisi now depends on; debug builds run clean.)
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
//! magnitude arm is a legitimate outcome and nothing is tuned to flip it.
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
use koalisi::algorithms::{AgentCapabilities, SynergisticCalculator};
use koalisi::decision::{
    AifDecisionPolicy, CoalitionDecisionPolicy, CouplingModel, Decision, DecisionContext,
    MagnitudePolicy, ThresholdPolicy,
};

/// Hardcoded pre-registration date — deterministic, never read from the clock.
const REPORT_DATE: &str = "2026-07-02";
/// Number of seeded instances (seeds `0..SEEDS`).
const SEEDS: u64 = 30;
/// Tasks per instance.
const TASKS: usize = 20;
/// Capability-universe width (bits `0..UNIVERSE_BITS`).
const UNIVERSE_BITS: u64 = 8;
/// Instances with `n <= ORACLE_MAX_N` are oracle-eligible (brute force ≤ 255
/// non-empty subsets).
const ORACLE_MAX_N: usize = 8;

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

/// Generate one seeded instance: the agent pool and the task stream. Called by
/// BOTH the arm runners and the oracle, guaranteeing byte-identical instances.
fn generate_instance(seed: u64) -> (Vec<Worker>, Vec<Task>) {
    let mut rng = SplitMix64::new(seed);

    let n = (4 + rng.next_u64() % 13) as usize;
    let agents: Vec<Worker> = (0..n)
        .map(|id| {
            let k = 1 + rng.next_u64() % 4;
            let caps = draw_distinct_bits(&mut rng, k);
            let trust = (20 + rng.next_u64() % 80) as u32;
            Worker { id, caps, trust }
        })
        .collect();

    let tasks: Vec<Task> = (0..TASKS)
        .map(|_| {
            let r = 1 + rng.next_u64() % 5;
            let required = draw_distinct_bits(&mut rng, r);
            let order = fisher_yates(&mut rng, n);
            Task { required, order }
        })
        .collect();

    (agents, tasks)
}

/// Per-seed result for one arm.
struct InstanceMetrics {
    seed: u64,
    n: usize,
    /// `completion_rate × mean_cov_eff` (stream-level product; see below).
    primary: f64,
    /// Total leave-sweep removals over the stream.
    churn: usize,
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
/// - PRIMARY = `completion_rate × mean_cov_eff` (stream-level product).
fn run_instance(
    policy: &dyn CoalitionDecisionPolicy,
    seed: u64,
    latencies: &mut Vec<f64>,
) -> InstanceMetrics {
    let (agents, tasks) = generate_instance(seed);
    let n = agents.len();

    let mut completed_count = 0usize;
    let mut cov_eff_sum = 0.0f64;
    let mut churn = 0usize;

    for task in &tasks {
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

        // Task metrics on the formed coalition.
        let union = members.iter().fold(0u32, |acc, &i| acc | agents[i].caps);
        let covered = (union & task.required).count_ones();
        if covered == task.required.count_ones() {
            completed_count += 1;
        }
        let cov_eff = if members.is_empty() {
            0.0
        } else {
            (f64::from(covered) / f64::from(task.required.count_ones())) / members.len() as f64
        };
        cov_eff_sum += cov_eff;
    }

    let completion_rate = completed_count as f64 / tasks.len() as f64;
    let mean_cov_eff = cov_eff_sum / tasks.len() as f64;
    InstanceMetrics {
        seed,
        n,
        primary: completion_rate * mean_cov_eff,
        churn,
    }
}

/// Run the full 30-seed battery for one arm, with a discarded seed-0 warm-up
/// first (so the measured latencies see warm caches and a warm allocator).
/// Returns the per-seed metrics and every measured per-decision latency (µs).
fn run_battery(policy: &dyn CoalitionDecisionPolicy) -> (Vec<InstanceMetrics>, Vec<f64>) {
    // Warm-up: full seed-0 instance, latencies discarded.
    let mut warm = Vec::new();
    let _ = run_instance(policy, 0, &mut warm);

    let mut latencies = Vec::new();
    let per_seed: Vec<InstanceMetrics> = (0..SEEDS)
        .map(|s| run_instance(policy, s, &mut latencies))
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
    // Two measured arms.
    let aif = AifDecisionPolicy::default();
    let mag = MagnitudePolicy::default();
    let (aif_seeds, aif_lat) = run_battery(&aif);
    let (mag_seeds, mag_lat) = run_battery(&mag);

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
                .map(|s| run_instance(&policy, s, &mut scratch).primary)
                .collect();
            (t, median(primaries))
        })
        .collect();

    print_report(&aif_seeds, &aif_lat, &mag_seeds, &mag_lat, &oracle, &sweep);
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn print_report(
    aif_seeds: &[InstanceMetrics],
    aif_lat: &[f64],
    mag_seeds: &[InstanceMetrics],
    mag_lat: &[f64],
    oracle: &[Option<f64>],
    sweep: &[(f64, f64)],
) {
    // Aggregates.
    let aif_primaries: Vec<f64> = aif_seeds.iter().map(|m| m.primary).collect();
    let mag_primaries: Vec<f64> = mag_seeds.iter().map(|m| m.primary).collect();
    let aif_primary_med = median(aif_primaries.clone());
    let mag_primary_med = median(mag_primaries.clone());
    let (aif_p_med, aif_p_iqr) = median_iqr(aif_primaries);
    let (mag_p_med, mag_p_iqr) = median_iqr(mag_primaries);
    let (aif_c_med, aif_c_iqr) = median_iqr(aif_seeds.iter().map(|m| m.churn as f64).collect());
    let (mag_c_med, mag_c_iqr) = median_iqr(mag_seeds.iter().map(|m| m.churn as f64).collect());
    let (aif_l_med, aif_l_iqr) = median_iqr(aif_lat.to_vec());
    let (mag_l_med, mag_l_iqr) = median_iqr(mag_lat.to_vec());

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
    println!("_{REPORT_DATE} · yamafaktory backend, pre-K1 · release build_");
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
    println!("| seed | n | aif_primary | mag_primary | aif_churn | mag_churn | oracle_primary |");
    println!("|-----:|--:|------------:|------------:|----------:|----------:|---------------:|");
    for i in 0..aif_seeds.len() {
        let a = &aif_seeds[i];
        let m = &mag_seeds[i];
        let oracle_cell = match oracle[i] {
            Some(o) => format!("{o:.4}"),
            None => "—".to_string(),
        };
        println!(
            "| {} | {} | {:.4} | {:.4} | {} | {} | {} |",
            a.seed, a.n, a.primary, m.primary, a.churn, m.churn, oracle_cell
        );
    }
    println!();

    // Aggregate table.
    println!("## Aggregates (median · IQR)");
    println!();
    println!("| metric | AIF | Magnitude |");
    println!("|--------|----:|----------:|");
    println!("| primary | {aif_p_med:.4} · {aif_p_iqr:.4} | {mag_p_med:.4} · {mag_p_iqr:.4} |");
    println!("| churn | {aif_c_med:.2} · {aif_c_iqr:.2} | {mag_c_med:.2} · {mag_c_iqr:.2} |");
    println!("| latency µs | {aif_l_med:.3} · {aif_l_iqr:.3} | {mag_l_med:.3} · {mag_l_iqr:.3} |");
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
    println!("**VERDICT: {verdict}**");
    println!();
    println!("_Falsification is a legitimate result; nothing was tuned to flip it (koalisi #7)._");
    println!();

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
         Debug builds run clean since the `catgraph-magnitude v0.1.1` dep \
         (catgraph #29 fixed the over-strict triangle `debug_assert` that \
         v0.1.0 tripped on this battery's non-dyadic couplings)._"
    );
}

fn pass(b: bool) -> &'static str {
    if b { "PASS" } else { "FAIL" }
}
