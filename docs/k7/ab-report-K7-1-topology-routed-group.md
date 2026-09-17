# A/B report: K7-1 — topology-routed group voting (tira ext-6) vs the candidate-blind group

> **VERDICT (official run 2026-09-17, seeds 90..120): `VALIDATED
> (topology-routed group)`** — H-T clears both conjuncts (2.4144×, 30/30) and
> every gate holds. **Three pre-committed readings fired with it and scope it:**
> the routed arm's PRIMARY is that of a learning-free redundancy prune (A2.2);
> on centre-present reads the candidate's own role query alone decides (clause
> 6); the control loses on the success term, by evicting members its own
> candidate-sensitive internal says are needed (§3). Registration:
> [`prereg-K7-1-topology-routed-group.md`](prereg-K7-1-topology-routed-group.md)
> (Amendments 1–3, all pre-run). Run of record: koalisi `1688b75`
> (`v0.38.0`), `aif-v0.14.0`, catgraph `v0.23.0` ×3, `--release`; raw output
> [`../runs/K7-1.log`](../runs/K7-1.log).
>
> **Read §3 and §4 item 1 before quoting §1.** The effect was seen off-block
> before the run, and the arm is not a deliberating group.

## 1. Result

`grp-topo` (candidate-star routing, λ = ½) against in-battery `grp-role`. Bar
(H-T): PRIMARY median ≥ 1.25× the control's **and** strictly superior on
≥ 18/30 seeds.

| cell | role | median PRIMARY | vs `grp-role` | superior | median churn |
|---|---|---:|---:|---:|---:|
| `grp-role` | control | 0.1764 | 1.0000× | — | 195.00 |
| `grp-role-blind` | second control | 0.0123 | 0.0698× | 0/30 | 194.00 |
| **`grp-topo`** | **confirmatory** | **0.4258** | **2.4144×** | **30/30** | 156.00 |
| `grp-topo-id` | X-identity cell | 0.1764 | 1.0000× | 0/30 | 195.00 |
| `grp-topo-q` (λ = ¼) | exploratory | 0.4258 | 2.4144× | 30/30 | 156.00 |
| `grp-topo-solo` (λ = 1) | exploratory | 0.4258 | 2.4144× | 30/30 | 156.00 |
| `ref-prune` | reference (A2.2) | 0.4258 | 2.4144× | 30/30 | 167.00 |
| `wf-asis` | context | 0.1522 | 0.8628× | 13/30 | 7.00 |

- H-T conjunct 1: 0.4258 / 0.1764 = **2.4144× — PASS**. Conjunct 2:
  **30/30 — PASS**.
- The prereg's registered expectation (§4: *"does **not** clear 1.25×"*) was
  wrong.

Scoped clauses, as the binary printed them:

1. `grp-topo`'s median 0.4258 exceeds `grp-role-blind`'s 0.0123.
   *`grp-role-blind` differs from `grp-role` in the masks, not the voters; its
   distance from `grp-topo` does not decompose candidate-sensitivity from
   coverage semantics.*
2. Does not apply (H-T passes). 7. Does not apply.
3. H-T passes and `grp-topo`'s median exceeds `wf-asis`'s 0.1522.
4. *"every voter is candidate-sensitive on the centre-present reads and on no
   centre-absent read; the centre-absent share is 24.4 %"* (3018 of 12388
   reads).
5. *"with the masks held fixed the candidate-blind voters are not
   load-bearing; EQ5b §3.2's reading was carried by its mask change"*
   (written post-smoke, A2.4).
6. `grp-topo-solo` differs from `grp-topo` on 0 acts: *"on centre-present
   reads the routed group's act is the act of the candidate's own role query
   alone; the other voters decided no read; the result concerns who decides,
   not the aggregation of more than one candidate-informed opinion, and shows
   no group deliberating about a candidate."* (written post-smoke, A2.4).

## 2. Gates

- **X-battery — PASS on the second of two serial runs.** The first run on
  `1688b75` flipped the v1 latency criterion (mag median 3.306 µs < aif
  3.871 µs → PASS, where `docs/runs/K4-archive.log` has FAIL) and with it
  `VERDICT (v1)`, Path A and `VERDICT (v2)`; all 13 hunks surviving the
  latency-column strip were latency or wall-clock lines, none in Part 11. The
  re-run the prereg allows reproduced all 33 verdict lines byte-for-byte
  (`cmp` silent) with 11 surviving hunks, all latency or wall-clock. Not
  re-baselined.
- **X-host — PASS** on `1688b75` (`tests/k7_group_host.rs`, 2 passed): the
  harness-hosted `grp-role` / `grp-role-blind` on the consumed block 330..360
  reproduce `docs/runs/K4-archive.log:2032`, `:2036` and the reach row `:2071`.
