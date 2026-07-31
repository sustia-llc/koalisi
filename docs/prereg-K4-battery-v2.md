# Pre-registration: K4 battery v2 — de-saturated regime (koalisi #61 / stack EQ1)

_Registered 2026-07-31, BEFORE any v2 implementation or run (posted to #61).
Owner-locked design decisions (2026-07-31, #61 design-lock comment): regime =
**both** (8-bit heterogeneous confirmatory core + 12-bit exploratory part);
de-saturation lever = **both, factorial** (exposed query γ × structure axis,
γ grid **{1, 4, 16}**); seeds = **120..150 confirmatory + 150..180 held out**
(90..120 stays soft-reserved); confirmatory status = **levers 1 + 2
confirmatory, lever 3 exploratory**; value model = **both** (size-normalized
TaskCoverage confirmatory, expected-outcome model exploratory). Born-on pins:
catgraph `v0.5.0` ×2, `aif-v0.11.0`, surrealdb-live-message `v0.2.1`
(mid-arc freeze holds; v0.17.0 baseline)._

## Motivation (evidence chain)

The v1→v6 K4 lineage produced three falsifications/nulls that the EQ1
hypothesis (#61) reads as **regime artifacts of the v1 battery**, not method
truths:

1. **Reliability rescales-but-never-reroutes** (gotcha 24, #57): with the v1
   `TaskCoverage` coefficients (full 100 / partial 15·bit / member cost 8·N)
   and `|required| ≤ 6`, full-coverage weight per unit reliability
   (`100/|required| ≥ 16.7`) exceeds the partial weight (15), so skipping a
   weak bit never pays and routing is **structurally impossible** — untested,
   not falsified. (At `|required| ≥ 7` skipping pays even at r ≡ 1 under the
   v1 coefficients — a coefficient artifact, which is why lever 1 requires the
   re-derived model below.)
2. **Score-space damping/hysteresis structurally inert** (Part 4f, #54
   Step 3): the fixed-γ=16 query posteriors saturate at ±0.5 (all join/leave
   score quantiles from p25 up exactly 0.5000), so no sub-0.5 margin can
   separate decisions. The NULL says nothing about margins in a de-saturated
   regime.
3. **Per-bit oracle signal not load-bearing** (Part 4e, gotcha 23): degraded
   whole-task success ≈ oracle (0.4381 vs 0.4406) — priced in ONE regime only.

The v1 battery draws `|required| ∈ 1..=5` of an 8-bit universe
(`UNIVERSE_BITS = 8`, worker caps 1..=4 bits) — every task sits below the
routing threshold and the γ=16 rail was never stressed. Battery v2 changes the
regime; it does NOT re-litigate #54 (B+D FINAL: mag = demonstrated default,
arm-E1 = capability evidence). v1 verdicts stay scoped to the v1 regime.

## Registered regime

### Part 5a — v2 core battery (confirmatory host for lever 2)

Additive new parts in `examples/strategy_comparison.rs` (release build; every
existing printed line byte-identical — the standing frozen-parts gate). Scope
B semantics unchanged (hidden ρ, `perf` matrix, `PRIMARY_B = success_rate ×
mean_cov_eff`, churn = leave-sweep removals).

- **v2 task draw**: `|required|` uniform on **2..=8** (v1: 1..=5), bits drawn
  distinct from the 8-bit universe; worker pool and caps draw unchanged
  (n ∈ 4..=16, k ∈ 1..=4). The v2 draw functions are NEW (additive); the v1
  draw functions are untouched.
- **Factorial cells**: γ ∈ **{1, 4, 16}** × regime ∈ **{v1-draw, v2-draw}** ×
  margin δ ∈ **{0, 0.15, 0.30}** (hysteresis h = 0 everywhere; h is
  exploratory-only in v2). Margin semantics = the Part 4f `MarginE1` wrapper
  (join requires `p > 0.5 + δ`), identity at δ = 0 asserted.
- **Arm config**: `PersistentAifConfig` gains **`query_gamma: Option<f64>`**,
  identity default `None` (= engine default γ = 16 under
  `StateInference::MeanField`; `Some(16.0)` must be bit-identical to `None`,
  asserted). New arm-config labels **arm-E1g1 / arm-E1g4 / arm-E1g16**;
  the registered **arm-E1** (v5) config is frozen and untouched.
- **Outcome signal**: confirmatory cells run the **degraded/L2 signal**
  (`observe_outcome(required, &[success; 8])` — the runtime-feasible #55
  contract). Oracle-signal twins of the same cells are **exploratory**
  (lever 3 pricing).
- **In-run baselines** on the same instances: `mag` (frozen
  `MagnitudePolicy::default()`) and `scalar` (frozen
  `AifDecisionPolicy::default()`) in both regimes — context rows, non-gating.
- Seeds **120..150** (warm-up on seed 120 discarded, per the range-battery
  convention). Per-cell fresh arm + per-seed factory, as always.

### Part 5b — reliability-routing test (confirmatory host for lever 1)

A population-search part (the #42 `search()` machinery + #57
`ReliabilityCoverage` shape), seeds **120..150**:

- **Value model (confirmatory)**: **`TaskCoverageV2`** — size-normalized:
  full-coverage bonus **100**, partial per-bit weight **`w(m) = 80/m`** where
  `m = |required|`, member cost **8·N** (unchanged). Properties (asserted in
  unit tests before the run): at r ≡ 1 the optimum covers all required bits at
  every m ∈ 2..=8 (`80/m < 100/m` strictly); under per-bit reliability
  weighting (the `ReliabilityCoverage` shape re-based on `TaskCoverageV2`
  coefficients), a sufficiently weak bit flips the optimum to skipping it.
- **Per-seed instance**: pool n ∈ 8..=16 workers (caps 1..=4 bits),
  `|required| = m` uniform on **{7, 8}**; **planted per-bit reliability**:
  one uniformly-chosen required bit b\* gets r[b\*] = **0.15**, all other
  required bits r = **0.9**. Confirmatory cells use the planted r directly
  (no learned beliefs — mechanism test). Learned-posterior twins (r̂ from a
  `PersistentAifState` fed by the outcome stream, the #57 `from_state` path)
  are **exploratory** (pipeline test; gotcha-24 recency caveats apply there).
- **Outcome measure**: `REAL(structure)` = expected `TaskCoverageV2` payoff
  under independent per-bit Bernoulli(r_b) success (failed bits count
  uncovered), computed in closed form — the realized-value yardstick that is
  NOT tautological w.r.t. either argmax.
- Both argmaxes per seed via `search()` at a pinned `PopulationConfig` (same
  seed for both, identical search budget; weighted vs unweighted differ only
  in the calculator).

### Part 5c — exploratory only (non-gating, printed after the verdicts)

- **12-bit widened slice**: universe 12 bits, `|required|` uniform 2..=12,
  worker caps 1..=6, γ = 4, δ = 0, degraded signal, seeds 120..150 — the
  koalisi-side `N_BITS` parameterization (persistent joint space 4096).
  Registered at the level of intent; plumbing details are implementation, and
  any deviation is recorded in the report, not silently absorbed.
- **Oracle-vs-degraded pricing** across the Part 5a v2-regime cells (lever 3).
- **Hysteresis h ∈ {0.15, 0.30}** at the best-performing (γ, δ) cell.
- **Expected-outcome value model** (success-probability semantics tied to the
  L2 `TaskOutcome` event) re-run of Part 5b, gated on its own gotcha-21
  degeneracy analysis (documented in the report).
- **Learned-posterior routing twins** of Part 5b (above).

## Confirmatory criteria (all from THIS run's 120..150 medians)

**H-S (lever 2, de-saturation — the score-space levers are live in a
de-saturated regime):** there EXISTS a cell (γ ∈ {1, 4}, regime = v2-draw,
δ > 0) such that, versus its own (same γ, regime = v2-draw, δ = 0) baseline:

- churn median ≤ **0.5 ×** the baseline churn median, AND
- `PRIMARY_B` median ≥ **0.9 ×** the baseline `PRIMARY_B` median, AND
- the churn reduction holds on ≥ **18/30** seeds (paired, same seed).

**H-R (lever 1, routing — reliability re-routes, not just rescales):** on
seeds 120..150, with planted reliabilities:

- **Sanity leg (run-invalidating if it fails)**: the unweighted
  `TaskCoverageV2` argmax covers all required bits (incl. b\*) on ≥ 27/30
  seeds — confirming the r ≡ 1 optimum is full coverage in this regime.
- the reliability-weighted argmax **skips b\*** (no block covers it) on
  ≥ **18/30** seeds, AND
- the weighted argmax's `REAL` is **strictly greater** than the unweighted
  argmax's `REAL` on ≥ **18/30** seeds, AND its median `REAL` ≥ **1.05 ×**
  the unweighted median.

**Verdict rule (pre-committed; per-lever, no cross-lever conjunction):**

- `VALIDATED (de-saturation)` = H-S in full; `FALSIFIED (de-saturation)` =
  anything less. If the (γ ∈ {1,4}, v2, δ = 0) score quantiles are STILL
  saturated at ±0.5 (recorded, non-gating), the falsification reads
  "γ is not the de-saturation lever", scoped accordingly.
- `VALIDATED (routing)` = H-R in full; `FALSIFIED (routing)` = anything less
  (with the sanity leg intact; a sanity-leg failure invalidates the run).
- **Headline claim discipline**: "regime artifact overturned" may be claimed
  for a lever only after its held-out replication (below) also passes.
  Without replication the claim is `VALIDATED (unreplicated)`.

Thresholds (18/30, 0.9×, 1.05×, 0.5×) inherit the v2→v6 family's 60%
consistency + margin conventions. Nothing is tuned post-hoc; the grids,
draws, coefficients, and bars above are locked before implementation.

**Held-out replication (150..180, pre-committed):** run ONLY for a lever whose
confirmatory hypothesis passes on 120..150, at the single winning cell
(H-S: the passing (γ, δ) cell; H-R: the registered cell), same bars,
fresh seeds. Pass ⇒ the headline claim; fail ⇒ `VALIDATED (unreplicated)`
stands with the replication failure recorded. 150..180 is consumed by this
use; if neither lever passes, it remains unconsumed and reserved.

## Run-validity gates (run-invalidating)

- **X-A (determinism anchor)**: arm-E1 (frozen v5 config: `query_gamma: None`,
  `query_dynamics: false` E6 form, oracle signal) on seeds **30..60**
  reproduces the #53 registered numbers exactly (median 0.4406 / churn
  136.00, asserted in-code).
- **X-B (identity gates)**: `query_gamma: Some(16.0)` ≡ `None` bit-for-bit on
  the X-A cell; the Part 5a δ = 0 margin wrapper ≡ unwrapped, per seed.
- **X-C (frozen parts)**: every existing printed line of Parts 1–4h
  byte-identical; all committed in-code asserts (scalar 0.1035, mag 0.2818,
  X2 0.4042/210.00, Part 4f identity) still pass.
- **X-D (H-R sanity leg)**: as stated in H-R.
- Latency is record-only, never gated.

## Pre-committed interpretation

- `VALIDATED (de-saturation)` ⇒ Part 4f's NULL is regime-scoped; score-space
  churn mitigation re-opens as a design axis for any future e1-lineage arm
  (its own registration). The #54 arm decision is unchanged.
- `FALSIFIED (de-saturation)` ⇒ the inertness claim generalizes beyond γ=16
  saturation at least across this γ/regime grid; state-based levers remain
  the only measured churn axis.
- `VALIDATED (routing)` ⇒ gotcha 24's rescale-not-reroute is coefficient- and
  size-scoped, not a property of reliability weighting; the #57
  `ReliabilityCoverage` slow-loop seam gains a measured routing regime (feeds
  the two-loop integration experiment, stack cross-cutting entry).
- `FALSIFIED (routing)` ⇒ reliability weighting does not route even where the
  coefficients permit it — a genuine negative for the slow-loop fitness
  story; gotcha 24 is strengthened from "structurally impossible" to
  "measured absent" in this regime.
- Lever 3 (oracle-vs-degraded) stays exploratory regardless of outcome; any
  confirmatory claim about signal fidelity needs its own registration.
- EQ-queue consequence: EQ1's landing (either way) is the fair-judgment
  baseline for EQ3/EQ4/EQ5 registrations (stack E-queue).

## Provenance

Owner decisions recorded on #61 (design-lock comment, 2026-07-31). Evidence
chain: gotcha 24 + `src/decision/reliability_value.rs` (#57),
`docs/k4-arm-choice-memo.md` + #54 Parts 4e/4f/4g (gotcha 23),
`docs/prereg-K4-v6-never-evict.md` + `docs/ab-report-K4-v6-never-evict.md`
(#56), `docs/per-bit-outcome-plumbing-design.md` (fidelity ladder),
`docs/ab-report-K4-v5-e1-persistent-aif.md` (#53, the arm-E1 anchor).
Engine pins unchanged from v0.17.0. Zero upstream features (EQ1 scope).
