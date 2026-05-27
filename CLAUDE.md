# CLAUDE.md — koalisi

Working-state document. See `README.md` for user-facing description.
This file is for picking the project back up later.

When you bump the project's behaviour, also:
- update `Cargo.toml` `version`
- add a new top section to `CHANGELOG.md` (Keep a Changelog format)
- update the relevant entries here (Current state, Worth flagging,
  File inventory) so future-me doesn't relitigate decisions

## Mission (one paragraph)

**koalisi** — a reference implementation of agentic coalitions in Rust.
Four-layer architecture: Core (CoalitionRuntime, lifecycle), Topology
(temporal hypergraph via hypergraph v4.2.0, event sourcing, CoalitionManager,
time-travel queries, analytics), Algorithms (DCVC workload distribution,
AIPA partition search, pluggable value calculators), and Runtime (kameo
actors, PubSub buses, remote gateway). The forex triangular arbitrage
domain is preserved as a working adapter; the architecture is domain-agnostic.
Evolved from four prior projects: dynamo (topology), coalesce (algorithms),
coalition_aif (decision — planned), and forex-arbitrage-swarm (runtime).

## Available tooling for this project

- **`graph` plugin v2.0.1** (`~/.claude/plugins/cache/sustia-claude-code-plugins/graph/2.0.1/`)
  ships a `hypergraph` agent plus six hypergraph skills tracking
  hypergraph v4.2.0 HEAD: `hypergraph-core`, `hypergraph-mutations`,
  `hypergraph-algorithms`, `hypergraph-analytics`, `hypergraph-projections`,
  `hypergraph-persistence`. Use them when working on `src/topology/` or
  planning Phase 5 (the `hypergraph-persistence` skill maps directly onto
  the planned `PersistentHypergraph` integration).
- `rust-v2:rust-dev-v2` / `rust-v2:rust-practical` — primary Rust agents per
  the user CLAUDE.md routing rules.

## Current state — 2026-05-26

### Done

- **Rename**: forex-arbitrage-swarm → koalisi v0.4.0
- **Core**: `CoalitionRuntime` (TaskTracker + CancellationToken + three-step
  shutdown), consolidated settings/logging in `core::config`
- **Topology layer** (from dynamo): `TemporalHypergraph<V, HE>` with event
  sourcing, `CoalitionManager` (form/join/leave/dissolve/merge),
  `TemporalQueries` (point-in-time state reconstruction),
  `TemporalAnalytics` (delta/time-series/activity), `HypergraphExecutor`
  (rayon↔tokio bridge), `Timestamp`/`TimeRange`/`Clock`
- **Algorithm layer** (from coalesce): `AgentCapabilities` trait,
  `ValueCalculator` trait + 4 calculators (Additive, Synergistic,
  Multiplicative, Weighted), `DCVCDistributor`, AIPA partition search
- **Forex adapter** (preserved from forex-arbitrage-swarm):

- **Core swarm**: `MarketMonitor` × N + `ArbitrageCoordinator` +
  `AlertSink` + 2 `PubSub`s wired by `Swarm::new`, all under a single
  `TaskTracker` + `CancellationToken`.
- **`SwarmFeeder`** — clone-able feed handle (owns only the monitor
  `ActorRef` map). Lets background tasks call `feed_tick` without
  borrowing the `Swarm`.
- **Examples (7 total)**:
  - `historical_bootstrap` — single-pair history replay, ring-buffer inspection
  - `live_pubsub` — scripted feed + user listener subscribed to the alert bus
  - `triangular_arbitrage` — full triangle, fires +45.25 bps and −72.06 bps signals
  - `supervised_swarm` — kameo `OneForOne` supervisor with `restart_limit(3, 5s)` + oneshot ready handshake (mirror of `sdb_server.rs` pattern)
  - `databento_historical` *(feature `databento`)* — decode bundled `.dbn.zst`, pump asap
  - `databento_live_replay` *(feature `databento`)* — `spawn_dbn_pump` on `swarm.task_tracker()` with `Pacing::Realtime`
  - `distributed_alert_consumer` *(feature `remote`)* — single binary with `ROLE=producer` / `ROLE=consumer`; libp2p + mDNS discovery; remote `PollOpportunities` ask via wire
