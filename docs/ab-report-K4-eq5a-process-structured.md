# A/B report: K4 EQ5a — process-structured tasks (workflows as string diagrams)

> **VERDICT (official run 2026-08-05, seeds 270..300): `FALSIFIED (process
> structure)`** — no H-P cell clears the family-wise bar; gates X-reduce,
> S-sound and S-dedup all PASS. Registration:
> `docs/prereg-K4-eq5a-process-structured.md` (Amendments 1–5, all
> pre-run). Run of record: koalisi `v0.26.0` + the EQ5a branch at
> `7b6c5d6`, catgraph `v0.8.0` ×3, `aif-v0.12.0`, `--release`.

## 1. Result

Four confirmatory cells against the control `wf-asis` (the EQ4-validated
typed arm staffing the workflow as written), seeds 270..300. Bar:
PRIMARY median ≥ **1.4×** control **and** strictly superior on ≥ **21/30**
seeds, both conjuncts in the same cell.

| cell | median PRIMARY | vs control | superior | conjunct 1 | conjunct 2 | cell |
|---|---:|---:|---:|---|---|---|
| `wf-asis` (control) | 0.1989 | 1.00× | — | — | — | — |
| `wf-rw-u` | 0.2104 | 1.06× | 17/30 | FAIL | FAIL | **FAIL** |
| `wf-rw-p` | 0.2112 | 1.06× | 19/30 | FAIL | FAIL | **FAIL** |
| `wf-val-u` | 0.2484 | 1.25× | **30/30** | FAIL | **PASS** | **FAIL** |
| `wf-val-p` | 0.2484 | 1.25× | **30/30** | FAIL | **PASS** | **FAIL** |

Bar for conjunct 1 was `1.4 × 0.1989 = 0.2785`. Context arms, non-gating:
`mag` 0.0461 (0.23×), `scalar` 0.0197, `arm-E1` 0.0558.

