# koalisi #7 — categorical-magnitude vs Active-Inference A/B report

_2026-07-02 · yamafaktory backend, pre-K1 · release build_

Pre-registered A/B harness (koalisi #7). AIF expected-free-energy arm
(`AifDecisionPolicy`) vs categorical-magnitude arm (`MagnitudePolicy`, t = 1).

## Protocol

- **Seeds:** 30 instances, seeds `0..30`, inline SplitMix64 (no `rand`).
- **Pool:** `n = 4 + next()%13` agents (n ∈ [4,16]); caps = k ∈ [1,4] distinct bits of an 8-bit universe; trust = 20 + next()%80.
- **Task stream:** T = 20 tasks; required = r ∈ [1,5] distinct bits of the universe.
- **Decision stream:** seeded Fisher–Yates arrival order (drawn once per task); first arrival joins unconditionally (bootstrap); subsequent arrivals via `should_join`; one leave sweep in arrival order via `should_leave`.
- **completed(task):** union of members' caps covers `required` fully.
- **coverage_eff(task):** (covered bits / required bits) / member_count, 0 if empty.
- **PRIMARY(seed):** completion_rate × mean_cov_eff (stream-level product).
- **Churn(seed):** total leave-sweep removals over the stream.

## Per-seed results

| seed | n | aif_primary | mag_primary | aif_churn | mag_churn | oracle_primary |
|-----:|--:|------------:|------------:|----------:|----------:|---------------:|
| 0 | 13 | 0.3100 | 0.3407 | 158 | 4 | — |
| 1 | 10 | 0.2600 | 0.5462 | 98 | 7 | — |
| 2 | 12 | 0.0506 | 0.3118 | 80 | 15 | — |
| 3 | 13 | 0.2042 | 0.4767 | 141 | 10 | — |
| 4 | 11 | 0.2071 | 0.3847 | 112 | 6 | — |
| 5 | 14 | 0.1450 | 0.4834 | 131 | 9 | — |
| 6 | 9 | 0.0425 | 0.1169 | 76 | 11 | — |
| 7 | 15 | 0.3467 | 0.4793 | 189 | 4 | — |
| 8 | 15 | 0.1167 | 0.4185 | 121 | 11 | — |
| 9 | 8 | 0.1161 | 0.1614 | 84 | 4 | 0.2770 |
| 10 | 12 | 0.2113 | 0.4560 | 119 | 8 | — |
| 11 | 9 | 0.1673 | 0.5259 | 82 | 12 | — |
| 12 | 10 | 0.2031 | 0.4575 | 102 | 7 | — |
| 13 | 14 | 0.3400 | 0.5000 | 178 | 5 | — |
| 14 | 7 | 0.0852 | 0.3455 | 59 | 10 | 0.4608 |
| 15 | 13 | 0.1467 | 0.4400 | 114 | 9 | — |
| 16 | 8 | 0.2550 | 0.4171 | 82 | 6 | 0.7333 |
| 17 | 5 | 0.0258 | 0.0993 | 33 | 7 | 0.1925 |
| 18 | 15 | 0.0900 | 0.3700 | 120 | 11 | — |
| 19 | 13 | 0.2562 | 0.5152 | 153 | 7 | — |
| 20 | 16 | 0.2687 | 0.4537 | 194 | 8 | — |
| 21 | 13 | 0.0612 | 0.6096 | 83 | 13 | — |
| 22 | 13 | 0.1896 | 0.4395 | 137 | 9 | — |
| 23 | 11 | 0.0627 | 0.5363 | 67 | 14 | — |
| 24 | 6 | 0.0984 | 0.3028 | 45 | 10 | 0.5667 |
| 25 | 11 | 0.1900 | 0.5453 | 90 | 8 | — |
| 26 | 15 | 0.2140 | 0.4625 | 156 | 7 | — |
| 27 | 14 | 0.0863 | 0.4562 | 107 | 11 | — |
| 28 | 13 | 0.3200 | 0.4384 | 162 | 4 | — |
| 29 | 14 | 0.2375 | 0.3882 | 168 | 5 | — |

## Aggregates (median · IQR)

| metric | AIF | Magnitude |
|--------|----:|----------:|
| primary | 0.1898 · 0.1585 | 0.4469 · 0.1087 |
| churn | 113.00 · 67.75 | 8.00 · 4.50 |
| latency µs | 1.440 · 0.136 | 4.226 · 4.056 |

_Latency: same hardware, both arms warm, sync path — the only machine-varying numbers in this report._

**Oracle regret** (n ≤ 8, 5 eligible seeds): AIF median 0.3757, Magnitude median 0.1156.

## Verdict

- Criterion 1 (non-inferiority): mag median 0.4469 ≥ 0.95 × aif median 0.1898 (PASS); mag strictly inferior in 0/30 seeds ≤ 40% (PASS). → PASS
- Criterion 2 (latency): mag median 4.226 µs < aif median 1.440 µs → FAIL

**VERDICT: FALSIFIED (latency)**

_Falsification is a legitimate result; nothing was tuned to flip it (koalisi #7)._

## t-sweep (exploratory, non-gating)

Magnitude at scales t ∈ {0.5, 1.0, 2.0, 10.0}. t = 1.0 sanity-checks the stable arm (median 0.4469). Example-only policy — the library arm is pinned to t = 1 (catgraph #22).

| t | magnitude primary median |
|----:|-------------------------:|
| 0.5 | 0.4490 |
| 1.0 | 0.4469 |
| 2.0 | 0.4468 |
| 10.0 | 0.4428 |

## Reproduce

```sh
cargo run --release --manifest-path Cargo.toml --target-dir /tmp/koalisi-target \
  --features decision,magnitude --example strategy_comparison
```

_Release build required for the latency criterion (optimized code). Debug builds run clean since the `catgraph-magnitude v0.1.1` dep (catgraph #29 fixed the over-strict triangle `debug_assert` that v0.1.0 tripped on this battery's non-dyadic couplings)._
