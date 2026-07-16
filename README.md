# koalisi

A reference implementation of **agentic coalitions** in Rust.

koalisi provides a layered architecture for building agent coalition systems —
from temporal hypergraph topology to coalition formation algorithms to
`tokio::sync` task-based runtime orchestration.

## Architecture

```
┌─────────────────────────────────────────────────┐
│  Runtime Layer (tokio tasks)                     │
│  broadcast buses, mpsc/oneshot handles,          │
│  TaskTracker + CancellationToken three-step      │
│  shutdown, task-restart supervision,             │
│  CoalitionService seam, optional durable log     │
├─────────────────────────────────────────────────┤
│  Algorithm Layer                                 │
│  DCVC workload distribution, AIPA partition      │
│  search, pluggable value calculators             │
├─────────────────────────────────────────────────┤
│  Topology Layer (catgraph-applied Hypergraph)   │
│  Temporal hypergraph, event sourcing,            │
│  CoalitionManager, time-travel queries,          │
│  analytics                                       │
├─────────────────────────────────────────────────┤
│  Core                                            │
│  CoalitionRuntime, config, logging               │
└─────────────────────────────────────────────────┘
```

## Modules

| Module | Description |
|--------|-------------|
| `core` | `CoalitionRuntime` (lifecycle), settings, logging |
| `topology` | Temporal hypergraph with event sourcing, `CoalitionManager`, time-travel queries, analytics (incl. `magnitude_history` coalition-diversity trajectories behind the `magnitude` feature) |
| `algorithms` | `ValueCalculator` trait + 4 base calculators + a feedback-weighting `FeedbackCalculator` wrapper (history/failure signals from a shared `FeedbackStore`), `DCVCDistributor`, AIPA partition search |
| `ingest` | Domain-neutral ingestion (K5): `Sample`/`DataSource` traits, generic `SampleMonitor<S>`, `Pacing` + `pump_source`, synthetic NEST-shaped multi-resolution and tauhokohoko-shaped sensor-event fixture sources (seeded, no credentials) |
| `decision` | `CoalitionDecisionPolicy` trait + always-available `ThresholdPolicy`; optional Active Inference strategy (`EfeValueCalculator`, `AifDecisionPolicy`) behind the `decision` feature; optional categorical-magnitude strategy (`MagnitudeValueCalculator`, `MagnitudePolicy`) behind the `magnitude` feature |
| `persistence` | Append-only event store (feature `persistence`): hash-chained streams, CBOR frame log (`FileEventStore`), crash-tail recovery, writer task; topology events tap in and replay back into a fresh `EventLog` all queries run on unchanged (P7.1 + P7.2) — see `docs/phase7-persistence-design.md` |
| `subsystems` | `CoalitionService` — the policy-gated coalition-membership seam (join/leave consult a `CoalitionDecisionPolicy` before mutating the hypergraph) — plus an optional durable decision log (`durable`) |

## Quick start

```rust
use koalisi::topology::{CoalitionManager, TemporalQueries, Timestamp};
use std::fmt::{Display, Formatter};

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
struct Agent(&'static str);
impl Display for Agent {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result { write!(f, "{}", self.0) }
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
struct Team(&'static str, usize);
impl Display for Team {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result { write!(f, "{}", self.0) }
}
impl From<Team> for usize { fn from(t: Team) -> usize { t.1 } }

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mgr = CoalitionManager::<Agent, Team>::empty();

    let a = mgr.add_agent(Agent("alice")).await?;
    let b = mgr.add_agent(Agent("bob")).await?;
    let c = mgr.form_coalition(vec![a, b], Team("alpha", 10)).await?;

    let members = mgr.coalition_members(c).await?;
    assert_eq!(members.len(), 2);

    // Time-travel: how many agents existed at T=0?
    let count = TemporalQueries::count_vertices_at(
        mgr.graph().events_ref(), Timestamp::new(0)
    ).await;
    assert_eq!(count, 1); // only alice
    Ok(())
}
```

## Examples

```sh
# Coalition topology — form, join, merge, time-travel
cargo run --example topology_coalition

# Algorithm values — calculators, DCVC, AIPA
cargo run --example algorithm_values

# Flagship: synthetic ingestion → coalition formation (domain-neutral, no credentials)
cargo run --example synthetic_ingestion

# Task-restart supervision (spawn_supervised over a synthetic monitor)
cargo run --example supervised_monitor

# Feature-gated
cargo run --release --features decision,magnitude --example strategy_comparison   # divergence demo + AIF-vs-magnitude A/B report (#7)
cargo run --features durable --example durable_decisions                          # durable decision log (needs Docker)
```

## Tests

```sh
cargo test                                 # 88 tests (core + topology + algorithms + decision + ingestion)
cargo test --features decision             # 119 tests (+ Active Inference decision strategies, scalar + multimodal)
cargo test --features magnitude            # 110 tests (+ categorical-magnitude decision strategy + trajectory analytics)
cargo test --features decision,magnitude   # 141 tests (both decision arms)
cargo test --features persistence          # 108 tests (+ chained event store + topology replay)
cargo test --features persistence,magnitude # 131 tests (incl. the live-vs-replayed parity gate)
cargo test --features durable              # + container-backed restart-durability test (needs Docker)
```

## Dependencies

- [catgraph-applied](https://github.com/sustia-llc/catgraph) (tag `v0.2.0`, kept in lockstep with catgraph-magnitude — one repo, one checkout) — CRUD hypergraph container backing the topology layer (the K1 re-back; replaced yamafaktory `hypergraph` v4.2.0)
- tokio + tokio-util — async runtime + lifecycle primitives
- rayon + tokio-rayon — CPU-bound graph operations bridge
- [surrealdb-live-message](https://github.com/sustia-llc/surrealdb-live-message) (tag `v0.2.1`, **optional**, feature `durable`) — two-tier restart-durable message bus for the coalition decision log
- [aif](https://github.com/sustia-llc/tira) (tag `aif-v0.9.0`, **optional**, feature `decision`) — active-inference engine for the AIF decision strategies (scalar + multimodal); `nalgebra` is only compiled when the feature is enabled
- [catgraph-magnitude](https://github.com/sustia-llc/catgraph) (tag `v0.2.0`, **optional**, feature `magnitude`) — enriched-category coalition magnitude for the categorical decision strategy

## Origin

koalisi consolidates four prior coalition projects into a single layered architecture:
- **dynamo** — temporal hypergraph + event sourcing + CoalitionManager
- **coalesce** — DCVC + AIPA + value calculators
- **coalition_aif** — Active Inference + EFE (retired; its ideas re-expressed on the `aif` reference engine, available behind the optional `decision` feature)
- **forex-arbitrage-swarm** — the runtime layer (originally kameo actors + PubSub; since K3 pure `tokio::sync` task seams)

koalisi is domain-agnostic. It began as a forex triangular-arbitrage tool;
that domain was removed in v0.11.0 (market/trading work now lives in the
sibling [`biome`](https://github.com/sustia-llc/biome) project). The
demonstrated runtime is a synthetic, non-financial coalition-formation
pipeline (`examples/synthetic_ingestion.rs`).

## License

MIT OR Apache-2.0