**The bar did not move.** It was raised from the lineage's standing
1.25× / 60 % to 1.4× / 70 % *before implementation*, to pay for four
looks (design-lock, #76). Both valuation cells land at exactly **1.25×
with a maximal 30/30 second conjunct** — they would clear the standing
bar and do not clear the registered one. That is a `FALSIFIED`, reported
with the margin in full rather than renegotiated. Post-hoc bar movement
is the one thing this lineage has never done, and Amendment 4 §A4.3
pre-committed to exactly this case before the run.

## 2. Gates (prereg §6)

- **S-sound — PASS.** 6600 declared writings verified across 11
  declaring cells (30 seeds × 20 tasks each), **0 unsound**. Every
  writing `replay`s under the registered 174 rules and the replayed
  content `content_eq`s the reported representative.
- **S-dedup — PASS.** `canonical_key(a) == canonical_key(b) ⟺
  content_eq(a, b)` on all **179 700** unordered pairs of the 600-workflow
  corpus, **0 disagreements**. Like-with-like holds by construction; the
  mono-vs-colored seam was **measured** (0 of 600 divergent), not assumed.
- **X-reduce — PASS.** On the degenerate world (zero stream draws, so
  bit-for-bit the v2t instances), `wf-asis` reproduces the EQ4 typed
  arm's acts + per-seed PRIMARY + churn on all 30 seeds, and both
  rewriting cells with an empty rule set reproduce `wf-asis`
  bit-identically.
- **X-battery — PASS**, checked outside the binary: Parts 1–8 diffed
  against a pre-change `v0.26.0` baseline with the trailing latency
  column normalised — every quality, ratio, superiority, churn and
  verdict value byte-identical; latency lines are the sole diff, the
  standing exclusion.
- **Determinism — PASS** (not a registered gate, run anyway): the
  official run on the committed tree reproduces the pre-commit dry run
  with **zero non-latency differences**.

## 3. Mechanism — where the signal actually was

Three findings, and the first two invert the registration's emphasis.

**3.1 The winning signal is valuation, not rewriting.** The confirmatory
hypothesis was a2 — *rewriting* as process optimization. The rewriting
cells reached 1.06×. The **valuation-only** cells, which change no demand
whatsoever (`demand moved` 0/600, `steps` 0.0, `best_cost = initial_cost`),
reached **1.25× on 30/30 seeds** — the strongest paired consistency in
this lineage since v5. The mechanism that converted was pricing the
**unstaffable residual**: `λ · Σ per_gen` over occurrences the coalition
cannot cover. That term is the first thing in the K4 lineage to put
**occurrence multiplicity and step scarcity** into a decision — magnitude
sees only the OR-mask of distinct demand and is structurally blind to
both.

This is a genuine EQ5 result: process *structure* carried signal, but
through how a coalition is **priced against** the process, not through
**rewriting** the process.

**3.2 The rewriting arm converted 100 % of its achievable margin — the
ceiling is what is low.** E-ceil (i), a large-fuel scarcity-priced
reference arm, posts **0.2112 — identical to `wf-rw-p`**, giving a
conversion fraction of **100.0 %**. The optimizer is not
underperforming: within the rewriting family, on this world, there is
almost nothing more to convert. The fuel sweep says the same thing from
another side — `{32, 128, 512, 2048}` produces **identical medians and
identical superiority counts at every point** (median `states_explored`
2.0, median steps 1.0). More search buys nothing.

**3.3 The two-sided lever bit, as designed.** E-conc: the declared demand
was pool-**infeasible** on **100/600 (16.7 %)** of tasks for `wf-rw-u`
and **96/600 (16.0 %)** for `wf-rw-p` — the registered failure mode,
counted and never re-drawn. Those tasks carry `Δ_infeasible` of **−0.0112
/ −0.0110** against `Δ_feasible` of **+0.0446 / +0.0457**. Rewriting won
where it helped and lost where it concentrated demand onto a
`(bit, role)` nobody held, which is exactly the two-sidedness the fusion
target was chosen to preserve. Success rate fell accordingly: control
0.8050 vs `wf-rw-u` 0.7150.

**3.4 Dedup bought nothing at these draws.** E-dedup: of 600 as-written
workflows, 492 distinct as written and **492 distinct as content** —
**0 arrivals** distinct as written but equal as content. The a1 half of
the EQ5 hypothesis has no purchase on this world's draw. Inside
`optimize`, `canonical_key` dedup is doing real work (median
`states_explored` 2.0), but at the corpus level a content-keyed table
buys nothing a writing-keyed one would not.

## 4. Implementation / deviation ledger (mandatory)

1. **§4's "3–5 oriented rules" is void** (A2.1). Amendment 1 replaced
   rules with schemas; the closure is 174 instances at the registered
   `bits = 8, roles = 3` (24 idempotence + 126 fusion + 24 absorption).
   All 174 construct — nothing silently dropped.
2. **The fusion modulus follows `bits`** (A2.2), identical to A1.1's
   literal `mod 8` at the registered width; configurations where the
   lever would go one-sided are unconstructible.
3. **The valuation formula was re-read** (A3.1). As originally
   registered it was **algebraically inert** — `cost_of(writing)` does
   not depend on the coalition, so the term cancelled from every margin;
   measured bit-identical to the control at all three λ. Re-read as the
   unstaffable residual. Liveness is now **measured, not assumed**:
   30/30 seeds diverge (2724 / 2638 acts), and at λ = 0 the cell
   reproduces the control exactly — causality pinned in both directions.
4. **The fusion schema was widened** (A3.2) after the narrow schema
   measured **43/600 (7.2 %)** eligibility, making a stream-level bar
   unreachable on merit. Widened reach, measured as `demand_moved`:
   **337/600 (56.2 %)**.
5. **The bit-4 void** (A5.2): `b'' = (b + b' + 4) mod 8` collapses onto a
   consumed bit whenever either operand is 4, so **bit 4 appears in no
   fusion instance at all** and can never be consumed. Wherever it sits
   between two otherwise-fusable same-role steps it permanently blocks
   them from becoming adjacent. The eligibility figure (358/600, 59.7 %)
   is therefore a **loose upper bound**, and the leg's power claim is
   anchored to `demand_moved` instead. **The schema was deliberately not
   re-tuned** — re-targeting an instrument after outcomes are visible is
   worse than disclosing a bound.
6. **The fairness clause overclaimed BGKSZ Thm 5.6** (A5.1). The theorem
   certifies theory-relative derivability only; the rules are
   hand-authored for this experiment. Corrected everywhere it appeared,
   and the scoping sentence now sits in the verdict block. Binding
   reading: a `VALIDATED` here would have meant "*a sound-by-stipulation
   process transformation can unlock staffing value in this world*", not
   "process reorganisation is generically valuable".
7. **S-sound's scope was widened** (A5.3) from 2 declaring cells (1200
   writings) to 11 (6600) — §6 always said "all tasks, all seeds", and
   the E-fuel and E-ceil declares were computing an `unsound` count that
   was discarded.
