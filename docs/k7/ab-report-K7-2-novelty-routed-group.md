# A/B report: K7-2 — novelty on/off on the topology-routed group

> **VERDICT (official run 2026-09-18, seeds 150..180): `FALSIFIED (novelty
> moves the outcome)`** — H-live passes (30/30 and 30/30), H-dead fails on the
> novelty-off cell alone (244 of 600 tasks, 0 of 30 seeds), H-move passes with
> novelty **on** the better routed cell (3.0017×, 30/30); every gate holds. The
> lock's registered expectation, *dead at the outcome*, was wrong. **It is
> scoped by clause 7:** the better cell ends exactly where the engine-free
> prune ends (600 of 600 tasks), so the contrast measures the routed query
> *without* the term, not a gain over a rule with no engine. Registration:
> [`prereg-K7-2-novelty-routed-group.md`](prereg-K7-2-novelty-routed-group.md)
> (Amendments 1–4, all pre-run). Run of record: koalisi `3a9557c` (`v0.40.0`),
> `aif-v0.14.0`, catgraph `v0.23.0` ×3, `--release`; raw output
> [`../runs/K7-2.log`](../runs/K7-2.log) (line numbers below are its).

## 1. Result

The counted criterion (prereg §5), as the binary printed it (`:81`–`:90`):

| leg | predicate | measured | |
|---|---|---|---|
| H-live 1 | `grp-topo-nonov` vs `grp-topo`, seeds with ≥ 1 raw-score-bit difference ≥ 18/30 | 30/30 | PASS |
| H-live 2 | seeds with ≥ 1 act difference ≥ 18/30 | 30/30 | PASS |
| H-dead, `grp-topo` | final member set = `ref-prune`'s on ≥ 570 of 600 tasks; PRIMARY bit-identical on ≥ 24/30 seeds | 600 of 600; 30/30 | PASS |
| H-dead, `grp-topo-nonov` | same | 244 of 600; 0/30 | **FAIL** |
| H-move, on over off | other's median > 0, ratio ≥ 1.25×, strictly superior ≥ 18/30 | 0.4248 / 0.1415 = 3.0017×; 30/30 | PASS |
| H-move, off over on | same | 0.3331×; 0/30 | FAIL |

| cell | role | median PRIMARY | median churn |
|---|---|---:|---:|
| `grp-topo` | contrast, novelty on | 0.4248 | 160.00 |
| `grp-topo-nonov` | contrast, novelty off | 0.1415 | 121.00 |
| `grp-role` | reference | 0.1696 | 209.00 |
| `grp-role-nonov` | reference | 0.1067 | 158.50 |
| `ref-prune` | reference | 0.4248 | 187.00 |
| `ref-first` | reference | 0.0000 | 0.00 |
| `ref-keep` | reference | 0.0801 | 0.00 |

Scoped clauses, as printed (`:427`–`:438`):

1. *"the contrast is the centre's query with and without the term only where
   the centre decides"*. E-follow: `grp-topo` 9711 of 9715, `grp-topo-nonov`
   8387 of 8571. *"on `grp-topo-nonov` the group's act differed from the
   centre's argmax on 184 of 8571 centre-present reads; on those reads the
   contrast is not the centre's query alone."* (and 4 of 9715 on `grp-topo`).
2. *"the ratio of medians novelty-off / novelty-on is 0.6291× on the unrouted
   pair and 0.3331× on the routed pair on this block."*
3. First divergence, routed pair: join read with `grp-topo` acting 27 seeds,
   leave read with `grp-topo` acting 3, the novelty-off cell acting first 0.
   Unrouted pair: 21 / 9 / 0.
4. `grp-topo-nonov` left `ref-prune` on 356 of 600 tasks; `grp-topo` passes
   both conjuncts.
5. **5a** S-nov's redundant-member leave read: novelty-off declines where
   novelty-on acts on 0 of 1 fresh and 1 of 1 warmed; exact ties 0 of 1.
   **5b** *"3 of 30 seeds with an act difference have as first divergence a
   leave read on which `grp-topo` acts and `grp-topo-nonov` declines"*; that
   bucket is not the largest. **5c** §4's consequence, *the lock's dead at the
   outcome fails*, is **confirmed**.
