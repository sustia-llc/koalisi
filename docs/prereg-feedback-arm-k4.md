# Pre-registration: feedback-weighted arm vs magnitude — K4 battery (koalisi #46; #41 follow-up)

_REGISTERED 2026-07-16 on #46 — posted **before any implementation or run** (git commit is
the immutable mirror; pending). Binding from this point: changes require a posted amendment
on #46 **before** any run. Lineage: #7 (v1/v2 criteria) → #43 K4-v3
(multimodal AIF, `FALSIFIED (multimodality)`) → this. Decisions locked by owner
2026-07-16: scope **B+A**, base **Synergistic**, fitness **0/1 per scope + continuous
exploratory**, weights **`hw=fw=0.5` registered + sweep exploratory**. Per the K4
falsification discipline: nothing may be tuned to flip a verdict; falsification is a
legitimate result. Changes require a posted amendment **before** any run._

## Question

koalisi #41 shipped `FeedbackCalculator<C>` + `FeedbackStore` — the SwarmAgentic
`c_p`/`c_f` velocity coefficients (`history_weight`/`failure_weight`) closed in pure Rust.
It plugs into the K4 battery as `ThresholdPolicy<FeedbackCalculator<C>>`. But the current
battery draws each task's requirements **i.i.d.** from a **static, deterministic** agent
pool, so past outcomes carry no signal about future task fit — feedback is a foregone null
there. The real question:

**H-main: when the environment has an exploitable reliability signal that neither
magnitude (diversity) nor EFE (coverage) observes, a feedback-weighted arm beats the
magnitude arm on realized task quality.**

Two batteries test this:
- **Scope A** (i.i.d., existing) — the null control: feedback ≈ its feedback-off base.
- **Scope B** (reliability-structured, new) — the confirmatory contest.

## Arms (both scopes)

Byte-identical instances/streams/decision rules across arms; only the value model +
feedback differ. Base calculator = **`SynergisticCalculator`** (matches Part 1 and the
diversity framing). Decision rule = `ThresholdPolicy` with `join_threshold =
leave_threshold = 0.0`, bootstrap-first-arrival, one leave sweep.

1. **`mag`** (frozen) — `MagnitudePolicy::default()` (t = 1). The catgraph-magnitude
   incumbent.
2. **`thr`** (feedback-off control) — `ThresholdPolicy::new(SynergisticCalculator, 0, 0)`.
   Isolates what feedback adds.
3. **`fb`** (new) — `ThresholdPolicy::new(FeedbackCalculator::new(SynergisticCalculator,
   hw = 0.5, fw = 0.5, store), 0, 0)`. `store = FeedbackStore::new(1.0)` **fresh per
   seed**; outcomes written back **once per task, after the leave sweep**, so within-task
   join/leave decisions see a constant store.

`aif-scalar`/`aif-mm` are not carried (orthogonal to the fb-vs-mag question; available if
continuity is wanted).

## Fitness signal (written to `record_outcome`)

Arm-external ground truth only — never the arm's own score (self-reinforcing runaway).
Members credited = the **final** coalition after the leave sweep.

- **Scope A confirmatory** — `completed` (0/1): union of member caps fully covers
  `required`. `FeedbackStore::new(1.0)` ⇒ an incomplete task is exactly a failure.
- **Scope B confirmatory** — `success` (0/1): `completed` **and** all final members
  "performed" on this task (reliability draw below). `FeedbackStore::new(1.0)` ⇒ a covered
  task that failed on reliability is a failure.
- **Exploratory (E5, both scopes)** — continuous `cov_eff` fitness with a threshold at the
  per-seed mean, to compare 0/1 vs graded credit. Non-gating.

## Scope B — reliability structure (no degrees of freedom left)

Extends `generate_instance(seed)`; all draws come from the same seeded `SplitMix64` stream
so instances stay byte-identical across arms.

- **Per-agent hidden reliability**, drawn once per seed after the caps/trust draw:
  bimodal — with prob **0.7** the agent is *reliable* (`ρ_i = 0.05`), else *flaky*
  (`ρ_i = 0.40`). (Bimodal gives feedback a crisp reliable/flaky signal to learn.)
- **Per-(task, agent) performance matrix** `perf[t][i] ∈ {0,1}`, pre-drawn once per
  instance: `perf[t][i] = 1` w.p. `1 − ρ_i`, independent draws from the seeded stream,
  **arm-independent** (drawn before any arm runs). This keeps every arm on identical
  instances — the prereg's core invariant.
- **Realized task success** for a formed coalition `S` on task `t`:
  `success = completed(S, t) AND (∀ i ∈ S: perf[t][i] == 1)`. Including a flaky member
  lowers the odds all members perform ⇒ feedback should learn to avoid flaky agents;
  `mag` (diversity) and EFE (coverage) are both blind to `perf`/`ρ`.
- **Primary metric (Scope B):** `PRIMARY_B(seed) = success_rate × mean_cov_eff`, where
  `success_rate = realized successes / T`. (Scope A keeps the committed
  `PRIMARY = completion_rate × mean_cov_eff`.)
- **Oracle (Scope B):** record-only/optional (`n ≤ 8`): the coalition maximizing
  `PRIMARY_B` over the realized `perf` matrix. Not a gate.

