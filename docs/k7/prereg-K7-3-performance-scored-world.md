# Pre-registration: K7-3 — the identity-keyed routed group on a performance-scored world

**Status: REGISTERED.** Committed BEFORE implementation. Design-lock of record:
[koalisi #100](https://github.com/sustia-llc/koalisi/issues/100) — the owner
lock, part 1 (2026-09-18), part 2 and its correction comment (2026-09-19). Two
design calls the lock left to this document were taken by the owner on
2026-09-19 before it was written: `ref-prune-id`'s record rule (§3) and the
thresholds of the counted criterion (§5). Amendments are pre-verdict only,
appended here and posted on #100 before any affected code runs; after the
official run this document is immutable ([`PROTOCOL.md`](../PROTOCOL.md) §1).

## 1. Registration statement

- **Question.** K7-2 ([report](ab-report-K7-2-novelty-routed-group.md) §5)
  measured that on v2w under `OutcomeSignal::RoleCoverage` the routed arm
  `grp-topo` ends where the engine-free `ref-prune` ends on 600 of 600 tasks.
  K7-3 asks: **on a world whose scored outcome is not a deterministic function
  of coverage, does a routed learning arm that is told who performed beat the
  engine-free prune?**
- **Born on:** koalisi `v0.42.0` (merge `d566d58`; `git log -1 --format='%h
  %D'` prints `d566d58 HEAD -> main, tag: v0.42.0, origin/main`):
  `aif-v0.14.0`, catgraph `v0.23.0` ×3, `surrealdb-live-message` `v0.2.2`,
  `rust-version` 1.93 (`rg -n '^rust-version|tag = ' Cargo.toml`).
- **Seeds:** `540..570`, released by the owner on #100 (ledger: reserved —
  K7-3).
- **Placement:** `examples/k7/k7_3.rs`, its own `[[example]]`
  (`required-features = ["harness", "decision", "process"]`); every library
  change through `src/`. `examples/k7/k7_1.rs`, `examples/k7/k7_2.rs` and
  `examples/strategy_comparison.rs` are not edited.
- **Library changes planned:** the `grp-id` model store in
  `src/decision/group_policy.rs`; `RefPruneId` in `src/harness/recon.rs`. The
  inherent `PersistentAifArm::observe_outcome` and
  `GroupAifPolicy::observe_outcome` keep their signatures.
- **Flat.** One `GroupAgent`, no nested member; tira #51 does not fire and no
  tira extension is needed.
- **Standing constraint.** koa#54 (magnitude = demonstrated default) is FINAL;
  no K7-3 outcome reopens it.
- **K4-lineage, K7-1 and K7-2 numbers are motivating evidence, never a bar.**
  Every criterion below is stated against cells run in this battery, on these
  seeds, at this pin. K4 Scope B scored `completed AND all final members
  performed`; its numbers do not transfer (lock part 2, fact 4).

## 2. World and outcome semantics

- **Instances.** `WorkflowSpec { performance: Some(PerformanceSpec {
  reliable_prob: 0.7, rho_reliable: 0.05, rho_flaky: 0.40 }),
  ..WorkflowSpec::default() }` — the v2w shape draw with the per-agent
  performance draw appended after it (`PerformanceSpec`'s rustdoc,
  `src/harness/workflow.rs`). One instance per seed; all seven cells run on
  that instance.
