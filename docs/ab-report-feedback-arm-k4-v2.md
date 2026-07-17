# koalisi #48 — selective-base feedback arm vs magnitude (K4 battery v2)

_2026-07-17 · catgraph backend · release build · base calculator `SynergisticCalculator` ·
`join_threshold = 100.0` · `leave_threshold = 0.0` · weights `hw = 0, fw = 1` · prereg
`docs/prereg-feedback-arm-k4-v2.md`. Reproduce: `cargo run --release --features
decision,magnitude --example strategy_comparison` (Part 4)._

Confirmatory battery **decomposing magnitude's edge** into **selectivity** (`thr-selective`)
and **reliability-gating** (the `fb-selective` increment). Three arms — `mag`
(`MagnitudePolicy`, frozen incumbent), `thr-selective` (feedback-OFF
`ThresholdPolicy<Synergistic>` at `join_threshold = 100.0`, isolating selectivity),
`fb-selective` (`ThresholdPolicy<FeedbackCalculator<Synergistic>>` at `join_threshold =
100.0`, `hw = 0`, `fw = 1`, fresh store per seed) — over two scopes. In the tables the
`thr`/`fb` columns are the `thr-selective`/`fb-selective` arms.

## Protocol

- **Shared grammar:** 30 seeds `0..30`, inline SplitMix64; pool `n ∈ [4,16]`, caps
  `k ∈ [1,4]` bits of an 8-bit universe, trust `20–99`; `T = 20` tasks, required `r ∈ [1,5]`
  bits; seeded Fisher–Yates arrival; bootstrap-first-arrival; one leave sweep; seed-0 warm-up
  discarded.
- **Scope A (null control):** i.i.d.; success ≡ `completed` (union of member caps covers
  `required`); PRIMARY = completion_rate × mean_cov_eff.
- **Scope B (contest):** per-agent hidden reliability `ρ_i` (bimodal: reliable `ρ=0.05` w.p.
  0.7, else flaky `ρ=0.4`) + a pre-drawn arm-independent `perf[t][i]` matrix (`perform` w.p.
  `1−ρ_i`); success ≡ `completed AND all final members performed`; PRIMARY_B = success_rate ×
  mean_cov_eff.
- **Feedback write-back:** `fb-selective` records `success` (0/1) for the final coalition once
  per task, AFTER the leave sweep; `FeedbackStore::new(1.0)` ⇒ any non-success is a failure.
  `mag`/`thr-selective` record nothing.

## Regression gate (run validity)

- **`mag` Scope-A median** 0.4469 == 0.4469 (`docs/ab-report-feedback-arm-k4.md`) → PASS
- **`mag` Scope-B median** 0.2818 == 0.2818 (`docs/ab-report-feedback-arm-k4.md`) → PASS
- Parts 1–3 stdout byte-identical to the committed baseline (only machine-varying latency
  lines + the new trailing separator differ); Part-3 Scope-B mag/thr/fb medians
  0.2818 / 0.0140 / 0.0140 and the E1 weight-sweep cells (incl. `hw=1, fw=2 → 0.0730`)
  reproduced.

## Scope A — i.i.d. null control

| seed | n | mag_primary | thr_primary | fb_primary | mag_churn | thr_churn | fb_churn |
|-----:|--:|------------:|------------:|-----------:|----------:|----------:|---------:|
| 0 | 13 | 0.3407 | 0.1393 | 0.1393 | 4 | 0 | 0 |
| 1 | 10 | 0.5462 | 0.1395 | 0.1395 | 7 | 0 | 0 |
| 2 | 12 | 0.3118 | 0.1274 | 0.1274 | 15 | 0 | 0 |
| 3 | 13 | 0.4767 | 0.1169 | 0.1169 | 10 | 0 | 0 |
| 4 | 11 | 0.3847 | 0.1238 | 0.1189 | 6 | 0 | 0 |
| 5 | 14 | 0.4834 | 0.0970 | 0.1110 | 9 | 0 | 0 |
| 6 | 9 | 0.1169 | 0.0358 | 0.0471 | 11 | 0 | 0 |
| 7 | 15 | 0.4793 | 0.0937 | 0.0937 | 4 | 0 | 0 |
| 8 | 15 | 0.4185 | 0.1250 | 0.1250 | 11 | 0 | 0 |
| 9 | 8 | 0.1614 | 0.0541 | 0.0532 | 4 | 0 | 0 |
| 10 | 12 | 0.4560 | 0.1174 | 0.1174 | 8 | 0 | 0 |
| 11 | 9 | 0.5259 | 0.1269 | 0.1269 | 12 | 0 | 0 |
| 12 | 10 | 0.4575 | 0.1719 | 0.1954 | 7 | 0 | 0 |
| 13 | 14 | 0.5000 | 0.0911 | 0.0911 | 5 | 0 | 0 |
| 14 | 7 | 0.3455 | 0.1339 | 0.0707 | 10 | 0 | 0 |
| 15 | 13 | 0.4400 | 0.1261 | 0.1261 | 9 | 0 | 0 |
| 16 | 8 | 0.4171 | 0.1971 | 0.1971 | 6 | 0 | 0 |
| 17 | 5 | 0.0993 | 0.0545 | 0.0355 | 7 | 0 | 1 |
| 18 | 15 | 0.3700 | 0.0918 | 0.0918 | 11 | 0 | 0 |
| 19 | 13 | 0.5152 | 0.1078 | 0.1078 | 7 | 0 | 0 |
| 20 | 16 | 0.4537 | 0.0916 | 0.0916 | 8 | 0 | 0 |
| 21 | 13 | 0.6096 | 0.1272 | 0.1272 | 13 | 0 | 0 |
| 22 | 13 | 0.4395 | 0.1092 | 0.1092 | 9 | 0 | 0 |
| 23 | 11 | 0.5363 | 0.1161 | 0.2110 | 14 | 0 | 0 |
| 24 | 6 | 0.3028 | 0.2533 | 0.2533 | 10 | 0 | 0 |
| 25 | 11 | 0.5453 | 0.1531 | 0.1531 | 8 | 0 | 0 |
| 26 | 15 | 0.4625 | 0.1116 | 0.1116 | 7 | 0 | 0 |
| 27 | 14 | 0.4562 | 0.1155 | 0.1155 | 11 | 0 | 0 |
| 28 | 13 | 0.4384 | 0.1047 | 0.1047 | 4 | 0 | 0 |
| 29 | 14 | 0.3882 | 0.0988 | 0.0988 | 5 | 0 | 0 |

