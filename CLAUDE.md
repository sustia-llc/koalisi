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

**Rolling session log (owner, 2026-09-20 — E-D3):** the live
`session-log.md` holds the **current lineage only**; at a lineage close it
splits verbatim into `session-log-<range>.md` and the live file restarts
under its banner. `CHANGELOG.md` follows the same shape — sections before
`v0.30.0` are archived (see above), and the next split is a new archive file,
never an edit to an old one.

**One record per fact class, a pointer everywhere else** (owner, 2026-09-20 —
the refocus plan's §1.26). What landed → `CHANGELOG.md` + the release ledger;
current state and sequence → `plans/current-roadmap.md`; cross-repo seams →
the stack roadmap; a mechanism that cost a run → one gotcha; the session
narrative → `session-log.md`; measured corrections to a plan section → that
plan's execution record; a registered verdict, bar or number → the immutable
prereg/report under `docs/`. At close-session, `rg` two or three distinctive
phrases of what was just written across the other notes files — a hit means a
copy was written where a pointer belongs.

## Where the rest of the record lives

This file is the working state. Three archives are held **outside this
repository** (owner call, 2026-08-09; the third added 2026-09-20); their
location is in `CLAUDE.local.md`:

- **`project-history.md`** — the full release ledger v0.4.0 → present verbatim,
  the Phase 5/6/7 narratives, the K1–K6 sections, the downstream/removed-work
  notes, and the obsolete gotchas 1–6 / 8–10.
- **`ab-lineage-gotchas.md`** — gotchas 20–28, 30–33 and 35–37 verbatim (the
  A/B-registration lineage). Indexed one line each at the end of §Worth
  flagging below; **read the full text there before designing or running any
  K4-lineage registration.**
- **`changelog-archive-pre-v0.30.0.md`** — this repo's `CHANGELOG.md` sections
  v0.29.0 → v0.1.0 verbatim (moved 2026-09-20; the live file starts at
  `[0.30.0]` and carries the banner). Same directory as the other two.
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

## Current state — 2026-09-20 (v0.42.0; last code release 2026-09-19)

Full release ledger v0.4.0 → v0.42.0 is the `project-history.md` archive (§1).
The three most recent entries are kept here in brief — read the ledger before
touching anything with a frozen battery, a pinned decision, or a registered doc.

**2026-09-20 cut no release** (Phase N of the refocus plan, no code): the
superseded programme sweep moved to `.claude/stack/completed/`, every carried
phase got a board issue (#103–#111 opened; #31/#32/#33 carried by comment),
the duty that a koalisi phase open a tira extension's downstream lock was
retired, and this file's sections v0.29.0 → v0.1.0 of `CHANGELOG.md` were
archived. `K7-3` stays PAUSED; next is R0 (`v0.43.0`).

### Latest three

Detail for each: its `CHANGELOG.md` section and the ledger.

- **`K7-3` scaffold 2 — v0.42.0 (2026-09-19, #100 lock part 2)**: the trait
  hook is `observe_outcome(required, per_bit_success, members)` — one
  `MemberOutcome { agent_id, performed }` per final member, in membership
  order; every member performed when the instance carries no draw. Breaking
  for trait implementors and callers; the inherent
  `PersistentAifArm::observe_outcome` and `GroupAifPolicy::observe_outcome`
  are unchanged, so no file under `examples/` is edited. No policy reads the
  argument yet. No registration, no run.

- **`K7-3` scaffold — v0.41.0 (2026-09-18, #100 lock part 1, PR #101)**:
  `WorkflowResult::performance_scored`, `Some` iff the instance carries a
  performance draw — keyed on the draw, not on `OutcomeSignal`; the harness
  loop and `recon` share predicate, task rule and aggregation. No
  registration, no run; X-battery NOT RUN. The K7-3 prereg lists the
  scaffold's seed-7000 sighting among its pre-run executions.

- **`K7-2` — v0.40.0 (2026-09-18, #97, PR #99)**: **`FALSIFIED (novelty moves
  the outcome)`**, `grp-topo` 0.4248 vs `grp-topo-nonov` 0.1415, 30/30, seeds
  150..180. The label is scoped: `grp-topo` ≡ the engine-free `ref-prune` on
  600 of 600 tasks. Report: `docs/k7/ab-report-K7-2-novelty-routed-group.md`;
  gotcha 36.

### Lineages, verdict trail, seed ledger, run protocol — `docs/`

Since v0.33.0 these live in the public repo: **`docs/README.md`** carries the
K4 verdict trail (17 rows, every prereg + report by path), the K7 lineage
rows as they land (K7-1 since v0.38.0, K7-2 since v0.40.0), and the **seed
ledger** (90..120 consumed by K7-1, 150..180 by K7-2); **`docs/PROTOCOL.md`** is the standing run
protocol (design-lock → prereg → 3-lens review → serial run → immutable
report; pin-first; latency-column-stripped diff; review on every PR);
**`docs/runs/`** holds the committed raw outputs and the re-pin drift recipe.
The per-registration gotcha numbers are the index at the end of §Worth
flagging. Rust implementation is dispatched per
`.claude/stack/agent-dispatch.md`.

### Tests passing

Suite counts and commands are §Reproducers — one line per suite, the count in
its comment. Lint + docs: `cargo fmt --check`; `cargo clippy --all-targets --
-D warnings` per feature set; `RUSTDOCFLAGS='-D warnings' cargo doc --no-deps`
at every feature.

### File inventory

```
koalisi/
├── Cargo.toml                              git tag deps: catgraph-applied + catgraph-magnitude + catgraph-syntax v0.23.0 in lockstep (one checkout); aif-v0.14.0, surrealdb-live-message v0.2.2, libp2p 0.57, sha2 0.11, metrics 0.24 + metrics-exporter-prometheus 0.18 (optional); no path deps; declared MSRV 1.93 above the measured tiers 1.88/1.89/1.92 — owner decision C-D1: KEEP, gotcha 34
├── README.md                               user-facing
├── CLAUDE.md                               THIS FILE
├── config/{default,development,test}.toml  coalition threshold, history capacity; [sdb]+[docker] for the durable feature's upstream SETTINGS (cwd-resolved)
├── src/
│   ├── lib.rs                              module surface + re-exports
│   ├── main.rs                             domain-neutral reference daemon: CoalitionRuntime + seed coalition + policy-gated join loop via CoalitionService
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
│   │   ├── analytics.rs                    TemporalAnalytics, GraphDelta + magnitude_history/MagnitudePoint (feature `magnitude`; gotcha 16)
│   │   ├── coalitions.rs                   CoalitionManager (form/join/leave/dissolve/merge; agent_coalition_history, seed_feedback_history)
│   │   └── executor.rs                     HypergraphExecutor (rayon↔tokio bridge)
│   ├── algorithms/
│   │   ├── mod.rs                          AgentCapabilities trait + CapabilityAgent (stock impl, also a VertexTrait) + re-exports
│   │   ├── value_calculation.rs            ValueCalculator + 4 base calculators
│   │   ├── feedback.rs                     FeedbackCalculator<C> wrapper + shared FeedbackStore (history/failure weights; gotcha 19)
│   │   ├── dcvc.rs                         DCVCDistributor, WorkloadShare
│   │   ├── aipa.rs                         Integer partitions, bounds, best-partition + 10 unit tests
│   │   └── population.rs                   population coalition-structure search atop AIPA (SplitMix64 PSO, gbest lineage) + record_trajectory (always compiled, no deps)
│   ├── harness/                            the K7 harness (feature `harness`, no deps): rng.rs (SplitMix64), instance.rs (InstanceSpec → Instance), battery.rs (run_instance / run_battery; calls the H1 hooks once per task), report.rs (percentiles, tables, Verdict), trace.rs (TracedPolicy: (leave, act, score bits) per decision); with `process`: workflow.rs (the Part 9/11 v2w world — WorkflowSpec/PerformanceSpec, OutcomeSignal, run_workflow_battery, WorkflowResult::performance_scored = Some iff the instance carries a performance draw) and recon.rs (reconstruct: arrival orders + trace → final member sets, PRIMARY, churn, performance score; roster_decomposition; the engine-free RefPrune / RefFirst / RefKeep); registrations are examples, never here
│   ├── decision/
│   │   ├── mod.rs                          CoalitionDecisionPolicy (+ the default no-op lifecycle hooks begin_task(&TaskStart) / observe_outcome(required, &[bool], &[MemberOutcome])) + ThresholdPolicy (always compiled)
│   │   ├── aif_policy.rs                   AifDecisionPolicy + EfeValueCalculator (feature `decision`)
│   │   ├── aif_mm_policy.rs                AifMmDecisionPolicy: a stateless multimodal POMDP per decision from binary union coverage — the K4-v3 bridge (feature `decision`)
│   │   ├── aif_persistent_policy.rs        PersistentAifArm (arm-E1): a persistent per-bit world model + a fresh query POMDP per decision with a replay window; role_query is the group arm's seam (feature `decision`)
│   │   ├── reliability_value.rs            ReliabilityCoverage: reliability-weighted coverage ValueCalculator from the persistent world-model snapshot (feature `decision`; gotcha 24)
│   │   ├── group_policy.rs                 GroupAifPolicy: aif::GroupAgent, R=3 role internals over arm-E1 world models, CertaintyWeighted, deterministic group_distribution read; VoteRouting {Off, CandidateStar{lambda}} → aif::RoutedAggregator over a per-decision aif::Topology; the H1 trait hooks; ledger fields (features `decision`+`process`; gotcha 33)
│   │   └── magnitude_policy.rs             MagnitudePolicy + MagnitudeValueCalculator + CouplingModel + CoalitionEvaluator cache (feature `magnitude`; gotcha 15); relevant_masks/magnitude_or_zero pub(crate) for magnitude_history
│   ├── process/                            process-structured tasks (feature `process`; gotcha 30)
│   │   ├── mod.rs                          re-exports + the four things a caller must know
│   │   ├── signature.rs                    Role (Color) + Step { bit, role } : r → r + Workflow = ColoredExpr<FrobeniusOr<Step>>
│   │   ├── demand.rs                       Demand + demand(): multiset over User occurrences, distinct set (spiders contribute none)
│   │   ├── theory.rs                       rule_theory() — the 3 schemas closed over (bit, role), 174 instances; Schema/LabelledRule/fusion_pairs
│   │   ├── cost.rs                         uniform_cost + StaffingTable + staffing_price
│   │   ├── rewrite.rs                      optimize_workflow + verify_optimization (replay + content_eq — the S-sound helper)
│   │   ├── residual.rs                     ResidualPolicy — the unstaffable-residual valuation lever (gotcha 31)
│   │   └── errors.rs                       ProcessError
│   ├── ingest/                             domain-neutral ingestion layer (always compiled, no new deps)
│   │   ├── mod.rs                          re-exports
│   │   ├── sample.rs                       Sample trait (Key routing + timestamp_ms + View)
│   │   ├── monitor.rs                      SampleMonitor<S> + SampleUpdate/Snapshot + handle + spawn (generic ring-buffer monitor; gotcha 13)
│   │   ├── source.rs                       DataSource trait + Pacing + PumpStats + pump_source/spawn_source_pump
│   │   └── synthetic.rs                    MultiResolutionSource (NEST-shaped) + SensorEventSource (tauhokohoko-shaped, changepoint)
│   ├── llm/
│   │   └── mod.rs                          LlmProvider trait + StubLlmProvider (Phase 5 anchor)
│   ├── persistence/                        chained event log (feature `persistence`; gotchas 17, 18)
│   │   ├── mod.rs                          feature docs (hash contract, durability, at-most-once) + pub surface
│   │   ├── envelope.rs                     StreamId/SequenceNo/RecordHash/Payload/EventRef/Record/StoredRecord/StreamHead
│   │   ├── errors.rs                       PersistenceError (hand-rolled; StreamWedged; P7.3/P7.5 anchor variants)
│   │   ├── chain.rs                        FrameV1 (private serde mirror), FRAME_VERSION, hashing, back-link check
│   │   ├── store.rs                        EventStore trait + FileEventStore (segments, rotation, torn-tail recovery, wedge)
│   │   ├── writer.rs                       spawn_store_writer (spawn_blocking, drain-on-cancel)
│   │   ├── wire.rs                         WireTopologyEvent<VW,HW> (13-variant serde mirror, u64 fields) + schema version
│   │   ├── tee.rs                          spawn_topology_forwarder (tap → CBOR → store writer; shutdown disciplines)
│   │   └── replay.rs                       replay_into_event_log (batched read → fresh EventLog; quiescence precondition)
│   └── subsystems/
│       ├── coalition_actor.rs              CoalitionService + handle (policy-gated membership seam) + DecisionRecord tap + spawn_decision_tee (always compiled) — THE runtime seam
│       ├── outcome.rs                      TaskOutcome + OutcomeSink fan-out + emit_outcome tap + spawn_outcome_forwarder (always compiled; the L2 outcome seam)
│       ├── remote.rs                       libp2p request-response coalition-event gateway + EventBuffer + RemoteCoalitionClient (feature `remote`; gotcha 29)
│       └── durable.rs                      DecisionEvent + DurableDecisionBus + forwarder (feature `durable`; gotcha 14)
├── examples/
│   ├── topology_coalition.rs               coalition lifecycle + time-travel queries
│   ├── algorithm_values.rs                 value calculators + DCVC + AIPA
│   ├── synthetic_ingestion.rs              FLAGSHIP: NEST + sensor fixtures → generic monitors → coalition formation via CoalitionService (default features)
│   ├── supervised_monitor.rs               spawn_supervised restart demo over SampleMonitor<SensorEvent>
│   ├── population_search.rs                TaskCoverage-driven structure search + record/replay (default features)
│   ├── population_reliability.rs           outcome stream → world-model snapshot → ReliabilityCoverage → search + replay (feature decision)
│   ├── strategy_comparison.rs              FROZEN K4 archive binary (Parts 1–11; requires ALL THREE: decision,magnitude,process); changes only through src/; its job is the X-battery gate at re-pins against docs/runs/K4-archive.log
│   ├── gauntlet.rs                         K7 harness skeleton over src/harness/ (feature `harness`), zero registrations; K7 registrations are one [[example]] each under examples/k7/k7_<n>.rs (K-D3)
│   ├── k7/k7_1.rs                          the K7-1 registration binary (features harness,decision,process): eight cells on seeds 90..120, one VERDICT line; K7_1_SEEDS=a..b = off-block smoke, refuses 0..480; FROZEN — a later registration's PR never edits it
│   ├── k7/k7_2.rs                          the K7-2 registration binary (features harness,decision,process): seven cells on seeds 150..180, one VERDICT line; the registered block needs K7_2_OFFICIAL=1 AND a 0.40.0 build; K7_2_SEEDS accepts only 6000..6003 / 6000..6030 and prints gate lines only; any other K7_* env var refuses; FROZEN
│   ├── remote_coalition_consumer.rs        gateway + client in one process over a live CoalitionService (feature `remote`)
│   ├── metrics_scrape.rs                   Prometheus counters + histograms over the decision tee, the outcome forwarder and the topology event tap; self-scrapes /metrics once and asserts every series against a recorder-free count (feature `metrics`)
│   └── durable_decisions.rs                durable decision log end-to-end (feature `durable`)
├── .claude/docs/                           TRACKED internal design docs + references (rest of .claude/ stays gitignored)
│   ├── phase7-persistence-design.md        Phase 7 EventStore design (P7.1–P7.5 phasing)
│   ├── k3-hot-path-bench.md                K3 kameo-vs-tokio bench evidence
│   ├── SwarmAgentic-summary.md             Phase 5 paper digest (Zhang et al. 2025)
│   └── 2506.15672v1.{md,pdf} + _images/    the SwarmAgentic paper itself (CC0 per its PDF metadata)
├── docs/                                   PUBLIC A/B showcase trail (registered docs are immutable)
│   ├── README.md                           the index — K4 verdict trail (17 rows, every prereg/report by path), K7 rows, seed ledger, immutability rule, layout
│   ├── PROTOCOL.md                         the run protocol (design-lock → prereg → 3-lens review → serial run → immutable report; gates; review; seeds; naming)
│   ├── runs/                               committed raw outputs — README.md (archive + drift-check recipe), K4-archive.log (one serial run at v0.32.0 pins), K7-<n>.log per registration
│   ├── k7/                                 the K7 lineage, all immutable: a prereg (+ amendments) and an ab-report per registration — K7-1 topology-routed-group, K7-2 novelty-routed-group
│   ├── prereg-*.md + ab-report-*.md        the K4 lineage: 13 prereg + 16 report pairs/singles (v1/v2, K1, K6 report-only), each row of docs/README.md names both files; per-registration gotchas indexed at the end of §Worth flagging
│   └── baseline-aif-scalar-scope-b.md, per-bit-outcome-plumbing-design.md, k4-arm-choice-memo.md   the three memos (v4/v5 baseline anchor; per-bit design note, gotcha 23; arm-choice decision memo — DECIDED B+D, FINAL)
└── tests/
    ├── topology_test.rs                    12 tests
    ├── algorithms_test.rs                  18 tests (incl. 3 feedback-loop/seeding tests)
    ├── decision_integration.rs             4–6 tests (feature-dependent)
    ├── durable_integration.rs              1 container-backed restart test (feature `durable`)
    ├── ingestion_integration.rs            3 tests (synthetic sources → monitors → coalition formation; default features)
    ├── magnitude_trajectory.rs             6 tests (hand-computed trajectory semantics; feature `magnitude`)
    ├── population_test.rs                  4 tests (population coalition-structure search; default features)
    ├── common/                             shared fixtures for the integration suites (mod.rs + algorithms.rs + topology.rs)
    ├── persistence_integration.rs          7 tests (roundtrip, rotation+reopen, tamper, torn tail, sealed opaque, writer drain, bounds; feature `persistence`)
    ├── topology_replay.rs                  3 tests (13-variant round-trip, reconstruction equality, schema/Sealed rejection; feature `persistence`)
    ├── remote_integration.rs               1 test (loopback round-trip service → tee → gateway → client, cursor deltas + seq ordering; feature `remote`)
    ├── replay_parity.rs                    1 test (magnitude_history live == replayed — THE parity gate; features `persistence,magnitude`)
    ├── harness_workflow.rs                 6 tests (the H0 identity gate — Part 9 `wf-asis` rows on 270..300 + Part 11 medians on 330..360 from docs/runs/K4-archive.log, the instance fixture, the v2-prefix pin, a hand-derived role-mismatch case, the 150..180 generate check; features `harness,process`)
    ├── k7_group_host.rs                    5 tests (3 X-host: harness-hosted grp-role / -blind / -nonov on 330..360 reproduce docs/runs/K4-archive.log; 2 X-carry: grp-role, grp-topo and RefPrune on 90..120 reproduce docs/runs/K7-1.log; ~75 s in debug; features `harness,decision,process`)
    ├── k7_2_novelty.rs                     2 tests (K7-2's S-nov: the routed λ=½ read on a hand-built fixture under novelty on / off, fresh and after one observed task — eight pinned (act, score bits) values; features `harness,decision,process`)
    └── fixtures/k7-workflow-v2w.txt        the committed print of every generated v2w instance on 270..300 and 330..360 (regenerate with K7_WRITE_FIXTURE=1 — only when the world changes by design)
```

Three further files belong to this project but are **not in this tree** — the
`project-history.md`, `ab-lineage-gotchas.md` and
`changelog-archive-pre-v0.30.0.md` archives (see §Where the rest of the record
lives; location in `CLAUDE.local.md`).

## Worth flagging (gotchas)

These cost time during the build; future-me should not relearn them.
Engineering contracts (7, 11–19, 29) are verbatim below. The A/B-lineage
gotchas (20–28, 30–33, 35–37) are indexed at the end of this section and live in full
in the `ab-lineage-gotchas.md` archive. Obsolete ones (1–6, 8–10 — all
kameo / forex / databento-era) are in the `project-history.md` archive §4.
The archives are held outside this repo; see `CLAUDE.local.md`.
Numbering is preserved across all of them.

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

14. **`durable` feature gotchas (surrealdb-live-message v0.2.2).**
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
      `SurrealDBContainer` (bollard) manages the instance. **koalisi has no
      container code** (`rg -n 'bollard|SurrealDBContainer' src tests
      examples` prints nothing): the test and the example boot the
      DB through upstream's `sdb::sdb_task`, and what koalisi owns is the
      `[sdb]` keys upstream reads from `config/default.toml` — `image`,
      `tag` (`v3.2.4` since v0.39.0, upstream's own test tag) and
      `container_name = "koalisi_sdb_container"`. `docker events --since
      <t> --until 0s --filter event=create` shows which image a run used.

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
    - **The MSRV gate verifies the DECLARATION (owner, 2026-09-17).**
      `cargo +1.93.0 check --all-targets --locked --features <set>` on the
      feature sets a PR's dependency or source change reaches (find them
      with `cargo tree --locked --features <sets> -i <crate>`; `durable`
      sits in no other set's superset). The host toolchain is newer than
      1.93, so this is the one gate that sees a std API or a dependency
      newer than the declaration. **Never `--ignore-rust-version`**: it
      suppresses the dependency checks too. The tier re-measurement
      (temporary `rust-version = 1.85.0`, per-tier probes, below-tier
      failures) is RETIRED — with 1.93 declared, cargo refuses every lower
      toolchain before it evaluates a dependency, and no decision reads the
      tiers.
    - **The tiers in `Cargo.toml`** (1.88 / 1.89 / 1.92) are a historical
      record, last measured at v0.37.0. **`rust-version = "1.93.0"` is
      DECLARED above them — owner decision C-D1 (2026-09-14): KEEP.** Do
      not change it in passing. Ground: edition 2024 means resolver 3, so the declared value
      bounds dependency resolution (`cargo update` holds packages to
      declared-MSRV-compatible versions; the opt-out is `[resolver]
      incompatible-rust-versions = "allow"`). Cost: a 1.88–1.92 downstream
      is refused a default-only build that would compile.
    - **Four wrong MSRV claims were retired between v0.17.0 and v0.31.0**,
      every one from measuring a chosen subset and generalising (the
      ledger has them). State which sets a check covered; cite the command.
    - **`cargo test … | rg '^test result' | tail` truncates.** Bare `tail` is
      `tail -10`; suites with 11+ result lines (`persistence,magnitude`) lose
      the lib-test line and undercount by ~95. Always `tail -20`.

### A/B-lineage gotchas 20–28, 30–33, 35–37 — index only

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
35. **K7-1 routed group (#90)** — on v2w under `OutcomeSignal::RoleCoverage` an
    engine-free redundancy prune reaches the routed arm's PRIMARY, so a learning
    axis read there may be dead at the outcome (register a no-engine reference
    cell; if dead, move the WORLD); candidate-star routing above λ ≈ 0.131 makes
    ONE voter decide; EQ5b's "blind voters are load-bearing" was carried by its
    mask change; positional trace diffs overstate act divergence; the H1 hook
    carries distinct steps; never smoke the confirmatory contrast before the run.
36. **K7-2 novelty on the routed group (#97)** — routing removes the
    candidate-blind VOTERS, not the candidate-blind QUERIES (the centre's own
    identical-mask reads); without the A-novelty term such a read follows its
    replay window (success ⇒ decline); the novelty-off arm loses on SUCCESS at
    the same size; `grp-topo` ≡ `ref-prune` 600 of 600, so key a continuation
    on the arm-vs-prune leg; `OutcomeSignal` does not reach the score; a
    "bytes rendered" smoke line and a multiplicative gate falsification both
    leak; a self-gated test file passes with 0 tests.
37. **Why three K7 registrations in a row reached the engine-free prune —
    the equivalence is DEFINITIONAL, not empirical (#100)** — `RefPrune`
    joins unconditionally and leaves iff redundant (`src/harness/recon.rs:454-478`),
    so ANY policy with those two act-sets IS `ref-prune` on member sets before
    any run; the world's single arrival-order leave sweep
    (`src/harness/workflow.rs:558-562`, `:614-628`) fixes the tie-break, so an
    arm that evaluates candidates ABSOLUTELY inherits arrival order; the escape
    is a COMPARATIVE read and needs no new plumbing; `RefPruneId` is the
    engine-free twin that BOUNDS every arm — run the engine-free pair first.

## Reproducers

All assume `cwd = koalisi/`.

```sh
# === default features (106 tests) ===
timeout 60s  cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target
timeout 30s  cargo run  --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --example topology_coalition
timeout 30s  cargo run  --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --example algorithm_values
timeout 30s  cargo run  --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --example synthetic_ingestion   # FLAGSHIP
timeout 30s  cargo run  --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --example supervised_monitor
timeout 30s  cargo run  --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --example population_search

# === decision-layer feature combos (162 / 135 / 191 tests) ===
timeout 120s cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features decision
timeout 120s cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features magnitude
timeout 120s cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features decision,magnitude
timeout 120s cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features magnitude-fast   # 143 (EQ3 L2+L3 + probes)
# strategy_comparison needs ALL THREE features — see the process block below.
timeout 60s  cargo run --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features decision --example population_reliability

# === with persistence feature (P7.1 store + P7.2 replay, 126 tests) ===
timeout 120s cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features persistence
timeout 120s cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features persistence,magnitude   # 156, incl. the replay parity gate

# === with remote feature (gateway, 112 tests) ===
timeout 120s cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features remote
timeout 60s  cargo run  --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features remote --example remote_coalition_consumer

# === with process feature (161 tests) ===
timeout 120s cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features process
timeout 300s cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features decision,magnitude,process   # 249
# NOTE: strategy_comparison is the FROZEN K4 archive; the battery runs
# SERIAL on a quiet machine with NO timeout wrapper — the archive + drift-check
# recipe is docs/runs/README.md.
cargo run --release --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features decision,magnitude,process --example strategy_comparison

# === with harness feature (131 tests; with process 231) ===
timeout 120s cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features harness
timeout 300s cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features harness,process   # 231, incl. the H0 identity gate
timeout 60s  cargo run  --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features harness --example gauntlet

# === K7-1 / K7-2 (features harness,decision,process, 326 tests incl. X-host, X-carry, S-nov) ===
timeout 600s cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features harness,decision,process
timeout 300s cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features harness,decision,process --example k7_2   # k7_2's own 7 unit tests (guards, mask mirror vs ledger on 90..93)
# K7-2's run of record is docs/runs/K7-2.log; the registered block refuses on any build but 0.40.0. Smoke (gate lines only, no cell value):
K7_2_SEEDS=6000..6003 cargo run --release --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features harness,decision,process --example k7_2
# The run of record is docs/runs/K7-1.log (serial, quiet machine; a 30-seed off-block smoke measured 66 s in release). Off-block smoke only:
K7_1_SEEDS=5000..5003 cargo run --release --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features harness,decision,process --example k7_1

# === with metrics feature (106 tests = the default suite) ===
timeout 120s cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features metrics
timeout 60s  cargo run  --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features metrics --example metrics_scrape   # KOALISI_METRICS_ADDR overrides the loopback listener address

# === with durable feature (107 tests; needs Docker; container-backed restart test) ===
timeout 300s cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features durable
timeout 120s cargo run  --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features durable --example durable_decisions
```

## Next steps

The 2026-07-03 design gate is RESOLVED (both inputs received). Phase narratives
— Phase 5 SwarmAgentic, Phase 6 decision layer + K1–K6, Phase 7 persistence —
are in the `project-history.md` archive §2, along with the gate record itself.
What is still open:

- **Phase 7 implementation**, in this order (the reorder is the ratified one;
  each issue carries its own scope comment, 2026-09-20):
  [#32](https://github.com/sustia-llc/koalisi/issues/32) P7.4 decision streams
  **minus the belief leg** → [#31](https://github.com/sustia-llc/koalisi/issues/31)
  P7.3 sealing + revocation registry **plus #32's belief leg** →
  [#33](https://github.com/sustia-llc/koalisi/issues/33) P7.5 federation
  manifests + FAIR provenance. **Not blocked on the tauhokohoko
  KEK-granularity answer**: if no reply has landed at #31's kickoff the owner
  rules granularity in-house and records it on #31 before any belief-sealing
  code. Design of record: `.claude/docs/phase7-persistence-design.md`; open
  calls in its §17 (SHA-256 vs BLAKE3; ciphertext reclamation;
  cross-federation `EventRef` addressing — resolve at #33).
- **Phase 5 remainder** — [#20](https://github.com/sustia-llc/koalisi/issues/20)
  keeps the LLM-dependent meta-layer (configurator + velocity-rewrite loop +
  transferability). Both LLM-free slices shipped (#41 v0.12.0, #42 v0.13.0). The
  only still-NEST-dependent piece is the NEST-H4 calibration-copilot deployment
  framing, which activates whenever ownership lands.
- **K7 lineage** — plan of record `.claude/stack/2026-09-20-koalisi-refocus.md`
  (ratified 2026-09-20; supersedes the remaining phases of the 2026-09-16
  programme sweep, now `.claude/stack/completed/`). Landed: the harness
  (v0.33.0), the board corrections, tira's `aif-v0.14.0`, the H scaffold
  (v0.35.0, #92), the #25 metrics example + a lock refresh (v0.36.0), the
  `aif-v0.14.0` re-pin (v0.37.0), `K7-1` (v0.38.0, `VALIDATED
  (topology-routed group)`), the `surrealdb-live-message` `v0.2.2` re-pin
  (v0.39.0), `K7-2` (v0.40.0, #97, `FALSIFIED (novelty moves the outcome)`),
  the `K7-3` harness scaffold (v0.41.0, `WorkflowResult::performance_scored`),
  scaffold 2 (v0.42.0, `MemberOutcome` on the outcome hook) and the plan's
  Phase N (2026-09-20, no code — the board and the notes).
  **`K7-3` is PAUSED pre-run** ([#100](https://github.com/sustia-llc/koalisi/issues/100));
  its prereg, library change and binary sit on branch
  `k7-3-performance-scored-world` at version 0.42.0, and **no seed of
  540..570 has run**. The pre-run review derived that `grp-id` as locked
  evaluates each candidate absolutely, so it inherits the world's
  arrival-order tie-break and reproduces the engine-free `ref-prune` —
  gotcha 37. **Next, in order:**
  1. **R0** (`v0.43.0`) — the engine-free pre-test: `RefPruneId` against
     `RefPrune` on `performance_scored.primary`, off-block, a test with no
     `VERDICT:` line and no seed block consumed. If reading identity moves
     nothing with no engine, no engine-bearing arm can win on this world.
  2. **F** (`v0.44.0`) — the four `K7-3` review findings plus the promotion of
     registration-agnostic code into `src/harness/report.rs`, forward-only
     (`k7_1.rs`, `k7_2.rs`, `strategy_comparison.rs` keep their private
     copies and are not edited).
  3. **R1** — `grp-id` re-locked as a **comparative** read (the centre scores
     the candidate against each same-step role-mate it can see), lock part 3
     on #100, then one amendment, then one serial run on **540..570**.
  4. Then #32 → #31 → #33 (P7.4 / P7.3 / P7.5), then #103 (EQ5b (c), block
     360..390), then the tag-gated phases from #105–#111.
  **A lock is not postable** until it names one read on which the arm's act
  differs from the engine-free reference and the term that makes it differ,
  and names which of two same-step coverers survives and through what input;
  "arrival order", or a term that cannot fire, means the arm is not
  registered. **Any novelty contrast on this world inherits K7-2's
  mechanism** — read `docs/k7/ab-report-K7-2-novelty-routed-group.md` §3
  first. **Numbering (owner, 2026-09-18):** the world-move took `K7-3`;
  every later registration takes the next free number when its own lock is
  posted, and the forward seed blocks are provisional notes — the
  authoritative ledger is `docs/README.md`.
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
