# Pre-registration: K4 v3 rematch — multi-modality AIF arm (koalisi #43 Part 2)

_Registered 2026-07-16, **before any implementation or run** of the multimodal arm.
Lineage: #7 (v1 criteria, run 1 2026-07-02 → `FALSIFIED (latency)`; v2 amendment →
`VALIDATED (B)` on the committed battery, `docs/ab-report-K4-catgraph-evaluator.md`).
This document is the v3 amendment and governs the rematch. Changes require a posted
amendment on #43 **before** any run. Falsification is a legitimate result; nothing may
be tuned to flip it._

## Question

K4/K6 established that the categorical-magnitude arm beats the scalar-EFE AIF arm on
decision quality (primary median 0.4469 vs 0.1898, strictly superior 30/30 seeds, churn
8 vs 113). The standing hypothesis (ecosystem roadmap §1, tira#12 → koalisi#43): the gap
comes from the **diversity-blindness of the scalar bridge** — competence is a single
number, so member-overlap structure never reaches the generative model, while
magnitude's entire signal is effective diversity. aif 0.6.0's multi-modality surface
(now pinned at `aif-v0.9.0`) lets coverage structure enter G directly.

**H-main: a multi-modality AIF bridge closes the quality gap vs magnitude.**

## Arms (confirmatory battery)

Identical instances, streams, and decision rules; only the value model differs.

1. **`mag`** — `MagnitudePolicy` (t = 1), the incumbent quality winner. Frozen.
2. **`aif-scalar`** — `AifDecisionPolicy` as shipped (scalar coverage → `competence_efe`,
   `BridgeParams::default()`, `join_margin = 0`, `belief_weight = 0`). Frozen.
3. **`aif-mm`** — the new multimodal arm. Specification (no degrees of freedom left):
   - For a task requiring bits `R = {b_1..b_r}` (r ∈ [1,5]) and a candidate coalition
     `S`, build the 0.9.0 engine POMDP via `GenerativeModel`/`from_model`:
     - **One observation modality per required bit** (r modalities), each 2×2 with
       column j = `[p_b, 1−p_b]` in the exact `competence_efe` A-shape
       (tira `coalition.rs:168-179`).
     - **Per-bit precision** `p_b = 0.5 + (max_precision − 0.5) · cov_b` with
       `cov_b = 1` if any member of `S` covers bit `b`, else `0` (union coverage,
       binary — clones add no precision to an already-covered bit; specialists flip an
       uninformative modality to an informative one).
     - Single 2-state hidden factor, **deterministic B** (2 controls, the
       `transition_noise = 0` form), uniform D, per-modality preferences
       `[success_preference, 1−success_preference]` — all constants from
       `BridgeParams::default()` (`max_precision` 0.95, `success_preference` 0.9,
       `alpha` 8.0).
     - Engine defaults otherwise: `AgentParams::default()` + the arm's α (MeanField,
       depth 1, no learning, no precision dynamics). Value = `−expected_free_energy()`.
     - **G is the raw sum over the r modalities — deliberately not normalized by r.**
       Decisions are within-task comparisons, so the scale cancels; an r-normalized
       variant is exploratory E3, not confirmatory.
   - Decision rule **identical** to `aif-scalar`: join iff joining lowers G by more than
     `join_margin = 0`; leave when staying does not lower G. Same
     `CoalitionDecisionPolicy` seam, same bootstrap-first-arrival, same leave sweep.

## Protocol (unchanged from the committed battery — byte-identical instances)

Seeds 0..30, SplitMix64; pool n ∈ [4,16], caps k ∈ [1,4] distinct bits of the 8-bit
universe, trust 20–99; T = 20 tasks, required r ∈ [1,5] bits; seeded Fisher–Yates
arrival order drawn once per task; `PRIMARY(seed) = completion_rate × mean_cov_eff`;
churn = leave-sweep removals; oracle brute force for n ≤ 8; latency measured per arm,
release build, same hardware, sync path. Three measured arms + oracle in one run of
`examples/strategy_comparison.rs`.

**Regression gate (run validity, not hypothesis):** the `mag` and `aif-scalar` per-seed
rows (primary, churn) must reproduce `docs/ab-report-K4-catgraph-evaluator.md`
seed-for-seed. Any drift invalidates the run — fix the harness, do not touch criteria.

## Confirmatory criteria (v3)

Medians over the 30 seeds, computed exactly as in the committed report.

- **H1 — gap closed** (magnitude's v2-Path-B.1 clear-superiority test, re-applied
  against the new arm, must now FAIL): `mag_primary_median < 1.25 × mm_primary_median`.
- **H2 — mechanism** (multimodality, not luck): `mm_primary_median ≥ 1.25 ×
  scalar_primary_median` **and** mm strictly superior to scalar on ≥ 60% of seeds
  (≥ 18/30).
- **S1 — churn (secondary)**: `mm_churn_median ≤ 0.5 × scalar_churn_median`
  (clone-blindness removed should cut the join/leave thrash).
- **Latency: record-only, non-gating** (per #43: expected to grow ~linearly with r;
  report medians + IQR per arm and the mm/scalar ratio). No latency criterion — v1's
  Path-A speed route is not re-registered here.

**Verdicts:**
- `VALIDATED (gap closed)` — H1 ∧ H2.
- `PARTIAL (mechanism only)` — H2 ∧ ¬H1 (multimodality real, magnitude still clearly ahead).
- `FALSIFIED (multimodality)` — ¬H2 (structure in the model didn't beat the scalar bridge).
S1 is reported alongside whichever verdict obtains and does not change it.

## Exploratory conditions (non-gating, run after the confirmatory battery)

Each is a single toggle on the registered `aif-mm` arm; results reported in the
t-sweep style (table, no verdict):

- **E1 — online learning**: `learn_a = true` + `use_param_info_gain = true`
  (`initial_precision` = flat 1.0 per joint state; aif 0.8.0): the arm learns
  per-modality precisions from outcomes instead of trusting declared coverage; novelty
  drives information-seeking toward uncertain members.
- **E2 — precision dynamics**: `transition_noise = 0.1` + `StateInference::
  MarginalMessagePassing { horizon: 2, iters: 10 }` + `PrecisionDynamics::default()` +
  `policy_depth = 2` (aif 0.9.0; requires stochastic B — deterministic-B dynamics is
  provably inert, engine-tested; note `gamma` is ignored under dynamics, γ₀ = 1).
- **E3 — r-normalized value**: value = `−G/r` (only relevant if a cross-task
  calculator comparison is added; decisions themselves are unaffected).

## Interpretation commitments

- If `VALIDATED (gap closed)`: the K4 quality verdict is attributed to scalar-bridge
  diversity-blindness, not to EFE per se; the two-loop roadmap keeps both arms and the
  choice becomes latency/semantics-driven.
- If `PARTIAL` or `FALSIFIED`: magnitude's quality dominance stands (v2 verdict
  unrefuted); the multimodal bridge is still reported as engine capability evidence,
  and attention shifts to koalisi #41's feedback calculator as the third baseline.
- The 30-seed battery and thresholds (1.25×, 60%, 0.5×) are inherited from the v2
  amendment to keep verdicts comparable across K4 runs — not chosen post hoc.