6. Does not apply (no median is 0).
7. *"the better cell ends where the engine-free prune ends; the contrast
   measures the query without the term, not a gain over a rule with no
   engine."*

A3.3's two readings do not apply: `grp-topo-nonov` matches `ref-keep` on 8 of
600 tasks and `ref-first` on 56 of 600 (`:270`–`:271`).

## 2. Gates

- **X-battery — not run**, by prereg §5's condition (owner, 2026-09-18). On
  `3a9557c`, `git diff --name-only v0.39.0..HEAD -- src Cargo.toml Cargo.lock`
  prints `Cargo.lock`, `Cargo.toml`, `src/harness/mod.rs`,
  `src/harness/recon.rs`; `git diff -U0 v0.39.0..HEAD -- Cargo.toml Cargo.lock`
  is koalisi's own `version` line in each, the `[[example]] k7_2` stanza and
  the `[[test]] k7_2_novelty` stanza. No path under `src/` outside
  `src/harness/`, no dependency line, no foreign lock stanza.
- **X-host, X-carry, S-nov — PASS** on `3a9557c`, before the run: `cargo
  nextest run --features harness,decision,process` 307 run, 307 passed — the
  three X-host tests (330..360 against `K4-archive.log:2032`, `:2036`,
  `:2037`, `:2071`, `:2076`), the two X-carry tests (90..120 against
  `K7-1.log:32`, `:34`, `:38`, `:45`–`:74`, `:164`, `:185`–`:188`,
  `:201`–`:204`), S-nov's two tests (A1.1's and A2.2's eight pins), and the
  150..180 generate check. `cargo test --features harness,decision,process
  --example k7_2`: 7 passed.
- **X-recon — PASS**, seven cells 30/30; recomputed churn equal on 210/210
  cell-seeds. **S-determinism — PASS**, four group cells 30/30. **Seed
  invariance — PASS**, both routed cells 30/30.
- **S-learn (i) — PASS**, four group cells 30/30. Exempt models, every group
  cell: `(157, r0)`, `(162, r2)`, `(170, r0)`, `(177, r1)`.
- **S-route — PASS.** Routed reads: `grp-topo` 8689, `grp-topo-nonov` 7686,
  the unrouted cells 0.
- Declines: `declines_upstream`, `declines_missing_role`,
  `declines_no_demand` and `begin_task_rejections` are 0 in every group cell;
  `decisions == reads` (`:420`–`:423`).
- The mask mirror's integrity check holds on 30/30 seeds for all four group
  cells (`:393`), so no row of the four-class table is withheld.

## 3. Mechanism

**3.1 The novelty-off arm loses on success, at the same size.**

| cell | success rate | mean cov_eff | mean final size | tasks ending empty | final members whose role has no demand |
|---|---:|---:|---:|---:|---:|
| `grp-topo` | 100.0 % | 0.4370 | 2.76 | 0 | 0 of 1654 |
| `grp-topo-nonov` | 46.2 % | 0.3411 | 2.73 | 6 | 232 of 1635 |
| `ref-prune` | 100.0 % | 0.4370 | 2.76 | 0 | 0 of 1654 |

By roster 1 / 2 / 3 the novelty-off success rate is 72.9 % / 45.2 % / 29.2 %
(`:298`–`:300`). The two routed cells end at the same mean size; the
novelty-off cell ends with the wrong members — it leaves demanded steps
uncovered and keeps 232 members whose role the task does not demand.

**3.2 It appears once a task has been observed** (`:374`–`:377`).

| cell | tasks | join-act rate | leave-act rate | success rate | mean final size |
|---|---|---:|---:|---:|---:|
| `grp-topo` | task 0 | 100.0 % (309 of 309) | 73.2 % | 100.0 % | 3.03 |
| `grp-topo` | tasks ≥ 1 | 93.7 % | 74.2 % | 100.0 % | 2.74 |
| `grp-topo-nonov` | task 0 | 97.4 % (301 of 309) | 72.5 % | 100.0 % | 3.03 |
| `grp-topo-nonov` | tasks ≥ 1 | 69.9 % (4105 of 5871) | 67.0 % | 43.3 % | 2.71 |