- **Loop.** `run_workflow_instance` as shipped at `v0.42.0`: first arrival
  joins unconditionally, each later arrival joins iff `should_join` acts, one
  leave sweep in arrival order, then `observe_outcome(required, per_bit,
  members)` with one `MemberOutcome { agent_id, performed }` per final member.
  Each task's `arrival` is a permutation of the pool
  (`src/harness/instance.rs`, `Task`'s rustdoc).
- **Scored outcome.** `WorkflowResult::performance_scored`: a distinct
  demanded `(bit, role)` step counts iff some final member of its role holds
  its bit **and performed on the task**; success iff the demand is non-empty
  and every distinct step counts; efficiency = counted fraction ÷ final member
  count; **PRIMARY-P** = `performance_scored.primary` = success rate × mean
  efficiency. The coverage-scored `WorkflowResult::primary` is reported beside
  it and enters no criterion.
- **Signals.** `grp-id` and `grp-topo` read `OutcomeSignal::Both`;
  `grp-topo-cov` reads `OutcomeSignal::RoleCoverage`. The signal reaches only
  the `per_bit` vector handed to the hook; every cell is scored the same way.

## 3. Cells

Every group cell is `GroupAifPolicy` with `PrecisionChannel::RoleRestricted`,
`CoverageMasks::RoleMatched`, `DecisionRead::Deterministic`,
`GroupVote::CertaintyWeighted`, `VoteRouting::CandidateStar { lambda: 0.5 }`,
`n_roles = 3`, `base: v5_e1_base()` (novelty on), one fresh policy per seed
built from `WorkflowInstance::role_map` — K7-1's `grp-topo` configuration
(K7-2 prereg §3).

| cell | configuration | role |
|---|---|---|
| `grp-id` | as `grp-topo`, with the agent-keyed model store below; reads `Both` + members | **the arm** |
| `ref-prune` | join always; leave iff every covered demanded step stays covered without the agent (`RefPrune`) | **the criterion's reference** |
| `ref-prune-id` | the identity prune below (`RefPruneId`) | reference — secondary read 1 |
| `grp-topo` | `WorldModelTopology::RoleSpecialised`, reads `Both` | reference — secondary reads 2, 3 |
| `grp-topo-cov` | `grp-topo` reading `RoleCoverage` on the same instances | reference — secondary read 3 |
| `ref-keep` | join always, never leave (`RefKeep`) | reference |
| `ref-first` | never join, never leave (`RefFirst`) | reference |

All cells report regardless of outcome. No unrouted `grp-role`, no novelty-off
cell (lock D-1).

### `grp-id` — fixed before code

- **Store.** A third model store beside `RoleSpecialised` and `Shared`: the
  `n_roles` role models of `RoleSpecialised`, plus one `PersistentAifArm` per
  agent of the role map, each built from `config.base`.
- **Query.** On a read whose candidate's role is on the task's roster (a
  centre-present read), the centre internal — the candidate's own role —
  builds its query with `role_query(required_r, cfg0, cfg1, …)` on **the
  candidate's agent model** in place of the role model; `required_r`, the
  masks, the seeds and the scale are what `grp-topo` passes. On a leave read
  the candidate is the member being read. Every other internal, and every
  internal of a centre-absent read, queries its role model as `grp-topo` does.
- **Role models observe** what `grp-topo`'s observe: per demanding role, its
  `required_r` against the `Both` per-bit vector.
- **An agent's model observes** only a task on which the agent is a final
  member and `required_r(own role) & capabilities != 0`: that mask as
  `required`, every bit of it read as the member's `performed`; every other
  bit is no-observation. An agent that is not a final member, whose role has
  no demand, or that holds no demanded bit of its role, observes nothing on
  that task.
- Routing, vote, read, novelty and the SP3 rule are `grp-topo`'s.

### `ref-prune-id` — fixed before code (owner, 2026-09-19)

- **Evidence.** Per agent, over the tasks observed so far: `n` = tasks on
  which it was a final member with `required_r(own role) & capabilities != 0`
  — the set `grp-id`'s agent model observes — and `p` = those on which it
  performed. Read from `MemberOutcome` and the last `begin_task`'s steps.
- **Record.** `(p + 1) / (n + 2)`; an agent with `n = 0` reads `0.5`. Records
  are compared exactly, as the integer cross-product `(p₁+1)(n₂+2)` against
  `(p₂+1)(n₁+2)`.
- **Join.** Acts on every call.
- **Leave.** Acts iff the agent is redundant under `ref-prune`'s predicate
  **and**, for every demanded step the agent covers, some other member shown,
  of that step's role and holding its bit, has a record ≥ the agent's.
- **Ties** therefore evict in arrival order, as `ref-prune` does; a policy
  that has observed no task takes `ref-prune`'s acts.
- Engine-free; every score is `0.0`.

## 4. Expectation

**From the lock (D-5).** `ref-prune`'s loss on this world is a flaky survivor:
which of two same-step coverers survives is fixed by arrival order,
independent of the draw. `grp-id` is given the input that separates them.

**Named risk (lock; gotcha-23 class).** An evicted agent stops earning
evidence, in `grp-id`'s agent model and in `ref-prune-id`'s record alike. The
query's A-novelty term is the counterweight on the arm; `ref-prune-id` has
none beyond the `0.5` prior.

**Registrant's note, derived and unmeasured, written before any number
exists.** The note changes no cell, criterion or label. K7-2's report §3.3
measured, with the novelty term on, that the centre's identical-mask **leave**
reads act on 1706 of 1706 (window holding a success) and 1596 of 1596 (not),
and that without the term a replayed success declines and a replayed failure
or an empty window acts. A redundant member's leave read is an identical-mask
read. If the term decides those reads on this world as it did there, the
candidate's record does not reach the leave act, `grp-id`'s sweep evicts in
arrival order, and the arm ends where `ref-prune` ends or within the bar of
it. A window holding a FAILURE did not occur on K7-2's world (lock part 2,
fact 5), and an agent model's window holds only that agent's bits, so neither
half of that inference has been measured. The registrant expects **no
`VALIDATED`**: label 3 or label 5 of §6, label 4 not excluded — on identical-mask
**join** reads K7-2 measured the term declining 270 of 1258 candidates whose
window held a success, which on an agent-keyed window is the reliable
candidate.

