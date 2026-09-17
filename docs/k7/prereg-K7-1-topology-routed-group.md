# Pre-registration: K7-1 — topology-routed group voting (tira ext-6) vs the candidate-blind group

**Status: REGISTERED.** Committed BEFORE implementation. Design-lock of record:
[koalisi #90](https://github.com/sustia-llc/koalisi/issues/90) — the owner lock
of 2026-09-16, its amendment of the same day (versions, scaffold, in-battery
bar, continuation) and the `aif-v0.14.0` note. Two design calls the lock left to
this document were taken by the owner on 2026-09-17 before it was written: the
adjacency rule (§4) and the bar (§5). Amendments are pre-verdict only, appended
here and posted on #90 before any affected code runs; after the official run
this document is immutable ([`PROTOCOL.md`](../PROTOCOL.md) §1).

## 1. Registration statement

- **Question (EQ5b candidate (a)).** EQ5b
  ([report](../ab-report-K4-eq5b-typed-two-engine.md) §3.2) measured that under
  role-matched coverage masks only the candidate's own role internal is
  candidate-sensitive, and that the other internals vote ACT at maximum
  `CertaintyWeighted` weight. K7-1 asks: **with the candidate-sensitive
  internal's output routed to every voter over an `aif::Topology`, masks
  unchanged, (i) is every voter candidate-sensitive, and (ii) does the routed
  arm beat the in-battery `grp-role` control?**
- **Born on:** koalisi `v0.37.0` (merge `7f6cf10`): `aif-v0.14.0`, catgraph
  `v0.23.0` ×3, `rust-version` 1.93. The pin landed in its own PR (#95) with
  one serial archive run reproducing all 33 verdict lines.
- **Seeds:** `90..120`, fresh (ledger: reserved — K7-1).
- **Placement:** `examples/k7/k7_1.rs`, its own `[[example]]`
  (`required-features = ["harness", "decision", "process"]`); every library
  change through `src/`. `examples/strategy_comparison.rs` is not edited.
- **Flat.** One `GroupAgent`, no nested member; tira #51 does not fire.
- **Standing constraint.** koa#54 (magnitude = demonstrated default) is FINAL;
  no K7-1 outcome reopens it.
- **K4-lineage numbers are motivating evidence, never the bar.** Every number
  quoted from EQ5b below is a Part 11 median on seeds 330..360 at `v0.29.0`
  pins. The bar is stated against arms run in this battery, on these seeds, at
  this pin.

## 2. World and outcome semantics

- **World:** `WorkflowSpec::default()` from `src/harness/workflow.rs` — the v2w
  draw (8 universe bits, pool `4..=16`, capabilities `1..=4`, 20 tasks,
  `required_bits` `2..=8`, 3 roles, re-draw cap 1000, fan-out denominator 4, no
  performance draw). Its identity with the frozen binary's generator is the H0
  gate of `v0.35.0` (`tests/harness_workflow.rs`).
- **Loop and score:** `run_workflow_battery` / `run_workflow_instance` as
  shipped — first arrival joins unconditionally, later arrivals join iff
  `should_join` acts, one leave sweep in arrival order, churn = removals;
  PRIMARY = success rate × mean coverage efficiency over the declared distinct
  `(bit, role)` steps, a step covered iff a final member **of its role** holds
  its bit.
- **Outcome semantics:** `OutcomeSignal::RoleCoverage` (Part 11's): bit `b` is
  `true` iff some demanded step on `b` is covered by a final member of that
  step's role.
- **Lifecycle:** the H1 hooks. `GroupAifPolicy` gains the two trait hooks:
  `begin_task(&TaskStart)` rebuilds the task's `Demand` with
  `Demand::from_steps` and calls the arm's own `begin_task`;
  `observe_outcome(required, per_bit)` forwards `per_bit` to the arm's own
  `observe_outcome`.
- **What the hook does not carry.** `TaskStart::steps` is the demand's
  **distinct** steps, so the rebuilt `Demand` has multiplicity 1 everywhere.
  The `MultiplicityWeighted` channel is therefore not hostable through H1 as
  defined, and no `grp-mult` cell is registered here.

## 3. Arms

Every group cell is `GroupAifPolicy` over `v5_e1_base()`,
`WorldModelTopology::RoleSpecialised`, `PrecisionChannel::RoleRestricted`,
`DecisionRead::Deterministic`, `GroupVote::CertaintyWeighted`, `n_roles = 3`,
one fresh policy per seed built from `WorkflowInstance::role_map`.

| arm | configuration | role |
|---|---|---|
| `grp-role` | `CoverageMasks::RoleMatched`, routing off | **control** |
| `grp-role-blind` | `CoverageMasks::RoleBlind`, routing off | **second control** (scoped clause, §6) |
| `grp-topo` | `RoleMatched`, candidate-star routing, λ = ½ | **confirmatory** |
| `grp-topo-id` | `RoleMatched`, candidate-star routing, λ = 0 | X-identity cell (gate, §5) |
| `grp-topo-q` | as `grp-topo`, λ = ¼ | registered exploratory, non-gating |
| `grp-topo-solo` | as `grp-topo`, λ = 1 | registered exploratory, non-gating |
| `wf-asis` | typed magnitude (`with_role_modulation`, `ρ = δ`), as built by `tests/harness_workflow.rs` | context, non-gating |

One confirmatory cell. All cells report regardless of outcome.

## 4. The routed arm

**Engine shape.** As `grp-role` in every respect but the active slot: the
`VotingAgent` is wrapped in `aif::RoutedAggregator` (`with_seed`, `n_actions =
2`) over an `aif::Topology` built **per decision**, because the roster (the
demanding roles, size 1–3) is per task and the candidate is per decision.
Members are indexed by roster position. The read stays
`GroupAgent::group_distribution`; no commit, no recording read; SP3 unchanged
(act iff `p(act) > 0.5`, ties decline).

**The adjacency rule (owner, 2026-09-17) — candidate-star.** Let the *centre*
be the roster position of the candidate's role, where the candidate's role is
read from the role map (the same coalition view `coverage_masks` reads).

- Centre on the roster: the centre's row is the identity row; every other
  member `i` has row weight `λ` on the centre and `1 − λ` on itself.
- Centre off the roster (the candidate's role has no demand on this task):
  every row is the identity row.
- `hops = 1`; readout = every member, in roster order.

The rule introduces one number, λ. The confirmatory value is **λ = ½** — the
uniform weight over `{self, centre}`. λ = 0 is the identity configuration
(`grp-topo-id`); λ = 1 makes every readout row the centre's output
(`grp-topo-solo`).

**Library surface.** `GroupAifConfig` gains a routing field whose default is
*off*, and *off* is the pre-existing `VotingAgent` construction verbatim. Every
`GroupAifConfig` literal in the frozen binary carries a struct-update base
(`rg -n -A8 'GroupAifConfig \{' examples/strategy_comparison.rs` → ten
literals, each ending `..GroupAifConfig::default()` or `..base`), so the field
compiles there unchanged. A λ outside `[0, 1]` or non-finite is a construction
error.

**Derived arithmetic (not measured; pinned by unit tests before the run).**
The CW mixture is `Σ wᵢ Pᵢ / Σ wᵢ` with `wᵢ = exp(−H(Pᵢ))`, computed on the
**routed** rows (`aif-v0.14.0` `group.rs:391`, `topology.rs:406`). With every
member output a delta, `k` blind members at `[0, 1]` and the centre at
`[1, 0]`:

- identity: `p(act) = k / (k + 1)` — a tie (decline) at `k = 1`, **act** at
  `k = 2`;
- candidate-star: `p(act) = k·w·(1 − λ) / (k·w + 1)` with `w = exp(−H(λ))`,
  which declines iff `w·(1 − 2λ) ≤ 1/k`. At `k = 2` the flip sits between
  λ = 0.130 (`0.5028 > ½`, act) and λ = 0.135 (`0.4914`, decline).

So at saturated outputs λ = ¼, ½ and 1 are predicted to produce the **same
acts** — the group follows the centre whenever one is on the roster — and to
differ in score bits. §5's E-λ counts both.

**Expectation, stated before any number exists.** tira's ext-6 study measured
CW routing as the exact identity for identical fixed-`A` members receiving the
identical observation; koalisi's role internals are distinct queries over
distinct masks and sit outside that case. The arithmetic above predicts the
routing is decision-live here. Whether it helps is unknown: EQ5b §3.2 found the
blind voters load-bearing, but measured that by changing the masks
(`grp-role-blind`), which K7-1 holds fixed. The registrant's expectation is
that `grp-topo` does **not** clear 1.25× over `grp-role`.

## 5. Registered legs and gates

### Confirmatory

- **H-T.** On seeds 90..120, `grp-topo` against in-battery `grp-role`: PRIMARY
  median ≥ **1.25×** the control's **AND** strictly superior on ≥ **18/30**
  seeds. Both conjuncts. (Owner, 2026-09-17: the lineage's standing bar for
  ≤ 2 confirmatory cells.)

### Structural answer to part (i) — counted, not a bar

- **H-S.** A readout row is *candidate-sensitive* iff its effective weight on
  a candidate-sensitive member is positive. Reported per cell: the share of
  successful reads in which **every** readout row is candidate-sensitive; the
  share with **none** (the centre-absent reads); and the blind-origin mass
  share `Σᵢ wᵢ·(Σ_{j blind} Wᵢⱼ) / Σᵢ wᵢ`, which at the identity equals
  EQ5b's blind CW weight share.
- **Pre-committed reading.** Routing is linear in the member outputs, and on a
  centre-absent read no member output depends on the candidate, so **no
  topology makes that read candidate-sensitive**. Part (i)'s answer is
  therefore fixed in form before the run: *"every voter is candidate-sensitive
  on the centre-present reads and on no centre-absent read; the centre-absent
  share is X %"*. (EQ5b's zero-candidate-sensitive share was 23.0 % —
  motivating only.)

### Registered exploratory / disclosure (all non-gating)

- **S-live (gotcha 31).** `grp-topo` vs `grp-role`, per seed: decisions
  differing by ACT, decisions differing by raw score bits, seeds with any act
  difference. Sequences are compared by position up to the shorter length, and
  the length difference is reported.
- **E-λ.** `grp-topo-q` and `grp-topo-solo` against `grp-topo`: median
  PRIMARY, act divergence, score-bit divergence. The §4 prediction is *zero or
  near-zero act divergence, non-zero score-bit divergence*; an act divergence
  is a count of non-saturated reads.
- **E-follow.** On centre-present reads of `grp-topo`: the rate at which the
  group's act equals the centre's own argmax.
- **Candidate reach** (EQ5b's A5.1 table, per group cell): zero
  candidate-sensitive share, mean blind internals, blind CW weight share (by
  member, and by origin per H-S), leave `cfg0 == cfg1`, act rate sensitive /
  blind. **E-agree**: roster 1 / 2 / 3, empty role-slots, unanimous reads,
  margin quartiles.
- **E-lat.** Median µs/decision per arm. Record-only.
- **Context.** `wf-asis` median PRIMARY, superior-seed counts against it.

### Gates (any failure ⇒ `RUN-INVALID`)

- **X-battery.** `src/decision/group_policy.rs` is on the frozen binary's
  path. One fresh serial archive run on the final tree, diffed against
  `docs/runs/K4-archive.log` with the latency column stripped
  ([`runs/README.md`](../runs/README.md)); checked outside the K7-1 binary. A
  latency-criterion flip is re-run once (gotcha 34) and recorded.
- **X-identity.** `grp-topo-id` reproduces `grp-role` bit-for-bit — acts, raw
  score bits, PRIMARY bits, churn — on 30/30 seeds. λ = 0 runs the
  `RoutedAggregator` path, so this gates the wrapper, the per-decision
  topology build and the config plumbing together.
- **X-host** (test suite, not the run). The harness-hosted `grp-role` and
  `grp-role-blind` over the **consumed** block 330..360 — read as a control,
  like H0's — reproduce `docs/runs/K4-archive.log:2032` and `:2036` (median
  PRIMARY 0.2270 / 0.0268, median churn 173.00 / 185.50) and the `grp-role`
  candidate-reach row `:2071` (23.0 %, 1.35, 0.626, 9671 of 13107, 80.4 % /
  95.1 %).
- **S-determinism.** Every group cell re-run from scratch per seed and
  compared on acts, raw score bits, PRIMARY bits and churn, 30/30. **Seed
  invariance** on `grp-topo`: a different `battery_seed` reproduces it
  bit-for-bit (the deterministic read draws nothing; `RoutedAggregator`'s RNG
  is reached only by `aggregate`, which this arm never calls).
- **S-learn (i).** Per seed and per group cell: the counted ledger is exact
  (`GroupAifCounters::s_learn_exact`), `models_moved` holds with its
  `expected == 0` exemption, exempt models disclosed per seed, and
  `begin_task` rejections are 0.
- **S-route (the lever is exercised and counted).** Per seed, on `grp-topo`,
  `grp-topo-q` and `grp-topo-solo`: reads routed through a non-identity
  topology == successful reads with a candidate-sensitive member and roster
  ≥ 2, the right side derived from the `AgreementSample` ledger and the left
  from a counter written where the topology is built. Deficit and surplus both
  fail. Block total > 0. On `grp-topo-id` and the routing-off cells the
  non-identity count is 0.

**Standing disclosure.** A zero score is not a decline; declines are read from
the counters (`declines_upstream`, `declines_missing_role`,
`declines_no_demand`), reported per cell.

## 6. Verdict labels (pre-committed)

- **`VALIDATED (topology-routed group)`** — H-T passes both conjuncts, all
  gates hold.
- **`FALSIFIED (topology-routed group)`** — gates hold, H-T fails either
  conjunct.
- **`RUN-INVALID`** — any gate fails.

**Pre-committed scoped clauses, fixed before any number is visible.** The bar
is computed against in-battery `grp-role` and only `grp-role`.

1. **Against `grp-role-blind`.** Whatever the verdict, the report states
   whether `grp-topo`'s median exceeds `grp-role-blind`'s. `grp-topo` above
   `grp-role-blind` and below `grp-role` reads *"candidate-sensitivity bought
   by routing costs less than candidate-sensitivity bought by role-blind
   masks, and still costs"*.
2. **A FALSIFIED with `grp-topo`'s median below `grp-role`'s** reads *"the
   blind voters are load-bearing with the masks held fixed"* — EQ5b §3.2
   without its mask confound. A FALSIFIED with the ratio in `[1.0, 1.25)`
   reads *"not worse, not validated"*.
3. **Against `wf-asis`.** A PASS whose median does not exceed in-battery
   `wf-asis` is reported as *"beats the blind-voter group, not the typed
   magnitude control"*.
4. **H-S's sentence** (§5) is reported verbatim with its measured share
   under every verdict.

A `VALIDATED` here would mean *"on this world, routing the candidate-sensitive
internal's output to every voter improves the role-slotted group at the
registered bar"* — not that a group deliberates about a candidate on the
centre-absent reads, and nothing about koa#54.

## 7. What this cannot settle

- EQ5b's verdict and every other K4-lineage verdict — untouched.
- The centre-absent reads: no routing reaches them (§5 H-S).
- Any adjacency rule but the candidate-star, and any λ as a tuned quantity —
  ¼ and 1 are disclosures, not a sweep to pick from.
- `Deterministic` / `Probabilistic` active slots, `hops > 1`, partial
  readouts, the `Shared` world-model topology (K7-3's axis), novelty (K7-2's
  axis), the multiplicity channel (§2).
- Transfer off the v2w world. One world, one draw family, 30 seeds.

## 8. Report

`docs/k7/ab-report-K7-1-topology-routed-group.md`, committed with
`docs/runs/K7-1.log`, carrying the verdict line exactly as printed, the bar,
every gate outcome, the mechanism tables of §5 and a numbered
implementation / deviation ledger. Immutable once recorded.

## Amendment 1 (pre-run, 2026-09-17) — which quantity §4's two flip numbers are

No battery has run. §4 stands as registered; this names a quantity it left
ambiguous.

- **A1.1.** In §4's candidate-star bullet, `0.5028` (λ = 0.130) and `0.4914`
  (λ = 0.135) are values of **`w·(1 − 2λ)`**, the left side of the decline
  condition `w·(1 − 2λ) ≤ 1/k` at `k = 2` — not values of `p(act)`. The
  corresponding `p(act)` values, pinned by the unit test
  `candidate_star_arithmetic_on_delta_outputs` in
  `src/decision/group_policy.rs` against the closed form to `1e-12`, are
  **0.5012** at λ = 0.130 (act) and **0.4963** at λ = 0.135 (decline). The
  flip sits between the two λ under either quantity; no prediction, cell, bar
  or gate changes.
- **A1.2 (disclosure).** With routing on, the H-S disclosure is computed
  through `aif::Topology::route` after the read; an error there is a decline
  counted in `declines_upstream`. No test reaches that path
  (`rg -n 'agreement sample routing failed' src/` names its one site); the
  per-cell `declines_upstream` count §5 already reports is where it would
  show.

## Amendment 2 (pre-run, 2026-09-17) — the three-lens review

No seed in 90..120 has run. The arm, λ, the bar, the seeds, the world and the
gates' predicates are unchanged. **Everything in this amendment was written
after off-block numbers for the confirmatory arm were visible (A2.1), and is
labelled so.** Owner decisions of 2026-09-17: register `ref-prune` (A2.2); add
the three readings of A2.4.

### A2.1 — pre-run executions of the confirmatory arm (disclosure)

Commit times: prereg `1948439` 11:32:34, routing code `c13a821` 12:03:26,
Amendment 1 `b342478` 12:03:47, the binary `0f1145e` 12:17:27. The arm, λ,
bar and seeds were committed before the first execution below. Every
execution used `K7_1_SEEDS` (no verdict line); `grp-topo` vs in-battery
`grp-role`, median PRIMARY:

| seeds | by | `grp-role` | `grp-topo` | ratio | superior |
|---|---|---:|---:|---:|---:|
| 330..333 (a **consumed** block) | implementation smoke, 12:10–12:16, plus six falsification runs on a copy | 0.2092 | 0.4225 | 2.0199× | 3/3 |
| 1000..1006 | review lens 1 | 0.1995 | 0.4571 | 2.2911× | 5/6 |
| 2000..2006 | review lens 2 | 0.2387 | 0.4508 | 1.8885× | 4/6 |
| 3000..3030 | review lens 3 (and a scratch probe on 3000..3060, outside the repository) | 0.1876 | 0.4392 | 2.3416× | 30/30 |

§4's registered expectation ("does **not** clear 1.25×") is therefore known
to disagree with every off-block execution before the run; it stands as
written. The official run is a replication, on a fresh block, of an effect
already seen on the same generator. The 330..333 smoke ran the confirmatory
arm on a consumed block, which §5's X-host carve-out (controls only) does not
cover. The binary's `K7_1_SEEDS` guard now refuses every block of the seed
ledger and every block assigned to a later K7 registration (`0..480`).

### A2.2 — `ref-prune`, a registered reference cell (non-gating; owner)

Review lens 3 measured off-block (3000..3030, 3030..3060) that `grp-topo`'s
final member sets equal those of an engine-free rule on 600 of 600 and 599 of
600 tasks, PRIMARY bit-identical on 30/30 and 29/30 seeds. The rule is
registered as an eighth cell:

- **`ref-prune`** — `should_join` always acts; `should_leave` acts iff every
  distinct demanded `(bit, role)` step covered by the current membership stays
  covered without the agent (coverage as the world scores it: a member of the
  step's role holding the bit). No engine, no learning, no outcome signal
  read. Built in the example from the instance's role map and the `TaskStart`
  steps.
- **Disclosure, every group cell against `ref-prune`:** tasks whose final
  member set is identical (of N), seeds with bit-identical PRIMARY (of 30).
- **Pre-committed reading.** If `grp-topo` matches `ref-prune` on ≥ 95 % of
  tasks, every verdict is reported with: *"`grp-topo`'s PRIMARY is the PRIMARY
  of a learning-free arrival-order redundancy prune; S-learn (i) certifies that
  the models learn, not that learning changes an outcome."*

Final member sets are reconstructed in the example from the instance's arrival
orders and the decision trace, and the reconstruction is asserted against the
harness: PRIMARY recomputed from it equals `WorkflowResult::primary` bitwise
on every cell and seed, else `RUN-INVALID`.

### A2.3 — outcome decomposition (disclosures, non-gating)

Per cell, and split by realised roster 1 / 2 / 3: tasks, success rate, mean
covered fraction, mean coverage efficiency, mean final coalition size, tasks
ending with an empty coalition. Per group cell: join-act and leave-act rates
split centre-present / centre-absent; the table *(read kind, roster, centre
vote, blind act votes) → group acts of n*; final members whose role has no
demand on the task; churn split centre-present / centre-absent.
`AgreementSample` gains the read kind and the group's act, so these and
E-follow are read from the ledger with no positional alignment. **E-follow is
printed for every group cell**, split join / leave, over all centre-present
reads — the unregistered seed filter of the first binary is removed.

### A2.4 — readings §6 did not cover (owner; written post-smoke)

5. **The mirror of clause 2.** `grp-topo`'s median at or above `grp-role`'s
   reads *"with the masks held fixed the candidate-blind voters are not
   load-bearing; EQ5b §3.2's reading was carried by its mask change"*. The
   EQ5b report is immutable; the sentence is the K7-1 report's.
6. **Who decides.** If `grp-topo-solo` differs from `grp-topo` on 0 acts,
   every verdict is reported with: *"on centre-present reads the routed
   group's act is the act of the candidate's own role query alone; the other
   voters decided no read; the result concerns who decides, not the
   aggregation of more than one candidate-informed opinion, and shows no group
   deliberating about a candidate."* Otherwise the reads where the group's act
   differs from the centre's argmax are counted per routed cell (E-follow).
7. **Ratio without superiority.** H-T failing conjunct 2 with the ratio at or
   above 1.25 reads *"ratio cleared, superiority did not"*; the verdict is
   `FALSIFIED`.

§6's *"A VALIDATED here would mean…"* sentence is read together with clause 6.

### A2.5 — H-T at a zero control median

Conjunct 1 is evaluated as `control median > 0` **and** `grp-topo median /
control median ≥ 1.25`; at a control median of exactly 0 the ratio prints
`n/a` and conjunct 1 fails. This is EQ5b's H-G form
(`examples/strategy_comparison.rs:11389–11391`).

### A2.6 — corrections to §5's text

- **E-λ / S-live.** *"an act divergence is a count of non-saturated reads"* is
  withdrawn. Traces are compared by position; after a seed's first divergent
  act the membership and the learned state differ, so later positions compare
  different decisions. The positional counts are an upper bound. The
  uncontaminated readings are *seeds with any act difference* and E-follow.
  S-live's three counts are printed **per seed** as well as pooled.
- **H-S's X.** X is the share of `grp-topo`'s successful reads with
  `sensitive_rows == 0`; the binary prints beside it the count of those reads
  whose centre is off the roster, and every cell's share in the H-S table.
- **Context.** Superior-seed counts against `wf-asis` are printed for all six
  group cells.
- **Constants not named in §5.** Seed invariance XORs `battery_seed` with
  `0x9E37_79B9_7F4A_7C15`. S-learn (i)'s non-vacuity tolerance is
  `S_LEARN_VACUITY_TOL = 1e-9` (`src/decision/group_policy.rs`), EQ5b's.

### A2.7 — mandatory report sentences

- *"`grp-role-blind` differs from `grp-role` in the masks, not the voters; its
  distance from `grp-topo` does not decompose candidate-sensitivity from
  coverage semantics."* Clause 1 stays as registered.
- The blind-origin mass share is mixture mass, not decision influence; it is
  reported beside E-follow for the same cell.
- S-route certifies that a non-identity topology was built and counted, not
  that an output moved; decision liveness is S-live and E-follow.
- Any churn comparison is quoted with the centre-absent churn of both arms.

### A2.8 — order of operations, aborts, §7

- The version bump to `0.38.0` lands before the official run, so
  `docs/runs/K7-1.log` names the released tree. X-host and X-battery run on
  that tree **before** the official run; the binary's `VERDICT:` line cannot
  see either, and the run is made only if both hold.
- A run that exits before printing a `VERDICT:` line is `RUN-INVALID`.
- §7 gains: whether any advantage survives an outcome signal that is not a
  deterministic function of coverage (`OutcomeSignal::Performance` / `Both`
  exist in the harness and are unused here).
