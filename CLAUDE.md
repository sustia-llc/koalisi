# CLAUDE.md — koalisi

Working-state document. See `README.md` for user-facing description.
This file is for picking the project back up later.

When you bump the project's behaviour, also:
- update `Cargo.toml` `version`
- add a new top section to `CHANGELOG.md` (Keep a Changelog format)
- update the relevant entries here (Current state, Worth flagging,
  File inventory) so future-me doesn't relitigate decisions
- cut + push an annotated `v{X.Y.Z}` tag on the release commit (the merge,
  for PR releases) — per-release tagging resumed 2026-07-27, owner call;
  v0.7.0–v0.15.0 were backfilled onto their merges the same day

**Review protocol (standing, owner 2026-08-05): EVERY PR gets `/code-review
low` before tag/merge — re-pin and deps+docs slices included** (suites,
clippy and a byte-identical battery cannot detect stale docs, the failure
mode a deps-only PR carries most often). Every finding is applied or
owner-adjudicated, whatever its severity. Reviewer output is evidence, not
verdict: check each claim against the diff. Full protocol:
`docs/PROTOCOL.md` §4.

**Maintenance rule**: new release entries go in FULL to the
`project-history.md` archive (§1); this file keeps only the latest three. New
gotchas: an engineering contract goes here in full, an A/B-lineage one goes to
the `ab-lineage-gotchas.md` archive with a one-line index entry here. **Keep
this file under 150k chars** — past that the harness truncates it and the tail
silently stops reaching the session.

## Where the rest of the record lives

This file is the working state. Two archives are held **outside this
repository** (owner call, 2026-08-09); their location is in `CLAUDE.local.md`:

- **`project-history.md`** — the full release ledger v0.4.0 → present verbatim,
  the Phase 5/6/7 narratives, the K1–K6 sections, the downstream/removed-work
  notes, and the obsolete gotchas 1–6 / 8–10.
- **`ab-lineage-gotchas.md`** — gotchas 20–28 and 30–33 verbatim (the
  A/B-registration lineage). Indexed one line each at the end of §Worth
  flagging below; **read the full text there before designing or running any
  K4-lineage registration.**
- **`docs/`** (in-repo, public) — the A/B showcase trail, indexed by
  `docs/README.md` (verdict trail + seed ledger) with the run protocol in
  `docs/PROTOCOL.md` and raw run outputs under `docs/runs/`. Registered docs
  are IMMUTABLE; they carry the public record of every verdict.
- **`.claude/docs/`** (in-repo, tracked) — the public internal design docs that
  stayed: the Phase 7 persistence design, the K3 hot-path bench, the
  SwarmAgentic digest + paper.

Gotcha numbering is preserved across the split, so existing cross-references
resolve **for holders of the archives**. Be aware of the limit: rustdoc and
report references to relocated gotchas — e.g. `src/decision/group_policy.rs`
(24, 25, 28, 32), `src/decision/reliability_value.rs` (21),
`src/process/residual.rs` (28), `CHANGELOG.md` (21) and
`docs/k4-arm-choice-memo.md` (20–23) — now name text that is NOT in this
repository. A public reader, or any session without the archives, cannot follow
them. The retained gotchas (7, 11–19, 29) are unaffected and self-contained.

## Mission (one paragraph)

**koalisi** — a reference implementation of agentic coalitions in Rust.
Four layers: Core (`CoalitionRuntime`, lifecycle), Topology (temporal
hypergraph over `catgraph_applied::Hypergraph`, event sourcing,
`CoalitionManager`, time-travel queries, analytics), Algorithms (DCVC workload
distribution, AIPA partition search, pluggable value calculators, population
structure search), and Runtime (tokio tasks with mpsc/oneshot command handles,
the `CoalitionService` policy-gated membership seam, a task-restart layer, an
optional SurrealDB-backed durable decision log, an optional libp2p remote
coalition-event gateway). The architecture is domain-agnostic; the
demonstrated runtime is a synthetic coalition-formation pipeline, and the
showcase is the pre-registered A/B trail in `docs/`. Market/trading work lives
in the sibling `biome` project.

## Available tooling for this project

- **No DeepCausality crate is in koalisi's lock under any feature**
  (`rg -n 'deep_causality|ultragraph' Cargo.lock` → nothing); the `causality`
  plugin's substrate skills describe crates that are not in the graph — do
  not route there. The topology backend contract is the module docs of
  `catgraph-applied/src/hypergraph.rs` at the pinned tag.
  `causality:causality-theory` stays apt as theory reference for the
  algebraic layer catgraph's enrichment sits on.
- Rust implementation is dispatched to the built-in `general-purpose` agent
  per `.claude/stack/agent-dispatch.md`; review is `/code-review low`.

## Current state — 2026-09-17 (v0.36.0)

Full release ledger v0.4.0 → v0.36.0 is the `project-history.md` archive (§1).
The three most recent entries are kept here in brief — read the ledger before
touching anything with a frozen battery, a pinned decision, or a registered doc.

### Latest three

- **Metrics example + lock refresh — v0.36.0 (2026-09-17, #25, PR #94)**: #25
  re-scoped by comment from the deleted `tick_bus`/`alert_bus` to the three
  tap surfaces, then feature `metrics` (off by default: optional `metrics`
  0.24 + `metrics-exporter-prometheus` 0.18 `http-listener` only, tokio
  `net` + `io-util`) and `examples/metrics_scrape.rs` — counters and
  histograms from a `spawn_decision_tee` sink, an `OutcomeSink` under
  `spawn_outcome_forwarder` and the `with_event_tap` receiver; one
  self-scrape of `/metrics`, every series asserted against a recorder-free
  count and the scripted value; eight perturbations falsified in a copy.
  No `src/` change. **`cargo update` rides as the PR's first commit** (owner
  call mid-session; 154 updated / 15 added / 21 removed, `wide` 1.7.1 and
  `safe_arch` 1.2.0 among them); the feature commit adds 14 lock stanzas
  and moves none. Fourteen suites identical per test binary on a `3ff92f6`
  worktree and on the branch (`metrics` lane 106); fmt, clippy (fifteen
  lanes) and doc (twelve sets) clean; **X-battery PASS on one serial run**
  (124 raw differing lines, 11 hunks after the column strip, all latency or
  wall-clock; 33 verdict lines byte-identical); MSRV tiers reproduced
  (1.88 / 1.89 / 1.92), `metrics` in the 1.88 tier. The service task runs
  on a bare `tokio::spawn` and owns the manager, so the topology tap closes
  only after `drop(service)` lets that task end — the example awaits the
  topology consumer's handle for that reason.