- **Databento adapter** (`subsystems::databento`, feature-gated):
  - `Pacing::{Asap, Realtime { speed_factor }}`
  - `SymbolMapper = Arc<dyn Fn(u32, Option<&str>) -> Option<Pair> + Send + Sync>`
  - `pump_dbn_file(feeder, path, mapper, pacing, token) -> Result<PumpStats>`
  - `spawn_dbn_pump(swarm, path, mapper, pacing) -> JoinHandle<...>` (uses `swarm.task_tracker()` + child cancellation token)
  - `mbp1_to_tick` — fixed-point + nanos→ms conversion
- **Remote alert gateway** (`subsystems::distributed`, feature `remote`):
  - `RemoteAlertGateway` actor: `#[derive(Actor, RemoteActor)]`, locally subscribed to `alert_bus`, exposes `#[remote_message]` asks `PollOpportunities`, `PeekOpportunityCount`, `ClearOpportunities`.
  - `enable_remote_alerts(&swarm, RemoteConfig) -> Result<RemoteHandle>` builds libp2p swarm (`tcp` + `noise` + `yamux` + `mdns` + `kameo::remote::Behaviour`), calls `init_global()`, listens, spawns the event loop on `swarm.task_tracker()` with a child cancellation token, registers the gateway.
  - `ArbitrageOpportunity`, `Triangle`, `Pair`, `Quote`, `Direction`, `Tick` all derive `Serialize + Deserialize` so the type doubles as the wire payload (POC trade-off — production would split this).
  - Hot path (monitor → coordinator → sink) deliberately stays on local mpsc; libp2p is only at the publish-to-outside-world boundary. Architectural rationale in §"Worth flagging" entry 8.
- **Tests passing**:
  | Suite | Tests | Command |
  |---|---|---|
  | Default | 6 unit + 5 integration | `cargo test` |
  | `--features databento` | + 4 databento integration | `cargo test --features databento` |
  | `--features remote` | + 1 remote integration | `cargo test --features remote` |
  | All 7 examples | exit 0 | see Reproducers below |

### File inventory

