# Pre-registration: K4 v4 — persistent-agent multimodal AIF arm (koalisi #44)

_Registered 2026-07-17, **before any implementation or run** of the persistent arm.
Lineage: #7 (v1 → `FALSIFIED (latency)`; v2 → `VALIDATED (B)` magnitude quality
dominance), #43 (v3 → `FALSIFIED (multimodality)`: the decision-equivalence theorem,
`docs/ab-report-K4v3-multimodal-aif.md`), #46/#48 (feedback arms → `FALSIFIED` /
`PARTIAL (mechanism only)`). Engine basis: `aif-v0.10.0` (tira cdc5c8f; pin bumped at
koalisi 2fdd894). Changes require a posted amendment on #44 **before** any run.
Falsification is a legitimate result; nothing may be tuned to flip it._

## Question

The v3 falsification is a theorem, not a measurement: with binary union coverage,
symmetric preferences, and deterministic B, multimodal G is affine in covered-bit count,
so sign(ΔG) decisions are identical to the scalar bridge's. Re-representing coverage
cannot move decisions. v4 breaks the theorem's premises with **decision-relevant
structure earned from outcomes**: a persistent agent learns per-bit reliability across
the task stream (Scope B's `perf` signal — the thing magnitude and all v3 arms are blind
to), and per-decision G queries are evaluated on the **learned** model (learned per-bit
precisions instead of binary coverage; learned stochastic per-bit B ⇒ live state
info-gain; cloned Dirichlet counts ⇒ live A- and B-novelty; replayed recent-outcome
window under MMP + PrecisionDynamics ⇒ history-conditioned γ).

**H-main: a persistent multimodal AIF bridge (E1+E2+B-novelty stack) closes the quality
gap vs magnitude on the reliability battery.**

## Arms (confirmatory battery — Scope B, 30 seeds, PRIMARY_B)

1. **`mag`** — `MagnitudePolicy` (t = 1). Frozen; Scope-B rows must reproduce
   `docs/ab-report-feedback-arm-k4-v2.md` (median 0.2818) seed-for-seed.
2. **`aif-scalar`** — `AifDecisionPolicy::default()`. Frozen; Scope-B rows pre-frozen
   2026-07-17 in `docs/baseline-aif-scalar-scope-b.md` (median **0.1035**, churn 113.00,
   koalisi 12ff76a) and must reproduce seed-for-seed.
3. **`aif-pers`** — the persistent arm. Specification (no degrees of freedom left;
   design note `.claude/plans/2026-07-17-k4v4-persistent-aif-design.md`):
   - **Persistent agent, one per seed** (fresh at seed boundary — the `run_fb_arm`
     factory pattern): 8 two-state factors (one per capability bit: providers
     reliable/unreliable), 8 three-outcome modalities `{success, failure, no-obs}`
     (no-obs row uniform ⇒ likelihood-neutral padding for bits ∉ `task.required`);
     per-factor single-control sticky B `[[0.9, 0.1], [0.1, 0.9]]` (ε = 0.1); uniform D.
     `AgentParams`: `learn_a` + `learn_b` + `learn_d`, `use_param_info_gain` +
     `use_b_info_gain`, `initial_precision` flat 1.0, `initial_precision_b = 4.0`,
     `initial_precision_d = 1.0`, `StateInference::MarginalMessagePassing { horizon: 2,
     iters: 10 }`, `seed = battery_seed` (hygiene — no `act()` is ever called; see
     Determinism). **No `PrecisionDynamics` on this agent** (single control ⇒ one policy
     ⇒ provably inert; registering it here would be a defect).
   - **Observation, once per task** after the leave sweep (the `record_outcome` site):
     bit b ∈ `task.required` observes `success` iff some final member providing b has
     `perf[t][i] = true`, else `failure`; bits ∉ required observe `no-obs`.
     **`TrialBoundary::PerStream` (registered, D5)**: `reset_window()` is NOT called
     within a seed — β and the MMP window persist across the whole 20-task stream.
   - **Query POMDP, fresh per decision** (state never mutated by hypothetical queries):
     r = |task.required| factors/modalities restricted to required bits; A per bit =
     the persistent agent's **learned** column-normalized pA 2×2 success/failure block
     if the candidate covers the bit, flat 0.5 if not; B per bit = the persistent
     agent's **learned** per-bit B lifted to the 2-control bridge shape (stochastic ⇒
     state info-gain live); C = `[0.9, 0.1]` per modality; D = the persistent agent's
     current per-bit beliefs (BMA X) restricted to required bits. `AgentParams`:
     `policy_depth = 2`, MMP `{horizon: 2, iters: 10}`, `PrecisionDynamics::default()`,
     `learn_a`/`learn_b` + both novelty flags ON with pA/pB cloned from the persistent
     counts (coverage-masked; query-side learning discarded), `alpha = 8.0`,
     `seed = battery_seed ⊕ hash(task, decision)` (hygiene). Before reading G the query
     **replays the persistent agent's last 2 task-outcome observations** (restricted to
     required bits) — this is what makes MMP/PrecisionDynamics live in the query; a
     fresh agent that never observes runs no precision loop (a latent defect in v3's E2
     spec, corrected here). Value = `−expected_free_energy()`.
   - **Decision rule identical** to `aif-scalar`/`aif-mm`: join iff
     `g_alone − g_coalition > join_margin = 0`; leave iff `g_out − g_in ≤ 0`; same
     bootstrap-first-arrival, same leave sweep, non-finite guards as
     `aif_mm_policy.rs`.

