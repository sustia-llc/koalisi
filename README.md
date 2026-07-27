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
| `algorithms` | `ValueCalculator` trait + 4 base calculators + a feedback-weighting `FeedbackCalculator` wrapper (history/failure signals from a shared `FeedbackStore`), `DCVCDistributor`, AIPA partition search, population coalition-structure search (`search`/`record_trajectory`, #42) |
| `ingest` | Domain-neutral ingestion (K5): `Sample`/`DataSource` traits, generic `SampleMonitor<S>`, `Pacing` + `pump_source`, synthetic NEST-shaped multi-resolution and tauhokohoko-shaped sensor-event fixture sources (seeded, no credentials) |
| `decision` | `CoalitionDecisionPolicy` trait + always-available `ThresholdPolicy`; optional Active Inference strategy (`EfeValueCalculator`, `AifDecisionPolicy`) behind the `decision` feature; optional categorical-magnitude strategy (`MagnitudeValueCalculator`, `MagnitudePolicy`) behind the `magnitude` feature |
| `persistence` | Append-only event store (feature `persistence`): hash-chained streams, CBOR frame log (`FileEventStore`), crash-tail recovery, writer task; topology events tap in and replay back into a fresh `EventLog` all queries run on unchanged (P7.1 + P7.2) — see `.claude/docs/phase7-persistence-design.md` |
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

# Population search over coalition structures atop AIPA (P5.2, #42)
cargo run --example population_search

# Feature-gated
cargo run --release --features decision,magnitude --example strategy_comparison   # divergence demo + AIF-vs-magnitude A/B report (#7)
cargo run --features durable --example durable_decisions                          # durable decision log (needs Docker)
```

> **Value models for structure search (population_search).** `search` maximises
> `Σ over blocks of ValueCalculator(block)`. The built-in calculators are
> *degenerate* for this: `AdditiveCalculator` is **constant** across every
> set-partition (its size / capability / trust terms sum to the same total for any
> grouping), and `SynergisticCalculator` / `MultiplicativeCalculator` favour
> all-singletons — so with those the answer is trivial and the improvement lineage
> is one epoch. The `population_search` example therefore uses a `TaskCoverage`
> value model (reward full coverage of a required capability set, penalise redundant
> members) whose optimum is a genuine non-trivial partition. Real structure search
> needs an interior-optimum value model — coverage-style, or the `magnitude` / EFE /
> `FeedbackCalculator` arms.

## The A/B process: pre-registered decision-strategy evaluation

`examples/strategy_comparison.rs` is the showcase: a head-to-head battery of
coalition-decision strategies — Active Inference arms built on
[aif](https://github.com/sustia-llc/tira) vs a categorical (magnitude-based)
baseline built on [catgraph](https://github.com/sustia-llc/catgraph) — run as
**pre-registered A/B experiments**. Criteria are fixed and committed *before*
each run (`docs/prereg-*.md`), verdicts are recorded against them
(`docs/ab-report-*.md`), and falsified arms stay falsified — the reports are
never rewritten.

The run history is deliberately adversarial:

| Run | Challenger arm | Verdict |
|-----|----------------|---------|
| K4 v1 ([#7](https://github.com/sustia-llc/koalisi/issues/7)) | scalar AIF bridge | `FALSIFIED (latency)` under v1 criteria; `VALIDATED (B)` under the pre-posted v2 amendment — magnitude superior on quality 30/30 seeds |
| K4 v3 | multimodal AIF (one modality per capability bit) | `FALSIFIED (multimodality)` — proved decision-equivalent to the scalar bridge, all 30 seeds |
| K4 v4 ([#44](https://github.com/sustia-llc/koalisi/issues/44)) | persistent AIF (learning + precision dynamics) | `FALSIFIED (persistence)` — genuinely escapes the v3 equivalence theorem, but loses on quality |
| K4 v5 ([#53](https://github.com/sustia-llc/koalisi/issues/53)) | E1-only persistent AIF (learned precisions + novelty, fixed γ) | `VALIDATED (gap closed)` — first arm to beat magnitude on out-of-sample quality (0.4406 vs 0.2720), at a churn + latency cost |
| K4 v6 ([#56](https://github.com/sustia-llc/koalisi/issues/56)) | v5 + never-evict state damping | `FALSIFIED (never-evict)` — the eviction churn *is* the winning mechanism (monotone cap series) |

Two feedback-calculator arms ran the same gauntlet
([#46](https://github.com/sustia-llc/koalisi/issues/46) `FALSIFIED`,
[#48](https://github.com/sustia-llc/koalisi/issues/48) `PARTIAL (mechanism
only)`). The arm-choice decision is recorded in
`docs/k4-arm-choice-memo.md`: magnitude remains the demonstrated default;
the E1 arm stands as capability evidence, arm selection being a cost–quality
tradeoff. The competitive pressure also flowed upstream — several `aif`
engine features (seed API, novelty EFE term, Dirichlet-count injection) were
cut specifically for these arms; tira's README tells the same story from the
upstream side.

## Tests

```sh
cargo test                                 # 103 tests (core + topology + algorithms + population search + decision + ingestion)
cargo test --features decision             # 147 tests (+ Active Inference decision strategies: scalar, multimodal, persistent)
cargo test --features magnitude            # 125 tests (+ categorical-magnitude decision strategy + trajectory analytics)
cargo test --features decision,magnitude   # 169 tests (both decision arms)
cargo test --features persistence          # 123 tests (+ chained event store + topology replay)
cargo test --features persistence,magnitude # 146 tests (incl. the live-vs-replayed parity gate)
cargo test --features durable              # + container-backed restart-durability test (needs Docker)
```

## Dependencies

- [catgraph-applied](https://github.com/sustia-llc/catgraph) (tag `v0.2.0`, kept in lockstep with catgraph-magnitude — one repo, one checkout) — CRUD hypergraph container backing the topology layer (the K1 re-back; replaced yamafaktory `hypergraph` v4.2.0)
- tokio + tokio-util — async runtime + lifecycle primitives
- rayon + tokio-rayon — CPU-bound graph operations bridge
- [surrealdb-live-message](https://github.com/sustia-llc/surrealdb-live-message) (tag `v0.2.1`, **optional**, feature `durable`) — two-tier restart-durable message bus for the coalition decision log
- [aif](https://github.com/sustia-llc/tira) (tag `aif-v0.11.0`, **optional**, feature `decision`) — active-inference engine for the AIF decision strategies (scalar, multimodal, persistent); `nalgebra` is only compiled when the feature is enabled
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

## References

- Zhang, Y., Lin, C., Tang, S., Chen, H., Zhou, S., Ma, Y., Tresp, V. (2025).
  *SwarmAgentic: Towards Fully Automated Agentic System Generation via Swarm
  Intelligence.* [arXiv:2506.15672](https://arxiv.org/abs/2506.15672) — the
  design inspiration for the population-based coalition-structure search
  (`algorithms::population`) and the feedback-weighted value calculator
  (`FeedbackCalculator`). A working digest (and a CC0 copy of the paper)
  lives in `.claude/docs/`.
- The Active Inference side of the A/B battery builds on the
  [aif](https://github.com/sustia-llc/tira) engine — see tira's README for
  its own paper-reproduction lineage (Waade et al.).

## License

MIT OR Apache-2.0
