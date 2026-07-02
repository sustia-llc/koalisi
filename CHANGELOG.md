# Changelog

All notable changes to **koalisi** will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

Design input #1 of 2 landed: SwarmAgentic-style optimisation becomes
the new Phase 5 (see `docs/SwarmAgentic-summary.md`). One more design
input still pending before any of Phases 5–7 begin implementation.
Phase ordering reshuffled: Persistence moved to Phase 7 (last) so the
SwarmAgentic particle traces and Active Inference belief states inform
the persistence schema before we commit to a wire format.

Planned work (tracked in [`CLAUDE.md`](./CLAUDE.md) §"Next steps"):

- **Phase 5 — SwarmAgentic-style optimisation**: language-driven PSO
  meta-layer that evolves coalition designs. Five integration ideas
  (configurator, failure-aware velocity ↔ EFE bridge, ValueCalculator
  feedback weights, AIPA + population hybrid, cross-model
  transferability). Depends on the new `LlmProvider` stub. *(Stub
  trait already in place — see "Added" below.)*
- **Phase 7 — Persistence**: feature-gated `PersistentHypergraph` from
  hypergraph v4.2.0 + an append-only `EventStore` trait. Must also
  durably record SwarmAgentic particle lineages and EFE belief
  snapshots.
- **Databento `LiveClient` integration** *(blocked on `DATABENTO_API_KEY`)*.
- **Synthetic DBN file** for end-to-end arb signal demo.
- **Remote gateway hardening** — bounded buffer, cursor-based polling,
  stable wire schema, QUIC transport alongside TCP.

### Added (unreleased — issue [#7] A/B harness, K4)

