# A/B report: K4 — the unstaffable-residual lever is a coverage proxy, not a process signal

> **VERDICT (official run 2026-08-05, seeds 300..330):
> `FALSIFIED (coverage proxy)`** — S-repl holds, all four gates hold,
> H-PS fails both conjuncts. Registration:
> `docs/prereg-K4-residual-process-specificity.md` (Amendment 1,
> pre-run). Run of record: koalisi `v0.27.0` + the #80 branch at
> `5b238b5`, catgraph `v0.8.0` ×3, `aif-v0.12.0`, `--release`.

## 1. Result

The lever **replicated, and then failed to be process-specific.**

| leg | measured | bar | outcome |
|---|---|---|---|
| **S-repl** (sanity) | `r_wf` **1.3355×** | ≥ 1.25 | **PASS** |
| **H-PS conjunct 1** | `lift_wf` **+0.3355** | ≥ 1.25 × `lift_flat` = **0.4194** | **FAIL** (contested) |
| **H-PS conjunct 2** | `margin_wf > margin_flat` on **0/30** | ≥ 18/30 | **FAIL** |

| arm | world | median PRIMARY | vs control | superior | churn |
|---|---|---:|---:|---:|---:|
| `ctl-wf` / `ctl-flat` | v2w / v2t | 0.1817 | 1.00× | — | 7.00 |
| `res-wf` | v2w | 0.2426 | **1.34×** | 30/30 | 12.00 |
| `res-flat` | v2t | 0.2426 | **1.34×** | 30/30 | 12.00 |
| `res-distinct` | v2w | 0.2426 | **1.34×** | 30/30 | 12.00 |

**`lift_wf` and `lift_flat` are equal to four decimals — +0.3355 both.**
The lever is real, it replicates out-of-sample *more strongly* than
EQ5a measured (1.34× here vs 1.25× there), and **the process structure
contributes nothing to it.**

## 2. Gates (prereg §5, all PASS)

- **X-pair** — the two worlds share the v2t prefix **bit-for-bit on
  30/30 seeds** (compared field by field, not by a derive that could
  silently stop covering a new field) and carry **identical distinct
  `(bit, role)` demand on 600/600 tasks**. Both halves are load-bearing:
  without the first, conjunct 2's pairing is meaningless; without the
  second, `res-flat` is not a control for `res-wf` at all.
- **X-identity (λ = 0)** — each `res-*` arm reproduces its own control
  bit-identically on acts, raw score bits, per-seed PRIMARY bits and
  churn, all 30 seeds. This is what makes every divergence below
  attributable to the residual rather than the wrapper.
- **S-live** — each arm diverges from its control on 30/30 seeds (2739
  acts, 5425 score bits). A cell that cannot move is RUN-INVALID, not a
  null.
- **S-probe** (Amendment A1.1) — 0 / 0 / 0 declines across the three
  registered cells, and 0 in every λ = 0, E-price and E-λ cell.
- **X-battery** — Parts 1–9 checked outside the binary against the EQ5a
  run of record: **latency-only diffs**, every quality / ratio /
  superiority / churn / verdict value byte-identical.

## 3. Mechanism — why (B) wins, precisely

**3.1 The worlds differ, and the difference reaches the score without
reaching a decision.** X-pair quantifies the one axis they differ on:
**178 of 600 tasks (29.7 %) repeat at least one step**, and the corpus
carries **2265 step occurrences on v2w against 2045 on v2t** — 1.11×
multiplicity. That excess *is* the process signal on offer. Measured
directly, `res-wf` vs `res-flat` differ on:

- **355 raw score bits**, and
- **0 decisions by act**, on **0/30** seeds.

So the multiplicity weighting **genuinely reaches the margin and
genuinely never crosses a threshold**. This is a mechanism statement,
not a null: the term is live (S-live proves it), it is simply too small
at these λ to change what anyone decides — so it cannot produce a
per-seed PRIMARY difference for conjunct 2 to detect.

**3.2 The weighting is not inert here — and that makes the result
stronger, not weaker.** Unlike EQ5a (which found the v2w median flat
across λ and both cost models on seeds 240..270), on this block the
weighting *does* move the outcome:

| cell | `r_wf` | `r_flat` |
|---|---:|---:|
| uniform, λ = 0.05 | 1.3355× | 1.3355× |
| staffing-priced, λ = 0.05 | **1.3869×** | **1.3869×** |
| uniform, λ = 0.01 | 1.3355× | 1.3355× |
| uniform, λ = 0.25 | **1.3682×** | **1.3682×** |

Pricing changes the result; λ = 0.25 changes the result. **Every single
cell has `r_wf` exactly equal to `r_flat`.** The finding is therefore
not the weak "the weighting is inert" that EQ5a's evidence suggested,
but the strong form: **whatever the weighting does, it does identically
whether or not the process structure is present.**