## 5. Registered legs and gates

### The counted criterion (owner, 2026-09-19) — `grp-id` against `ref-prune`, on PRIMARY-P

- **H-equiv — both conjuncts.** Final member set identical on ≥ **570 of 600**
  tasks **and** PRIMARY-P bit-identical on ≥ **24/30** seeds.
- **H-beats — all three.** `ref-prune`'s median PRIMARY-P > 0, ratio of
  medians `grp-id` / `ref-prune` ≥ **1.25×**, **and** `grp-id` strictly
  superior on ≥ **18/30** seeds.
- **H-below — all three.** `grp-id`'s median PRIMARY-P > 0, ratio of medians
  `ref-prune` / `grp-id` ≥ **1.25×**, **and** `ref-prune` strictly superior on
  ≥ **18/30** seeds.
- At a block of other than 30 seeds × 20 tasks the thresholds are 95 % of
  tasks, 80 % of seeds (H-equiv) and 60 % of seeds (H-beats, H-below), rounded
  up.
- At most one of the three can pass: H-equiv leaves at most 6 seeds that
  differ, and H-beats and H-below each need 18.

### Registered secondary reads (none gating; each reported in the same three-way form)

1. `grp-id` against `ref-prune-id` — is a gain the engine's or the identity
   signal's. With it: `ref-prune-id` against `ref-prune`.
2. `grp-topo` against `ref-prune` — part 1's question, as locked.
3. `grp-topo` against `grp-topo-cov` — does reading failures move the
   identity-blind arm.

### Registered disclosures (all non-gating)

- **Arms table**, per cell: median and per-seed PRIMARY-P, its success rate
  and mean efficiency, the coverage-scored PRIMARY, churn, mean final size,
  tasks ending empty.
- **Against each reference** (`ref-prune`, `ref-prune-id`, `ref-keep`,
  `ref-first`), every group cell: tasks with identical final member set, seeds
  with bit-identical PRIMARY-P.
- **Non-performance among final members**, per cell: final member-slots, those
  that did not perform, and the same over slots covering ≥ 1 demanded step.
- **After a failure** (the named risk), per cell: over agents with ≥ 1 observed
  non-performance as a final member, the share of later tasks on which they
  end as final members, beside the same share for agents with none; and the
  per-cell count of agents never a final member after their first observed
  non-performance.
