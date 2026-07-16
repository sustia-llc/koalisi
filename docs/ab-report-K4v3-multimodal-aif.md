# koalisi #43 Part 2 — K4 v3 rematch report: multi-modality AIF arm

_2026-07-16 · aif-v0.9.0 pin · three-arm battery (`mag` / `aif-scalar` / `aif-mm`) ·
release build. Governed by the pre-registration
[`prereg-K4v3-multimodal-aif.md`](prereg-K4v3-multimodal-aif.md) (koalisi eb63335,
committed before implementation). Lineage: #7 v1 → v2 → this v3._

## VERDICT (K4-v3): FALSIFIED (multimodality)

- **H1 (gap closed):** mag median 0.4469 < 1.25 × mm median 0.1898 → **FAIL**
- **H2 (mechanism):** mm median 0.1898 ≥ 1.25 × scalar median 0.1898 → **FAIL**;
  mm strictly superior to scalar in 0/30 seeds (≥ 18 required) → **FAIL**
- **S1 (churn, secondary):** mm churn median 113.00 ≤ 0.5 × scalar 113.00 → **FAIL**
- **Latency (record-only):** mm median 4.289 µs vs scalar 2.793 µs — 1.54× (grows with
  modality count r ∈ [1,5], as anticipated in the registration).

**Why — a decision-equivalence theorem, not a near-miss.** The registered `aif-mm` arm
is decision-equivalent to `aif-scalar`: per-seed primary **and** churn match
seed-for-seed on all 30 seeds. With binary union coverage and symmetric per-modality
preferences over a deterministic-B bridge, each required bit contributes either
`G_cov = competence_efe(1)` or `G_unc = competence_efe(0)` (unit-anchored at
0.215 / 1.204), so the multimodal `G = k·G_cov + (r−k)·G_unc` is **affine in the
covered-bit count k** — exactly the information the scalar coverage fraction `k/r`
carries. Both arms' join/leave rules depend only on `sign(ΔG)` at `join_margin = 0`,
so every act is identical. Structure enters the *value* (G magnitudes, latency) but
not the *decision*. Characterized by a committed test
(`aif_mm_policy::tests::mm_and_scalar_agree_on_acts`); reported, not tuned.

**Interpretation (per the registration's commitments):** the v2 verdict stands —
magnitude's quality dominance is unrefuted. The scalar bridge's weakness on this
battery is **not** cured by re-expressing the same union-coverage information as
modalities; the registered hypothesis attributed magnitude's win to a representation
gap that, at margin 0 with binary coverage, does not exist. Attention shifts to
koalisi #41's feedback calculator as the third baseline, and any future AIF quality
claim must come from a regime where per-bit structure is *decision-relevant*, not just
representable — see Exploratory disposition below.

## Regression gate (run validity): PASS

The `mag` and `aif-scalar` per-seed rows (primary, churn, oracle) reproduce
[`ab-report-K4-catgraph-evaluator.md`](ab-report-K4-catgraph-evaluator.md)
**seed-for-seed** (all 30 rows; latency is, as always, the only machine-varying
column). The v1/v2 scoring blocks reproduce their recorded verdicts —
`FALSIFIED (latency)` / `VALIDATED (B)` — unchanged.

## Exploratory disposition (E1–E3)

- **E3 (r-normalized value):** resolved analytically, no run needed — `−G/r` is a
  positive per-task rescaling, decisions depend on `sign(ΔG)`, so acts are identical
  by the same theorem. (The registration itself noted decisions are unaffected.)
- **E1 (online learning) and E2 (stochastic B + MMP + PrecisionDynamics, depth 2):
  deferred to a follow-up issue rather than run here.** Honest reason: both are
  underspecified for this arm as registered — the mm arm constructs a *fresh* POMDP
  per decision, while `learn_a` (E1) and β-persistence (E2) are only meaningful with
  an agent that *persists across the task's decision stream* (trial semantics,
  `reset_window` boundaries). Running a hasty per-decision variant would measure
  nothing. E2 is the identified lever — a live exact-MI info-gain term is
  non-monotone in per-bit structure, which is precisely what the equivalence theorem
  says the registered arm lacks — but it needs a persistence design, which deserves
  its own registration, not an improvised exploratory run.

## The registered battery (three arms)

Protocol identical to the committed K4 battery (seeds 0..30, SplitMix64, pool
n ∈ [4,16], caps k ∈ [1,4] of 8 bits, T = 20 tasks, r ∈ [1,5] required bits,
seeded arrival order, bootstrap-first-join, one leave sweep;
`PRIMARY = completion_rate × mean_cov_eff`; oracle brute force for n ≤ 8).

### Per-seed results

