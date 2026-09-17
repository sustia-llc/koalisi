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
