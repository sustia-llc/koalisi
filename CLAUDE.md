# CLAUDE.md — forex-arbitrage-swarm

Working-state document for the POC. See `README.md` for the user-facing
description; this file is for picking the project back up later.

## Mission (one paragraph)

A kameo-actor swarm POC for triangular forex arbitrage. Per-pair
`MarketMonitor` actors publish ticks into a `PubSub<TickUpdate>`; a single
`ArbitrageCoordinator` subscribes, walks `Vec<Triangle>`, fires
`ArbitrageOpportunity` events through a second pubsub on hysteresis-
guarded threshold crosses; an `AlertSink` (plus optional user listeners)
consume the alerts. The top-level `Swarm` is Coalition-shaped — it owns
the actor refs, a `TaskTracker`, and a root `CancellationToken`, with the
library-first three-step shutdown (`cancel → close → drain`) mirrored
from `surrealdb-live-message`. No persistence layer (per project
constraint).

## Current state — 2026-05-23

### Done

- **Core swarm**: `MarketMonitor` × N + `ArbitrageCoordinator` +
  `AlertSink` + 2 `PubSub`s wired by `Swarm::new`, all under a single
  `TaskTracker` + `CancellationToken`.
- **`SwarmFeeder`** — clone-able feed handle (owns only the monitor
  `ActorRef` map). Lets background tasks call `feed_tick` without
  borrowing the `Swarm`.
- **Examples (6 total)**:
  - `historical_bootstrap` — single-pair history replay, ring-buffer inspection
  - `live_pubsub` — scripted feed + user listener subscribed to the alert bus
  - `triangular_arbitrage` — full triangle, fires +45.25 bps and −72.06 bps signals
  - `supervised_swarm` — kameo `OneForOne` supervisor with `restart_limit(3, 5s)` + oneshot ready handshake (mirror of `sdb_server.rs` pattern)
  - `databento_historical` *(feature `databento`)* — decode bundled `.dbn.zst`, pump asap
  - `databento_live_replay` *(feature `databento`)* — `spawn_dbn_pump` on `swarm.task_tracker()` with `Pacing::Realtime`
- **Databento adapter** (`subsystems::databento`, feature-gated):
  - `Pacing::{Asap, Realtime { speed_factor }}`
  - `SymbolMapper = Arc<dyn Fn(u32, Option<&str>) -> Option<Pair> + Send + Sync>`
  - `pump_dbn_file(feeder, path, mapper, pacing, token) -> Result<PumpStats>`
  - `spawn_dbn_pump(swarm, path, mapper, pacing) -> JoinHandle<...>` (uses `swarm.task_tracker()` + child cancellation token)
  - `mbp1_to_tick` — fixed-point + nanos→ms conversion
- **Tests passing**:
  | Suite | Tests | Command |
  |---|---|---|
  | Default | 6 unit + 5 integration | `cargo test` |
  | `--features databento` | + 4 databento integration | `cargo test --features databento` |
  | All 6 examples | exit 0 | see Reproducers below |

### File inventory

```
forex-arbitrage-swarm/
├── Cargo.toml                              path deps on ../../agentics/kameo{,/actors}
├── README.md                               user-facing
├── CLAUDE.md                               THIS FILE
├── config/{default,development,test}.toml  swarm threshold, history capacity, delivery
├── src/
│   ├── lib.rs                              module surface + re-exports
│   ├── main.rs                             daemon binary with scripted live feed
│   ├── logger.rs                           idempotent tracing_subscriber setup
│   ├── settings.rs                         config crate, env override
│   ├── market.rs                           Pair/Tick/Quote/Triangle/TickUpdate/Opportunity + 6 unit tests
│   └── subsystems/
│       ├── monitor.rs                      MarketMonitor (Tick, GetSnapshot, Ping)
│       ├── coordinator.rs                  ArbitrageCoordinator (TickUpdate, GetQuotes, Ping) + hysteresis
│       ├── sink.rs                         AlertSink (ArbitrageOpportunity, GetAlerts, DrainAlerts, Ping)
│       ├── swarm.rs                        Swarm + SwarmConfig + SwarmFeeder
│       └── databento.rs                    DBN adapter (gated)
├── examples/
│   ├── historical_bootstrap.rs
│   ├── live_pubsub.rs
│   ├── triangular_arbitrage.rs
│   ├── supervised_swarm.rs
│   ├── databento_historical.rs             (required-features = ["databento"])
│   └── databento_live_replay.rs            (required-features = ["databento"])
└── tests/
    ├── integration_test.rs                 5 tests
    └── databento_integration.rs            4 tests (required-features = ["databento"])
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
   - We use `--manifest-path forex-arbitrage-swarm/Cargo.toml --target-dir /tmp/forex-arbitrage-swarm-target` to avoid contention with Zed's own `cargo check`.
   - Wrap with `timeout 30s` (or 60s, 120s as appropriate) so a hang in a freshly-built binary is killed cleanly, not just the shell wrapper.
   - Pattern: `timeout 30s cargo run --manifest-path … --target-dir … --example foo 2>/dev/null ; echo "exit=$?"`. Exit 124 = unix `timeout` fired.

## Reproducers

The canonical commands. All assume `cd sustia-llc`.

```sh
# === default features ===
timeout 60s  cargo test --manifest-path forex-arbitrage-swarm/Cargo.toml --target-dir /tmp/forex-arbitrage-swarm-target
timeout 30s  cargo run  --manifest-path forex-arbitrage-swarm/Cargo.toml --target-dir /tmp/forex-arbitrage-swarm-target --example historical_bootstrap
timeout 30s  cargo run  --manifest-path forex-arbitrage-swarm/Cargo.toml --target-dir /tmp/forex-arbitrage-swarm-target --example live_pubsub
timeout 30s  cargo run  --manifest-path forex-arbitrage-swarm/Cargo.toml --target-dir /tmp/forex-arbitrage-swarm-target --example triangular_arbitrage
timeout 30s  cargo run  --manifest-path forex-arbitrage-swarm/Cargo.toml --target-dir /tmp/forex-arbitrage-swarm-target --example supervised_swarm

# === with databento feature ===
timeout 120s cargo test --manifest-path forex-arbitrage-swarm/Cargo.toml --target-dir /tmp/forex-arbitrage-swarm-target --features databento
timeout 30s  cargo run  --manifest-path forex-arbitrage-swarm/Cargo.toml --target-dir /tmp/forex-arbitrage-swarm-target --features databento --example databento_historical
timeout 30s  cargo run  --manifest-path forex-arbitrage-swarm/Cargo.toml --target-dir /tmp/forex-arbitrage-swarm-target --features databento --example databento_live_replay
```

Expected outcomes documented in README §"What the integration test verifies"
and §"Test data" (databento section).

## Next steps

### A. Databento `LiveClient` integration  **(blocked: needs `DATABENTO_API_KEY`)**

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

### C. Smaller nice-to-haves (optional)

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
  surface we use is stable (no remote/metrics features needed).

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
