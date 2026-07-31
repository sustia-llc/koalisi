# A/B report: corrected block-level routing test (koalisi #63 / EQ1 tail)

> **RESULTS (2026-07-31):**
> **H-BR (leg A, confirmatory): `FALSIFIED (block-routing)`** — sanity
> 28/30 ≥ 27 (PASS, the run is valid), skip 4/30 < 18 (FAIL). Per the
> pre-committed interpretation this is mechanism-scoped, and the registered
> mechanism diagnostics measure the scope directly: the b\*-window
> (width `100·Δr/m` = 4.71 at m = 7 / 4.13 at m = 8) barely exceeds the
> spacing of the competing singleton-value lattice (4.00 / 3.50), and the
> four firing seeds are exactly those where the best leftover block's value
> landed inside the window. "Routing does not fire at these coefficients on
> this draw" is measured; "reliability weighting cannot route" is NOT a
> licensed reading.
> **Leg C (exploratory, product-form full bonus): `DEGENERATE (context
> only)`** — all-singletons ties or beats the `search()` argmax on 30/30
> (bar 15). A FOURTH gotcha-21 mechanism: a multiplicative
> (success-probability) full bonus collapses at sub-1 reliabilities and can
> no longer pay for the overlap a merge destroys.
> **Leg L (exploratory, learned posterior): ordering 30/30** — the learned
> `r̂` ranks the planted weak bit below the reference strong bit on every
> seed even in the much harder 0.02-vs-0.35 regime (item 4 measured
> 0.15-vs-0.9). The runtime-feasible learned input remains ordering-robust;
> levels stay wildly uncalibrated (median spread 0.1222), per gotcha 24.

