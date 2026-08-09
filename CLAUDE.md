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

**Review protocol (standing, owner 2026-08-05): EVERY PR gets the
reviewer pass before tag/merge — re-pin and deps+docs slices included.**
Every finding is applied or owner-adjudicated, including ones below the
review skill's confidence bar. The former carve-out for re-pins ("the
re-pin protocol's own gates are the review", PR #62 precedent) is
REVOKED: the first re-pin reviewed under it (PR #77) produced three real
findings, one of them a miss of the checklist directly above — and
suites, clippy and a byte-identical battery structurally cannot detect
stale docs, which is the failure mode a deps-only PR carries most often.
Reviewer output is evidence, not verdict: check each claim against the
diff (a PR #77 panel called the lockfile delta clean when it was not).

**Maintenance rule (2026-08-09)**: new release entries go in FULL to the
`project-history.md` archive (§1); this file keeps only the latest three plus a
verdict-trail row. New gotchas: an engineering contract goes here in full, an
A/B-lineage one goes to the `ab-lineage-gotchas.md` archive with a one-line
index entry here. **Keep this file under 150k chars** — past that the harness
truncates it and the tail silently stops reaching the session.

## Where the rest of the record lives

This file is the working state. Everything historical or registration-specific
was moved out on 2026-08-09, when CLAUDE.md had crossed the 150k-char harness
limit (163.2k) and its tail was being silently truncated. Nothing was deleted.
The two archives are held **outside this repository** (owner call, 2026-08-09);
their location is recorded in `CLAUDE.local.md`:

- **`project-history.md`** — the full release ledger v0.4.0 → v0.31.0 verbatim,
  the Phase 5/6/7 narratives, the K1–K6 sections, the downstream/removed-work
  notes, and the obsolete gotchas 1–6 / 8–10.
- **`ab-lineage-gotchas.md`** — gotchas 20–28 and 30–33 verbatim (the
  A/B-registration lineage). Indexed one line each at the end of §Worth
  flagging below; **read the full text there before designing or running any
  K4-lineage registration.**
- **`docs/`** (in-repo, public) — the A/B showcase trail: one prereg + one
  report per registration. Registered docs are IMMUTABLE, and they carry the
  public record of every verdict summarised in the trail table below.
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
Four-layer architecture: Core (CoalitionRuntime, lifecycle), Topology
(temporal hypergraph via `catgraph_applied::Hypergraph` since K1 — was
yamafaktory hypergraph v4.2.0 — event sourcing, CoalitionManager,
time-travel queries, analytics), Algorithms (DCVC workload distribution,
AIPA partition search, pluggable value calculators), and Runtime (since K3:
tokio tasks with mpsc/oneshot command handles — kameo is gone — the
`CoalitionService` policy-gated membership seam, a thin task-restart layer,
an optional SurrealDB-backed durable decision log, and — since v0.25.0,
#38 — an optional libp2p remote coalition-event gateway). koalisi began as a
forex triangular-arbitrage tool; that domain was removed in v0.11.0 (#37) —
the architecture is domain-agnostic and the demonstrated runtime is now a
synthetic coalition-formation pipeline. Market/trading work lives in the
sibling `biome` project.
Evolved from four prior projects: dynamo (topology), coalesce (algorithms),
coalition_aif (decision — planned), and forex-arbitrage-swarm (runtime — the
forex domain since removed).

## Available tooling for this project

- **`causality` (DeepCausality) plugin** — **scope SHRANK sharply at the
  v0.9.0 re-pin (v0.31.0); read this before routing.** koalisi's topology
  backend is `catgraph_applied::Hypergraph` (plain `Vec`/`HashMap` container —
  read its module docs at the pinned catgraph tag for the contract). catgraph
  no longer sits on the DeepCausality substrate for any of it: cg#219/#221 gave
  it its own `Zero`/`One` + `Dual` (retiring `deep_causality_num`) and cg#220
  its own toposort + connected components (retiring `ultragraph`). **There is
  no `ultragraph` package in koalisi's lock at all**, so
  `causality:causal-graphs` describes a crate that is not in the dependency
  graph — do NOT route graph-algorithm questions there; the implementations
  are catgraph's own. What remains is a single optional edge:
  `catgraph-syntax` → `deep_causality_haft` → `algebra` → `num`, present only
  under feature `process`. `causality:causality-theory` is still apt for the
  algebraic layer (`Rig`, HKT/witnesses) catgraph's enrichment sits on.
- ~~`graph` plugin v2.0.1~~ (yamafaktory hypergraph skills) — **OBSOLETE for
  `src/topology/` since K1**; historical reference only (pre-K1 semantics, the
  dropped `PersistentHypergraph` idea — see the Phase 7 narrative in the
  `project-history.md` archive §2).
- `rust-v2:rust-dev-v2` / `rust-v2:rust-practical` — primary Rust agents per
  the user CLAUDE.md routing rules.
- `surrealdb:surrealdb-rust-v3` / `surrealdb:surrealdb-search` /
  `surrealdb:surrealql-language` — for K3 (#6) surrealdb-live-message work,
  per the user CLAUDE.md routing rules.

## Current state — 2026-08-09 (v0.31.0)

Full release ledger v0.4.0 → v0.31.0 is the `project-history.md` archive (§1).
The three most recent entries are kept here in brief — read the ledger before
touching anything with a frozen battery, a pinned decision, or a registered doc.

### Latest three

- **catgraph re-pin `v0.8.0` ×3 → `v0.9.0` — v0.31.0 (2026-08-09)**: all three
  catgraph deps in lockstep (K6 rule). Upstream v0.9.0 is dependency
  streamlining — cg#219/#221 give catgraph its own `Zero`/`One` + `Dual`,
  cg#220 its own toposort + components. **Drift check CLEAN**: all ten suites
  at baseline counts (106/162/135/191/143/126/156/112/159/239, measured BEFORE
  the bump too — all ten matched the table, so no documentation drift hides in
  the comparison; plus `durable` 107), default clippy `--all-targets` clean
  from a fresh target dir, and **X-battery PASS with zero non-latency diffs**
  (both runs 2129 lines; of 122 differing lines, 102 are table rows whose only
  changed field is the final latency column and 20 are prose lines reporting
  latency — strip that column and the diff is empty).
  ⚠ **MSRV: re-tested; declared floor stays 1.93 but it is
  FEATURE-CONDITIONAL, and the note carried since v0.17.0 was wrong.** The
  DeepCausality chain enters through exactly ONE edge — optional
  `catgraph-syntax` → `haft` → `algebra` → `num` — so it is present **only
  under `process`**. `cargo tree -i deep_causality_haft` finds nothing at
  default features or at `decision,magnitude`, and `cargo +1.92 check
  --all-targets --ignore-rust-version` **succeeds** at default features and at
  `decision,magnitude,persistence,remote` (1.91 too; 1.85/1.86 fail on
  let-chains). Only with `process` on do `algebra` / `haft` / `num` each demand
  1.93. So the old note fails twice over: `num` is not removed by v0.9.0 (it
  survives beneath `haft → algebra`) and was never the sole cause — but the
  floor is **koalisi's own**, imposed by its optional feature, not the
  substrate's. `rust-version = "1.93.0"` declares the max across features
  (cargo has no per-feature MSRV) and therefore refuses 1.91/1.92 downstreams
  whose graph has zero DeepCausality crates; **whether to lower it is an OPEN
  owner decision**, recorded in `Cargo.toml`. Obligation CLOSED as corrected.
  (This entry's own first draft claimed "no catgraph re-pin can lift this
  floor" — the reviewer disproved it; see **gotcha 34**, which now warns
  against a third single-measurement claim.)
  **Lockfile**: FOUR catgraph packages move (core `catgraph` rides along as a
  transitive), `ultragraph 0.9.2` leaves entirely, `union-find 0.4.4` is newly
  referenced but was already present (no package added), `deep_causality_num`
  vanishes from two dependency arrays while its stanza stays; net −1. **Third
  time a `name`/`version` grep would have missed the real story** (v0.26.0,
  v0.29.0). Breaking rider (cg#219/#221: `Rig` via `deep_causality_num`'s
  `Zero`/`One` → `catgraph_applied::rig`) verified NOT applicable — koalisi
  names no `Rig` impl, no `rig::`, no `deep_causality`/`ultragraph` path.
  See **gotcha 34** for the battery-concurrency trap this run walked into.
- **EQ5b typed two-engine RUN — v0.30.0 (2026-08-08, #78): `VALIDATED
  (two-engine)`** — the second validated registration in the K4 lineage, after
  EQ4. **Read the mechanism before quoting the verdict**: role specialisation
  measured NEGATIVE (shared-model reference cells 0.2409 vs specialised 0.2270,
  specialised superior on only 15/30), and the arm depends on the structural
  defect the review found — 23.0 % of decisions have ZERO candidate-sensitive
  internals, and removing those blind voters (`grp-role-blind`) collapses the
  arm 0.2270 → **0.0268**. Official re-run seeds 330..360: `grp-role` 0.2270 =
  1.2567× / 22-of-30, `grp-mult` 0.2266 = 1.2544× / 22-of-30, control `wf-asis`
  0.1806; both cells clear conjunct 1 by 0.7 % / 0.4 % and **the margin is
  reported in full, not renegotiated**. Did NOT exceed `wf-val-p` (0.2435) ⇒
  pre-committed scoped claim "beats the typed control, not the strongest process
  cell"; DID exceed `arm-E1` (0.0403). Gates X-battery / X-identity /
  S-determinism / S-learn all PASS. **koa#54 stays FINAL** — no EQ5b outcome
  reopens it. Library `src/decision/group_policy.rs` (`GroupAifPolicy`, features
  `decision` + `process`). Report
  `docs/ab-report-K4-eq5b-typed-two-engine.md` (13-item ledger). See
  **gotcha 33**.
- **aif re-pin `aif-v0.12.0` → `aif-v0.13.0` — v0.29.0 (2026-08-08)**: the EQ5b
  pin-first step; **catgraph deliberately NOT moved** (stays `v0.8.0` ×3), so an
  aif-only hop. Drift check CLEAN — all ten suites at baseline counts (measured
  before the bump too), clippy `--all-targets` clean from a fresh target dir,
  frozen K4 battery reproduced with zero non-latency diffs. What it buys
  (tira#53, the EQ5b gate): `GroupAgent::group_distribution` — the group action
  distribution formed with no RNG draw and no `last_action` advance — plus
  `group_distribution_recording`, `record_group_action`, the defaulted
  `Aggregator` distribution twins, and `VotingAgent::weighted_mixture` made
  `pub`. Two breaking riders verified no-ops here (`AifError` is now
  `#[non_exhaustive]` + gains `Unsupported(String)`, used only in return
  position at 6 sites and never matched on; `communication` moved behind a
  default-off feature). **Lockfile delta is NOT a package count** —
  re-resolution also moved three unrelated transitive edges; a
  `name`/`version`/`source` grep structurally cannot see dependency-array edges,
  so read the whole diff. ~~⚠ MSRV re-test still owed at the next *catgraph*
  re-pin — the 1.93 floor is `deep_causality_num =0.4.1` propagating through
  catgraph, and v0.9.0 removes it.~~ **RETIRED as WRONG at v0.31.0** — `num`
  is not removed (it survives beneath `haft → algebra`) and was never the sole
  cause; the floor is in fact koalisi's own, gated behind the optional
  `process` feature. Do NOT act on the struck sentence; see the v0.31.0 entry
  and **gotcha 34**. See **gotcha 32**.

### K4 A/B lineage — verdict trail

Most rows are pre-registered runs whose prereg + report pair lives in `docs/`
and is IMMUTABLE. Four are not, and `docs/` will not yield a prereg for them:
**v1/v2** registered on issue #7 (report only), **K1** and **K6** are a
backend-parity and an optimization re-run rather than registrations (report
only), and **#54** is a decision memo + design note. Full entries in the
history ledger.

| # | Arm / question | Seeds | Verdict | Gotcha |
|---|---|---|---|---|
| v1 / v2 | magnitude vs scalar AIF (#7) | 0..30 | `FALSIFIED (latency)` / `VALIDATED (B)` | — |
| K1 | catgraph backend parity (#4) | 0..30 | byte-identical re-run | 12 |
| K6 | evaluator hot path (#14) | 0..30 | Path A missed; dual verdict unchanged | 15 |
| v3 | multimodal AIF (#43) | 0..30 | `FALSIFIED (multimodality)` — decision-equivalent to scalar | — |
| v4 | persistent AIF (#44) | 0..30 | `FALSIFIED (persistence)` — escapes v3's theorem, loses on performance | — |
| v5 | E1-only persistent (#53) | 30..60 | **`VALIDATED (gap closed)`** 1.62× | — |
| #46 | feedback arm | 0..30 | `FALSIFIED (feedback)` | 20 |
| #48 | selective-base feedback | 0..30 | `PARTIAL (mechanism only)` | 22 |
| #54 | arm-choice memo, Steps 1–4 | 30..60 | **DECIDED B+D — CLOSED, FINAL** | 23 |
| v6 | never-evict (#56) | 60..90 | `FALSIFIED (never-evict)` — churn IS the e1 mechanism | — |
| EQ1 | battery v2 de-saturation (#61) | 120..150 | `FALSIFIED (de-saturation)`; lever 1 `RUN-INVALID` → #63 | 25 |
| #63 | corrected block-level routing | 180..210 | `FALSIFIED (block-routing)` | 26 |
| EQ3 | latency re-match (#69) | 210..240 | `FALSIFIED (latency re-match)` | 27 |
| EQ4 | typed roles (#72) | 240..270 | **`VALIDATED (typed roles)`** 3.61×, 30/30 | 28 |
| EQ5a | process-structured (#76) | 270..300 | `FALSIFIED (process structure)` | 30 |
| #80 | residual process-specificity | 300..330 | `FALSIFIED (coverage proxy)` | 31 |
| EQ5b | typed two-engine (#78) | 330..360 | **`VALIDATED (two-engine)`** 1.2567×, 22/30 | 33 |

**Seed ledger** — consumed: 0..90, 120..150, 180..360. **Reserved-unconsumed:
90..120 and 150..180.**

**Standing run protocol** (every registration): owner design-lock posted on the
issue BEFORE prereg → prereg doc committed BEFORE implementation → 3-lens review
BEFORE the official run → amendments are pre-verdict only → registered docs are
immutable (results append, sections are never edited). Pin-first: dependency
re-pins land in their own PR so the registration is born on the final pins.
Rust implementation is dispatched to `rust-v2:rust-dev-v2`.

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
| `--features process` | 159 (EQ5a surface + #80 ResidualPolicy) | `cargo test --features process` |
| `--features decision,magnitude,process` | 239 (the Part 9 + Part 10 + Part 11 batteries) | `cargo test --features decision,magnitude,process` |
| `--features durable` | +1 container-backed restart test; needs Docker | `cargo test --features durable` |
| All examples | exit 0 | see Reproducers below |

(All non-remote suites are the pre-v0.25.0 baselines +3 — the always-compiled
`spawn_decision_tee` unit tests.)

### File inventory

```
koalisi/
├── Cargo.toml                              git tag deps: catgraph-applied + catgraph-magnitude + catgraph-syntax v0.9.0 in lockstep (one checkout — K6); aif-v0.13.0, surrealdb-live-message, libp2p 0.56 (optional); no path deps since K3; MSRV 1.93 (set by the DeepCausality substrate, NOT liftable by a catgraph re-pin — gotcha 34)
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
│   ├── decision/
│   │   ├── mod.rs                          CoalitionDecisionPolicy + ThresholdPolicy (always compiled)
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
│   ├── strategy_comparison.rs              divergence demo + K4 A/B battery (requires ALL THREE: decision,magnitude,process — since EQ5a Part 9)
│   ├── remote_coalition_consumer.rs        #38 (v0.25.0): gateway + client in one process over a live CoalitionService (feature `remote`)
│   └── durable_decisions.rs                durable decision log end-to-end (feature `durable`)
├── .claude/docs/                           TRACKED internal design docs + references (docs/ reorg 2026-07-27; rest of .claude/ stays gitignored)
│   ├── phase7-persistence-design.md        Phase 7 EventStore design (#21 deliverable; P7.1–P7.5 phasing)
│   ├── k3-hot-path-bench.md                K3 kameo-vs-tokio bench evidence
│   ├── SwarmAgentic-summary.md             Phase 5 paper digest (Zhang et al. 2025)
│   └── 2506.15672v1.{md,pdf} + _images/    the SwarmAgentic paper itself (CC0 per its PDF metadata)
├── docs/                                   PUBLIC A/B showcase trail — pre-registrations, reports, evidence (registered docs are immutable)
│   ├── ab-report-K4-{yamafaktory,catgraph}.md   K4 A/B + backend-parity reports
│   ├── ab-report-K4-catgraph-evaluator.md  K6 post-optimization re-run + parity + latency profile (#33 evidence)
│   ├── baseline-aif-scalar-scope-b.md      frozen scalar Scope-B baseline (v4/v5 prereg anchor)
│   ├── prereg-feedback-arm-k4.md           #46 pre-registration (feedback-arm K4 rematch; result appended)
│   ├── ab-report-feedback-arm-k4.md        #46 run — FALSIFIED (feedback); Scope A null + Scope B reliability contest + E1 sweep
│   ├── prereg-feedback-arm-k4-v2.md        #48 pre-registration (selective-base rematch, join=100, hw=0/fw=1; result appended)
│   ├── ab-report-feedback-arm-k4-v2.md     #48 run — PARTIAL (mechanism only); selectivity vs reliability-gating decomposition + E1 threshold sweep
│   ├── per-bit-outcome-plumbing-design.md  #54 Step 2 design note — outcome-signal fidelity ladder; degraded ≈ oracle result (gotcha 23)
│   ├── k4-arm-choice-memo.md               #54 Step 4 decision memo — DECIDED B+D 2026-07-18; postscript: #56 FALSIFIED ⇒ B's park final
│   ├── prereg-K4-v6-never-evict.md         #56 pre-registration (never-evict, dual-signal, 60..90; result appended)
│   ├── ab-report-K4-v6-never-evict.md      #56 run — FALSIFIED (never-evict); cap-series monotonicity = churn is the mechanism
│   ├── prereg-K4-battery-v2.md             #61 EQ1 pre-registration (de-saturated regime; Part 5c scope DONE v0.19.0)
│   ├── ab-report-K4-battery-v2.md          #61 run — lever 2 FALSIFIED (de-saturation), lever 1 RUN-INVALID (sanity leg → #63); v2-regime context inversion; + Part 5c addendum (v0.19.0)
│   ├── prereg-K4-routing-corrected.md      #63 pre-registration (block-level routing; legs A/C/L; + pre-impl tie-break amendment)
│   ├── ab-report-K4-routing-corrected.md   #63 run — FALSIFIED (block-routing), mechanism-scoped (window vs lattice); leg C DEGENERATE (4th gotcha-21 mechanism); leg L ordering 30/30 (gotcha 26)
│   ├── prereg-K4-eq3-latency-rematch.md    #69 EQ3 pre-registration (+ pre-run Amendment 1: L2 behind toggle, H-par′, skeletal route; result appended)
│   ├── ab-report-K4-eq3-latency-rematch.md #69 run — FALSIFIED (latency re-match); empty-band CONFIRMED; L3 ζ-asymmetry ceiling; 10-item ledger (gotcha 27)
│   ├── prereg-K4-eq4-typed-roles.md        #72 EQ4 pre-registration (+ pre-run Amendments 1–2: typed Scope-A, E-ρq-inv cell, E-T3 counters; result appended)
│   ├── ab-report-K4-eq4-typed-roles.md     #72 run — VALIDATED (typed roles), first since v5; 43.5% conversion; E-ρq anti-alignment + inverse cell; 15-item ledger (gotcha 28)
│   ├── prereg-K4-eq5a-process-structured.md #76 EQ5a pre-registration (+ FIVE pre-run amendments: pinned constants, erratum, two inert-leg fixes, the 3-lens review)
│   ├── ab-report-K4-eq5a-process-structured.md #76 run — FALSIFIED (process structure); valuation converts where rewriting does not; 100% of a low ceiling; 15-item ledger (gotcha 30)
│   ├── prereg-K4-residual-process-specificity.md #80 pre-registration (+ pre-run Amendment 1: probe declines gated, floor-condition disclosure)
│   ├── ab-report-K4-residual-process-specificity.md #80 run — FALSIFIED (coverage proxy); the lever replicates 1.34x but lift_wf == lift_flat; 355 score bits, 0 acts; 8-item ledger (gotcha 31)
│   ├── prereg-K4-eq5b-typed-two-engine.md  #78 EQ5b pre-registration (+ SIX pre-verdict amendments: unbuildable arm, coverage masks, CW rationale false, the A3.1 erratum, the 3-lens review, the unsatisfiable non-vacuity guard)
│   └── ab-report-K4-eq5b-typed-two-engine.md #78 run — VALIDATED (two-engine) 1.2567x / 22-of-30; role specialisation NEGATIVE and the candidate-blind voters load-bearing; 13-item ledger (gotcha 33)
└── tests/
    ├── topology_test.rs                    12 tests
    ├── algorithms_test.rs                  18 tests (incl. 3 feedback-loop/seeding tests, #41)
    ├── decision_integration.rs             4–6 tests (feature-dependent)
    ├── durable_integration.rs              1 container-backed restart test (feature `durable`)
    ├── ingestion_integration.rs            3 tests (K5: synthetic sources → monitors → coalition formation; default features)
    ├── magnitude_trajectory.rs             6 tests (#18: hand-computed trajectory semantics; feature `magnitude`)
    ├── persistence_integration.rs          7 tests (#29: roundtrip, rotation+reopen, tamper, torn tail, sealed opaque, writer drain, bounds; feature `persistence`)
    ├── topology_replay.rs                  3 tests (#30: 13-variant round-trip, reconstruction equality, schema/Sealed rejection; feature `persistence`)
    ├── remote_integration.rs               1 test (#38: loopback round-trip service → tee → gateway → client, cursor deltas + seq ordering; feature `remote`)
    └── replay_parity.rs                    1 test (#30: magnitude_history live == replayed — THE parity gate; features `persistence,magnitude`)
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

14. **`durable` feature gotchas (surrealdb-live-message v0.2.0).**
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
    - **The MSRV floor is FEATURE-CONDITIONAL — measure per feature set,
      never once.** The whole DeepCausality chain enters through exactly
      ONE edge: optional `catgraph-syntax` → `haft` → `algebra` → `num`,
      i.e. only under feature `process`. `cargo tree -i deep_causality_haft`
      finds **nothing** at default features or at `decision,magnitude`, and
      `cargo +1.92 check --all-targets --ignore-rust-version` **succeeds**
      at default features and at `decision,magnitude,persistence,remote`
      (1.91 too; 1.85/1.86 fail on let-chains). Only with `process` on do
      `algebra` 0.2.0 / `haft` 0.4.2 / `num` 0.4.1 each demand 1.93.
      `rust-version = "1.93.0"` declares the MAXIMUM across features because
      cargo has no per-feature MSRV — so it refuses 1.91/1.92 downstreams
      whose graph contains zero DeepCausality crates.
      **TWO successive wrong claims have been retired here; do not write a
      third from a single measurement.** (a) "the floor is `num` alone,
      liftable at the next catgraph re-pin" — false, `num` survives beneath
      `haft`. (b) "no catgraph re-pin can lift it, the DeepCausality
      substrate must move" — also false, the floor is koalisi's own, imposed
      by its optional feature; gating or dropping `catgraph-syntax` lifts the
      default-build floor today. Both errors came from measuring ONLY with
      `--features decision,magnitude,process` and generalising. Any MSRV
      claim must state the feature set it was measured under.
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
# NOTE: strategy_comparison now requires all THREE features (Part 9), and the
# battery run takes ~21 min — NO timeout wrapper on battery runs (run protocol).
cargo run --release --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features decision,magnitude,process --example strategy_comparison

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
- **K4 lineage** — the next registration follows the standing run protocol under
  §Current state. Seeds 90..120 and 150..180 stay reserved-unconsumed.
- **[#25] Metrics example** — still valid but needs reframing: instrument the
  `CoalitionService` decision path / topology events, not the deleted
  `tick_bus`/`alert_bus`.
- **MSRV re-test** — owed at the next *catgraph* re-pin; v0.9.0 removes
  `deep_causality_num =0.4.1`, the crate that forces the current 1.93 floor.

Downstream projects (nautilus_trader bridge, tauhokohoko integration) and
removed work (databento → `biome`, the forex-coupled backlog) are recorded in
the `project-history.md` archive §3.

## Open questions (jot anything here as it comes up)

> The former forex open questions (coordinator hysteresis per-direction; the
> databento feature split) are moot — the forex swarm (v0.11.0, #37) and the
> databento adapter (v0.10.0) are both gone. Nothing open here right now.
