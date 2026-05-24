# Changelog

All notable changes to **forex-arbitrage-swarm** will be documented in
this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

Planned (tracked in [`CLAUDE.md`](./CLAUDE.md) §"Next steps"):

- **Databento `LiveClient` integration** *(blocked on `DATABENTO_API_KEY`)* —
  real-time websocket source via `databento::LiveClient`, same kameo
  wiring as the existing DBN-file adapter. New module
  `subsystems::databento_live`, `spawn_live_pump` on the swarm's
  `TaskTracker`, `examples/databento_live.rs`.
- **Synthetic DBN file for end-to-end arb signal** — generate a
  forex-shaped `.dbn.zst` at runtime via `dbn::encode::AsyncDbnEncoder`
  so the file adapter can demonstrate the full "DBN decode → triangle arb
  fires" path (the bundled `test_data.mbp-1.dbn.zst` is too small).
- **Remote gateway hardening** — bounded `VecDeque` buffer with eviction
  cap; cursor-based `PollSince { last_seq }` semantics; stable
  `RemoteArbitrageOpportunityV1` wire schema separate from the local
  `ArbitrageOpportunity`; multiple gateways under one libp2p swarm; QUIC
  transport alongside TCP.
- **Nice-to-haves** — `metrics`/Prometheus example, multi-triangle stress
  test, bid/ask-aware execution-cost modelling, switch off path
  dependencies when `kameo 0.20.0` lands on crates.io.

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
[0.3.0]: #030--2026-05-24
[0.2.0]: #020--2026-05-23
[0.1.0]: #010--2026-05-23