On task 0 the two cells succeed on every seed at the same size. The first act
difference falls on task 0 in 5 seeds, task 1 in 20, task 2 in 4, task 3 in 1
(`:203`), and on a **join** read in 27 of 30.

**3.3 Where the term decides: the centre's identical-mask reads whose window
holds a success** (`:397`–`:400`; group acts of centre-present reads).

| cell | read | masks identical, window true | masks identical, window false | masks differ, window true | masks differ, window false |
|---|---|---:|---:|---:|---:|
| `grp-topo` | join | 78.5 % (988 of 1258) | 100.0 % (1065 of 1065) | 100.0 % | 100.0 % |
| `grp-topo-nonov` | join | **18.8 % (291 of 1545)** | 87.6 % (1133 of 1293) | 100.0 % | 100.0 % |
| `grp-topo` | leave | 100.0 % (1706 of 1706) | 100.0 % (1596 of 1596) | 0.0 % (0 of 1011) | 0.0 % (0 of 643) |
| `grp-topo-nonov` | leave | 68.7 % (532 of 774) | 100.0 % (1766 of 1766) | 1.1 % (5 of 450) | 12.9 % (106 of 822) |

Where
the candidate changes the centre's coverage the two cells agree on joins
(100 %); they part where it does not. There a join is admitted by `grp-topo`
and, when the role's recent window holds a success, declined by
`grp-topo-nonov` on 81.2 % of reads. A candidate whose bits a role-mate
already holds is the role's *subsumed* joiner; declining it while the leave
sweep still evicts redundant members (68.7 % / 100.0 %) is how steps end
uncovered. The novelty-off cell also evicts members whose removal uncovers a
step on 111 of 1272 such reads; `grp-topo` on 0 of 1654. The *window* column
is a world-side stand-in for the engine's replay window, not a read of it
(A3.4).

**3.4 Lens 3's derivation (A3.5), against the run.** Confirmed in direction:
without the term an identical-mask read follows its window (18.8 % against
87.6 % on joins, 68.7 % against 100.0 % on leaves). **Contradicted in one
clause:** *"with the term, toward act whatever its window holds"* — `grp-topo`
declines 270 of 1258 identical-mask joins whose window holds a success. §4's
registrant's note called the consequence (5c) and not the place: the first
divergence is a redundant-member leave in 3 of 30 seeds.

**3.5 The premise holds on 97.9 % of the novelty-off cell's centre-present
reads.** All 184 departures are join reads (`:285`), and the votes table
places them (`:343`–`:349`): a declining centre overridden by blind act votes
on 103 of 464 (roster 2, one blind act vote), 38 of 318 and 39 of 195 (roster
3, one and two), and an acting centre not followed on 3 of 611 and 1 of 214.
With the term the same rows read 0 of 134, 0 of 31 and 4 of 69. On those 184
reads the contrast is not the centre's query alone.

**3.6 Centre-absent reads** (`:326`–`:327`). Without the term the arm admits
an off-demand candidate on 74.7 % of reads (92.8 % with it) and evicts an
off-demand member on 80.6 % (100.0 % with it); the 232 retained off-demand
members of 3.1 are that gap.

**3.7 The unrouted pair.** `grp-role-nonov` 0.1067 against `grp-role` 0.1696
(0.6291×; novelty-on strictly superior on 26/30, novelty-off on 4/30). The
same four-class pattern shows at smaller contrast (`:401`–`:404`: identical
masks, window true, joins 45.8 % against 94.4 %). Clause 2 forbids subtracting
one ratio from the other, and this report does not.