**3.3 The internal consistency check landed exact, and structurally.**
The registration predicted `res-distinct ≈ res-flat`; measured, they are
**bit-identical**. By X-pair the two worlds carry the same distinct
demand over the same pool, so `res-distinct`'s residual *is*
`res-flat`'s, element for element. Likewise `ctl-wf` ≡ `ctl-flat`
bit-identically — the typed policy sees only the OR-mask of distinct
demand and the scorer counts only distinct coverage, so multiplicity is
invisible to both. The controls contribute no world difference of their
own, which is exactly what makes every `res-wf` − `res-flat` gap
attributable to the residual weighting alone.

## 4. What this settles

**EQ5a's valuation result was a coverage penalty, not a process
signal.** The lever's 1.25×/30-of-30 there — and its 1.34×/30-of-30
here — comes from penalising *how much distinct demand is uncovered*,
which is information magnitude's own coverage term already tracks, not
from anything about the compositional structure of the process. The
`(A)` reading recorded as EQ5a's most promising open lead is dead, and
this document is where it dies.

Stated plainly, as prereg §6 requires: **this is a real finding, not a
lever failure.** The lever works. It is simply not what we hoped it
was, and the honest version of "the first mechanism in this lineage to
put occurrence multiplicity and step scarcity into a decision" is that
it puts them into the *score* and they never reach a *decision*.

**Untouched, as pre-committed (§6):** the v1/v2 K4 verdicts, EQ3's,
EQ4's and EQ5a's verdicts, and the koa#54 arm question (mag =
demonstrated default, FINAL). **Shipping `ResidualPolicy` to the library
is not adopting it** — and this result gives no reason to.

## 5. Implementation / deviation ledger (mandatory)

1. **The lever was promoted to the library** (`src/process/residual.rs`)
   out of EQ5a's example-side `P9ValuationPolicy`, per §7 / lock D6. The
   promotion is behaviour-preserving **by construction**: upstream
   defines `coalition_value` as
   `coalition_magnitude_from_couplings(…, 1.0)`, so the library's
   `magnitude_or_zero` and the example's former `magnitude_at_t(.., 1.0)`
   are the same function. Part 9's call sites reroute through the
   library type; no second copy remains example-side, and X-battery
   confirms Part 9's numbers did not move.
2. **The flat world** is Part 8's `draw_typed_instance` verbatim dressed
   with EQ5a's degenerate all-parallel shape (zero stream draws), scored
   through the Part 9 runner — EQ5a's X-reduce already proved that route
   reproduces the EQ4 typed arm exactly. X-pair asserts agreement with
   both constructions.
3. **Probe declines became a gate** (Amendment A1.1). The probe is
   **untyped** while every Part 10 policy is **typed**, so a typed-only
   error would fold a correction onto a genuine decline; a
   reported-only count would corrupt PRIMARY and acts without tripping
   any other gate. Measured 0 everywhere. Closing the underlying gap
   needs a typed-evaluation probe that does not exist upstream — a
   catgraph-side surface, deliberately not improvised here. **Scoping:**
   λ = 0 cells are covered transitively (a decline there would already
   fail X-identity); the exploratory E-price / E-λ cells report but are
   not gated, since gating them would promote a leg the registration
   fixed as non-gating.
4. **An undefined `r_flat` is a distinct condition** from a measured
   `lift_flat ≤ 0` (A1.2), and a **trivial conjunct-1 pass announces
   itself** (A1.3). Neither branch fired: this run is **contested**,
   `lift_flat` measured and positive, so `bar_c1 = 0.4194` is a real bar
   and both conjuncts did discriminating work. Both branches are covered
   by unit tests instead.
5. **A registered-exploratory addition:** the `res-wf` vs `res-flat`
   act/score-bit divergence line. The confirmatory legs see only
   PRIMARY, where "the term moved the margin but flipped nothing" is
   invisible — without it the run reads as a flat null rather than the
   mechanism statement in §3.1. Reporting only; no bar, cell, or
   computed leg touched.
6. **X-battery classification note:** two differing lines are wall-clock
   **seconds** (the E-fuel `declare_secs` column and the A3.2
   search-cost prose), not the `µs/decision` column. Adjudicated as
   timing measurements and therefore permitted under the standing
   exclusion — named here rather than folded silently into
   "latency-only".
7. **3-lens pre-run review:** 0 blocking, 0 important, 3 minor, all
   applied as Amendment 1. The lenses also verified as sound the three
   load-bearing design bets: that the contrast can discriminate at all,
   that the floored conjunct 1 is implemented literally, and that exact
   ties are excluded from "superior" rather than silently counted.
8. **Latency** (never gating): `res-*` ≈ 24.4 µs/decision vs controls
   ≈ 11.7. The ~2× is the A5.3 evaluation probe — a harness safety
   device, not intrinsic to the lever.

## 6. What remains open

- **Would the multiplicity term ever cross a threshold?** At these λ it
  reaches the score and stops. A design that made it decision-relevant
  would need either a much larger λ (which E-λ shows begins to move the
  ratio — equally on both worlds, so it buys no process-specificity) or
  a world where multiplicity is far more skewed than 1.11×. Neither is
  implied.
- **The typed-evaluation probe gap** is upstream work if anyone wants
  the probe to match the arm it guards.
- **Nothing here reopens the arm question**, and nothing here recommends
  adopting `ResidualPolicy`.

This document is immutable. Follow-ups land as an appended addendum.