**Scope A medians:** mag 0.4469 · thr-selective 0.1165 · fb-selective 0.1162. fb-selective
strictly beats thr-selective in 4/30 seeds. Registered null holds (`thr-selective ≈
fb-selective`; neither clears H1 — `mag 0.4469 < 1.25 × 0.1162` is FAIL); no red flag.

## Scope B — reliability-structured contest

| seed | n | mag_primary | thr_primary | fb_primary | mag_churn | thr_churn | fb_churn |
|-----:|--:|------------:|------------:|-----------:|----------:|----------:|---------:|
| 0 | 13 | 0.2433 | 0.0418 | 0.0484 | 4 | 0 | 0 |
| 1 | 10 | 0.4600 | 0.0349 | 0.0948 | 7 | 0 | 0 |
| 2 | 12 | 0.2004 | 0.0191 | 0.0245 | 15 | 0 | 0 |
| 3 | 13 | 0.2860 | 0.0526 | 0.0783 | 10 | 0 | 0 |
| 4 | 11 | 0.2228 | 0.0196 | 0.0120 | 6 | 0 | 0 |
| 5 | 14 | 0.3222 | 0.0306 | 0.0440 | 9 | 0 | 0 |
| 6 | 9 | 0.0701 | 0.0119 | 0.0113 | 11 | 0 | 0 |
| 7 | 15 | 0.2929 | 0.0094 | 0.0533 | 4 | 0 | 0 |
| 8 | 15 | 0.3952 | 0.0813 | 0.1635 | 11 | 0 | 0 |
| 9 | 8 | 0.0922 | 0.0270 | 0.0355 | 4 | 0 | 0 |
| 10 | 12 | 0.2880 | 0.0235 | 0.1008 | 8 | 0 | 0 |
| 11 | 9 | 0.3713 | 0.0444 | 0.0623 | 12 | 0 | 0 |
| 12 | 10 | 0.3304 | 0.0995 | 0.1600 | 7 | 0 | 0 |
| 13 | 14 | 0.2750 | 0.0000 | 0.0647 | 5 | 0 | 0 |
| 14 | 7 | 0.2591 | 0.0096 | 0.0000 | 10 | 0 | 0 |
| 15 | 13 | 0.3575 | 0.0315 | 0.0730 | 9 | 0 | 0 |
| 16 | 8 | 0.2085 | 0.0493 | 0.0705 | 6 | 0 | 0 |
| 17 | 5 | 0.0993 | 0.0467 | 0.0355 | 7 | 0 | 1 |
| 18 | 15 | 0.2775 | 0.0230 | 0.0848 | 11 | 0 | 0 |
| 19 | 13 | 0.3149 | 0.0108 | 0.0000 | 7 | 0 | 0 |
| 20 | 16 | 0.3781 | 0.0733 | 0.1771 | 8 | 0 | 0 |
| 21 | 13 | 0.4171 | 0.0127 | 0.0203 | 13 | 0 | 0 |
| 22 | 13 | 0.2442 | 0.0109 | 0.0833 | 9 | 0 | 0 |
| 23 | 11 | 0.4767 | 0.0794 | 0.0283 | 14 | 0 | 0 |
| 24 | 6 | 0.2329 | 0.1200 | 0.0253 | 10 | 0 | 0 |
| 25 | 11 | 0.3938 | 0.0613 | 0.0386 | 8 | 0 | 0 |
| 26 | 15 | 0.3700 | 0.0614 | 0.1532 | 7 | 0 | 0 |
| 27 | 14 | 0.2683 | 0.0289 | 0.0251 | 11 | 0 | 0 |
| 28 | 13 | 0.1705 | 0.0000 | 0.0490 | 4 | 0 | 0 |
| 29 | 14 | 0.2283 | 0.0296 | 0.0574 | 5 | 0 | 0 |

