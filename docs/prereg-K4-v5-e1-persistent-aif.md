# Pre-registration: K4 v5 — E1-only persistent AIF arm, out-of-sample (koalisi #53)

_Registered 2026-07-17, **before any confirmatory run** on the registered seed range.
Lineage: #7 (v1/v2), #43 (v3 `FALSIFIED (multimodality)` — the decision-equivalence
theorem), #44 (v4 `FALSIFIED (persistence)` — registered E1+E2+B-novelty stack;
S1 proved genuine theorem escape; ablations isolated the E1 lever). Engine
`aif-v0.11.0`; arm implementation 943d139 (unchanged for v5). Changes require a
posted amendment on #53 before any run. Falsification is a legitimate result;
nothing may be tuned to flip it._

## Question

The v4 ablations decomposed the persistent arm exactly: with learning off it
reproduces the scalar bridge seed-for-seed (theorem recovery); with the E2 decision
machinery off (**E6**) it posted **0.4042 on seeds 0..30 — 1.43× magnitude's 0.2818**.
The motivating observation is exploratory and, because the battery is deterministic,
re-scoring the same seeds cannot confirm it. This registration is the out-of-sample
test.

**H-main: outcome-learned per-bit precision (the E1 lever) driving plain fixed-γ
MeanField queries closes the K4 quality gap vs magnitude on fresh instances.**

## Arms (confirmatory battery — Scope B, seeds 30..60, PRIMARY_B)

1. **`mag`** — `MagnitudePolicy` (t = 1). Frozen code; per-seed rows on 30..60
   computed in this run (deterministic given the seed).
2. **`aif-scalar`** — `AifDecisionPolicy::default()`. Frozen code; same.
3. **`aif-e1`** — the #44 `PersistentAifArm` (943d139, **no code changes**) with the
   **E6 configuration exactly as swept in the v4 exploratory battery**: persistent
   8-bit reliability world model (A1.2 anchors, `initial_pa` injection, learn_a +
   learn_b + learn_d, both novelty flags, MMP{2,10} on the persistent agent,
   `TrialBoundary::PerStream`, per-bit outcome observations, exact coverage-masked
   count injection per v4 Amendment 2) — with **queries running MeanField at fixed
   γ = 16, no `PrecisionDynamics`** (the E6 toggle). Same membership-factor query
   structure, replayed 2-outcome window, deterministic `action_probabilities`
   posterior decisions (join iff p > 0.5, leave iff p ≥ 0.5). No degrees of freedom:
   the configuration is pinned by the committed v4 code's E6 branch.

## Protocol

Instance generation, metrics, and decision-stream mechanics identical to the v4
Part 4c battery — only the seed range differs: **seeds 30..60** (30 fresh instances
from the same SplitMix64 generator; pool/task/ρ/perf draws deterministic per seed).
`PRIMARY_B = success_rate × mean_cov_eff`. One release run of
`examples/strategy_comparison.rs` (new Part 4d).

**Regression gate (run validity, not hypothesis):** every existing printed section
(Parts 1–4c on seeds 0..30, including the v4 Part 4c confirmatory table) must be
byte-identical to the committed 2026-07-17 run; `scalar` 0..30 median ≡ 0.1035 and
`mag` ≡ 0.2818 asserted in-code. Any drift invalidates the run.

**Determinism:** unchanged from v4 — no `act()` sampling anywhere; seeds are hygiene.

## Confirmatory criteria (v5)

Medians over seeds 30..60. Thresholds inherit the 1.25× / 60% family (v2→v4
comparability; not chosen post hoc). All baselines are this run's own 30..60 rows —
cross-seed-range comparisons (e.g. vs 0.2818) are reported as context only, never
scored.

- **H1 — gap closed**: `mag_primaryB_median(30..60) < 1.25 × e1_primaryB_median(30..60)`.
- **H2 — mechanism**: `e1_primaryB_median(30..60) ≥ 1.25 × scalar_primaryB_median(30..60)`
  **and** e1 strictly superior to scalar on ≥ 18/30 of the new seeds.
- **S1 — act divergence (non-gating)**: seeds (of 30..60) where e1's act stream
  differs from scalar's.
- **S2 — churn (non-gating)**: e1 vs scalar churn medians; the 0..30 E6 churn (210)
  flagged high — report whether the pattern persists.
- **Latency: record-only, non-gating.**

**Verdicts:**
- `VALIDATED (gap closed)` — H1 ∧ H2.
- `PARTIAL (mechanism only)` — H2 ∧ ¬H1.
- `FALSIFIED (E1)` — ¬H2 (the out-of-sample test failed; the 0..30 observation is
  attributed to seed-set variance).

## Exploratory conditions (non-gating, after the confirmatory battery)

- **X1 — novelty off** on 30..60 (the v4 E7 analog — second-largest ablation effect).
- **X2 — seeds 0..30 re-score** of the registered arm (comparability row only; must
  reproduce the v4 E6 numbers 0.4042 / 210 exactly, doubling as a determinism check).

## Interpretation commitments

- `VALIDATED`: the first arm to beat magnitude on quality — the K4 verdict becomes
  conditional: magnitude wins on churn/latency/simplicity, E1-persistent AIF on raw
  quality; arm choice moves to a cost-quality tradeoff discussion (new issue), and
  the churn gap (S2) is the first thing it must address.
- `PARTIAL`: the E1 mechanism is real out-of-sample but magnitude stays clearly
  ahead; recorded as engine-capability evidence; no further K4 AIF registrations
  from this lineage without a new mechanism.
- `FALSIFIED (E1)`: the 0..30 E6 number was seed-set luck; the AIF quality track
  closes entirely (v2 verdict final); the scalar bridge remains the supported
  semantics primitive.
