# A/B report: K4 EQ5b — typed two-engine contest (`GroupAgent`-over-e1 vs the typed magnitude arm)

> **VERDICT (official re-run 2026-08-08, seeds 330..360): `VALIDATED
> (two-engine)`** — both confirmatory cells clear both conjuncts; all gates
> hold. **Scoped by two pre-committed clauses that fired automatically: the
> result beats the typed control but NOT the strongest process cell
> (`wf-val-p`), and it DOES exceed `arm-E1`.** Registration:
> `docs/prereg-K4-eq5b-typed-two-engine.md` (Amendments 1–6, all pre-verdict).
> Run of record: koalisi `v0.29.0` + the EQ5b branch at `c2b1609`, catgraph
> `v0.8.0` ×3, `aif-v0.13.0`, `--release`.
>
> **Read §3 before quoting §1.** The verdict is thin (0.7 % over the bar) and
> the mechanism analysis contradicts the arm's name in two specific ways that
> the registered hypothesis did not anticipate.

## 1. Result

Two confirmatory cells against the control `wf-asis` (the EQ4-validated typed
arm), seeds 330..360. Bar: PRIMARY median ≥ **1.25×** control **and** strictly
superior on ≥ **18/30** seeds, both conjuncts in the same cell.

| cell | median PRIMARY | vs control | superior | conjunct 1 | conjunct 2 | cell |
|---|---:|---:|---:|---|---|---|
| `wf-asis` (control) | 0.1806 | 1.00× | — | — | — | — |
| `grp-role` | 0.2270 | **1.2567×** | **22/30** | **PASS** | **PASS** | **PASS** |
| `grp-mult` | 0.2266 | **1.2544×** | **22/30** | **PASS** | **PASS** | **PASS** |

**The margin is thin and is reported in full: 0.7 % and 0.4 % over the bar.**
It is not renegotiated in either direction. This is the same discipline EQ5a
applied when its valuation cells landed at exactly 1.25× against a 1.4× bar and
were reported `FALSIFIED` — a bar pinned before the run governs a narrow pass
exactly as it governs a narrow miss.

Reference and context arms, non-gating: `wf-val-p` 0.2435 · `grp-role-shared`
0.2409 · `grp-mult-shared` 0.2404 · `grp-role-det` — · `grp-role-nonov` 0.1125 ·
`grp-role-blind` 0.0268 · `arm-E1` 0.0403.

**Both pre-committed scoped clauses fired on their own**, as registered before
any number existed:

- **Did NOT exceed `wf-val-p` (0.2435)** ⇒ reported as **"beats the typed
  control, not the strongest process cell"** (§6).
- **DID exceed `arm-E1` (0.0403)** ⇒ A5.2's clause reads the other way: the
  group shape does measurable work beyond the single-agent form of the same
  engine.

## 2. Gates

- **X-battery — PASS.** Parts 1–10 diffed outside the binary against a `main`
  baseline captured before HEAD ran: **zero non-latency differences**. The
  arm-E1 seam is byte-identical on the frozen path.
- **X-identity — PASS** on all three legs (role-specialised, shared, scale
  alignment). Asked on the degenerate world, where SP2's identity configuration
  `m ≡ 1` exists; the draw consumes no extra stream, so the base prefix is
  bit-for-bit the v2w instance of the same seed.
- **S-determinism — PASS.** All five model cells 30/30 on acts, raw score bits,
  PRIMARY bits and churn, plus **seed invariance** 30/30 — the deterministic
  read consumes no randomness at all, which reproducibility alone would not
  show.
- **S-learn (i) — PASS.** 30/30 exact ledgers on every cell and 30/30 moved
  within A6.1's scope. `begin_task` rejections 0. **Exempt models disclosed:**
  `(354, r1)` and `(355, r1)`, on every role-specialised cell and neither shared
  cell.
- **S-live** — reported as a **disclosure**, not a gate (A5.5): as implemented
  it could not fail.
- **S-learn (ii) and the member-advance conjunct** — struck as vacuous by A1.4.

## 3. Mechanism — the two findings that scope the verdict

