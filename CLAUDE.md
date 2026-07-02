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
(temporal hypergraph via `catgraph_applied::Hypergraph` since K1 — was
yamafaktory hypergraph v4.2.0 — event sourcing, CoalitionManager,
time-travel queries, analytics), Algorithms (DCVC workload distribution,
AIPA partition search, pluggable value calculators), and Runtime (kameo
actors, PubSub buses, remote gateway). The forex triangular arbitrage
domain is preserved as a working adapter; the architecture is domain-agnostic.
Evolved from four prior projects: dynamo (topology), coalesce (algorithms),
coalition_aif (decision — planned), and forex-arbitrage-swarm (runtime).

## Available tooling for this project

- **`graph` plugin v2.0.1** (`~/.claude/plugins/cache/sustia-claude-code-plugins/graph/2.0.1/`)
  ships a `hypergraph` agent plus six skills tracking **yamafaktory**
  hypergraph v4.2.0. **STALE for `src/topology/` since K1 (#4)** — the backend
  is now `catgraph_applied::Hypergraph` (read its module docs at the catgraph
  tag instead). The skills remain useful only as historical reference for the
  pre-K1 semantics and for the now-obsolete `PersistentHypergraph` idea (see
  the Phase 7 note).
- `rust-v2:rust-dev-v2` / `rust-v2:rust-practical` — primary Rust agents per
  the user CLAUDE.md routing rules.

## Current state — 2026-07-02

### Done

- **catgraph backend swap (K1, issue #4)**: `TemporalHypergraph`/`SharedGraph`
  re-backed on `catgraph_applied::Hypergraph` (git tag `v0.1.1`, the
  catgraph#23 container); yamafaktory `hypergraph` v4.2.0 dep **dropped**.
  Direct swap (approved deviation — no feature flag; parity by commits).
  API deltas: `TemporalError`/`TemporalResult` lost their unused `<V, HE>`
  generics; `VertexTrait`/`HyperedgeTrait` are koalisi-local blanket aliases
  (`Copy + Eq + Debug + Send + Sync`); no-op updates now `Ok` (try_join
  idempotency finally true — guard test); clears infallible. K4 backend-parity
  re-run committed (`docs/ab-report-K4-catgraph.md`): quality numbers and both
  verdicts byte-identical to the yamafaktory report. See §"Worth flagging"
  entry 12 for the index/idempotency contract.
- **A/B harness (K4, issue #7)**: `examples/strategy_comparison.rs` Part 2 —
  the pre-registered AIF-vs-magnitude battery (30 SplitMix64 seeds, PRIMARY =
  completion-rate × coverage-efficiency, oracle regret ≤ 8-agent pools, churn +
  latency secondaries, exploratory t-sweep). Committed run:
  `docs/ab-report-K4-yamafaktory.md`. **Verdict: FALSIFIED (latency) under v1;
  VALIDATED (B) under the v2 amendment** (#7 comment 2026-07-02: Path A =
  original speed route OR Path B = quality dominance ≥ 1.25× median + ≥ 60% of
  seeds + latency ≤ 10× — harness prints both verdicts). Magnitude superior on
  quality in 30/30 seeds (0.4469 vs 0.1898 median) and 14× less churn; 4.37 µs
  vs 1.48 µs per decision fails the v1 strict-latency gate. Incremental-
  magnitude optimization filed as catgraph#31 (strengthens Path A). Runs
  `--release` (latency criterion; the catgraph #29 debug-assert caveat is
  resolved since the `v0.1.1` dep bump — debug builds run clean).
  Backend-parity re-run deferred to K1 (#4). See Phase 6 §K4 below.
- **Magnitude decision arm (K2, issue #5)**: `MagnitudePolicy` +
  `MagnitudeValueCalculator` behind a new `magnitude` feature — the categorical
  A/B mirror of the AIF arm, backed by `catgraph_magnitude::coalition_value`
  (git tag `v0.1.1`, SSH URL — catgraph is private, same rationale as `aif`;
  pinned `t = 1`). Capabilities map to directed substitutability couplings
  `A(i→j) = |rel_i ∩ rel_j| / |rel_i|`; clones skeletalize into one effective
  agent; **task-irrelevant agents (`rel == 0`) are excluded** (a vacuous 1.0
  coupling collapses diversity — review-confirmed). Feature-independent of
  `decision`; either, both, or neither may be enabled. See Phase 6 §K2 below.
- **Decision layer (Phase 6, B0–B7) — v0.6.0**: pluggable `CoalitionDecisionPolicy`
  + always-available `ThresholdPolicy`; optional `decision` feature bridging `u32`
  capabilities to the `aif` AIF engine (`EfeValueCalculator`, `AifDecisionPolicy`)
  via a capability-coverage→POMDP-precision map; `tokio-rayon` async offload;
  `examples/strategy_comparison.rs`. See Phase 6 section below. 10 `decision::` tests.
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
  | Default | 68 | `cargo test` |
  | `--features decision` | 87 | `cargo test --features decision` |
  | `--features magnitude` | 77 | `cargo test --features magnitude` |
  | `--features decision,magnitude` | 96 | `cargo test --features decision,magnitude` |
  | `--features databento` | + 4 databento integration | `cargo test --features databento` |
  | `--features remote` | + 1 remote integration | `cargo test --features remote` |
  | All 7 examples | exit 0 | see Reproducers below |

### File inventory

```
koalisi/
├── Cargo.toml                              path deps: kameo; git tag deps: catgraph-applied v0.1.1 (topology), aif + catgraph-magnitude (optional)
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
│   ├── decision/
│   │   ├── mod.rs                          CoalitionDecisionPolicy + ThresholdPolicy (always compiled)
│   │   ├── aif_policy.rs                   AifDecisionPolicy + EfeValueCalculator (feature `decision`)
│   │   └── magnitude_policy.rs             MagnitudePolicy + MagnitudeValueCalculator + CouplingModel (feature `magnitude`)
│   ├── llm/
│   │   └── mod.rs                          LlmProvider trait + StubLlmProvider (Phase 5 anchor)
│   └── subsystems/
│       ├── monitor.rs                      MarketMonitor (Tick, GetSnapshot, Ping)
│       ├── coordinator.rs                  ArbitrageCoordinator (TickUpdate, GetQuotes, Ping)
│       ├── sink.rs                         AlertSink (ArbitrageOpportunity, GetAlerts, DrainAlerts, Ping)
│       ├── swarm.rs                        Swarm (wraps CoalitionRuntime) + SwarmConfig + SwarmFeeder
│       ├── coalition_actor.rs              CoalitionActor (policy-gated membership seam, issue #1)
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

12. **catgraph backend contracts (K1, #4) — rely on these, don't re-derive.**
    - **Stable, never-reused indices**: `VertexIndex`/`HyperedgeIndex` come from
      monotonic counters and survive removals AND `clear()` — the event-sourced
      replay stores raw indices and depends on this.
    - **No-op updates return `Ok`** (yamafaktory errored `…Unchanged`):
      `try_join_coalition` re-join is genuinely idempotent now; the guard test
      is `rejoin_existing_member_is_idempotent`. Don't add code that relies on
      an `Err` to detect "already present".
    - `add_vertex`/`clear`/`clear_hyperedges` are infallible; weights are
      `Copy`, read by value. Hyperedges are ORDERED `Vec<VertexIndex>` with
      duplicates allowed — dedup before handing member lists to
      `catgraph_magnitude::coalition_value` (it errors on duplicates).
    - Full contract: module docs of `catgraph-applied/src/hypergraph.rs` at the
      pinned tag.

## Reproducers

All assume `cwd = koalisi/`.

```sh
# === default features (68 tests) ===
timeout 60s  cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target
timeout 30s  cargo run  --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --example topology_coalition
timeout 30s  cargo run  --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --example algorithm_values
timeout 30s  cargo run  --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --example triangular_arbitrage
timeout 30s  cargo run  --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --example historical_bootstrap
timeout 30s  cargo run  --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --example live_pubsub
timeout 30s  cargo run  --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --example supervised_swarm

# === decision-layer feature combos (86 / 76 / 95 tests) ===
timeout 120s cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features decision
timeout 120s cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features magnitude
timeout 120s cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features decision,magnitude
timeout 120s cargo run --release --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features decision,magnitude --example strategy_comparison

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

> **GATE — 2026-05-27 (1 of 2 design inputs received).** Input #1
> landed: integrate SwarmAgentic-style optimisation as the new Phase 5
> (was Persistence). One more design input still pending before
> implementation begins on any of Phases 5–7. The LLM stub in
> `src/llm/mod.rs` is the only code anchor in place so far; everything
> else is plan-only.
>
> Reordering: Phase 5 = SwarmAgentic features (was nothing), Phase 6 =
> Decision layer / Active Inference (unchanged scope, moved up), Phase 7
> = Persistence (was Phase 5, moved last so the SwarmAgentic + EFE
> dynamics that *generate* the events can settle before we commit to a
> durable storage format).

### Phase 5: SwarmAgentic-style optimisation  *(planned — gated, see above)*

Lift the SwarmAgentic framework (Zhang et al., 2025 — see
`docs/SwarmAgentic-summary.md` for full paper digest) into koalisi as a
meta-layer that *evolves coalition designs* by language-driven PSO.
Five concrete integration ideas, ported verbatim from the summary's
"Concrete integration ideas worth flagging for Phase 6+" section:

1. **SwarmAgentic as a `CoalitionManager` configurator.**
   Offline system discovery from a task description: SwarmAgentic
   produces an agent set `A` and collaboration structure `W`, which map
   directly onto koalisi vertices (`add_agent`) and hyperedges
   (`form_coalition`). New module `src/swarmagentic/configurator.rs` +
   `examples/swarmagentic_bootstrap.rs` showing a synthetic task →
   discovered coalition → runtime execution flow.

2. **Failure-aware velocity ↔ Active Inference EFE.** SwarmAgentic's
   flaw-driven velocity update (`c_f · r_f · F(v)`) and the planned
   Phase 6 EFE calculator are two mechanisms for self-optimisation;
   they're complementary, not competitors. The plan: EFE handles fast
   within-coalition decisions; SwarmAgentic-style LLM rewrites handle
   slow between-iteration structural changes. The trait surface in
   `src/llm/mod.rs` is where both phases meet.

3. **`ValueCalculator` extension with feedback weights.** SwarmAgentic's
   three coefficients (`c_f` failure / `c_p` personal-best / `c_g`
   global-best) are direct analogues of `WeightedCalculator`'s
   `size`/`capability`/`trust`/`synergy` weights. Add `history_weight`
   (derived from `CoalitionManager::agent_coalition_history`) and
   `failure_weight` (derived from past low-value outcomes) so the
   value-calculation feedback loop closes inside Rust without LLM
   round-trips for every score.

4. **Population-based search atop AIPA.** AIPA enumerates integer
   partitions deterministically; SwarmAgentic maintains a *population*
   of full system designs. Hybrid: AIPA generates candidate partitions,
   a SwarmAgentic-style swarm evolves agent assignments + collaboration
   policies per partition, and `TemporalHypergraph` records every
   particle's trajectory so good lineages can be replayed via
   `TemporalQueries`. The fitness function is the existing
   `ValueCalculator` trait.

5. **Cross-model transferability as a koalisi value-prop.** SwarmAgentic
   shows discovered systems transfer across LLMs. If the runtime layer
   (kameo + PubSub + libp2p gateway) stays provider-agnostic, a
   SwarmAgentic-discovered coalition spec can be re-instantiated under
   different LLM backends without re-running the search. The `LlmProvider`
   trait in `src/llm/mod.rs` is the abstraction boundary that makes this
   work — discovered specs reference the trait, not a concrete backend.

#### LLM dependency

SwarmAgentic operates as an *optimiser ≠ executor* pattern: an LLM
proposes flaw analyses + velocity rewrites; the koalisi runtime
executes the resulting design. We ship the stub `src/llm/mod.rs` now
(trait + `StubLlmProvider` that returns an error) so plan documents
and future code can reference `LlmProvider::complete(prompt)` without
committing to a backend. Real backends (OpenAI / Anthropic / Ollama /
local llama.cpp) land later behind a future `llm` feature flag with
per-backend sub-features.

#### Scope NOT covered by Phase 5

- Multimodal LLMs / vision / embodied perception — SwarmAgentic itself
  is text-only (paper §"Limitations").
- The actual offline optimisation loop driving real LLM calls — that's
  a follow-up after the stub is fleshed out.
- Persisting the discovered systems — that's Phase 7 (Persistence).

### Phase 6: Decision layer (Active Inference)  *(SHIPPED v0.6.0, 2026-05-29)*

Active Inference is a **pluggable, optional** coalition-decision strategy — never
forced on all swarms; it coexists with non-AIF strategies behind a trait, selectable
per swarm. It does **not** port `coalition_aif` (that prototype is retired/archived;
its AIF math was buggy — unnormalized belief updates, obs/state dim confusion, no-op
learning, ad-hoc RNG). Instead koalisi depends on the code-reviewed `aif` reference
engine from the `tira` repo and bridges to it.

- **Dependency:** `aif = { git = "ssh://git@github.com/sustia-llc/tira", tag =
  "aif-v0.4.0", optional = true }`, behind `[features] decision = ["dep:aif"]`. SSH
  URL (not HTTPS) because `tira` is private and git here is SSH-only — cargo's libgit2
  HTTPS fetch can't authenticate. Feature-off builds compile **no `aif` and no
  `nalgebra`**. (`aif` uses `nalgebra` internally — NOT `ndarray`; the old "adds
  ndarray dependency" note was wrong. koalisi itself adds no matrix lib: the bridge
  boundary is plain `u32`/`f64`.)
- **`src/decision/` module** (`mod.rs` always compiled; `aif_policy.rs` feature-gated):
  - `CoalitionDecisionPolicy` trait — `should_join`/`should_leave` (and dyn-compatible
    `*_async` variants returning boxed futures) over `&dyn AgentCapabilities` +
    `&[&dyn AgentCapabilities]` + `&DecisionContext` (`{ required_capabilities: u32 }`).
    Returns `Decision { act: bool, score: f64 }`. Object-safe.
  - `ThresholdPolicy<C: ValueCalculator>` — always available, non-AIF baseline. Joins
    when the candidate's **marginal** coalition value clears a threshold. Reuses the
    existing `ValueCalculator` impls unchanged.
  - `EfeValueCalculator` (feature `decision`) — impl of the EXISTING `ValueCalculator`;
    coalition value = `−G` (negated expected free energy). Slots alongside
    Additive/Synergistic/Multiplicative/Weighted.
  - `AifDecisionPolicy` (feature `decision`) — impl of `CoalitionDecisionPolicy`; joins
    iff coalition membership lowers `G` (mirrors aif's `decide_join`).
- **The capability→EFE bridge (the crux).** `aif::POMDPAgent::expected_free_energy()`
  is *policy-posterior weighted*: if membership only shifts preferences over a flexible
  observation model, the agent routes around conflict and `G ≈ 0` for everyone — the
  decision degenerates. So the bridge maps **capability coverage** of
  `required_capabilities` → observation-model **precision** of a 2-state POMDP, built
  via `POMDPAgent::new` directly (NOT via `aif::CoalitionEvaluator`, whose
  `observation_probs` can't see members). Higher coverage ⇒ sharper `A` ⇒ lower `G`
  (verified monotone: `G(0)=0.511 > G(0.5)=0.121 > G(1)=0.017`). Non-degeneracy is
  unit-tested: an agent covering a new required bit lowers `G` (joins); a redundant
  clone does not. `BridgeParams` (`max_precision` 0.95, `success_preference` 0.9,
  `alpha` 8.0) tunes the mapping.
- **Execution (sync engine, async edge).** The `aif` engine stays sync. EFE is
  CPU-bound, so `AifDecisionPolicy`'s `should_join_async`/`should_leave_async` snapshot
  capability masks to owned `u32` (the `&dyn` borrows aren't `'static`) and offload to
  the rayon pool via `tokio_rayon::spawn(..).await` — callable from a kameo handler
  without blocking the tokio worker. The async methods are on the trait via boxed
  futures, so `Box<dyn CoalitionDecisionPolicy>` callers reach the non-blocking path.
- **A/B proof:** `examples/strategy_comparison.rs` Part 1 (since K4 the example
  is `required-features = ["decision", "magnitude"]`) runs one join scenario
  under both `ThresholdPolicy(Synergistic)` and `AifDecisionPolicy` and prints
  their divergence (Threshold joins on raw marginal value; AIF declines when
  coverage doesn't improve). Part 2 is the K4 battery — see §K4 below.
- **Tests:** feature-off 30; feature-on 40 (monotonicity, coverage helper,
  non-degeneracy + degeneracy guards, leave, EfeValueCalculator ordering,
  ThresholdPolicy join/leave + object-safety + high-threshold, sync/async equivalence,
  async-via-trait-object). Both modes clippy-pedantic + `cargo doc` clean for the new
  files. NaN/±∞ margins are guarded (no decision or score made on a non-finite value).

Relation to Phase 5 (idea #2): EFE handles fast within-coalition join/leave decisions;
SwarmAgentic-style LLM rewrites handle slow between-iteration structural changes — they
meet at the `src/llm/mod.rs` trait surface.

**Decision-layer follow-ups — DONE (issues #1, #2 closed; PR #3 merged to `main`):**
- [#1](https://github.com/sustia-llc/koalisi/issues/1) — `CoalitionManager::{try_join,try_leave}_coalition`
  (policy-gated, `where V: AgentCapabilities`) + `subsystems::coalition_actor::CoalitionActor`
  (kameo seam holding `Box<dyn CoalitionDecisionPolicy>` + `DecisionContext`). The actor's
  `JoinRequest`/`LeaveRequest` consult the policy via the async offload before mutating
  membership; `AifDecisionPolicy` is never named at the seam. `AgentCapabilities` gained a
  `Send + Sync` supertrait so capability views cross `.await`. Tested in
  `tests/decision_integration.rs` (both feature modes).
- [#2](https://github.com/sustia-llc/koalisi/issues/2) — re-exported
  `TrustBeliefs`/`CompatibilityBeliefs`/`CoalitionHistory` from `aif` (no bump — `aif-v0.5.0`
  already exposes them + `belief_weighted_preference`). `BridgeParams.belief_weight` (default
  `0.0`) blends a belief alignment scalar into the **competence** driving the observation model
  (`competence = (1-w)·coverage + w·alignment`), so beliefs modulate `G` without collapsing to
  a preference-only shift — non-degeneracy (B2/B4) preserved. `AifDecisionPolicy::with_beliefs`
  carries the beliefs; with `belief_weight > 0` a trusted redundant agent can join, a distrusted
  coverage-improving partnership can be declined, and history shifts the margin. Leave is the
  symmetric dual of join (`comp_out` neutral `0.5` = "agent not in coalition"). Trust
  reconciliation: `trust_level()` = static baseline, `TrustBeliefs` = dynamic EMA (no koalisi
  `TrustGraph` exists). Tested in `decision/aif_policy.rs` (7 unit) + `decision_integration.rs`
  (belief-aware join through the live actor). 86 tests `--features decision`.

Cross-project plan (upstream `aif` + this Phase B): see
`~/Documents/iwahi/tira/.claude/plans/aif-merge-koalisi-integration.md`.

**K2 — magnitude decision arm ([#5](https://github.com/sustia-llc/koalisi/issues/5), DONE 2026-07-02).**
Part of the coalition semantic-layer roadmap (Phase K, plan in
`~/Documents/tsondru/tsondru-notes/catgraph/plans/2026-07-01-coalition-semantic-layer.md`).
The categorical A/B mirror of the AIF arm, behind feature `magnitude`
(independent of `decision` — either, both, or neither):
- Dep: `catgraph-magnitude = { git = "ssh://git@github.com/sustia-llc/catgraph",
  tag = "v0.1.1", optional = true }` (shipped at `v0.1.0`; bumped for the
  catgraph#29 triangle-tolerance fix). SSH not HTTPS (catgraph is private; the
  issue-#5 pinned dep line says HTTPS but cargo's libgit2 can't authenticate —
  same story as `aif`/tira). `coalition_value` = magnitude at pinned `t = 1`;
  the t-sweep belongs to the K4 A/B harness (#7).
- **Mapping (the semantic heart)**: directed substitutability
  `A(i→j) = |rel_i ∩ rel_j| / |rel_i|`, `rel = caps & required`. Clones
  (identical relevant masks) are mutually 1.0 ⇒ upstream skeletalizes them into
  ONE effective agent (deliberate — mirrors AIF clone degeneracy); subsumed
  agents get Möbius weight 0; disjoint specialists count fully (`Mag = m`).
- **Gotcha (review-caught, hand-verified)**: task-irrelevant agents
  (`rel == 0`) must be EXCLUDED from the member set, not vacuously coupled at
  1.0 — a one-way 1.0 coupling to every member drives the bystander's Möbius
  weight negative and *collapses* diversity (3 specialists + 1 bystander ⇒ 1.0,
  and the bystander's presence ejects a unique specialist on leave). Regression
  test: `irrelevant_bystander_neither_collapses_value_nor_corrupts_decisions`.
- Join iff `Mag(with) − Mag(without) > join_margin`; leave iff removing the
  agent doesn't lower Mag (exact AIF dual). Upstream `CatgraphError` ⇒
  policy-level decline / `-∞` value, never a panic. Async offloads the whole
  `O(m³)` computation to rayon (not just the final eval as in the AIF arm).
- No cross-arm scalar calibration (pinned): only within-arm rank order matters;
  the A/B outcome metric is pre-registered on
  [#7](https://github.com/sustia-llc/koalisi/issues/7) (K4 consumes this arm).

**K4 — A/B harness ([#7](https://github.com/sustia-llc/koalisi/issues/7), DONE 2026-07-02).**
`examples/strategy_comparison.rs` (`required-features = ["decision", "magnitude"]`,
run `--release`): Part 1 = the original Threshold-vs-AIF divergence demo
(unchanged); Part 2 = the #7-pre-registered battery. Committed run:
`docs/ab-report-K4-yamafaktory.md` (yamafaktory backend, pre-K1; deterministic
except latency — SplitMix64 inline, no `rand` dep).
- **Result: `FALSIFIED (latency)` under v1; `VALIDATED (B)` under v2.** Quality:
  magnitude superior in 30/30 seeds (median primary 0.4469 vs 0.1898), churn
  8 vs 113, oracle regret 0.1156 vs 0.3757. Latency: 4.37 µs vs 1.48 µs median
  per decision (the O(m³) Möbius closure vs AIF's fixed 2-state POMDP) — fails
  the v1 strict gate. Run 1's recorded v1 verdict stands.
- **Criterion amendment v2** (#7 comment 2026-07-02, posted before any re-run —
  post-hoc w.r.t. run 1, pre-registered w.r.t. all subsequent runs): VALIDATED
  iff **Path A** (original: non-inferiority + strictly lower latency) OR
  **Path B** (quality dominance: median ≥ 1.25× AIF, strictly superior in
  ≥ 60% of seeds, latency ≤ 10× AIF). Harness prints BOTH verdicts (v1 + v2)
  for cross-run comparability. Run 1 under v2: Path B passes on all three legs.
- **Latency follow-up**: incremental/paired coalition-magnitude evaluation
  filed upstream as catgraph#31 (O(m²) bordered updates for the join sweep) —
  non-gating under v2, strengthens Path A; matters more at larger pools.
- **Protocol decisions** (pre-reg left open, documented in the example): first
  arrival joins unconditionally (AIF's join margin from an empty coalition is
  exactly 0 with a strict `>`, so it cannot self-start); one leave sweep per
  task, all removals count as churn; latency measured on the sync path, warm.
- **Upstream find (RESOLVED)**: the battery's non-dyadic couplings tripped a
  debug-only over-strict triangle-inequality `debug_assert` in
  `catgraph-magnitude v0.1.0` (ULP noise: `−ln(a·b)` vs `−ln a + −ln b`) —
  catgraph#29, fixed by catgraph PR #30 (merged, tagged `v0.1.1`); koalisi dep
  bumped to `v0.1.1` and debug builds run clean. Release builds were never
  affected. `--release` remains required for the latency criterion only.
- **Deferred (pre-registered)**: backend-parity re-run after K1 (#4) lands —
  same battery on the catgraph backend, results must match within noise.
- Exploratory t-sweep lives in the example (`TSweepMagnitudePolicy`), NOT the
  library — t = 1 stays the pinned stable arm (catgraph #22). Sweep medians
  were flat (0.4428–0.4490 across t ∈ {0.5, 1, 2, 10}).

### Phase 7: Persistence  *(planned — gated, see above; was Phase 5, moved last)*

> **K1 impact (2026-07-02): the original `PersistentHypergraph` approach is
> OBSOLETE.** It came from yamafaktory hypergraph v4.2.0, which K1 (#4)
> dropped; `catgraph_applied::Hypergraph` deliberately has no serde/persistence
> (upstream design). Phase 7 must re-plan graph-state durability — likely
> event-log-only (the `EventStore` trait below + point-in-time reconstruction
> already covers state rebuild), or a catgraph-side snapshot format. Also note
> K3's surrealdb-live-message direction seeds the message-event stream side.

Feature-gated persistence via an `EventStore` trait for temporal event
durability. Default impl: append-only file log with `rmp-serde`. Moved to the
end of the pipeline so the SwarmAgentic optimisation traces (Phase 5) and
Active Inference belief states (Phase 6) inform the persistence schema
before we commit to a wire format. See `.claude/plans/` for the
original design (graph-snapshot part superseded per the K1 note; new
requirement is that `EventStore` must also be able to durably record
SwarmAgentic particle lineages and EFE belief snapshots).

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
