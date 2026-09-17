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
| `harness` | The K7 A/B harness (feature `harness`): seeded instance generation, the battery loop (bootstrap join, policy-gated arrivals, one leave sweep), per-seed metrics, report helpers — the plumbing every K7 registration shares; with `process`, the workflow world (`WorkflowSpec`: per-agent roles, per-task declared `Demand`, role-matched step coverage, an optional performance draw) and the per-task lifecycle hook every policy can observe (`begin_task` / `observe_outcome`) |
| `subsystems` | `CoalitionService` — the policy-gated coalition-membership seam (join/leave consult a `CoalitionDecisionPolicy` before mutating the hypergraph) — plus a decision-tap tee (`spawn_decision_tee`), an optional durable decision log (`durable`), and an optional libp2p remote coalition-event gateway (`remote`: bounded buffer, cursor polling, stable `RemoteCoalitionEventV1` wire schema) |

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
cargo run --release --features decision,magnitude,process --example strategy_comparison   # the frozen K4 archive battery (Parts 1–11, ~20 min, serial)
cargo run --features harness --example gauntlet                                   # K7 harness skeleton
K7_1_SEEDS=5000..5003 cargo run --release --features harness,decision,process --example k7_1   # K7-1 off-block smoke (no verdict; the run of record is docs/runs/K7-1.log)
cargo run --features decision --example population_reliability                    # learned-reliability fitness for the structure search (#57)
cargo run --features durable --example durable_decisions                          # durable decision log (needs Docker)
cargo run --features remote --example remote_coalition_consumer                   # remote coalition-event gateway + client, one process (#38)
cargo run --features metrics --example metrics_scrape                             # Prometheus counters/histograms over the decision, outcome and topology-event taps, self-scraped and asserted (#25)
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

