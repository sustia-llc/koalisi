# Pre-registration: K4 EQ5a — process-structured tasks (workflows as string diagrams)

**Status: REGISTERED.** Committed BEFORE implementation. Design-lock of
record: [koalisi #76](https://github.com/sustia-llc/koalisi/issues/76)
(owner design-lock comment, 2026-08-05, D1–D11 + the verdict rule + the
EQ5a/EQ5b split). Pre-run amendments are legitimate per the standing
protocol (committed to this doc + posted to #76 BEFORE any affected
code); after the official run this document is immutable.

## 1. Registration statement

- **Hypothesis (EQ5, stack E-queue entry 5; EQ5a = the process half):** a
  coalition's compositional process structure carries value signal. On a
  workflow-structured world, an arm that **optimizes the process before
  staffing it** — bounded convex-DPO rewriting over content, catgraph
  `prop::presentation::rewrite` — outperforms the same arm staffing the
  workflow as written.
- **Split (design-lock):** EQ5a registers the process arm. **EQ5b** — the
  two-engine typed contest (`GroupAgent` with role-slotted internals over
  arm-E1's persistent world model, vs this arm) — is a separate document
  on its own fresh seed block. No EQ5a leg depends on it.
- **Born on:** koalisi `v0.26.0` (catgraph `v0.8.0` ×2 — the pin-first
  re-pin, [PR #77](https://github.com/sustia-llc/koalisi/pull/77), merge
  `e28b3d9`, drift check clean on all eight suites, default clippy, and
  a byte-identical Parts 1–8 battery reproduction; `aif-v0.12.0`; MSRV
  1.93). `catgraph-syntax v0.8.0` arrives with the gated feature in the
  implementation PR, in lockstep with the other two (D6/D11). Regime:
  v2-style draw + typed (the standing E-queue D6 decision), extended to
  workflows per §2.
- **Seeds:** `270..300`, fresh (90..120 + 150..180 remain reserved; no
  release implied). EQ5b draws its own block.
- **Battery placement:** `examples/strategy_comparison.rs` **Part 9**;
  frozen Parts 1–8 are the byte-identity regression gate (§6 X-battery).
- **Falsifiability posture:** this lineage has repeatedly planted signals
  that failed to convert (gotcha 24 rescale-not-reroute; gotcha 25
  unsatisfiable predicate; gotcha 26 window-vs-lattice). Three structural
  choices here exist so the test can fail: the optimizer minimizes
  **process** cost, which the confirmatory uniform cells are not allowed
  to align with the scorer (§5); a cheaper writing may **concentrate**
  demand on a scarce `(bit, role)`, and infeasible optimized demand is
  **counted, never re-drawn** (§2); and the control is the **already
  validated typed arm** (§3), so EQ5a cannot re-harvest EQ4's margin.

## 2. World: the v2w regime (workflow-structured tasks)

Per-seed, the v2t prefix is drawn first and the workflow draw is APPENDED
off the same SplitMix64 stream (the #46/#48/#72 shared-prefix discipline —
the v2t prefix of the stream stays bit-identical to a pure-v2t draw of the
same seed):

- **v2t prefix, unchanged (EQ4 §2):** `draw_prefix_v2` (pool `n ∈ 4..=16`,
  worker caps `1..=4` bits, `|required| ∈ 2..=8`, 8-bit universe, seeded
  arrival shuffle), then `R = 3` worker roles and per-required-bit role
  tags, uniform.
- **Steps.** Each tagged required bit `(b, r)` becomes a **step generator**
  `s_{b,r} : r → r` — role-preserving, so a step consumes and produces a
  wire of its own role color. The role alphabet `Λ = {R0, R1, R2}` is
  `PropSignature::Color`; a fieldless enum satisfies the whole bound stack
  (`Clone+Eq+Hash+Debug`, `+Ord` for spiders, `+Copy` for the cospan
  functor).
- **Shape draw.** Steps are grouped by role; within a role the draw builds
  a sequential chain (`Free::compose` — legal exactly because `s : r → r`
  is endo on its color), and roles are combined with `Free::tensor`.
  Per-role **spiders** (`FrobeniusOr` δ/μ, feature-gated `catgraph-syntax`)
  fan a role wire out and back in, so a step may feed two same-role
  successors. Colors are pinned with `ColoredExpr::new` — the only way to
  pin them (`id`/`braid` are color-polymorphic by design). Draw parameters
  (chain length, fan-out probability) are pinned in the harness before the
  run.
- **Verified composition (checked, not assumed).** The signature the whole
  battery runs over is `FrobeniusOr<Step>` — spiders and user steps are
  the two arms of one generator type; `ColoredExpr::new(source_word, expr)`
  pins the colors; and `optimize` / `cost_of` / `canonical_key` accept that
  signature directly. This is the exact shape exercised by cg's own W1
  example (`catgraph-syntax/examples/workflow_dedup.rs` at `v0.8.0`,
  including a priced `per_gen` over `FrobeniusOr<Step>`). No adapter is
  owed.
- **Demand.** A workflow's demand is the multiset of `(bit, role)` over its
  **generator occurrences**. Coverage is per distinct pair: a step
  occurrence is covered iff some coalition member of role `r` holds bit
  `b`, so **multiplicity prices the process but does not add coverage
  demand** — the distinction the rule theory (§4) is designed to exploit.
- **Feasibility guarantee (gotcha 25 / #63 lesson).** The **as-written**
  demand is checked for role-matched feasibility in the pool; an
  infeasible task is re-drawn (full task draw — size, bits, tags, shape;
  ≤ 1000 total attempts, initial draw = attempt 1; still-infeasible after
  1000 is RUN-INVALID, expected never). **The optimized demand is NOT
  re-drawn** — an optimizer that concentrates demand on a `(bit, role)` no
  pool member holds is the registered failure mode, and its rate is a
  mandatory disclosure (§5 E-conc), not a draw condition.
- **Degenerate world (for §6 X-reduce):** every task's diagram is the
  all-parallel `tensor` of its distinct `(b, r)` steps, empty rule set,
  uniform cost. The demand multiset is then exactly the v2t tagged
  required set and the metric reduces to EQ4's. **The degenerate shape
  draw consumes ZERO stream draws** — load-bearing for the gate: the
  instances it produces are then bit-for-bit the v2t instances of the
  same seed, so X-reduce compares two code paths on one world rather
  than two worlds.

## 3. Arms

| arm | policy | role in this registration |
|---|---|---|
| `wf-asis` | typed magnitude (`with_role_modulation`, oracle `ρ = δ`) staffing the **as-written** workflow | **control** |
| `wf-rw-u` | requirement rewriting, uniform cost `\|_\| 1` | confirmatory cell |
| `wf-rw-p` | requirement rewriting, staffing-priced cost | confirmatory cell |
| `wf-val-u` | valuation-only, uniform cost | confirmatory cell |
| `wf-val-p` | valuation-only, staffing-priced cost | confirmatory cell |
| `mag` | frozen untyped `MagnitudePolicy` over the flattened demand | context, non-gating |
| `aif` / `arm-E1` | scalar bridge / `PersistentAifArm` v5 E1 config | context, non-gating |

The control is the **EQ4-validated typed arm**, deliberately: EQ5a
measures process signal *beyond* role signal. All arms decide over the
same arrival stream under the standing battery protocol (bootstrap first
arrival, one leave sweep per task, churn = all removals, latency
record-only).

## 4. The process arms (D3 × D5, locked)

**Rule theory (D4, pinned pre-run).** A hand-authored theory of 3–5
oriented rules over the step alphabet, each built through
`RewriteRule::new` (which enforces parallel sides, re-validated
well-formedness, non-empty `lhs`, mono left interface). It must contain at
least one **fusion** rule — replacing two distinct steps by one — because
an idempotence-only theory changes occurrence count without changing
distinct demand, and the rewriting cells would be structurally inert (the
gotcha-25 failure class, avoided by construction). Spider SCFM equations
enter as ordinary rules over opaque generators (cg's locked B1 substrate).
The exact rules are pinned in the harness and printed with the run.

**Fuel (D4).** Confirmatory cells run at one pinned budget **F = 256
applications**. The sweep `{32, 128, 512, 2048}` is a registered
exploratory axis (§5 E-fuel) measuring how much of any margin is
fuel-bought — a direct probe of cg's registered no-termination posture.
`RewriteOutcome::fuel_exhausted()` on any seed is a **mandatory
disclosure**, not a RUN-INVALID condition.

**Cost models (D5).**
- **uniform** — `per_gen = |_| 1`, cg's default: generator count. The arm
  is **metric-blind**; a pass means process cost genuinely converts.
- **staffing-priced** — `per_gen(s_{b,r})` weighted by the scarcity of
  `(b, r)` in the pool (pinned formula, computed from the drawn pool
  before any decision). Higher pass probability, weaker claim; registered
  as its own cells rather than substituted for the uniform ones.

**Mechanisms (D3).**
- **requirement rewriting** — the arm calls `optimize(as_written, rules,
  F, per_gen)`, takes `RewriteOutcome::best()` as its **declared writing**,
  and staffs that. Membership decisions change.
- **valuation-only** — demand is the as-written one; the process cost of
  the *current* coalition's writing enters the decision score as the
  additive term `−λ · cost_of(content, per_gen)`, with **λ a single
  constant pinned before the run** (its value recorded here by pre-run
  amendment if not fixed at first commit; any λ grid is exploratory and
  non-gating — an unpinned coefficient would be a free parameter and this
  lineage does not grant those). Demand and declared writing are
  unchanged. Prior is low:
  score-space levers have measured inert twice in this lineage (gotcha 23
  Part 4f, gotcha 25 join rail) — registered because the contrast
  mechanism-scopes any rewriting result.

**Fairness clause (registered, load-bearing).** Rules are sound equalities
modulo the theory, so a coalition staffing a rewritten writing does the
same job as one staffing the original — this is exactly what BGKSZ Thm 5.6
licenses per step. Scoring therefore runs against each arm's **declared
writing**, and every declared writing that is not the as-written one is
`replay`-verified against the registered rules and `content_eq`-checked
against the reported representative before it is scored (§6 S-sound). An
arm may not grade its own homework by declaring an unverified writing, and
the control is not penalised for a legitimate alternative it did not take.
Consequence to state in the report: `cov_eff` denominators differ across
arms by design — when both complete, the ratio term is 1.0 for both and
the contrast comes through **member count**, which is the claimed
advantage.

**Placement (D8).** Library, behind a new feature (`process`, implying
`magnitude` and gating the `catgraph-syntax` dependency): the workflow
type, demand extraction, and the optimize hook a runtime would use.
Battery scaffolding — world draw, scorers, tables — stays example-side.
Upstream error handling unchanged: `CatgraphError` ⇒ decline / `-∞`, never
a panic. `optimize` failure on a task is a decline-and-count, never a
panic and never a silent as-written fallback.

## 5. Registered legs

- **H-P (confirmatory, family-wise).** On the v2w world, seeds 270..300,
  each of the four cells against `wf-asis`: PRIMARY median ≥ **1.4×** the
  control's AND strictly superior on ≥ **70 %** of seeds (**≥ 21/30**).
  Both conjuncts, in the same cell. Any cell may carry the verdict; the
  raised bar (from the lineage's standing 1.25× / 60 %) pays for the four
  looks and is pinned pre-run. **All four cells report regardless of
  outcome** — no cell-shopping, no post-hoc bar movement.
- **E-fuel (registered exploratory, non-gating).** The `{32, 128, 512,
  2048}` sweep on the rewriting cells: margin vs fuel, `fuel_exhausted`
  rate, `states_explored` medians.
- **E-conc (registered disclosure, non-gating).** Rate at which the
  optimized demand is role-matched **infeasible** in the pool, and the
  per-seed contribution of those tasks to any margin — the registered
  failure mode, measured.
- **E-dedup (registered exploratory, non-gating — the a1 half).** Over the
  drawn corpus: how many arrivals are distinct as written but equal as
  content (`canonical_key` collisions), the bucket-size distribution, and
  what a content-keyed table buys over a writing-keyed one. Reported as a
  corpus fact and a latency fact; it cannot carry the verdict (EQ3 spent
  this lineage's appetite for latency legs).
- **E-ceil (registered exploratory, non-gating, example-side).** A
  fully-informed reference arm within the rewriting family — NOT a
  supremum (the #72 A2.5 correction). `cost_of` is a sum of per-generator
  weights over occurrences, so "minimise **distinct** `(bit, role)`
  demand" is **not expressible** as a `per_gen` at all; E-ceil therefore
  runs as (i) a large-fuel scarcity-priced cell, plus (ii) on a
  pinned subsample, a harness-side brute-force minimum-distinct-demand
  search over the class enumerated by `optimize` at high fuel. The gap
  between E-ceil and the rewriting cells mechanism-scopes any H-P
  failure: optimizer/objective limit vs no convertible signal present.
- **Instrumentation (non-gating):** per-run medians of `initial_cost`,
  `best_cost`, step count, `states_explored`; distinct-demand reduction
  distribution; the printed rule theory.

## 6. Gates (any failure ⇒ RUN-INVALID)

- **X-battery** — frozen Parts 1–8 byte-identical on every
  quality/churn/verdict line vs a fresh pre-change baseline (latency-only
  diffs permitted, standing exclusion).
- **X-reduce** — on the degenerate world (§2), all 30 seeds: `wf-asis`
  reproduces the EQ4 typed arm's **acts + per-seed PRIMARY + churn
  bit-identically**, and the rewriting cells (empty rule set) reproduce
  `wf-asis` bit-identically. Raw score bits may differ in low bits where
  evaluation re-associates — the EQ3 H-par′ lesson: parity gates on
  decisions, not float bit patterns.
- **S-sound** — every declared writing that is not the as-written one
  `replay`s under the registered rules and the result `content_eq`s the
  reported representative, all tasks, all seeds. The trace is the
  ground-truth check cg designed it to be.
- **S-dedup** — `canonical_key(a) == canonical_key(b) ⟺ content_eq(a, b)`
  on the drawn corpus, plus a **like-with-like** assert: one content entry
  point per dedup table (mono and colored contents of the same expression
  differ — cg seam note). No iteration-order dependence anywhere
  (`ContentKey` is not `Ord`).

## 7. Verdict labels (pre-committed)

- `VALIDATED (process structure)` — at least one H-P cell passes both
  conjuncts at the family-wise bar, all §6 gates hold. The report names
  the carrying cell and its mechanism.
- `FALSIFIED (process structure)` — §6 gates hold, no cell clears the bar.
  E-ceil / E-conc / E-fuel mechanism-scoping is reported but cannot
  upgrade the verdict.
- `RUN-INVALID` — any §6 gate fails (a corrected registration would be a
  new document, #63 precedent).

Pre-committed interpretations: the v1/v2 K4 verdicts, EQ3's and EQ4's
verdicts, and the #54 arm question (mag = demonstrated default, FINAL) are
UNTOUCHED regardless of outcome. A `VALIDATED (process structure)` result
speaks to the process-vs-as-written contrast within the typed magnitude
family, not to the mag-vs-aif arm question — that contest is EQ5b's.
Latency is never gating in this registration.

## 8. Report

`docs/ab-report-K4-eq5a-process-structured.md` — registered sections
mirror §5/§6; implementation/deviation ledger mandatory; appended-addendum
convention applies to any follow-up.

## Amendment 1 (pre-run, 2026-08-05 — posted to #76 BEFORE implementation)

§4 deliberately left four registered variables to be pinned before the
run. They are pinned here, ahead of any implementation, so they are the
registration's choices rather than the implementer's. No bar, seed, leg,
or gate changes.

- **A1.1 — the rule theory (D4).** Three schemas, each closed over the
  `(bit, role)` index set; the theory is their closure, printed in full
  with the run. Step generators are `s_{b,r} : r → r` (role-preserving),
  so every same-role composite is parallel to every other and
  `RewriteRule::new`'s equal-source-and-target-words condition is met by
  construction.
  1. **Idempotence** — `s_{b,r} ; s_{b,r} ⇒ s_{b,r}`, all 8 bits × 3
     roles. Reduces occurrence count; leaves distinct demand unchanged.
  2. **Fusion** — one designated rule per role:
     `s_{b1(r),r} ; s_{b2(r),r} ⇒ s_{b3(r),r}` with
     `b1(r) = 2r`, `b2(r) = 2r + 1`, `b3(r) = (2r + 4) mod 8`.
  3. **Spider absorption** — `δ_r ; (s_{b,r} ⊗ s_{b,r}) ; μ_r ⇒ s_{b,r}`,
     all 8 × 3: a same-step fan-out-and-rejoin collapses. This is the
     schema that justifies the D6 `catgraph-syntax` dependency; the
     spiders participate as opaque generators (cg's locked B1 substrate).

  **`b3(r) ∉ {b1(r), b2(r)}` is the load-bearing choice, and it is what
  keeps the confirmatory leg honest.** Had the fusion target been one of
  the two consumed bits, every application would strictly shrink distinct
  demand and the rewriting cells would win *by construction* — the mirror
  image of the inertness trap §4 already guards against. With
  `b3 = (2r + 4) mod 8` the fused step demands a capability neither
  consumed step required, so an application may land on a `(bit, role)`
  that is scarce or absent in the drawn pool. The lever is therefore
  two-sided, which is what E-conc (§5) measures.

- **A1.2 — fuel (D4).** Confirmatory cells at **F = 256** applications;
  the registered sweep is `{32, 128, 512, 2048}` (exploratory,
  non-gating). `fuel_exhausted()` counts are a mandatory per-cell
  disclosure.

- **A1.3 — the staffing price (D5).** For the priced cells,
  `per_gen(s_{b,r}) = 1 + scarcity(b, r)` where
  `scarcity(b, r) = pool_size − |{workers of role r holding bit b}|`,
  computed once from the drawn pool **before any decision** and constant
  for the task. Spiders price at 1. Unstaffable steps therefore cost
  `1 + pool_size` — expensive but finite, so the optimizer routes away
  from them without the objective going non-finite.

- **A1.4 — λ, the valuation-only coefficient (D3b).** Pinned at
  **λ = 0.05**, with the exploratory non-gating grid
  `{0.01, 0.05, 0.25}`. Rationale, stated plainly because it is a
  judgment call rather than a derivation: magnitude margins on this
  world are O(1) while uniform cost is O(3–10), so λ ≈ 0.05 makes the
  process term a tiebreaker between otherwise close candidates rather
  than the dominant term — the regime where a valuation-only lever has
  any chance of moving a decision without simply overriding the
  magnitude signal. The prior on this cell is low regardless (§4).

- **A1.5 — draw parameters (§2).** Per-role chain length is the count of
  that role's tagged required bits (no free parameter); the spider
  fan-out probability is **0.25** per same-role adjacent pair, drawn off
  the appended stream. The degenerate world (X-reduce) sets fan-out to 0
  and consumes zero stream draws, per §2.

## Amendment 2 (pre-run, 2026-08-05 — posted to #76 BEFORE the official run)

Stage-1 implementation surfaced four items: one genuine contradiction
inside this document, one harmless generalisation, and two upstream
behaviours worth stating before the harness is written. No bar, seed,
leg, or gate changes.

- **A2.1 — §4 "3–5 oriented rules" is an ERRATUM; the schema closure
  governs.** §4 was written before Amendment 1 replaced hand-authored
  rules with three *schemas* closed over the `(bit, role)` index set.
  At the registered `bits = 8, roles = 3` that closure is **51 rule
  instances** (24 idempotence + 3 fusion + 24 spider absorption), not
  3–5. The count "3–5" refers to **schemas**, and A1.1 is the binding
  text; §4's numeral is void (the #72 A2.1 precedent, where an erroneous
  gloss was voided in favour of the named frozen anchor). Every one of
  the 51 instances constructs through `RewriteRule::new` — no schema
  instance is rejected for parallelism or mono-interface, so nothing is
  silently dropped from the theory.
- **A2.2 — the fusion modulus follows `bits`, not a literal 8**, i.e.
  `b3(r) = (2r + 4) mod bits`. Identical to A1.1 at the registered
  `bits = 8` (`r = 0 → 4`, `r = 1 → 6`, `r = 2 → 0`, pinned by a test).
  The theory builder additionally **refuses** any `(bits, roles)` where
  `2·roles > bits` or `b3 ∈ {b1, b2}` — a configuration in which the
  fusion lever would become one-sided is now unconstructible rather than
  merely unintended (A1.1's rationale, enforced).
- **A2.3 — `Free::compose` checks widths, not colors.** A same-width
  wrong-color junction (`s_{b,r0} ; s_{b,r1}`) composes cleanly and is
  rejected only at `ColoredExpr::new`. The shape draw must therefore pin
  every diagram through `ColoredExpr::new` and treat that pin as the
  color gate; a diagram that is never pinned is not role-checked at all.
- **A2.4 — trace tampering is largely unreachable from outside catgraph**
  (`RewriteStep` has no public constructor), so S-sound's negative
  direction is exercised by the reachable tampers — empty rules slice,
  mismatched `start`, reordered rules — rather than a forged step. The
  gate's positive direction (`replay` + `content_eq` on every declared
  writing, every task, every seed) is unchanged and remains mandatory.
