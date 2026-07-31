# Pre-registration: corrected reliability-routing test (koalisi #63 / EQ1 tail)

_Registered 2026-07-31, BEFORE any implementation or run (posted to #63).
Owner-locked design decisions (2026-07-31, #63 design-lock comment D1–D6):
flip region = **both routes, separate legs** (planting confirmatory,
coefficients exploratory); skip predicate = **highest-value block** + an
attribution twin; draw = **rejection re-draw** coverage guarantee; seeds =
**180..210 fresh** (90..120 and 150..180 both stay reserved); the
learned-posterior twin **rides, exploratory/non-gating**. Born-on pins:
catgraph `v0.5.0` ×2, `aif-v0.11.0`, surrealdb-live-message `v0.2.1`
(v0.19.0 baseline). Example-only: zero library changes._

## Motivation (evidence chain)

The registered EQ1 lever 1 (#61, `docs/ab-report-K4-battery-v2.md` Part 5b)
landed `RUN-INVALID (sanity leg)` with a structural analysis showing the
registered criterion could not have produced a meaningful verdict on any run:
the partition-level skip predicate is vacuous (`search()` partitions the
whole pool), member-cost savings cannot express across partitions of a fixed
pool, and the instance draw missed pool coverage on 5/30 seeds. #63 filed the
corrected requirements; Part 5c (v0.19.0 addendum) added two feasibility
facts — partition-level skip predicates are vacuous under ANY calculator
(item 3), and learned reliability posteriors rank a planted weak bit
correctly 30/30 (item 4). This registration is the corrected test.

**Design-lock structural notes (verified against the code's value forms;
they shape the legs below):**

1. **Member savings cannot express at partition level even inside the flip
   region.** Moving the b\*-provider out of a full-coverage block is strictly
   total-value-negative under both calculators (member costs are constant
   across partitions; the coverage terms lose `(20/m)·Σ r`). The flip
   carries ONLY through top-block value ordering; the two argmax
   *partitions* may coincide, and structure-level payoff differences are
   structurally suppressed. Structure-level `REAL` is therefore **recorded
   exploratory, never gated** in this registration.
2. **Attribution needs a counterfactual twin.** The full block's weighted
   devaluation is partly driven by the non-b\* bits being weak (at the leg-A
   planting the b\*-specific window is `(100/m)·(0.35 − 0.02)` ≈ 4.7 value
   points), so per-seed skip firing requires the flip to DISAPPEAR when b\*
   alone is re-planted strong on the same draw.
3. **With a coverage-guaranteed draw the old partition-level sanity leg is
   vacuous** (partition union == pool union). The corrected sanity leg is
   block-level.

## Registered design

One additive part (**Part 6**) in `examples/strategy_comparison.rs`
(release build; every existing printed line of Parts 1–5c byte-identical —
the standing frozen-parts gate). Seeds **180..210**.

### Instance draw (both legs)

`draw_routing_instance_corrected(seed)` — the Part 5b draw shape (pool
`n ∈ 8..=16`, caps `k ∈ 1..=4` distinct bits, `m = |required|` uniform on
{7, 8}, `b*` uniform among required bits) with the **coverage guarantee**:
the WHOLE instance is re-drawn off the same per-seed `SplitMix64` stream
until the pool union covers `required` (rejection sampling; deterministic
per seed; rejection counts recorded as context). A fresh stream per seed, as
in Part 5b; the Part 5b draw function is untouched.

Plantings (constants, asserted before the run — see the coefficient gates):

- **Leg A planting**: `r[b*] = 0.02`, every other required bit **0.35**
  (non-required entries never read).
- **Leg A attribution counterfactual**: the same draw with `r[b*] = 0.35`
  (uniform 0.35 across required).
- **Leg C planting**: `r[b*] = 0.15`, every other required bit **0.98**.
- **Leg C attribution counterfactual**: the same draw with `r[b*] = 0.98`.

**Design-lock refinement (recorded):** the design-lock comment sketched
leg C at the leg-A planting; that is unsound — the product-form full bonus
collapses under the counterfactual too (`Π r` at seven 0.35-bits ≈ 6·10⁻⁴),
so the attribution conjunct could never fire. Leg C requires near-1
non-b\* bits; the 0.15/0.98 planting satisfies flip + counterfactual
non-flip with margin (gates below). This refines a design-lock sub-param;
no locked decision changes.

### Leg A (CONFIRMATORY) — flip-region planting, coefficients unchanged

Calculators: `TaskCoverageV2` exactly as registered in #61 (full
`100·mean(r over required)`, partial `w(m) = 80/m` per covered bit, member
cost 8·N). Per seed, three `search()` argmaxes at the same pinned
`PopulationConfig::default().with_seed(seed)`:

- `U` — unweighted (`r ≡ 1`),
- `W` — weighted at the leg-A planting,
- `W̄` — weighted at the attribution counterfactual.

**Top block** of a structure = the block maximizing the respective
calculator's `calculate_value` (the Part 5b diagnostic's `max_by` with
`partial_cmp`; on exact value ties `max_by` keeps the LAST maximal block in
the structure's canonical block order — deterministic for a fixed
structure). _[Amended pre-implementation 2026-07-31: the original wording
said "first-encountered"; the operational definition — the Part 5b helper —
was always `max_by`, whose tie-break is last-maximal.]_

**Coefficient gate (asserted in-code before the run):** the block-level flip
condition `8m > 100·r[b*] + 20·Σ_{b≠b*} r_b` HOLDS at the leg-A planting for
both m (m = 7: 56 > 44; m = 8: 64 > 51) and FAILS at the counterfactual for
every bit (m = 7: 56 < 77; m = 8: 64 < 84); the r ≡ 1 single-full-block
property (existing assert) is unchanged.

### Leg C (EXPLORATORY) — product-form full bonus, gated on degeneracy

Calculator `TaskCoverageV2P` (example-only): identical to `TaskCoverageV2`
except the full-coverage branch pays **`100·Π_{b∈required} r_b`**
(success-probability shape). At `r ≡ 1` it coincides with
`TaskCoverageV2::unweighted`, so leg C reuses argmax `U`. Per seed two more
argmaxes: `P` (leg-C planting) and `P̄` (leg-C counterfactual), same
`PopulationConfig`.

**Coefficient gate (asserted):** r ≡ 1 full-coverage optimality with margin
(`100 > (80/m)(m−1) + 8` for every m ∈ 2..=8; worst case m = 8: 100 > 78);
equal-size flip at the leg-C planting (m = 7: partial-skip 67.2 > full 13.3;
m = 8: 68.6 > 13.0); counterfactual non-flip including the one-member-smaller
comparison (m = 7: 86.8 > 67.2 + 8; m = 8: 85.1 > 68.6 + 8).

**Degeneracy gate (runs FIRST, run-and-label-context — the ratified
deviation-6 reading):** on the 30 leg-C-planted instances, compare `P`
against the all-singletons structure under `TaskCoverageV2P`; if
all-singletons ties or beats `P` on ≥ 15/30, leg C is labeled
`DEGENERATE (context only)` and its rows carry no reading beyond the
degeneracy mechanism. Either way every leg-C row is measured and printed.

### Leg L (EXPLORATORY) — learned-posterior twin

Per seed, on the leg-A planting: a fresh 8-bit `PersistentAifArm`
(`e1_config()`, observed-into only) consumes 20 tasks of independent per-bit
Bernoulli(`r_b`) outcomes off a salted stream (the Part 5c item-4 pipeline:
`SplitMix64::new(seed ^ P6_TWIN_SEED_SALT)`, a fresh salt constant so item 4's
stream is untouched); `r̂[b] = beliefs[b][0]`; argmax `L` from
`TaskCoverageV2::weighted(required, r̂)`. Recorded: the gotcha-24 ordering
check (`r̂[b*] <` the lowest-indexed strong bit's `r̂`), the top-block skip
predicate for `L` vs `U`, and `REAL`. No bar, no verdict; note the
0.02-vs-0.35 gap is a smaller ordering regime than item 4's 0.15-vs-0.9 —
that is part of what the leg measures.

### Yardstick

`REAL` = the #61 `real_payoff` closed form at the respective leg's PLANTED
reliability vector (structure-level, exploratory per structural note 1;
per-block `REAL` is near-tautological w.r.t. the weighted calculators and is
NOT used as a leg).

## Confirmatory criteria (leg A only, seeds 180..210)

**H-BR (block-level routing — reliability weighting demotes weak-bit blocks
in its own ranking, attributably to b\*):**

- **Sanity leg (run-invalidating if it fails):** the top block of `U` (under
  the unweighted calculator) achieves full required coverage on ≥ **27/30**
  seeds. (Non-vacuous: PSO non-exhaustiveness and the partial-term
  double-count can break it.)
- **Skip leg:** on ≥ **18/30** seeds, ALL of, per seed:
  1. the top block of `W` (weighted calculator) omits `b*`;
  2. the top block of `U` (unweighted calculator) covers `b*`;
  3. the top block of `W̄` (counterfactual calculator) covers `b*`
     (attribution: the flip vanishes when b\* alone is strong).

**Verdict rule (pre-committed):** `VALIDATED (block-routing)` = sanity ∧
skip; `FALSIFIED (block-routing)` = sanity ∧ ¬skip; `RUN-INVALID (sanity
leg)` otherwise. Bars inherit the family's 27/30 sanity and 18/30 (60%)
consistency conventions; nothing is tuned post-hoc. Structure-level `REAL`
medians and per-seed deltas, the count of seeds where `W`'s and `U`'s
*partitions* differ at all, rejection-sampling counts, and the raw firing
counts of each skip conjunct are all recorded as context, never gated.

## Run-validity gates (run-invalidating)

- **X-A (frozen parts):** every existing printed line of Parts 1–5c
  byte-identical against a fresh pre-change baseline (standing protocol:
  single writer per output path, unique paths, no timeout wrapper, latency
  lines excluded per the standing exclusion).
- **X-B (coefficient gates):** the leg-A and leg-C assertions above, in-code,
  before the battery loop.
- **X-C (sanity leg):** as stated in H-BR.
- Existing test suites at their v0.19.0 counts (103/159/125/181/123/146)
  plus whatever Part 6 unit tests add; the example binary's own tests stay
  green.

## Pre-committed interpretation

- `VALIDATED (block-routing)` ⇒ reliability weighting at the v2 coefficients
  routes at block level where the flip condition holds — gotcha 24's
  rescale-not-reroute becomes coefficient- and level-scoped (partition-level
  suppression stands, per structural note 1); the #57 slow-loop fitness
  story gains a measured block-level routing regime. Claim discipline: the
  headline is block-level ONLY — no structure-level routing claim is
  available from this design.
- `FALSIFIED (block-routing)` ⇒ even inside the algebraic flip region the
  search's block ranking does not route attributably (e.g. the b\*-window is
  too narrow against pool noise) — a genuine negative for reliability-driven
  block routing at these coefficients; gotcha 24 is strengthened.
- `RUN-INVALID (sanity leg)` ⇒ the r ≡ 1 optimum fails to realize through
  `search()` at this planting/pool shape; a formulation problem again — file
  the follow-up, draw no routing conclusion.
- Leg C: if non-degenerate, its skip/attribution rows read as "a
  reliability-INsensitive-full-bonus family can make routing
  reliability-driven" — exploratory context for any future value-model
  design; if `DEGENERATE (context only)`, the third gotcha-21 mechanism
  extends to the product form and per-block success-probability bonuses
  join the do-not-use list for structure search (gotcha-25 addendum
  material). No verdict either way.
- Leg L: feasibility context for the runtime pipeline only.
- The arm question stays CLOSED (#54 B+D); nothing here reopens it.

## Provenance

Owner decisions recorded on #63 (design-lock comment, 2026-07-31, D1–D6 +
the ratified run-and-label-context gate reading and the EQ3–EQ5 v2-regime
default). Evidence chain: `docs/ab-report-K4-battery-v2.md` (Part 5b
structural analysis + Part 5c items 3/4), `docs/prereg-K4-battery-v2.md`
(§Part 5b, §H-R), gotchas 21/24/25, `src/decision/reliability_value.rs`
(#57). Seeds ledger after this registration: 0..30 / 30..60 / 60..90 /
120..150 consumed; 180..210 consumed by this run; 90..120 (lockout) and
150..180 (replication) reserved. Engine pins unchanged from v0.19.0. Zero
library changes; example + docs only.