- **Pre-registered A/B harness** ([#7], `examples/strategy_comparison.rs`, now
  `required-features = ["decision", "magnitude"]`): Part 1 keeps the original
  Threshold-vs-AIF divergence demo unchanged; Part 2 benchmarks
  `AifDecisionPolicy` vs `MagnitudePolicy` over the #7-pre-registered battery —
  30 SplitMix64-seeded instances (no `rand` dep), pools of 4–16 agents over an
  8-bit capability universe, 20-task streams with seeded arrival orders,
  unconditional first-arrival bootstrap (AIF cannot self-start from empty),
  one leave sweep. PRIMARY = completion-rate × coverage-efficiency; oracle
  regret (brute force, pools ≤ 8); churn + warm sync-path latency secondaries;
  exploratory t-sweep (t ∈ {0.5, 1, 2, 10}) via an example-local policy — the
  library arm stays pinned at t = 1.
- **Committed report** `docs/ab-report-K4-yamafaktory.md` (yamafaktory backend,
  pre-K1, release build; deterministic except latency). Result: magnitude arm
  superior on the primary metric in **30/30 seeds** (median 0.4469 vs 0.1898),
  churn 8 vs 113, oracle regret 0.1156 vs 0.3757 — but median per-decision
  latency 4.37 µs vs 1.48 µs, so the pre-committed **verdict is
  `FALSIFIED (latency)`** (criterion 1 pass, criterion 2 fail; nothing tuned).
- **Upstream find (resolved):** the battery surfaced a debug-only panic in
  `catgraph-magnitude v0.1.0` (over-strict triangle-inequality `debug_assert`,
  ULP noise on non-dyadic couplings) — filed catgraph#29, fixed by catgraph
  PR #30, tagged `v0.1.1`. **koalisi dep bumped `v0.1.0` → `v0.1.1`**; debug
  builds run clean. The harness runs `--release` for the latency criterion.

[#7]: https://github.com/sustia-llc/koalisi/issues/7

### Added (unreleased — issue [#5] magnitude decision arm, K2)

- **`MagnitudePolicy` — the categorical A/B mirror of the AIF arm** ([#5],
  new feature `magnitude`, independent of `decision`): coalition join/leave
  decisions scored by **coalition magnitude** (effective-member diversity)
  instead of expected free energy.
  - New dep `catgraph-magnitude` (git tag `v0.1.0`, SSH URL — catgraph is
    private, same rationale as the `aif` dep; never a path dep). Feature-off
    builds compile none of it.
  - `src/decision/magnitude_policy.rs`: `MagnitudeValueCalculator` (impl
    `ValueCalculator`; value = `catgraph_magnitude::coalition_value` at the
    pinned `t = 1` Shannon-diversity arm) + `MagnitudePolicy` (impl
    `CoalitionDecisionPolicy`, sync + rayon-offloaded async — the whole
    `O(m³)` magnitude computation runs on the rayon pool) + `CouplingModel`
    (the mapping, public for direct testing).
  - **The capabilities→coupling mapping** (the semantic heart): directed
    substitutability `A(i→j) = |rel_i ∩ rel_j| / |rel_i|` over
    required-masked capability bits. Capability clones are mutually coupled
    at `1.0` and **skeletalize** into one effective agent (deliberate mirror
    of the AIF arm's clone degeneracy); subsumed agents get Möbius weight 0;
    disjoint specialists count fully. **Task-irrelevant agents
    (`rel == 0`) are excluded from the member set** — a vacuous `1.0`
    coupling would drive their Möbius weight negative and collapse coalition
    diversity (code-review finding, hand-verified: 3 specialists + 1
    bystander scored 1.0 instead of 3.0 and ejected a unique specialist on
    leave).
  - Upstream `CatgraphError`s are policy-level outcomes (decline / `-∞`
    value), never panics. Dedup by `agent_id` before every upstream call.
  - Tests: 9 unit (upstream seam pins — chain `0.7/0.5 ⇒ Mag(1) = 1.80`,
    mutual-1.0 skeletalization ⇒ `1.0`; mapping table; hand-computed
    calculator values `3.0`/`4/3`/`1.0`; join/leave mirrors; bystander
    regression; Err path; async ≡ sync incl. through
    `Box<dyn CoalitionDecisionPolicy>`). Suite: 67 default / 76 `magnitude`
    / 86 `decision` (unchanged) / 95 both.

[#5]: https://github.com/sustia-llc/koalisi/issues/5

### Added (unreleased — issue [#1] decision wiring)

- **Live decision call site** ([#1]): the `AifDecisionPolicy` / async-offload
  primitives shipped in v0.6.0 now have a real seam.
  - `CoalitionManager::{try_join_coalition, try_leave_coalition}`
    (`src/topology/coalitions.rs`, gated `where V: AgentCapabilities`):
    consult a `&dyn CoalitionDecisionPolicy` over the coalition's current
    membership and a real `DecisionContext`, then apply the mutation iff the
    policy returns `act`. The CPU-bound part runs through the policy's async
    offload (`should_{join,leave}_async`), so the runtime worker is not blocked
    even for the AIF policy.
  - `subsystems::coalition_actor::CoalitionActor<V, HE>` — kameo actor owning a
    `CoalitionManager`, a `Box<dyn CoalitionDecisionPolicy>`, and a
    `DecisionContext`. `JoinRequest`/`LeaveRequest`/`Members` messages drive
    policy-gated membership. `AifDecisionPolicy` is never named here — the AIF
    strategy and `ThresholdPolicy` are interchangeable behind the trait object.
  - `tests/decision_integration.rs` (4 feature-off + 1 feature-on): apply /
    decline / force-leave through the actor, the manager primitive in isolation,
    and AIF non-degeneracy through the live actor (an agent covering a new
    required bit joins; a redundant clone does not).
- **`AgentCapabilities: Send + Sync`** (`src/algorithms/mod.rs`): lets
  `&dyn AgentCapabilities` capability views cross `.await` points / threads, as
  the async decision seam requires. All concrete agent types are `Copy` data, so
  the bound is satisfied for free.

[#1]: https://github.com/sustia-llc/koalisi/issues/1

### Added (unreleased — issue [#2] belief-aware scoring)

- **Trust / compatibility / history beliefs in the AIF decision path** ([#2],
  feature `decision`): the decision now reflects more than capability coverage.
  - `koalisi::decision::{TrustBeliefs, CompatibilityBeliefs, CoalitionHistory}`
    re-exported from `aif` (plain `f64`/`HashMap`, no new deps).
  - `BridgeParams.belief_weight` (default `0.0`) blends a belief *alignment*
    scalar (from `aif::belief_weighted_preference`) into the **competence** that
    drives the POMDP observation model:
    `competence = (1 - belief_weight)·coverage + belief_weight·alignment`. At the
    default `0.0` the policy is pure coverage — behavior is byte-for-byte
    unchanged. Because beliefs modulate the *observation model* (not just
    preferences), the decision stays **non-degenerate** (the B2/B4 requirement):
    membership still alters achievable `G`.
  - `AifDecisionPolicy` now carries `trust`/`compat`/`history` (constructible via
    `AifDecisionPolicy::with_beliefs`). With `belief_weight > 0`: a
    capability-redundant but well-trusted agent can now join; a coverage-improving
    but badly-distrusted partnership can be declined; recorded coalition history
    shifts the margin. The async (rayon) path is preserved — belief lookups happen
    in the sync prologue, only the EFE math is offloaded.
  - Trust reconciliation: `AgentCapabilities::trust_level()` is the *static*
    baseline; `TrustBeliefs` is the *dynamic* EMA-learned signal. They are
    complementary, not duplicated — koalisi has no `TrustGraph`.
  - Tests: 6 new policy unit tests (belief-weight-zero parity, high-trust join,
    low-trust block, history flip, leave control, sync/async equivalence) + 1
    integration test driving a belief-aware join through the live `CoalitionActor`.

[#2]: https://github.com/sustia-llc/koalisi/issues/2

### Added (unreleased — Phase 5 prep)

- **`koalisi::llm` stub module** (`src/llm/mod.rs`): defines the
  `LlmProvider` trait (`fn complete(&self, prompt: &str) -> impl
  Future<Output = anyhow::Result<String>> + Send`) plus
  `StubLlmProvider` that returns `"no LLM backend configured yet"`.
  Plan documents and future Phase 5/6 code reference `LlmProvider`;
  real backends (OpenAI / Anthropic / Ollama / local llama.cpp) land
  later behind a future `llm` feature flag with per-backend
  sub-features. One unit test covers the stub error path (default
  test count: 26 → 27).

## [0.6.0] — 2026-05-29

Phase 6 — a **pluggable, optional** Active Inference coalition-decision
layer, built on the code-reviewed `aif` reference engine (the `tira`
repo, tag `aif-v0.4.0`). AIF is never forced on all swarms: it coexists
with a non-AIF baseline behind a trait, selectable per swarm. This does
**not** port the retired `coalition_aif` prototype (its AIF math was
buggy); it bridges koalisi capabilities to the correct engine.

### Added

- **`decision` module** (`src/decision/`, `mod.rs` always compiled):
  - `CoalitionDecisionPolicy` trait — object-safe; `should_join` /
    `should_leave` plus dyn-compatible `should_join_async` /
    `should_leave_async` (boxed-future variants) over
    `&dyn AgentCapabilities` + `&[&dyn AgentCapabilities]` +
    `&DecisionContext { required_capabilities: u32 }`, returning
    `Decision { act: bool, score: f64 }`.
  - `ThresholdPolicy<C: ValueCalculator>` — always available, non-AIF
    baseline; joins when a candidate's marginal coalition value clears a
    threshold. Reuses the existing `ValueCalculator` impls unchanged.
- **Optional `decision` feature** (`decision = ["dep:aif"]`):
  - `aif = { git = "ssh://git@github.com/sustia-llc/tira", tag =
    "aif-v0.4.0", optional = true }` (SSH because `tira` is private and
    git is SSH-only). Feature-off builds compile **no `aif` and no
    `nalgebra`**; koalisi itself adds no matrix library.
  - `EfeValueCalculator` — impl of the EXISTING `ValueCalculator`;
    coalition value = `−G` (negated expected free energy). Slots
    alongside Additive/Synergistic/Multiplicative/Weighted.
  - `AifDecisionPolicy` — impl of `CoalitionDecisionPolicy`; joins iff
    coalition membership lowers `G`. The capability→EFE bridge maps
    coverage of `required_capabilities` to a 2-state POMDP
    observation-model precision (built via `aif::POMDPAgent` directly),
    so membership alters `G` (not just preferences) and the decision is
    non-degenerate (verified monotone; unit-tested). CPU-bound EFE is
    offloaded to the rayon pool via `tokio-rayon`; the async trait
    methods keep `Box<dyn CoalitionDecisionPolicy>` non-blocking.
- **`examples/strategy_comparison.rs`** (`required-features =
  ["decision"]`) — one join scenario under both `ThresholdPolicy` and
  `AifDecisionPolicy`, printing where they diverge.
- Tests: 30 default / 40 with `--features decision` (incl. monotonicity,
  non-degeneracy + degeneracy guards, sync/async equivalence,
  async-via-trait-object). Non-finite (NaN/±∞) margins are guarded.

### Notes

- Deferred (tracked as GitHub issues #1, #2): wiring `AifDecisionPolicy`
  into a live kameo / `CoalitionManager` call site, and recovering aif's
  `TrustBeliefs`/`CompatibilityBeliefs`/`CoalitionHistory` for richer
  scoring. See `CLAUDE.md` §"Phase 6".

## [0.5.0] — 2026-05-27

Max-effort `/code-review` sweep across the topology and algorithms
layers. Three reviewers (reuse, quality, efficiency) flagged 33
distinct findings; all applied or explicitly deferred. Includes real
API breakage — see "Breaking" below.

### Fixed

- **`TemporalHypergraph::create_snapshot` race**: the marker append and
  the snapshots-table insert now happen under a single events-log write
  guard. `Snapshot::event_index` correctly points at the marker
  (previously off-by-one even single-threaded). `temporal.rs:507-531`.
- **`TemporalQueries::{vertex,hyperedge}_lifespan` double-lock race**:
  `created` and `removed` are now sampled under a single read lock via
  private `_impl` helpers, eliminating the window where a concurrent
  writer could yield an inconsistent lifespan. `queries.rs`.
- **`TemporalAnalytics::{vertex,hyperedge}_count_series` O(samples × |V|)
  re-locking**: replaced with a single-pass chronological walk that
  maintains a `HashSet<VertexIndex>` of live entities and emits counts
  at sample boundaries. One lock acquisition per call instead of one
  per sample. `analytics.rs`.
- **`TemporalAnalytics::delta` silently dropped clears**: `HyperedgesCleared`
  and `GraphCleared` events are now recorded in two new `GraphDelta`
  fields (`hyperedges_cleared`, `graph_cleared`) instead of being
  swallowed.
- **`EventLog::events_until` ignored its own index**: now uses
  `time_index.range(..=ts)`, O(log n + k) instead of O(n).
- **`EventLog::snapshot_before` linear HashMap scan**: snapshot index
  is now a `BTreeMap<Timestamp, (SnapshotId, usize)>` so
  `snapshot_before` is O(log n) via `.range(..=t).next_back()`.
- **`CoalitionManager::{join,leave}_coalition` TOCTOU window**: replaced
  the two-round-trip read-then-write with a new atomic
  `TemporalHypergraph::update_hyperedge_vertices_try` mutator helper
  that holds the graph write lock across the membership check + update.
- **`agent_coalition_history` lock-holding scan**: now snapshots the
  events vec under the read lock, then processes outside. The linear
  scan no longer blocks concurrent writers for its duration.
- **AIPA combinatorial wastefulness**:
  - `partition_count(n)` switched from full Vec enumeration to the
    O(n²) dynamic-programming recurrence (no per-partition allocation;
    n=50 went from ~204k Vec allocations to 51 usize cells).
  - `find_best_partition` replaced sort-then-take-first with `max_by`;
    O(p(n)) instead of O(p(n) log p(n)).
  - `compute_all_partition_bounds` pre-aggregates max/avg per coalition
    size once before the partition loop instead of rescanning each
    bucket per partition.

### Changed

- **Atomics**: `Clock` and `TemporalHypergraph::snapshot_counter`
  switched from `Ordering::SeqCst` to `Relaxed` (`tick`) /
  `Acquire`/`AcqRel` (compare-exchange) — same correctness, no x86_64
  `MFENCE` on every clock tick.
- **`TemporalAnalytics::events_by_type`**: returns `HashMap<&'static str,
  usize>` instead of `HashMap<String, usize>` and routes through the
  existing `TemporalEvent::event_type()` method — eliminates one
  `String` allocation per event in range and deduplicates the 13-arm
  match.
- **`DCVCDistributor::calculate_statistics`**: returns a typed
  `DistributionStats` struct (`min`, `max`, `avg`, `total`) instead of
  an unnamed 4-tuple. Computed in a single fold (no per-share Vec
  allocation, no three-pass min/max/sum).
- **`ValueCalculator` impls**: shared helpers (`size_bonus`,
  `capability_bonus`, `trust_sum`, `combined_capabilities`) and named
  constants (`SIZE_UNIT`, `CAP_UNIT`, `SYNERGY_UNIT`, `TEAM_UNIT`,
  `TEAM_THRESHOLD`) replace the three duplicated implementations'
  magic numbers.
- **`EventLog`**: `stats()` is now O(1) (incremental on `append`)
  instead of O(total events). All five `.or_insert_with(Vec::new)`
  call sites use `.or_default()` (clippy::pedantic alignment).
- **`HypergraphExecutor`**: gained `with_num_threads(n)` constructor
  for downstream runtimes that already partition CPUs across rayon
  pools. Module + struct documentation added.
- **CLAUDE.md**: new §"Available tooling for this project" section
  pointing at the `graph` plugin v2.0.1 (hypergraph agent + six
  hypergraph skills tracking hypergraph v4.2.0 HEAD). New gate notice
  on Phases 5 and 6 noting they are pending two user design inputs.
- **Test fixtures**: extracted shared `Agent`/`Coalition`/`as_caps`
  scaffolding from `tests/{algorithms,topology}_test.rs` into
  `tests/common/{algorithms,topology}.rs`. Examples stay standalone
  per the cargo-examples idiom.

### Breaking

- `Timestamp` and `SnapshotId` inner `u64` fields are now `pub(crate)`.
  External callers must use `Timestamp::new(n)` / `SnapshotId::new(n)`
  and `Timestamp::value()` / `SnapshotId::value()` instead of tuple
  construction or field access.
- `EventStats::new()` removed — use `EventStats::default()`.
- `SynergisticCalculator::new()` removed — use
  `SynergisticCalculator::default()` or the unit-struct literal
  `SynergisticCalculator`.
- `DCVCDistributor::calculate_statistics()` return type changed from
  `(usize, usize, f64, usize)` to `DistributionStats { min, max, avg,
  total }`. Callers must replace `let (min, max, avg, total) = ...`
  with field access.
- `koalisi::logger` and `koalisi::settings` modules removed — use
  `koalisi::core::config::{setup_logging, Settings, CoalitionSettings,
  SETTINGS}` directly.
- `koalisi::topology::EXEC` no longer re-exported at the topology
  module root (now `pub(crate)` in `executor.rs`). External callers
  can construct their own pool via `HypergraphExecutor::with_num_threads`.
- 6 `TemporalAnalytics` methods and 7 `CoalitionManager` temporal-query
  methods are now `pub(crate)` (no in-crate or external callers; will
  be re-promoted when Phase 5/6 wires them up): `vertex_count_series`,
  `hyperedge_count_series`, `mutation_rate`, `most_active_vertices`,
  `most_active_hyperedges`, `events_by_type`; `was_member_at`,
  `coalition_formed_at`, `coalition_dissolved_at`, `coalition_lifespan`,
  `count_agents_at`, `count_coalitions_at`, `agent_coalition_history`.
- `GraphDelta` gained two new fields (`hyperedges_cleared`,
  `graph_cleared`). Exhaustive pattern matches on `GraphDelta` will
  need to cover them.

### Verified

- `cargo test` (default): 26 lib + 15 algorithms + 11 topology + 5
  integration + 2 doctests = **59 pass, 0 fail, 0 warnings**.
- `cargo test --features databento`: + 4 integration tests pass.
- `cargo test --features remote`: + 1 integration test passes.
- `cargo check --all-targets` across all three feature configurations:
  clean.
- All seven examples (`topology_coalition`, `algorithm_values`,
  `triangular_arbitrage`, `historical_bootstrap`, `live_pubsub`,
  `supervised_swarm`, `distributed_alert_consumer`) plus the two
  databento examples exit 0 with expected output.

## [0.4.0] — 2026-05-26

Rename + topology and algorithm layer integration. Consolidates three
prior projects into a single layered crate; `forex-arbitrage-swarm`
becomes the runtime adapter.

### Added

- **Rename**: `forex-arbitrage-swarm` → `koalisi`. The original crate
  name survives as the runtime adapter's nickname; everything else
  re-aligned under `koalisi::`.
- **`core` layer** — `CoalitionRuntime` (domain-agnostic three-step
  shutdown: cancel → close → drain) extracted from the old `Swarm`'s
  lifecycle plumbing. `core::config` consolidates `Settings`,
  `CoalitionSettings`, and `setup_logging` behind one module.
- **`topology` layer** (ported from dynamo, on top of hypergraph
  v4.2.0):
  - `TemporalHypergraph<V, HE>` — event-sourced wrapper around
    `Hypergraph` with full per-mutation event recording.
  - `EventLog<V, HE>` — chronological `Vec<TemporalEvent>` plus
    `BTreeMap` time index, `HashMap` vertex/hyperedge indices,
    `HashMap` snapshot index.
  - `TemporalEvent<V, HE>` — 13-variant enum covering every
    hypergraph mutation plus snapshot markers.
  - `CoalitionManager<V, HE>` — agent (vertex) + coalition (hyperedge)
    API: `add_agent`, `form_coalition`, `join_coalition`,
    `leave_coalition`, `dissolve_coalition`, `merge_coalitions`,
    `coalition_members`, etc.
  - `TemporalQueries` — point-in-time state reconstruction via event
    replay (`vertex_exists_at`, `hyperedge_vertices_at`,
    `count_vertices_at`, etc.).
  - `TemporalAnalytics` — delta computation, time-series sampling,
    activity ranking, mutation-rate aggregation.
  - `HypergraphExecutor` (+ `EXEC` singleton) — rayon ↔ tokio bridge
    so the std `RwLock` inside the upstream hypergraph crate doesn't
    leak across `.await`.
  - `Timestamp`, `TimeRange`, `Clock` — logical-time primitives with
    thread-safe monotonic counter.
  - `examples/topology_coalition.rs` — form/join/merge + time-travel
    walkthrough.
  - `tests/topology_test.rs` — 11 integration tests.
- **`algorithms` layer** (ported from coalesce):
  - `AgentCapabilities` trait — abstract interface (`agent_id`,
    `capabilities`, `trust_level`) so calculators stay
    domain-agnostic.
  - `ValueCalculator` trait + four implementations:
    `AdditiveCalculator`, `SynergisticCalculator`,
    `MultiplicativeCalculator`, `WeightedCalculator` (with
    `balanced` / `capability_focused` / `trust_focused` presets).
  - `DCVCDistributor` — Distributed Coalitional Value Calculation
    workload split (Rahwan & Jennings, 2007).
  - `aipa` module — Anytime Integer-Partition Algorithm (Rahwan et
    al., 2007): `generate_integer_partitions`, `compute_*_bound`,
    `compute_all_partition_bounds`, `find_best_partition`,
    `partition_count`, `verify_partition`.
  - `examples/algorithm_values.rs` — calculator comparison + DCVC
    distribution + AIPA bound enumeration walkthrough.
  - `tests/algorithms_test.rs` — 15 integration tests.
- README rewrite documenting the four-layer architecture with the
  forex domain as a working adapter, the new examples, and the four
  origin projects.

### Changed

- `forex-arbitrage-swarm`'s public surface migrated under
  `koalisi::subsystems::{coordinator, monitor, sink, swarm}` (forex
  domain) and `koalisi::market` (value types). Examples and tests
  updated to the new paths.
- `CLAUDE.md` reorganised: four-layer mission statement, new file
  inventory with topology + algorithms additions, all 11 "worth
  flagging" gotchas updated for the koalisi naming.

## [0.3.0] — 2026-05-24

### Added

- **Remote alert gateway** (`subsystems::distributed`, feature `remote`):
  - `RemoteAlertGateway` actor (`#[derive(Actor, RemoteActor)]`), locally
    subscribed to `Swarm::alert_bus()`, remotely registered via
    `kameo::remote::Behaviour` running on a libp2p swarm.
  - `#[remote_message]` asks `PollOpportunities`, `PeekOpportunityCount`,
    `ClearOpportunities` exposed over the wire.
  - `enable_remote_alerts(&swarm, RemoteConfig) -> Result<RemoteHandle>`
    one-shot entry point: builds libp2p swarm (TCP + noise + yamux +
    mDNS + `kameo::remote::Behaviour`), calls `init_global()`,
    `listen_on(...)`, spawns the event loop on `swarm.task_tracker()`
    with a child of `swarm.cancellation_token()`, registers the gateway.
- **`Serialize + Deserialize` derives on wire-bound market types**:
  `Tick`, `Quote`, `Triangle`, `Direction`, `ArbitrageOpportunity`. These
  types now double as the on-wire payload for the remote gateway.
  *(POC trade-off — a production deployment would split internal and
  wire types behind a conversion boundary.)*
- `examples/distributed_alert_consumer.rs` — single binary with
  `ROLE=producer` / `ROLE=consumer` env-var dispatch. Producer runs a
  scripted feeder + registers the gateway; consumer mDNS-discovers and
  polls remote peers every 3 s.
- `tests/remote_integration.rs` — in-process wire round-trip:
  `enable_remote_alerts` → trigger an arb locally →
  `RemoteActorRef::<RemoteAlertGateway>::lookup_all(...)` finds self →
  `ask(&PollOpportunities)` round-trips the opportunity through full
  rmp-serde + libp2p `request-response`. Asserts edge_bps and cross pair
  survive the wire.
- New `[features]` entry: `remote = ["kameo/remote", "dep:libp2p"]`.
- `libp2p = { version = "0.56", features = ["macros", "noise", "mdns",
  "tcp", "tokio", "yamux"], optional = true }` direct dependency.
- README + `CLAUDE.md` document the hybrid architecture (local mpsc hot
  path + libp2p publish-to-outside-world boundary), the 4 new "worth
  flagging" entries (#8–#11 in `CLAUDE.md`) covering libp2p quirks, and
  the canonical `cd forex-arbitrage-swarm && cargo … --features remote`
  invocation.

### Changed

- README architecture diagram now shows the optional libp2p layer
  branching off the alert pubsub.
- `CLAUDE.md` reorganised: §"Next steps" reordered (A = LiveClient,
  B = synthetic DBN, C = remote gateway hardening, D = nice-to-haves);
  done-list and file inventory updated for the new files.

### Verified

- `cargo test --features remote` — 1 new test passes (16 tests total
  with `--features 'databento remote'`).
- `cargo run --features remote --example distributed_alert_consumer` —
  producer comes up with a real libp2p peer ID, gateway registers, mDNS
  starts listening.

## [0.2.0] — 2026-05-23

### Added

- **Databento DBN-file adapter** (`subsystems::databento`, feature
  `databento`):
  - `Pacing::{Asap, Realtime { speed_factor }}` — historical-bootstrap
    vs live-replay pacing.
  - `SymbolMapper = Arc<dyn Fn(u32, Option<&str>) -> Option<Pair> + Send + Sync>`
    closure for mapping DBN `instrument_id` / symbol strings to swarm
    `Pair` values.
  - `pump_dbn_file(feeder, path, mapper, pacing, token) -> Result<PumpStats>` —
    decode + route loop with FIFO-deterministic `SwarmFeeder::feed_tick`.
  - `spawn_dbn_pump(swarm, path, mapper, pacing) -> JoinHandle<...>` —
    fire-and-forget on `swarm.task_tracker()` with a child of
    `swarm.cancellation_token()`. Joins `Swarm::shutdown()` lifecycle.
  - `mbp1_to_tick(msg, pair)` helper — converts DBN fixed-point prices
    (1e-9 scale) to f64 and nanoseconds to milliseconds.
- **`SwarmFeeder`** — clone-able feed handle exposed via `Swarm::feeder()`.
  Captures only the `MarketMonitor` `ActorRef` map; lets background
  tasks call `feed_tick` without borrowing the `Swarm` itself.
- `examples/databento_historical.rs` — decode bundled `.dbn.zst` with
  `Pacing::Asap`, inspect the monitor snapshot.
- `examples/databento_live_replay.rs` — same file but spawned via
  `spawn_dbn_pump` with `Pacing::Realtime { speed_factor: 1e6 }` +
  custom alert listener subscribed to `alert_bus`.
- `tests/databento_integration.rs` — 4 tests: fixed-point conversion,
  file decode + routing, lifecycle join, cancellation honour.
- New `[features]` entry: `databento = ["dep:dbn", "dep:time"]`.
- Optional dependencies: `dbn = { version = "0.58", features = ["async"] }`,
  `time = { version = "0.3", features = ["macros"] }`.

### Changed

- `Swarm` gained a `feeder()` accessor returning the new `SwarmFeeder`
  type.

## [0.1.0] — 2026-05-23

Initial POC release.

### Added

- **Core actor swarm** (Coalition-shaped, modeled on
  `surrealdb-live-message`'s `Coalition<T>`):
  - `MarketMonitor` — one per pair, holds a `VecDeque<Tick>` ring buffer
    (capped at `history_capacity`), the latest `Quote`, and publishes a
    `TickUpdate` to the swarm's tick pubsub on every tick. Exposes
    `GetSnapshot`, `Ping`.
  - `ArbitrageCoordinator` — subscribed to `PubSub<TickUpdate>`. Walks
    `Vec<Triangle>` on every update; when `|edge_bps|` crosses
    `threshold_bps`, publishes an `ArbitrageOpportunity` to the alert
    pubsub. Per-triangle hysteresis: requires the edge to dip back
    below threshold before the same triangle can re-fire. Exposes
    `GetQuotes`, `Ping`.
  - `AlertSink` — subscribed to `PubSub<ArbitrageOpportunity>`. Buffers
    every received opportunity; exposes `GetAlerts`, `DrainAlerts`,
    `Ping`.
  - `Swarm` — top-level orchestrator owning all actor refs, both
    pubsubs, a `TaskTracker`, and a root `CancellationToken`. Library-
    first three-step shutdown (cancel → close → drain). Public API:
    `new`, `feed_tick`, `replay_history`, `flush`, `alerts`, `shutdown`,
    plus accessor methods.
- **Market value types** (`market` module): `Pair` (with `FromStr` for
  `"EUR/USD"`-style parsing), `Tick`, `Quote`, `Triangle` (with
  validation that legs share a common quote currency), `TickUpdate`,
  `ArbitrageOpportunity`, `Direction`.
- **Reference daemon** (`src/main.rs`) — assembles a swarm with the
  EUR/USD-GBP/USD-EUR/GBP triangle, attaches a scripted live feeder,
  waits for `ctrl-c`, drains cleanly.
- **Examples (4)**:
  - `historical_bootstrap` — single-pair history replay + ring-buffer
    snapshot inspection.
  - `live_pubsub` — scripted live feed + custom listener actor
    subscribed to the alert bus.
  - `triangular_arbitrage` — full triangle replay producing +45.25 bps
    and −72.06 bps signals, exercises hysteresis-rearm-flip.
  - `supervised_swarm` — kameo `OneForOne` supervisor with
    `restart_limit(3, 5s)` on a `MarketMonitor`; uses a `oneshot`
    readiness handshake (mirror of `surrealdb-live-message`'s
    `sdb_server::SurrealDBContainer::start_and_wait`) to race-freely
    obtain the supervised child's `ActorRef` from `main`.
- **Test suite (11 tests)**:
  - 6 unit tests in `market.rs` (pair parsing, triangle validation,
    edge_bps math).
  - 5 integration tests in `tests/integration_test.rs` (end-to-end
    triangular arb wiring, hysteresis, rearm + flip, ring-buffer
    eviction, empty-triangle rejection, unknown-pair rejection).
- **Configuration**: `config/default.toml` + `config/{development,test}.toml`,
  loaded via the `config` crate with `RUN_MODE` env override. Settings:
  `logger.level`, `swarm.threshold_bps`, `swarm.history_capacity`,
  `swarm.delivery_strategy`.
- **Logging**: idempotent `tracing_subscriber` setup via
  `logger::setup()` (uses `std::sync::Once` so tests sharing the
  subscriber don't race).
- README, `CLAUDE.md` working-state document, gitignore.

### Project conventions established

- Library-first lifecycle: callers wire their own top-level shutdown
  using `swarm.cancellation_token()` + `swarm.task_tracker()`. No
  `tokio-graceful-shutdown`, no `SubsystemHandle`.
- Path dependencies on local `kameo` checkout
  (`../../agentics/kameo{,/actors}`) until the upstream API stabilises
  on crates.io.
- `--manifest-path Cargo.toml --target-dir /tmp/forex-arbitrage-swarm-target`
  convention to avoid contention with Zed's own `cargo check`.
- `timeout 30s …` wrapper on `cargo run` invocations to bound runaway
  binaries cleanly.

[Unreleased]: #unreleased
[0.5.0]: #050--2026-05-27
[0.4.0]: #040--2026-05-26
[0.3.0]: #030--2026-05-24
[0.2.0]: #020--2026-05-23
[0.1.0]: #010--2026-05-23
