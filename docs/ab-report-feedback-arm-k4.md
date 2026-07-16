# koalisi #46 — feedback-weighted arm vs magnitude (K4 battery)

_2026-07-16 · catgraph backend · release build · base calculator `SynergisticCalculator` · `ThresholdPolicy` thresholds 0.0_

Confirmatory battery (prereg `docs/prereg-feedback-arm-k4.md`). Three arms — `mag` (`MagnitudePolicy`, frozen incumbent), `thr` (feedback-OFF `ThresholdPolicy<Synergistic>`), `fb` (`ThresholdPolicy<FeedbackCalculator<Synergistic>>`, `hw = fw = 0.5`, fresh store per seed) — over two scopes.

## Protocol

- **Shared grammar:** 30 seeds `0..30`, inline SplitMix64; pool `n ∈ [4,16]`, caps `k ∈ [1,4]` bits of an 8-bit universe, trust `20–99`; `T = 20` tasks, required `r ∈ [1,5]` bits; seeded Fisher–Yates arrival; bootstrap-first-arrival; one leave sweep; seed-0 warm-up discarded.
- **Scope A (null control):** i.i.d.; success ≡ `completed` (union of member caps covers `required`); PRIMARY = completion_rate × mean_cov_eff (the committed Part 2 metric).
- **Scope B (contest):** per-agent hidden reliability `ρ_i` (bimodal: reliable `ρ=0.05` w.p. 0.7, else flaky `ρ=0.4`) + a pre-drawn arm-independent `perf[t][i]` matrix (`perform` w.p. `1−ρ_i`); success ≡ `completed AND all final members performed`; PRIMARY_B = success_rate × mean_cov_eff.
- **Feedback write-back:** `fb` records `success` (0/1) for the final coalition once per task, AFTER the leave sweep; `FeedbackStore::new(1.0)` ⇒ any non-success is a failure. `mag`/`thr` record nothing.

## Scope A — i.i.d. null control

| seed | n | mag_primary | thr_primary | fb_primary | mag_churn | thr_churn | fb_churn |
|-----:|--:|------------:|------------:|-----------:|----------:|----------:|---------:|
| 0 | 13 | 0.3407 | 0.0769 | 0.0769 | 4 | 0 | 0 |
| 1 | 10 | 0.5462 | 0.1000 | 0.1000 | 7 | 0 | 0 |
| 2 | 12 | 0.3118 | 0.0833 | 0.0833 | 15 | 0 | 0 |
| 3 | 13 | 0.4767 | 0.0769 | 0.0769 | 10 | 0 | 0 |
| 4 | 11 | 0.3847 | 0.0909 | 0.0909 | 6 | 0 | 0 |
| 5 | 14 | 0.4834 | 0.0714 | 0.0714 | 9 | 0 | 0 |
| 6 | 9 | 0.1169 | 0.0245 | 0.0245 | 11 | 0 | 0 |
| 7 | 15 | 0.4793 | 0.0667 | 0.0667 | 4 | 0 | 0 |
| 8 | 15 | 0.4185 | 0.0667 | 0.0667 | 11 | 0 | 0 |
| 9 | 8 | 0.1614 | 0.0398 | 0.0398 | 4 | 0 | 0 |
| 10 | 12 | 0.4560 | 0.0833 | 0.0833 | 8 | 0 | 0 |
| 11 | 9 | 0.5259 | 0.1111 | 0.1111 | 12 | 0 | 0 |
| 12 | 10 | 0.4575 | 0.1000 | 0.1000 | 7 | 0 | 0 |
| 13 | 14 | 0.5000 | 0.0714 | 0.0714 | 5 | 0 | 0 |
| 14 | 7 | 0.3455 | 0.0907 | 0.0907 | 10 | 0 | 0 |
| 15 | 13 | 0.4400 | 0.0769 | 0.0769 | 9 | 0 | 0 |
| 16 | 8 | 0.4171 | 0.1250 | 0.1250 | 6 | 0 | 0 |
| 17 | 5 | 0.0993 | 0.0494 | 0.0494 | 7 | 0 | 0 |
| 18 | 15 | 0.3700 | 0.0667 | 0.0667 | 11 | 0 | 0 |
| 19 | 13 | 0.5152 | 0.0769 | 0.0769 | 7 | 0 | 0 |
| 20 | 16 | 0.4537 | 0.0625 | 0.0625 | 8 | 0 | 0 |
| 21 | 13 | 0.6096 | 0.0769 | 0.0769 | 13 | 0 | 0 |
| 22 | 13 | 0.4395 | 0.0769 | 0.0769 | 9 | 0 | 0 |
| 23 | 11 | 0.5363 | 0.0909 | 0.0909 | 14 | 0 | 0 |
| 24 | 6 | 0.3028 | 0.1667 | 0.1667 | 10 | 0 | 0 |
| 25 | 11 | 0.5453 | 0.0909 | 0.0909 | 8 | 0 | 0 |
| 26 | 15 | 0.4625 | 0.0667 | 0.0667 | 7 | 0 | 0 |
| 27 | 14 | 0.4562 | 0.0714 | 0.0714 | 11 | 0 | 0 |
| 28 | 13 | 0.4384 | 0.0769 | 0.0769 | 4 | 0 | 0 |
| 29 | 14 | 0.3882 | 0.0714 | 0.0714 | 5 | 0 | 0 |