**3.8 Live.** Routed pair: 3909 positional act differences and 10179
score-bit differences (upper bounds after each seed's first divergence),
30/30 seeds with each; the traces differ in length by 1402 decisions.

## 4. Implementation / deviation ledger (mandatory)

1. **Pre-run executions of the binary** (prereg A3.1, A4.1): 24, all under
   `K7_2_SEEDS` or refused before an instance was generated — blocks
   `6000..6003` (smokes and gate falsifications) and `6000..6030` (twice). No
   execution printed a table, median, ratio or per-seed value of a cell.
   **Two leaks, both before Amendment 3:** ten smoke runs printed a byte count
   of the suppressed report — a function of every suppressed value, from a line
   the registrant's brief had asked for — and the implementer reports starting
   to reason from a gap between two of them toward a label on `6000..6003`
   before stopping; one X-recon falsification (`× 0.5`) showed PRIMARY ≠ 0 for
   all five cells on seeds 6000–6002. The byte count was removed and
   falsifications made additive before the final smokes.
2. **Hand-fixture reads of the novelty-off configuration**: S-nov's four fresh
   and four warmed reads; one unbriefed probe by the implementer at warm-up
   0 / 1 / 2 / 5 / 20 (A1.2), deleted after one run; S-nov's falsification
   reads in a copy. No generated instance was involved. Amendments 1–4 were
   written with those values visible and say so.
3. **The three review lenses ran nothing** (their reports): code, the engine
   source and published logs only.
4. **The registered expectation was wrong**, and was doubted before the run:
   §4's registrant's note, then A1.2's fixture values, then A3.5.
5. **X-battery was not run** (§2), by the prereg's own condition.
6. **Amendment 2's warmed read** calls `begin_task` again before the read
   (begin → observe → begin → read), following `GroupAifPolicy::begin_task`'s
   once-per-task contract; A2.1 did not spell that out.
7. **Test-side hosting on consumed blocks**: X-host on 330..360, X-carry on
   90..120, the binary's mirror test on 90..93 (`grp-topo`, `grp-role`).
8. **The official run needed an opt-in** (`K7_2_OFFICIAL=1`, A3.2) and ran
   once, on a quiet machine (`pgrep -c 'cargo|rustc'` printed 0), stderr
   empty. The binary refuses the registered block on any build but `0.40.0`.
9. **The gates' configurations equal §3's by value**, with no assertion tying
   them (A3.8). X-recon shares its scoring with the harness loop; the
   hand-value test in `src/harness/recon.rs` is the independent pin.
10. **Latency is record-only**: median µs/decision 52.964 (`grp-topo`),
    44.209 (`grp-topo-nonov`), 0.161 (`ref-prune`).

## 5. What this does and does not settle

- **Settled:** on v2w under `RoleCoverage`, removing the A-novelty term from
  the routed group's queries costs two thirds of its PRIMARY (0.3331×, the
  novelty-on cell superior on 30/30), on the success term, from the first
  observed task on.
- **Settled, and scoping the label:** the better cell is the prune. `grp-topo`
  ends where `ref-prune` ends on 600 of 600 tasks, PRIMARY bit-identical on
  30/30 seeds. The term does not buy an outcome a rule with no engine misses;
  its absence breaks the query — an identical-mask join is then decided by
  whether the role's last tasks succeeded, and a success declines it.
- **Settled against the lock's framing:** routing removes the candidate-blind
  voters, not the candidate-blind queries. The contrast is the centre's query
  with and without the term, on 97.9 % of the novelty-off cell's
  centre-present reads.
- **Not settled:** whether the term, or learning at all, changes an outcome
  where the outcome is not a deterministic function of coverage
  (`OutcomeSignal::Performance` / `Both`) — the continuation below; the 184
  override reads; λ, the adjacency rule, the world-model topology; transfer off
  v2w. EQ5b's and K7-1's reports and verdicts are untouched. koa#54
  (magnitude = demonstrated default) stays FINAL.

**Continuation (A3.7).** `grp-topo` passes both of its H-dead conjuncts: the
next registration moves the world, on its own issue, next free K7 number,
fresh seed block.

This document is immutable. Follow-ups land as an appended addendum.