**Scope B medians:** mag 0.2818 · thr-selective 0.0301 · fb-selective 0.0512.

### Scope B secondaries (record-only, non-gating)

| metric (median) | mag | thr-selective | fb-selective |
|-----------------|----:|--------------:|-------------:|
| success_rate | 0.5500 | 0.2500 | 0.1750 |
| churn | 8.00 | 0.00 | 0.00 |
| latency µs | 3.478 | 0.201 | 0.180 |

## Confirmatory verdict (Scope B)

- **H1 (beats magnitude):** mag median 0.2818 < 1.25 × fb-selective median 0.0512
  (= 0.0640) → **FAIL**.
- **H2 (mechanism beyond selectivity):** fb-selective median 0.0512 ≥ 1.25 × thr-selective
  median 0.0301 (= 0.0376) → PASS; AND fb-selective strictly superior to thr-selective in
  **21/30** seeds ≥ 18 → PASS → **PASS**.

**VERDICT (selective-feedback arm, #48): `PARTIAL (mechanism only)`**

_VALIDATED = H1 ∧ H2 · PARTIAL (selectivity only) = H1 ∧ ¬H2 · PARTIAL (mechanism only) =
H2 ∧ ¬H1 · FALSIFIED = ¬H1 ∧ ¬H2. Thresholds (1.25×, 18/30) inherited from the
K4-v2/v3/#46 amendments; nothing tuned to flip the verdict (koalisi #48)._

## E1 — selectivity threshold sweep (exploratory, non-gating, Scope B)

`thr-selective` (feedback-off) and `fb-selective` (`hw = 0, fw = 1`) `PRIMARY_B` + churn
medians over 30 seeds by `join_threshold`. The `join = 100.0` row equals the confirmatory
arms (sanity).

| join_threshold | thr-selective PRIMARY_B | fb-selective PRIMARY_B | thr churn (med) | fb churn (med) |
|---------------:|------------------------:|-----------------------:|----------------:|---------------:|
| 50.0 | 0.0140 | 0.0506 | 0.00 | 0.50 |
| 75.0 | 0.0141 | 0.0446 | 0.00 | 0.00 |
| 100.0 | 0.0301 | 0.0512 | 0.00 | 0.00 |
| 125.0 | 0.0906 | 0.0451 | 0.00 | 0.00 |
| 150.0 | 0.0937 | 0.0413 | 0.00 | 0.00 |

## Synthesis

**#46 root cause fixed, but the gap to magnitude stands.** The falsified #46 run joined the
whole pool (`join = 0`, churn 0), so feedback could only reorder arrivals. A positive
`join_threshold` restores selectivity: `thr-selective` and `fb-selective` both leave the
0.0140 full-join floor, and at the registered `join = 100` `fb-selective` (0.0512) beats
`thr-selective` (0.0301) on ≥18/30 seeds — **H2 passes, so failure-weighting captures a
reliability signal orthogonal to plain selectivity.** But neither arm approaches magnitude's
0.2818 (H1 fails ~5.5×): magnitude's small, high-`cov_eff` coalitions (churn 8, success_rate
0.55) remain the thing to beat.

**Where the mechanism bites (E1).** The feedback increment is not monotone in the base's
tightness. It is largest in a *middle* selectivity band: at `join ∈ {50, 75, 100}`
`fb-selective` > `thr-selective`, but by `join ∈ {125, 150}` pure selectivity **overtakes**
feedback (thr 0.0906/0.0937 vs fb 0.0451/0.0413). A very tight base already forms small
coalitions, and the `fw=1` penalty then evicts merely-*unlucky* good agents (a reliable
agent that failed a covered task still accrues a failure under the 0/1 `success` signal),
so failure-weighting starts *removing* value the tighter the base. The registered `join =
100` sits inside the band where feedback helps — chosen before the sweep was seen, so the
`PARTIAL (mechanism only)` verdict is not threshold-shopped.

**Reading.** Magnitude's dominance is **not** pure selectivity — feedback adds a genuine,
measurable reliability signal on top of a selective base (that is what H2 establishes). It
is simply not enough to close a 5.5× quality gap on this battery. `#41` is not refuted (the
calculator math is unit-proven; Scope A is the expected null). The reliability battery
remains the natural testbed for the persistent-agent AIF arm (#44), which is the next lever
to try against magnitude's selectivity.