**Scope A medians:** mag 0.4469 · thr 0.0769 · fb 0.0769. fb strictly beats thr in 0/30 seeds.
_Registered prediction: fb ≈ thr and fb does NOT clear H1 (mag 0.4469 < 1.25 × fb 0.0769 is FAIL). A Scope-A fb win is a RED FLAG to investigate (metric/leakage bug), not a success._

## Scope B — reliability-structured contest

| seed | n | mag_primary | thr_primary | fb_primary | mag_churn | thr_churn | fb_churn |
|-----:|--:|------------:|------------:|-----------:|----------:|----------:|---------:|
| 0 | 13 | 0.2433 | 0.0077 | 0.0077 | 4 | 0 | 0 |
| 1 | 10 | 0.4600 | 0.0100 | 0.0100 | 7 | 0 | 0 |
| 2 | 12 | 0.2004 | 0.0000 | 0.0000 | 15 | 0 | 0 |
| 3 | 13 | 0.2860 | 0.0231 | 0.0231 | 10 | 0 | 0 |
| 4 | 11 | 0.2228 | 0.0136 | 0.0136 | 6 | 0 | 0 |
| 5 | 14 | 0.3222 | 0.0143 | 0.0143 | 9 | 0 | 0 |
| 6 | 9 | 0.0701 | 0.0041 | 0.0041 | 11 | 0 | 0 |
| 7 | 15 | 0.2929 | 0.0000 | 0.0000 | 4 | 0 | 0 |
| 8 | 15 | 0.3952 | 0.0200 | 0.0200 | 11 | 0 | 0 |
| 9 | 8 | 0.0922 | 0.0149 | 0.0149 | 4 | 0 | 0 |
| 10 | 12 | 0.2880 | 0.0083 | 0.0083 | 8 | 0 | 0 |
| 11 | 9 | 0.3713 | 0.0389 | 0.0389 | 12 | 0 | 0 |
| 12 | 10 | 0.3304 | 0.0400 | 0.0400 | 7 | 0 | 0 |
| 13 | 14 | 0.2750 | 0.0000 | 0.0000 | 5 | 0 | 0 |
| 14 | 7 | 0.2591 | 0.0065 | 0.0065 | 10 | 0 | 0 |
| 15 | 13 | 0.3575 | 0.0154 | 0.0154 | 9 | 0 | 0 |
| 16 | 8 | 0.2085 | 0.0250 | 0.0250 | 6 | 0 | 0 |
| 17 | 5 | 0.0993 | 0.0423 | 0.0423 | 7 | 0 | 0 |
| 18 | 15 | 0.2775 | 0.0000 | 0.0000 | 11 | 0 | 0 |
| 19 | 13 | 0.3149 | 0.0000 | 0.0000 | 7 | 0 | 0 |
| 20 | 16 | 0.3781 | 0.0219 | 0.0219 | 8 | 0 | 0 |
| 21 | 13 | 0.4171 | 0.0000 | 0.0000 | 13 | 0 | 0 |
| 22 | 13 | 0.2442 | 0.0038 | 0.0038 | 9 | 0 | 0 |
| 23 | 11 | 0.4767 | 0.0227 | 0.0227 | 14 | 0 | 0 |
| 24 | 6 | 0.2329 | 0.0667 | 0.0667 | 10 | 0 | 0 |
| 25 | 11 | 0.3938 | 0.0182 | 0.0182 | 8 | 0 | 0 |
| 26 | 15 | 0.3700 | 0.0267 | 0.0267 | 7 | 0 | 0 |
| 27 | 14 | 0.2683 | 0.0071 | 0.0071 | 11 | 0 | 0 |
| 28 | 13 | 0.1705 | 0.0000 | 0.0000 | 4 | 0 | 0 |
| 29 | 14 | 0.2283 | 0.0143 | 0.0143 | 5 | 0 | 0 |

**Scope B medians:** mag 0.2818 · thr 0.0140 · fb 0.0140.

### Scope B secondaries (record-only, non-gating)

