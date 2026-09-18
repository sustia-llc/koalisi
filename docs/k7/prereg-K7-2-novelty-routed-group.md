# Pre-registration: K7-2 — novelty on/off on the topology-routed group

**Status: REGISTERED.** Committed BEFORE implementation. Design-lock of record:
[koalisi #97](https://github.com/sustia-llc/koalisi/issues/97) — the owner lock
of 2026-09-17. Four design calls the lock left to this document were taken by
the owner on 2026-09-18 before it was written: the counted criterion and its
three-way reading (§5, §6), the smoke policy (§8), the promotion of K7-1's
example-side instruments into `src/harness/` (§1), and the scope of X-battery
(§5). Amendments are pre-verdict only, appended here and posted on #97 before
any affected code runs; after the official run this document is immutable
([`PROTOCOL.md`](../PROTOCOL.md) §1).

## 1. Registration statement

- **Question (EQ5b candidate (b)).** EQ5b
  ([report](../ab-report-K4-eq5b-typed-two-engine.md) §3.3) measured
  `grp-role-nonov` against `grp-role` and could not separate the inherited v5
  novelty mechanism from the candidate-blind bias: novelty-off also collapses a
  candidate-blind internal's read from 1.0 to 0.5
  (`docs/runs/K4-archive.log:2065`). K7-1
  ([report](ab-report-K7-1-topology-routed-group.md) §3.3) measured that under
  candidate-star routing at λ = ½ the group's act equals the centre's own
  argmax on 9370 of 9370 centre-present reads. K7-2 asks: **on that routed
  arm, what does `query_novelty: true` vs `false` change — in the score, in
  the acts, and at the outcome?**
- **Born on:** koalisi `v0.39.0` (merge `ef33af7`): `aif-v0.14.0`, catgraph
  `v0.23.0` ×3, `surrealdb-live-message` `v0.2.2`, `rust-version` 1.93. `src/`
  is unchanged since `v0.38.0` (`git log --oneline v0.38.0..ef33af7 -- src`
  prints nothing).
- **Seeds:** `150..180`, released by the owner on #97 (ledger: reserved —
  K7-2).
- **Placement:** `examples/k7/k7_2.rs`, its own `[[example]]`
  (`required-features = ["harness", "decision", "process"]`); every library
  change through `src/`. `examples/k7/k7_1.rs` and
  `examples/strategy_comparison.rs` are not edited.
- **Promotion (owner, 2026-09-18).** K7-1's example-side instruments — the
  trace → final-member-set reconstruction, the `ref-prune` policy, the
  decomposition by roster — move into `src/harness/` as the first code commit,
  with their own pins (§5 X-carry). No change to `src/decision/` is planned.
- **Flat.** One `GroupAgent`, no nested member; tira #51 does not fire.
- **Standing constraint.** koa#54 (magnitude = demonstrated default) is FINAL;
  no K7-2 outcome reopens it.
- **K4-lineage and K7-1 numbers are motivating evidence, never a bar.** Every
  criterion below is stated against cells run in this battery, on these seeds,
  at this pin.

## 2. World and outcome semantics

K7-1's, unchanged ([its prereg](prereg-K7-1-topology-routed-group.md) §2):
`WorkflowSpec::default()` (the v2w draw), `run_workflow_battery` /
`run_workflow_instance` as shipped, PRIMARY = success rate × mean coverage
efficiency over the declared distinct `(bit, role)` steps,
`OutcomeSignal::RoleCoverage`, the H1 lifecycle hooks (`TaskStart::steps`
carries distinct steps; multiplicity 1 through the hook).

## 3. Cells

Every group cell is `GroupAifPolicy` over `WorldModelTopology::RoleSpecialised`,
`PrecisionChannel::RoleRestricted`, `CoverageMasks::RoleMatched`,
`DecisionRead::Deterministic`, `GroupVote::CertaintyWeighted`, `n_roles = 3`,
one fresh policy per seed built from `WorkflowInstance::role_map`. *Novelty
off* is `base: PersistentAifConfig { query_novelty: false, ..v5_e1_base() }` —
EQ5b's `grp-role-nonov` construction
(`examples/strategy_comparison.rs:11761`). The flag's non-test reads are
`src/decision/aif_persistent_policy.rs:775`, `:802`, `:803`
(`rg -n 'query_novelty' src/decision/aif_persistent_policy.rs`): it sets the
per-decision query's `use_param_info_gain` and `use_b_info_gain` and nothing
else.

