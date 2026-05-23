# forex-arbitrage-swarm

A POC swarm of [kameo](https://github.com/tqwewe/kameo) actors that detects
triangular forex-arbitrage opportunities. Each agent in the swarm watches a
specific currency pair; collectively they fire `ArbitrageOpportunity` events
when the synthetic cross-rate (built from two USD-quoted legs) diverges from
the actual cross by more than a configured basis-point threshold.

No persistence yet — everything lives in mpsc mailboxes and pubsub
broadcasts under kameo. A SurrealDB / message-store layer can be folded in
later without changing the actor topology.

## Architecture

```
┌──────────────┐  Tick (per pair)  ┌──────────────────┐
│ feeders      ├──────────────────▶│ MarketMonitor    │  (one per pair)
│ (hist/live)  │                   │ - ring buffer    │
└──────────────┘                   │ - latest quote   │
                                   └────────┬─────────┘
                                            │ Publish(TickUpdate)
                                            ▼
                                  ┌──────────────────────┐
                                  │ PubSub<TickUpdate>   │
                                  └────────┬─────────────┘
                                           │
                                           ▼
                          ┌────────────────────────────────┐
                          │ ArbitrageCoordinator           │
                          │  - tracks latest quote/pair    │
                          │  - walks all triangles         │
                          │  - hysteresis-based firing     │
                          └────────────┬───────────────────┘
                                       │ Publish(ArbitrageOpportunity)
                                       ▼
                            ┌──────────────────────────────┐
                            │ PubSub<ArbitrageOpportunity> │
                            └────────────┬─────────────────┘
                                         ▼
                                ┌──────────────────┐
                                │ AlertSink (+ any │
                                │ user listeners)  │
                                └──────────────────┘
```

`Swarm` is the top-level container. It owns all actor refs, both pubsubs, a
`TaskTracker`, and a root `CancellationToken` (the same library-first
lifecycle shape as `surrealdb-live-message`'s `Coalition<T>`).

### Why kameo

- **Mailbox-mediated state**: each `MarketMonitor` mutates its own ring buffer
  serially via the kameo mpsc — no `Arc<Mutex<...>>` for the per-pair view.
- **Pubsub fan-out**: `kameo_actors::pubsub::PubSub<M>` is a typed broadcast
  channel that fits the "monitor → coordinator" and "coordinator → sink"
  hops without us hand-rolling routing.
- **Supervision**: the `supervised_swarm` example uses
  `MarketMonitor::supervise(...)` so a panicking handler is restarted by
  the swarm rather than tearing down the whole binary.
- **Deterministic flush primitive**: every actor exposes a `Ping` ask. With
  `DeliveryStrategy::Guaranteed`, the FIFO mailbox + `ask` chain lets the
  integration test verify alerts without any `sleep`s.

## Layout

```
src/
├── lib.rs                    — public re-exports
├── main.rs                   — daemon: build swarm + scripted live feed + ctrl-c
├── logger.rs                 — tracing setup
├── settings.rs               — config-driven settings
├── market.rs                 — value types (Pair, Tick, Quote, Triangle, TickUpdate, ArbitrageOpportunity)
└── subsystems/
    ├── monitor.rs            — MarketMonitor actor
    ├── coordinator.rs        — ArbitrageCoordinator actor
    ├── sink.rs               — AlertSink actor
    ├── swarm.rs              — Swarm orchestrator (Coalition-shaped) + SwarmFeeder
    └── databento.rs          — DBN-file adapter (feature `databento`)

examples/
├── historical_bootstrap.rs        — feed one pair's history, inspect the monitor
├── live_pubsub.rs                 — scripted live feed + custom listener actor
├── triangular_arbitrage.rs        — full triangle, observe firing + hysteresis
├── supervised_swarm.rs            — kameo OneForOne supervision restarts a panicking monitor
├── databento_historical.rs        — (feature: databento) decode a `.dbn.zst` file, pump asap
└── databento_live_replay.rs       — (feature: databento) spawn the pump on `swarm.task_tracker()`
                                    with realtime pacing

tests/
├── integration_test.rs            — end-to-end alert wiring, hysteresis, ring-buffer eviction
└── databento_integration.rs       — (feature: databento) Mbp1→Tick conversion,
                                    file decode, cancellation
```

## Run

```sh
# the four core examples
cargo run --example historical_bootstrap
cargo run --example live_pubsub
cargo run --example triangular_arbitrage
cargo run --example supervised_swarm

# the databento DBN-file adapter examples
cargo run --features databento --example databento_historical
cargo run --features databento --example databento_live_replay

# unit + integration tests (`--features databento` adds the adapter suite)
cargo test
cargo test --features databento

# the daemon (scripted EUR/USD-GBP/USD-EUR/GBP feed; ctrl-c to stop)
cargo run
```

## Configuration

`config/default.toml` controls the cross-cutting knobs:

```toml
[swarm]
threshold_bps = 5.0           # opportunity fires when |edge| > threshold
history_capacity = 1024       # per-monitor ring buffer
delivery_strategy = "guaranteed"  # "guaranteed" | "best_effort"
```

`RUN_MODE=test cargo test` loads `config/test.toml` on top of the default,
mirroring `surrealdb-live-message`'s settings pattern.

## What the integration test verifies

`tests/integration_test.rs`:

1. **Aligned market → no alerts.** EUR/USD=1.10, GBP/USD=1.30, EUR/GBP at
   synthetic — the coordinator stays silent.
2. **Cross dislocates to 0.85 → exactly one ~45 bps alert** with
   `direction = BuySyntheticSellActual` and `detected_at_ms` set to the
   triggering tick's timestamp.
3. **Hysteresis.** Feeding the same dislocated cross again does not fire a
   second alert — the triangle is in its "fired" state until it drops back
   below the threshold.
4. **Rearm + flip.** Re-aligning the cross rearms; a dislocation in the
   opposite direction fires a second, oppositely-signed alert.
5. **Ring-buffer eviction.** Feeding 12 ticks into a 5-capacity monitor
   leaves only the most recent 5 in the snapshot.
6. **API guards.** Empty triangles are rejected at construction; unknown
   pairs are rejected at `feed_tick`.

## Databento adapter (optional feature: `databento`)

The `subsystems::databento` module decodes a `.dbn` / `.dbn.zst` file from
Databento's ecosystem and pumps the MBP-1 top-of-book records into the
swarm via a clone-able [`SwarmFeeder`].

```rust,ignore
use forex_arbitrage_swarm::subsystems::databento::{
    mapper_from_fn, spawn_dbn_pump, Pacing,
};

let mapper = mapper_from_fn(|instrument_id, symbol| match symbol {
    Some("ESH1") => Some("EUR/USD".parse().unwrap()),
    _ => None,
});

// Spawns on `swarm.task_tracker()` with a child of `swarm.cancellation_token()` —
// `Swarm::shutdown()` will cancel + drain it.
let handle = spawn_dbn_pump(&swarm, "data.dbn.zst", mapper, Pacing::Realtime {
    speed_factor: 100.0,
});
let stats = handle.await??;
println!("{stats:?}");
```

### Pacing

- `Pacing::Asap` — pump every record as fast as the decoder produces it
  (historical bootstrap path).
- `Pacing::Realtime { speed_factor }` — pace records by their `ts_recv`
  deltas; `speed_factor` accelerates/decelerates the playback. Non-positive
  values behave as `Asap`.

### Test data

The `examples/databento_*` examples look for
`../trade/databento-rs/tests/data/test_data.mbp-1.dbn.zst` (Databento's
bundled 2-record decoder fixture, which happens to contain `ESH1` E-mini
S&P futures — not forex). Override the location with `DBN_TEST_PATH=...`.
The fixture is sufficient to verify the decode → mapper → swarm path; for
actual arbitrage detection you'd supply your own forex-bearing DBN file.

## Next steps (not in this POC)

- Real price feeds (websocket subscribers wired into `swarm.feed_tick(...)`,
  or `databento::LiveClient` swapped in alongside the file adapter).
- Persistence — replace the per-monitor ring buffer with a SurrealDB
  live-query subscription, OR keep the ring buffer and add a separate
  archival actor that drains the tick pubsub into storage.
- More triangles (USD-, EUR-, JPY-quoted families) — the coordinator
  already walks `Vec<Triangle>`.
- Move from mid-price arithmetic to bid/ask-aware execution-cost modeling.