```
koalisi/
├── Cargo.toml                              path deps: kameo, git dep: hypergraph v4.2.0
├── README.md                               user-facing
├── CLAUDE.md                               THIS FILE
├── config/{default,development,test}.toml  coalition threshold, history capacity, delivery
├── src/
│   ├── lib.rs                              module surface + re-exports
│   ├── main.rs                             daemon binary with scripted live feed
│   ├── market.rs                           Pair/Tick/Quote/Triangle/TickUpdate/Opportunity + 6 unit tests
│   ├── core/
│   │   ├── mod.rs                          re-exports
│   │   ├── config.rs                       Settings + CoalitionSettings + setup_logging
│   │   └── runtime.rs                      CoalitionRuntime (TaskTracker + CancellationToken)
│   ├── topology/
│   │   ├── mod.rs                          re-exports + hypergraph type re-exports
│   │   ├── timestamp.rs                    Timestamp, TimeRange, Clock + 8 unit tests
│   │   ├── events.rs                       TemporalEvent (13 variants), EventStats
│   │   ├── event_log.rs                    EventLog with BTreeMap time + HashMap entity indices
│   │   ├── errors.rs                       TemporalError, TemporalResult
│   │   ├── temporal.rs                     TemporalHypergraph<V, HE>, SharedGraph, Snapshot
│   │   ├── queries.rs                      TemporalQueries (point-in-time state)
│   │   ├── analytics.rs                    TemporalAnalytics, GraphDelta
│   │   ├── coalitions.rs                   CoalitionManager (form/join/leave/dissolve/merge)
│   │   └── executor.rs                     HypergraphExecutor (rayon↔tokio bridge)
│   ├── algorithms/
│   │   ├── mod.rs                          AgentCapabilities trait + re-exports
│   │   ├── value_calculation.rs            ValueCalculator + 4 calculators
│   │   ├── dcvc.rs                         DCVCDistributor, WorkloadShare
│   │   └── aipa.rs                         Integer partitions, bounds, best-partition + 10 unit tests
│   └── subsystems/
│       ├── monitor.rs                      MarketMonitor (Tick, GetSnapshot, Ping)
│       ├── coordinator.rs                  ArbitrageCoordinator (TickUpdate, GetQuotes, Ping)
│       ├── sink.rs                         AlertSink (ArbitrageOpportunity, GetAlerts, DrainAlerts, Ping)
│       ├── swarm.rs                        Swarm (wraps CoalitionRuntime) + SwarmConfig + SwarmFeeder
│       ├── databento.rs                    DBN adapter (feature `databento`)
│       └── distributed.rs                  RemoteAlertGateway + libp2p wiring (feature `remote`)
├── examples/
│   ├── topology_coalition.rs               coalition lifecycle + time-travel queries
│   ├── algorithm_values.rs                 value calculators + DCVC + AIPA
│   ├── historical_bootstrap.rs             single-pair history replay
│   ├── live_pubsub.rs                      scripted feed + user listener
│   ├── triangular_arbitrage.rs             full triangle, fires arb signals
│   ├── supervised_swarm.rs                 kameo supervisor + restart
│   ├── databento_historical.rs             (feature `databento`)
│   ├── databento_live_replay.rs            (feature `databento`)
│   └── distributed_alert_consumer.rs       (feature `remote`)
└── tests/
    ├── topology_test.rs                    11 tests
    ├── algorithms_test.rs                  15 tests
    ├── integration_test.rs                 5 tests (forex)
    ├── databento_integration.rs            4 tests (feature `databento`)
    └── remote_integration.rs               1 test (feature `remote`)
```

## Worth flagging (gotchas)

These cost time during the build; future-me should not relearn them.