| cell | configuration | role |
|---|---|---|
| `grp-topo` | candidate-star routing, λ = ½, novelty on — K7-1's confirmatory cell | **contrast, novelty on** |
| `grp-topo-nonov` | as `grp-topo`, novelty off | **contrast, novelty off** |
| `grp-role` | routing off, novelty on (`GroupAifConfig::default()`) | reference — EQ5b's confounded pair |
| `grp-role-nonov` | routing off, novelty off | reference — EQ5b's confounded pair |
| `ref-prune` | K7-1 A2.2's engine-free rule: join always; leave iff every covered demanded `(bit, role)` step stays covered without the agent | reference |

Novelty is the only axis inside each pair. All cells report regardless of
outcome.

## 4. Registered prediction

**From the lock:** on this world — outcome a deterministic function of
coverage — novelty is **live in the score and the acts and dead at the
outcome**: the two routed cells compute different scores, take different acts,
and both end where `ref-prune` ends.

**Registrant's note, derived and unmeasured, written before any number
exists.** The note changes no cell, criterion or label. EQ5b measured that an
internal whose two coverage configurations are identical reads `p(act) = 1.0`
with novelty on and `0.5` with novelty off (`docs/runs/K4-archive.log:2008`,
`:2065`; the `novelty_off` test helper's rustdoc in
`src/decision/group_policy.rs`), and that 73.8 % of `grp-role`'s leave queries
have `cfg0 == cfg1` (`:2071`). A member whose removal changes no covered step
is such a query for its own role's internal — the centre. If the centre reads
0.5 there with novelty off, SP3's tie declines, and `grp-topo-nonov` does not
evict the members `grp-topo` and `ref-prune` evict; by the same arithmetic it
does not admit a candidate that adds no coverage. Under that reading the
redundancy prune **is** the novelty term, and the lock's *dead at the outcome*
fails. §5's S-nov pins the hand-fixture read before the run; the prediction
above stands as locked whatever S-nov shows.

## 5. Registered legs and gates

Traces are compared by position up to the shorter length. Up to a seed's first
act difference the two cells of a pair have taken the same acts over the same
instance, so *seeds with any difference* is uncontaminated; positional totals
after it are an upper bound (K7-1 prereg A2.6).

### The counted criterion (owner, 2026-09-18)

- **H-live — both conjuncts.** `grp-topo-nonov` against `grp-topo`: seeds with
  ≥ 1 raw-score-bit difference ≥ **18/30** **and** seeds with ≥ 1 act
  difference ≥ **18/30**.
- **H-dead — all four conjuncts.** For **each** of `grp-topo` and
  `grp-topo-nonov`, against in-battery `ref-prune`: final member set identical
  on ≥ **570 of 600** tasks **and** PRIMARY bit-identical on ≥ **24/30**
  seeds. At a block of other than 30 seeds × 20 tasks the thresholds are 95 %
  of tasks and 80 % of seeds, rounded up.