- **X-identity — PASS.** `grp-topo-id` ≡ `grp-role` on trace entries, PRIMARY
  bits and churn, 30/30.
- **X-recon — PASS** (A2.2). PRIMARY recomputed from the reconstructed final
  member sets equals `WorkflowResult::primary` bitwise on all eight cells,
  30/30 each; recomputed churn equal on 240/240 cell-seeds.
- **S-determinism — PASS**, six group cells 30/30. **Seed invariance — PASS**,
  30/30.
- **S-learn (i) — PASS**, six group cells 30/30, `begin_task_rejections` 0.
  **Exempt model disclosed:** `(107, r1)` on every group cell.
- **S-route — PASS.** Routed reads: `grp-topo` 8602 · `grp-topo-q` 8613 ·
  `grp-topo-solo` 8602 · the three unrouted cells 0. *S-route certifies that a
  non-identity topology was built and counted, not that an output moved;
  decision liveness is S-live and E-follow.*
- Declines: `declines_upstream`, `declines_missing_role`,
  `declines_no_demand` are 0 in every group cell (`decisions == reads`).

## 3. Mechanism

**3.1 The control fails on the success term, on the three-role tasks.**

| cell | roster | tasks | success rate | mean cov_eff | mean final size | tasks ending empty |
|---|---|---:|---:|---:|---:|---:|
| `grp-role` | all | 600 | 51.7 % | 0.3848 | 1.62 | 111 |
| `grp-role` | 1 / 2 / 3 | 98 / 343 / 159 | 88.8 % / 65.0 % / **0.0 %** | 0.7279 / 0.4279 / 0.0803 | 1.48 / 2.12 / 0.63 | 0 / 2 / 109 |
| `grp-topo` | all | 600 | 99.7 % | 0.4340 | 2.73 | 0 |
| `grp-topo` | 1 / 2 / 3 | 98 / 343 / 159 | 100.0 % / 99.7 % / 99.4 % | 0.7313 / 0.4281 / 0.2635 | 1.58 / 2.48 / 3.97 | 0 / 0 / 0 |

`grp-role` succeeds on **0 of 159** roster-3 tasks and ends 109 of them with
an empty coalition. The read that does it: *leave, roster 3, centre declines,
two blind act votes* — `grp-role` acts (evicts) on **607 of 607**, `grp-topo`
on **0 of 629**. That is §4's identity arithmetic `p(act) = k/(k + 1)` at
`k = 2`: two candidate-blind voters outvote the one internal that can see the
member is needed. At roster 2 (*leave, centre declines, one blind act vote*)
the control evicts on 115 of 839, the routed arm on 0 of 848. The routed arm
ends **larger** (2.73 vs 1.62 members) and wins on success (99.7 % vs
51.7 %), not by a smaller coalition raising coverage efficiency (0.4340 vs
0.3848).

**3.2 The routed arm is a redundancy prune, not a deliberating group.**
`grp-topo`'s final member set equals `ref-prune`'s on **598 of 600** tasks and
its PRIMARY is bit-identical on **28 of 30** seeds; the two differing seeds
are 98 and 110, and on both `ref-prune` is higher (0.4708 vs 0.4473, 0.3142
vs 0.2961) — `ref-prune` succeeds on 600 of 600 tasks, `grp-topo` on 598.
The registered reading fires: *"`grp-topo`'s PRIMARY is the PRIMARY of a
learning-free arrival-order redundancy prune; S-learn (i) certifies that the
models learn, not that learning changes an outcome."* On this world — outcome
signal a deterministic function of coverage — the engine, its learned
precisions and the routing together reproduce a rule with no engine.

**3.3 Who decides.** E-follow, centre-present reads, group act == the
centre's own argmax: `grp-topo` **100.0 % (9370 of 9370)**, `grp-topo-solo`
100.0 % (9370 of 9370), `grp-topo-q` 99.9 % (9373 of 9381), `grp-role` 91.3 %
(8751 of 9583; leave reads 85.5 %). E-λ: `grp-topo-solo` vs `grp-topo` — 0
positional act differences, 3805 score-bit differences, 0/30 seeds: λ is live
in the score and dead at the decision between ½ and 1. `grp-topo-q` vs
`grp-topo` — 323 positional act differences on 5/30 seeds (an upper bound
after the first divergence); within `grp-topo-q` the group's act differs from
the centre's argmax on 8 of 9381 centre-present reads.
*The blind-origin mass share is mixture mass, not decision influence:*
`grp-topo` carries 0.427 of its mixture mass from blind members (`grp-role`
0.638, `grp-topo-solo` 0.244) while blind members decided 0 of its 9370
centre-present reads.

