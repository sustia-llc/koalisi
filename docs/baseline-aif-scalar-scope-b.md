# Scalar-AIF Scope-B baseline (K4-v4 prereg)

Frozen per-seed `PRIMARY_B` + churn for the shipped scalar Active-Inference
bridge `AifDecisionPolicy::default()` run through the **Scope B** (reliability
contest) battery of `examples/strategy_comparison.rs`. This is the incumbent
baseline the K4-v4 preregistration (koalisi #44) scores its next arm against.

- **Run date:** 2026-07-17
- **aif pin:** `aif-v0.10.0` (`Cargo.toml`: `aif = { git = ..., tag = "aif-v0.10.0" }`)
- **koalisi commit basis:** `main` @ `2fdd894` (working tree otherwise clean;
  the baseline section is an additive, non-gating printout in
  `examples/strategy_comparison.rs`)
- **Arm:** `AifDecisionPolicy::default()` — the scalar `competence_efe` bridge;
  stateless per call, so all 30 seeds run directly (`store = None`), exactly as
  the `mag` arm does.
- **Scope B:** per-agent hidden reliability `ρ_i` (bimodal) + a pre-drawn
  arm-independent `perf[t][i]` matrix; success ≡ `completed AND all final
  members performed`; `PRIMARY_B = success_rate × mean_cov_eff`.
- **Determinism:** Scope-B instances are byte-identical to every other Scope-B
  arm — each comes from a fresh `generate_instance_b(seed)`, not shared state.
  Seeds `0..30`, seed-0 warm-up discarded.

Regenerate with:

```sh
cargo run --release --features decision,magnitude --example strategy_comparison
```

(the final section, "koalisi #44 — scalar-AIF Scope-B baseline").

## Per-seed rows

| seed | n | primary_B | churn |
|-----:|--:|----------:|------:|
| 0 | 13 | 0.1550 | 158 |
| 1 | 10 | 0.1800 | 98 |
| 2 | 12 | 0.0422 | 80 |
| 3 | 13 | 0.1896 | 141 |
| 4 | 11 | 0.0887 | 112 |
| 5 | 14 | 0.0967 | 131 |
| 6 | 9 | 0.0283 | 76 |
| 7 | 15 | 0.2383 | 189 |
| 8 | 15 | 0.1050 | 121 |
| 9 | 8 | 0.0774 | 84 |
| 10 | 12 | 0.1300 | 119 |
| 11 | 9 | 0.0912 | 82 |
| 12 | 10 | 0.1719 | 102 |
| 13 | 14 | 0.2800 | 178 |
| 14 | 7 | 0.0365 | 59 |
| 15 | 13 | 0.0800 | 114 |
| 16 | 8 | 0.1062 | 82 |
| 17 | 5 | 0.0258 | 33 |
| 18 | 15 | 0.0675 | 120 |
| 19 | 13 | 0.1367 | 153 |
| 20 | 16 | 0.1971 | 194 |
| 21 | 13 | 0.0350 | 83 |
| 22 | 13 | 0.1021 | 137 |
| 23 | 11 | 0.0448 | 67 |
| 24 | 6 | 0.0656 | 45 |
| 25 | 11 | 0.1425 | 90 |
| 26 | 15 | 0.1810 | 156 |
| 27 | 14 | 0.0479 | 107 |
| 28 | 13 | 0.1800 | 162 |
| 29 | 14 | 0.1583 | 168 |

**Medians:** `primary_B` **0.1035** · churn **113.00**.

## Cross-checks (same run, frozen sections unchanged)

The additive section did not perturb any pre-existing printed value:

- Part 2 (Scope A): `mag` median **0.4469**, `aif-scalar` / `aif-mm` median
  **0.1898**; `aif-mm` decision-equivalent to `aif-scalar` (seed-for-seed) →
  **FALSIFIED (multimodality)** (K4-v3).
- Part 3 (#46): Scope-A `mag` **0.4469**, Scope-B `mag` **0.2818**.
- Part 4 (#48): regression gate PASS (`mag` Scope-A 0.4469, Scope-B 0.2818);
  Scope-B `mag` **0.2818** · `thr-selective` **0.0301** · `fb-selective`
  **0.0512** → **PARTIAL (mechanism only)**.

For reference, the scalar-AIF Scope-B median (0.1035) sits between the
`mag` ceiling (0.2818) and the feedback/threshold arms (0.0140–0.0512) — it is
a distinct incumbent, not reproduced by any existing Scope-B arm.