_Registered design: `docs/prereg-K4-routing-corrected.md` (committed 15eccd1
+ pre-implementation amendment 8120c7b, posted to #63 BEFORE implementation;
owner design-lock D1–D6 on #63 BEFORE the prereg). Run 2026-07-31,
`--release`, seeds **180..210** (90..120 and 150..180 both remain reserved).
Pins: catgraph `v0.5.0` ×2 · `aif-v0.11.0` · surrealdb-live-message `v0.2.1`
(the v0.19.0 born-on set). **Zero library changes** — example
(`strategy_comparison` Part 6) + docs only._

## Run-validity gates

| Gate | Registered requirement | Result |
|---|---|---|
| X-A | Every pre-existing printed line of Parts 1–5c byte-identical vs a fresh pre-change baseline | **PASS** (full-run diff: 20 differing lines, all latency, the standing exclusion) |
| X-B | Leg-A flip condition holds at the planting (m = 7: 56 > 44; m = 8: 64 > 51) and fails at the uniform counterfactual (56 ≤ 77; 64 ≤ 84); leg-C r ≡ 1 margin (100 > 78 worst case), equal-size flip (67.2 > 13.3; 68.6 > 13.0), counterfactual non-flip incl. one member smaller (86.8 > 75.2; 85.1 > 76.6) | **PASS** (asserted in-code before the loop, algebraic forms over the registered constants) |
| X-C | Sanity leg: `U`'s top block achieves full required coverage on ≥ 27/30 | **PASS — 28/30** |

Suites at the run commit: 103 default / 159 `decision` / 125 `magnitude` /
**181** `decision,magnitude` / 123 `persistence` / 146
`persistence,magnitude` — all unchanged from v0.19.0 (Part 6's five new
tests live in the example binary, which runs only under `--example`:
**26** there, 25 passed + 1 pre-existing release-only `#[ignore]`). Default
clippy `--all-targets` clean.

## Leg A — confirmatory block-level routing

`TaskCoverageV2` (registered v2 coefficients, unchanged) vs its
reliability-weighted twin at the flip-region planting `r[b*] = 0.02` /
others 0.35, plus the uniform-0.35 attribution counterfactual `W̄`.
Coverage-guaranteed rejection re-draw (2 re-draws across 30 seeds; the #61
draw missed coverage on 5/30). Skip predicate = highest-value block under
each argmax's own calculator (`max_by`, last-maximal on ties).

- **Sanity: 28/30** (bar 27). The one C2 failure is seed **196** — the one
  seed whose minimum-multiplicity cover (9) exceeds the analytic
  feasibility bound `1.25·m` (8.75), i.e. the draw supplied no
  low-redundancy cover and no search could have found a full-coverage top
  block there. The registered per-seed `min_mult` column makes this
  attribution immediate — the diagnostic #61 had to reconstruct post-hoc.
  The second sanity miss is seed **188** (its `win_lo` −3.77 identifies a
  partial top block covering b\* + 3 of 7 bits, so C2 still holds there);
  its cover was feasible (`min_mult` 8 < 8.75) — a search miss, not a draw
  miss, and within the bar's slack.
- **Skip: 4/30** (bar 18) — seeds 181, 187, 195, 209. Per-conjunct counts:
  C1 (W-top omits b\*) 6/30 · C2 29/30 · C3 (control) 28/30; the
  control-failure channel (C1 ∧ C2 ∧ ¬C3, a size effect of the uniform
  counterfactual, not routing evidence) accounts for only **1/30**. The
  shortfall is ¬C1: on 24/30 seeds the weighted argmax's top block still
  covers b\*.
- **Channel: ranking, not formation.** `W`'s argmax contains a
  full-coverage block on 29/30 seeds (all but 196) — the weighted model
  keeps FORMING the full block; on the firing seeds it merely ranks a
  leftover block above it. The hypothesis's "demotes weak-bit blocks in its
  own ranking" is the channel the data show.
- **Mechanism (the registered diagnostics):** firing requires the best
  leftover block's weighted value `v_left` to land strictly inside the
  window (`win_lo`, `win_hi`) that the b\* planting opens at `U`'s top
  block — width 4.71 (m = 7) / 4.13 (m = 8) against a leftover-value
  lattice of spacing 4.00 / 3.50. The four firing seeds are exactly the
  window hits (e.g. 181: `v_left` 8.00 ∈ (6.29, 11.00)); on the modal
  (s = 3, m = 8) configuration `v_left` = 6.00 sits just below
  `win_lo` = 6.88 — structurally unfirable regardless of routing. A
  pre-run exact-optimum replica (PSO removed) predicted 3/30 fired and
  sanity 29/30; the live search landed at 4/30 and 28/30 — PSO noise of
  ±1 seed around the structural prediction.
- **Attribution validity (measured surprise):** `W̄`'s and `U`'s argmax
  partitions differ on **27/30** seeds, against an exact-arithmetic
  expectation of 0 (at uniform reliability the weighted total is an
  increasing affine map of the unweighted total). The divergence is
  float-tie trajectory splitting: this fitness landscape is saturated with
  EXACT ties (disjoint-set merges cost zero — the gotcha-21 tie family),
  and the counterfactual's independently-rounded coverage sums turn some
  ties into ±1-ulp strict orderings, sending the PSO down different
  trajectories. Consequence: C1-vs-C3 is predominantly an
  intent-to-treat contrast across different partitions rather than a
  fixed-partition two-point contrast. The control still behaves (C3 holds
  28/30); the caveat scopes per-seed attribution strength, not the
  aggregate.
- Structure-level `REAL` (context, never gated): medians REAL_w −3.4271 ·
  REAL_u −5.1989; ΔREAL is exactly 0.0000 on 15/30 seeds and two-sided on
  the rest (8 positive, 6 negative, one +0.0007). Neither calculator is a REAL-maximizer at this planting (the
  weighted model credits full coverage `(20/m)·Σ r` ≈ 6.06 where REAL
  credits `20·Π r` ≈ 0.0007), and REAL itself prefers full coverage over
  the b\*-skip by only ~0.23 per block at equal size — the yardstick is
  nearly indifferent exactly where the models disagree.

**Verdict (pre-committed rule): `FALSIFIED (block-routing)`** — sanity ∧
¬skip. Applying the prereg's pre-committed interpretation: "even inside the
algebraic flip region the search's block ranking does not route attributably
(e.g. the b\*-window is too narrow against pool noise)". The mechanism
diagnostics confirm the parenthetical is the operative clause: the window is
real (the four hits fire, C1 ∧ C3 behaves) but its width barely exceeds the
resolution of the competing block values, so the 18/30 bar was never
reachable on this draw distribution. Gotcha 24's rescale-not-reroute is
strengthened from "structurally impossible (v1 coefficients)" to "measured
non-firing (v2 coefficients, flip-region planting)" — **scoped to this
window-vs-lattice geometry**, not to reliability weighting per se.

## Leg C — product-form full bonus (exploratory)

`TaskCoverageV2P` (full-coverage bonus `100·Π_{b∈required} r_b`; partial
term and member cost unchanged; coincides exactly with
`TaskCoverageV2::unweighted` at r ≡ 1), planting `r[b*] = 0.15` / others
0.98, uniform-0.98 counterfactual.

**Degeneracy gate (ran FIRST, run-and-label-context): `DEGENERATE (context
only)` — 30/30** all-singletons ties-or-beats the argmax (bar 15); the
argmax literally IS all-singletons on 0/30 and the grand coalition
ties-or-beats on 0/30, so the 30 is genuine model degeneracy plus PSO
shortfall against a flat landscape, not a boundary-argmax artifact. This is
a **fourth gotcha-21 mechanism**: a multiplicative success-probability
bonus collapses at any materially-sub-1 planting (`100·Π r` ≈ 13.3 at the
leg-C planting vs a partial-skip block at 67.2), so merging to full
coverage stops paying for the per-block partial-term overlap it destroys —
the item-3 double-count made worse by a collapsed bonus. Context rows
recorded per the registration (three-conjunct skip fires 13/30; REAL_p
156.47 vs REAL_u 146.26 — cross-objective by construction, no routing
reading; see the printed direction-disagreement disclosure).

**Interpretation correction (recorded — ledger item 6):** the prereg's
pre-committed interpretation for a NON-degenerate leg C describes the
family as "reliability-INsensitive-full-bonus". That mischaracterizes
`TaskCoverageV2P`: its full bonus is maximally reliability-SENSITIVE (a
product), and it routes by bonus-collapse, the opposite mechanism of
gotcha 25's "reliability-insensitive full bonus" remedy. The clause is moot
on this run (the leg is DEGENERATE) and must not be transcribed into
gotcha 25 as written.

## Leg L — learned-posterior twin (exploratory)

Per seed: fresh 8-bit `aif-e1` arm, 20-task per-bit Bernoulli stream at the
leg-A planting (own salt; item 4's stream untouched), `r̂[b] =
beliefs[b][0]`, argmax under `TaskCoverageV2::weighted(required, r̂)`.

- **Ordering 30/30** — `r̂[b*] < r̂[strong]` on every seed, in a regime
  deliberately harder than item 4's (absolute gap 0.33 vs 0.75, and the
  reference strong bit itself fails most tasks at Bernoulli(0.35), so both
  bits read low). The gotcha-24 split holds at its second point: ordering
  robust, levels meaningless (`r̂[b*]` reads 0.0000–0.0063; median spread
  0.1222).
- The skip column (L-top omits b\* 22/30; jointly with U-top covering it
  21/30) is mostly a scale artifact where the spread is small — a
  near-uniform `r̂` collapses `L`'s partition ranking onto `U`'s, leaving
  only the member-cost-driven top-block shift — and carries no routing
  reading. REAL_l median −4.9708 ≈ REAL_u −5.1989.
- **Consequence:** the #63 feasibility fact survives its harder test — a
  runtime's learned posterior identifies the weak bit reliably even at
  near-floor reliabilities. What is missing for learned-input routing is
  not the input; it is a value model whose flip region is wider than its
  competitor lattice (leg A's geometry problem).

## Deviation / implementation ledger

1. **Pre-implementation prereg amendment** (8120c7b, before any code): the
   top-block tie-break wording corrected from "first-encountered" to
   `max_by`'s actual last-maximal semantics; the operational definition
   (the Part 5b helper) was always the governing one.
2. **Leg-C planting refinement** (recorded in the prereg §"Design-lock
   refinement"): the design-lock sketched leg C at the leg-A planting;
   unsound (the product bonus collapses under the counterfactual too, so
   attribution could never fire). Leg C runs at 0.15/0.98 with flip +
   counterfactual non-flip asserted (X-B). No locked decision changed.
3. **Degeneracy tie tolerance**: the gate's ties-or-beats comparison uses
   the frozen item-3 tolerance (`≥ best − 1e-9`, ~1e-11 relative here);
   the prereg wrote "ties or beats" without a tolerance. Bias direction is
   toward DEGENERATE, i.e. toward withholding a claim; on this run the
   count is 30/30 with or without it.
4. **Counterfactual gate comparison kept non-strict** (`8m ≤ rhs`), the
   exact logical negation of the registered flip condition — an owner call
   resolving a reviewer split (one lens wanted `<` to mirror the prereg's
   illustrative strict numerals, which are properties of the values, not
   the condition). Values sit far from the boundary either way.
5. **Review-driven non-gating additions** (3-lens review — correctness /
   registration-conformance / modeling-semantics — applied BEFORE the
   official run; 1 blocking + 11 important + 12 minor findings, all
   applied or owner-adjudicated): the mechanism-diagnostics table +
   mechanism-scope paragraph (the blocking finding), the attribution-
   validity and control-failure counts, the C2-entailment note, the leg-C
   REAL direction disclosure, leg-L spread/joint columns, and
   claim-precision rewordings (degeneracy label, partition-constancy
   clauses, stylized-configuration qualifier, stream-independence wording).
   No registered bar, predicate, or verdict string moved.
6. **Prereg interpretation correction** for leg C — see the leg C section;
   recorded so gotcha 25 never gains the "reliability-INsensitive" entry.
7. **Suite-count clarification**: `cargo test` totals are unchanged
   (example `#[cfg(test)]` modules run only under `--example`; that binary
   went 21 → 26 tests).
8. **Run provenance**: a pre-findings binary (identical battery logic,
   fewer context columns) produced byte-identical quality numbers on all
   three legs — the review cycle changed disclosure, not results. The
   official run is the one quoted here; X-A was diffed against a fresh
   pre-change baseline from the same session.

## Pre-committed interpretation, applied

- `FALSIFIED (block-routing)` ⇒ reliability-driven block routing did not
  fire at the family's consistency bar even inside the algebraic flip
  region — a genuine negative for reliability-weighted routing **at these
  coefficients and this draw geometry**. Gotcha 24 strengthens as scoped
  above. Any future routing attempt must widen the window-to-lattice ratio
  by design (coefficients whose Δr window spans multiple leftover values),
  and that is a new registration.
- Leg C ⇒ per-block multiplicative success-probability bonuses join the
  gotcha-21 do-not-use list for structure search (fourth mechanism).
- Leg L ⇒ the learned-posterior input remains ordering-robust at
  near-floor reliabilities; feasibility for any future routing design is
  re-confirmed, and the blocker is the value-model geometry, not the
  signal.
- The arm question stays CLOSED (#54 B + D); nothing here reopens it.

## Provenance

Owner decisions: #63 design-lock comment (D1–D6, 2026-07-31) + the M6
adjudication (ledger item 4). Registered doc: `docs/prereg-K4-routing-
corrected.md`. Evidence chain: `docs/ab-report-K4-battery-v2.md` (Part 5b
structural analysis + Part 5c items 3/4), gotchas 21/24/25,
`src/decision/reliability_value.rs` (#57). Seeds ledger after this run:
0..30 / 30..60 / 60..90 / 120..150 / **180..210** consumed; 90..120
(lockout) and 150..180 (replication) reserved. Engine pins unchanged from
v0.19.0. Zero library changes; example + docs only.
