# koalisi

A reference implementation of **agentic coalitions** in Rust.

koalisi provides a layered architecture for building agent coalition systems —
from temporal hypergraph topology to coalition formation algorithms to
actor-based runtime orchestration.

## Architecture

```
┌─────────────────────────────────────────────────┐
│  Runtime Layer (kameo actors)                    │
│  PubSub buses, TaskTracker, CancellationToken,  │
│  three-step shutdown, libp2p remote gateway      │
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
| `topology` | Temporal hypergraph with event sourcing, `CoalitionManager`, time-travel queries, analytics |
| `algorithms` | `ValueCalculator` trait + 4 calculators, `DCVCDistributor`, AIPA partition search |
| `decision` | `CoalitionDecisionPolicy` trait + always-available `ThresholdPolicy`; optional Active Inference strategy (`EfeValueCalculator`, `AifDecisionPolicy`) behind the `decision` feature; optional categorical-magnitude strategy (`MagnitudeValueCalculator`, `MagnitudePolicy`) behind the `magnitude` feature |
| `subsystems` | Forex-specific kameo actors (monitor, coordinator, sink, swarm) |
| `market` | Forex value types (Pair, Tick, Quote, Triangle, ArbitrageOpportunity) |

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

# Forex arbitrage — triangular arb detection
cargo run --example triangular_arbitrage

# Full list
cargo run --example historical_bootstrap
cargo run --example live_pubsub
cargo run --example supervised_swarm

# Feature-gated
cargo run --release --features decision,magnitude --example strategy_comparison   # divergence demo + AIF-vs-magnitude A/B report (#7)
cargo run --features databento --example databento_historical
cargo run --features databento --example databento_live_replay
cargo run --features remote --example distributed_alert_consumer
```

## Tests

```sh
cargo test                                 # 68 tests (core + topology + algorithms + decision + forex)
cargo test --features decision             # 87 tests (+ Active Inference decision strategy)
cargo test --features magnitude            # 77 tests (+ categorical-magnitude decision strategy)
cargo test --features decision,magnitude   # 96 tests (both decision arms)
cargo test --features databento            # + 4 databento integration tests
cargo test --features remote               # + 1 remote integration test
```

## Dependencies

- [kameo](https://github.com/tqwewe/kameo) — actor framework (path dep, pre-0.20.0)
- [catgraph-applied](https://github.com/sustia-llc/catgraph) (tag `v0.1.1`) — CRUD hypergraph container backing the topology layer (the K1 re-back; replaced yamafaktory `hypergraph` v4.2.0)
- tokio + tokio-util — async runtime + lifecycle primitives
- rayon + tokio-rayon — CPU-bound graph operations bridge
- [aif](https://github.com/sustia-llc/tira) (tag `aif-v0.5.0`, **optional**, feature `decision`) — active-inference engine for the AIF decision strategy; pulls `nalgebra` only when the feature is enabled
- [catgraph-magnitude](https://github.com/sustia-llc/catgraph) (tag `v0.1.1`, **optional**, feature `magnitude`) — enriched-category coalition magnitude for the categorical decision strategy

## Origin

koalisi consolidates four prior coalition projects into a single layered architecture:
- **dynamo** — temporal hypergraph + event sourcing + CoalitionManager
- **coalesce** — DCVC + AIPA + value calculators
- **coalition_aif** — Active Inference + EFE (retired; its ideas re-expressed on the `aif` reference engine, available behind the optional `decision` feature)
- **forex-arbitrage-swarm** — kameo actor runtime + PubSub + lifecycle

The forex domain is preserved as a working adapter; the architecture is domain-agnostic.

## License

MIT OR Apache-2.0