| seed | n | mag_primary | scalar_primary | mm_primary | mag_churn | scalar_churn | mm_churn | oracle_primary |
|-----:|--:|------------:|---------------:|-----------:|----------:|-------------:|---------:|---------------:|
| 0 | 13 | 0.3407 | 0.3100 | 0.3100 | 4 | 158 | 158 | — |
| 1 | 10 | 0.5462 | 0.2600 | 0.2600 | 7 | 98 | 98 | — |
| 2 | 12 | 0.3118 | 0.0506 | 0.0506 | 15 | 80 | 80 | — |
| 3 | 13 | 0.4767 | 0.2042 | 0.2042 | 10 | 141 | 141 | — |
| 4 | 11 | 0.3847 | 0.2071 | 0.2071 | 6 | 112 | 112 | — |
| 5 | 14 | 0.4834 | 0.1450 | 0.1450 | 9 | 131 | 131 | — |
| 6 | 9 | 0.1169 | 0.0425 | 0.0425 | 11 | 76 | 76 | — |
| 7 | 15 | 0.4793 | 0.3467 | 0.3467 | 4 | 189 | 189 | — |
| 8 | 15 | 0.4185 | 0.1167 | 0.1167 | 11 | 121 | 121 | — |
| 9 | 8 | 0.1614 | 0.1161 | 0.1161 | 4 | 84 | 84 | 0.2770 |
| 10 | 12 | 0.4560 | 0.2113 | 0.2113 | 8 | 119 | 119 | — |
| 11 | 9 | 0.5259 | 0.1673 | 0.1673 | 12 | 82 | 82 | — |
| 12 | 10 | 0.4575 | 0.2031 | 0.2031 | 7 | 102 | 102 | — |
| 13 | 14 | 0.5000 | 0.3400 | 0.3400 | 5 | 178 | 178 | — |
| 14 | 7 | 0.3455 | 0.0852 | 0.0852 | 10 | 59 | 59 | 0.4608 |
| 15 | 13 | 0.4400 | 0.1467 | 0.1467 | 9 | 114 | 114 | — |
| 16 | 8 | 0.4171 | 0.2550 | 0.2550 | 6 | 82 | 82 | 0.7333 |
| 17 | 5 | 0.0993 | 0.0258 | 0.0258 | 7 | 33 | 33 | 0.1925 |
| 18 | 15 | 0.3700 | 0.0900 | 0.0900 | 11 | 120 | 120 | — |
| 19 | 13 | 0.5152 | 0.2562 | 0.2562 | 7 | 153 | 153 | — |
| 20 | 16 | 0.4537 | 0.2687 | 0.2687 | 8 | 194 | 194 | — |
| 21 | 13 | 0.6096 | 0.0612 | 0.0612 | 13 | 83 | 83 | — |
| 22 | 13 | 0.4395 | 0.1896 | 0.1896 | 9 | 137 | 137 | — |
| 23 | 11 | 0.5363 | 0.0627 | 0.0627 | 14 | 67 | 67 | — |
| 24 | 6 | 0.3028 | 0.0984 | 0.0984 | 10 | 45 | 45 | 0.5667 |
| 25 | 11 | 0.5453 | 0.1900 | 0.1900 | 8 | 90 | 90 | — |
| 26 | 15 | 0.4625 | 0.2140 | 0.2140 | 7 | 156 | 156 | — |
| 27 | 14 | 0.4562 | 0.0863 | 0.0863 | 11 | 107 | 107 | — |
| 28 | 13 | 0.4384 | 0.3200 | 0.3200 | 4 | 162 | 162 | — |
| 29 | 14 | 0.3882 | 0.2375 | 0.2375 | 5 | 168 | 168 | — |

### Aggregates (median · IQR)

| metric | Magnitude | AIF scalar | AIF multimodal |
|--------|----------:|-----------:|---------------:|
| primary | 0.4469 · 0.1087 | 0.1898 · 0.1585 | 0.1898 · 0.1585 |
| churn | 8.00 · 4.50 | 113.00 · 67.75 | 113.00 · 67.75 |
| latency µs | 3.516 · 7.913 | 2.793 · 0.097 | 4.289 · 1.468 |

_Latency: same hardware, all arms warm, sync path — the only machine-varying numbers
in this report._

**Oracle regret** (n ≤ 8, 5 eligible seeds): AIF median 0.3757, Magnitude median 0.1156.

## Arm under test

`AifMmDecisionPolicy` / `MmEfeValueCalculator`
(`src/decision/aif_mm_policy.rs`): for required bits `R` (r = |R|), one 2×2
observation modality per bit at `p_b = 0.5 + (max_precision − 0.5)·cov_b`
(`cov_b ∈ {0,1}`, union coverage), per-modality preferences
`[success_preference, 1 − success_preference]`, single 2-state factor, deterministic
2-control B, uniform D, `AgentParams::default()` + α — via aif 0.9.0
`GenerativeModel`/`from_model`; value = `−expected_free_energy()`; join/leave rule
identical to the scalar arm at `join_margin = 0`. Unit-anchored to
`competence_efe(0)`/`competence_efe(1)` per modality and exact-additivity-tested.

## Reproduce

```sh
cargo run --release --manifest-path Cargo.toml --target-dir /tmp/koalisi-target \
  --features decision,magnitude --example strategy_comparison
```

_Deterministic given seeds; release build required for the latency columns._

_Falsification is a legitimate result; nothing was tuned to flip it. Criterion
history: v1 (2026-07-02) `FALSIFIED (latency)`; v2 amendment `VALIDATED (B)`; v3
(this run) `FALSIFIED (multimodality)` — each scored under criteria posted before
the run._