**3.1 Role specialisation is net NEGATIVE, and the reference legs say so.**
E-life: the **shared**-model cells score **higher** than the role-specialised
confirmatory cells — 0.2409 vs 0.2270 on the role-restricted channel, 0.2404 vs
0.2266 on the multiplicity channel — with the specialised cell superior on only
**15/30** and **13/30** seeds, i.e. worse than a coin flip. The two differ on
1284 and 1316 acts across 30/30 seeds, so this is a real contrast, not noise.

A1.1 relocated D2a's contrast onto exactly this axis and called it *"a group of
role specialists"* versus *"three views of one model"*. **The three views win.**
Whatever produces the 1.26×, it is not each role learning only its own bits.

**3.2 The arm's performance depends on the structural defect the review
found.** A5.1 established that under role-matched masks only the candidate's
own role internal is candidate-sensitive, and that the blind ones vote
`p(act) = 1.0` at maximum `CertaintyWeighted` weight. At full block:
**23.0 %** of decisions have **zero** candidate-sensitive internals; the mean
1.35 blind internals carry **0.626 of the CW weight**; leave-path
`cfg0 == cfg1` on **9671 of 13107 (73.8 %)** queries; act rate **80.4 %
sensitive vs 95.1 % blind**.

`grp-role-blind` removes exactly those voters — under blind masks every internal
is candidate-sensitive — and the arm **collapses to 0.0268**, an eighth of
`grp-role`'s 0.2270, differing on 1470 acts across 30/30 seeds.

So the candidate-blind maximum-weight ACT voters are **load-bearing for
performance**. Taken with 3.1, the honest mechanism reading is: *what beats the
typed magnitude control on this world is a permissively-joining group dominated
by members structurally blind to the candidate* — not role specialisation, and
not deliberation.

**3.3 The inherited v5 mechanism is load-bearing.** `grp-role-nonov`
(`query_novelty: false`, S-learn (iii)) posts **0.1125** against 0.2270,
differing on 3601 acts across 30/30 seeds — v5's X1 collapse reproduced at the
group. **Caveat, registered in A5.3 and not resolved here:** novelty-off also
collapses the candidate-blind internal's read from 1.0 to 0.5, so this cell
prices the mechanism **and** the bias of 3.2 together, and the two are **not
separated**.

**3.4 A3.2's smoke reading did not survive the block.** On 2 smoke seeds
`grp-mult` and `grp-role` differed on 0 acts / 23 score bits, and A3.2
pre-committed that if that held, `grp-mult` would carry no independent look. At
scale they differ on **181 acts** and 1526 score bits across **22/30** seeds.
**The clause does not fire**: both cells are independent looks, and the 2-cell
bar under D6 was the right correction.

**3.5 A5.8's corrected sign is confirmed.** `grp-role` 10741 acts vs `grp-mult`
10732 — **fewer**, as the corrected reading predicts: concentration is evidence
*possessed*, so higher multiplicity lowers epistemic pull and makes the arm
*less* willing to staff a repeated step. The registration's original "evidence
demand" framing was backwards and was withdrawn before the run.

## 4. Implementation / deviation ledger (mandatory)

1. **The first official run returned `RUN-INVALID`** (`7e265fe`), on S-learn.
   Amendment 6 diagnosed a **mis-specified gate**: the non-vacuity guard
   demanded movement from world models with `expected == 0`. Seeds 354 and 355,
   role 1 — measured `updates = 0, expected = 0, max |pA delta| = 0e0`.
2. **Mechanism: pool-absence.** `p8_task_feasible` requires a pool worker *of
   the tagged role*, so a role with no worker can never legally be tagged. Seed
   354 is `n = 5`, roles `[3, 0, 2]`; 355 is `n = 4`, `[2, 0, 2]`. On the block:
   2 (seed, role) pairs with no worker, **0** with zero demand despite one.
3. **The guard was unsatisfiable in general, not unlucky.** `P(role absent) =
   3(2/3)ⁿ − 3(1/3)ⁿ`, 0.553 at `n = 4`; base rate 12.5 % of seeds, ≈ 3.76
   expected failures per 30-seed block. **No block passes it** — which is why
   the guard, not the block, was corrected.
