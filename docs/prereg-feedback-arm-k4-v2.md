# Pre-registration: selective-base feedback arm vs magnitude — K4 battery v2 (koalisi #48; absorbs #49)

_REGISTERED 2026-07-17 on #48 — posted **before any implementation or run** (git commit is
the immutable mirror; pending). Binding from this point: changes require a posted amendment
on #48 **before** any run. Lineage: #7 (v1/v2 criteria) → #43 K4-v3 (multimodal AIF,
`FALSIFIED (multimodality)`) → #46 (feedback arm, full-join base, `FALSIFIED (feedback)`) →
this. Decisions locked by owner 2026-07-17: base **Synergistic**, `join_threshold` **100.0**
(registered confirmatory; exploratory grid `{50,75,100,125,150}`), `leave_threshold` **0.0**
(justified below), weights **`hw=0, fw=1`** (failure-only; absorbs #49). Per the K4
falsification discipline: nothing may be tuned to flip a verdict; falsification is a
legitimate result. Changes require a posted amendment **before** any run._

## Question

#46 falsified the feedback arm, but root-caused *why*: `ThresholdPolicy<Synergistic>` at
`join_threshold = 0` joins the **entire pool** and never leaves (churn 0), so feedback could
only *reorder* arrivals, never *gate* membership — and the balanced `hw=fw=0.5` cancelled
(`history ≈ failures` per member). Magnitude's whole edge is **selectivity**: it forms small,
high-`cov_eff` coalitions (median churn 8, Scope-B PRIMARY 0.2818 vs the full-join arms'
0.0140). The open question:

**H-main: with a *selective* base (a positive `join_threshold`, so the coalition stays
small) plus a *failure-weighted* feedback signal that evicts flaky members, a feedback arm
can be both small AND reliable — and contest magnitude on the Scope-B reliability battery
(`PRIMARY_B = success_rate × mean_cov_eff`).**

This run cleanly **decomposes** magnitude's advantage into two levers:
- **selectivity** — isolated by the `thr-selective` control (positive threshold, no feedback);
- **reliability-gating** — the increment `fb-selective` adds on top via failure-weighting.

## Arms (both scopes)

Byte-identical instances/streams/decision rules across arms; only the value model + feedback
differ. Base calculator = **`SynergisticCalculator`** (matches Part 2 / #46; isolates the
threshold as the changed variable). Bootstrap-first-arrival, one leave sweep per task.

1. **`mag`** (frozen) — `MagnitudePolicy::default()` (t = 1). The catgraph-magnitude incumbent;
   its Scope-A/B rows are the regression gate.
2. **`thr-selective`** (feedback-off control) —
   `ThresholdPolicy::new(SynergisticCalculator, 100.0, 0.0)`. Isolates what **selectivity
   alone** buys over the falsified `join = 0` control.
3. **`fb-selective`** (new, headline) —
   `ThresholdPolicy::new(FeedbackCalculator::new(SynergisticCalculator, hw = 0.0, fw = 1.0,
   store), 100.0, 0.0)`. `store = FeedbackStore::new(1.0)` **fresh per seed**; outcomes written
   back **once per task, after the leave sweep**, so within-task join/leave decisions see a
   constant store.

**`leave_threshold = 0.0` — justification (not a free knob):** kept identical to #46/frozen so
the *only* registered changes are `join_threshold (0 → 100)` and weights `((0.5,0.5) → (0,1))`.
`should_leave` evicts iff `marginal_of_staying < 0`. Under `FeedbackCalculator(hw=0, fw=1)`
the member counters cancel (gotcha 19) and `marginal_of_staying(x) = base_marginal(x) −
25·failures(x)` — so a flaky member is evicted exactly when its accrued failures outweigh its
base contribution (`base_marginal(x) < 25·failures(x)`). The failure signal thus acts through
the *unchanged* leave gate; raising `leave_threshold` would confound selectivity with churn.

`aif-scalar`/`aif-mm` are not carried (orthogonal to the fb-vs-mag question; the frozen Part 2
rows remain available for continuity).

## Fitness signal (written to `record_outcome`)

Arm-external ground truth only — never the arm's own score. Members credited = the **final**
coalition after the leave sweep. Identical to #46:

- **Scope A confirmatory** — `completed` (0/1): union of member caps fully covers `required`.
- **Scope B confirmatory** — `success` (0/1): `completed` **and** all final members "performed"
  on this task. `FeedbackStore::new(1.0)` ⇒ a covered task that failed on reliability is a
  failure (an accrued `failures` count on every member of a failed coalition).

## Scope B — reliability structure (reused verbatim from #46, no new degrees of freedom)

Unchanged from the #46 registration and its committed `generate_instance_b`:

- **Per-agent hidden reliability** `ρ_i`, drawn once per seed after the caps/trust draw:
  bimodal — reliable (`ρ = 0.05`) w.p. `0.70`, else flaky (`ρ = 0.40`).
- **Per-(task, agent) performance matrix** `perf[t][i] = (draw < 1 − ρ_i)`, pre-drawn once per
  instance, **arm-independent** (drawn off the same seeded stream after the shared prefix, so
  the prefix stays byte-identical to Scope A and Part 2).
- **Realized success:** `success(S,t) = completed(S,t) AND (∀ i ∈ S: perf[t][i])`.
- **Primary (Scope B):** `PRIMARY_B(seed) = success_rate × mean_cov_eff`.

`mag` (diversity) and EFE (coverage) are blind to `perf`/`ρ`; only feedback can learn it.

## Protocol (shared; unchanged instance grammar)

Seeds `0..30`, `SplitMix64`; pool `n ∈ [4,16]`, caps `k ∈ [1,4]` distinct bits of the 8-bit
universe, trust `20–99`; `T = 20` tasks, required `r ∈ [1,5]` bits; seeded Fisher–Yates arrival
order per task; churn = leave-sweep removals; latency per arm, `--release`, sync path, seed-0
warm-up discarded. Store reset between seeds ⇒ the 30 instances stay independent. Reuses the
#46 `run_instance(policy, seed, scope, store, latencies)` harness and the `Arm { policy, store }`
per-seed factory — **no new library API**.

**Regression gate (run validity, not hypothesis):**
- On **Scope A** and **Scope B**, `mag` per-seed `primary`/`churn` must reproduce the committed
  `docs/ab-report-feedback-arm-k4.md` seed-for-seed (Scope-B mag median 0.2818, Scope-A 0.4469).
- `thr-selective` with `hw = fw = 0` must equal a plain `ThresholdPolicy::new(Synergistic,
  100.0, 0.0)` run (feedback-off identity).
Any drift invalidates the run — fix the harness, never the criteria.

## Confirmatory criteria (evaluated on Scope B; Scope A is the null control)

Medians over 30 seeds; thresholds (1.25×, 18/30) inherited from the K4-v2/v3/#46 amendments for
cross-run comparability.

- **H1 — beats magnitude:** `mag_primary_median < 1.25 × fb_selective_primary_median`
  (magnitude no longer clearly superior).
- **H2 — mechanism (feedback does the work *beyond* selectivity):** `fb_selective_primary_median
  ≥ 1.25 × thr_selective_primary_median` **and** `fb-selective` strictly superior to
  `thr-selective` on ≥ 18/30 seeds.

**Verdicts:**
- `VALIDATED (selective-feedback arm)` — H1 ∧ H2.
- `PARTIAL (selectivity only)` — H1 ∧ ¬H2 (a positive threshold closes the gap, but the *feedback*
  increment is not the driver — selectivity alone did it). **This is a genuinely likely outcome**
  and a legitimate scientific result: it would attribute magnitude's edge to selectivity, not
  reliability-gating.
- `PARTIAL (mechanism only)` — H2 ∧ ¬H1 (feedback beats its selective base; magnitude still ahead).
- `FALSIFIED (selective feedback)` — ¬H1 ∧ ¬H2.

**Registered prediction (honest, not tuned):** on the full-join base the best exploratory
feedback cell reached only `0.0730` vs mag `0.2818`; selectivity is the untested lever. We expect
`thr-selective` to move substantially toward mag (selectivity is most of mag's edge), and
`fb-selective ≥ thr-selective` (failure-weighting can only decline flaky members, never help
them). Whether `fb-selective` clears **both** `1.25×`-of-mag (H1) and the mechanism gate over
`thr-selective` (H2) is the open question. A `PARTIAL (selectivity only)` — H1 without H2 — is the
prediction we would not be surprised by.

**Scope A commitment:** `thr-selective ≈ fb-selective` and neither clears H1 (no reliability
signal exists in the i.i.d. scope). A Scope-A `fb` win is a red flag (metric/leakage bug), not a
success.

Secondary, record-only: churn + `success_rate` medians per arm (expected `fb-selective` churn ↑
vs the falsified `join=0` arms, and `fb-selective success_rate > thr-selective` if H2 holds);
latency medians + the fb/thr ratio.

## Exploratory (non-gating, table-only, run after the confirmatory battery)

- **E1 — threshold sweep** `join_threshold ∈ {50, 75, 100, 125, 150}` on Scope B, all three
  metrics (`thr-selective` and `fb-selective` PRIMARY_B + churn per threshold) — locates where
  selectivity peaks and whether the feedback increment grows with a tighter base.
- **E2 — weight × threshold** `(hw, fw) ∈ {0, 0.5, 1, 2}²` at the registered `join = 100` (the
  #46 E1 sweep re-run on the selective base — does the cancellation break once membership is
  gated?).
- **E3 — oracle gap (Scope B):** for `n ≤ 8`, the reliability-aware oracle `PRIMARY_B`, to bound
  how much of the gap any arm could close.

## Interpretation commitments

- `VALIDATED` ⇒ a small-and-reliable feedback arm captures a signal orthogonal to both diversity
  (`mag`) and coverage (EFE); the reliability battery becomes the natural testbed for the
  persistent-agent AIF arm (#44).
- `PARTIAL (selectivity only)` ⇒ magnitude's dominance is a *selectivity* phenomenon reproducible
  by a plain positive threshold; feedback is documented as correct-but-not-the-lever here. The
  feedback-arm line closes cleanly.
- `PARTIAL (mechanism only)` / `FALSIFIED` ⇒ magnitude's quality dominance stands; #41 is not
  refuted (the calculator math is unit-proven; Scope A is the expected null).
- Thresholds (1.25×, 18/30, seeds/T/universe, `ρ`/flaky-fraction) are inherited or reused from
  #46, not chosen post hoc. `#49` (failure-weighted `hw=0, fw=1`) is **absorbed** as the
  registered `fb-selective` weighting.

## Result — PARTIAL (mechanism only), 2026-07-17

Run committed as `docs/ab-report-feedback-arm-k4-v2.md`. Scope B medians: `mag 0.2818 ·
thr-selective 0.0301 · fb-selective 0.0512`. **H1 FAIL** (`mag 0.2818 ≥ 1.25 × fb 0.0512` —
magnitude ~5.5× ahead, unbeaten); **H2 PASS** (`fb 0.0512 ≥ 1.25 × thr 0.0301` and
fb-selective strictly superior to thr-selective on **21/30** seeds). Verdict = `H2 ∧ ¬H1` =
**PARTIAL (mechanism only)**. Scope A held the registered null (`thr-selective ≈ fb-selective`,
fb superior in only 4/30, neither clears H1 — no red flag). Regression gate held (`mag`
0.4469/0.2818 reproduced; Part 3 byte-identical incl. the `hw=1,fw=2 → 0.0730` E1 cell).

**Reading.** The #46 root cause is fixed — a positive `join_threshold` restores selectivity and
lifts both arms off the 0.0140 full-join floor — and failure-weighting adds a *genuine
reliability signal on top of a selective base* (that is exactly what H2 establishes: magnitude's
edge is **not** pure selectivity). But it does not close the ~5.5× quality gap; magnitude's
small high-`cov_eff` coalitions remain the thing to beat. The **E1 sweep** shows the feedback
increment is non-monotone in base tightness — it helps in a *middle* band (`join ∈ {50,75,100}`)
and pure selectivity **overtakes** it at `join ∈ {125,150}` (thr 0.0906/0.0937 vs fb 0.0451/0.0413),
because a tight base plus the 0/1 `fw=1` penalty starts evicting merely-unlucky good agents. The
registered `join = 100` was fixed before the sweep was seen, so the verdict is not
threshold-shopped. `#41` not refuted; `#49` absorbed (registered `fb-selective` weighting). The
reliability battery remains the testbed for the persistent-agent AIF arm (#44). Nothing tuned to
flip the verdict.
