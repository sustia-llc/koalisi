# A/B report: K4-v4 — persistent-agent multimodal AIF arm (koalisi #44)

_Registered run 2026-07-17, `examples/strategy_comparison.rs` Part 4c (release,
`--features decision,magnitude`), governed by `docs/prereg-K4-v4-persistent-aif.md`
including Amendments 1–2 (posted to #44 before implementation/run). Engine
`aif-v0.11.0` (79da34f); arm commit 943d139. 30 seeds, Scope B (reliability contest),
`PRIMARY_B = success_rate × mean_cov_eff`._

## VERDICT: `FALSIFIED (persistence)`

- **H1 (gap closed): FAIL** — mag median 0.2818 ≥ 1.25 × pers median 0.0326 (0.0407).
- **H2 (mechanism): FAIL** — pers median 0.0326 < 0.129427 (1.25 × scalar 0.1035), and
  pers strictly superior to scalar on only **2/30** seeds (needed ≥ 18).
- **S1 (act divergence, non-gating):** pers differs from scalar on **30/30** seeds —
  the arm **genuinely escapes the v3 decision-equivalence theorem** (the first AIF arm
  to do so; v3's mm arm diverged on 0/30). The falsification is *performance*, not
  theorem-collapse.
- **S2 (churn, non-gating):** pers 20.00 vs scalar 113.00 — the arm is far more
  selective (small, stable coalitions), but too selective to cover.
- **Latency (record-only):** pers 852.077 µs/decision (measured this run). For scale:
  the scalar arm measured 2.832 µs in the K4 v1/v2 report (**prior run**, same
  hardware — Part 4c does not re-measure scalar latency, so the ~300× ratio is
  cross-run and indicative only). Each pers decision is an MMP × PrecisionDynamics ×
  4-policy POMDP with a replayed 2-outcome window. Expected; no latency criterion
  registered.

**Regression gate (run validity): PASS** — `aif-scalar` Scope-B median ≡ 0.1035
(`docs/baseline-aif-scalar-scope-b.md`) and `mag` ≡ 0.2818
(`docs/ab-report-feedback-arm-k4-v2.md`), asserted in-code; all prior Parts
(1/2/3/3b/4) byte-identical.

## Per-seed rows

See the run output (committed alongside; Part 4c table): pers beats scalar only on
seeds 17 and 23; pers posts 0.0000 on seeds 6 and 14 (no successful completions).

## Exploratory E4–E8 (single toggles off the registered arm; no verdicts)

| condition | median PRIMARY_B | churn median |
|-----------|----------------:|-------------:|
| E4 `TrialBoundary::PerTask` | 0.0326 | 20.00 |
| E5 learning off | **0.1035** | 113.00 |
| E6 dynamics off (MeanField query, fixed γ = 16) | **0.4042** | 210.00 |
| E7 novelty off | 0.1342 | 112.50 |
| E8 `initial_precision_b` ∈ {1, 4, 16} | 0.0326 (all) | 20.00 (all) |

## Mechanism analysis (reported, not tuned)

1. **Why the registered arm fails: the E2 machinery, not the E1 lever** (analysis —
   per-decision posteriors and join timing were **not separately instrumented** in
   this run; the causal story below is the hypothesis most consistent with the
   ablation spread, not a measured finding). Under `PrecisionDynamics` γ starts at
   1/β₀ = 1.0 vs the fixed γ = 16 (documented engine behavior), which plausibly
   flattens query policy posteriors toward 0.5; with the join rule at
   `p(join) > 0.5` this is consistent with the observed pattern — churn 20 (tiny
   coalitions), low coverage, PRIMARY_B collapse — and with E6 (removing exactly the
   dynamics/MMP toggle) recovering 0.4042. Alternative contributors (the novelty
   join-bias, interaction with α = 8 marginalization) are not excluded; E7's partial
   recovery (0.1342) suggests the novelty bias is a real second factor. E8's total
   insensitivity (identical medians across a 16× pB-concentration sweep) shows the
   B-novelty concentration is not an active ingredient either way. E4 ≡ registered
   shows the trial boundary is irrelevant under these dynamics.
2. **E5 is the theorem-recovery sanity**: with learning off the persistent arm's
   median and churn reproduce scalar's *exactly* (0.1035 / 113.00) — without learned
   per-bit structure the arm collapses back to coverage-equivalence, precisely as the
   v3 theorem predicts. The pipeline is sound; the registered configuration is what
   underperforms.
3. **E6 is the headline exploratory observation**: learned per-bit precisions
   (the E1 lever, fed by the persistent world model) driving plain fixed-γ MeanField
   queries posts **0.4042 — 3.9× scalar and 1.43× above magnitude's 0.2818** (churn
   210, the thrash of an aggressive learner). This is exploratory, non-gating, and
   carries **no verdict**; per the prereg's own discipline it would need a fresh
   pre-registration (new arm shape: E1-only persistent AIF) to be claimed. It is the
   direct analog of how v3's E2 observation seeded this v4.
4. **What died and what didn't.** Dead: the registered E1+E2+B-novelty *stack* — MMP +
   γ/β dynamics + novelty at the decision layer subtract value on this battery
   (each single-toggle removal helps: E6 ≫ E7 > registered). Alive and now
   evidenced: persistence itself (S1 30/30 divergence) and outcome-learned per-bit
   precision (E5 vs E6 spread = the entire effect).

## Interpretation (per the prereg's pre-committed commitments)

`FALSIFIED (persistence)`: the registered E1/E2/B-novelty stack is exhausted for this
battery; #44 closes with the negative result and the AIF track exits the K4 quality
contest as registered — magnitude's quality dominance (v2) stands, now against four
successive AIF/feedback challengers. The scalar `competence_efe` bridge remains the
supported coalition-value primitive for semantics (tira #1 scoping). The E6
observation is recorded as exploratory data only; whether to pre-register an E1-only
v5 arm is a new decision outside this registration's commitments.

## Provenance

- Prereg: `docs/prereg-K4-v4-persistent-aif.md` (+ Amendments 1–2, posted to #44
  before implementation and run respectively).
- Baselines: `docs/baseline-aif-scalar-scope-b.md` (scalar Scope-B, pre-frozen),
  `docs/ab-report-feedback-arm-k4-v2.md` (mag Scope-B).
- Engine: tira `aif-v0.11.0` (0.10.0 seed API + B-novelty; 0.10.1 read accessors;
  0.11.0 `initial_pa`/`initial_pb` count injection — each cut for this arm).
- Implementation: `src/decision/aif_persistent_policy.rs` (943d139); the
  decision-dead gap on aif ≤ 0.10.1 and its resolution are documented in Amendment 2.