- **K7 harness scaffold H0 + H1 — v0.35.0 (2026-09-16, #92, PR #93)**: memo →
  owner lock on #92 (six items, all as recommended) → code.
  `src/harness/workflow.rs` (needs `process`) is the Part 9/11 v2w world
  copied out of the frozen binary — `WorkflowSpec` (v2 prefix + roles +
  tags with feasibility re-draw + shape draw + optional `PerformanceSpec`
  appended last), `WorkflowInstance`/`WorkflowTask`, `OutcomeSignal`
  {RoleCoverage, Performance, Both}, `WorkflowArm` (per-instance factory),
  `run_workflow_instance`/`run_workflow_battery` with Part 11's
  role-matched distinct-step scorer; `InstanceSpec::draw(&mut rng)` split
  out of `generate`. `CoalitionDecisionPolicy` gains default no-op
  `begin_task(&TaskStart)` / `observe_outcome(required, &[bool])`, called
  once per task by both runners; `Demand::from_steps`. **Identity gate**
  `tests/harness_workflow.rs`: the copy driving `wf-asis` reproduces Part
  9's thirty per-seed rows on 270..300 (`docs/runs/K4-archive.log:1787–1816`)
  and Part 11's medians 0.1806 / 7.50 on 330..360 (`:2031`) — the log
  prints NO per-seed `wf-asis` on 330..360, a correction to the plan's
  gate text; `tests/fixtures/k7-workflow-v2w.txt` pins every instance on
  both blocks. Frozen binary diff empty; gauntlet byte-identical bar the
  latency column. Thirteen suites (`process` 161, dmp 241, `harness` 126,
  new `harness,process` 198, rest unchanged); clippy/doc/fmt clean at
  every lane; MSRV tiers reproduced (1.88 / 1.89 / 1.92); **X-battery PASS
  on the second of two serial runs** — the first flipped the v1 latency
  criterion (gotcha 34's Path A noise; every non-latency line identical),
  the re-run reproduced all 33 verdict lines. Base-tree red fixed: two
  `persistence`-gated doc links in `remote.rs`. Review: no findings.
- **Housekeeping re-pin + trim — v0.34.0 (2026-09-16, PR #91)**: `sha2`
  0.10 → 0.11, `libp2p` 0.56 → 0.57, `surrealdb-types` 3.2.1 → 3.2.4, lock
  refreshed (+22/−7 stanzas; `sha2`/`digest` 0.10 and 0.11 now coexist —
  koalisi on 0.11, the surrealdb stack on 0.10). Whole tree rustfmt-clean
  under rustfmt 1.9.0 (38 files, whitespace only, the frozen archive binary
  included); 18 intra-doc links fixed so `cargo doc -D warnings` is green
  at every feature set; `Cargo.toml`, this file and README trimmed to what
  they state. **All twelve suites at baseline + `harness` 125; X-battery
  PASS with zero non-latency diffs** (one serial run vs
  `docs/runs/K4-archive.log`: 61 raw differing lines, 11 latency/timing
  survivors after the column strip, all 33 verdict lines identical); MSRV
  tiers unchanged on the new lock (1.88 / 1.89 / 1.92), `rust-version`
  1.93 kept.

### Lineages, verdict trail, seed ledger, run protocol — `docs/`

Since v0.33.0 these live in the public repo: **`docs/README.md`** carries the
K4 verdict trail (17 rows, every prereg + report by path), the K7 lineage
rows as they land, and the **seed ledger** (90..120 reserved for K7-1,
150..180 reserved-unconsumed); **`docs/PROTOCOL.md`** is the standing run
protocol (design-lock → prereg → 3-lens review → serial run → immutable
report; pin-first; latency-column-stripped diff; review on every PR);
**`docs/runs/`** holds the committed raw outputs and the re-pin drift recipe.
The per-registration gotcha numbers are the index at the end of §Worth
flagging. Rust implementation is dispatched per
`.claude/stack/agent-dispatch.md`.

### Tests passing

| Suite | Tests | Command |
|---|---|---|
| Default | 106 | `cargo test` |
| `--features decision` | 162 | `cargo test --features decision` |
| `--features magnitude` | 135 | `cargo test --features magnitude` |
| `--features decision,magnitude` | 191 | `cargo test --features decision,magnitude` |
| `--features magnitude-fast` | 143 | `cargo test --features magnitude-fast` (EQ3 L2+L3 + probes) |
| `--features persistence` | 126 | `cargo test --features persistence` |
| `--features persistence,magnitude` | 156 (incl. the #18/#30 replay parity gate) | `cargo test --features persistence,magnitude` |
| `--features remote` | 112 (gateway buffer + loopback round-trip) | `cargo test --features remote` |
| `--features process` | 161 (EQ5a surface + #80 ResidualPolicy + `Demand::from_steps`) | `cargo test --features process` |
| `--features decision,magnitude,process` | 241 (the Part 9 + Part 10 + Part 11 batteries) | `cargo test --features decision,magnitude,process` |
| `--features durable` | 107 (+1 container-backed restart test; needs Docker) | `cargo test --features durable` |
| `--features harness` | 126 (+20 K7 harness unit tests incl. the H1 hook order pin) | `cargo test --features harness` |
| `--features harness,process` | 198 (+ the H0 workflow world: 13 unit tests, the 5-test identity gate `tests/harness_workflow.rs` against `docs/runs/K4-archive.log`) | `cargo test --features harness,process` |
| `--features metrics` | 106 (the default suite; the feature gates two optional deps and `examples/metrics_scrape.rs`) | `cargo test --features metrics` |
| All examples | exit 0 | see Reproducers below |
| Lint + docs | clean | `cargo fmt --check`; `cargo clippy --all-targets -- -D warnings` per feature set; `RUSTDOCFLAGS='-D warnings' cargo doc --no-deps` at every feature (green since v0.34.0) |

### File inventory

```
koalisi/
├── Cargo.toml                              git tag deps: catgraph-applied + catgraph-magnitude + catgraph-syntax v0.23.0 in lockstep (one checkout); aif-v0.13.0, surrealdb-live-message v0.2.1, libp2p 0.57, sha2 0.11, metrics 0.24 + metrics-exporter-prometheus 0.18 (optional); no path deps; declared MSRV 1.93 above the measured tiers 1.88/1.89/1.92 — owner decision C-D1: KEEP, gotcha 34
├── README.md                               user-facing
├── CLAUDE.md                               THIS FILE
├── config/{default,development,test}.toml  coalition threshold, history capacity; [sdb]+[docker] for the durable feature's upstream SETTINGS (cwd-resolved)
├── src/
│   ├── lib.rs                              module surface + re-exports
│   ├── main.rs                             domain-neutral reference daemon: CoalitionRuntime + seed coalition + policy-gated join loop via CoalitionService (v0.11.0)
│   ├── core/
│   │   ├── mod.rs                          re-exports
│   │   ├── config.rs                       Settings + CoalitionSettings + setup_logging
│   │   ├── runtime.rs                      CoalitionRuntime (TaskTracker + CancellationToken)
│   │   └── supervision.rs                  spawn_supervised (K3 task-restart layer, replaces kameo OneForOne)
│   ├── topology/
│   │   ├── mod.rs                          re-exports + hypergraph type re-exports
│   │   ├── timestamp.rs                    Timestamp, TimeRange, Clock + 8 unit tests
│   │   ├── events.rs                       TemporalEvent (13 variants), EventStats
│   │   ├── event_log.rs                    EventLog with BTreeMap time + HashMap entity indices
│   │   ├── errors.rs                       TemporalError, TemporalResult
│   │   ├── temporal.rs                     TemporalHypergraph<V, HE>, SharedGraph, Snapshot
│   │   ├── queries.rs                      TemporalQueries (point-in-time state)
│   │   ├── analytics.rs                    TemporalAnalytics, GraphDelta + magnitude_history/MagnitudePoint (#18, feature `magnitude`)
│   │   ├── coalitions.rs                   CoalitionManager (form/join/leave/dissolve/merge; agent_coalition_history pub + seed_feedback_history since #41)
│   │   └── executor.rs                     HypergraphExecutor (rayon↔tokio bridge)
│   ├── algorithms/
│   │   ├── mod.rs                          AgentCapabilities trait + CapabilityAgent (stock impl, also a VertexTrait; v0.11.0) + re-exports
│   │   ├── value_calculation.rs            ValueCalculator + 4 base calculators
│   │   ├── feedback.rs                     FeedbackCalculator<C> wrapper + shared FeedbackStore (history/failure weights, #41)
│   │   ├── dcvc.rs                         DCVCDistributor, WorkloadShare
│   │   ├── aipa.rs                         Integer partitions, bounds, best-partition + 10 unit tests
│   │   └── population.rs                   P5.2 (#42): population coalition-structure search atop AIPA (SplitMix64 PSO, gbest lineage) + record_trajectory (always compiled, no deps)
│   ├── harness/                            v0.33.0: the K7 harness (feature `harness`, no deps) — rng.rs (SplitMix64, permutation, distinct_bits), instance.rs (InstanceSpec → Instance over CapabilityAgent; `draw(&mut rng)` since v0.35.0), battery.rs (SeedRange, Arm, run_instance — calls the H1 hooks once per task — run_battery, FLAT_SIGNAL_WIDTH), report.rs (percentile, median_iqr, superior_count, tables, Verdict), workflow.rs (v0.35.0, needs `process`: WorkflowSpec/PerformanceSpec → WorkflowInstance/WorkflowTask, OutcomeSignal, WorkflowArm, run_workflow_instance, run_workflow_battery — the Part 9/11 v2w world copied out of the frozen binary); registrations are examples, never here
│   ├── decision/
│   │   ├── mod.rs                          CoalitionDecisionPolicy (+ the v0.35.0 default no-op lifecycle hooks begin_task(&TaskStart) / observe_outcome(required, &[bool])) + ThresholdPolicy (always compiled)
│   │   ├── aif_policy.rs                   AifDecisionPolicy + EfeValueCalculator (feature `decision`)
│   │   ├── reliability_value.rs            ReliabilityCoverage (#57, v0.16.0): reliability-weighted coverage ValueCalculator from the persistent world-model snapshot (feature `decision`; gotcha 24)
│   │   ├── group_policy.rs                 GroupAifPolicy (#78, v0.30.0): aif::GroupAgent, R=3 role internals over arm-E1 world models, CertaintyWeighted, deterministic group_distribution read (features `decision`+`process`; gotcha 33)
│   │   └── magnitude_policy.rs             MagnitudePolicy + MagnitudeValueCalculator + CouplingModel + CoalitionEvaluator cache (K6) (feature `magnitude`); relevant_masks/magnitude_or_zero pub(crate) for #18
│   ├── process/                            EQ5a (#76, v0.27.0): process-structured tasks (feature `process`; gotcha 30)
│   │   ├── mod.rs                          re-exports + the four things a caller must know
│   │   ├── signature.rs                    Role (Color) + Step { bit, role } : r → r + Workflow = ColoredExpr<FrobeniusOr<Step>>
│   │   ├── demand.rs                       Demand + demand(): multiset over User occurrences, distinct set (spiders contribute none)
│   │   ├── theory.rs                       rule_theory() — the 3 schemas closed over (bit, role), 174 instances; Schema/LabelledRule/fusion_pairs
│   │   ├── cost.rs                         uniform_cost + StaffingTable + staffing_price (Amendment A1.3)
│   │   ├── rewrite.rs                      optimize_workflow + verify_optimization (replay + content_eq — the S-sound helper)
│   │   ├── residual.rs                     #80 (v0.28.0): ResidualPolicy — the unstaffable-residual valuation lever, promoted out of EQ5a's example side (gotcha 31)
│   │   └── errors.rs                       ProcessError
│   ├── ingest/                             K5 (#8): domain-neutral ingestion layer (always compiled, no new deps)
│   │   ├── mod.rs                          re-exports
│   │   ├── sample.rs                       Sample trait (Key routing + timestamp_ms + View)
│   │   ├── monitor.rs                      SampleMonitor<S> + SampleUpdate/Snapshot + handle + spawn (generic ring-buffer monitor; K3 contracts verbatim)
│   │   ├── source.rs                       DataSource trait + Pacing + PumpStats + pump_source/spawn_source_pump
│   │   └── synthetic.rs                    MultiResolutionSource (NEST-shaped) + SensorEventSource (tauhokohoko-shaped, changepoint)
│   ├── llm/
│   │   └── mod.rs                          LlmProvider trait + StubLlmProvider (Phase 5 anchor)
│   ├── persistence/                        P7.1 (#29): chained event log (feature `persistence`)
│   │   ├── mod.rs                          feature docs (hash contract, durability, at-most-once) + pub surface
│   │   ├── envelope.rs                     StreamId/SequenceNo/RecordHash/Payload/EventRef/Record/StoredRecord/StreamHead
│   │   ├── errors.rs                       PersistenceError (hand-rolled; StreamWedged; P7.3/P7.5 anchor variants)
│   │   ├── chain.rs                        FrameV1 (private serde mirror), FRAME_VERSION, hashing, back-link check
│   │   ├── store.rs                        EventStore trait + FileEventStore (segments, rotation, torn-tail recovery, wedge)
│   │   ├── writer.rs                       spawn_store_writer (spawn_blocking, drain-on-cancel)
│   │   ├── wire.rs                         WireTopologyEvent<VW,HW> (13-variant serde mirror, u64 fields) + schema version (#30)
│   │   ├── tee.rs                          spawn_topology_forwarder (tap → CBOR → store writer; shutdown disciplines) (#30)
│   │   └── replay.rs                       replay_into_event_log (batched read → fresh EventLog; quiescence precondition) (#30)
│   └── subsystems/
│       ├── coalition_actor.rs              CoalitionService + handle (policy-gated membership seam, #1) + DecisionRecord tap (K3) + spawn_decision_tee (#38, always compiled) — THE runtime seam
│       ├── outcome.rs                      #55 (v0.14.0): TaskOutcome + OutcomeSink fan-out + emit_outcome tap + spawn_outcome_forwarder (always compiled; the L2 outcome seam)
│       ├── remote.rs                       #38 (v0.25.0): libp2p request-response coalition-event gateway + EventBuffer + RemoteCoalitionClient (feature `remote`; gotcha 29)
│       └── durable.rs                      DecisionEvent + DurableDecisionBus + forwarder (feature `durable`, K3)
├── examples/
│   ├── topology_coalition.rs               coalition lifecycle + time-travel queries
│   ├── algorithm_values.rs                 value calculators + DCVC + AIPA
│   ├── synthetic_ingestion.rs              FLAGSHIP (v0.11.0): NEST + sensor fixtures → generic monitors → coalition formation via CoalitionService (default features)
│   ├── supervised_monitor.rs               spawn_supervised restart demo over SampleMonitor<SensorEvent> (v0.11.0; was supervised_swarm)
│   ├── population_search.rs                #42: TaskCoverage-driven structure search + record/replay (default features; was missing from this inventory — added v0.16.0)
│   ├── population_reliability.rs           #57 (v0.16.0): outcome stream → world-model snapshot → ReliabilityCoverage → search + replay (feature decision)
│   ├── strategy_comparison.rs              FROZEN K4 archive binary (Parts 1–11; requires ALL THREE: decision,magnitude,process); changes only through src/; its job is the X-battery gate at re-pins against docs/runs/K4-archive.log
│   ├── gauntlet.rs                         v0.33.0: K7 harness skeleton over src/harness/ (feature `harness`), zero registrations; K7 registrations are one [[example]] each under examples/k7/k7_<n>.rs (K-D3)
│   ├── remote_coalition_consumer.rs        #38 (v0.25.0): gateway + client in one process over a live CoalitionService (feature `remote`)
│   ├── metrics_scrape.rs                   #25 (v0.36.0): Prometheus counters + histograms over the decision tee, the outcome forwarder and the topology event tap; self-scrapes /metrics once and asserts every series against a recorder-free count (feature `metrics`)
│   └── durable_decisions.rs                durable decision log end-to-end (feature `durable`)
├── .claude/docs/                           TRACKED internal design docs + references (docs/ reorg 2026-07-27; rest of .claude/ stays gitignored)
│   ├── phase7-persistence-design.md        Phase 7 EventStore design (#21 deliverable; P7.1–P7.5 phasing)
│   ├── k3-hot-path-bench.md                K3 kameo-vs-tokio bench evidence
│   ├── SwarmAgentic-summary.md             Phase 5 paper digest (Zhang et al. 2025)
│   └── 2506.15672v1.{md,pdf} + _images/    the SwarmAgentic paper itself (CC0 per its PDF metadata)
├── docs/                                   PUBLIC A/B showcase trail (registered docs are immutable)
│   ├── README.md                           v0.33.0: the index — K4 verdict trail (17 rows, every prereg/report by path), K7 rows, seed ledger, immutability rule, layout
│   ├── PROTOCOL.md                         v0.33.0: the run protocol (design-lock → prereg → 3-lens review → serial run → immutable report; gates; review; seeds; naming)
│   ├── runs/                               v0.33.0: committed raw outputs — README.md (archive + drift-check recipe), K4-archive.log (one serial run at v0.32.0 pins), K7-<n>.log per registration
│   ├── prereg-*.md + ab-report-*.md        the K4 lineage: 13 prereg + 16 report pairs/singles (v1/v2, K1, K6 report-only), each row of docs/README.md names both files; per-registration gotchas indexed at the end of §Worth flagging
│   └── baseline-aif-scalar-scope-b.md, per-bit-outcome-plumbing-design.md, k4-arm-choice-memo.md   the three memos (v4/v5 baseline anchor; #54 Step 2 design note, gotcha 23; #54 Step 4 decision memo — DECIDED B+D, FINAL)
└── tests/
    ├── topology_test.rs                    12 tests
    ├── algorithms_test.rs                  18 tests (incl. 3 feedback-loop/seeding tests, #41)
    ├── decision_integration.rs             4–6 tests (feature-dependent)
    ├── durable_integration.rs              1 container-backed restart test (feature `durable`)
    ├── ingestion_integration.rs            3 tests (K5: synthetic sources → monitors → coalition formation; default features)
    ├── magnitude_trajectory.rs             6 tests (#18: hand-computed trajectory semantics; feature `magnitude`)
    ├── population_test.rs                  4 tests (#42: population coalition-structure search; default features)
    ├── common/                             shared fixtures for the integration suites (mod.rs + algorithms.rs + topology.rs)
    ├── persistence_integration.rs          7 tests (#29: roundtrip, rotation+reopen, tamper, torn tail, sealed opaque, writer drain, bounds; feature `persistence`)
    ├── topology_replay.rs                  3 tests (#30: 13-variant round-trip, reconstruction equality, schema/Sealed rejection; feature `persistence`)
    ├── remote_integration.rs               1 test (#38: loopback round-trip service → tee → gateway → client, cursor deltas + seq ordering; feature `remote`)
    ├── replay_parity.rs                    1 test (#30: magnitude_history live == replayed — THE parity gate; features `persistence,magnitude`)
    ├── harness_workflow.rs                 5 tests (v0.35.0, #92: the H0 identity gate — Part 9 `wf-asis` rows on 270..300 + Part 11 medians on 330..360 from docs/runs/K4-archive.log, the instance fixture, the v2-prefix pin, a hand-derived role-mismatch case; features `harness,process`)
    └── fixtures/k7-workflow-v2w.txt        the committed print of every generated v2w instance on 270..300 and 330..360 (regenerate with K7_WRITE_FIXTURE=1 — only when the world changes by design)
```

Two further files belong to this project but are **not in this tree** — the
`project-history.md` and `ab-lineage-gotchas.md` archives (see §Where the rest
of the record lives; location in `CLAUDE.local.md`).

## Worth flagging (gotchas)

These cost time during the build; future-me should not relearn them.
Engineering contracts (7, 11–19, 29) are verbatim below. The A/B-lineage
gotchas (20–28, 30–33) are indexed at the end of this section and live in full
in the `ab-lineage-gotchas.md` archive. Obsolete ones (1–6, 8–10 — all
kameo / forex / databento-era) are in the `project-history.md` archive §4.
Both archives are held outside this repo; see `CLAUDE.local.md`.
Numbering is preserved across all three files.

7. **Cargo target dir + timeout convention (project-wide).**
   - We use `--manifest-path Cargo.toml --target-dir /tmp/koalisi-target` to avoid contention with the IDE's own `cargo check`. Run from inside the `koalisi` worktree.
   - Wrap with `timeout 30s` (or 60s, 120s as appropriate) so a hang in a freshly-built binary is killed cleanly, not just the shell wrapper.
   - Pattern: `timeout 30s cargo run --manifest-path Cargo.toml --target-dir /tmp/… --example foo 2>/dev/null ; echo "exit=$?"`. Exit 124 = unix `timeout` fired.

11. **libp2p `#[derive(NetworkBehaviour)]` requires the `macros` feature.** LIVE AGAIN since v0.25.0 (#38 re-added the gateway; the dep line carries `macros`). Was obsolete v0.11.0–v0.24.0 while the `libp2p` dep was gone.

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

13. **K3 runtime contracts (tokio::sync seams).** *(The forex swarm bullets —
    `tick_bus`/`alert_bus` broadcast buses, the `flush()` monitor→coordinator→
    sink drain barrier, `SwarmFeeder` — were removed with the swarm in v0.11.0
    (#37). What survives:)*
    - **`SampleMonitorHandle::feed` is acknowledged** (an oneshot ack barrier);
      `::tell` is fire-and-forget. The generic `ingest` monitor keeps this
      distinction; `Ping`/snapshot handlers drain the buffered `broadcast`
      before replying, which is what keeps the ingestion tests deterministic —
      don't "optimize" the drain away.
    - **Restart layer**: `spawn_supervised` rebuilds from the factory on PANIC
      only (token cancellation is not a failure); sliding-window
      `restart_limit`; exceeding gives up + cancels the child token.
      Demonstrated by `examples/supervised_monitor.rs`.

14. **`durable` feature gotchas (surrealdb-live-message v0.2.1).**
    - **Upstream `SETTINGS` resolves from the CONSUMER's cwd**: `config/default`
      (required) + `config/{RUN_MODE}` + env. koalisi's `config/default.toml`
      carries `[sdb]` + `[docker]` for it (inert feature-off). Running durable
      tests/examples from another cwd breaks settings resolution.
    - **`SurrealValue` derive emits absolute crate paths**: koalisi pins
      `#[surreal(crate = "::surrealdb_types")]` on `DecisionEvent` (hence the
      small gated `surrealdb-types` dep). Without the pin the derive resolves
      `::surrealdb::types`, which koalisi doesn't depend on.
    - **Linking `serde_json` (via surrealdb) makes untyped empty-vec asserts
      ambiguous** (`impl PartialEq<Value> for usize`): write
      `vec![Vec::<usize>::new()]`, not `vec![vec![]]` — bit aipa tests once.
    - **The decision tap never blocks**: `try_send` + drop-with-warn on
      full/closed. Durability is at-least-once from the tap ONWARD; a dropped
      tap record is a koalisi-side loss (size the channel accordingly).
    - Docker required for the container-backed test; upstream's
      `SurrealDBContainer` (bollard) manages the instance.

15. **K6 evaluator-cache contracts (magnitude arm, #14) — rely on these.**
    - **Rank-order identity does NOT freeze decisions.** The catgraph#31
      amendment guarantees `value_with` ranks candidates identically to fresh,
      but `MagnitudePolicy` compares margins against an absolute threshold
      (`> join_margin`, default 0): candidates with *mathematically zero*
      margins (subsumed/redundant masks — the majority of declines) are decided
      by ±1e-16 float noise, and incremental noise ≠ fresh noise. The
      **knife-edge fresh fallback** (`KNIFE_EDGE_REL_BAND = 1e-6` rel) recomputes
      the `with` side fresh inside the band — that is what keeps the battery
      seed-for-seed reproducible. Never remove it while decision behavior is
      pinned; widening the band only costs latency, narrowing it risks flips.
    - **`base_value()` bit-identity survives pool extension**: extra candidate
      agents/couplings in the evaluator pool don't perturb the base coalition
      (restrict-then-close drops them) — pinned by
      `base_value_bit_identical_with_nonempty_registry`.
    - **Registry retention is measured, not aesthetic**: scoping the candidate
      registry to one `required` degenerates to rebuild-per-decision on
      arrival-sweep streams (each task = fresh requirement, each candidate seen
      once) and regressed the battery median BELOW the pre-K6 baseline
      (4.90 vs 3.915 µs). Retained-with-cap (256) is the measured optimum.
      Evaluator construction ≈ 10–15× a plain `coalition_value` (cache
      extraction + coupling HashMap) — catgraph#33 territory, don't "fix" it
      downstream.
    - `MagnitudePolicy`/`MagnitudeValueCalculator` are no longer `Copy`;
      `Clone` SHARES the cache (Arc). One instance per concurrent
      membership-stream, or accept rebuild thrash (correct, just slower).

16. **`magnitude_history` (#18) trajectory contracts — rely on these.**
    - **Fresh eval per sample, NOT `CoalitionEvaluator`**: consecutive
      trajectory samples differ in member set by construction, so the
      evaluator's `(required, member_masks)` base key misses every sample and
      each rebuild costs ≈10–15× one fresh eval (gotcha 15's measured
      number). Don't "optimize" it back in; revisit only for a sweep-shaped
      variant (many candidates against one fixed base).
    - **Clears divergence is deliberate**: `HyperedgesCleared`/`GraphCleared`
      dissolve the trajectory (`members → None`, terminal `0.0` point) even
      though `TemporalQueries::hyperedge_vertices_at` ignores them — the
      point-in-time query structurally cannot see clear events
      (`hyperedge_index()` returns `None` for both), while the trajectory
      walks the full unfiltered log. Commented at both sites; don't
      "reconcile" by breaking either.
    - **Change-driven sampling semantics**: baseline point at resolved window
      start iff live; change points for `start < ts <= end`;
      `HyperedgeReversed` folds membership but never samples (order-only);
      multi-event timestamps settle before sampling; members with unresolved
      weights are skipped and uncounted; `member_count` is pre-dedup /
      pre-relevance (so clone joins show count↑ magnitude-flat =
      skeletalization); upstream `CatgraphError` ⇒ warn + `NEG_INFINITY`
      point, never a panic.

17. **P7.1 persistence contracts (#29) — rely on these.**
    - **Hash contract**: `RecordHash` = SHA-256 over the exact stored frame
      bytes EXCLUDING the u32 LE length prefix; `prev_hash` lives inside the
      NEXT frame. Verification re-reads disk bytes — it must NEVER re-encode
      a decoded frame. On-disk algorithm change = `FRAME_VERSION` bump (the
      in-memory `RecordHash.algorithm` tag is not on the wire).
    - **Wedge-on-write-failure**: any write-path error inside `append` wedges
      the stream — further appends return `StreamWedged` until the store is
      REOPENED (open re-scans structure and truncates a torn tail; full-chain
      check is an explicit `verify()`). Pre-write errors (encode, rotation
      dir-creation) do NOT wedge. Don't "fix" the writer task to retry into a
      wedged stream.
    - **Torn-tail truncation happens ONLY at the last segment's tail**; a
      short/invalid frame anywhere else is a hard `Decode` error — never
      silent truncation mid-log.
    - **The writer seam is at-most-once from the tee onward** (producer
      try_send may drop per the tap contract; a failed append is warned and
      dropped). K3's durable-bus at-least-once came from CHANGEFEED cursor
      replay, which has NO P7.1 analogue — do not transcribe that claim.
    - `Payload::Sealed` is schema-only until P7.3 (store round-trips it
      opaquely); `Lineage` is reserved until #20 unholds; open slurps whole
      segments + keeps 8 B/record offsets — fine at P7.1 scale.

18. **P7.2 tap/replay contracts (#30) — rely on these.**
    - **Tap fires UNDER the events write guard** (all 13 `record_event`
      sites + the SnapshotMarker site): tap order is always identical to
      log order, even with concurrent mutators sharing a cloned graph —
      the property the replay "same events, same order" guarantee and the
      parity gate rest on. Don't "optimize" the tap out of the guard.
    - **Install the tap BEFORE the first mutation** if downstream needs the
      full history — pre-tap events are never mirrored.
    - **Shutdown discipline**: lossless = drop the tap sender → forwarder
      drains → writer drains → `tracker.wait()`. Cancelling a shared token
      is PROMPT teardown: both tasks stop and an in-flight record can be
      dropped after acceptance (still at-most-once). Pick one; don't mix.
    - **Replay requires a quiescent pipeline** — replaying while a writer
      is appending silently yields a prefix, not an error.
    - Wire conversions are inherent `from_event`/`try_into_event` (NOT
      `From`/`TryFrom` impls — deliberate, §4 note); `schema_version >
      WIRE_TOPOLOGY_SCHEMA_VERSION` and `Sealed` payloads on the Topology
      stream are replay errors.

19. **Feedback-calculator contracts (#41) — rely on these.**
    - **Seed a given `FeedbackStore` at most once, before recording begins.**
      `seed_feedback_history` recomputes the FULL episode count from the event
      log and *adds* it (`add_history` accumulates, never replaces), so
      re-seeding the same store from the same log doubles every agent's
      history. On a mid-slice `get_agent` error, earlier agents remain seeded —
      reseed into a fresh store.
    - **Ids count per occurrence.** `record_outcome` bumps a duplicated member
      id twice; hyperedge member lists are ordered `Vec`s with duplicates
      allowed (gotcha 12), so dedup before recording if you want
      at-most-once-per-agent semantics. (Consistent with the base calculators,
      which also count duplicate agents per occurrence.)
    - `Clone` SHARES the store (Arc) — a `FeedbackCalculator` clone does not
      fork its feedback history. Non-finite outcomes are ignored + warned (the
      store can't be NaN-poisoned); non-finite *weights* propagate to the score
      where `ThresholdPolicy`'s non-finite guard declines the action.

29. **Remote-gateway contracts (#38, v0.25.0) — rely on these.**
    - **Quietening mDNS does NOT disable it.** The v0.10.0 gateway's
      "disabled" mDNS (24 h `query_interval`) still received other peers'
      inbound announcements — a live `mdns::Behaviour` discovers passively
      regardless of its own query cadence (measured: two in-process swarms
      found each other over three LAN interfaces). `enable_mdns: false` now
      wraps the behaviour in `Toggle::from(None)`, which is genuinely off.
      Never reintroduce the interval trick.
    - **Delivery is at-most-once with TWO lossy hops**: `emit_decision`
      try_send → `spawn_decision_tee` try_send → gateway buffer. Both hops
      drop-with-warn; drops correlate in time (same burst). Size both
      channels for peak burst. The tee needs no per-sink `catch_unwind`
      (sinks are plain `mpsc::Sender`s — no caller code runs inside it),
      unlike `spawn_outcome_forwarder`'s trait-object sinks.
    - **The `buffer_cap` clamp guards unbounded growth, not empty polls.**
      `push_record` evicts only when `len == cap`; unclamped `cap = 0`
      evicts only while empty and then grows without bound. `new(0)` ⇒
      `new(1)`; don't "relax" the clamp thinking cap 0 just means empty
      replies.
    - **No `Clear` request, on purpose** (the v0.10.0 gateway had one):
      destructive under multiple consumers. Eviction belongs solely to the
      cap; consumers track their own `last_seq` cursor and detect gaps
      (first returned `seq > last_seq + 1` ⇒ evicted events).
    - **`RemoteCoalitionClient::poll_since` refuses events with
      `schema_version > REMOTE_WIRE_SCHEMA_VERSION`** (the P7.2
      replay-error discipline). A schema bump is a wire-contract change —
      old clients must error, not guess.
    - **Token convention**: `enable_remote_gateway` uses the passed
      `CancellationToken` DIRECTLY (same as `spawn_decision_tee` /
      `spawn_outcome_forwarder`); callers wanting isolated gateway
      cancellation pass their own `child_token()`.
    - A future topology-event gateway is a SECOND `request_response`
      behaviour on the same swarm (`/koalisi/topology-events/1`, own
      schema version), NOT new `EventRequest` variants.

34. **Drift-check protocol contracts (v0.31.0) — rely on these.**
    - **Frozen-battery runs must be SERIAL.** "Latency is excluded from the
      comparison" is true of latency as a *reported metric* and FALSE as a
      guarantee that load cannot change the output: **Path A is a latency
      criterion**, so timing noise propagates straight into a `VERDICT` line.
      Measured at the v0.9.0 re-pin: batteries run concurrently with cargo
      test suites produced a pre-bump run scoring Path A **PASS** — which
      contradicts the report of record — and a 174-line diff showing a
      spurious `VALIDATED (A+B)` → `VALIDATED (B)` "drift". Re-run serially
      on a quiet machine, both sides reproduced the documented
      `FALSIFIED (latency)` / `VALIDATED (B)` with zero non-latency diffs.
      Check `pgrep -c 'cargo|rustc'` before starting, and never run the two
      sides of a comparison under different load.
    - **Diff the battery with the latency COLUMN stripped, not by grepping
      for the word "latency".** Table rows carry latency as an unlabelled
      final column, so a keyword filter leaves ~100 rows looking like real
      diffs. Strip the trailing `| <float> |` from both sides and diff again;
      an empty result is the actual X-battery PASS.
    - **MSRV re-measurement procedure.** The declared 1.93 makes cargo
      refuse any lower toolchain before it evaluates a dependency, so:
      temporarily set `rust-version` to 1.85.0, run `cargo +<v> check
      --all-targets --locked --features <set>` **per feature set** (every
      feature, not the widest set — `durable` sits in no other tier's
      superset), restore 1.93.0, diff the manifest. **Never
      `--ignore-rust-version`**: it suppresses the dependency checks too,
      so it cannot see a dependency floor.
    - **The tiers are in `Cargo.toml`** (1.88 / 1.89 / 1.92, re-measured at
      every re-pin). **`rust-version = "1.93.0"` is DECLARED above them —
      owner decision C-D1 (2026-09-14): KEEP.** Do not change it in
      passing. Ground: edition 2024 means resolver 3, so the declared value
      bounds dependency resolution (`cargo update` holds packages to
      declared-MSRV-compatible versions; the opt-out is `[resolver]
      incompatible-rust-versions = "allow"`). Cost: a 1.88–1.92 downstream
      is refused a default-only build that would compile.
    - **Four wrong MSRV claims were retired between v0.17.0 and v0.31.0**,
      every one from measuring a chosen subset and generalising (the
      ledger has them). Measure every feature; cite the command.
    - **`cargo test … | rg '^test result' | tail` truncates.** Bare `tail` is
      `tail -10`; suites with 11+ result lines (`persistence,magnitude`) lose
      the lib-test line and undercount by ~95. Always `tail -20`.

### A/B-lineage gotchas 20–28, 30–33 — index only

Full text: the **`ab-lineage-gotchas.md`** archive (held outside this repo; see
`CLAUDE.local.md`). Each entry records a
mechanism that already cost a run — read the relevant one in full before
touching the arm it governs.

20. **Feedback-arm K4 battery (#46)** — balanced `hw=fw` weights CANCEL in the
    full-join regime (`history ≈ failures`); feedback can only bite via a
    dominant failure term on a *selective* base. Plus the Part 3 harness
    contract (store fresh per seed, write-back after the leave sweep).
21. **Population search (#42)** — the built-in calculators are DEGENERATE for
    *structure* search (Additive is constant across partitions; Synergistic /
    Multiplicative favour all-singletons), so `search` needs an
    interior-optimum value model; `search` is a pure function of the seed;
    `record_trajectory` is fresh-manager-only. *(Also a live library contract
    for `src/algorithms/population.rs`.)*
22. **Selective-base feedback (#48)** — a positive `join_threshold` makes
    feedback bite but does not beat magnitude, and the increment is
    NON-MONOTONE in the threshold (pure selectivity overtakes at 125/150).
23. **E1 outcome-signal fidelity (#54 Part 4e/4f/4g)** — the per-bit oracle
    signal is NOT load-bearing (degraded ≈ oracle); cheap reliability-gating on
    mag is WORSE than bare mag (absorbing exclusion); score-space margins and
    hysteresis on the e1 arm are INERT (γ=16 posteriors saturate at ±0.5).
24. **`ReliabilityCoverage` (#57)** — the belief read is RECENCY-dominated, not
    an aggregate (ordering is the robust signal, never present it as a success
    rate); reliability RESCALES the coverage optimum and does NOT route around
    weak bits; the 0.5 unknown-bit prior is OPTIMISTIC.
25. **Battery-v2 (#61 + the v0.19.0 Part 5c addendum)** — the e1 join rail is
    margin-proof (join quantiles exactly +0.5 in every measured cell, 8 bits and
    12); never write a "skips a bit" predicate over `search()` output; random
    pools do not cover big requirements; `query_gamma` is inert under
    `query_dynamics: true`.
26. **Corrected routing test (#63, Part 6)** — block-level routing needs
    window > lattice; per-block multiplicative full bonuses are gotcha-21
    degenerate (the FOURTH mechanism); uniform-rescale counterfactuals do NOT
    hold the partition fixed.
27. **EQ3 levers (#69)** — the default mag arm is L1-only and bit-frozen; L2 is
    DELIBERATELY decision-changing (never enable it expecting a pure speedup);
    L3's fast route needs exactly-symmetric ζ; the cg#153 empty band is
    CONFIRMED on koalisi traffic.
28. **Typed arm (#72)** — `with_role_modulation` is opt-in with a structural
    identity default and evaluates FRESH both sides; ρ-modulation is diversity
    accounting, NOT coverage routing; `with_eq3_levers` / `with_evaluator_leave`
    are silently INERT under a typed config.
30. **EQ5a process (#76)** — multiplicity prices a process, it does not add
    coverage demand (an idempotence-only theory is structurally inert); a fusion
    target inside the consumed pair rigs the leg; a cost term constant in the
    coalition CANCELS; soundness is theory-relative; a decline is
    indistinguishable from a zero margin at the `Decision` seam.
31. **Residual lever (#80)** — a term can be LIVE in the score and dead at the
    decision (355 score bits, 0 acts); always report act-vs-score-bit divergence
    alongside any score-space lever; design the discrimination so the controls
    are bit-identical; `ResidualPolicy` is shipped, NOT adopted.
32. **`GroupAgent` deterministic read (`aif-v0.13.0`, tira#53)** — a read
    without a commit is a FIXED POINT under MeanField but DOUBLE-COUNTS under
    MMP, which is what koalisi configures; read → decide →
    `record_group_action`, NEVER mixed with the recording read;
    `CertaintyWeighted` is the only mode with a usable gradient at R = 3; the
    read is RNG-free but not side-effect-free.
33. **EQ5b group arm (#78)** — under role-matched coverage masks a group CANNOT
    deliberate about a candidate; an indifferent internal does not abstain, it
    votes ACT at MAXIMUM confidence; those blind voters are load-bearing for
    performance (removing them collapses the arm 8.5×); role specialisation
    measured NEGATIVE; a non-vacuity gate must exempt `expected == 0`.

## Reproducers

All assume `cwd = koalisi/`.

```sh
# === default features (106 tests) ===
timeout 60s  cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target
timeout 30s  cargo run  --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --example topology_coalition
timeout 30s  cargo run  --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --example algorithm_values
timeout 30s  cargo run  --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --example synthetic_ingestion   # FLAGSHIP
timeout 30s  cargo run  --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --example supervised_monitor
timeout 30s  cargo run  --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --example population_search   # P5.2 (#42)

# === decision-layer feature combos (162 / 135 / 191 tests) ===
timeout 120s cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features decision
timeout 120s cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features magnitude
timeout 120s cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features decision,magnitude
# strategy_comparison needs ALL THREE features — see the process block below.
timeout 60s  cargo run --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features decision --example population_reliability   # #57 (v0.16.0)

# === with persistence feature (P7.1 store + P7.2 replay, 126 tests) ===
timeout 120s cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features persistence
timeout 120s cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features persistence,magnitude   # 156, incl. the replay parity gate

# === with remote feature (#38 gateway, 112 tests) ===
timeout 120s cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features remote
timeout 60s  cargo run  --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features remote --example remote_coalition_consumer

# === with process feature (EQ5a #76 + #80 ResidualPolicy, 159 tests) ===
timeout 120s cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features process
timeout 300s cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features decision,magnitude,process   # 239
# NOTE: strategy_comparison is the FROZEN K4 archive; the ~21 min battery runs
# SERIAL on a quiet machine with NO timeout wrapper — the archive + drift-check
# recipe is docs/runs/README.md.
cargo run --release --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features decision,magnitude,process --example strategy_comparison

# === with harness feature (K7 skeleton v0.33.0; workflow world + lifecycle hook v0.35.0) ===
timeout 120s cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features harness
timeout 300s cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features harness,process   # 198, incl. the H0 identity gate
timeout 60s  cargo run  --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features harness --example gauntlet

# === with metrics feature (#25, v0.36.0; 106 tests = the default suite) ===
timeout 120s cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features metrics
timeout 60s  cargo run  --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features metrics --example metrics_scrape   # KOALISI_METRICS_ADDR overrides the loopback listener address

# === with durable feature (needs Docker; container-backed restart test) ===
timeout 300s cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features durable
timeout 120s cargo run  --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features durable --example durable_decisions
```

## Next steps

The 2026-07-03 design gate is RESOLVED (both inputs received). Phase narratives
— Phase 5 SwarmAgentic, Phase 6 decision layer + K1–K6, Phase 7 persistence —
are in the `project-history.md` archive §2, along with the gate record itself.
What is still open:

- **Phase 7 implementation** — [#31](https://github.com/sustia-llc/koalisi/issues/31)
  P7.3 sealing + revocation registry is next, **blocked on the tauhokohoko
  KEK-granularity answer for belief sealing**; then
  [#32](https://github.com/sustia-llc/koalisi/issues/32) P7.4 decision/belief
  streams, then [#33](https://github.com/sustia-llc/koalisi/issues/33) P7.5
  federation manifests + FAIR provenance. Design of record:
  `.claude/docs/phase7-persistence-design.md`; open calls in its §17 (SHA-256 vs
  BLAKE3; ciphertext reclamation; cross-federation `EventRef` addressing —
  resolve at #33).
- **Phase 5 remainder** — [#20](https://github.com/sustia-llc/koalisi/issues/20)
  keeps the LLM-dependent meta-layer (configurator + velocity-rewrite loop +
  transferability). Both LLM-free slices shipped (#41 v0.12.0, #42 v0.13.0). The
  only still-NEST-dependent piece is the NEST-H4 calibration-copilot deployment
  framing, which activates whenever ownership lands.
- **K7 lineage** — plan `.claude/stack/2026-09-16-koalisi-tira-programme-sweep.md`
  (ratified 2026-09-16; supersedes §3–§5 of the K7 round plan). Landed: the
  harness (v0.33.0), the K0 board corrections, tira's `aif-v0.14.0`, the H
  scaffold (v0.35.0, #92), M — the #25 metrics example + a lock refresh
  (v0.36.0). Next in koalisi: C1 — the `aif-v0.14.0` re-pin PR (v0.37.0, pin-first,
  one serial archive run diffed against `docs/runs/K4-archive.log`); then
  C2 — the `K7-1` registration (v0.38.0: prereg `docs/k7/prereg-K7-1-<slug>.md`
  before code, `examples/k7/k7_1.rs`, seeds 90..120, in-battery controls
  `grp-role` / `grp-role-blind` on the H0 workflow world with Part 11 outcome
  semantics) per `docs/PROTOCOL.md`; then `K7-2` (seeds 150..180, released)
  and `K7-3` (360..390) on the same pin.
- **MSRV — C-D1 DECIDED (owner, 2026-09-14): KEEP `rust-version = 1.93.0`.**
  The v0.23.0 re-pin removed the last DeepCausality edge and the `process`
  tier with it; the measured cross-feature maximum is 1.92 and the
  declaration stays above it on the resolver-3 ground. Tiers recorded in
  `Cargo.toml` and gotcha 34. Nothing is owed; **do not re-file.**

Downstream projects (nautilus_trader bridge, tauhokohoko integration) and
removed work (databento → `biome`, the forex-coupled backlog) are recorded in
the `project-history.md` archive §3.

## Open questions (jot anything here as it comes up)

> The former forex open questions (coordinator hysteresis per-direction; the
> databento feature split) are moot — the forex swarm (v0.11.0, #37) and the
> databento adapter (v0.10.0) are both gone. Nothing open here right now.
