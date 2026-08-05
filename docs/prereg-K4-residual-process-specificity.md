# Pre-registration: K4 — is the unstaffable-residual lever process-specific, or a coverage proxy?

**Status: REGISTERED.** Committed BEFORE implementation. Design-lock of
record: [koalisi #80](https://github.com/sustia-llc/koalisi/issues/80)
(owner design-lock comment, 2026-08-05, D1–D8 + the formalism pin).
Pre-run amendments are legitimate per the standing protocol (committed
to this doc + posted to #80 BEFORE any affected code); after the
official run this document is immutable.

A **koalisi-side K4-lineage registration** (D1) — no E-queue number, no
upstream feature, no cross-repo seam (#56 / #63 precedent).

## 1. Registration statement

- **What EQ5a left open.** EQ5a
  (`docs/ab-report-K4-eq5a-process-structured.md`) measured the
  unstaffable-residual valuation lever at **median PRIMARY 0.2484 vs
  control 0.1989 (1.25×), strictly superior on 30/30 seeds** — the
  strongest paired consistency in this lineage since v5 — but under a
  family-wise bar raised to 1.4× for four looks, so it carried **no
  verdict**. EQ5a measured the margin and cannot explain it.
- **Hypothesis (H-PS, confirmatory):** the lever's advantage is
  **process-specific** — materially larger on the workflow world than on
  a structurally flat world carrying the same coverage demand.
- **The null is (B), and it is the favourite.** (B) = the residual is a
  monotone penalty on uncovered demand, i.e. a **coverage proxy** in
  process clothing, which would work identically on flat tasks.
- **Born on:** koalisi `v0.27.0` (catgraph `v0.8.0` ×3, `aif-v0.12.0`,
  MSRV 1.93). No re-pin implied — the lever needs no upstream surface.
- **Seeds:** `300..330`, fresh (90..120 + 150..180 remain reserved; no
  release implied).
- **Battery placement:** `examples/strategy_comparison.rs` **Part 10**;
  frozen Parts 1–9 are the byte-identity regression gate (§6 X-battery).
- **Falsifiability posture.** Two independent EQ5a measurements already
  point at (B): **λ across a 25× range** (0.01 / 0.05 / 0.25) **and both
  cost models all produce the identical median 0.2484** while act counts
  differ (2726 / 2638 / 2684). If the weight of a scarcity-weighted term
  does not move the outcome, the weighting is probably not the
  mechanism. This registration is designed so the unglamorous answer can
  win.

## 2. The sharpened question (a pre-implementation refinement of the lock)

The #80 lock framed the contrast as "process structure vs flat". Reading
the EQ5a implementation makes it sharper, and the sharper form is what
this document registers:

**The v2w shape draw repeats and fans steps but never introduces a new
`(bit, role)`** — this is why EQ5a's as-written feasibility was exactly
the v2t feasibility its prefix already guaranteed (measured: 0
infeasible, structurally). Therefore **v2w and v2t carry IDENTICAL
distinct demand per task**. The two worlds pose the same coverage
problem; they differ only in **occurrence multiplicity** (fan-out
repeats a step) and hence in the per-occurrence priced weights.

So (A) does not mean "workflows help" in some diffuse sense. It reduces
to a precise claim:

> **Occurrence multiplicity and/or scarcity weighting carry
> decision-relevant signal beyond the distinct uncovered set.**

And (B) is its exact negation: the lever reads only *how much distinct
demand is uncovered*, which magnitude's own coverage term already
tracks. Stating it this way is what makes the arms below a genuine
decomposition rather than a vibe.

## 3. Worlds and arms

Both worlds are drawn from the **same SplitMix64 stream prefix per
seed** (the #46/#48/#72 shared-prefix discipline), so every contrast
below is **paired within a seed**.

| world | draw | distinct demand |
|---|---|---|
| **v2w** | the EQ5a workflow world (steps, chains, fan-out p = 0.25) | the v2t tagged required set |
| **v2t** | the EQ4 flat world (multiplicity ≡ 1, no fan-out, no spiders) | *identical, same seed* |

| arm | world | residual prices | isolates |
|---|---|---|---|
| `ctl-wf` | v2w | — (typed magnitude, oracle ρ = δ) | control |
| `ctl-flat` | v2t | — (same policy) | control |
| `res-wf` | v2w | **occurrences** | the full lever |
| `res-flat` | v2t | occurrences (≡ distinct there) | **(B): does the lever need the process at all?** |
| `res-distinct` | v2w | the **distinct** uncovered set | multiplicity specifically |

`res-distinct` should land near `res-flat` if the sharpened reading in
§2 is right; that near-coincidence is a registered **internal
consistency check**, reported and explained either way.

The lever is unchanged from EQ5a Amendment A3.1:

```
value(S) = Mag(S) − λ · Σ per_gen(g)   over demand elements g the coalition
                                        S cannot cover
```

λ = **0.05**, the EQ5a pin. Cost model: **uniform** for the confirmatory
cells (metric-blind, EQ5a A1.4's reasoning), staffing-priced as a
registered exploratory cell.

## 4. Registered legs

- **S-repl (registered sanity leg, must hold).** `r_wf ≥ 1.25`, where
  `r_w` = median PRIMARY(`res-w`) / median PRIMARY(`ctl-w`) on world
  `w` — i.e. the lever must first **replicate EQ5a's margin
  out-of-sample** on seeds 300..330. If S-repl fails, the discrimination
  is moot: the run reports `NOT REPLICATED` and produces no
  process-specificity verdict. (This is where #80's D2 replication
  option lands — as a precondition, not a parallel leg.)
- **H-PS (confirmatory), both conjuncts required.** With
  `lift_w = r_w − 1`:
  1. `lift_wf ≥ 1.25 × max(lift_flat, 0)`.
     **The floor is load-bearing** (#80 formalism pin): a raw
     `lift_wf / lift_flat` blows up as `lift_flat → 0` and **flips sign**
     when the lever hurts on flat, which would let a mediocre `res-wf`
     pass on a negative denominator. Flooring at 0 also means that if the
     lever does nothing on flat, any positive workflow lift satisfies
     conjunct 1 — which is *correct* (no flat lift ⇒ process-specific)
     and is exactly why S-repl carries the "and the effect is real" half.
  2. Paired per-seed: `margin_wf > margin_flat` on ≥ **18/30** seeds,
     where `margin_w` = per-seed PRIMARY(`res-w`) − PRIMARY(`ctl-w`) —
     a **difference, not a ratio**, because differences are well-behaved
     at zero.
- **E-mult (registered exploratory, non-gating).** `res-distinct` vs
  `res-wf` on v2w: the multiplicity-only contrast, and the internal
  consistency check against `res-flat` per §3.
- **E-price (registered exploratory, non-gating).** The staffing-priced
  cost model on both worlds. EQ5a measured uniform and priced as
  median-identical; whether that survives the flat world is the
  question.
- **E-λ (registered exploratory, non-gating).** λ ∈ {0.01, 0.05, 0.25}
  on both worlds. EQ5a found the median flat across this range on v2w;
  if it is flat on v2t too, that is further (B) evidence.
- **Context, non-gating:** churn (D4 — EQ5a measured 10.5 vs 6.5, a 1.6×
  cost), latency (D5 — report-only; the ~2× is the A5.3 evaluation
  probe, a harness safety device, not intrinsic to the lever), and the
  untyped `mag` / `scalar` / `arm-E1` baselines.

## 5. Gates (any failure ⇒ RUN-INVALID)

- **X-battery** — frozen Parts 1–9 byte-identical on every
  quality/ratio/superiority/churn/verdict line vs a fresh pre-change
  baseline; latency-only diffs permitted (standing exclusion).
- **X-identity (λ = 0)** — at λ = 0 each `res-*` arm reproduces its own
  control **bit-identically on acts and score bits**, all 30 seeds. This
  is the causality pin for the entire lever, promoted from EQ5a's test
  (`part9_valuation_is_live_a3_1`) to a gate here.
- **X-pair** — the two worlds' instances share the v2t prefix
  bit-for-bit per seed, asserted; without it the pairing in H-PS
  conjunct 2 is meaningless.
- **S-live** — each `res-*` arm at λ = 0.05 diverges from its control on
  ≥ 1 seed (the regression the original EQ5a formulation passed
  silently by being algebraically inert). A cell that cannot move is
  RUN-INVALID, not a null result.

## 6. Verdict labels (pre-committed)

- `VALIDATED (process-specific)` — S-repl holds, H-PS passes both
  conjuncts, all §5 gates hold.
- `FALSIFIED (coverage proxy)` — S-repl holds, gates hold, H-PS fails
  either conjunct. **This is the (B) outcome and it is a real finding**:
  it would say EQ5a's valuation result was a coverage penalty, not a
  process signal, and the report must say so plainly rather than
  presenting it as a lever failure.
- `NOT REPLICATED` — S-repl fails; no process-specificity verdict is
  produced.
- `RUN-INVALID` — any §5 gate fails.

Pre-committed interpretations: the v1/v2 K4 verdicts, EQ3's, EQ4's and
EQ5a's verdicts, and the **koa#54 arm question (mag = demonstrated
default, FINAL)** are UNTOUCHED regardless of outcome. **Shipping the
policy to the library (D6) is not adopting it** — adoption would be a
separate decision this registration does not license in either
direction. No post-hoc bar movement. Latency is never gating.

## 7. Placement

Library, behind the existing **`process`** feature (D6): the residual
policy becomes a `CoalitionDecisionPolicy` wrapper a runtime could use,
rather than the example-side `P9ValuationPolicy` EQ5a left behind. The
harness consumes the library type; battery scaffolding (world draws,
scorers, tables) stays example-side.

## 8. Report

`docs/ab-report-K4-residual-process-specificity.md` — registered
sections mirror §4/§5; implementation/deviation ledger mandatory;
appended-addendum convention applies to any follow-up.