1. **kameo supervised actors keep the same `ActorId` across restart.**
   - In `examples/supervised_swarm.rs` the original monitor (id=#2) panics, gets restarted, and the NEW actor is also id=#2.
   - `monitor.wait_for_shutdown()` therefore HANGS on a supervised actor — from the ref's perspective the actor only blips down and back up; "shutdown" never finalises.
   - Workaround: after `tell(ForcePanic)`, do a brief sleep + `supervisor.ask(Ping)` to confirm the supervisor is alive. The original `monitor` ref still works against the restarted instance because the id is preserved.

2. **`anyhow::Context` shadows `kameo::Context`.**
   - kameo's prelude exports `Context<Self, Reply>` (the actor handler parameter type).
   - anyhow exports a `Context` trait for `.context("…")?` on `Result`.
   - `use anyhow::{Context, ...}` + `use kameo::prelude::*` → compile error "expected a type, found a trait" on `_: &mut Context<Self, Self::Reply>`.
   - Fix: `use anyhow::Context as _;` brings the trait into scope for the extension method without binding the name.
   - Applied in: `examples/databento_live_replay.rs`. Watch for it whenever both crates appear in the same example.

3. **Bundled DBN test fixture is futures, not forex.**
   - `../trade/databento-rs/tests/data/test_data.mbp-1.dbn.zst` = 2 records, `instrument_id=5482`, symbol `ESH1` (E-mini S&P 500 March 2021), prices ≈ $3720.38.
   - Sufficient to verify the adapter pipeline; insufficient to fire triangular arb signals (only one leg).
   - The `SymbolMapper` signature is `Fn(u32, Option<&str>) -> Option<Pair>` so production users can swap in a forex-bearing DBN file with their own mapping.

4. **Path dependencies on kameo.**
   - `Cargo.toml` references `../../agentics/kameo` and `../../agentics/kameo/actors`.
   - This breaks if either repo moves. When the upstream `kameo 0.20.0` stabilises on crates.io with the API we're using, switch to a version dep.

5. **DBN file discovery for examples.**
   - Examples and tests probe these paths in order:
     1. `$DBN_TEST_PATH` (if set + exists)
     2. `../trade/databento-rs/tests/data/test_data.mbp-1.dbn.zst`
     3. `../../trade/databento-rs/tests/data/test_data.mbp-1.dbn.zst`
     4. `../databento-rs/tests/data/test_data.mbp-1.dbn.zst`
   - Tests "skip with diagnostic" (print + pass) if not found, so CI without the file still goes green.

6. **PubSub `Subscribe` requires the subscriber to be alive.**
   - `swarm.alert_bus().ask(Subscribe(listener))` panics/errors if `listener` hasn't been spawned yet.
   - All current examples spawn the listener first, then subscribe — keep that order.

7. **Cargo target dir + timeout convention (project-wide).**
   - We use `--manifest-path Cargo.toml --target-dir /tmp/forex-arbitrage-swarm-target` to avoid contention with Zed's own `cargo check`. Run from inside the `forex-arbitrage-swarm` worktree.
   - Wrap with `timeout 30s` (or 60s, 120s as appropriate) so a hang in a freshly-built binary is killed cleanly, not just the shell wrapper.
   - Pattern: `timeout 30s cargo run --manifest-path Cargo.toml --target-dir /tmp/… --example foo 2>/dev/null ; echo "exit=$?"`. Exit 124 = unix `timeout` fired.

8. **libp2p remote actor RPC: hybrid, NOT hot-path.**
   - The `remote` feature is intentionally a *publish-to-outside-world* boundary, NOT a replacement for the local mpsc hot path.
   - Rationale: local `ask` is sub-μs (see `kameo/benches/overhead.rs`); `remote::ask` adds rmp-serde encode/decode + libp2p `request-response` over yamux + noise + network I/O — ≅10μs loopback, ≅10µs–1ms real network. Default `remote::messaging::Config::request_timeout` is **10 seconds**, sized for the network, not for actor-internal calls.
   - Trait-bound asymmetry: local needs `Send + 'static`. Remote needs `Send + 'static + Serialize + DeserializeOwned` on messages, `Serialize` on `Reply::Ok` and `Reply::Error`, `#[derive(RemoteActor)]` on the actor, `#[remote_message]` on each exposed handler. Strictly more, not less.
   - The hybrid we settled on: `MarketMonitor` → `tick_bus` → `Coordinator` → `alert_bus` → `AlertSink` is all local mpsc. A separate `RemoteAlertGateway` actor subscribes to `alert_bus` AND is remote-registered. Off-process consumers (`RemoteActorRef::<RemoteAlertGateway>::lookup(name).ask(&PollOpportunities)`) get alerts without ever touching the hot path.

9. **`kameo::remote::Behaviour::init_global()` is process-wide.**
   - Called once inside `enable_remote_alerts`. Calling it twice in the same process (e.g., from two integration tests in a single binary) will conflict.
   - For now: one `remote_integration` test only. Future remote tests need to share the libp2p swarm, OR use `serial_test` + tear-down hooks.

10. **`ActorRef::register` returns `Result<(), RegistryError>`; with the `remote` feature it becomes `async`.**
    - Signature is `register(impl Into<Arc<str>>)`. Passing `&String` doesn't work — `&String` does not impl `Into<Arc<str>>`. Pass `&str` (via `.as_str()` or `&literal`).
    - Without `remote`: sync, just returns `Result<(), RegistryError>` (no `.await`).
    - With `remote`: returns a future that resolves once libp2p propagates the registration.

11. **libp2p `#[derive(NetworkBehaviour)]` requires the `macros` feature.**
    - kameo enables libp2p with `cbor, kad, noise, mdns, quic, request-response, tcp, tokio, yamux`, but NOT `macros`. Our `Cargo.toml` adds libp2p directly with `macros` so `SwarmBehaviour` derive works.
    - Generated event-enum naming: `#[derive(NetworkBehaviour)] struct SwarmBehaviour {...}` produces `SwarmBehaviourEvent`. Match on `SwarmEvent::Behaviour(SwarmBehaviourEvent::Mdns(…))`.

## Reproducers

All assume `cwd = koalisi/`.

```sh
# === default features (57 tests) ===
timeout 60s  cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target
timeout 30s  cargo run  --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --example topology_coalition
timeout 30s  cargo run  --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --example algorithm_values
timeout 30s  cargo run  --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --example triangular_arbitrage
timeout 30s  cargo run  --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --example historical_bootstrap
timeout 30s  cargo run  --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --example live_pubsub
timeout 30s  cargo run  --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --example supervised_swarm

# === with databento feature ===
timeout 120s cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features databento
timeout 30s  cargo run  --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features databento --example databento_historical
timeout 30s  cargo run  --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features databento --example databento_live_replay

# === with remote feature ===
timeout 60s  cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features remote
ROLE=producer timeout 60s cargo run --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features remote --example distributed_alert_consumer
ROLE=consumer timeout 60s cargo run --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features remote --example distributed_alert_consumer
```

## Next steps

> **GATE — 2026-05-27.** Two additional design inputs are pending from
> the user before phases 5 and 6 begin implementation. Do NOT start
> Phase 5 or Phase 6 work until those inputs are recorded here and the
> gate is lifted. The current Phase 5/6 entries below are the *prior*
> plan and may be revised once the new inputs land.

### Phase 5: Persistence  *(planned — gated, see above)*

Feature-gated persistence using hypergraph v4.2.0's `PersistentHypergraph`
for graph state plus an `EventStore` trait for temporal event durability.
Default impl: append-only file log with `rmp-serde`. See `.claude/plans/`
for full design.

### Phase 6: Decision layer (Active Inference)  *(planned — gated, see above)*

Feature-gated (`decision`) port of coalition_aif's EFE calculator,
BeliefState, CoalitionBelief. Adds `ndarray` dependency. Most experimental.

### Downstream: nautilus_trader bridge  *(separate project)*

IB adapter patterns from the nautilus_trader glean analysis inform a
separate `koalisi-nautilus` bridge project. Not a koalisi feature.

### Downstream: tauhokohoko integration  *(separate project)*

Salmon Prisoner's Dilemma simulator using koalisi's coalition primitives
with deep_causality's CSM/EPP/Teloid/Uncertain layers. See
`~/Documents/tauhokohoko/tauhokohoko/requirements/causal-context-architecture.md`.

### Legacy: Databento `LiveClient` integration  **(blocked: needs `DATABENTO_API_KEY`)**

The `LiveClient` is databento's real-time websocket-style subscriber.
Same kameo wiring as the DBN-file adapter — only the source changes.

**Pre-req**: obtain an API key from <https://databento.com/portal/keys>,
export `DATABENTO_API_KEY=…` (32 ASCII chars, validated by
`databento::ApiKey::new`).

**Plan**:

1. **Cargo.toml**:
   - Add `databento = { version = "0.51", default-features = false, features = ["live"], optional = true }`.
     The `live` feature pulls `tokio/net` + `tokio/time` and nothing else
     (no reqwest, no zstd) — much lighter than `historical`.
   - Either extend the existing `databento` feature OR add a separate
     `databento-live` feature. Recommend separate so the file adapter
     stays buildable without network deps.
2. **Module**: `src/subsystems/databento_live.rs` (new sibling of
   `databento.rs`).
   - `pub async fn pump_live(feeder: SwarmFeeder, params: LiveParams,
     mapper: SymbolMapper, token: CancellationToken) -> Result<PumpStats>`
   - `pub fn spawn_live_pump(swarm: &Swarm, …) -> JoinHandle<Result<PumpStats>>`
     — same TaskTracker + child token pattern as `spawn_dbn_pump`.
   - `LiveParams { api_key: Option<String>, dataset: Dataset, symbols:
     Vec<String>, schema: Schema, stype_in: SType }`. When `api_key` is
     `None`, fall back to `LiveClient::builder().key_from_env()`.
3. **Loop shape**:
   ```rust
   let mut client = LiveClient::builder()
       .key_from_env()?.dataset(params.dataset).build().await?;
   client.subscribe(Subscription::builder()
       .symbols(params.symbols).schema(Schema::Mbp1)
       .stype_in(params.stype_in).build()).await?;
   client.start().await?;

   let mut symbol_map = PitSymbolMap::new();
   loop {
       tokio::select! {
           biased;
           _ = token.cancelled() => break,
           rec = client.next_record() => match rec? {
               Some(rec) => {
                   symbol_map.on_record(rec)?;
                   if let Some(msg) = rec.get::<Mbp1Msg>() {
                       let sym = symbol_map.get(msg.hd.instrument_id);
                       let Some(pair) = mapper(msg.hd.instrument_id,
                           sym.map(String::as_str)) else { continue; };
                       feeder.feed_tick(mbp1_to_tick(msg, pair)).await?;
                   }
               }
               None => break,
           }
       }
   }
   ```
   Note: `PitSymbolMap` (live-style, updated inline) replaces the
   `symbol_map_for_date` used by the file adapter, because live streams
   emit `SymbolMappingMsg` records inline rather than carrying them in
   metadata.
4. **Example**: `examples/databento_live.rs` (required-features
   `["databento-live"]`). Should:
   - Read `DATABENTO_API_KEY` from env, error clearly if missing.
   - Subscribe to a forex-bearing dataset (whichever is on the API key's
     allowlist — historically forex is on `IFEU.IMPACT` or similar; check
     `databento.com/docs/standards-and-conventions/list-of-datasets`).
   - Wire `spawn_live_pump` onto a swarm with appropriate triangles.
   - Bound by `tokio::signal::ctrl_c()` or a `Duration` cap so the demo
     doesn't run forever.
5. **Integration test**: probably none (real network). Could mock the
   websocket but it's a lot of work for a POC. Skip; rely on the file
   adapter test for the `Mbp1Msg → Tick` correctness.

**Mapping that exists upstream**: `databento-rs/examples/live.rs` shows
the canonical loop shape — use it as template.

**Symbol mapper considerations**: live streams emit
`SymbolMappingMsg` records inline. Either:
- Use `decode_record_ref()` + `PitSymbolMap::on_record` to track them
  dynamically (preferred — matches `examples/live.rs` upstream), OR
- Take a hardcoded `HashMap<u32, Pair>` from the caller (simpler but
  inflexible).

### B. Synthetic DBN file for end-to-end arb signal demo  *(unblocked)*

The bundled fixture isn't forex and only has 2 records, so the existing
databento examples can't show the full "DBN decode → triangle arb fires"
path. Generate a synthetic file at runtime.

**Plan**:

1. **Module**: `src/subsystems/databento_synthetic.rs` (gated by feature
   `databento`, alongside `databento.rs`).
   - `pub fn synthesize_triangle_file(path: &Path, scenario: &TriangleScenario) -> Result<()>`
   - `TriangleScenario { pairs: [Pair; 3], aligned_ticks: usize, dislocate_at: usize, dislocation_bps: f64, start_ts_nanos: u64, tick_interval_ms: u64 }`
   - Builds a `dbn::Metadata` with three symbol mappings (one per pair,
     instrument_ids 1/2/3), then writes alternating `Mbp1Msg` records
     using `dbn::encode::AsyncDbnEncoder::with_zstd(file, &metadata)`.
2. **Example**: `examples/databento_synthetic_arbitrage.rs`:
   ```text
   1. tmpfile = /tmp/forex-arb-{pid}.dbn.zst
   2. synthesize_triangle_file(tmpfile, TriangleScenario { ... }) creating
      a sequence: 50 aligned ticks + 1 dislocated cross + 50 aligned
   3. spawn_dbn_pump(swarm, tmpfile, mapper, Pacing::Asap)
   4. await pump completion, assert 1 arb opportunity fired, print it
   ```
3. **Mapper**: trivial since we control the symbols — `match symbol {
   Some("EURUSD") => Some(p("EUR/USD")), ... }`.
4. **Test**: a new integration test in `tests/databento_integration.rs`
   that synthesizes, pumps, asserts the arb. This becomes the
   "end-to-end DBN demo with real arb signal" the bundled fixture can't
   show.

**dbn encoder reference**: see `examples/split_symbols.rs` in
`databento-rs` for `AsyncDbnEncoder::with_zstd` usage; also
`dbn::MetadataBuilder` for constructing the Metadata header.

**Subtle bits**:
- `Metadata::start` should be set to the first record's `ts_recv` so
  `symbol_map_for_date` works (the file adapter relies on this — see
  the `pump_decoder` symbol map fallback logic).
- Symbol mappings live in `metadata.mappings: Vec<SymbolMapping>`,
  built via `MetadataBuilder::mappings(...)`. Each `SymbolMapping`
  needs a date range covering the records' timestamps.

### C. Remote gateway hardening  *(unblocked, low priority)*
- see /home/oryx/Documents/category/deep_causality/examples/avionics_examples/flight_envelope_monitor/README.md for a template for trade_envelope_monitor
The POC remote gateway works (round-trip integration test green) but is
minimal. To make it production-shaped:

1. **Bounded buffer + size cap.** Replace `Vec<ArbitrageOpportunity>` in
   `RemoteAlertGateway` with a `VecDeque` capped at e.g. `1024`; oldest
   evicted on push. Consumers that lag forever should not OOM the producer.
2. **Sequence numbers + cursor-based polling.** Add a monotonic `seq:
   u64` field to each buffered alert; replace `PollOpportunities` with
   `PollSince { last_seq: u64 } -> Vec<(u64, ArbitrageOpportunity)>`.
   Consumers remember the last seq they saw and only fetch deltas.
3. **Stable wire schema.** `ArbitrageOpportunity` is currently the wire
   type; this couples on-wire format to internal struct changes. Define a
   `RemoteArbitrageOpportunityV1` in `distributed.rs` and convert at the
   boundary.
4. **Multiple gateways under one libp2p swarm.** `init_global()` is
   one-shot per process; future work may want both an alert gateway AND,
   say, a tick gateway. Factor the libp2p swarm setup out of
   `enable_remote_alerts` so the same swarm hosts multiple registered
   actors.
5. **QUIC transport in addition to TCP.** `custom_swarm.rs` shows
   `.with_quic()` as an additional transport. Useful for higher-RTT
   links. Cheap to add.

### D. Smaller nice-to-haves (optional)

- **Metrics example** — subscribe a `metrics::Counter`-driven actor to
  both pubsubs, scrape via `metrics-exporter-prometheus`. Requires
  kameo's `metrics` feature.
- **Multi-triangle stress test** — verify `coordinator.triangles: Vec<Triangle>`
  scales: 10 triangles, 30 unique pairs, all sharing some legs. Adversarial
  pricing that flips multiple triangles at once.
- **Bid/ask-aware execution model** — replace mid-price arithmetic with
  realistic execution costs (you cross the spread on each leg). Triangle
  `edge_bps` becomes signed by direction and net of spread.
- **Switch off path deps when kameo 0.20.0 is on crates.io** — the API
  surface we use is stable (we now use both `remote` and default features).

## Open questions (jot anything here as it comes up)

- Whether the `databento` feature should split into `databento-file` and
  `databento-live` so users who only want the file adapter don't pull
  `tokio/net`. Currently they don't anyway (file adapter uses only
  `dbn` + `tokio/fs`/`io-util`); revisit if `LiveClient` integration
  adds significant deps.
- Whether the coordinator's hysteresis state should be per-direction
  (so a +45 bps fire doesn't suppress a −45 bps fire on the same
  triangle without an intervening realignment). Currently it's a single
  bool per triangle — the `triangular_arbitrage` example exercises the
  "realign + flip" path and works, but the semantic is debatable.