- **Centre-present reads by what the centre's query sees**, per group cell and
  read kind, act rate in four classes: the centre's masks identical on its
  role's required bits × whether the last two observations of the model the
  centre queried hold a failure on a bit the role requires now. Computed in
  the example from the instance and the reconstruction — a world-side stand-in
  for the engine's replay window, not a read of it. K7-2 A3.4's integrity
  check (mirror leave-query counts equal the ledger's) gates the table's
  printing per cell, not the run.
- **Live**, `grp-id` against `grp-topo`: seeds with ≥ 1 raw-score-bit
  difference, seeds with ≥ 1 act difference, first divergence (position, task
  index, read kind, acting cell) per seed and pooled.
- **E-follow**, the votes table and candidate reach (K7-2 §5 / A3.4), every
  group cell.
- **Outcome decomposition** by realised roster 1 / 2 / 3, and task 0 against
  tasks ≥ 1, per cell, on the performance-scored fields.
- **E-lat.** Median µs/decision per cell. Record-only.
- **Declines**: `declines_upstream`, `declines_missing_role`,
  `declines_no_demand` per group cell.

### Gates (any failure ⇒ `RUN-INVALID`)

- **X-battery — conditional on the diff** (K7-2 prereg §5's rule). The frozen
  archive binary names nothing in `src/harness/` (`rg -c
  'koalisi::harness|harness::' examples/strategy_comparison.rs` matches
  nothing at `d566d58`). On the final tree `git diff --name-only v0.42.0..HEAD
  -- src Cargo.toml Cargo.lock` is read: a path under `src/` outside
  `src/harness/`, or a dependency line in `Cargo.toml` / a stanza other than
  koalisi's own in `Cargo.lock`, puts the gate on — one fresh serial archive
  run diffed against `docs/runs/K4-archive.log` with the latency column
  stripped, a latency-criterion flip re-run once. §1's planned change to
  `src/decision/group_policy.rs` puts it on.
- **X-host, X-carry, S-nov** (test suite). `tests/k7_group_host.rs` (5 tests)
  and `tests/k7_2_novelty.rs` (2 tests) pass with their pinned values and
  their source unchanged; the passed counts are recorded.
- **X-id0** (in-binary; this registration's X-identity). Per seed, the task-0
  trace entries (leave flag, act, raw score bits) of `grp-id` equal
  `grp-topo`'s, and those of `ref-prune-id` equal `ref-prune`'s. Before any
  task is observed every model is at its prior and every record is `0.5`.
- **X-recon** (in-binary). Per seed and cell, all seven: `primary` and
  `performance_scored` recomputed from the reconstructed final member sets
  (`Recon::performance_scored`) equal the `WorkflowResult`'s bitwise; every
  trace consumed exactly.
- **S-determinism.** Every group cell and `ref-prune-id` re-run from scratch
  per seed and compared on trace entries, PRIMARY-P bits, PRIMARY bits and
  churn. **Seed invariance** on `grp-id` and `grp-topo`: `battery_seed ^
  0x9E37_79B9_7F4A_7C15` reproduces each bit-for-bit.
- **S-learn (i).** Per seed and group cell: `s_learn_exact`, `models_moved`
  with its `expected == 0` exemption, `begin_task_rejections == 0` — over the
  role models.
- **S-learn (id).** Per seed on `grp-id`: every agent model's applied-update
  count equals the count recomputed in the binary from the reconstruction
  (tasks on which the agent is a final member with `required_r(own role) &
  capabilities != 0`); block total > 0.
- **S-route.** Per seed on `grp-id`, `grp-topo`, `grp-topo-cov`:
  `routed_reads` == ledger reads with `candidate_sensitive >= 1` and `roster
  >= 2`, block total > 0 per cell.
- **S-draw** (the world moved). Per seed the instance carries a performance
  draw; over the block, `ref-keep`'s final member-slots include ≥ 1 that did
  not perform.
- **S-id** (test suite; the lever reaches the engine). On a hand-built fixture
  — no generated instance — two role-mates with identical capabilities, one
  having been observed through the trait hook as performing and the other as
  not: the centre's leave read of each, under `grp-id`, differs in raw score
  bits between the two candidates; under `grp-topo` the two reads are
  bit-equal. The test pins the `(act, score bits)` values it measures, and
  pins `RefPruneId`'s acts and records on the same fixture by hand-derived
  values. The pinned values are entered here by a pre-run amendment.

## 6. Verdict labels (pre-committed, in precedence order)

1. **`RUN-INVALID`** — any gate fails, or the run exits before printing a
   `VERDICT:` line.
2. **`VALIDATED (identity-keyed group beats the prune)`** — H-beats passes.
3. **`FALSIFIED (arm ≡ prune)`** — H-equiv passes. Reads: *"told who
   performed, the routed arm still ends where a learning-free arrival-order
   prune ends."*
4. **`FALSIFIED (arm below the prune)`** — H-below passes.
5. **`FALSIFIED (no effect at the bar)`** — none of the three passes. Reported
   with the ratio, both strictly-superior counts and the identical-set task
   count in the verdict paragraph.

**Pre-committed scoped clauses, fixed before any number is visible.**

1. **Whose gain.** Label 2 is reported with secondary read 1. If
   `ref-prune-id` passes H-beats' three conjuncts against `ref-prune` and
   `grp-id` does not pass them against `ref-prune-id`, the verdict carries:
   *"an engine-free rule reading the same identity signal reaches the bar
   against the prune; the arm does not reach it against that rule."*
2. **Was the signal usable.** Labels 3, 4 and 5 are reported with
   `ref-prune-id` against `ref-prune` in the three-way form, and say only
   which of the three legs it passed, or none.
3. **Zero medians.** A ratio conjunct that fails because the divisor's median
   is 0 is reported as: *"`<cell>`'s median PRIMARY-P is 0; the ratio is
   undefined; `<other>` is strictly superior on n/30 seeds."* The predicates
   are unchanged.
4. **The premise.** E-follow is reported for every routed cell under every
   verdict; below 100 % on `grp-id` the verdict carries: *"on `grp-id` the
   group's act differed from the centre's argmax on n of N centre-present
   reads; on those reads the agent-keyed query did not decide."*
5. **§4's note** is reported as **confirmed** iff the label is 3, 4 or 5 and
   **contradicted** iff it is 2; the four-class table's identical-mask leave
   row for `grp-id` is quoted under every verdict.
6. **The named risk** is reported from the *after a failure* disclosure under
   every verdict, as the two shares and the count, with no summary word.

**Continuation.** None is pre-committed; the next lock reads this report
first.

## 7. What this cannot settle

- Every earlier registered verdict — untouched.
- Novelty on this world: no novelty-off cell is registered, and any such
  contrast inherits K7-2's mechanism (its report §3).
- Any `PerformanceSpec` but 0.7 / 0.05 / 0.40; a draw that is not independent
  per task and agent.
- Any λ but ½, any adjacency rule but the candidate-star, the `Shared`
  world-model topology (EQ5b candidate (c), block 360..390), nested groups,
  the multiplicity channel.
- Which part of `grp-id` carries a difference from `grp-topo`: the store
  changes the centre's model and what that model observes together.
- Transfer off the v2w shape. One world, one draw family, 30 seeds.

## 8. Pre-run executions

- **Already seen (lock part 2).** Scaffold 1's falsification runs (PR #101)
  printed, in test failure messages, performance-scored values of `RefPrune`
  and of an always-join policy on off-block seed 7000 under §2's values; no
  group-arm value was produced. The shipped suite hosts `RefPrune`
  (`src/harness/recon.rs`) and an always-join, never-leave fixed policy
  (`src/harness/workflow.rs`) under §2's draw on `7000..7003`
  (`rg -n '7000\.\.7003' src/harness`).
- **The contrast is not visible before the run.** `K7_3_SEEDS` accepts exactly
  two blocks, **`8000..8003`** and **`8000..8030`**, and refuses every other
  value. Under it the binary runs every cell and gate and renders every table,
  prints the header, the gate lines and `VERDICT: SMOKE (no verdict)` — or
  `VERDICT: RUN-INVALID` when a gate fails — and prints **no** table, median,
  ratio, count, per-seed value or byte count of any cell. The gate lines carry
  pass counts per cell and nothing else.
- **The registered block needs an explicit opt-in.** It runs only with
  `K7_3_OFFICIAL=1` and no `K7_3_SEEDS`; with neither, with both, with
  `K7_3_OFFICIAL` other than `1`, or with any other environment variable whose
  name starts `K7_`, the binary refuses, exit 2, before the header and before
  generating an instance. In official mode it also refuses unless
  `CARGO_PKG_VERSION` is `0.43.0`.
- **No other execution of `grp-id` or `ref-prune-id` over a generated
  instance** — by the implementer, a reviewer or a test — precedes the run.
  Tests host them on hand-built fixtures only; tests may host `grp-topo` and
  the K7-2 reference cells on consumed blocks.
- **Gate falsifications are additive** (`+ 1.0`, a forced error), never
  multiplicative, and run in a copy of the source under a smoke block.
- The three review lenses are read-only and run no cargo command, binary,
  test or probe.
- The report lists every pre-run execution of the binary: environment, tree,
  who, and what the output carried.

## 9. Order of operations

The version bump to `0.43.0` lands before the official run, so
`docs/runs/K7-3.log` names the released tree. X-host, X-carry, S-nov, S-id,
the example's own unit tests and X-battery run on that tree **before** the
official run; the binary's `VERDICT:` line cannot see them, and the run is made
only if all hold. The run is serial on a quiet machine (`pgrep -c
'cargo|rustc'` prints 0), `--release`, the binary run directly.

## 10. Report

`docs/k7/ab-report-K7-3-performance-scored-world.md`, committed with
`docs/runs/K7-3.log`, carrying the verdict line exactly as printed, the
criterion, every gate outcome, §5's tables, §6's clauses and a numbered
implementation / deviation ledger whose item 1 is §8's execution list,
the seed-7000 sighting included. Immutable once recorded.

## Amendment 1 (pre-run, 2026-09-19) — S-id's pinned values, a fixture finding, one reading of §3

No generated instance has been run under `grp-id` or `ref-prune-id`;
`examples/k7/k7_3.rs` does not exist yet. Cells, criterion, labels, the
registered gates' predicates, seeds and smoke blocks are unchanged. Commits:
prereg `f654a52`; library and tests `54fcd33`. **A1.1's and A1.2's values were
visible when A1.3 was written.**

- **A1.1 — S-id, as pinned** (`tests/k7_3_identity.rs`). Agents 0, 1, 4 = bits
  0, 1 @ r0; 2 = bit 2 @ r1; 3 = bit 0 @ r1; 5 = bit 0 @ r0. Steps `(0,r0)
  (1,r0) (2,r1)`, λ = ½, seed 11, a fresh policy per read. *Warmed* = one
  observed task through the trait hooks: agents 0..=3 each read for leave out
  of `{0,1,2,3}`, then the three required bits `true` with members `0:
  performed, 1: not, 2: performed, 3: not`. The read is the leave of the
  candidate out of `{candidate, its role-mate of 0 / 1, 2}`.

  | read | store | act | score bits |
  |---|---|---|---|
  | fresh, agent 0; fresh, agent 1 | `grp-id` | act | `0x3fe0000000000000` |
  | fresh, agent 0; fresh, agent 1 | `grp-topo` | act | `0x3fe0000000000000` |
  | warmed, agent 0 (performed) | `grp-id` | act | `0x3fdffffff9df29c8` |
  | warmed, agent 1 (did not perform) | `grp-id` | act | `0x3fdffffffb2e8100` |
  | warmed, agent 4 (never a member) | `grp-id` | act | `0x3fdffffffb2e8100` |
  | warmed, agent 0; warmed, agent 1 | `grp-topo` | act | `0x3fdffffff9df29c8` |

  The two warmed role-mates differ in score bits under `grp-id` and are
  bit-equal under `grp-topo`, so the gate's predicate holds. `RefPruneId`'s
  acts and `(p, n)` records over five hand-written tasks were derived before
  the first run and passed unmodified (the implementer's report); the sequence
  carries one exact tie, `(1, 2)` against `(0, 0)`, which evicts.
- **A1.2 — what the fixture shows beside the predicate.** The non-performer's
  warmed read is bit-equal to the read of a role-mate that was never a member;
  the separation in A1.1 is carried by the performer's success observation.
  All nine reads act. A lever that records a non-performer as no-observation
  left the score-bit pins green (the implementer's falsification F2b), so the
  score bits cannot see a dropped failure observation.
- **A1.3 — S-id's predicate, extended.** §5's predicate and A1.1's pins stand.
  Added: a test compares the warmed agent models' pA rows (success / failure /
  no-observation, per bit) against an untouched agent's model through
  `GroupAifPolicy::agent_model_snapshot`; under F2b it goes red. §4's note and
  §6's clause 5 are unchanged: A1.2 is one task and one read shape on a
  hand-built fixture, and no criterion reads it.
- **A1.4 — a reading of §3 the library makes.** `MemberOutcome` carries no
  capabilities. `required_r(own role) & capabilities` takes an agent's
  capabilities as last shown to a `should_join` / `should_leave` call, in
  `grp-id` and in `RefPruneId` alike; both are still built from the role map
  alone. §2's leave sweep reads every member, so every final member has been
  shown. A member never shown observes and counts nothing; on `grp-id` S-learn
  (id), whose expected count is computed from the instance's capabilities,
  reads that as a deficit.
- **A1.5 — executions to date** (the implementer's report). Every execution of
  `grp-id` and `RefPruneId` was `tests/k7_3_identity.rs` on A1.1's fixture and
  its five-task prune sequence: on the tree, three development runs, one
  post-lint run and two full `harness,decision,process` lanes; in a `cp -r`
  copy with its own target dir, one baseline and 20 lever runs. `rg -n
  'generate' tests/k7_3_identity.rs` matches its module-doc line alone, and
  `rg -l 'RefPruneId|AgentKeyed' src tests examples` lists
  `src/decision/group_policy.rs`, `src/harness/recon.rs`,
  `src/harness/mod.rs` and that test file.
- **A1.6 — names.** `grp-id`'s store is `WorldModelTopology::AgentKeyed`;
  S-learn (id) reads `GroupAifPolicy::agent_model_updates`. The agent models
  are outside `ModelLabel`, `model_updates` and `model_snapshots`, which the
  frozen examples match exhaustively (`rg -n 'ModelLabel::' examples`).

## Withdrawal (2026-09-21, before any run)

**`K7-3` is withdrawn and never ran.** No seed in `540..570` was executed under
any cell; the block is retired unconsumed ([`README.md`](../README.md#seed-ledger)).
Owner decision, 2026-09-21, recorded on
[#100](https://github.com/sustia-llc/koalisi/issues/100).

- **What preceded it.** The owner paused the run after the three-lens review
  (#100, 2026-09-19): `grp-id` as locked reads each candidate absolutely, so
  §2's arrival-order leave sweep decides which of two same-step coverers
  survives. `RefPruneId` was then measured against `RefPrune` off-block on §2's
  world — `v0.43.0`, PR #112, `tests/ref_prune_id.rs`, seeds `9000..9030`:
  `median performance-scored PRIMARY ref-prune-id 0.2391 (0x3fce99b563bf8f5c)
  ref-prune 0.2363 (0x3fce3f77ba0afcd9); final member sets identical on 344 of
  600 tasks; PRIMARY bit-identical on 4 of 30 seeds; ref-prune-id above on 13
  seeds, ref-prune above on 13`. That is the second half of §5's secondary
  read 1; at §5's thresholds it passes none of H-equiv (570 of 600 tasks, 24
  of 30 seeds), H-beats and H-below (18 of 30 seeds each). The owner re-posed
  the question rather than re-lock the arm on this world.
- **Where the code is.** `harness::RefPruneId` and its pins are on `main`
  from `v0.43.0`. The `AgentKeyed` store, `tests/k7_3_identity.rs` and
  `examples/k7/k7_3.rs` are not; they are preserved at tag `k7-3-withdrawn`
  (commit `51678a3`).
- **Pre-run executions**, the list §10's report would have carried: the
  seed-7000 sighting (§8); Amendment 1's A1.5; the 25 executions of the
  binary, including the gate-falsification leak on smoke block `8000..8003`
  (#100, 2026-09-19); the R0 executions (PR #112's body).
- **What it leaves.** No verdict and no §6 label. §4's note is neither
  confirmed nor contradicted. The re-posed question takes the next free
  number at its own lock.
