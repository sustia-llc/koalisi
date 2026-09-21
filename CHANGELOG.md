# Changelog

All notable changes to **koalisi** will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

Planned work — issue-tracked (details in [`CLAUDE.md`](./CLAUDE.md)
§"Next steps"):
- **Phase 5 — SwarmAgentic meta-layer remainder** ([#20]): configurator,
  velocity-rewrite loop, transferability (needs an LLM backend behind
  `src/llm/mod.rs`).
- **Phase 7 — Persistence implementation**, in this order: [#31] sealing +
  revocation registry plus #32's belief leg → [#33] federation manifests +
  FAIR provenance. #31 does not wait on the tauhokohoko KEK-granularity
  answer: without a reply at its kickoff the granularity is ruled in-house
  and recorded on #31 before any belief-sealing code.
- **K7 lineage**: EQ5b (c) ([#103]); the multi-coalition harness scaffold and
  the tira-extension re-pins and registrations ([#104]–[#111]).

## [0.44.0] — 2026-09-21

Phase 7's `Decisions` stream, minus the belief leg ([#32]): each persisted
decision carries one causal parent into the `Topology` stream.

### Added
- **`DecisionTrace`** and **`CoalitionService::spawn_with_trace_tap`**
  (`subsystems::coalition_actor`, always compiled): the trace is the
  `DecisionRecord` plus `event_log_len` and `timestamp`, the manager's
  in-memory event-log length and logical clock, read by the service task
  before it calls the manager. Emitted for every join or leave the manager
  answers with a `Decision`, including a leave of a non-member, which the
  manager answers with `act == false` without consulting the policy; a
  manager error emits nothing. Non-blocking `try_send`, drop-with-warn.
  `DecisionRecord`, `spawn` and `spawn_with_tap` keep their signatures.
- **`persistence::WireDecision`** and `WIRE_DECISION_SCHEMA_VERSION = 1`: the
  serde mirror of `DecisionRecord` (`agent_id` as `u64`, `kind` as `"join"` /
  `"leave"`), with inherent `from_record` / `try_into_record`; an unknown
  kind is a `WireConversionError`.
- **`persistence::spawn_decision_store_forwarder`** (feature `persistence`):
  writes each trace to the `Decisions` stream as a CBOR `WireDecision` with
  the trace's `timestamp` and one parent, `Topology` sequence number
  `event_log_len − 1` (no parent when the length is 0). It forwards with
  `send().await`; the store writer still makes one `append` attempt per
  record, so delivery from the tap onward is at-most-once. The parent is the
  event-log entry at that index only when the `Topology` tap was installed on
  an empty log and dropped nothing.
- **`subsystems::durable::spawn_decision_log_bus_tee`** (features
  `persistence` + `durable`): per trace, the `Decisions` record first, then
  the `DecisionEvent` on the durable bus.
- **`persistence::WireLineage`**: an uninhabited enum reserved for [#20]; no
  producer.
- **`tests/decision_stream.rs`** (feature `persistence`, 4 tests):
  `decision_records_carry_the_decision_time_topology_parent` (hand-derived
  parents 4, 5, 5, 5, 6, 6 on a six-decision fixture; each `act == true`
  join's own event is the `Topology` record after its parent; both streams
  verify), `full_writer_channel_drops_no_decision` (writer channel capacity
  1, 32 traces, 32 records), `wire_decision_round_trips_both_kinds_and_rejects_unknown`,
  `record_parent_is_absent_on_an_empty_log`. **`tests/decision_log_bus_tee.rs`**
  (features `persistence,durable`, container-backed):
  `every_trace_reaches_the_decisions_stream_and_the_bus`. One unit test in
  `coalition_actor`: `trace_tap_reads_the_position_before_the_manager_call`.

### Changed
- `WireConversionError`'s `Display` reads `wire conversion failed: …` (was
  `wire topology conversion failed: …`).
- Rustdoc: the decision tap no longer calls its records "policy-consulted"
  (a non-member leave is tapped without a consult); the store writer's
  delivery section names the forwarders' `send().await`.
- **`K7-3` withdrawn before its run** ([#100], PR #113): its
  pre-registration is in `docs/k7/` with an appended Withdrawal section,
  seeds 540..570 are retired unconsumed, and `docs/PROTOCOL.md` §1 gains the
  withdrawal rule (item 9). The branch's code is preserved at tag
  `k7-3-withdrawn`.

### Gates
- **Falsified in a copy**: the length read moved after the manager call
  turns `decision_records_carry_the_decision_time_topology_parent` red
  (parent `SequenceNo(5)`, the join's own event, for expected 4) and the unit
  test red; `send().await` → `try_send` turns
  `full_writer_channel_drops_no_decision` red (32 traces, 1 record); swapped
  kind labels turn the round-trip test red; dropping the bus send turns
  `every_trace_reaches_the_decisions_stream_and_the_bus` red (0 of 3).
- `cargo nextest run`: default 104, `persistence` 128,
  `persistence,magnitude` 158, `durable` 105, `persistence,durable` 130,
  0 failed; both container tests booted SurrealDB (no SKIP line).
  `cargo test --doc --features persistence,durable`: 3 passed, 2 ignored.
  `cargo clippy --all-targets -- -D warnings` on `--no-default-features`,
  default, `persistence`, `durable`, `persistence,durable`;
  `RUSTDOCFLAGS='-D warnings' cargo doc --no-deps` on `persistence` and
  `persistence,durable`; `cargo fmt --check`; `cargo +1.93.0 check
  --all-targets --locked --features persistence,durable`.
- `cargo test` on every other lane, each one test above v0.43.0 (the
  always-compiled trace-tap unit test): `decision` 163, `magnitude` 136,
  `decision,magnitude` 192, `magnitude-fast` 144, `remote` 113, `process`
  162, `decision,magnitude,process` 250, `harness` 132, `harness,process`
  235, `harness,decision,process` 330, `metrics` 107.
- **K4 archive gate** (the diff reaches the decision path): one serial
  release run of `examples/strategy_comparison.rs` on a quiet machine
  (`pgrep -c 'cargo|rustc'` 0), diffed against `docs/runs/K4-archive.log`
  per `docs/runs/README.md`. The 33 `VERDICT|FALSIFIED|VALIDATED` lines are
  byte-identical; after the latency-column strip, 10 lines differ, every
  one a latency value (three `latency µs` table rows, seven lines of
  latency prose). PASS.

## [0.43.0] — 2026-09-21

Phase R0 of the refocus plan: the engine-free identity prune measured against
the redundancy prune on the performance-scored world, as a test. No
registration, no `VERDICT:` line, no seed block consumed; `K7-3` stays paused
([#100](https://github.com/sustia-llc/koalisi/issues/100)).

### Added
- **`harness::RefPruneId`** (features `harness` + `process`): the engine-free
  identity prune. Per agent it keeps `(p, n)` over the tasks on which the
  agent is a final member holding a demanded bit of its role, `p` counting
  those it performed; its record is `(p + 1) / (n + 2)`, compared as integer
  cross-products. `should_join` acts on every call; `should_leave` acts iff
  `RefPrune`'s would and every demanded step the agent covers has another
  shown member covering it with a record at least the agent's. Every score is
  `0.0`. Byte-identical to `src/harness/{mod,recon}.rs` at `54fcd33` on branch
  `k7-3-performance-scored-world`.
- **`tests/ref_prune_id.rs`** (features `harness,process`, 3 tests): the two
  hand-derived `RefPruneId` pins from that commit's `tests/k7_3_identity.rs`,
  and `r0_ref_prune_id_against_ref_prune_off_block` — over seeds
  `9000..9030` of `WorkflowSpec::default()` with the performance draw
  `0.7 / 0.05 / 0.40`, one fresh `RefPruneId` and one fresh `RefPrune` per
  seed under `OutcomeSignal::Both`. It asserts `RefPruneId`'s median
  `performance_scored.primary` is above `RefPrune`'s and prints:
  `median performance-scored PRIMARY ref-prune-id 0.2391 (0x3fce99b563bf8f5c)
  ref-prune 0.2363 (0x3fce3f77ba0afcd9); final member sets identical on 344 of
  600 tasks; PRIMARY bit-identical on 4 of 30 seeds; ref-prune-id above on 13
  seeds, ref-prune above on 13`.

### Gates
- **Falsified in a copy**: `RefPruneId::should_leave` acting on `RefPrune`'s
  predicate alone turns the R0 test red (both medians `0x3fce3f77ba0afcd9`,
  600 of 600 tasks, 30 of 30 seeds) and the hand-derivation pin red at task
  3; the record pin, which calls no leave, stays green.
- `cargo nextest run`: `harness,process` 231, `harness,decision,process` 326,
  0 failed; `cargo test --doc` 3 passed on each. `cargo clippy --all-targets
  -- -D warnings` on `harness`, `harness,process` and
  `harness,decision,process`; `RUSTDOCFLAGS='-D warnings' cargo doc --no-deps
  --features harness,decision,process`; `cargo fmt --check`; `cargo +1.93.0
  check --all-targets --locked` on `harness,process` and
  `harness,decision,process`.
- **X-battery not run**: `git diff --name-only v0.42.0..HEAD -- src Cargo.toml
  Cargo.lock` names `src/harness/mod.rs`, `src/harness/recon.rs`,
  `Cargo.toml` (the version and one `[[test]]` stanza) and `Cargo.lock`
  (koalisi's own stanza); `rg -c 'koalisi::harness|harness::'
  examples/strategy_comparison.rs` matches nothing.

## [0.42.0] — 2026-09-19

The second harness scaffold for `K7-3`
([#100](https://github.com/sustia-llc/koalisi/issues/100), design-lock part
2): the outcome hook names the task's final members. No registration, no run.

### Changed
- **`CoalitionDecisionPolicy::observe_outcome`** takes a third argument,
  `members: &[MemberOutcome]` — one entry per final member of the task, in
  membership order. **Breaking** for implementors and callers of the trait
  hook; the default still does nothing. The inherent
  `PersistentAifArm::observe_outcome` and `GroupAifPolicy::observe_outcome`
  are unchanged.
- `run_workflow_instance` (features `harness` + `process`): a member's
  `performed` is its entry in the task's performance row — a task without a
  row, and an agent index outside a row, did not perform; every member
  performed when the instance carries no draw. `run_instance` reports every
  member performed. `TracedPolicy` forwards the argument; `GroupAifPolicy`'s
  hook ignores it.

### Added
- **`decision::MemberOutcome`** (`agent_id`, `performed`), re-exported at the
  crate root.

### Gates
- **Identity.** `cargo nextest run`, base `dcb9daf` → scaffold tree:
  `harness,process` 223 → 228, `harness,decision,process` 318 → 323, 0 failed
  on either side; the sorted PASS names differ by the five added tests alone.
  `tests/harness_workflow.rs` and `tests/k7_2_novelty.rs` change by the hook
  signature; `git diff --name-only v0.41.0..HEAD -- examples` prints nothing.
  `cargo test --features harness,decision,process --example k7_2` 7 passed.
- **X-battery PASS**, on by K7-2's prereg §5 condition (the diff names
  `src/decision/mod.rs`, `src/decision/group_policy.rs` and `src/lib.rs`).
  One serial release run of `strategy_comparison` on the 0.42.0 tree against
  `docs/runs/K4-archive.log`: 2129 lines each; with the latency column
  stripped 11 line pairs differ, each a latency or wall-clock figure; the 33
  `VERDICT|FALSIFIED|VALIDATED` lines are byte-identical (`cmp`).
- Suites by `cargo test`: `harness` 131, `harness,process` 231,
  `harness,decision,process` 326. `cargo nextest run` on default, `decision`
  and `decision,magnitude,process`: 103, 159, 246 passed. `cargo clippy
  --all-targets -- -D warnings` on those six lanes and
  `--no-default-features`; `RUSTDOCFLAGS='-D warnings' cargo doc --no-deps
  --features harness,decision,process`; `cargo fmt --all -- --check`; `cargo
  +1.93.0 check --all-targets --locked --features harness,decision,process`.

## [0.41.0] — 2026-09-18

The harness scaffold for `K7-3`
([#100](https://github.com/sustia-llc/koalisi/issues/100), design-lock part
1): a performance-scored outcome beside the coverage-scored one. No
registration, no run.

### Added
- **`harness::PerformanceScored`** (`success_rate`, `mean_cov_eff`, `primary`)
  and **`WorkflowResult::performance_scored: Option<PerformanceScored>`**
  (features `harness` + `process`). `run_workflow_instance` fills it iff the
  instance carries a performance draw, under any `OutcomeSignal`: a distinct
  demanded `(bit, role)` step counts iff a final member of its role holds the
  bit and performed on the task; success, coverage efficiency and PRIMARY are
  formed from that count as the coverage-scored fields are from theirs.
  Without a draw it is `None`. The step predicate is pub:
  `harness::step_covered_performed`.
- **`src/harness/recon.rs`**: `TaskEnd::performed: Option<usize>` with
  `performed_success()` / `performed_cov_eff()`, and
  `Recon::performance_scored`, over the harness loop's own predicate and task
  rule. A task without a row, and an agent index outside a row, did not
  perform.

### Changed
- `run_workflow_instance` returns `WorkflowError::PerformanceShape` for a
  misshapen draw under every signal (before: under `Performance` / `Both`
  only).

### Gates
- **Identity.** `tests/harness_workflow.rs` and `tests/k7_group_host.rs` are
  unedited (`git diff --name-only v0.40.0..HEAD -- tests examples` prints
  nothing) and pass. `cargo nextest run`, pre-scaffold tree `e09fffb` →
  scaffold tree: `harness,process` 212 → 223, `harness,decision,process`
  307 → 318, 0 failed on either side; the 11 added tests are the scaffold's
  unit tests. `cargo test --features harness,decision,process --example k7_2`
  7 passed.
- **X-battery was not run**, by K7-2's prereg §5 condition. `git diff
  --name-only v0.40.0..HEAD -- src Cargo.toml Cargo.lock` prints `Cargo.lock`,
  `Cargo.toml`, `src/harness/mod.rs`, `src/harness/recon.rs`,
  `src/harness/workflow.rs`; the manifest and lock hunks are koalisi's own
  `version` line; `rg -c 'koalisi::harness|harness::'
  examples/strategy_comparison.rs` matches nothing.
- Suites by `cargo test`: `harness,process` 226, `harness,decision,process`
  321. `cargo clippy --all-targets -- -D warnings` on default, `harness`,
  `harness,process`, `harness,decision,process`; `RUSTDOCFLAGS='-D warnings'
  cargo doc --no-deps --features harness,decision,process`; `cargo fmt --all
  -- --check`.

## [0.40.0] — 2026-09-18

`K7-2`, the second registration of the K7 lineage
([#97](https://github.com/sustia-llc/koalisi/issues/97)): `query_novelty` on
against off on K7-1's topology-routed group. **`FALSIFIED (novelty moves the
outcome)`** — seeds 150..180, novelty on 0.4248 against novelty off 0.1415
(3.0017×, 30/30), every gate holding; the better cell ends where the
engine-free `ref-prune` ends on 600 of 600 tasks. Pre-registration
(Amendments 1–4, all pre-run), report and raw output:
[`docs/k7/`](docs/k7/prereg-K7-2-novelty-routed-group.md),
[`docs/runs/K7-2.log`](docs/runs/K7-2.log).

### Added
- **`src/harness/recon.rs`** (features `harness` + `process`): the instruments
  K7-1 kept example-side — `reconstruct` (arrival orders + decision trace →
  per-task final member sets, PRIMARY and churn; `ReconError`), `TaskEnd`,
  `Recon`, `step_covered`, `member_set_identity`, `RosterRow` +
  `roster_decomposition`, and three engine-free reference policies:
  `RefPrune` (join always; leave iff every covered demanded step stays
  covered), `RefFirst` (never join, never leave), `RefKeep` (join always,
  never leave). `examples/k7/k7_1.rs` is not edited.
- **`examples/k7/k7_2.rs`**, its own `[[example]]` (features
  `harness,decision,process`): seven cells, five in-binary gates (X-recon,
  S-determinism, seed invariance, S-learn (i), S-route), the counted criterion
  H-live / H-dead / H-move, one `VERDICT:` line. The registered block runs
  only with `K7_2_OFFICIAL=1` on a `0.40.0` build; `K7_2_SEEDS` accepts
  `6000..6003` and `6000..6030` and prints gate lines only; any other `K7_`
  environment variable refuses.
- **`tests/k7_group_host.rs`**: X-host for `grp-role-nonov` on 330..360
  against `docs/runs/K4-archive.log:2037` / `:2076`; X-carry for `grp-topo`,
  `grp-role` and `ref-prune` on 90..120 against `docs/runs/K7-1.log` (per-seed
  PRIMARY, pooled rows, the 598-of-600 identity, the roster rows).
  **`tests/k7_2_novelty.rs`** (S-nov): the routed read on a hand-built
  fixture, fresh and after one observed task, eight pinned `(act, score
  bits)` values. `tests/harness_workflow.rs`: `WorkflowSpec::default()`
  generates every seed of 150..180.
- `docs/README.md`: the K7-2 row; seed block 150..180 consumed.
  `CLAUDE.md`: the index line for gotcha 35.

### Gates
- **X-battery was not run** (prereg §5's condition; owner, 2026-09-18). `git
  diff --name-only v0.39.0..3a9557c -- src Cargo.toml Cargo.lock` prints
  `Cargo.lock`, `Cargo.toml`, `src/harness/mod.rs`, `src/harness/recon.rs`; the
  manifest and lock hunks are koalisi's own `version` line and the `k7_2` /
  `k7_2_novelty` stanzas; `rg -c 'koalisi::harness|harness::'
  examples/strategy_comparison.rs` matches nothing.
- On `3a9557c`, before the run: `cargo nextest run --features
  harness,decision,process` 307 passed (X-host, X-carry, S-nov, the generate
  check among them); `cargo test --features harness,decision,process --example
  k7_2` 7 passed. Suites by `cargo test`: `harness` 129, `harness,process`
  215, `harness,decision,process` 310; the other lanes' inputs are unchanged.
- clippy `--all-targets -- -D warnings` on the three harness lanes, `cargo doc`
  `-D warnings`, `cargo fmt --all -- --check` and `cargo +1.93.0 check
  --all-targets --locked --features harness,decision,process` clean.

## [0.39.0] — 2026-09-17

Dependency re-pin: `surrealdb-live-message` `v0.2.1` → `v0.2.2`, on its own
ahead of `K7-2` ([#97](https://github.com/sustia-llc/koalisi/issues/97)). No
`src/` change.

### Changed
- **`surrealdb-live-message` re-pinned `v0.2.1` → `v0.2.2`**, the pin its own
  commit. Upstream's tag moves `surrealdb` / `surrealdb-types` 3.2.1 → 3.2.4
  and rewrites one rustdoc block. `Cargo.lock` moves one stanza
  (`surrealdb_live_message` 0.2.1 → 0.2.2); `cargo update -p
  surrealdb_live_message` reported "Locking 0 packages".
- **`config/default.toml`: SurrealDB container image tag `v3.1.5` → `v3.2.4`**,
  its own commit — upstream's own test-harness tag. The container is
  upstream's; koalisi supplies the `[sdb]` keys it reads.
- **The MSRV gate verifies the declaration** (owner, 2026-09-17): `cargo
  +1.93.0 check --all-targets --locked --features <set>` on the feature sets
  a change reaches. The tier re-measurement is retired; the tiers in
  `Cargo.toml` are a historical record, last measured at `v0.37.0`.

### Gates
- Scope (owner, 2026-09-17): `durable` only. With every other feature
  enabled, `cargo tree --locked --features
  decision,magnitude,magnitude-fast,persistence,remote,process,harness,metrics
  -i surrealdb_live_message` reports that the package matches nothing, so the
  frozen archive binary and the other lanes build from inputs identical to
  `v0.38.0`. **X-battery and the all-lane comparison were not run.**
- `cargo test --features durable`: 107, identical per test binary to a
  `7f6cf10` worktree; the container-backed restart test ran on
  `surrealdb/surrealdb:v3.2.4` (`docker events`: the run's
  `koalisi_sdb_container` was created from that image). `examples/durable_decisions`
  ends `stopped cleanly`.
- clippy `--all-targets --features durable -- -D warnings` and `cargo doc`
  `-D warnings` clean; `cargo +1.93.0 check --all-targets --locked --features
  durable` passes.

## [0.38.0] — 2026-09-17

`K7-1`, the first registration of the K7 lineage
([#90](https://github.com/sustia-llc/koalisi/issues/90)): topology-routed
group voting (tira ext-6) against the candidate-blind group —
**`VALIDATED (topology-routed group)`**, 2.4144×, 30/30 on seeds 90..120.
Registration, amendments, run and report:
[`docs/k7/`](docs/k7/prereg-K7-1-topology-routed-group.md),
[`docs/runs/K7-1.log`](docs/runs/K7-1.log).

### Added
- **`VoteRouting`** on `GroupAifConfig` (features `decision` + `process`):
  `Off` (the default — the bare `VotingAgent` active slot) and
  `CandidateStar { lambda }`, which wraps the active slot in
  `aif::RoutedAggregator` over an `aif::Topology` built per decision on the
  realised roster: the candidate's role internal is the centre, every other
  row places `lambda` on it and `1 − lambda` on itself, identity rows when the
  centre is off the roster. A `lambda` outside `[0, 1]` and a routing combined
  with `SeededSampling` or a non-`CertaintyWeighted` vote are construction
  errors (`GroupAifError::InvalidRoutingWeight`, `UnsupportedRouting`).
- **`GroupAifPolicy` implements the lifecycle hooks** of
  `CoalitionDecisionPolicy`: `begin_task(&TaskStart)` rebuilds the `Demand`
  with `Demand::from_steps`; `observe_outcome` forwards the per-bit signal.
  `TaskStart::steps` carries distinct steps, so every multiplicity is 1
  through the hook.
- `AgreementSample` gains `leave`, `group_act`, `sensitive_rows`,
  `blind_origin_share`, `centre_vote`; `GroupAifCounters` gains
  `routed_reads`, `begin_task_rejections`, `outcome_updates_unapplied`. The
  last was added by review after the run of record (tree `1688b75`); an
  off-block smoke (`K7_1_SEEDS=5000..5003`) on the binaries before and after
  it is byte-identical once the latency column is stripped.
- **`harness::TracedPolicy`** (feature `harness`): wraps a policy, forwards
  all four trait methods, records `(leave, act, score bits)` per decision.
- **`examples/k7/k7_1.rs`** (`harness,decision,process`): eight cells over
  seeds 90..120, six in-binary gates computed before any table, the H-T bar,
  the `ref-prune` identity, the outcome decomposition by roster, one
  `VERDICT:` line. `K7_1_SEEDS=a..b` runs an off-block smoke with no verdict
  and refuses `0..480`.
- **`tests/k7_group_host.rs`**: the harness-hosted `grp-role` /
  `grp-role-blind` on 330..360 reproduce `docs/runs/K4-archive.log:2032`,
  `:2036` and the reach row `:2071`.

### Changed
- `docs/README.md`: the K7-1 row; seed block 90..120 consumed.
- With routing on, an error routing the H-S disclosure after a successful
  read is a decline counted in `declines_upstream`
  (`rg -n 'agreement sample routing failed' src/` names the site). `Off`
  calls no fallible routing code.

### Gates
- Suites on the branch: the seven lanes the diff cannot touch are identical
  per test binary to a `7f6cf10` worktree (`magnitude` 135, `magnitude-fast`
  143, `persistence` 126, `persistence,magnitude` 156, `remote` 112,
  `metrics` 106, `durable` 107); default 106, `decision` 162,
  `decision,magnitude` 191, `process` 161 unchanged;
  `decision,magnitude,process` 241 → 249, `harness` 126 → 129,
  `harness,process` 198 → 201; new lane `harness,decision,process` 291.
- `cargo fmt --all -- --check` clean; clippy `--all-targets -- -D warnings`
  clean at `--no-default-features`, default and fourteen feature lanes; `cargo
  doc` with `-D warnings` clean at default and fourteen feature sets.
- **X-battery PASS on the second of two serial runs** on `1688b75`. The
  first flipped the v1 latency criterion (mag 3.306 µs < aif 3.871 µs) and
  three verdict-text lines with it; all 13 hunks surviving the column strip
  were latency or wall-clock lines. The re-run: 11 surviving hunks, all
  latency or wall-clock, all 33 verdict lines byte-identical (`cmp`). Not
  re-baselined.
- MSRV, the sets whose code changed, `rust-version` temporarily at 1.85.0:
  1.88 passes default, `magnitude`, `persistence`, `remote`, `process`,
  `harness`, `harness,process`, `metrics`; 1.89 passes
  `harness,decision,process` and `decision,magnitude,process`; 1.88 refuses
  `harness,decision,process` (aif 0.14.0 / nalgebra 0.35.0 / safe_arch
  1.2.0). Not re-measured in this release: `decision`, `decision,magnitude`,
  `magnitude-fast`, `persistence,magnitude` at 1.89 and `durable` at 1.92 —
  no dependency moved and no code under them changed.
  `rust-version = "1.93.0"` kept (C-D1).

## [0.37.0] — 2026-09-17

Dependency re-pin: `aif-v0.13.0` → `aif-v0.14.0`, on its own ahead of the
`K7-1` registration
([#90](https://github.com/sustia-llc/koalisi/issues/90)). No `src/` change.

### Changed
- **`aif` re-pinned `aif-v0.13.0` → `aif-v0.14.0`**, the pin its own commit.
  The tag adds `aif::Topology` and `aif::RoutedAggregator` (tira #46, ext-6);
  koalisi names neither (`rg -n 'RoutedAggregator|aif::Topology|aif::\{[^}]*Topology'
  src/ examples/ tests/` prints nothing). `Cargo.lock` moves one
  stanza: `aif` 0.13.0 → 0.14.0 (`git diff --numstat` on the pin commit: 2
  insertions, 2 deletions in the lock).
- `Cargo.toml` MSRV comment: `aif` 0.14 declares `rust-version` 1.89, so
  under `decision` it joins nalgebra / safe_arch / wide in refusing 1.88.

### Gates
- Fourteen suites, identical per test binary on `main` (`372bf06` worktree)
  and on the pinned tree (127 `test result` lines, `cmp` clean): 106 / 162 /
  135 / 191 / 143 / 126 / 156 / 112, `process` 161,
  `decision,magnitude,process` 241, `durable` 107, `harness` 126,
  `harness,process` 198, `metrics` 106.
- `cargo fmt --all -- --check` clean; clippy `--all-targets -- -D warnings`
  clean at `--no-default-features`, default and thirteen feature lanes
  including the frozen-binary lane; `cargo doc` with `-D warnings` clean at
  default and thirteen feature sets.
- **X-battery PASS on one serial run** of the frozen archive binary on the
  pinned tree: 2129 lines, 122 raw differing lines against
  `docs/runs/K4-archive.log`, 10 hunks after the latency-column strip, all
  latency figures; all 33 verdict lines byte-identical (`cmp`). No drift.
- MSRV tiers reproduced with `rust-version` temporarily at 1.85.0: 1.88 for
  default, `magnitude`, `persistence`, `remote`, `process`, `harness`,
  `harness,process`, `metrics` (1.87 fails on catgraph's let-chains); 1.89
  for `decision`, `magnitude-fast`, `decision,magnitude,process` (1.88
  refused by aif 0.14.0 / nalgebra 0.35.0 / safe_arch 1.2.0 / wide 1.7.1 on
  the `decision` sets, by the latter three on `magnitude-fast`); 1.92 for
  `durable` (1.91 fails to compile `diskann`). `rust-version = "1.93.0"`
  kept (C-D1).

## [0.36.0] — 2026-09-17

A Prometheus metrics example over the three runtime tap surfaces ([#25], as
re-scoped on the issue 2026-09-17), and a lock refresh (PR #94). No `src/`
change.

### Added
- **Feature `metrics`** (off by default): optional `metrics` 0.24 and
  `metrics-exporter-prometheus` 0.18 (`default-features = false`,
  `http-listener` only), plus tokio's `net` + `io-util`.
- **`examples/metrics_scrape.rs`** (`required-features = ["metrics"]`): one
  consumer per tap surface — `koalisi_decisions_total{kind,act}` +
  `koalisi_decision_score` from a `spawn_decision_tee` sink,
  `koalisi_task_outcomes_total{success}` + `koalisi_outcome_members` from an
  `OutcomeSink` under `spawn_outcome_forwarder`,
  `koalisi_topology_events_total{event_type}` from the
  `TemporalHypergraph::with_event_tap` receiver. A scripted workload, a
  lossless shutdown, one `GET /metrics` against the example's own loopback
  listener (`KOALISI_METRICS_ADDR` overrides the address), and every scraped
  counter, histogram count and histogram sum asserted against a
  recorder-free count and against the scripted value.

### Changed
- **`Cargo.lock` refreshed** with `cargo update` under `rust-version =
  "1.93.0"`, as its own commit ahead of the feature (154 packages updated,
  15 added, 21 removed; `wide` 1.5.0 → 1.7.1, `safe_arch` 1.0.0 → 1.2.0,
  `simba` 0.10.0 → 0.10.2, `surrealdb` 3.2.1 → 3.2.4 among them). The
  manifest's dependency requirements are unchanged. The feature commit then
  adds 14 stanzas and moves none (`git diff --numstat`: 160 insertions, 0
  deletions).
- `Cargo.toml` MSRV comment: `metrics` joins the 1.88 tier; the 1.89 tier
  names `safe_arch` 1.2 / `wide` 1.7.

### Gates
- Fourteen suites, identical per test binary on `main` (`3ff92f6`
  worktree) and on the branch: 106 / 162 / 135 / 191 / 143 / 126 / 156 /
  112, `process` 161, `decision,magnitude,process` 241, `durable` 107,
  `harness` 126, `harness,process` 198; the new `metrics` lane 106.
- `cargo fmt --all -- --check` clean; clippy `--all-targets -- -D warnings`
  clean at `--no-default-features`, default and thirteen feature lanes
  including `metrics` and the frozen-binary lane; `cargo doc` with `-D
  warnings` clean at default and eleven feature sets.
- The example's assertions falsified in a copy of the tree: eight
  perturbations (one skipped increment per surface, an inverted `act`
  label, a renamed `event_type`, an extra series, a skipped histogram
  record, an off-by-one histogram value), each exiting non-zero with the
  mismatching values in the message.
- **X-battery PASS on one serial run** of the frozen archive binary on the
  refreshed lock: 2129 lines, 124 raw differing lines against
  `docs/runs/K4-archive.log`, 11 hunks after the latency-column strip, all
  latency or wall-clock figures; all 33 verdict lines byte-identical.
- MSRV tiers reproduced on the refreshed lock with `rust-version`
  temporarily at 1.85.0: 1.88 for default, `magnitude`, `persistence`,
  `remote`, `process`, `harness`, `harness,process`, `metrics` (1.87 fails
  on catgraph's let-chains); 1.89 for `decision`, `magnitude-fast`,
  `decision,magnitude,process` (1.88 refused by nalgebra 0.35.0 /
  safe_arch 1.2.0 / wide 1.7.1); 1.92 for `durable` (1.91 fails to compile
  `diskann`). `rust-version = "1.93.0"` kept (C-D1).
- `/code-review low`, two passes, both on the example: the first four
  findings (a probed loopback port could be taken before the exporter bound
  it — now retried on a fresh port; the last connect error was discarded;
  the sample parser split label sets on bare commas and took the last token
  as the value — now quote-aware, value first; the series-set check compared
  lengths — now the sets), the second one (label pairs kept surrounding
  whitespace — now trimmed; that fix shipped without a further pass). The
  set assertion falsified in a copy with an extra series labelled
  `Bo,gus} x`.

## [0.35.0] — 2026-09-16

The K7 harness scaffold (phase H of the K7 round, [#92]): the Part 9/11
workflow world copied out of the frozen archive binary, and a per-task
lifecycle hook on `CoalitionDecisionPolicy`. No registration; no behaviour
change for any existing policy.

### Added
- **`koalisi::harness::workflow`** (features `harness` + `process`): the
  v2w world of `examples/strategy_comparison.rs` Parts 9/11 as library
  code — `WorkflowSpec` (the v2 prefix draw, per-agent roles, per-required-bit
  role tags with the feasibility re-draw, the shape draw, an optional
  `PerformanceSpec` draw appended last), `WorkflowInstance` / `WorkflowTask`
  (declared `Demand` per task, `StaffingTable`), `OutcomeSignal`
  (`RoleCoverage`, `Performance`, `Both`), `WorkflowArm` (a per-instance
  policy factory), `run_workflow_instance` / `run_workflow_battery` with
  Part 11's role-matched distinct-step scorer, `WorkflowError`. The frozen
  binary is untouched (`git diff --stat examples/strategy_comparison.rs`
  → nothing).
- **Identity gate** `tests/harness_workflow.rs` (features `harness,process`):
  the copy, driving the `wf-asis` control (`MagnitudePolicy` with the
  identity role modulation) reproduces Part 9's thirty per-seed `wf-asis`
  rows on seeds 270..300 (`docs/runs/K4-archive.log:1787–1816`) and Part
  11's medians 0.1806 / 7.50 on 330..360 (`:2031`);
  `tests/fixtures/k7-workflow-v2w.txt` pins every generated instance on
  both blocks; `InstanceSpec { required_bits: 2..=8, .. }` is pinned equal
  to the frozen `draw_prefix_v2` at seed 270.
- **Lifecycle hook**: `CoalitionDecisionPolicy::begin_task(&TaskStart)` and
  `observe_outcome(required, &[bool])`, default no-ops; `TaskStart {
  required, steps: &[(bit, role)] }` (re-exported at the crate root).
  `run_instance` calls them once per task before the arrivals (empty
  `steps`, the flat per-bit union over 32 bits, `FLAT_SIGNAL_WIDTH`) and
  after the leave sweep; `run_workflow_instance` passes the declared
  distinct steps and the selected `OutcomeSignal` bits.
- `Demand::from_steps` (feature `process`): a `Demand` from an iterator of
  steps, one occurrence per yielded step.
- `InstanceSpec::draw(&mut SplitMix64)`: the draw body of `generate`,
  callable on a caller-owned stream.

### Fixed
- Two intra-doc links in `src/subsystems/remote.rs` pointed at a
  `persistence`-gated item; `RUSTDOCFLAGS='-D warnings' cargo doc
  --features remote` was red on `main` since v0.34.0 (measured on
  `1c08d6b`). Now code spans.

### Gates
- Thirteen suites: 106 / 162 / 135 / 191 / 143 / 126 / 156 / 112, `process`
  161 (+2), `decision,magnitude,process` 241 (+2), `durable` 107, `harness`
  126 (+1), and the new `harness,process` lane 198.
- `cargo fmt --check` clean; clippy `--all-targets -- -D warnings` clean at
  `--no-default-features`, default and every feature lane including
  `harness,process` and the frozen-binary lane
  `decision,magnitude,process`; `cargo doc` with `-D warnings` clean at
  default and all nine feature sets.
- Every new pin falsified in a copy (fixture pin, identity rows, medians,
  hand-derived role-mismatch case, hook order, hook placement,
  `from_steps` multiplicity); the Part 9 fan-out denominator was found
  unable to move the identity rows (it changes occurrence multiplicity
  only) and is caught by the fixture pin instead.
- `examples/gauntlet.rs` output identical to the pre-scaffold tree except
  the summary's latency column (H1 X-identity).
- **X-battery PASS on the second of two serial runs**: the first run
  (2129 lines, 130 raw differing lines, 28 after the latency-column strip)
  flipped the v1 latency criterion to PASS (aif median 4.118 µs vs the
  archive's 3.312 µs) and with it the v1/v2 verdict lines — gotcha 34's
  timing-noise flip on Path A, every quality, churn, ratio and superiority
  line identical; the re-run on the same quiet machine (124 raw differing
  lines, 22 after the strip, all latency or wall-clock figures) reproduced
  all 33 verdict lines byte-identically. Recorded, not re-baselined.
- MSRV tiers reproduced with `rust-version` temporarily at 1.85.0: 1.88 for
  default, `magnitude`, `persistence`, `persistence,magnitude`, `remote`,
  `process`, `harness`, `harness,process` (1.87 fails on catgraph's
  let-chains); 1.89 for `decision`, `magnitude-fast`,
  `decision,magnitude`, `decision,magnitude,process` (1.88 refused by
  nalgebra 0.35 / safe_arch 1.0 / wide 1.5); 1.92 for `durable` (1.91 fails
  to compile). `rust-version = "1.93.0"` kept (C-D1).
- `/code-review low`, two passes: the first (implementation commits) no
  findings; the second (whole branch) three — `redraw_cap = 0` behaved as
  `1` (now `WorkflowError::ZeroRedrawCap`, the field documented as draws
  including the first), `WorkflowResult::declined` was always `0`
  (removed), and `Demand::from_steps` has no production caller yet
  (kept: owner lock item 2 on [#92], the constructor a K7 arm needs to
  rebuild the `Demand` from the hook's `steps`).

[#92]: https://github.com/sustia-llc/koalisi/issues/92

## [0.34.0] — 2026-09-16

Housekeeping re-pin: three direct dependencies move, the whole tree is
rustfmt-clean, `cargo doc` is warning-free at every feature set, and the
manifest and the two working-state documents are trimmed to what they state.
No behaviour change.

### Changed
- **Dependencies**: `sha2` 0.10 → **0.11**, `libp2p` 0.56 → **0.57**,
  `surrealdb-types` 3.2.1 → **3.2.4**; lock refreshed (22 package stanzas
  added, 7 removed; the libp2p 0.5x sub-crates, hickory 0.26, curve25519 /
  ed25519-dalek 3–5, and the digest 0.11 stack move with them). The lock now
  carries `sha2` 0.10.9 and 0.11.0 and `digest` 0.10.7 and 0.11.3 side by
  side: koalisi's `persistence` uses 0.11, the surrealdb stack still pulls
  0.10 (`cargo tree -i sha2@0.10.9 --features durable`).
- **rustfmt** applied to the whole tree under rustfmt 1.9.0 (38 files,
  whitespace and line-wrapping only; `cargo fmt --check` was red on `main`
  since the toolchain moved). This includes `examples/strategy_comparison.rs`
  (100 hunks): the frozen K4 archive binary is reformatted, not changed — the
  drift check below is the evidence.
- **Intra-doc links**: 18 rustdoc links fixed so `RUSTDOCFLAGS='-D warnings'
  cargo doc --no-deps` is green at default and at every feature set
  (`decision,magnitude,process,persistence,remote,magnitude-fast,harness,durable`):
  8 at default (`src/lib.rs`, `src/algorithms/aipa.rs`,
  `src/subsystems/coalition_actor.rs`) and 10 feature-gated
  (`aif_persistent_policy.rs`, `magnitude_policy.rs`, `durable.rs`,
  `remote.rs`) — links to private items become plain code spans, redundant
  explicit targets are dropped, one unresolved name gets its full path.
- **`Cargo.toml` trimmed**: the 80-line MSRV comment becomes 14 lines (the
  tiers, the procedure, the resolver-3 ground, a pointer to gotcha 34);
  dependency and feature comments state what each is for, without history.
- **`CLAUDE.md` refreshed**: review protocol and record-location paragraphs
  shortened, the mission and tooling sections rewritten to the current tree,
  the v0.32.0 and v0.31.0 entries compressed (full text in the private
  ledger), gotcha 34 reduced to its contracts, a lint + docs row in the gate
  table.
- **`README.md` refreshed**: dependency list (libp2p 0.57, sha2 0.11 +
  ciborium row, history dropped), the `durable` test count.

### Gates
- All twelve suites at baseline: 106 / 162 / 135 / 191 / 143 / 126 / 156 /
  112 / 159 / 239, `durable` 107, `harness` 125.
- `cargo fmt --check` clean; clippy `--all-targets -- -D warnings` clean at
  default, the six-feature set, `harness` and `durable`; `cargo doc` with
  `-D warnings` clean at default and at all eight features;
  `remote_coalition_consumer` example exits 0 on libp2p 0.57.
- **X-battery PASS — zero non-latency diffs.** One serial release run of the
  reformatted binary on the new lock (`pgrep -c 'cargo|rustc'` = 0 at start,
  nothing alongside), 2129 lines, diffed against `docs/runs/K4-archive.log`
  per `docs/runs/README.md`: 61 raw differing lines; with the trailing
  latency column stripped, 11 survive and every one reports latency or
  wall-clock time (three `latency µs` rows, Criterion 2 / Path B.3 with their
  FAIL / PASS outcomes unchanged, the mm/scalar latency ratio 1.33× on both
  sides, three `Medians … Latency … (record-only)` lines whose median and churn
  fields are identical, the 12-bit `arm-E1g4` latency, the A3.2 sweep time
  2.2 s → 2.3 s). All 33 `VERDICT|FALSIFIED|VALIDATED` lines identical
  (`diff` exit 0). The committed log stays the artifact of record.
- **MSRV tiers unchanged on the new lock** (procedure of gotcha 34): 1.88
  (default; `magnitude,persistence,remote,process,harness` together — 1.87
  fails on the catgraph let-chains) · 1.89
  (`decision,magnitude,process,magnitude-fast`; 1.88 refused: nalgebra /
  safe_arch / wide) · 1.92 (`durable`; 1.91 fails compiling `diskann`).
  `rust-version` stays 1.93.0.

## [0.33.0] — 2026-09-16

The K7 scaffold — Phase A of the stack's 2026-09-16 K7 round plan. The K4
lineage is closed and its binary frozen as the archive; the next lineage gets
its own harness, its own example, a public index, a public protocol, and
committed raw run outputs. No registration runs here.

### Added
- **`docs/README.md`** — the index of the A/B showcase trail: the K4 verdict
  trail (17 rows, every pre-registration and report by path), the K7 lineage
  section (empty), the **seed ledger** (moved here from `CLAUDE.md`; 90..120
  reserved for `K7-1`, 150..180 reserved-unconsumed), the immutability rule
  and the `docs/` layout. Every file under `docs/` is named in it (basename
  sweep: `fd -t f . docs -x basename {}` against `rg -o -F -f`, nothing
  missing).
- **`docs/PROTOCOL.md`** — the run protocol every registration follows:
  design-lock → pre-registration → three-lens review → serial run on a quiet
  machine → immutable report; pin-first; the latency-column-stripped diff;
  review on every PR; the seed rules; naming. `CLAUDE.md` keeps a pointer.
- **`docs/runs/`** — committed raw outputs. `K4-archive.log` is ONE fresh
  serial run of `examples/strategy_comparison.rs` at the `v0.32.0` pins
  (release build, `pgrep -c 'cargo|rustc'` = 0 at start, no suite alongside):
  2129 lines, 33 lines matching `VERDICT|FALSIFIED|VALIDATED`, every `VERDICT`
  headline equal to the trail of record. `docs/runs/README.md` states the
  archive recipe and the re-pin drift check (one fresh run, diffed with the
  trailing latency column stripped, verdict lines diffed separately).
- **Feature `harness`** (default off, no dependencies) and **`src/harness/`**
  — the K7 round's shared plumbing: `rng` (`SplitMix64`, `permutation`,
  `distinct_bits`), `instance` (`InstanceSpec` with a `Default` of 8 bits /
  pool 4..=16 / 1..=4 capabilities / trust 20..=99 / 20 tasks / 1..=5
  required bits → `Instance` over `CapabilityAgent`), `battery` (`SeedRange`,
  `Arm`, `InstanceResult`, `BatteryResult`, `run_instance`, `run_battery`:
  bootstrap join, policy-gated arrivals, one leave sweep in arrival order,
  completion × mean coverage efficiency), `report` (`percentile`,
  `median_iqr`, `superior_count`, the per-seed and summary tables with latency
  as the summary's last column only, `Verdict`). 19 unit tests; 15 falsified
  red-then-green in a `cp -r` copy.
- **`examples/gauntlet.rs`** (`required-features = ["harness"]`) — the K7
  harness skeleton: prints the default spec and one smoke arm
  (`ThresholdPolicy<SynergisticCalculator>`, seeds 0..3), no verdict line,
  zero registrations. It names none of the K4 helpers (`rg -n -F` over
  `draw_prefix generate_instance coalition_view best_subset oracle_primary
  InstanceMetrics Worker SplitMix64 draw_distinct_bits fisher_yates next_unit
  Scope` on the file → nothing) and uses only `koalisi::harness`,
  `koalisi::algorithms::SynergisticCalculator` and
  `koalisi::decision::ThresholdPolicy`. K7 registrations are one
  `[[example]]` each under `examples/k7/k7_<n>.rs`.

### Changed
- `examples/strategy_comparison.rs` — header comment naming it the frozen K4
  archive binary and the X-battery gate; the run line now carries all three
  required features. One comment hunk, no code change (`git diff --stat`:
  9 insertions, 2 deletions; `rg -c '^@@'` on the diff → 1).
- `README.md` — the A/B section names the index, the protocol, the runs
  directory and the two binaries; `harness` module row; example and test
  lines.
- `CLAUDE.md` — the verdict-trail table, seed ledger, standing run protocol
  and the per-file `docs/` inventory are replaced by pointers to `docs/`;
  61,387 → 58,417 chars (`wc -c`).

### Gates
- All twelve suites at baseline, every `test result` line summed: 106 / 162 /
  135 / 191 / 143 / 126 / 156 / 112 / 159 / 239, `durable` 107; the new
  `harness` suite 125 (106 + 19).
- Clippy `--all-targets -- -D warnings` clean from fresh target dirs at
  default, `decision,magnitude,process,persistence,remote,magnitude-fast`,
  `harness` and `durable`.
- `cargo run --features harness --example gauntlet` exits 0.
- Zero-hit confirmations: `rg -n 'deep_causality|ultragraph' Cargo.lock` →
  nothing. `rg -n strategy_comparison src/` is **not** a zero-hit: two rustdoc
  lines in `src/algorithms/population.rs` (60, 82) name the example as the
  origin of the `SplitMix64` / `next_unit` convention; left as is.
- Two reds on the **base tree**, neither a gate of record in this repo and
  neither touching a line this release adds: `cargo fmt --check` under
  rustfmt 1.9.0 flags 38 files on `main` (`git worktree add` of `54f0855`,
  exit 1); `RUSTDOCFLAGS=-D warnings cargo doc --no-deps` fails on 8
  pre-existing intra-doc links (`src/lib.rs:6,18`, `src/algorithms/aipa.rs:19`
  ×5, `src/subsystems/coalition_actor.rs:137`) at any feature set without
  `process`. The new `harness` bullet in `src/lib.rs` is plain backticks, not
  an intra-doc link, so it adds no ninth.

### MSRV — tiers reproduced; `harness` joins the 1.88 tier
Committed procedure (`rust-version` temporarily 1.85.0, `cargo +<v> check
--all-targets --locked --features <set>`, restored, manifest diff shows only
the intended hunks; never `--ignore-rust-version`):

| tier | feature sets | evidence |
|---|---|---|
| 1.88 | default · `magnitude` · `persistence` · `remote` · `process` · **`harness`** | 1.87 fails: five `let` chains in the `catgraph 0.23.0` lib (default and `harness` probed at 1.87; the other four contain that lib and passed at 1.88) |
| 1.89 | `decision` · `magnitude-fast` · `decision,magnitude,process` | 1.88 refused declaratively: nalgebra 0.35.0 / safe_arch 1.0.0 / wide 1.5.0 |
| 1.92 | `durable` | 1.91 fails compiling `diskann` (lifetime / `Iterator` not general enough) |

`rust-version = "1.93.0"` unchanged (owner decision C-D1).

## [0.32.0] — 2026-09-14

The catgraph `v0.9.0` → `v0.23.0` re-pin (#87), Phase C of the stack's
2026-09-13 downstream re-pin plan. All three catgraph deps move in lockstep per
the K6 one-repo-one-checkout rule; the pin is its own commit so a battery drift
would be attributable to it. **Drift check CLEAN.** The `process` MSRV tier
collapses from 1.93 to 1.88 because the last DeepCausality edge is gone.

### Changed
- `catgraph-applied`, `catgraph-magnitude`, `catgraph-syntax` → **`v0.23.0`**
  (15 upstream tags, `v0.10.0` … `v0.23.0`; the consumer-facing digest is the stack roadmap's
  cross-repo seams items (1)–(9)).
- **MSRV comment in `Cargo.toml` rewritten** — three measured tiers, the 1.93
  tier gone; the declared `rust-version` stays 1.93.0 pending owner decision
  C-D1 (see MSRV below).
- `README.md` dependency lines and `CLAUDE.md` tooling / gotcha 34 text: the
  `catgraph-syntax` → DeepCausality sentence is retired.

### Compile-breaking set — none
Every name the stack roadmap's seams items (1)–(9) call breaking, plus the
earlier zero-hit list, was searched **unanchored** over `src/`, `tests/`,
`examples/` (`_` is a word character, so a `-w` search cannot see
`from_permutation_on_domain`):

```
rg -n -o 'is_left_id|is_right_id|assert_valid|FrobeniusMorphism|cospan_to_frobenius|as_cospan|PetriNet|PetriDecoration|new_unchecked|Composable|is_left_identity|is_right_identity|represents_id|from_permutation|HypergraphLattice|GaugeGroup|HypergraphRewriteGroup|plaquette_action|total_action|record_transition|wilson_loop|is_causally_invariant|structurally_equal|RewriteRule|apply_at|replay|RewriteRejection|RewriteBoundary|RewriteSide|UnitInterval|from_rig_value|UNIT_INTERVAL_FLOOR|\b(Cospan|Span|Rig|Tropical|MatR|MatKron|Decomposition|EvalPath|Arrow|haft)\b' src tests examples
```

Three names hit; each is compile-neutral, and the clippy runs below are the
proof:
- `RewriteRule` (19 hits; the calls are `RewriteRule::new(lhs, rhs)` at
  `src/process/theory.rs:292,304,319`, propagated with `?`) and `replay`
  (175 hits, all but one koalisi's own `replay_into_event_log` / `run_replay` /
  replay buffers; the catgraph call is `replay(start, rules, outcome.steps())?`
  at `src/process/rewrite.rs:111`) — item (8), core + applied #447: return
  types unchanged, only the `CatgraphError` variant and message text moved.
  koalisi matches no rewrite rejection and asserts on no message text
  (`rg -n Presentation src tests examples` → two constructions of koalisi's own
  errors, `src/process/theory.rs:263` and `src/process/signature.rs:204`, plus
  two doc lines).
- `UnitInterval` (3 hits, `src/decision/magnitude_policy.rs:277,1032,1040`) —
  item (9), **applied** #451: `UnitInterval::new(p)?` at line 1040 is fed
  `CouplingModel::coupling` (`magnitude_policy.rs:732–739`), a ratio of
  `count_ones` over a non-zero `u32` mask, so any positive `p` is ≥ 1/32 and
  the new `Err` on `0 < p < 1e-9` is unreachable from this caller.

Two shapes re-verified at the tag rather than assumed:
`CatgraphError::Presentation { message }` is unchanged
(`catgraph/src/errors.rs:449` at `v0.23.0`), and `FrobeniusOr` still has five
variants (`catgraph-syntax/src/frobenius.rs:173–184`), so the exhaustive match
at `src/process/cost.rs:120–124` compiles. `EvalPath::MergeOnly` (magnitude
`v0.19.1`) does not reach koalisi, which names
`ZeroDiversityProof::SkeletalMerge` (`magnitude_policy.rs:3086,3226,3668`,
`examples/strategy_comparison.rs:5967,6253,13197,13229`) and never `EvalPath`.

### Gates
- **All eleven suites at baseline counts on BOTH sides**, measured on a
  worktree of `main` before the pin and on the pinned tree after, every
  `test result` line summed: 106 / 162 / 135 / 191 / 143 / 126 / 156 / 112 /
  159 / 239, plus `durable` 107.
- Clippy `--all-targets -- -D warnings` clean from a fresh target dir at
  default features and at
  `decision,magnitude,process,persistence,remote,magnitude-fast`.
- **X-battery PASS — zero non-latency diffs.** Both runs serial on a quiet
  machine (`pgrep -c 'cargo|rustc'` = 0 at each start), 2129 lines each. 70
  line-pairs differ raw; 59 are table rows whose only changed cell is the
  trailing latency column, and the 11 that survive the column strip are lines
  that report latency or wall-clock time in prose or in a `latency µs` row
  (Criterion 2 and Path B.3 keep their FAIL / PASS outcomes; the Part 9 A3.2
  search-cost disclosure reads 0.3 s / 2.1 s against 0.1 s / 1.1 s, one run each
  side). All 33 `VERDICT` / `FALSIFIED` / `VALIDATED` lines are
  byte-identical (`rg 'VERDICT|FALSIFIED|VALIDATED'` on each side, `diff`
  exit 0), both headline verdicts included (`FALSIFIED (latency)` /
  `VALIDATED (B)`). Of the 11 surviving pairs, one is the mm/scalar
  **latency** ratio (1.46× → 1.52×) and three are `Medians … Churn …
  Latency` lines whose only changed field is the latency — their median,
  churn and quality fields are identical.
- The three predicted value-exposure rows (applied #451 `UnitInterval`
  closure, magnitude #450 `SCHUR_SLOW_FALLBACK_TOL` retune, magnitude #436
  `MergeOnly` on mutual clones) produced no decision change on the frozen
  battery. No drift note is filed.

### MSRV — three tiers now; the 1.93 tier is gone; declaration is C-D1
Measured with the committed procedure (`rust-version` temporarily 1.85.0,
`cargo +<v> check --all-targets --locked --features <set>` per set, restored,
manifest diff clean; never `--ignore-rust-version`):

| tier | feature sets | evidence |
|---|---|---|
| 1.88 | default · `magnitude` · `persistence` · `remote` · **`process`** | 1.87 fails: five `let` chains in the `catgraph 0.23.0` lib |
| 1.89 | `decision` · `magnitude-fast` · `decision,magnitude,process` | 1.88 refused declaratively: nalgebra 0.35.0 / safe_arch 1.0.0 / wide 1.5.0 |
| 1.92 | `durable` | 1.91 fails compiling `diskann` (lifetime / `Iterator` not general enough) |

The cross-feature maximum is therefore **1.92**, and `rust-version = "1.93.0"`
now sits above it. The value is **unchanged in this release**: whether to move
it is owner decision C-D1 (stack re-pin plan §4). Of the two 2026-08-09 grounds
for declaring the maximum, the showcase ground no longer holds
(`strategy_comparison`'s feature set checks on 1.89) and the resolver-3 ground
applies to any lower value as before. Retired claim (b) of v0.31.0 ("no
catgraph re-pin can lift it") is now also *empirically* wrong: this re-pin
lifted it.

### Lockfile — read in full, not grepped
- **Four** catgraph packages move (core `catgraph` rides as a transitive).
- `deep_causality_algebra 0.2.0`, `deep_causality_haft 0.4.2`,
  `deep_causality_num 0.4.1` leave — package stanzas and the
  `catgraph-syntax` dependency-array edge. 681 → 678 packages, net **−3**.
  `rg -n 'deep_causality|ultragraph' Cargo.lock` → nothing.
- `catgraph-applied`'s dependency array swaps `rand 0.10.2` for
  `rand_core 0.10.1` (cg#239); the `rand 0.10.2` stanza survives for three
  other consumers.
- Two unrelated edges moved under re-resolution, invisible to a
  `name`/`version`/`source` grep (fourth occurrence: v0.26.0, v0.29.0,
  v0.31.0): `data-encoding-macro-internal 0.1.18` now references
  `syn 2.0.118` (was `1.0.109`) and `tempfile` references `getrandom 0.3.4`
  (was `0.4.3`). Both old versions keep their stanzas.

### Process
No CI: koalisi tracks no workflow (`git ls-files | rg '\.github|workflows'` →
nothing); suites and battery run locally. The pre-pin
baseline ran on `git worktree add /tmp/koalisi-base e612a5f` with its own
target dir; the pinned tree used the repo's `/tmp/koalisi-target`; clippy and
each MSRV toolchain used fresh dirs.

## [0.31.0] — 2026-08-09

The catgraph `v0.8.0` → `v0.9.0` re-pin, run under the standing re-pin protocol
(own PR, no code changes mixed in). All three catgraph deps move in lockstep per
the K6 one-repo-one-checkout rule. **Drift check CLEAN.**

The headline is a correction, not a discharge: the MSRV re-test this re-pin was
supposed to satisfy has been owed since v0.17.0 on a premise that turns out to
be false in both halves.

### Changed
- `catgraph-applied`, `catgraph-magnitude`, `catgraph-syntax` → **`v0.9.0`**.
  Upstream's `v0.9.0` is a dependency-streamlining release: cg#219/#221 make
  catgraph own the `Zero`/`One` identity traits and the forward-mode `Dual`
  (retiring its direct `deep_causality_num` / `deep_causality_num_dual` deps),
  and cg#220 makes it own the toposort and connected-components passes
  (retiring `ultragraph`).
- **MSRV comment in `Cargo.toml` rewritten** — the floor stays **1.93.0**, now
  recorded as a measured fact with its actual cause.

### MSRV — re-test done; declared floor stays 1.93, but it is FEATURE-CONDITIONAL
The whole DeepCausality chain enters through **exactly one edge** — the optional
`catgraph-syntax` → `deep_causality_haft` → `algebra` → `num` — so it is present
**only under feature `process`**:

- `cargo tree -i deep_causality_haft` → *no match* at default features, and no
  match at `decision,magnitude`. It resolves only with `--features process`.
- `cargo +1.92 check --all-targets --ignore-rust-version` → **exit 0** at
  default features, and **exit 0** at `decision,magnitude,persistence,remote`.
  1.91 also succeeds; 1.85/1.86 fail on let-chains (stabilised 1.88), so the
  non-`process` floor is >1.86 and ≤1.91 (not pinned exactly — 1.87–1.90 not
  installed).
  **⚠ This bullet is FALSE — see the correction below.** `durable` is
  non-`process` and its floor is **1.92**, and these numbers were taken with
  `--ignore-rust-version`, which cannot see a dependency floor at all.
- Only with `process` on do three crates each demand 1.93:
  `deep_causality_algebra 0.2.0`, `deep_causality_haft 0.4.2`,
  `deep_causality_num 0.4.1`.

The note carried since v0.17.0 said the floor was `deep_causality_num =0.4.1`
alone "propagating through catgraph", re-testable once catgraph v0.9.0 "removes
that crate". Both halves fail: v0.9.0 retires only catgraph's *direct*
dependency (the crate survives beneath `haft → algebra`, as upstream's own
manifest states), and `num` was never the sole cause since `haft` and `algebra`
demand 1.93 independently.

**But the floor is koalisi's own, not the substrate's** — the `process` tier is
imposed by koalisi's own optional feature.

> **⚠ Corrected same day (2026-08-09).** The measurements in this section were
> taken with `--ignore-rust-version`, which bypasses the gate a downstream hits
> — and suppresses the *dependency* rust-version checks too, so it cannot see a
> dependency floor at all. Precisely one claim above is falsified by
> re-measurement: **"the non-`process` floor is >1.86 and ≤1.91"** (flagged
> inline at that bullet). It is wrong twice — `durable` is non-`process` and
> needs **1.92**, and `decision` / `magnitude-fast` need **1.89**
> (`nalgebra 0.35` / `safe_arch` / `wide`), which the flag hid. The section's
> other claims — the single `catgraph-syntax` → `haft` edge, `cargo tree -i`
> finding no match without `process`, and the three 1.93-demanding crates —
> re-verified correct. The real picture is **four tiers**: 1.88 default /
> `magnitude` / `persistence` / `remote` · 1.89 + `decision` /
> `magnitude-fast` · 1.92 + `durable` · 1.93 + `process`.
> **`rust-version` stays 1.93** (owner, same day): lowering was implemented,
> measured and reverted, because `strategy_comparison` requires all three
> features so the A/B showcase needs 1.93 regardless, and edition 2024's
> resolver 3 makes a lower declared MSRV a silent brake on dependency updates
> (`cargo update --dry-run` at 1.88 held back nalgebra, roaring, safe_arch,
> wide and the deep_causality crates). See `Cargo.toml` and gotcha 34.

The obligation is closed as *corrected*, not *discharged*.

### Lockfile — read in full, not grepped
- **Four** catgraph packages move, not three: the core `catgraph` crate goes
  along as a transitive of `catgraph-applied`.
- `ultragraph 0.9.2` leaves entirely — package stanza plus both
  dependency-array references.
- `union-find 0.4.4` is newly referenced by `catgraph-applied`; it was already
  in the lock, so **no package is added**.
- `deep_causality_num` disappears from two dependency arrays while its package
  stanza stays. A `name`/`version`/`source` grep shows it unchanged in both
  locks and reports nothing — only the dependency arrays reveal that its path
  changed while the crate survived. Third occurrence of this trap (v0.26.0,
  v0.29.0).
- Net: **−1 package**.

### Breaking rider — not applicable here
cg#219/#221 are breaking for downstream scalars: a crate implementing `Rig` for
its own type via `deep_causality_num`'s `Zero`/`One` must move to
`catgraph_applied::rig::{Zero, One}`. koalisi names no `Rig` impl, no `rig::`
import and no `deep_causality` or `ultragraph` path — it consumes concrete
surfaces (`Coalition`, `HomMap`, `LawvereMetricSpace`, `UnitInterval`,
`ZeroDiversityProof`). Verified by search across `src/`, `examples/`, `tests/`
and `Cargo.toml`.

### Gates
- **All ten suites at baseline counts**, measured **before** the bump as well
  (all ten matched the table, so no documentation drift hides in the
  comparison): 106 / 162 / 135 / 191 / 143 / 126 / 156 / 112 / 159 / 239, plus
  `durable` at 107 (= default + 1 container-backed restart test).
- Default clippy `--all-targets` clean from a fresh target dir.
- **X-battery PASS — zero non-latency diffs.** Both runs 2129 lines; of 122
  differing lines, 102 are table rows whose only changed field is the final
  latency column, and 20 are prose lines that explicitly report latency. With
  the latency column stripped the diff is empty. Every quality / ratio /
  superiority / churn / verdict line is byte-identical, both headline verdicts
  included (`FALSIFIED (latency)` / `VALIDATED (B)`).

### Method note — battery runs must be SERIAL
The first attempt at the X-battery comparison was **invalid** and was discarded.
Both runs had been executed concurrently with cargo test suites on the reasoning
that latency is excluded from the comparison anyway. It is excluded as a
*reported metric* but still feeds **Path A**, the v1 speed criterion — so
latency noise propagates into a *verdict* line. The contaminated pre-bump run
reported Path A **PASS**, contradicting the report of record, and the resulting
diff showed a spurious `VALIDATED (A+B)` → `VALIDATED (B)` "change". Re-run
serially on a quiet machine, both sides reproduce the documented
`FALSIFIED (latency)` / `VALIDATED (B)`. See CLAUDE.md **gotcha 34**.

## [0.30.0] — 2026-08-08

The EQ5b typed two-engine registration
([#78](https://github.com/sustia-llc/koalisi/issues/78)). Verdict:
**`VALIDATED (two-engine)`** — the second validated registration in the K4
lineage, after EQ4 (`docs/ab-report-K4-eq5b-typed-two-engine.md`; prereg
`docs/prereg-K4-eq5b-typed-two-engine.md` + Amendments 1–6, all pre-verdict).

**Read the mechanism before quoting the verdict.** It passes by 0.7 %, and two
findings contradict the arm's name.

### Added
- **`src/decision/group_policy.rs`** — `GroupAifPolicy` (features `decision` +
  `process`): `aif::GroupAgent` with a `CopyAgent` sensory slot, **R = 3
  role-slotted internals** over arm-E1's persistent world model, a
  `VotingAgent` in `CertaintyWeighted`, deciding through `aif-v0.13.0`'s
  **deterministic** `group_distribution` read (no RNG, no `last_action`
  advance). Carries `WorldModelTopology`, `PrecisionChannel`, `CoverageMasks`,
  `GroupVote`, `DecisionRead`, and the S-learn instrumentation
  (`GroupAifCounters`, `ModelUpdateAudit`, `S_LEARN_VACUITY_TOL`).
- `examples/strategy_comparison.rs` **Part 11** — ten arms, H-G with both
  conjuncts printed per confirmatory cell, four in-binary gates, and every
  registered disclosure.

### Result
- `grp-role` **0.2270 = 1.2567×**, superior **22/30** — PASS both conjuncts.
- `grp-mult` **0.2266 = 1.2544×**, superior **22/30** — PASS.
- Control `wf-asis` 0.1806. **The margin (0.7 % / 0.4 % over the bar) is
  reported in full and not renegotiated** — the same discipline EQ5a applied to
  a narrow miss against a higher bar.
- Pre-committed scoped clauses fired automatically: **did not** exceed
  `wf-val-p` (0.2435) ⇒ *"beats the typed control, not the strongest process
  cell"*; **did** exceed `arm-E1` (0.0403).
- Gates X-battery / X-identity / S-determinism (+ seed invariance) / S-learn
  all **PASS**.

### Mechanism — both findings cut against the framing
- **Role specialisation is net NEGATIVE.** The shared-model reference cells
  score *higher* than the role-specialised confirmatory cells (0.2409 vs
  0.2270; 0.2404 vs 0.2266), with the specialised cell superior on only 15/30
  and 13/30 seeds.
- **The arm depends on the structural defect the review found.** 23.0 % of
  decisions have **zero** candidate-sensitive internals; the blind ones carry
  **0.626** of the CW weight and act at 95.1 % vs 80.4 %. Removing them
  (`grp-role-blind`) collapses the arm to **0.0268**. What beats the typed
  control is a permissively-joining group dominated by members blind to the
  candidate — not deliberation, not specialisation.
- `grp-role-nonov` 0.1125 vs 0.2270 reproduces v5's X1 collapse at the group,
  but prices the mechanism **and** that bias together; they are not separated.

### Process
Six pre-verdict amendments, three correcting the registration's own reasoning:
the arm as first registered was **unbuildable**; D2's `CertaintyWeighted`
rationale was **measured false**; A3.1's replacement prediction was **measured
false**. A 3-lens review (5 blocking / ~15 important / ~30 minor) ran before the
run with every finding dispositioned. **The first official run returned
`RUN-INVALID`** on a mis-specified S-learn guard that demanded movement from
world models with `expected == 0`; the guard was unsatisfiable on **any** seed
block (12.5 % base rate), so the guard — not the block — was corrected, and the
re-run is a reproduction because the guard is post-hoc and provably could not
move a value (verified: zero measured values changed).

**Seeds consumed: 330..360. 90..120 and 150..180 remain reserved.**
**koa#54 (mag = demonstrated default) stays FINAL** — no EQ5b outcome reopens it.

### Changed
- Suites: `decision,magnitude,process` 215 → **239**; all other configurations
  unchanged.


---

> 📄 **Sections v0.29.0 → v0.1.0 are archived.** This file began at 153,187 B
> (2,685 lines) on 2026-09-20; everything from `[0.29.0]` (2026-08-08) down to
> `[0.1.0]` (2026-05-23) was moved out verbatim that day and this file now
> starts at `[0.30.0]`. The archive is held outside this repository, beside the
> release ledger — its location is in `CLAUDE.local.md` (file name:
> `changelog-archive-pre-v0.30.0.md`); the per-release narrative for the same
> span is §1 of `project-history.md`.

[Unreleased]: #unreleased
[#20]: https://github.com/sustia-llc/koalisi/issues/20
[#25]: https://github.com/sustia-llc/koalisi/issues/25
[#31]: https://github.com/sustia-llc/koalisi/issues/31
[#32]: https://github.com/sustia-llc/koalisi/issues/32
[#33]: https://github.com/sustia-llc/koalisi/issues/33
[#100]: https://github.com/sustia-llc/koalisi/issues/100
[#103]: https://github.com/sustia-llc/koalisi/issues/103
[#104]: https://github.com/sustia-llc/koalisi/issues/104
[#111]: https://github.com/sustia-llc/koalisi/issues/111