| metric (median) | mag | thr | fb |
|-----------------|----:|----:|---:|
| success_rate | 0.5500 | 0.1500 | 0.1500 |
| churn | 8.00 | 0.00 | 0.00 |
| latency µs | 3.776 | 0.272 | 0.634 |

_Expected if H-main holds: fb success_rate > thr ≈ mag (feedback learns to avoid flaky members)._

## Confirmatory verdict (Scope B)

- **H1 (beats magnitude):** mag median 0.2818 < 1.25 × fb median 0.0140 → FAIL
- **H2 (mechanism):** fb median 0.0140 ≥ 1.25 × thr median 0.0140 (FAIL) AND fb strictly superior to thr in 0/30 seeds ≥ 18 (FAIL) → FAIL

**VERDICT (feedback arm, #46): FALSIFIED (feedback)**

_VALIDATED = H1 ∧ H2 · PARTIAL (mechanism only) = H2 ∧ ¬H1 · FALSIFIED = ¬H2. Thresholds (1.25×, 18/30) inherited from the K4-v2/v3 amendments; falsification is a legitimate result and nothing is tuned to flip it (koalisi #46)._

## E1 — weight sweep (exploratory, non-gating, Scope B)

`fb` `PRIMARY_B` median over 30 seeds by (history_weight `hw`, failure_weight `fw`). `(0, 0)` ≡ the feedback-off `thr` control; `(0.5, 0.5)` = the confirmatory arm.

| hw \ fw | fw=0.0 | fw=0.5 | fw=1.0 | fw=2.0 |
|--------:|-------:|-------:|-------:|-------:|
| hw=0.0 | 0.0140 | 0.0477 | 0.0584 | 0.0260 |
| hw=0.5 | 0.0140 | 0.0140 | 0.0362 | 0.0500 |
| hw=1.0 | 0.0140 | 0.0140 | 0.0140 | 0.0730 |
| hw=2.0 | 0.0140 | 0.0140 | 0.0140 | 0.0140 |

## Interpretation

The verdict is a genuine falsification, not a wiring bug — the E1 sweep proves the
mechanism is live: every failure-dominant cell moves `PRIMARY_B` off the `0.0140` control
(`(hw=0, fw=1) = 0.0584`, `(hw=1, fw=2) = 0.0730`), and the `(0.5, 0.5)` cell reproduces the
confirmatory `fb` median exactly.

Two things kill the registered arm:

1. **The balanced `hw = fw = 0.5` weighting cancels in the full-join regime.** At
   `ThresholdPolicy` threshold 0 with `SynergisticCalculator`, every marginal is positive,
   so every agent joins and none leaves (`thr`/`fb` churn = 0). In that regime each member
   accrues history and failures together (`history ≈ failures` per agent), and the balanced
   marginal `+0.5·25·history − 0.5·25·failures` nets to ≈ 0 — `fb` never diverges from `thr`
   (0/30 seeds). Feedback can only bite by declining an agent, which needs the failure term
   to push a marginal negative; only the failure-dominant cells (`fw > hw`) do that.

2. **Even the best cell cannot reach magnitude.** The strongest sweep cell (`0.0730`) is
   still ~4× below `mag` (`0.2818`). Magnitude wins by being *selective* (churn 8, small
   high-`cov_eff` coalitions); `thr`/`fb` join the whole pool, so their `cov_eff`
   (÷ member count) is structurally low regardless of the reliability signal. Feedback
   reshapes *which* agents are in the full pool at the margin, but does not induce the
   selectivity that drives magnitude's quality.

The reliability signal is real and orthogonal to diversity/coverage — `fb`'s Scope-B
`success_rate` should exceed `thr`'s once the weighting doesn't cancel — but on this
`ThresholdPolicy`-at-0 base it does not translate into `PRIMARY_B` dominance. Filed as
follow-ups (each a new hypothesis, own pre-registration): a **selective base** (a positive
`join_threshold`, so feedback can gate membership rather than only reorder a full pool) —
[#48](https://github.com/sustia-llc/koalisi/issues/48); a **failure-weighted** point
(`hw = 0, fw = 1`) — [#49](https://github.com/sustia-llc/koalisi/issues/49).
`#41`'s calculator is not refuted: Scope A is the expected null (`fb ≈ thr`, 0/30) and the
`(0, 0)` sweep cell equals the `thr` control, confirming the feedback-off identity.

## Reproduce

```sh
cargo run --release --manifest-path Cargo.toml --target-dir /tmp/koalisi-target \
  --features decision,magnitude --example strategy_comparison
```

Part 3 (this report) prints after the frozen Part 2 (#7/#43) battery. Deterministic
(inline SplitMix64, no `rand`, no wall-clock); the Scope-A `mag` column reproduces
`docs/ab-report-K4-catgraph-evaluator.md` seed-for-seed (the regression gate).

