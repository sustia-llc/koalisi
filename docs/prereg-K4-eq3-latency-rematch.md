# Pre-registration: K4 EQ3 — cg latency re-match (koalisi #69 / stack EQ3)

**Status: REGISTERED.** Committed BEFORE implementation (the #63/#61
discipline). Owner design-lock posted to
[#69](https://github.com/sustia-llc/koalisi/issues/69) BEFORE this document
(2026-08-02): D2 strict Path-A analogue · D3 all three levers registered ·
D4 fresh seeds 210..240 · placement library, frozen-battery X-gated.
Registered sections are immutable once the official run starts; deviations
land in the report's ledger, never as edits here.

**Pins of record:** koalisi `v0.21.0` (catgraph `v0.6.0` ×2 at `cd8fa94`,
re-pinned drift-free — PR #70; `aif-v0.12.0`; slm `v0.2.1`). The
registration is born on the final pins.

## Hypothesis

The K4 v1 **Path-A latency falsification** — magnitude quality-dominant but
strictly slower per decision (K6 re-run: mag 3.552 µs vs aif 1.435 µs
median; this session's fresh v1-regime lines: 3.566/3.793 vs 2.825/2.745 —
latency is the one non-deterministic column) — is **revisable** now that
the magnitude evaluation path is profiled and faster. The K6 profile
decomposes the miss: pure incremental hit 1.27 µs (already at AIF parity),
knife-edge fresh-recompute tax 5.05 µs on ~62 % of hits, evaluator
construction ~27–30 µs on rebuilds, leave-side fresh 5.55 µs × ~20 % of
decisions. catgraph v0.6.0 ships levers against exactly those buckets.

## Registered library changes (the three levers — owner D3)

All three land in `src/decision/magnitude_policy.rs` (library placement,
K6 precedent). Identity contracts are asserted, not assumed.

- **L1 — scratch adoption (cg#33).** The evaluator cache carries one
  `EvalScratch`; `CoalitionEvaluator::value_with` calls become
  `value_with_scratch` (and `value_with_report_scratch`, L2). Upstream
  contract: bit-identical results, no cross-call state.
- **L2 — zero-diversity proof branch (cg#153).** The join/leave dispatch
  consumes `value_with_report_scratch`. When `JoinReport::zero_proof` is
  `Some(_)` (any of the three exactly-decidable classes: `SkeletalMerge`
  = mutual-1.0 clone ∧ ¬interior-improvement; incoming / outgoing profile
  duplicates), the REAL increment is exactly 0 — the branch uses
  `with := base` (increment 0) directly and **skips the knife-edge fresh
  recompute**. Branching is on the proof, never on `increment() == 0`
  (the JoinReport contract). Unprovable candidates keep today's logic
  unchanged: knife-edge band check → fresh recompute inside the band.
- **L3 — `f64-fast` fresh-eval routing (cg#165).** New off-default cargo
  feature **`magnitude-fast`** (= `magnitude` + `catgraph-magnitude/f64-fast`)
  plus a runtime toggle on `MagnitudePolicy` (identity default OFF).
  Toggle ON routes **fresh full-coalition evaluations** (the
  `magnitude_of_masks` sites: unprovable-knife-edge recompute, leave-side
  fresh, degenerate-shape guards) through the public route
  `HomMap` → `Coalition::from_enriched` →
  `as_weighted_cospan().clone().into_metric_space()` →
  `magnitude_f64(&space, t = 1)` (ζ entries upstream-documented
  ULP-identical to the generic path; the factorization differs —
  Cholesky → LBLT → Gauss–Jordan fallback — so **bit-identity is NOT
  implied** and is measured, not asserted). Toggle OFF is the identity
  (generic `coalition_value` path), asserted by X-B.
- **NOT registered:** any change to `KNIFE_EDGE_REL_BAND` (1e-6). The
  empty-band measurement (instrumentation below) informs a possible
  FUTURE registration; no band change ships in EQ3 regardless of outcome
  (gotcha 15).

## X-gates (run-invalidating, checked before the official run)

- **X-A (feature-off identity):** build `--features decision,magnitude`;
  frozen Parts 1–6 reproduce byte-identically on every
  quality/churn/verdict line vs a fresh pre-change baseline captured on
  `v0.21.0` main (latency lines are the sole permitted diff — standing
  exclusion). This gates L1+L2, which are unconditional library changes.
- **X-B (feature-on, toggle-off identity):** build
  `--features decision,magnitude,magnitude-fast` (the official run
  binary); Parts 1–6 byte-identical again with every policy at the
  toggle's identity default. This gates the feature's mere presence.
- **Unit gates (committed with the implementation, before the run):**
  (a) proof-branch equivalence — on fixture streams, proof-fired
  decisions equal the pre-change fresh-recompute decisions bit-exactly;
  (b) f64 anchor — `magnitude_f64(space, 1)` vs `coalition_value` on
  fixture mask sets agrees within a documented tolerance AND the koalisi
  space-construction reproduces `coalition_value` semantics on the
  gotcha-12/K2 fixtures (dedup, irrelevant-agent exclusion, clone
  skeletalization); (c) toggle identity — flag OFF is bit-identical to
  the feature-off build on a fixture battery.

## Part 7 — the registered re-match battery

Additive new part in `examples/strategy_comparison.rs`; every existing
printed line byte-identical (X-A/X-B). Release build, no timeout wrapper,
single writer, unique output paths.

- **Regime (owner D1 / standing D6): v2-style.** Instance machinery =
  Part 5a's v2 draw, Scope B semantics unchanged: `|required|` uniform
  2..=8 (bits distinct, 8-bit universe), pool n ∈ 4..=16, caps 1..=4,
  hidden per-agent reliability ρ + `perf` matrix,
  `PRIMARY_B = success_rate × mean_cov_eff`, churn = leave-sweep
  removals.
- **Seeds (owner D4): 210..240** (fresh; 90..120 and 150..180 stay
  reserved). Range-battery conventions as always (per-seed fresh arms).
- **Arms:**
  - `scalar` — frozen `AifDecisionPolicy::default()` (the v1 latency
    comparator; quality row context-only).
  - `mag` — library `MagnitudePolicy::default()`, toggle OFF (carries
    L1+L2; the lever-decomposition row).
  - `mag-eq3` — same policy, toggle ON (carries L1+L2+L3; the registered
    challenger).
- **Latency measurement:** the v1 protocol verbatim — `Instant` around
  the sync `should_join`/`should_leave` calls (joins + leave sweep),
  per-decision µs pooled per arm across all 30 seeds; median + IQR
  reported. Warm, `--release`.

### Confirmatory legs (evaluated in this order)

- **H-par (decision parity):** `mag-eq3`'s decision stream (every
  join/leave act, in order) is bit-exactly equal to `mag`'s on **30/30
  seeds**. When H-par holds, every quality column of `mag-eq3` equals
  `mag`'s by construction — the owner-locked D5 quality gate. (L1+L2
  identity to the PRE-change arm is already pinned by X-A at the frozen
  parts; H-par pins L3.)
- **H-lat (strict Path-A analogue):** pooled median per-decision latency
  `mag-eq3` **strictly <** `scalar` on the same instances.

**Verdict grammar:** `VALIDATED (latency re-match)` = H-par ∧ H-lat.
`FALSIFIED (latency re-match)` = H-par ∧ ¬H-lat. A H-par failure yields
`FALSIFIED (parity)` regardless of H-lat — the arithmetic route changed
decisions, so no latency claim is made on a non-equivalent arm; per-seed
act-divergence counts and `PRIMARY_B` deltas are then reported as the
finding.

### Registered context rows (printed, never gated)

- `mag` (toggle OFF) median latency — decomposes L1+L2 vs L3.
- Latency ratios `mag-eq3/scalar`, `mag/scalar`; gap-shrink vs the K6
  reference (3.552 vs 1.435 µs, v1 regime — cross-regime, labeled as
  such).
- Frozen Part 2's mag latency line in the same run (v1 regime, L1+L2
  adopted) vs this session's pre-change baselines (3.566/3.793 µs) —
  the before/after on the frozen battery, latency-only by construction.
- Quality medians of all three arms (v2 regime; comparable to #61's
  Part 5a context rows mag 0.1286 / scalar 0.1332).

### Registered instrumentation (non-gating; printed after the verdict)

- **Increment distribution** on `mag`'s Part 7 join stream: |with − base|
  log-decade histogram (1e-16..1e0) + exact-zero count — the cg#153
  [1e-13, 1e-6) empty-band hypothesis measured on koalisi traffic; feeds
  a possible future band registration, changes nothing here.
- **Proof fire-rate by class** (SkeletalMerge / incoming-dup /
  outgoing-dup) and the share of former knife-edge recomputes retired.
- **Latency decomposition by bucket** mirroring the K6 profile table
  (pure hit / proof hit / knife-edge fresh / rebuild / leave path).
- **`FactorizationPath` counts** for the `mag-eq3` arm (Cholesky / LBLT /
  Gauss–Jordan fallback rates) via `ConditionReport` where cheaply
  available.

## Interpretation (pre-committed)

- `VALIDATED (latency re-match)` ⇒ the v1 Path-A falsification is
  **revised at the v2 regime for these levers**: magnitude is
  quality-dominant (standing v2 verdict) AND latency-competitive. The
  claim is regime- and lever-scoped; the v1 recorded verdict stands as
  history; the arm question (#54 B+D) stays CLOSED — this changes cost
  accounting, not the default-arm decision.
- `FALSIFIED (latency re-match)` ⇒ the latency gap survives the full
  v0.6.0 lever set; the decomposition rows locate the residual (expected
  candidates: rebuild construction, which no registered lever addresses).
  No further latency lever is implied.
- `FALSIFIED (parity)` ⇒ the f64 route is not decision-preserving on
  this stream; L3 is not adoptable as a default; L1+L2's identity
  adoption is unaffected (X-A). A future L3 attempt needs its own
  registration with a quality-non-inferiority design.
- Either way: L1+L2 ship (identity-gated); the band measurement is
  recorded; any band change or routing redesign is a NEW registration.

## Non-goals

- No quality re-judging of arms (the #54 arm question is CLOSED; e1 rows
  do not ride — latency is off-axis for the 63 µs persistent arm).
- No `KNIFE_EDGE_REL_BAND` change.
- No leave-side evaluator variant-B revisit (K6 measured it slower).
- No upstream catgraph changes (the public f64 route suffices; a
  convenience `coalition_value_f64` upstream is suggested, non-blocking).
- Seeds 90..120 / 150..180 stay reserved.

## Amendment 1 (pre-run, 2026-08-02 — owner-locked; posted to #69 before any Part 7 code)

Implementation surfaced two registered-text defects BEFORE the official
run (the #44/#63 pre-run amendment precedent; nothing below alters the
D2/D4 locks or the seeds):

**A1.1 — L2 is decision-changing; it moves behind the toggle (owner
lock).** Measured on a 60-seed corpus: the three-class proof branch
flips 0.77 % of certified decisions vs the frozen arm — every flip a
certified exact-zero increment that the old fresh recompute scored as
`+2e-16…+7e-16` noise and joined (one such seed sits inside frozen
Part 2). The registered premise "L1+L2 identity is pinned by X-A" is
therefore FALSE for the in/out profile-duplicate classes (SkeletalMerge
measured 0/465 flips). Owner adjudication: the library default stays
FROZEN — **L2 joins L3 behind the `magnitude-fast` toggle**. Arm
mapping becomes: `mag` = L1 only (default path, `value_with_scratch`,
knife-edge logic byte-identical to pre-EQ3); `mag-eq3` = toggle ON =
L2 (all three proof classes) + L3. X-A/X-B gate the L1-only default;
the K6 knife-edge regression fixture keeps its original assertions.

**A1.2 — H-par is replaced by H-par′ (owner lock: characterized
divergence + non-inferiority).** With L2 intentionally divergent on
certified exact-zeros, bit-exact stream parity would falsify by design.
H-par′, two conjuncts, both confirmatory:

- (i) **Shape**: within each task, at the FIRST decision where
  `mag-eq3`'s act differs from `mag`'s, the certified shape must hold:
  a `ZeroDiversityProof` fired for `mag-eq3` at that decision AND
  `mag`'s own margin there is ≤ 1e-15 in magnitude. Any first-divergence
  without that shape ⇒ **FALSIFIED (parity)**. Subsequent divergences
  within the same task are membership-cascade effects (task state resets
  per task in the harness), exempt from the shape check and counted
  separately as context.
- (ii) **Quality non-inferiority**: median `PRIMARY_B(mag-eq3)` ≥
  **0.98×** median `PRIMARY_B(mag)`; per-seed deltas printed.

Verdict grammar update: `VALIDATED (latency re-match)` = H-par′ ∧
H-lat; `FALSIFIED (latency re-match)` = H-par′ ∧ ¬H-lat;
`FALSIFIED (parity)` = ¬H-par′ (either conjunct), regardless of H-lat.

**A1.3 — L3 route correction (factual).** The registered route
(`as_weighted_cospan().clone().into_metric_space()`, the FULL member
space) is singular on any mutual-1.0 clone pair — `coalition_value`
inverts the SKELETAL space. Corrected route (implemented + fixture-
gated): rebuild the skeletal space from the public surface — one
representative per `Coalition::member_classes()` class,
`d(a,b) = −ln(cospan.weight(rep_a, rep_b))`, `+∞` at weight 0 — then
`magnitude_f64(&space, 1.0)`. Reproduces upstream's internal
construction exactly on the K2/gotcha fixtures.

**Interpretation updates.** "L1+L2 ship (identity-gated)" reads as:
**L1 ships identity-gated; L2+L3 ship opt-in** behind
`magnitude-fast` + toggle. Proof-class fire-rate instrumentation is
measured on the `mag-eq3` arm (the default arm no longer pays the
report-scan cost). A future default-adoption of L2 (any class beyond
this arc) is a NEW registration — it would unfreeze pinned decisions.

## Process

Implementation next (3-lens review — correctness /
registration-conformance / modeling-semantics — BEFORE the official
run; every finding applied or owner-adjudicated). Official run on
210..240 → immutable report `docs/ab-report-K4-eq3-latency-rematch.md`
with a deviation ledger. Example-binary tests extended for the unit
gates; suite-count changes recorded in the report and CLAUDE.md.