4. **A6.1 keys on `expected == 0`, not pool-absence** — one seed in 3000 (1567,
   role 0) reaches zero demand *with* the role present, via a lone single-bit
   worker.
5. **The re-run used the same block, and the numbers had been seen.** Disclosed
   without hedging. What makes it a reproduction is not good faith but that the
   guard is evaluated post-hoc from counters and never feeds a decision, so the
   amendment was **incapable** of changing an outcome — a claim registered as a
   checkable condition before the fix was written.
6. **The byte-identity condition was met one line wider than its literal
   wording.** A6.2 said "except the S-learn line and the verdict line";
   the diff is lines 23, 110, 124, 130. **Line 110 is the E-seed section's
   second printing of the same S-learn quantity.** Recorded as a deviation
   rather than argued away. All four are S-learn readouts or the verdict; **no
   measured value moved** — PRIMARY, ratios, superiority, churn, act counts,
   score bits and every disclosure are bit-identical across the two runs.
7. **S-learn has now been wrong twice, both corrections pre-verdict.**
   Registered as "did the state move?", which passes a double-updating MMP arm
   (caught by the review); corrected to a counted ledger with `any → all`,
   which made it unsatisfiable (caught by the run). The counted half — the only
   half that detects the failure mode the gate exists for — was **30/30 exact
   in both runs**.
8. **Five pre-run amendments preceded the run, three correcting the
   registration's own reasoning.** A1: the arm as registered was unbuildable.
   A3: D2's CW rationale measured false. A4: A3's own prediction measured false.
   A reader who wants to discount a registration corrected six times has the
   full trail; every amendment predates the code it governs, and A3/A4/A6 state
   the data that drove them.
9. **A1.3's rationale was withdrawn as measurably false and inverted in sign**
   (A5.1): an indifferent member reads ≈ 1.0 at weight 1.0, not `[0.5, 0.5]` at
   0.5. Its decision — drop empty-demand roles — stands.
10. **The arm was NOT re-specified after A5.1.** Restricting the roster to
    candidate-sensitive internals forces R = 1, so under role-matched masks a
    group deliberating about one candidate is structurally impossible. That is
    the finding, disclosed and measured, not a defect repaired mid-flight.
11. **X-identity is asked on the degenerate world**, since SP2's identity
    configuration `m ≡ 1` does not exist on v2w. Stated in the printed line.
12. **Latency is record-only and never gating.** A3.3's "~37 µs/decision" did
    not reproduce and was **withdrawn**; the run reports measured figures.
13. **`grp-role-det` and `grp-seed` are read-probes** — reported, never gated,
    under A4.2's registered criterion (a cell that varies the READ is reported;
    a cell that varies the MODEL is gated).

## 5. What this does and does not settle

- **Settled:** on the v2w world, a role-slotted `GroupAgent` over arm-E1's
  persistent world model beats the EQ4-validated typed magnitude arm at the
  registered bar — by 0.7 %, on 22/30 seeds, with every gate holding.
- **Settled, and cutting against the arm's name:** the win is **not** from role
  specialisation (shared models score higher, §3.1) and **not** from
  deliberation about the candidate (§3.2 — the blind voters are load-bearing).
  A `VALIDATED (two-engine)` here means *"on this world, a role-restricted
  active-inference group outperforms the typed magnitude arm"* — **not** that
  aif beats magnitude generally, and **not** anything about **koa#54 (mag =
  demonstrated default, FINAL)**, which no EQ5b outcome reopens.
- **Settled:** the arm does **not** reach `wf-val-p` (0.2435), EQ5a's strongest
  process cell — and #80 already measured that lever as a coverage proxy rather
  than process-specific.
- **Untouched, as pre-committed:** the v1/v2 K4 verdicts, EQ3's, EQ4's, EQ5a's
  and #80's.
- **Open:** whether any advantage transfers off v2w — one world, one draw
  family, 30 seeds; whether a group whose members are all candidate-sensitive
  can be built at all under a role-matched coverage semantics; how much of §3.3's
  ablation is the v5 mechanism and how much is the candidate-blind bias; and
  whether the deterministic read is the right group decision rule, which the
  non-gating `grp-role-det` leg measures but does not settle.

This document is immutable. Follow-ups land as an appended addendum.
