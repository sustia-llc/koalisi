# forex-arbitrage-swarm

A POC swarm of [kameo](https://github.com/tqwewe/kameo) actors that detects
triangular forex-arbitrage opportunities. Each agent in the swarm watches a
specific currency pair; collectively they fire `ArbitrageOpportunity` events
when the synthetic cross-rate (built from two USD-quoted legs) diverges from
the actual cross by more than a configured basis-point threshold.

No persistence yet — everything lives in mpsc mailboxes and pubsub
broadcasts under kameo. A SurrealDB / message-store layer can be folded in
later without changing the actor topology.

Three optional capabilities ship as cargo features on top of the core swarm:

| Feature | What it adds |
|---|---|
| *(none)* | Core actor swarm, 4 examples, 11 tests |
| `databento` | DBN file decoder + adapter — feed `.dbn.zst` historical files at `Asap` or `Realtime { speed_factor }` pacing |
| `remote` | libp2p-based remote alert gateway — register the swarm's alerts so other processes / machines can discover and pull them via `RemoteActorRef::<RemoteAlertGateway>::lookup(name)` |

Project state, gotchas, and detailed next-steps are tracked in
[`CLAUDE.md`](./CLAUDE.md); per-release changes in
[`CHANGELOG.md`](./CHANGELOG.md).

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
LOCAL MPSC                              ┌────────────────────────────────┐
HOT PATH                                │ ArbitrageCoordinator           │
(sub-μs)                                │  - tracks latest quote/pair    │
                                        │  - walks all triangles         │
                                        │  - hysteresis-based firing     │
                                        └────────────┬───────────────────┘
                                                     │ Publish(ArbitrageOpportunity)
                                                     ▼
                                          ┌──────────────────────────────┐
                                          │ PubSub<ArbitrageOpportunity> │
                                          └──┬────────────────────┬──────┘
                                             │                    │
                                             ▼                    ▼
                                  ┌──────────────────┐  ┌──────────────────────────────┐
                                  │ AlertSink (+ any │  │ RemoteAlertGateway           │
                                  │ user listeners)  │  │  (feature `remote` only)     │
                                  └──────────────────┘  └──────────┬───────────────────┘
                                                                   │ #[remote_message]
LIBP2P                                                             │ PollOpportunities
PUBLISH-TO-OUTSIDE-WORLD                                           │ PeekOpportunityCount
BOUNDARY (~10μs–1ms)                                               │ ClearOpportunities
                                                                   ▼
                                                       ┌────────────────────────────┐
                                                       │ Remote consumers           │
                                                       │ (other procs / machines):  │
                                                       │  RemoteActorRef::lookup    │
                                                       │  + ask(&PollOpportunities) │
                                                       └────────────────────────────┘
```

`Swarm` is the top-level container. It owns all actor refs, both pubsubs, a
`TaskTracker`, and a root `CancellationToken` (the same library-first
lifecycle shape as `surrealdb-live-message`'s `Coalition<T>`).

The split between LOCAL MPSC HOT PATH and LIBP2P PUBLISH-TO-OUTSIDE-WORLD
BOUNDARY is deliberate — see `CLAUDE.md` §"Worth flagging" #8 for the
latency / trait-bound rationale.

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
- **Composable libp2p**: `kameo::remote::Behaviour` is a
  `NetworkBehaviour` that drops into your own `#[derive(NetworkBehaviour)]`
  alongside `mdns`, `gossipsub`, etc. — the `remote` feature uses this to
  add cross-process RPC without owning the transport.

## Layout

```
src/
├── lib.rs                    — public re-exports
├── main.rs                   — daemon: build swarm + scripted live feed + ctrl-c
├── logger.rs                 — tracing setup
├── settings.rs               — config-driven settings
├── market.rs                 — value types (Pair, Tick, Quote, Triangle,
│                               TickUpdate, ArbitrageOpportunity, Direction)
└── subsystems/
    ├── monitor.rs            — MarketMonitor actor
    ├── coordinator.rs        — ArbitrageCoordinator actor
    ├── sink.rs               — AlertSink actor
    ├── swarm.rs              — Swarm orchestrator + SwarmFeeder
    ├── databento.rs          — DBN-file adapter        (feature `databento`)
    └── distributed.rs        — Remote alert gateway    (feature `remote`)

examples/
├── historical_bootstrap.rs        — feed one pair's history, inspect the monitor
├── live_pubsub.rs                 — scripted live feed + custom listener actor
├── triangular_arbitrage.rs        — full triangle, observe firing + hysteresis
├── supervised_swarm.rs            — kameo OneForOne supervision restarts a panicking monitor
├── databento_historical.rs        — (feature: databento) decode `.dbn.zst`, pump asap
├── databento_live_replay.rs       — (feature: databento) spawn pump on TaskTracker, realtime pacing
└── distributed_alert_consumer.rs  — (feature: remote)    ROLE=producer / ROLE=consumer demo

tests/
├── integration_test.rs            — end-to-end alert wiring, hysteresis, ring-buffer eviction
├── databento_integration.rs       — (feature: databento) Mbp1→Tick conversion, file decode, cancellation
└── remote_integration.rs          — (feature: remote)    libp2p wire round-trip via RemoteActorRef
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

# the remote alert gateway example (two terminals)
ROLE=producer cargo run --features remote --example distributed_alert_consumer
ROLE=consumer cargo run --features remote --example distributed_alert_consumer

# tests (add features to include the optional suites)
cargo test
cargo test --features databento
cargo test --features remote
cargo test --features 'databento remote'

# the daemon (scripted EUR/USD-GBP/USD-EUR/GBP feed; ctrl-c to stop)
cargo run
```

`CLAUDE.md` documents the canonical `--manifest-path Cargo.toml --target-dir
/tmp/forex-arbitrage-swarm-target` + `timeout 30s` invocation pattern we use
to avoid contention with editor-managed cargo runs.

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
swarm via a clone-able `SwarmFeeder`.

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

## Remote alert gateway (optional feature: `remote`)

The `subsystems::distributed` module spins up a libp2p swarm
(TCP + noise + yamux + mDNS + `kameo::remote::Behaviour`) and registers a
`RemoteAlertGateway` actor that's subscribed to the local alert bus and
exposes its buffer to remote peers via `#[remote_message]` asks.

The hot path (monitor → coordinator → sink) deliberately stays on local
mpsc — see `CLAUDE.md` §"Worth flagging" #8 for the rationale. Remote is
the *publish-to-outside-world* layer, not a replacement for actor RPC.

```rust,ignore
use forex_arbitrage_swarm::subsystems::distributed::{
    enable_remote_alerts, PollOpportunities, RemoteAlertGateway, RemoteConfig,
};

// Producer side: enable the gateway on a built swarm.
let handle = enable_remote_alerts(&swarm, RemoteConfig::default()).await?;
println!("listening as peer {}", handle.local_peer_id);

// Consumer side (different process, mDNS-discovered):
use futures::TryStreamExt;
use kameo::prelude::RemoteActorRef;

let mut peers = RemoteActorRef::<RemoteAlertGateway>::lookup_all("forex_swarm_alerts");
while let Some(peer) = peers.try_next().await? {
    let opps = peer.ask(&PollOpportunities).send().await?;
    for opp in opps {
        println!("{opp}");
    }
}
```

### Remote-callable messages

| Message | Reply | Semantics |
|---|---|---|
| `PollOpportunities` | `Vec<ArbitrageOpportunity>` | Returns a *clone* of the buffer (does not drain) — consumers can poll repeatedly |
| `PeekOpportunityCount` | `u64` | Cheap liveness probe — no payload copy |
| `ClearOpportunities` | `u64` | Drops the buffer; returns the count that was dropped |

### What's actually on the wire

`ArbitrageOpportunity`, `Triangle`, `Pair`, `Quote`, `Direction`, `Tick`
all derive `Serialize + Deserialize` so the swarm's local types double as
the wire payload. This is a POC trade-off — a production deployment would
typically split into stable `RemoteArbitrageOpportunityV1` wire types
behind a conversion boundary.

## Next steps (not in this POC)

Tracked in detail in [`CLAUDE.md`](./CLAUDE.md):

- **A. Databento `LiveClient` integration** — real-time websocket source,
  same kameo wiring as the file adapter. *Blocked on `DATABENTO_API_KEY`.*
- **B. Synthetic DBN file** — generate a forex-shaped DBN at runtime via
  `dbn::encode::AsyncDbnEncoder` so we can demonstrate end-to-end arb
  detection through the file adapter (the bundled fixture is too small).
- **C. Remote gateway hardening** — bounded buffer, cursor-based polling
  with sequence numbers, stable `RemoteArbitrageOpportunityV1` wire schema,
  multiple gateways under one libp2p swarm, QUIC transport.
- **D. Smaller nice-to-haves** — `metrics`/Prometheus example,
  multi-triangle stress test, bid/ask-aware execution-cost model, switch
  off path deps when kameo 0.20.0 is published.