## Protocol (shared; unchanged instance grammar)

Seeds `0..30`, `SplitMix64`; pool `n ∈ [4,16]`, caps `k ∈ [1,4]` distinct bits of the
8-bit universe, trust `20–99`; `T = 20` tasks, required `r ∈ [1,5]` bits; seeded
Fisher–Yates arrival order per task; churn = leave-sweep removals; latency per arm,
`--release`, same hardware, sync path; seed-0 warm-up discarded. Feedback makes `fb`
decisions path-dependent on **within-seed task order** by design; the store is reset
between seeds so the 30 instances stay independent.

**Harness shape (implementation):** `run_battery(make: Fn(u64) -> Arm)` where
`Arm { policy: Box<dyn CoalitionDecisionPolicy>, store: Option<FeedbackStore> }` is built
fresh per seed (and for warm-up). `run_instance` calls `store.record_outcome(&final_members,
fitness)` after the leave sweep when `store` is `Some`. Existing arms pass `store: None`
and clone their policy (`MagnitudePolicy::clone` shares its cache — gotcha 15 — so per-seed
cloning preserves today's behaviour). No new library API required.

**Regression gate (run validity, not hypothesis):** on **Scope A**, `mag` per-seed
`primary`/`churn` must reproduce `docs/ab-report-K4-catgraph-evaluator.md` seed-for-seed;
`thr` (feedback-off) must equal `fb` with `hw = fw = 0` and equal a plain
`ThresholdPolicy<Synergistic>` run. Any drift invalidates the run — fix the harness, never
the criteria.

## Confirmatory criteria

Medians over 30 seeds; thresholds (1.25×, 60%) inherited from the K4-v2/v3 amendments for
cross-run comparability. Evaluated **on Scope B** (the contest); Scope A is the null
control, reported alongside.

- **H1 — beats magnitude:** `mag_primary_median < 1.25 × fb_primary_median` on Scope B
  (magnitude no longer clearly superior).
- **H2 — mechanism (feedback does the work):** `fb_primary_median ≥ 1.25 ×
  thr_primary_median` **and** `fb` strictly superior to `thr` on ≥ 18/30 seeds.

**Verdicts:**
- `VALIDATED (feedback arm)` — H1 ∧ H2.
- `PARTIAL (mechanism only)` — H2 ∧ ¬H1 (feedback beats its base; magnitude still ahead).
- `FALSIFIED (feedback)` — ¬H2 (feedback did not move quality over the plain base, even
  with an exploitable reliability signal).

**Scope A commitment (registered prediction):** `fb ≈ thr` (no clear superiority either
way) and `fb` does **not** clear H1. A Scope-A `fb` win would be a red flag to investigate
(likely a metric/leakage bug), not a success.

Secondary, record-only: churn medians; latency medians + IQR and the fb/thr ratio; the
Scope-B `success_rate` per arm (expected `fb > thr ≈ mag` if H-main holds).

## Exploratory (non-gating, table-only, run after the confirmatory battery)

- **E1 — weight sweep** `(hw, fw) ∈ {0, 0.5, 1, 2}²` on Scope B.
- **E2 — ablation:** history-only (`fw = 0`) vs failure-only (`hw = 0`).
- **E3 — log seeding:** `seed_feedback_history` from a synthetic event log vs empty start.
- **E4 — reliability spread:** vary the flaky fraction / `ρ` gap (how much structure
  feedback needs to win).
- **E5 — continuous fitness:** `cov_eff` credit (threshold = per-seed mean) vs the 0/1
  signal.

## Interpretation commitments

- `VALIDATED` on Scope B ⇒ feedback captures a reliability signal orthogonal to both
  diversity (`mag`) and coverage (EFE); the two-loop roadmap gains a third genuinely
  distinct arm, and the reliability battery becomes the natural testbed for a future
  persistent-agent AIF arm (#44).
- `FALSIFIED`/`PARTIAL` ⇒ magnitude's quality dominance stands; feedback is documented as
  a correct calculator whose payoff needs richer structure than this battery supplies. #41
  is not refuted (Scope A is the expected null; the calculator math is unit-proven).
- Thresholds (1.25×, 60%, seeds/T/universe) are inherited, not chosen post hoc.

## Result — FALSIFIED (feedback), 2026-07-16

Run committed as `docs/ab-report-feedback-arm-k4.md`. Scope B medians: `mag 0.2818 · thr
0.0140 · fb 0.0140`; H1 FAIL, H2 FAIL (fb strictly superior to thr on **0/30** seeds).
Scope A held the registered null (`fb ≈ thr`, 0/30 — no red flag). The registered
`hw = fw = 0.5` weighting **cancels** in the full-join `ThresholdPolicy`-at-0 regime
(`history ≈ failures` per member ⇒ balanced marginal ≈ 0); the E1 sweep confirms the
mechanism is live (failure-dominant cells move the metric, best `0.0730`) but no cell
reaches magnitude. Magnitude's edge is selectivity (churn 8, small high-`cov_eff`
coalitions) that the full-join base does not induce. `#41` not refuted. Follow-ups filed (each a
**new** registration): a selective base (positive `join_threshold`) — #48; a
failure-weighted point (`hw=0, fw=1`) — #49 (likely folds into #48). Nothing tuned to
flip the verdict.