8. **The decline-vs-zero-margin ambiguity was removed, not bounded**
   (A5.3). The library reports an upstream decline as
   `Decision { act: false, score: 0.0 }` — identical to a legitimate
   exact-zero margin, a population EQ3 measured at ~43 % of the stream.
   The harness now detects evaluation failure **independently** via its
   own `relevant_masks` + `magnitude_at_t`. Probe declines: **0 / 0**.
   *Disclosed residual:* the probe's couplings are untyped where the
   arm's are ρ-modulated, so a failure only the ρ-modulated matrix could
   trigger would slip past — no typed-evaluation probe exists upstream.
9. **E-conc separates causes** (A5.3): pool-infeasible vs
   declined-and-counted. Declines were **0** on this world, so the
   pre-fix rate was not in fact inflated — but the number now measures
   what E-conc claims it measures.
10. **§2's re-draw prose is broader than the code** (A5.4): the loop
    re-draws `required` + `tags`; the shape is drawn once, after
    feasibility is confirmed. Functionally equivalent — there is never a
    stale shape to discard.
11. **E-ceil leg (ii) is not literally "brute-force"** (A5.4):
    `optimize` exposes only `best()`, its visited set is private, so the
    class cannot be enumerated. Implemented as a pinned multi-objective
    sweep at fuel 2048 (uniform, priced, and one "price this step at
    1000" objective per distinct as-written step), labelled an upper
    bound at every point of use.
12. **Fan-out shape is a draw choice the registration did not spell
    out.** A firing pair writes `δ_r ; (s ⊗ s) ; μ_r` — exactly the
    absorption schema's LHS. A generic two-different-successors split
    would leave 24 of 174 instances dead on drawn traffic and make the
    `catgraph-syntax` dependency decorative.
13. **Dry runs executed on the registered seed block** (A4.1), so the
    official run is a reproduction, not a first look. **Amendment 3 was
    written after a dry run had shown a verdict line** (A4.2); both its
    changes were driven by mechanism facts — an algebraic cancellation
    and a 7.2 % reach — and no bar was touched. A reader who disagrees
    with that characterisation has what they need to discount the
    confirmatory leg.
14. **Amendment 5 changed no decision.** Every cell reproduces its
    pre-A5 value exactly — as a measurement-and-claims amendment should.
15. **Latency, never gating** (§7): valuation cells 23.1 µs/decision
    (roughly double, from the two extra probe evaluations per decision),
    rewriting cells 7.1–7.2 µs, control 11.1 µs.

## 5. What this does and does not settle

- **Settled:** on the v2w world, bounded convex-DPO rewriting under a
  stipulated theory does not convert into coalition-value advantage at
  the registered bar. Not for want of search — the fuel sweep is flat and
  the reference arm's whole margin was already captured.
- **Settled:** the process signal that *does* convert is valuation
  against the unstaffable residual — 1.25×, 30/30 — which is below the
  registered bar and therefore carries **no verdict**. It is the natural
  candidate for a fresh registration, and would need one: nothing here
  licenses adopting it.
- **Untouched, as pre-committed (§7):** the v1/v2 K4 verdicts, EQ3's and
  EQ4's verdicts, and the #54 arm question (mag = demonstrated default,
  FINAL). This result speaks to the process-vs-as-written contrast within
  the typed magnitude family, not to mag-vs-aif — that contest is EQ5b's
  ([#78](https://github.com/sustia-llc/koalisi/issues/78)).
- **Open:** whether the valuation-residual mechanism survives a bar set
  for it rather than inherited from a four-cell family; whether a rule
  theory with external semantic content (rather than stipulated
  equivalences) would move the rewriting half; and whether the bit-4
  class of dead targets is an artifact of the modular formula or of
  fusion-style schemas generally.

This document is immutable. Follow-ups land as an appended addendum.