The showcase is a head-to-head battery of coalition-decision strategies —
Active Inference arms built on [aif](https://github.com/sustia-llc/tira) vs a
categorical (magnitude-based) baseline built on
[catgraph](https://github.com/sustia-llc/catgraph) — run as **pre-registered
A/B experiments**. Criteria are fixed and committed *before* each run
(`docs/prereg-*.md`), verdicts are recorded against them
(`docs/ab-report-*.md`), and falsified arms stay falsified — the reports are
never rewritten. [`docs/README.md`](docs/README.md) indexes every run and the
seed ledger; [`docs/PROTOCOL.md`](docs/PROTOCOL.md) is the protocol; the raw
output of each run of record is committed under
[`docs/runs/`](docs/runs/README.md).

Two binaries carry it. `examples/strategy_comparison.rs` is the **K4 lineage**
(Parts 1–11, the table below), frozen as the archive: it changes only through
`src/`, and a fresh serial run diffed against `docs/runs/K4-archive.log` is the
drift gate at every dependency re-pin. The **K7 lineage** runs on
`examples/gauntlet.rs` and one example per registration under `examples/k7/`,
sharing its plumbing through `src/harness/` (feature `harness`); its
pre-registrations and reports are under `docs/k7/`.

The run history is deliberately adversarial:

| Run | Challenger arm | Verdict |
|-----|----------------|---------|
| K4 v1 ([#7](https://github.com/sustia-llc/koalisi/issues/7)) | scalar AIF bridge | `FALSIFIED (latency)` under v1 criteria; `VALIDATED (B)` under the pre-posted v2 amendment — magnitude superior on quality 30/30 seeds |
| K4 v3 | multimodal AIF (one modality per capability bit) | `FALSIFIED (multimodality)` — proved decision-equivalent to the scalar bridge, all 30 seeds |
| K4 v4 ([#44](https://github.com/sustia-llc/koalisi/issues/44)) | persistent AIF (learning + precision dynamics) | `FALSIFIED (persistence)` — genuinely escapes the v3 equivalence theorem, but loses on quality |
| K4 v5 ([#53](https://github.com/sustia-llc/koalisi/issues/53)) | E1-only persistent AIF (learned precisions + novelty, fixed γ) | `VALIDATED (gap closed)` — first arm to beat magnitude on out-of-sample quality (0.4406 vs 0.2720), at a churn + latency cost |
| K4 v6 ([#56](https://github.com/sustia-llc/koalisi/issues/56)) | v5 + never-evict state damping | `FALSIFIED (never-evict)` — the eviction churn *is* the winning mechanism (monotone cap series) |
| EQ1 ([#61](https://github.com/sustia-llc/koalisi/issues/61)) | de-saturated query regime (γ sweep) | `FALSIFIED (de-saturation)` — γ frees only the leave stream; the join rail is margin-proof at p = 1.0 |
| EQ1-corrected ([#63](https://github.com/sustia-llc/koalisi/issues/63)) | block-level coverage routing | `FALSIFIED (block-routing)` — the value window barely exceeds the competing singleton lattice |
| EQ3 ([#69](https://github.com/sustia-llc/koalisi/issues/69)) | incremental-magnitude latency levers | `FALSIFIED (latency re-match)` — bit-parity held, but the 2.5× gap never closed |
| EQ4 ([#72](https://github.com/sustia-llc/koalisi/issues/72)) | typed roles (ρ-modulated coupling) | `VALIDATED (typed roles)` — 3.61× on 30/30 seeds; the lever is *retained role-diverse redundancy*, not coverage routing |
| EQ5a ([#76](https://github.com/sustia-llc/koalisi/issues/76)) | process-structured tasks (string-diagram rewriting) | `FALSIFIED (process structure)` — rewriting converted 100 % of a low ceiling; the signal was *valuation*, not rewriting |
| EQ5a follow-up ([#80](https://github.com/sustia-llc/koalisi/issues/80)) | is the residual lever process-specific? | `FALSIFIED (coverage proxy)` — it replicates at 1.34× and then behaves identically without the process structure |
| EQ5b ([#78](https://github.com/sustia-llc/koalisi/issues/78)) | `GroupAgent` of role-slotted AIF internals over the v5 world model | `VALIDATED (two-engine)` — 1.2567× on 22/30, **by 0.7 %**; but role specialisation measured *negative* and the win rides members structurally blind to the candidate |
| K7-1 ([#90](https://github.com/sustia-llc/koalisi/issues/90)) | EQ5b's group with the candidate's own role internal routed to every voter (`aif` ext-6 topology, λ = ½) vs the EQ5b group | `VALIDATED (topology-routed group)` — 2.41× on 30/30; but the routed arm's outcome is that of an **engine-free redundancy prune** (598 of 600 tasks), one voter decides every read it can see, and the control loses by evicting needed members on every three-role task. The effect was seen off-block before the run |

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
cargo test                                 # 106 tests (core + topology + algorithms + population search + decision + ingestion + decision-tap tee)
cargo test --features decision             # 162 tests (+ Active Inference decision strategies: scalar, multimodal, persistent + the derived ReliabilityCoverage calculator)
cargo test --features magnitude            # 135 tests (+ categorical-magnitude decision strategy + typed-role modulation + trajectory analytics)
cargo test --features decision,magnitude   # 191 tests (both decision arms)
cargo test --features magnitude-fast       # 143 tests (+ EQ3 opt-in levers + read-only probes)
cargo test --features persistence          # 126 tests (+ chained event store + topology replay)
cargo test --features persistence,magnitude # 156 tests (incl. the live-vs-replayed parity gate)
cargo test --features remote               # 112 tests (+ gateway event buffer + loopback round-trip)
cargo test --features process              # 161 tests (+ process-structured workflows + the unstaffable-residual policy)
cargo test --features decision,magnitude,process # 249 tests (the full A/B battery surface)
cargo test --features durable              # 107 tests (+ container-backed restart-durability test; needs Docker)
cargo test --features harness              # 129 tests (+ the K7 harness: rng, instance generation, battery loop, lifecycle hook, decision trace, report helpers)
cargo test --features harness,process      # 201 tests (+ the K7 workflow world and its identity gate against docs/runs/K4-archive.log)
cargo test --features harness,decision,process # 291 tests (+ the group arm hosted on the harness and its identity gate against the same log)
cargo test --features metrics              # 106 tests (the default suite; the feature gates two optional deps and the metrics_scrape example)
```

## Dependencies

- [catgraph-applied](https://github.com/sustia-llc/catgraph) (tag `v0.23.0`, kept in lockstep with catgraph-magnitude and catgraph-syntax — one repo, one checkout, all three move together) — CRUD hypergraph container backing the topology layer
- tokio + tokio-util — async runtime + lifecycle primitives
- rayon + tokio-rayon — CPU-bound graph operations bridge
- [surrealdb-live-message](https://github.com/sustia-llc/surrealdb-live-message) (tag `v0.2.1`, **optional**, feature `durable`) — two-tier restart-durable message bus for the coalition decision log
- [aif](https://github.com/sustia-llc/tira) (tag `aif-v0.14.0`, **optional**, feature `decision`) — active-inference engine for the AIF decision strategies (scalar, multimodal, persistent); `nalgebra` is only compiled when the feature is enabled
- [catgraph-magnitude](https://github.com/sustia-llc/catgraph) (tag `v0.23.0`, **optional**, feature `magnitude`) — enriched-category coalition magnitude for the categorical decision strategy
- [catgraph-syntax](https://github.com/sustia-llc/catgraph) (tag `v0.23.0`, **optional**, feature `process`) — colored-syntax layer over the free-prop term surface for process-structured tasks; depends on catgraph + catgraph-applied + thiserror only, so no DeepCausality crate is in the dependency graph under any feature
- ciborium 0.2 + sha2 0.11 (**optional**, feature `persistence`) — CBOR frames and SHA-256 chaining for the append-only event store
- libp2p 0.57 (**optional**, feature `remote`) — TCP+noise+yamux `request-response` transport for the remote coalition-event gateway
- metrics 0.24 + metrics-exporter-prometheus 0.18 (`http-listener` only) (**optional**, feature `metrics`) — the metrics facade and the Prometheus scrape listener used by `examples/metrics_scrape.rs`

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