- **H-move — read only when H-dead fails.** Either routed cell against the
  other: the other's median PRIMARY > 0, the ratio of medians ≥ **1.25×**,
  **and** strictly superior on ≥ **18/30** seeds. The direction is named. (The
  lineage's standing effect bar.)

### Registered disclosures (all non-gating)

- **S-live**, per seed and pooled, for both pairs (`grp-topo-nonov` vs
  `grp-topo`; `grp-role-nonov` vs `grp-role`): positional act differences,
  raw-score-bit differences, seeds with any of each, length difference.
- **First divergence**, per seed, both pairs: the position of the first act
  difference, its read kind (join / leave, from the trace entry) and which
  cell acted; pooled counts by read kind and acting cell.
- **Against `ref-prune`**, every group cell: tasks with identical final member
  set, seeds with bit-identical PRIMARY; among the differing tasks, the count
  whose `(covered steps, final size)` equal `ref-prune`'s.
- **Inside each pair**: tasks with identical final member set, seeds with
  bit-identical PRIMARY, ratio of median PRIMARY, strictly-superior seeds in
  both directions.
- **E-follow**, every group cell, join / leave split, over all centre-present
  reads, from the `AgreementSample` ledger.
- **Outcome decomposition** (K7-1 A2.3's): per cell and by realised roster
  1 / 2 / 3 — tasks, success rate, mean coverage efficiency, mean final size,
  tasks ending empty; join-act and leave-act rates split centre-present /
  centre-absent; churn with the same split.
- **Candidate reach** per group cell (EQ5b's A5.1 row: zero
  candidate-sensitive share, mean blind internals, blind CW weight share,
  leave `cfg0 == cfg1`, act rate sensitive / blind) and the blind-origin mass
  share beside E-follow.
- **E-lat.** Median µs/decision per cell. Record-only.
- **Declines**: `declines_upstream`, `declines_missing_role`,
  `declines_no_demand` per group cell. A zero score is not a decline.

### Gates (any failure ⇒ `RUN-INVALID`)

- **X-battery — conditional on the diff (owner, 2026-09-18).** The frozen
  archive binary names nothing in `src/harness/`
  (`rg -c 'koalisi::harness|harness::' examples/strategy_comparison.rs` matches
  nothing). On the final tree, `git diff --name-only v0.39.0..HEAD -- src
  Cargo.toml Cargo.lock` is read: a path under `src/` outside `src/harness/`,
  or a dependency line in `Cargo.toml` / a stanza other than koalisi's own in
  `Cargo.lock`, puts the gate on — one fresh serial archive run diffed against
  `docs/runs/K4-archive.log` with the latency column stripped, a
  latency-criterion flip re-run once. Otherwise the gate is recorded **not
  run**, with that command's output, in the report, the CHANGELOG and the PR
  body.
- **X-host** (test suite). `tests/k7_group_host.rs`'s two K7-1 tests, plus the
  harness-hosted `grp-role-nonov` over the **consumed** block 330..360
  reproducing `docs/runs/K4-archive.log:2037` (median PRIMARY 0.1125, median
  churn 140.00) and its candidate-reach row `:2076` (23.0 %, 1.35, 0.621, 8201
  of 11173, 67.1 % / 73.6 %).
- **X-carry** (test suite; this registration's X-identity). Over the
  **consumed** block 90..120, §3's `grp-topo` and `grp-role` configurations
  and the promoted `ref-prune`, hosted by `run_workflow_instance`, reproduce
  `docs/runs/K7-1.log`: the per-seed PRIMARY columns of lines 45–74 at 4 dp,
  the pooled rows `:32`, `:34`, `:38` (median PRIMARY, median churn), and —
  through the promoted reconstruction — `grp-topo`'s identity with `ref-prune`
  on 598 of 600 tasks and 28 of 30 seeds (`:164`). Novelty has no identity
  value inside a boolean, so the gate is carried by K7-1's run of record: the
  novelty-on side of the contrast and both promoted instruments are the ones
  K7-1 measured.
- **X-recon** (in-binary). PRIMARY recomputed from the reconstructed final
  member sets equals `WorkflowResult::primary` bitwise on all five cells, per
  seed; every trace consumed exactly.
- **S-determinism.** Every group cell re-run from scratch per seed and
  compared on trace entries (leave flag, act, raw score bits), PRIMARY bits
  and churn. **Seed invariance** on `grp-topo` and `grp-topo-nonov`:
  `battery_seed ^ 0x9E37_79B9_7F4A_7C15` reproduces each bit-for-bit.
- **S-learn (i).** Per seed and group cell: `GroupAifCounters::s_learn_exact`,
  `models_moved` with its `expected == 0` exemption at `S_LEARN_VACUITY_TOL`,
  exempt models disclosed, `begin_task_rejections == 0`.
- **S-route.** Per seed on `grp-topo` and `grp-topo-nonov`: `routed_reads` ==
  ledger reads with `candidate_sensitive >= 1` and `roster >= 2`, block total
  > 0; on `grp-role` and `grp-role-nonov`, `routed_reads == 0`.
- **S-nov** (test suite; the lever reaches the engine). On a hand-built
  fixture — no generated instance — one leave read of a member whose removal
  changes no covered step and one join read of a candidate that adds a covered
  step, each under §3's `grp-topo` and `grp-topo-nonov` configurations: the raw
  score bits differ between the two configurations on at least one of the two
  reads. The test pins the four `(act, score)` values it measures; if they
  disagree with §4's note, the note is corrected by a pre-run amendment.

## 6. Verdict labels (pre-committed, in precedence order)

1. **`RUN-INVALID`** — any gate fails, or the run exits before printing a
   `VERDICT:` line.
2. **`FALSIFIED (novelty not live)`** — H-live fails either conjunct. Score
   conjunct passing with the act conjunct failing reads *"live in the score,
   dead at the decision"* (the [#80 report](../ab-report-K4-residual-process-specificity.md)'s
   class); both failing reads *"inert on the routed arm"*.
3. **`VALIDATED (novelty live, dead at the outcome)`** — H-live and H-dead
   both pass. Reads: *"on v2w under `RoleCoverage` the v5 novelty term changes
   what the routed group computes and which acts it takes, and not where it
   ends: with it and without it the arm ends where a learning-free
   arrival-order redundancy prune ends."*
4. **`FALSIFIED (novelty moves the outcome)`** — H-live passes, H-dead fails,
   H-move passes. Reads: *"novelty {on|off} is the better routed cell at the
   lineage bar (X×, n/30)"*, with which of H-dead's four conjuncts failed.
5. **`FALSIFIED (not dead by count, no effect at the bar)`** — H-live passes,
   H-dead and H-move fail. Reported with the *against `ref-prune`* and
   *inside each pair* disclosures in the verdict paragraph.

**Pre-committed scoped clauses, fixed before any number is visible.**

1. **The premise.** The contrast prices the mechanism alone only where the
   candidate-blind voters decide no read. E-follow is reported for both routed
   cells under every verdict; if either is below 100 %, the verdict is
   reported with: *"on `<cell>` the group's act differed from the centre's
   argmax on n of N centre-present reads; on those reads the contrast is not
   the centre's query alone."*
2. **Against EQ5b's pair.** The report gives the in-battery ratio of medians
   `grp-role-nonov` / `grp-role` beside `grp-topo-nonov` / `grp-topo` and says
   only: *"novelty-off costs the unrouted group X× and the routed group Y× on
   this block."* It does not subtract one from the other.
3. **Where the first divergence falls.** The pooled first-divergence counts by
   read kind and acting cell are quoted under every verdict.
4. **H-dead failing on one cell only** is reported as which cell left
   `ref-prune` and on how many tasks.
5. **§4's note** is reported as confirmed or contradicted by S-nov's pinned
   values and by the first-divergence table, under every verdict.

**Continuation (the lock's).** `VALIDATED` ⇒ the next registration moves the
world (`OutcomeSignal::Performance` / `Both` with the harness's performance
draw), on its own issue, next free K7 number, fresh seed block. Any
`FALSIFIED` ⇒ K7-3 is locked next, and its lock reads this report first.

## 7. What this cannot settle

- EQ5b's verdict, K7-1's verdict and every other registered verdict —
  untouched.
- Novelty under any outcome signal that is not a deterministic function of
  coverage.
- Novelty on the centre-absent reads, which no routing reaches (K7-1 H-S).
- Any λ but ½, any adjacency rule but the candidate-star, the `Shared`
  world-model topology (K7-3's axis), the multiplicity channel.
- The two information-gain flags separately: `query_novelty` sets both.
- Transfer off the v2w world. One world, one draw family, 30 seeds.

## 8. Pre-run executions (owner, 2026-09-18)

- **The contrast is not visible before the run.** `K7_2_SEEDS` accepts exactly
  two blocks, **`6000..6003`** and **`6000..6030`**, and refuses every other
  value. Under it the binary runs every cell and gate and renders every table,
  prints the header, the gate lines and `VERDICT: SMOKE (no verdict)`, and
  prints **no** table, median, ratio, count or per-seed value of any cell. The
  gate lines carry pass counts per cell and nothing else.
- **No other execution of `grp-topo-nonov` over a generated instance** — by
  the implementer, a reviewer or a test — precedes the run. The test suite
  hosts `grp-role` and `grp-role-nonov` on 330..360 and `grp-topo`, `grp-role`
  and `ref-prune` on 90..120 (reads EQ5b and K7-1 already published); S-nov's
  fixture is hand-built.
- Review lenses verify by reading code and by the executions above.
- The report lists every pre-run execution of the binary: block, commit, who,
  and that the output carried no cell value.

## 9. Order of operations

The version bump to `0.40.0` lands before the official run, so
`docs/runs/K7-2.log` names the released tree. X-host, X-carry, S-nov and —
if §5 puts it on — X-battery run on that tree **before** the official run; the
binary's `VERDICT:` line cannot see them, and the run is made only if all
hold. The run is serial on a quiet machine (`pgrep -c 'cargo|rustc'` prints
0), `--release`.

## 10. Report

`docs/k7/ab-report-K7-2-novelty-routed-group.md`, committed with
`docs/runs/K7-2.log`, carrying the verdict line exactly as printed, the
criterion, every gate outcome (X-battery's *not run* with its command output,
if so), §5's tables, §6's clauses and a numbered implementation / deviation
ledger including §8's execution list. Immutable once recorded.

## Amendment 1 (pre-run, 2026-09-18) — S-nov's pinned values, §4's note, one unbriefed probe

No generated instance has been run under `grp-topo-nonov`; `examples/k7/k7_2.rs`
does not exist yet. Cells, criterion, labels, gates' predicates, seeds and smoke
blocks are unchanged. Commit times: prereg `41d4eef` 08:49:48; library and
tests `fb6afb8`. **A1.2's values were visible when A1.3 was written.**

- **A1.1 — S-nov, as pinned** (`tests/k7_2_novelty.rs`; fresh policy per read,
  agents `0b011`@r0, `0b001`@r0, `0b100`@r1, steps `(0,r0) (1,r0) (2,r1)`,
  λ = ½, seed 11). Raw score bits differ between the two configurations on 2
  of 2 reads, so the gate's predicate holds.

  | read | novelty | act | score bits |
  |---|---|---|---|
  | leave, member whose removal changes no covered step | on | act | `0x3fe0000000000000` |
  | same | off | act | `0x3fdffffd4d048564` |
  | join, candidate adding `(1, r0)` | on | act | `0x3fe0000000000000` |
  | same | off | act | `0x3fdfffff837f4f34` |

- **A1.2 — disclosure: an unbriefed probe on the same hand-built fixture.**
  While measuring S-nov the implementer ran, once, a temporary test reading the
  same two queries after 0, 1, 2, 5 and 20 warm-up tasks (by the implementer's
  report, each `begin_task` + `observe_outcome` with the three required bits
  `true`), then deleted it; its 20 output lines were read by the registrant. No
  generated instance was involved, so §8's rule was not crossed; it is a wider
  look at the `grp-topo-nonov` configuration than S-nov's four reads and is
  entered in §8's ledger. At warm-up 0 it reproduces A1.1. At every warm-up
  ≥ 1: the leave read is **act** with novelty on (score ≈ +0.5) and **decline**
  with novelty off (score −0.5 at warm-up ≥ 2); the join read of the
  step-adding candidate is **act** with novelty on (≈ +0.5) and **decline**
  with novelty off (≈ −0.333).
- **A1.3 — §4's note, corrected.** On a policy that has observed no task the
  redundant-member leave read acts under both configurations; the note's
  tie-decline does not occur there. A1.2 shows the on/off act difference the
  note derives appearing after one observed task, and shows a second one the
  note did not derive: with novelty off the fixture's step-adding candidate is
  declined. The note's *"by the same arithmetic it does not admit a candidate
  that adds no coverage"* is withdrawn as unmeasured. §4's registered
  prediction stands as locked.
- **A1.4 — S-nov's predicate is unchanged**: the fresh-fixture reads, as §5
  registered. No warmed-fixture pin is added.
