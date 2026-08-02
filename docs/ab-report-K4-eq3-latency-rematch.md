# A/B report: K4 EQ3 — cg latency re-match (koalisi #69 / stack EQ3)

**VERDICT: `FALSIFIED (latency re-match)`** — H-par′ (i) PASS · H-par′ (ii)
PASS · H-lat FAIL, scored under the amended grammar of
`docs/prereg-K4-eq3-latency-rematch.md` §Amendment 1 (prereg committed
557af83 BEFORE implementation; Amendment 1 committed d46b562 BEFORE any
Part 7 code; both posted to
[#69](https://github.com/sustia-llc/koalisi/issues/69)).

**Run of record: 2026-08-02**, seeds **210..240** (fresh; 90..120 and
150..180 stay reserved), v2-draw regime, Scope B, `--release`, build
`--features decision,magnitude,magnitude-fast`. Pins: koalisi `v0.21.0`
(catgraph `v0.6.0` ×2 at `cd8fa94`, `aif-v0.12.0`, slm `v0.2.1`). Code
under test: branch `feat/69-eq3-latency-rematch` at 7c1af9d (levers
b0d4408, Part 7 cbcb247, 3-lens review batch 7c1af9d). This document is
immutable; anything later is an appended addendum, never an edit.

## Hypothesis and levers (registered)

The K4 v1 Path-A latency falsification (K6 report of record: mag 3.552 µs
vs aif 1.435 µs median per decision, quality-dominant but latency-slower)
is revisable on catgraph v0.6.0's EQ3 feature set. Levers, per Amendment 1
(A1.1): **L1** `value_with_scratch` adoption — the library default, arm
`mag`; **L2** zero-diversity proof branch (all three cg#153 classes) +
**L3** `f64-fast` fresh-eval routing — together behind the off-default
`magnitude-fast` feature + `with_eq3_levers(true)`, arm `mag-eq3`.
Comparator `scalar` = `AifDecisionPolicy::default()`. `KNIFE_EDGE_REL_BAND`
untouched (non-goal).

## X-gates (all PASS before the official run)

- **X-A** (feature-off `decision,magnitude` build): frozen Parts 1–6
  byte-identical on every quality/churn/verdict line vs the fresh
  pre-change v0.21.0 baseline; latency lines the sole diff; the only
  addition a labeled Part 7 SKIP block.
- **X-B** (official feature-on binary): Parts 1–6 byte-identical again —
  verified on the smoke binary and re-verified on the official run output.
- Unit gates: 60-seed default-path bit-exact stream gate (acts + score
  bits, both leave variants); proof-branch equivalence per class; f64
  anchor on the K2/gotcha fixtures incl. a Gauss–Jordan-class fixture;
  toggle identity. Suites at run time: 103 default / 126 magnitude / 182
  decision,magnitude / **134** magnitude-fast; example binary 26 (+1
  ignored) and 31 (+1) with the feature; clippy clean on both feature sets.

## Review trail

3-lens review (correctness / registration-conformance /
modeling-semantics) BEFORE the official run: **0 blocking · 7 IMPORTANT ·
~21 minor; every finding applied (commit 7c1af9d) or ledgered below.**
The correctness lens' headline finding (L3's factorization route
engagement) is §Interpretation's load-bearing input.

## Arms (pooled over 30 seeds — the registered measurement)

_One policy instance per arm reused across all 30 seeds (the v1 Part 2
code shape; comparability requires equal cache-warmth history — deviation
ledger item 5)._

| arm | median PRIMARY_B | median churn | median µs/decision | IQR µs | decisions |
|-----|-----------------:|-------------:|-------------------:|-------:|----------:|
| `scalar` | 0.0927 | 103.50 | 2.675 | 0.108 | 10379 |
| `mag` (L1) | 0.1078 | 8.50 | 6.211 | 13.014 | 7640 |
| `mag-eq3` (L2+L3) | 0.1105 | 8.00 | 4.830 | 13.691 | 7597 |

## H-par′ (i) — first-divergence shape: **PASS (49/49)**

7580 paired decisions compared (77 leave steps structural, where only one
arm still held the member). 49 task-level first divergences, every one
carrying the certified shape (a fired `ZeroDiversityProof` for `mag-eq3`
AND |`mag` margin| ≤ 1e-15). All 49 are join-side; margins span
2.220e-16..8.882e-16; classes: incoming-dup and outgoing-dup only (no
SkeletalMerge first-divergence — consistent with the library corpus
measurement that the merge class never flips). The printed table shows 40
rows with 9 PASS rows elided; **shape-FAILING rows print unconditionally
by construction**, and there were none.

- **Shape-bound headroom:** largest certified margin 8.882e-16 = 89 % of
  the registered absolute bound 1e-15. One ulp at magnitude ~16 is
  1.78e-15, so a legitimate certified flip on a larger coalition could
  exceed the bound and would be scored `FALSIFIED (parity)` —
  conservative direction; this run stands as scored; a scale-relative
  bound is future-registration material (ledger item 6).
- Cascaded post-first divergences (A1.2-exempt, context): **84**.
  **Cascade residual:** 551/600 tasks (91.8 %) diverge nowhere and are
  fully verified decision-for-decision; the only L3-unverified stream is
  the post-divergence tail of the 49 divergent tasks (ledger item 7).
- Leave note: the default leave variant is the fresh two-evaluation path —
  L2 cannot fire on a leave; any leave-side first divergence would have
  failed the shape by construction. None occurred.

## H-par′ (ii) — quality non-inferiority: **PASS**

median `PRIMARY_B(mag-eq3)` **0.1105** ≥ 0.98 × `mag` 0.1078 = 0.1056.
`mag-eq3` in fact exceeds `mag`'s median: declining certified exact-zero
joins (redundant agents the frozen arm admits on +2e-16 roundoff) is
mildly quality-positive on this draw. Per-seed deltas (Δ =
eq3 − mag): 11 seeds bit-equal 0.0000 · 10 positive (max +0.0332, seed
232) · 9 negative (min −0.0197, seed 238) — the full 30-row table is in
the run output and reproducible from the committed code at 7c1af9d.

## H-lat — strict Path-A analogue: **FAIL**

pooled median per-decision latency `mag-eq3` **4.830 µs** vs `scalar`
**2.675 µs** (strict `<` required).

**VERDICT: `FALSIFIED (latency re-match)`** (= H-par′ ∧ ¬H-lat under
Amendment 1's grammar).

## Context (registered, never gated)

- **Lever decomposition:** `mag` (L1 only) 6.211 µs → `mag-eq3` (L2+L3)
  4.830 µs.
- **Ratios:** `mag-eq3`/`scalar` **1.81×** · `mag`/`scalar` 2.32× · K6
  reference 2.48× (cross-regime: K6 is v1-draw; the ratio is the *more*
  comparable quantity, not an invariant one).
- **Frozen-battery before/after** (v1 regime, `mag`, latency-only by
  construction): this binary's Part 2 median **3.439 µs** vs pre-change
  baselines 3.566/3.793 µs and the K6 report-of-record 3.552 µs — L1
  alone is a mild improvement on the frozen battery.
- **Quality medians** (v2 regime): scalar 0.0927 · mag 0.1078 · mag-eq3
  0.1105. (#61 Part 5a's v2 context rows — mag 0.1286 < scalar 0.1332 on
  seeds 120..150 — used a different seed range; the ordering here is
  mag > scalar. The v2-regime quality inversion is seed-range-sensitive;
  neither row gates anything and the #54 arm question stays CLOSED.)

## Instrumentation (registered, non-gating)

### Increment distribution — `mag`'s join stream, 4365 probed decisions

| class / decade | count |
|---|------:|
| non-finite | 0 |
| exact 0 | 1835 |
| < 1e-16 (underflow) | 0 |
| [1e-16, 1e-15) | 40 |
| [1e-15, 1e-14) | 2 |
| [1e-3, 1e-2) | 5 |
| [1e-2, 1e-1) | 174 |
| [1e-1, 1e0) | 1331 |
| ≥ 1e0 | 978 |

**The cg#153 empty-band hypothesis is CONFIRMED on koalisi traffic: 0
decisions in [1e-13, 1e-6).** The increment distribution is bimodal —
exact/near-exact zeros vs ≥ 1e-3 — with nothing within seven decades of
the 1e-6 band edge. Knife-edge population: 1877/4365 probed joins
(43.0 %). No band change ships in EQ3 (non-goal); a narrowing would be
its own registration.

### Proof fire-rate — `mag-eq3` arm

SkeletalMerge 854 (19.6 %) · incoming-dup 318 (7.3 %) · outgoing-dup 697
(16.0 %) · all 1869 = 42.8 % of probed joins. **Former knife-edge
recomputes retired, measured on the frozen arm's stream (the recomputes
actually paid pre-EQ3): 1870/1877 = 99.6 %** (eq3's own stream, a
different decision population after L2 drift: 1869/1871 = 99.9 %).

### Latency by bucket (instrumentation pass — shape only, probe-disturbed)

| bucket | mag count | mag med µs | eq3 count | eq3 med µs |
|--------|----------:|-----------:|----------:|-----------:|
| join/empty | 206 | 1.278 | 206 | 1.366 |
| join/excluded | 1009 | 0.300 | 1009 | 0.297 |
| join/clear | 2488 | 5.647 | 2494 | 4.856 |
| join/band | 1877 | 8.338 | 2 | 55.190 |
| join/proof | 0 | — | 1869 | 1.797 |
| join/probe-err | 0 | — | 0 | — |
| leave/fresh | 2060 | 8.473 | 2017 | 7.663 |

L2 converts the 8.3 µs band bucket (1877 decisions) into a 1.8 µs proof
bucket (1869) + 2 residual band decisions; the surviving cost centers are
`join/clear` (~4.9 µs × 2494) and `leave/fresh` (~7.7 µs × 2017). The K6
`rebuild`/`hit` split is NOT reproduced (deviation ledger item 1).

### `FactorizationPath` — `mag-eq3` fresh evaluations (3984 counted)

Cholesky 2871 (72.1 %) · LBLT 0 · Gauss–Jordan fallback 1113 (27.9 %) ·
Err magnitudes 0 (errored evaluations would still be route-counted).
Sites counted: both magnitudes of every variant-A leave + the `with` side
of every unprovable-knife-edge join; NOT bootstrap joins (1×1 ζ) or the
excluded branch (no fresh eval). **Read H-lat against this split**: the
f64 handle takes Cholesky/LBLT only on exactly-symmetric ζ; the
substitutability coupling is asymmetric whenever two members' relevant
widths differ, so the fallback share re-enters the rig-generic path after
paying the dense build + symmetry scan — on that share L3 is net
overhead, and `mag-eq3`'s improvement comes chiefly from L2. (The 3-lens
review measured the whole-join-stream fallback share at 70.9 % on
v2-shaped draws; the 72.1 % Cholesky here reflects the L3-routed population —
post-formation coalitions are small/skeletal and often symmetric.)

## Deviation ledger

1. **Bucket table drops the K6 `rebuild`/`hit` split** — unregistered
   deviation on its own justification: the split is visible only from
   cache-internal state, and exposing it cheaply means instrumenting the
   code H-lat times. The "where cheaply available" latitude attaches to
   the FactorizationPath row, not this table.
2. Prereg §Registered context rows retains pre-A1.1 lever attribution
   ("L1+L2" — reads L1 vs L2+L3 after A1.1); the implementation and this
   report use the corrected reading.
3. Unit gate (a) re-read post-A1.1 (bit-exact equality unsatisfiable by
   A1.1's own finding): discharged as an act-strict + 1e-9-relative-score
   fixture half plus a divergence-characterization corpus half.
4. Unit gate (c) discharged jointly by the within-build toggle-identity
   test ∧ X-B (a single unit test cannot span builds).
5. Prereg "(per-seed fresh arms)" describes the learning-arm factory
   pattern; Part 7's stateless arms use the v1 Part 2 shared-instance
   convention, disclosed in print.
6. The registered 1e-15 shape bound is absolute with 89 % observed
   headroom; conservative direction; future reuse needs a scale-relative
   bound.
7. The A1.2 cascade exemption leaves the post-divergence tails of the 49
   divergent tasks L3-unverified (551/600 tasks fully verified); the
   library L3-isolation corpus test is v1-regime.
8. `probe_fresh_factorization` pre-fix dropped Err-route paths
   (GJ-undercount); fixed at 7c1af9d BEFORE the official run — official
   numbers are complete.
9. CLAUDE.md's K6 entry cited 3.658/1.387 µs — numbers absent from the
   K6 report of record (3.552/1.435; 3.915 pre-K6; 4.900 variant-B) —
   corrected to the committed numbers in the release for this report.
10. The L3 route required the A1.3 skeletal-space correction (the
    registered full-cospan route is singular on mutual-1.0 clone pairs);
    implemented via public `member_classes()` representatives,
    fixture-gated.

## Interpretation (per the pre-committed clauses + Amendment 1)

- **The latency gap survives the full catgraph v0.6.0 lever set** at the
  v2 regime: 1.81× vs the strict-crossing bar. The residual is located,
  not speculated: (a) the **`leave/fresh` path** (~7.7 µs × ~27 % of
  decisions — two full fresh evaluations per leave, untouched by any
  registered lever; K6 already measured evaluator-variant-B slower); (b)
  **`join/clear`** (~4.9 µs — the evaluator's genuine incremental work +
  rebuild amortization); (c) **L3's engagement ceiling** — the f64 fast
  factorization requires exactly-symmetric ζ, which substitutability
  couplings mostly are not; on the fallback share L3 is net overhead.
- **L2 is the working lever**: 99.6 % of the frozen arm's knife-edge
  recomputes are certificate-retired at ~1.8 µs vs ~8.3 µs, with
  decision changes confined to certified exact-zeros that are mildly
  quality-POSITIVE (H-par′ (ii) passed above 1.0×). L2+L3 remain opt-in
  (`magnitude-fast` + `with_eq3_levers`); **L1 ships as the default**,
  identity-gated (X-A/X-B), and improves even the frozen v1 battery
  (3.55 → 3.44 µs).
- **The v1 `FALSIFIED (latency)` verdict is NOT revised.** The v2
  quality verdict (VALIDATED (B)) is untouched; the #54 arm question
  stays CLOSED; nothing here reopens either.
- **Upstream follow-up (non-blocking, filed against catgraph):** an
  asymmetric-capable fast factorization (or a cheap symmetry pre-check
  that skips the dense build) would raise L3's engagement from ~28 % of
  its routed traffic; the per-class/per-k engagement data in this report
  and the review ledger is the evidence pack. A leave-side incremental
  path (downdate or reduced-set reuse) is the larger prize
  (~27 % of decisions at ~7.7 µs) but K6's variant-B measurement says a
  naive form is slower — engine-side design work, its own registration.
- Any future latency re-match (new levers, symmetric fast path, leave
  incrementals) is a NEW registration; none is implied by this
  falsification.

## Reproduction

`cargo run --release --features decision,magnitude,magnitude-fast
--example strategy_comparison` at 7c1af9d (or the merge commit) on the
v0.21.0 pins. Latency lines are the standing non-deterministic exclusion;
every other Part 7 line is deterministic in the seeds.
