# A/B report: K4 battery v2 — de-saturated regime (koalisi #61 / stack EQ1)

> **RESULTS (2026-07-31):**
> **Lever 2 (de-saturation): `FALSIFIED (de-saturation)`** — no (γ ∈ {1, 4},
> v2-draw, δ > 0) cell reaches the churn ≤ 0.5× bar (best: 173.00 vs a 175.00
> baseline; the bar was ≤ 87.5). Mechanism measured: γ de-saturates only the
> **leave** stream; the **join** stream stays saturated at p = 1.0 in every
> cell, and the registered margin lever acts on joins.
> **Lever 1 (routing): `RUN-INVALID (sanity leg)`** — the unweighted argmax
> covers all required bits on 25/30 < 27. No verdict; two structural
> formulation flaws in the registered criterion are documented below.
> **Part 5c (registered exploratory): four of its five items did not run**
> (the 12-bit widened slice, leave-side hysteresis, expected-outcome model,
> learned-posterior twins) — deferred by owner decision 2026-07-31 to a
> follow-up session; their results will be recorded as an appended
> exploratory addendum, never by editing the sections above. The fifth item
> — oracle-vs-degraded pricing — ran as the Part 5a oracle twins and is
> reported in the Lever 3 section below.

_Registered design: `docs/prereg-K4-battery-v2.md` (committed d9881e1 +
posted to #61 BEFORE implementation). Run 2026-07-31, `--release`, seeds
**120..150** (150..180 held out — **unconsumed**: nothing validated, so no
replication run occurred; the range stays reserved per the prereg).
Pins: catgraph `v0.5.0` ×2 · `aif-v0.11.0` · surrealdb-live-message `v0.2.1`
(the registered born-on set; mid-arc freeze held). New arm-config lever:
`PersistentAifConfig::query_gamma: Option<f64>` (identity default `None` =
engine γ 16; live only on the MeanField / `query_dynamics: false` path)._

## Run-validity gates

| Gate | Registered requirement | Result |
|---|---|---|
| X-A | arm-E1 (frozen v5 config, oracle) reproduces 0.4406 / 136.00 on 30..60 | **PASS** (in-code assert, standing Part 4d gate) |
| X-B(a) | `query_gamma: Some(16.0)` ≡ `None` bit-for-bit on the X-A cell | **PASS** (asserted, all 30 seeds) |
| X-B(b) | `MarginE1(δ = 0, h = 0)` ≡ unwrapped arm per seed, all 6 (γ, regime) cells | **PASS** (asserted) |
| X-C | Every pre-existing printed line of Parts 1–4h byte-identical | **PASS** (full-run diff vs pre-implementation baseline: additions only; latency lines the sole permitted diff, per the standing exclusion) |
| X-D | H-R sanity leg ≥ 27/30 | **FAIL — 25/30 ⇒ Part 5b RUN-INVALID** |

Suites at the run commit: 154 `decision` / 176 `decision,magnitude`
(+2 `query_gamma` unit tests over the v0.17.0 baselines); default clippy
`--all-targets` clean.

## Part 5a — factorial battery (lever 2, CONFIRMATORY)

γ ∈ {1, 4, 16} × regime ∈ {v1-draw: `|required|` 1..=5, v2-draw:
`|required|` 2..=8} × join margin δ ∈ {0, 0.15, 0.30}; degraded/L2 signal;
Scope B; seeds 120..150.

### Factorial cell medians (degraded signal)

| regime | arm | δ | median PRIMARY_B | churn median |
|--------|-----|--:|-----------------:|-------------:|
| v1-draw | arm-E1g1 | 0.00 | 0.3860 | 181.00 |
| v1-draw | arm-E1g1 | 0.15 | 0.3860 | 179.50 |
| v1-draw | arm-E1g1 | 0.30 | 0.3734 | 172.50 |
| v1-draw | arm-E1g4 | 0.00 | 0.3860 | 181.00 |
| v1-draw | arm-E1g4 | 0.15 | 0.3860 | 180.50 |
| v1-draw | arm-E1g4 | 0.30 | 0.3860 | 180.50 |
| v1-draw | arm-E1g16 | 0.00 | 0.3860 | 181.00 |
| v1-draw | arm-E1g16 | 0.15 | 0.3860 | 181.00 |
| v1-draw | arm-E1g16 | 0.30 | 0.3860 | 180.50 |
| v2-draw | arm-E1g1 | 0.00 | 0.1621 | 175.00 |
| v2-draw | arm-E1g1 | 0.15 | 0.1573 | 174.50 |
| v2-draw | arm-E1g1 | 0.30 | 0.1573 | 173.00 |
| v2-draw | arm-E1g4 | 0.00 | 0.1621 | 175.00 |
| v2-draw | arm-E1g4 | 0.15 | 0.1621 | 174.50 |
| v2-draw | arm-E1g4 | 0.30 | 0.1586 | 174.50 |
| v2-draw | arm-E1g16 | 0.00 | 0.1621 | 175.00 |
| v2-draw | arm-E1g16 | 0.15 | 0.1621 | 175.00 |
| v2-draw | arm-E1g16 | 0.30 | 0.1621 | 175.00 |

### In-run baselines on the same instances (context, non-gating)

| regime | arm | median PRIMARY_B | churn median |
|--------|-----|-----------------:|-------------:|
| v1-draw | mag | 0.2991 | 9.00 |
| v1-draw | scalar | 0.1093 | 101.50 |
| v2-draw | mag | 0.1286 | 10.00 |
| v2-draw | scalar | 0.1332 | 141.00 |

**Context finding (non-gating, no verdict):** the v2 regime inverts the v1
quality ordering — `mag` 0.1286 < `scalar` 0.1332 < `e1-degraded` 0.1621.
This is the first measured regime in which magnitude does not lead on
quality under the runtime-feasible signal. It is a context row on 30 seeds,
registered as context only; any arm-choice consequence is its own owner
decision per the #61 non-goals (the #54 B+D decision is untouched).

### Decision-score quantiles at δ = 0 (the mechanism observable)

| regime | arm | stream | n | p25 | p50 | p75 |
|--------|-----|--------|--:|----:|----:|----:|
| v1-draw | arm-E1g1 | join | 5760 | 0.5000 | 0.5000 | 0.5000 |
| v1-draw | arm-E1g1 | leave | 6290 | 0.4361 | 0.4890 | 0.4981 |
| v1-draw | arm-E1g4 | join | 5760 | 0.5000 | 0.5000 | 0.5000 |
| v1-draw | arm-E1g4 | leave | 6290 | 0.5000 | 0.5000 | 0.5000 |
| v1-draw | arm-E1g16 | join | 5760 | 0.5000 | 0.5000 | 0.5000 |
| v1-draw | arm-E1g16 | leave | 6290 | 0.5000 | 0.5000 | 0.5000 |
| v2-draw | arm-E1g1 | join | 5760 | 0.5000 | 0.5000 | 0.5000 |
| v2-draw | arm-E1g1 | leave | 6343 | 0.1137 | 0.4521 | 0.4989 |
| v2-draw | arm-E1g4 | join | 5760 | 0.5000 | 0.5000 | 0.5000 |
| v2-draw | arm-E1g4 | leave | 6343 | 0.3644 | 0.5000 | 0.5000 |
| v2-draw | arm-E1g16 | join | 5760 | 0.4994 | 0.5000 | 0.5000 |
| v2-draw | arm-E1g16 | leave | 6343 | 0.4994 | 0.5000 | 0.5000 |

The battery **did** create de-saturation — but only on the **leave** stream
(γ = 1, v2-draw: leave p25 = 0.1137 vs Part 4f's wall of exact 0.5000s).
The **join** stream is pinned at score +0.5 (p = 1.0, certainty) in every
cell including γ = 1 × v2-draw. The registered margin lever acts on joins
(`p > 0.5 + δ`), so it faces the one stream γ does not free. Any follow-up
churn lever on this arm should target the leave stream (hysteresis h — the
Part 5c exploratory axis) or membership state (#56 lineage), not join
margins.

### H-S evaluation (γ ∈ {1, 4}, v2-draw, δ > 0 vs own δ = 0 baseline)

| arm | δ | churn med | base | ≤ 0.5× | PRIMARY_B med | base | ≥ 0.9× | paired churn↓ | ≥ 18/30 | cell |
|-----|--:|----------:|-----:|:------:|--------------:|-----:|:------:|--------------:|:-------:|:----:|
| arm-E1g1 | 0.15 | 174.50 | 175.00 | FAIL | 0.1573 | 0.1621 | PASS | 17/30 | FAIL | FAIL |
| arm-E1g1 | 0.30 | 173.00 | 175.00 | FAIL | 0.1573 | 0.1621 | PASS | 23/30 | PASS | FAIL |
| arm-E1g4 | 0.15 | 174.50 | 175.00 | FAIL | 0.1621 | 0.1621 | PASS | 7/30 | FAIL | FAIL |
| arm-E1g4 | 0.30 | 174.50 | 175.00 | FAIL | 0.1586 | 0.1621 | PASS | 10/30 | FAIL | FAIL |

**VERDICT (lever 2): `FALSIFIED (de-saturation)`.** Scoping per the
registered verdict rule: the δ = 0 scores DID de-saturate in at least one
(γ ∈ {1, 4}, v2-draw) row, so the falsification is about the **margin
lever**, not about reaching a de-saturated regime. Part 4f's inertness
claim survives its first out-of-regime test in the one place it was
registered to be re-tested: join-side score space.

## Part 5b — reliability-routing test (lever 1, CONFIRMATORY)

`TaskCoverageV2` (full 100 · partial `w(m) = 80/m` · member cost 8·N) vs its
reliability-weighted twin; pool n ∈ 8..=16 (caps 1..=4 bits), m ∈ {7, 8},
planted `r[b*] = 0.15` / others 0.9; both argmaxes from `search()` at the
same pinned `PopulationConfig` and seed; `REAL` = closed-form expected
payoff under per-bit Bernoulli(r).

### H-R legs (seeds 120..150)

- **Sanity leg (run-invalidating):** unweighted argmax covers every
  required bit on **25/30 < 27 → FAIL**.
- Skip leg: weighted argmax skips `b*` on **0/30** (moot).
- REAL leg: weighted strictly greater on **11/30**; medians **129.8003 vs
  129.8003** (tied; moot).

**VERDICT (lever 1): `RUN-INVALID (sanity leg)`** — per the prereg's
pre-committed rule, a sanity-leg failure invalidates the run rather than
producing a verdict. **No routing conclusion may be drawn from this run in
either direction.**

### Structural analysis (why the registered criterion could not work)

Two formulation flaws in the registered H-R, both discovered
post-registration (implementation-time and run-time), both now measured:

1. **The skip predicate is unsatisfiable under partition semantics.**
   `search()` returns a partition of the ENTIRE pool (`assignment[i]`
   defined for every agent), so the union over blocks equals the pool
   union. "No block covers b\*" therefore reduces to "no pool agent
   provides b\*" — independent of the calculator — and is mutually
   exclusive with the sanity leg. Whenever sanity holds, skip is 0/N by
   construction. Related: over a fixed pool the 8·N member cost is
   constant across partitions, so "skipping to save a member" cannot
   express at the partition level; and at the `TaskCoverageV2` rates the
   full branch pays 100/m per unit reliability per bit vs the partial
   branch's 80/m, so a same-size block never profits from skipping at any
   reliability. Reliability re-ranks structures only through *which* bits
   the partial blocks cover — visible in the per-seed `REAL` deltas (both
   directions, netting to a tied median).
2. **The instance draw does not guarantee pool coverage.** n ∈ 8..=16
   agents × 1..=4 random bits fails to cover a 7–8-bit requirement on 5/30
   seeds, which is what tripped the sanity bar.

**Attribution (pre-committed-interpretation discipline):** the registered
prereg's interpretation for `FALSIFIED (routing)` — "reliability weighting
does not route even where the coefficients permit it" — is NOT available
here: no verdict was produced, and the structural analysis shows the
registered instance was not in the coefficient-permitting regime in the
first place (the flip region needs member-count savings at near-zero
reliabilities, far from the 0.9/0.15 planting). Gotcha 24's
rescale-not-reroute is neither strengthened nor weakened by this run.
A corrected routing test — coverage-guaranteed pool draw, a block-level
skip predicate, flip-region planting/coefficients — requires a **new
registration** (follow-up issue filed from this report).

Non-registered diagnostic (exploratory, printed after the verdict): the
weighted argmax's single highest-value block omits `b*` where the
unweighted one includes it on 2/30 seeds.

## Lever 3 — oracle-vs-degraded pricing (EXPLORATORY, non-gating)

Oracle twins of the Part 5a cells. Headline medians at δ = 0:

| regime | signal | arm-E1g16 median PRIMARY_B |
|--------|--------|---------------------------:|
| v1-draw | degraded | 0.3860 |
| v1-draw | oracle | 0.3948 (+2.3%) |
| v2-draw | degraded | 0.1621 |
| v2-draw | oracle | 0.1881 (+16.0%) |

The oracle–degraded gap widens roughly 2% → 16% moving to the harder
regime: per-bit signal fidelity starts to price in exactly where the v1
battery could not see it (Part 4e's "degraded ≈ oracle" is regime-scoped).
Exploratory by registration; any confirmatory fidelity claim needs its own
registration.

## Pre-committed interpretation (as registered, applied)

- Lever 2 `FALSIFIED` ⇒ the inertness of score-space churn levers
  generalizes across this γ/regime grid — with the measured refinement
  that the join rail (certainty joins) is the blocker; leave-side and
  state-based levers remain the only live churn axes.
- Lever 1 `RUN-INVALID` ⇒ no update to gotcha 24 in either direction; the
  corrected test is a new registration.
- Lever 3 stays exploratory; the widened fidelity gap motivates (not
  licenses) a registered fidelity test in a future entry.
- EQ-queue: EQ1's landing is the fair-judgment baseline for EQ3/EQ4/EQ5;
  the v2-regime context inversion (mag < scalar < e1-degraded) is the
  strongest single argument that later entries must register on v2-style
  regimes, not v1.

## Provenance

Prereg `docs/prereg-K4-battery-v2.md` (d9881e1, immutable; its Part 5c
scope remains registered and pending). Design-lock + prereg + results
posted to #61. Implementation: `query_gamma` lever in
`src/decision/aif_persistent_policy.rs` (+2 unit tests); Parts 5a/5b
additive in `examples/strategy_comparison.rs` (`run_seed_b` became a thin
`Regime::V1` wrapper over a new `run_seed_b_regime` — the regime parameter
selects the v1/v2 instance draw; the 4-arg outcome hook is unchanged, it
has carried the member list since Part 4g — call-site refactor
output-neutral, proven by gate X-C).
Battery run: single-writer release run, byte-identity verified against the
pre-implementation baseline capture. Seeds ledger after this run: 0..30,
30..60, 60..90 consumed by v1-lineage registrations; **120..150 consumed by
this run**; 90..120 soft-reserved (lockout axis); 150..180 reserved
(unconsumed replication range).

---

## Appended addendum — the deferred Part 5c items (2026-07-31, follow-up session)

_The four Part 5c items deferred on run day (see the RESULTS banner above)
ran in a follow-up session under the same registration
(`docs/prereg-K4-battery-v2.md` §"Part 5c — exploratory only", which fixes
their SCOPE and nothing about their outcome). **Everything here is
exploratory and non-gating** — no verdict is derived and none of the
registered sections above were edited. Run: `--release`, single writer,
seeds 120..150 only (ledger unchanged: 90..120 and 150..180 stay reserved).
**Gate X-C re-verified for this run**: every pre-existing printed line of
Parts 1–5b byte-identical to a fresh pre-change baseline capture; latency
lines the sole diff, per the standing exclusion. In-run gates X-A, X-B(a),
X-B(b), the new item-2 baseline assert, and the item-3 identity gate all
held (in-code asserts). Pins unchanged (catgraph `v0.5.0` ×2 ·
`aif-v0.11.0`). Suites at the addendum commit: 159 `decision` / 181
`decision,magnitude` (+5 library unit tests over v0.18.0); the example's
own test binary adds 9 more (one `#[ignore]`d, release-only)._

### Implementation & deviation ledger (recorded, not silently absorbed)

The prereg registered item 1 "at the level of intent; plumbing details are
implementation, and any deviation is recorded in the report". The ledger:

1. **`PersistentAifConfig::n_bits: usize`** (v0.19.0, feature `decision`):
   identity default **8**; out-of-range values CLAMP to `1..=16` with a
   `tracing::warn!` rather than erroring (no fitting `aif::AifError`
   variant; batteries never panic). At `n_bits = 8` the arm is bit-for-bit
   the registered arm — asserted in-code (`n_bits_eight_is_identity`) and
   by X-A/X-C.
2. **`observe_outcome` widened `&[bool; 8]` → `&[bool]`** (length mismatch
   warns + skips), and the shared harness outcome hook widened the same
   way — a type change that touches every frozen part's call sites
   (`&[bool; 8]` coerces), output-neutral by X-C.
3. **A latent out-of-universe-mask hazard was fixed in passing**:
   `decide()` now masks `required` to the low `n_bits` bits. A no-op for
   every previously-reachable input (the universe was always 8 and masks
   were drawn from it); load-bearing only once `n_bits` is configurable.
4. **Test-cost concessions**: the 12-bit battery smoke is `#[ignore]`d
   (debug-build cost breaks the 120 s test convention; it passes in
   release), and the 12-bit cell has no in-run X-B(b)-style identity gate
   (it would double the addendum's most expensive battery) — the wrapper
   identity is asserted at 2-seed scale in the ignored release test.
5. **Item 2's cell selection involves TWO interpretations** of the
   registered "at the best-performing (γ, δ) cell", both printed with
   rationale in the run output: the search is restricted to the v2-draw
   regime (lever 2's own regime; v1-draw cells score higher in absolute
   terms but their leave streams barely de-saturate), and the δ = 0
   three-way γ tie is broken toward the most de-saturated LEAVE stream
   (γ = 1) — the stream hysteresis acts on.
6. **Item 3's "gated on its own gotcha-21 degeneracy analysis" is read as
   "always run, label degenerate results as context"**, not "skip if
   degenerate" — the transparent reading consistent with the
   everything-measured norm. The output quotes the registered wording at
   the point of interpretation.
7. **Item 4's outcome stream**: a fresh `SplitMix64(seed ^
   0x5C17_0000_0000_0000)`, 20 tasks/seed (= the batteries' own stream
   length), independent per-bit Bernoulli at the planted reliabilities;
   non-perturbation of Part 5b's draw schedule is pinned by
   `part5c_twin_stream_does_not_perturb_5b`.

### Item 1 — 12-bit widened slice (`w12-draw`)

Registered cell `arm-E1g4`, δ = 0, degraded signal; 12-bit universe
(`|required|` uniform 2..=12, caps 1..=6, pool draw as v2); the arm at
`n_bits = 12` (persistent joint space 4096; query joint up to 8192).

| regime | arm | δ | median PRIMARY_B | churn median |
|--------|-----|--:|-----------------:|-------------:|
| w12-draw | arm-E1g4 | 0.00 | 0.1657 | 165.50 |
| w12-draw | mag | — | 0.0607 | 11.50 |
| w12-draw | scalar | — | 0.1062 | 127.00 |

Decision-score quantiles on the cell: join p25/p50/p75 all exactly
**0.5000** (n = 5760); leave p25 **−0.4999** / p50 0.4840 / p75 0.5000
(n = 6345). Latency 2021.630 µs/decision median (record-only; ~31× the
8-bit e1 arm — the `2^(|required|+1)` query joint).

**Readings (context, non-gating):** (a) the v2-regime quality inversion
PERSISTS and WIDENS on the wider universe — `mag` 0.0607 < `scalar`
0.1062 < `e1-degraded` 0.1657 (e1 ≈ 2.7× mag, vs 1.26× at v2-draw) — the
inversion looks like a property of de-saturated/harder regimes, not an
8-bit artifact; (b) **the join rail is margin-proof at 12 bits too** —
every join quantile sits at exactly +0.5 (p = 1.0) while γ = 4 leaves
de-saturate hard (p25 at the −0.5 rail) — extending gotcha 25's join-rail
finding beyond the universe it was measured on.

### Item 2 — leave-side hysteresis h ∈ {0.15, 0.30}

Cell per deviation 5: `MarginE1(δ = 0, h)` over `arm-E1g1`, v2-draw,
degraded; the h = 0 row is the sweep's own in-line baseline (asserted
equal to the unwrapped arm; X-B(b) pinned that equal to the Part 5a cell).

| arm | h | median PRIMARY_B | churn median | vs base churn | paired churn↓ |
|-----|--:|-----------------:|-------------:|--------------:|--------------:|
| arm-E1g1 | 0.00 | 0.1621 | 175.00 | 1.00× | — |
| arm-E1g1 | 0.15 | 0.1503 | 171.00 | 0.98× | 25/30 |
| arm-E1g1 | 0.30 | 0.1229 | 148.50 | 0.85× | 29/30 |

**Reading (context, non-gating): the first score-space lever in the E1
lineage measured to MOVE churn at all** — h = 0.30 cuts churn to 0.85× on
29/30 paired seeds (Part 4f's δ/h grid and Part 5a's join margins moved
nothing). It is live because it acts on the one stream γ de-saturates.
But it PAYS: `PRIMARY_B` drops 0.1621 → 0.1229 (−24%) for a 15% churn
cut — nowhere near the family's 0.5×-churn-at-≥0.9×-quality shape. So
leave-side score space is live-but-expensive; the #56/#54 conclusion that
cheap churn mitigation needs a STATE-based lever stands unrevised.

### Item 3 — expected-outcome value model

The model: a block's fitness IS its closed-form expected realized payoff
(`w(m)·Σ_{b∈C} r_b − 8|S| + [C = required]·20·Π_{b∈required} r_b` — the
per-block term of `REAL`), so `search()` optimizes the yardstick directly.
Identity `Σ blocks == real_payoff` asserted on all 30 argmaxes.

**Degeneracy analysis (ran FIRST, per the registered gating): `MOSTLY
DEGENERATE`** — the `search()` argmax is literally all-singletons on 1/30
seeds, but all-singletons ties or beats it on **22/30**. Mechanism: a
THIRD gotcha-21 degeneracy shape — the per-block partial term
double-counts any required bit two blocks both cover, so splitting weakly
dominates merging unless a merge creates full coverage worth more than
the overlap it destroys, and at the planted reliabilities the
full-coverage residual `20·Π r` is far too small. (The many ties are
merges of agents with disjoint covered sets, which cost exactly zero.)

Re-run of the Part 5b comparison, **context only (degenerate-by-analysis)**:
`REAL_e` median 134.5000 vs `REAL_u` 129.8003; strictly greater on 20/30
(expected — the search optimizes the yardstick itself; the 4 reversals are
PSO search noise, the swarm is not exhaustive); skips `b*` on 0/30 (the
same structurally-vacuous partition-level quantity as the registered
skip leg).

### Item 4 — learned-posterior routing twins

Per-seed: a fresh 8-bit `aif-e1` arm observes the 20-task Bernoulli
stream (deviation 7), then `r̂[b] = beliefs[b][0]` (the #57 `from_state`
read); the weighted twin is `TaskCoverageV2::weighted(required, r̂)`;
`REAL` still scores against the PLANTED `r`.

**The gotcha-24 ordering check holds 30/30**: `r̂[b*]` spans 0.0006–0.2357
against `r̂[strong]` 0.3811–0.9757 — a clean cross-bit ordering on top of
wildly non-calibrated levels, exactly the split gotcha 24 predicts
(ordering robust, levels recency-dominated). `REAL_l` median 129.8003 —
IDENTICAL to the unweighted median; strictly greater on only 8/30.

**Reading:** the learned-posterior pipeline a runtime would actually have
WORKS as an input (the weak bit is reliably identifiable), but pushed
through a rescale-only calculator it neither helps nor hurts the median —
consistent with the registered run's structural analysis that reliability
weighting at these coefficients re-ranks only through which bits partial
blocks cover.

### Consequences for the open threads

- **#63 (corrected routing registration):** item 4 supplies the missing
  feasibility fact — the learned-posterior input is available and
  ordering-robust on exactly the planted-weak-bit shape #63 needs; item 3
  independently re-confirms that partition-level skip predicates are
  vacuous under ANY calculator, so #63's block-level predicate is the only
  viable formulation. Both strengthen the filed requirements; neither
  changes them.
- **EQ queue:** the 12-bit inversion row strengthens the case (already in
  the pre-committed interpretation above) that later EQ entries should
  register on v2-style regimes.
- **Churn thread:** leave-side hysteresis joins the measured map as
  live-but-expensive; the state-lever conclusion (#56 lineage) is
  unchanged.
