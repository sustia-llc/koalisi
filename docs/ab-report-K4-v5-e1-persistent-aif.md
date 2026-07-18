# A/B report: K4-v5 — E1-only persistent AIF arm, out-of-sample (koalisi #53)

_Registered run 2026-07-17, `examples/strategy_comparison.rs` Part 4d (release,
`--features decision,magnitude`), governed by `docs/prereg-K4-v5-e1-persistent-aif.md`
(committed + posted to #53 before the run). Arm: the #44 `PersistentAifArm` (943d139,
zero code changes) in the v4 E6 configuration — persistent 8-bit reliability world
model + exact count injection, queries MeanField at fixed γ = 16, no
`PrecisionDynamics`. Engine `aif-v0.11.0`. Confirmatory battery: **out-of-sample
seeds 30..60**, Scope B, `PRIMARY_B = success_rate × mean_cov_eff`; all thresholds
from this run's own 30..60 medians._

## VERDICT: `VALIDATED (gap closed)`

- **H1 (gap closed): PASS** — mag median 0.2720 < 1.25 × e1 median 0.4406 (= 0.5508).
  e1's median is **1.62×** magnitude's on the same fresh instances.
- **H2 (mechanism): PASS** — e1 0.4406 ≥ 0.158333 (1.25 × scalar 0.1267), and e1
  strictly superior to scalar on **30/30** seeds (needed 18).
- **S1 (act divergence, non-gating):** 30/30 vs scalar.
- **S2 (churn, non-gating):** e1 136.00 vs scalar 79.50 on 30..60 — elevated but
  materially below the 0..30 E6 figure (210) flagged in the prereg. (Magnitude's
  churn was not tabulated in Part 4d; its historical Scope-B churn is far lower —
  cross-context, unscored.)
- **Latency (record-only):** e1 63.296 µs/decision this run — ~13.7× faster than
  the v4 registered arm's 867.978 µs (**remeasured this same run** in the Part 4c
  reproduction; latency legitimately varies run-to-run and is not part of the
  byte-identity gate), still orders of magnitude above the µs-scale stateless arms
  (prior-run figures, cross-run context only).

**Run validity: PASS** — all Parts 1–4c byte-identical (scalar 0..30 ≡ 0.1035, mag ≡
0.2818 asserted in-code), and the **X2 determinism gate** re-scored the registered
arm on seeds 0..30 and reproduced the v4 E6 numbers **exactly** (0.4042 / 210.00,
asserted in-code).

## Result detail (seeds 30..60)

Medians: **e1 0.4406 · scalar 0.1267 · mag 0.2720**. Per-seed: e1 beats scalar on
30/30; e1 posts the highest of the three arms on the large majority of seeds (e.g.
0.6083 vs mag 0.3402 on seed 36). mag leads e1 on exactly two seeds: 59 (0.0945 vs
0.0576 — e1's worst seed) and 42 (0.2367 vs 0.2246, a narrow margin). Full table in
the run output.

## Exploratory (non-gating)

| condition | median PRIMARY_B | churn median |
|-----------|----------------:|-------------:|
| X1 novelty off (30..60) | 0.1308 | 82.00 |
| X2 registered re-score (0..30 ≡ v4 E6) | 0.4042 | 210.00 |

**X1 is the mechanism headline**: removing the novelty (parameter-info-gain) terms
from the queries collapses the arm to near-scalar level (0.1308 ≈ scalar 0.1267).
Combined with the v4 ablations (E5 learning-off ≡ scalar exactly), the winning
configuration is specifically **learned per-bit precisions + novelty-driven
epistemic joining at fixed γ** — neither lever suffices alone. (Ablation inference,
not per-decision instrumentation; the same hedge as the v4 report applies.)

## Interpretation (per the prereg's pre-committed commitments)

`VALIDATED (gap closed)`: **the first arm to beat magnitude on quality in the K4
lineage** (v1→v5). The K4 verdict becomes conditional: magnitude wins on churn,
latency, and simplicity; E1-persistent AIF wins on raw decision quality
(reliability-aware coalition selection — the signal magnitude is structurally blind
to). Per the commitment, arm choice now moves to a cost-quality tradeoff discussion
(new issue), and the churn gap (136 vs magnitude's historically near-zero thrash) is
the first thing that discussion must address. The v2 magnitude verdict is not
overturned — it was scoped to the arms then registered; this is a new arm winning a
new registration on fresh instances.

## Provenance

- Prereg: `docs/prereg-K4-v5-e1-persistent-aif.md` (caa7f8e, posted to #53 pre-run).
- Motivating exploratory observation: v4 E6, `docs/ab-report-K4-v4-persistent-aif.md`.
- Arm: `src/decision/aif_persistent_policy.rs` @ 943d139 (unchanged for v5); Part 4d
  wiring ec91014. Engine: tira `aif-v0.11.0` (79da34f).
- Baselines: in-run frozen-code rows on the same 30..60 instances; 0..30 gates
  `docs/baseline-aif-scalar-scope-b.md` + `docs/ab-report-feedback-arm-k4-v2.md`.