**3.4 The centre-absent reads.** 24.4 % of `grp-topo`'s reads have no
candidate-sensitive row under any routing. There the arm admits the candidate
at 95.1 % (1398 of 1470) and evicts it at 100.0 % (1548 of 1548); those
evictions are 1548 of its 4692 removals (33.0 %), and no final coalition holds
a member whose role has no demand (0 of 1636). *Churn is quoted with that
floor:* `grp-topo` 4692 vs `grp-role` 5620 removals, of which centre-absent
1548 vs 1598 — the reduction is in the centre-present removals (3144 vs 4022).

**3.5 S-live.** `grp-topo` vs `grp-role`: 2194 positional act differences and
6574 score-bit differences, 30/30 seeds with an act difference (positional;
an upper bound after a seed's first divergence; per-seed rows in the log).

**3.6 Context.** `wf-asis` 0.1522 — success 76.5 %, mean final size 5.01.
`grp-role` is strictly superior to it on 17/30 seeds, `grp-topo` on 30/30.

## 4. Implementation / deviation ledger (mandatory)

1. **The effect was seen before the run.** Arm, λ, bar and seeds were
   committed at 11:32 (`1948439`); the first execution of the confirmatory
   arm was at 12:10. Pre-run executions, all under `K7_1_SEEDS` with no
   verdict line, `grp-topo` vs `grp-role`: 330..333 **(a consumed block)**
   2.0199× 3/3; 1000..1006 2.2911× 5/6; 2000..2006 1.8885× 4/6; 3000..3030
   2.3416× 30/30 (plus a reviewer's scratch probe on 3000..3060); 4000..4003
   and 4000..4030 2.6094× 30/30. Amendments 2 and 3 — `ref-prune`, the
   decomposition, clauses 5–7 — were written after those numbers were visible
   and change no arm, λ, bar, seed or gate predicate. The official run is a
   replication of a seen effect on a fresh block. The binary now refuses
   `K7_1_SEEDS` over `0..480`.
2. **Three pre-run amendments.** A1 named the quantity of §4's two flip
   numbers. A2 applied the three-lens review (correctness: no defect, four
   low findings; conformance: eleven deviations, none moving H-T at a positive
   control median; modelling: the prune equivalence and the success-term
   mechanism, both measured off-block). A3 continued the execution ledger.
3. **The log's header names "Amendments 1 and 2"**; Amendment 3 (`2f82122`)
   predates the run and changes nothing the binary computes.
4. **X-battery needed its one permitted re-run** (§2): a v1 latency-criterion
   flip on the first serial run, every non-latency line identical on both.
5. **E-λ's registered sentence was withdrawn pre-run (A2.6).** Positional act
   counts overstate: on an off-block block 56 positional differences stood
   for 1 state-aligned disagreement. On 90..120 `grp-topo-solo` vs `grp-topo`
   is 0 either way.
6. **`MultiplicityWeighted` is not hostable through the H1 hook** —
   `TaskStart::steps` carries distinct steps — so no `grp-mult` cell exists
   (prereg §2).
7. **A new decline path exists under routing only** (A1.2): an error routing
   the H-S disclosure declines and counts in `declines_upstream`. Measured 0
   in every cell.
8. **S-learn (i)'s exemption fired once**: `(107, r1)`, `expected == 0`.
9. **Latency is record-only**: median µs/decision 51.487 (`grp-topo`) vs
   51.082 (`grp-role`); `ref-prune` 0.137.

## 5. What this does and does not settle

- **Settled:** on the v2w world with a coverage-determined outcome signal,
  routing the candidate's own role internal to every voter at λ = ½ beats the
  role-slotted group at the registered bar — 2.41×, 30/30, every gate holding.
- **Settled, and scoping the name:** part (i)'s answer is structural — every
  voter is candidate-sensitive on the 75.6 % of reads with the candidate's
  role on the roster and on none of the rest. The win is **who decides**: the
  candidate's own role query alone, whose leave read is a coverage-redundancy
  test, in place of a CW mixture two blind act-voters dominate. It is not
  deliberation, and a rule with no engine reaches the same PRIMARY.
- **Settled against EQ5b's reading:** with the masks held fixed, the
  candidate-blind voters are not load-bearing — they cost the control every
  three-role task. EQ5b's `grp-role-blind` collapse came from its mask change.
  EQ5b's report and verdict are untouched.
- **Not settled:** whether any advantage survives an outcome signal that is
  not a deterministic function of coverage (`OutcomeSignal::Performance` /
  `Both` are unused here) — the condition under which learning could change
  an outcome and `ref-prune` could lose; any adjacency rule but the
  candidate-star; λ as a tuned quantity; the centre-absent reads; transfer off
  v2w. koa#54 (magnitude = demonstrated default) stays FINAL.

This document is immutable. Follow-ups land as an appended addendum.