## Protocol

Byte-identical Scope-B instances: seeds 0..30, SplitMix64, pool n ∈ [4,16], caps
k ∈ [1,4] of 8 bits, T = 20 tasks, r ∈ [1,5], hidden per-agent ρ (reliable 0.05 w.p.
0.7, else flaky 0.40), pre-drawn arm-independent `perf[t][i]`;
`PRIMARY_B = success_rate × mean_cov_eff` with `success = completed ∧ all final members
performed`. Latency per arm, release build, same hardware. One run of
`examples/strategy_comparison.rs` (new Part 4b).

**Regression gate (run validity, not hypothesis):** `mag` Scope-B rows ≡
`ab-report-feedback-arm-k4-v2.md`; `aif-scalar` Scope-B rows ≡
`baseline-aif-scalar-scope-b.md`; all Part-2 Scope-A rows and Part-3/4 outputs
byte-unchanged. Any drift invalidates the run — fix the harness, never the criteria.

**Determinism:** the arm never calls `act()` — decisions are `sign(ΔG)` over
deterministic belief/EFE arithmetic; stochastic B is a model object, not simulation
sampling. tira #10 seeds are set as hygiene so any future sampling ablation inherits
determinism; they are not load-bearing in the registered arm.

## Confirmatory criteria (v4)

Medians over the 30 seeds, computed exactly as in the committed reports. Thresholds
inherited from v2/v3 for cross-run comparability (ratified 2026-07-17).

- **H1 — gap closed**: `mag_primaryB_median < 1.25 × pers_primaryB_median`
  (with mag frozen at 0.2818, this requires pers median > 0.22544).
- **H2 — mechanism (persistence, not luck)**: `pers_primaryB_median ≥ 1.25 ×
  scalar_primaryB_median` (≥ **0.129375** at the frozen 0.1035) **and** pers strictly
  superior to scalar on ≥ 60% of seeds (≥ 18/30).
- **S1 — act divergence (secondary, non-gating)**: count of seeds where pers's
  join/leave act stream differs from scalar's. The v3 theorem predicts divergence > 0
  for any arm that actually escapes it; **divergence = 0 on all 30 seeds means the arm
  collapsed back to the theorem** and is reported as such.
- **S2 — churn (secondary, non-gating)**: `pers_churn_median` vs scalar's 113.00.
- **Latency: record-only, non-gating** (the query replays a 2-observation window under
  MMP × dynamics × 4 policies — expected to be the slowest arm; report medians + IQR).

**Verdicts:**
- `VALIDATED (gap closed)` — H1 ∧ H2.
- `PARTIAL (mechanism only)` — H2 ∧ ¬H1 (persistence real; magnitude still clearly ahead).
- `FALSIFIED (persistence)` — ¬H2 (learned per-bit structure did not beat the scalar
  bridge; if S1 divergence = 0 the failure mode is theorem-collapse, reported cleanly).

## Exploratory conditions (non-gating, run after the confirmatory battery)

Single toggles on the registered arm, t-sweep style tables, no verdicts:

- **E4 — `TrialBoundary::PerTask`**: `reset_window()` after each task (β/window per
  task; the D5 alternative).
- **E5 — E2-only**: learning flags off (persistent agent still accumulates nothing;
  queries use binary coverage + stochastic B + dynamics) — isolates the info-gain lever.
- **E6 — E1-only**: dynamics off, MeanField queries — isolates the learned-precision lever.
- **E7 — novelty-off**: `use_param_info_gain = use_b_info_gain = false` in queries —
  isolates the novelty terms' decision contribution.
- **E8 — `initial_precision_b` sensitivity**: 1.0 / 4.0 / 16.0.

## Interpretation commitments

- `VALIDATED`: the K4 quality gap is attributed to the frozen arms' outcome-blindness;
  reliability-learning AIF becomes a live alternative to magnitude and the two-loop
  roadmap re-evaluates arm choice on latency/semantics.
- `PARTIAL`: magnitude's dominance stands; the persistent bridge is reported as engine
  capability evidence (the first arm to beat scalar on Scope B, if H2 held) and the
  next lever is combining reliability-learning with magnitude's selectivity, not more
  AIF representation work.
- `FALSIFIED (persistence)`: the E1/E2/B-novelty stack is exhausted for this battery;
  #44 closes with the negative result and the AIF track exits the K4 quality contest
  (scalar bridge remains the supported coalition-value primitive for semantics, per
  tira #1's original scoping).
- The 30-seed battery and thresholds (1.25×, 60%) are inherited from v2/v3 to keep
  verdicts comparable across K4 runs — not chosen post hoc.
