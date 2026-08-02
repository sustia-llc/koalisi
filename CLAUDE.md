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

## Mission (one paragraph)

**koalisi** — a reference implementation of agentic coalitions in Rust.
Four-layer architecture: Core (CoalitionRuntime, lifecycle), Topology
(temporal hypergraph via `catgraph_applied::Hypergraph` since K1 — was
yamafaktory hypergraph v4.2.0 — event sourcing, CoalitionManager,
time-travel queries, analytics), Algorithms (DCVC workload distribution,
AIPA partition search, pluggable value calculators), and Runtime (since K3:
tokio tasks with mpsc/oneshot command handles — kameo is gone — the
`CoalitionService` policy-gated membership seam, a thin task-restart layer,
and an optional SurrealDB-backed durable decision log). koalisi began as a
forex triangular-arbitrage tool; that domain was removed in v0.11.0 (#37) —
the architecture is domain-agnostic and the demonstrated runtime is now a
synthetic coalition-formation pipeline. Market/trading work lives in the
sibling `biome` project.
Evolved from four prior projects: dynamo (topology), coalesce (algorithms),
coalition_aif (decision — planned), and forex-arbitrage-swarm (runtime — the
forex domain since removed).

## Available tooling for this project

- **`causality` (DeepCausality) plugin** — the relevant graph tooling since K1
  (#4): koalisi's topology backend is `catgraph_applied::Hypergraph` (plain
  `Vec`/`HashMap` container — read its module docs at the pinned catgraph tag
  for the contract), and catgraph itself builds on the DeepCausality substrate
  (`deep_causality_num` `Zero`/`One`; `ultragraph` for catgraph's graph
  algorithms, Track A). Use `causality:causality-applied` (skills
  `causality:causal-graphs` — ultragraph — and `causality:data-structures`)
  for substrate-level questions, and `causality:causality-theory` for the
  algebraic layer (`Rig`, HKT/witnesses) catgraph's enrichment sits on.
- ~~`graph` plugin v2.0.1~~ (yamafaktory hypergraph skills) — **OBSOLETE for
  `src/topology/` since K1**; historical reference only (pre-K1 semantics, the
  dropped `PersistentHypergraph` idea — see the Phase 7 note).
- `rust-v2:rust-dev-v2` / `rust-v2:rust-practical` — primary Rust agents per
  the user CLAUDE.md routing rules.
- `surrealdb:surrealdb-rust-v3` / `surrealdb:surrealdb-search` /
  `surrealdb:surrealql-language` — for K3 (#6) surrealdb-live-message work,
  per the user CLAUDE.md routing rules.

## Current state — 2026-08-02

### Done

- **catgraph re-pin `v0.5.0` ×2 → `v0.6.0` — v0.21.0 (2026-08-02)**: the
  EQ3 pin-first step — the latency-re-match registration
  ([#69](https://github.com/sustia-llc/koalisi/issues/69)) must be born
  on the final pins. Both catgraph deps bumped in lockstep (K6 rule).
  **Drift check CLEAN**: six suites at baseline counts
  (103/159/125/181/123/146 incl. the replay parity gate), default clippy
  `--all-targets` clean, frozen K4 battery (Parts 1–6, `--release`)
  reproduced byte-identical on every quality/churn/verdict line vs a
  fresh v0.5.0 baseline — latency-only diffs, per the protocol's standing
  exclusion. Lockfile moved exactly the three catgraph workspace packages
  (+ rider: refreshed the lock's stale `koalisi 0.19.0` version line);
  v0.6.0's breaking set is applied-side (cg#202/cg#185) and untouched by
  koalisi's consumption; MSRV stays 1.93. Downstream catch: README's
  catgraph tag citations were still `v0.2.0` (missed in the v0.5.0
  re-pin) — fixed to `v0.6.0`. **EQ3 itself is IN FLIGHT**: #69 carries
  the owner design-lock (posted BEFORE prereg, 2026-08-02): D2 strict
  Path-A analogue; D3 all three levers registered (cg#33
  `value_with_scratch` + cg#153 `value_with_report` proof-branch + cg#165
  `f64-fast` via the public `HomMap` → `Coalition::from_enriched` →
  `as_weighted_cospan().clone().into_metric_space()` → `magnitude_f64`
  route — no coalition-level f64 entry exists upstream, verified at the
  tag); D4 fresh seeds 210..240 (90..120 + 150..180 stay reserved);
  placement = library, frozen-battery X-gated (feature-off AND
  feature-on-flag-off); `KNIFE_EDGE_REL_BAND` narrowing is
  measurement-only this arc (the cg#153 [1e-13, 1e-6) empty-band check
  on koalisi traffic). Next: prereg doc → 3-lens review → official run.
  Kickoff: `.claude/plans/2026-08-02-eq3-latency-rematch-kickoff.md`.
  ⚠ checkout hygiene: `${BASE_PATH}/tsondru/catgraph` carries a STALE
  `v0.6.0` tag (old pre-workspace lineage, different commit) — the
  checkout of record is `${BASE_PATH}/sustia-llc/catgraph`.
- **aif re-pin `aif-v0.11.0` → `aif-v0.12.0` — v0.20.0 (2026-08-01)**: the
  pre-EQ4 adoption slice, run under the standing re-pin protocol (own PR,
  no code changes mixed in). **Drift check CLEAN**: six suites at baseline
  counts (103/159/125/181/123/146 incl. the replay parity gate), default
  clippy `--all-targets` clean, frozen K4 battery (Parts 1–6, `--release`)
  reproduced with all quality/churn/verdict content byte-identical —
  latency-only diff lines, per the protocol's standing exclusion. Lockfile
  moved exactly one package (`aif 0.12.0`); MSRV unchanged (1.93). The one
  breaking rider (tira#9 serde feature-gating on
  `Message`/`MessageContent`/`InfoRequestType`) is grep-verified unused in
  koalisi. What the hop buys: tira#39 generic blanket slots
  (`InternalAgent`/`Aggregator`) + tira#41 `GroupAgent` nesting — the
  aif-side substrate EQ4 (typed roles) registers against; the EQ4 prereg
  gate "aif 0.12.0 adopted first" is now DISCHARGED. Sources of the
  release-anchored "aif 0.11.0 count injection" wording
  (`aif_persistent_policy.rs` docs, the Part 3 battery label) deliberately
  unchanged — they date the mechanism, not the pin, and the battery label
  is frozen report text.
- **#63 corrected block-level routing test — 2026-07-31 (example+docs, no
  bump)**: the EQ1 tail, full discipline (owner design-lock D1–D6 on #63
  BEFORE prereg; prereg `docs/prereg-K4-routing-corrected.md` committed
  BEFORE implementation; 3-lens review — 1 blocking + 11 important + 12
  minor, ALL applied/adjudicated — BEFORE the official run; report
  `docs/ab-report-K4-routing-corrected.md`). New Part 6 in
  `strategy_comparison`: coverage-guaranteed rejection re-draw, block-level
  highest-value-block skip predicate + b*-strong attribution counterfactual,
  seeds 180..210. **H-BR `FALSIFIED (block-routing)`** — sanity 28/30 PASS,
  skip 4/30 vs bar 18; mechanism-scoped: the b*-window (4.71/4.13) barely
  exceeds the competing singleton-value lattice (4.00/3.50), and the 4
  firing seeds are exactly the window hits (an exact-optimum replica
  predicted 3/30 pre-run). W forms the full block 29/30 ⇒ ranking channel.
  **Leg C (product-form full bonus) `DEGENERATE (context only)` 30/30** —
  the FOURTH gotcha-21 mechanism. **Leg L: learned-posterior ordering
  30/30** even at 0.02-vs-0.35 — the feasibility fact survives; the routing
  blocker is value-model geometry, not the signal. Gotcha 24 strengthened,
  scoped to the window-vs-lattice geometry. See **gotcha 26**. Suites
  unchanged (181 decision,magnitude; example binary 26). Seeds consumed:
  180..210; 90..120 + 150..180 still reserved.
- **EQ1 Part 5c addendum — v0.19.0 (2026-07-31, #61)**: the four deferred
  registered-exploratory items, appended to the immutable report
  (`docs/ab-report-K4-battery-v2.md` §Appended addendum — 7-entry
  implementation/deviation ledger; registered sections untouched; X-C
  re-verified against a fresh pre-change baseline, latency-only diffs).
  Library: `PersistentAifConfig::n_bits` (identity default 8, clamp
  1..=16 + warn; `observe_outcome` now takes `&[bool]` — breaking but
  coercion-compatible; `decide()` masks `required` to the universe). All
  context, non-gating: **(1) 12-bit slice** — the join rail is
  margin-proof at 12 bits too (all join quantiles exactly +0.5) and the
  v2-regime inversion WIDENS (mag 0.0607 < scalar 0.1062 < e1-degraded
  0.1657, ≈2.7× mag; latency ~2 ms/decision at query joint ≤8192);
  **(2) hysteresis** — FIRST score-space lever to move churn on the E1
  lineage (h=0.30: 0.85× churn, 29/30 paired) but pays −24% quality ⇒
  live-but-expensive, the #56 state-lever conclusion stands; **(3)
  expected-outcome model `MOSTLY DEGENERATE`** (third gotcha-21 mechanism:
  per-block partial-term double-counting; re-run context only); **(4)
  learned-posterior twins** — gotcha-24 ordering check 30/30 (r̂ ranks the
  planted weak bit below strong on every seed; REAL median unchanged) ⇒
  the #63 corrected test's learned input is feasible. 3-lens review
  (correctness / registration-conformance / modeling-semantics): 0
  blocking; all findings applied incl. the two cell-selection disclosure
  items. Seeds: 120..150 re-used per the addendum's registration; 90..120
  + 150..180 still reserved. Suites: **159** decision / **181**
  decision,magnitude (+5 lib tests; +9 example-binary tests, one
  release-only `#[ignore]`). See gotcha 25 (extended).
- **EQ1 battery v2 RUN — v0.18.0 (2026-07-31, #61)**: the registered
  de-saturated-regime run; BOTH confirmatory levers negative, everything
  measured and reported (`docs/prereg-K4-battery-v2.md` d9881e1 pre-impl +
  `docs/ab-report-K4-battery-v2.md`). Design-lock (owner, 5 decisions +
  sub-params) posted to #61 BEFORE prereg; prereg BEFORE implementation.
  **Lever 2 `FALSIFIED (de-saturation)`**: no (γ ∈ {1,4}, v2-draw, δ>0)
  cell nears churn ≤ 0.5× (best 173 vs 175 base; bar 87.5) — mechanism:
  γ de-saturates ONLY the leave stream (g1/v2 leave p25 0.1137); the JOIN
  stream stays at p = 1.0 in every cell and the registered margin acts on
  joins (see gotcha 25). **Lever 1 `RUN-INVALID (sanity leg)`** (25/30 <
  27): pool draw doesn't guarantee coverage + the skip predicate is
  unsatisfiable under `search()` partition semantics — corrected
  registration = **#63**; gotcha 24 untouched either way. **Lever 3
  (exploratory)**: oracle–degraded gap ~2% (v1-draw) → ~16% (v2-draw).
  **Context rows (non-gating): the v2 regime INVERTS the v1 ordering —
  mag 0.1286 < scalar 0.1332 < e1-degraded 0.1621** (mag churn ~10; #54
  B+D untouched per non-goals). New lever
  `PersistentAifConfig::query_gamma: Option<f64>` (identity `None` = γ16;
  MeanField path only; labels arm-E1g1/g4/g16, arm-E1 frozen). Parts
  5a/5b additive (X-C byte-identity PASS; `run_seed_b` → thin V1 wrapper
  over `run_seed_b_regime` — regime param only, hook unchanged since 4g). **Part 5c (registered exploratory: 12-bit slice,
  leave-side hysteresis h, expected-outcome model, learned-posterior
  twins) DEFERRED to a follow-up session — lands as an appended addendum,
  never edits to registered sections.** Seeds: 120..150 consumed;
  150..180 reserved-unconsumed; 90..120 still soft-reserved. Suites:
  **154** decision / **176** decision,magnitude (+2 query_gamma tests);
  others unchanged.
- **catgraph re-pin v0.2.0 → v0.5.0 — v0.17.0 (2026-07-30)**: the EQ1
  ([#61](https://github.com/sustia-llc/koalisi/issues/61)) pin-first step —
  battery-v2 registrations must be born on the final pins. Both catgraph
  deps bumped in lockstep (K6). **Drift check CLEAN**: six suites at
  baseline counts (103/152/125/174/123/146 incl. the replay parity gate),
  default clippy `--all-targets` clean, and the frozen K4 battery
  reproduced with all quality/churn/verdict content byte-identical —
  latency-only diff lines, per the protocol's standing exclusion. Recorded
  finding: **MSRV 1.88 → 1.93** (forced by `deep_causality_num 0.4.1`;
  catgraph itself declares no rust-version); lockfile also dropped the
  `primal`/`hamming` tree. **EQ1 itself is the next arc** (stack E-queue
  rank 1; #61 carries the hypothesis + the five design decisions to lock
  with the owner BEFORE prereg; kickoff
  `.claude/plans/2026-07-30-eq1-battery-v2-kickoff.md`). Rider queued on
  #61: the aif `0.11.0 → 0.12.0` adoption decision (breaking serde
  feature-gating) resolves before the EQ1 prereg — tag NOT yet on the tira
  remote as of 2026-07-30, confirm before acting.
- **#57 e1-derived ValueCalculator — v0.16.0 (2026-07-27)**: the #54
  option-D slow-loop seam, shipped. New `src/decision/reliability_value.rs`
  (feature `decision`): `ReliabilityCoverage` — the `TaskCoverage`
  interior-optimum shape with each required bit weighted by the persistent
  world model's reliability posterior (`beliefs[b][0]`, state 0 = reliable;
  `from_state(&PersistentAifState)` + plain-array `new`). Design sketch
  posted to #57 BEFORE code (prereg-lite; owner calls: beliefs source /
  TaskCoverage skeleton / decision-gated placement). All-ones reduces
  EXACTLY to `TaskCoverage` (low 8 bits); 5 unit tests incl. the gotcha-21
  non-degeneracy discipline. Example `population_reliability` (E1 config)
  = the #42 acceptance pattern end-to-end. 3-lens review (correctness /
  gating-conventions / modeling-semantics): 2 blocking + 6 important +
  9 minor, ALL applied — the semantics lens measured two doc overclaims
  (recency-dominated beliefs; "routes around weak bits" false at these
  coefficients) now corrected as contracts — see **gotcha 24**. Suites:
  103 default / **152** decision / **174** decision,magnitude (frozen
  battery untouched). First contemporaneous resumed tag: `v0.16.0`.
- **Public-release sweep — 2026-07-27 (docs+metadata, no bump)**: the
  repo is flip-ready. History audit CLEAN (no plans/notes/secrets ever
  tracked; accept-history recommended — a rewrite would churn every tag
  hash). All four git-tag deps re-pinned SSH→HTTPS (tira went public
  mid-session; catgraph + surrealdb-live-message already were) — outside
  builds work for every feature. Added `LICENSE-MIT`/`LICENSE-APACHE` +
  Cargo.toml `repository`/`readme`; `Cargo.lock` now TRACKED (was
  gitignored); README gained "The A/B process" (K4 v1→v6 verdict trail,
  mirrors tira's "Downstream A/B" section) and the aif tag drift fix;
  CLAUDE.md scrubbed of private notes-repo wiring. Prereg/ab-report docs
  untouched (registered docs are immutable). Remaining = owner ratifies
  accept-history + flips visibility.
- **#56 K4-v6 never-evict arm — v0.15.0 (2026-07-18): `FALSIFIED
  (never-evict)`**. The #54 option-B registered adoption path, run and closed
  same day (prereg 68ba8fc posted PRE-implementation; owner-locked lever =
  `eviction_cap: Some(0)`, BOTH signals gating, hybrid bar).
  `PersistentAifConfig` gained `eviction_cap: Option<u32>` +
  `rejoin_lockout_tasks: u64` (identity defaults — X-A reproduced
  0.4406/136.00, all frozen gates held); Part 4h = the registered battery
  (seeds 60..90). **Result: ne 0.0143/0.0141 vs bar 0.3551, superior to
  scalar 3/30 — H1+H3 FAIL both signals.** Mechanistic headline (exploratory
  cap series, monotone 0→0.014 / 1→0.020 / 2→0.025 / 4→0.047 / ∞→0.384):
  **eviction churn IS the e1 mechanism** — the leave sweep converts
  reliability beliefs into small high-cov_eff coalitions; hardens #54 Step 1b
  to a causal reading. Lockout axis gentler (k=1: 0.3222 at churn 104.5 =
  1.13× mag) but under the family bar — if any interior point is ever tried,
  it's lockout on seeds 90..120 (none implied). Context: e1-k0 replicates on
  a third range (0.3840 vs mag 0.2841, 1.35×; churn 186.5). **The E1 lineage
  now has both damping directions measured: score-space inert (4f),
  state-space destructive (4h). Memo option B park is FINAL: mag = default,
  e1 = capability evidence; #57 (slow loop) unaffected.** Report
  `docs/ab-report-K4-v6-never-evict.md`. Suites: 103 default / **169**
  decision,magnitude (+3 lever tests).
- **#55 task-completion event seam — v0.14.0 (2026-07-18)**: the #54 option-B
  no-regret build. New always-compiled `src/subsystems/outcome.rs` (zero new
  deps): `TaskOutcome { required, members, success }` (the L2 contract —
  whole-task success suffices per #54 Step 2), `OutcomeSink` fan-out trait
  (`FeedbackStore` scalarized impl + closure impl = the arm side-channel; the
  `Fn` blanket is THE arm-attachment route — a future trait-bound blanket
  would collide with it, conscious decision), `emit_outcome` (try_send
  drop-with-warn, the tap contract), `spawn_outcome_forwarder` (biased cancel
  vs drain-to-None — pick one discipline, don't mix; **panicking sinks are
  isolated** via per-sink catch_unwind, review-caught). Emission is the
  DOMAIN's job — koalisi never synthesizes outcomes; NOT a `TemporalEvent`
  (durable home = P7.4 #32). Demo: `synthetic_ingestion` §4 (deterministic
  outcomes, lossless-only token). 5 module tests. **Suites: 103 default /
  166 decision,magnitude.** 8-angle review: 2 confirmed applied (panic guard;
  example discipline-mixing), 3 conventions applied (version/CHANGELOG/this
  entry), rest refuted-as-designed or example-trivia — see PR #58.
- **#54 Steps 1–2: arm-choice evidence — 2026-07-18 (example+docs only, no
  bump)**: the cheap-evidence + load-bearing-feasibility steps from the #54
  sequencing comment. `examples/strategy_comparison.rs` gains **Part 4e**
  (unregistered, exploratory; Parts 1–4d frozen and reproduced byte-identical,
  X2 gate 0.4042/210.00 held): (a) **mag churn on 30..60 finally measured —
  median 8.00** vs e1 136 (17×; the churn gap is now a number, not folklore);
  (b) **degraded-outcome e1** — `run_seed_b`'s hook gained a third
  `success: bool` arg and `persistent_battery_range_degraded` feeds
  `observe_outcome(req, &[success; 8])` (whole-coalition success smeared across
  required bits — the runtime-feasible L2 signal). **Result: degraded ≈ oracle**
  (0.4381 vs 0.4406 median, bit-identical 17/30 seeds, scalar 0.1267) ⇒ the
  per-bit oracle signal is NOT load-bearing; the runtime needs only ONE new
  domain-emitted task-completion event `(required, members, success)` — the
  #41-seam information contract. Step 1b correlation (offline): e1 churn vs
  quality Spearman **+0.22**, vs e1−mag margin **+0.17** — e1 wins *with*
  thrash, not despite it; Step-3 mitigation is plausible but not free. Design
  note: `docs/per-bit-outcome-plumbing-design.md` (fidelity ladder L0/L1/L2 +
  runtime-seam inventory + event sketch: NOT a `TemporalEvent`; durable home =
  P7.4 #32). See **gotcha 23**. Suites: decision,magnitude **161** (v4/v5 cycle
  added 10 over the table below; re-measured green 2026-07-18).
  **Step 3 ran same day (owner GO; Part 4f, 66ea144): NULL — the frontier is
  FLAT.** `MarginE1` wrapper (identity at (0,0) asserted per-seed) swept
  δ∈{0..0.45}×h∈{0..0.30} under both signals: every cell 0.4406/136 (oracle) /
  0.4381/143–142 (degraded). Mechanism: the fixed-γ=16 posteriors **saturate at
  ±0.5** (p25–max of all join/leave scores = 0.5000) — decisions are binary, no
  sub-0.5 threshold separates them; e1 churn is confident flip-flopping from
  between-task belief updates, not marginal indecision. Score-space
  margins/hysteresis are a dead lever; churn mitigation needs a STATE-based
  design (dwell-time/cooldown — new arm, own registration). No v6 candidate
  from this lever. **Step 4 memo COMMITTED same day:
  `docs/k4-arm-choice-memo.md`** — full lineage v1→v5 + Steps 1–3 ledger;
  options A (adopt e1) / B (keep mag + event seam + state-based v6) / C
  (hybrid) / D (slow-loop fitness); recommendation **B + event seam + D**;
  **owner decision PENDING** (§6 checkboxes). **Post-memo probe (owner-approved,
  Part 4g, f0c13eb): C's cheap form RULED OUT** — mag + #41-FeedbackStore veto
  is WORSE than bare mag in every cell (absorbing-exclusion mechanism; see the
  new gotcha-23 bullet); catgraph scan found no mag-quality lever pending
  (catgraph#33 = latency polish only, #46 = role research, wrong axis).
  Recommendation B+D strengthened; memo §4-C updated in place.
  **DECIDED (owner, 2026-07-18): B + D — #54 CLOSED.** Follow-ups filed:
  **#55** (arm-agnostic task-completion event seam — build FIRST, no-regret),
  **#56** (K4-v6 state-based low-churn E1 arm, dwell/cooldown; prereg on seeds
  60..90, bar ≥ ~0.40 at materially lower churn; design decisions to lock with
  owner BEFORE prereg — lever choice, k grid, oracle-vs-degraded signal),
  **#57** (e1-derived `ValueCalculator` for the #42/#20 population-search
  fitness; depends on #55). Magnitude stays the demonstrated default arm.
- **Selective-base feedback arm — #48 (2026-07-17, example-only, no bump)**: the
  #46 rematch on a *selective* base. `examples/strategy_comparison.rs` gains
  **Part 4** (the frozen Part 3 #46 battery is the byte-identical regression gate):
  arms `mag` (frozen) / `thr-selective` = `ThresholdPolicy(Synergistic,
  join_threshold=100, leave=0)` / `fb-selective` = same over
  `FeedbackCalculator(Synergistic, hw=0, fw=1)` — decomposing magnitude's edge into
  **selectivity** (thr-selective) + **reliability-gating** (the fb-selective
  increment). **Verdict: `PARTIAL (mechanism only)`** — Scope B medians `mag 0.2818 ·
  thr-selective 0.0301 · fb-selective 0.0512`; H1 FAIL (mag ~5.5× ahead, unbeaten),
  H2 PASS (`fb ≥ 1.25×thr` AND fb superior to thr 21/30). So magnitude's dominance is
  **not** pure selectivity — feedback adds a real reliability signal on a selective
  base — but it doesn't close the gap. #49 **absorbed** (registered `hw=0,fw=1`).
  See **gotcha 22**; prereg `docs/prereg-feedback-arm-k4-v2.md`, report
  `docs/ab-report-feedback-arm-k4-v2.md`. Independent review: 0 blocking/important, 1
  minor efficiency applied (single-battery sweep). Suites unchanged (98/129/120/151/
  118/141 — example-only).
- **Population search atop AIPA — v0.13.0 (2026-07-16, #42)**: Phase 5 idea 4
  shipped as the second LLM-free slice. New `src/algorithms/population.rs`
  (always compiled, ZERO new deps): a deterministic `SwarmAgentic`-style search
  over **coalition structures** (set-partitions) maximising `Σ over blocks of
  ValueCalculator(block)`. AIPA integer-partition shapes seed a diverse
  population; a single-`SplitMix64` PSO (per-agent global-best/personal-best
  pulls + random mutation, seeded ⇒ pure function of the seed) evolves it.
  `search()` is **pure + sync** (returns `SearchOutcome { best, lineage,
  iterations_run }`, `lineage` = the strictly-improving global-best chain);
  `record_trajectory()` is a **separate async** step that writes the gbest
  lineage into a `CoalitionManager` as successive form/dissolve epochs, replayable
  via `TemporalQueries` — the #42 acceptance gate (`tests/population_test.rs`
  reconstructs the final structure and asserts set-equality). Decisions A/A/A:
  shape-anchored seeding, gbest-lineage recording, pure/async split.
  **Gotcha 21**: the built-in calculators are degenerate for *structure* search
  (Additive is CONSTANT across partitions; Synergistic/Multiplicative favour
  all-singletons) — the search only does real work with an interior-optimum value
  model (coverage-style, or the magnitude/EFE/feedback arms); the example uses a
  `TaskCoverage` calculator to show it. **Suites: 98 default / 129 decision / 120
  magnitude / 151 decision,magnitude / 118 persistence / 141 persistence,magnitude**
  (+10 everywhere: 5 lib unit + 4 integration + 1 module doctest). Independent
  review 0 findings ≥80; two sub-bar hardenings applied (empty-block filter in
  `blocks()`; `record_trajectory` length precondition).
- **Feedback-weighted ValueCalculator — v0.12.0 (2026-07-16, #41)**: Phase 5
  idea 3 shipped as an LLM-free slice (promoted by the 2026-07-15 de-gate).
  New `src/algorithms/feedback.rs` (always compiled, zero new deps):
  `FeedbackCalculator<C: ValueCalculator>` wraps ANY base calculator with two
  SwarmAgentic velocity-coefficient analogues — `history_weight` (≈ c_p,
  personal-best; rewards recorded membership episodes) and `failure_weight`
  (≈ c_f, repulsion; penalises outcomes strictly `<` a threshold), each scaled
  by `HISTORY_UNIT`/`FAILURE_UNIT` = 25.0. Signals live in a shared
  `FeedbackStore` (`Arc<RwLock<counters>>`; `Clone` SHARES — K6 cache
  precedent); `record_outcome` closes the loop with no LLM round-trips and
  ignores+warns non-finite values. Plumbing: `agent_coalition_history`
  promoted `pub` (the Phase 5 anchor activating) +
  `CoalitionManager::seed_feedback_history` seeds history from the event log
  (seed-once contract; failures aren't seedable — the log has no outcomes).
  Under `ThresholdPolicy` the join marginal decomposes exactly as
  `base_marginal + hw·25·history(x) − fw·25·failures(x)` (members' counters
  cancel). See gotcha 19. 5-panel review applied (2 important findings →
  documented contracts). 0.12.0 also carries the earlier-uncut #43 Part 1/2
  work (aif pin `v0.9.0`, `AifMmDecisionPolicy` mm arm + K4-v3 FALSIFIED
  report — see §Phase 6). **Suites: 88 default / 119 decision / 110 magnitude
  / 141 decision,magnitude / 108 persistence / 131 persistence,magnitude**
  (+12 everywhere vs the post-#43 baselines: 9 unit + 3 integration).
  Payoff: a feedback-aware arm for a K4 rematch — REALISED as
  [#46](https://github.com/sustia-llc/koalisi/issues/46), **FALSIFIED (feedback)
  2026-07-16** (see Phase 5 idea 3 + gotcha 20). `ThresholdPolicy<FeedbackCalculator<C>>`
  drops into the battery via a per-seed `Arm { policy, store }` factory; the loop
  is closed with `record_outcome` per task after the leave sweep, store reset per
  seed (feedback is decision-CHANGING, unlike the magnitude evaluator cache).
- **Forex domain REMOVED — v0.11.0 (2026-07-14, #37)**: the
  de-financialisation pass completed. koalisi is now a purely domain-agnostic
  coalition runtime. Deleted `src/market.rs` and the arbitrage swarm
  (`subsystems/{coordinator,sink,swarm,monitor}.rs`) + the
  `historical_bootstrap` / `live_pubsub` / `triangular_arbitrage` /
  `hot_path_bench` examples + `tests/integration_test.rs`. **The `remote`
  feature went too** (the libp2p alert gateway `subsystems/distributed.rs` +
  `distributed_alert_consumer` + `remote_integration` + the `libp2p` dep): it
  subscribed to the deleted swarm's `alert_bus` and published
  `ArbitrageOpportunity`; reframing needs a coalition-event broadcast surface
  that doesn't exist yet, so it was **deferred to #38** (domain-neutral remote
  coalition-event gateway), not reworked. KEPT + unchanged: all of
  core/topology/algorithms/decision/ingest/llm/persistence and
  `subsystems::coalition_actor::CoalitionService` (the seam) + `durable`.
  NEW/rewritten: `examples/synthetic_ingestion.rs` is the flagship demo
  (synthetic sources → `SampleMonitor`s → coalition formation via
  `CoalitionService`); `src/main.rs` is a domain-neutral `CoalitionRuntime`
  daemon (seed coalition + bounded policy-gated join loop);
  `examples/supervised_swarm.rs` → `examples/supervised_monitor.rs`
  (`spawn_supervised` over a `SampleMonitor<SensorEvent>`). Gotchas 1/8/13 now
  obsolete (swarm/remote-specific). **Suites: 76 default / 95 decision / 98
  magnitude / 117 decision,magnitude / 96 persistence / 119
  persistence,magnitude** (dropped the forex `integration_test` + `market`/
  `monitor` unit tests; added 2 `CoalitionRuntime` shutdown tests from the
  code-review pass). Verified: `cargo test` green, default clippy
  `--all-targets` clean, all four features compile, both examples run.
- **Databento (DBN) adapter REMOVED — v0.10.0 (2026-07-14)**: koalisi is a
  domain-agnostic coalition runtime; market-data decode now lives entirely in
  the sibling `biome` project (its own OHLCV `load_prices_dbn`, a different
  shape than koalisi's old MBP-1 → `Tick` arb pump — nothing was ported, this
  is a pure deletion). Deleted `subsystems/databento.rs`, the two
  `databento_*` examples, `tests/databento_integration.rs`; dropped the
  `databento` feature and its `dbn` + `time` deps (`time` was databento-only).
  The domain-neutral `ingest::{Pacing, PumpStats, DataSource, pump_source}`
  layer (#8) is untouched (it was already the generalisation; only lost the
  databento name-drops). koalisi issues #22 (LiveClient) / #23 (synthetic DBN
  arb) closed as out-of-scope → biome owns that. **First step of a broader
  de-financialisation pass** — forex removal followed in v0.11.0 (see the top
  entry). Gotchas 3/5 now obsolete. (At v0.10.0: 87 default, forex still
  present.)
- **P7.2 topology projection + replay (#30) — v0.9.0**: the event log now
  persists and replays. Always-compiled optional **event tap** on
  `TemporalHypergraph` (`with_event_tap`; try_send drop-with-warn; `Clone`
  shares it; taps UNDER the events write guard ⇒ tap order == log order
  under concurrency); `WireTopologyEvent<VW,HW>` serde mirror (13 variants,
  raw u64 fields, `WIRE_TOPOLOGY_SCHEMA_VERSION`, inherent
  `from_event`/`try_into_event`, identity projection `VW = V`);
  `spawn_topology_forwarder` (tap → CBOR payload → P7.1 store writer;
  lossless drop-to-drain vs prompt token-cancel shutdown — gotcha 18);
  `replay_into_event_log(store, from)` rebuilds a fresh `EventLog` all
  existing queries run on unchanged. **The pre-registered #18 parity gate
  held**: `magnitude_history` live vs replayed exactly equal
  (`tests/replay_parity.rs`). Suites 87 / 107 persistence / 130
  persistence,magnitude / 128 (frozen arms untouched).
- **P7.1 persistence core (#29) — v0.8.0**: new `src/persistence/` behind
  feature `persistence` (deps `ciborium` + `sha2`, gated; default build
  unchanged). The signed-off §6 `EventStore` trait
  (`append`/`read_from`/`head`/`verify`) over six independently hash-chained
  streams; `FileEventStore` — CBOR frames via a private `FrameV1` mirror
  (public envelope types serde-free), per-stream segment dirs
  (`{first_seq:020}.seg`), SHA-256 over exact stored frame bytes (length
  prefix excluded), rotation with cross-segment chain continuity, torn-tail
  truncation at the last segment only, tail re-verification on open,
  **wedge-on-write-failure** (`StreamWedged`; reopen recovers), bounded
  random-access reads; `spawn_store_writer` (spawn_blocking appends, biased
  cancel, drain-on-cancel). `Payload::Sealed` schema-only until P7.3;
  Lineage reserved (#20). Writer seam is **at-most-once** from the tee
  (K3's cursor-replay has no analogue here yet). See gotcha 17.
- **Phase 7 persistence DESIGN (#21) — v0.7.0**: the RE-PLAN deliverable is
  `.claude/docs/phase7-persistence-design.md` (design only; NO EventStore code ships
  in 0.7.0). Layered: portable CBOR append-only hash-chained log = source of
  truth, K3 `durable` bus demoted to optional rebuildable projection.
  Envelope records (`Plain | Sealed` payloads, `parents: Vec<EventRef>` DAG)
  over six independently chained streams (Topology/Decisions/Beliefs/
  Lineage-reserved/Registry/Provenance); crypto-deletion = per-subject KEK
  destruction, keystore OUTSIDE the log; revocation = appended Registry
  events; bilateral manifest-gated federation (NOT the buses/gateway); FAIR
  provenance stream. Implementation phased **P7.1–P7.5 as follow-up issues**
  (doc §16); open calls in §17 (KEK granularity needs tauhokohoko input).
  Satisfies the #21 payload [R1]–[R9]; pin-conformance-reviewed.
- **Magnitude trajectory (#18) — v0.7.0**: `TemporalAnalytics::
  magnitude_history` + `MagnitudePoint` (feature `magnitude`) — change-driven
  replay of coalition membership + member weights over the event log,
  pinned-t=1 fresh `coalition_value` per sample point (CoalitionEvaluator
  deliberately NOT used — see gotcha 16), one read-guard pass + one
  tokio_rayon offload. `relevant_masks`/`magnitude_or_zero` promoted
  `pub(crate)` (visibility+docs only; decision arms untouched). 6 seeded
  tests in `tests/magnitude_trajectory.rs`. Independent of the EventStore
  build-out; P7.2's parity gate replays the log and must reproduce this
  series.
- **Domain-neutral ingestion (K5, issue #8)**: new `src/ingest/` (always
  compiled, zero new deps) — `Sample` trait (`Key`/`View`/`timestamp_ms`),
  generic `SampleMonitor<S>` (the MarketMonitor logic, verbatim-generic; K3
  contracts unchanged), `DataSource` + domain-neutral `Pacing` +
  `pump_source`/`spawn_source_pump` (`PumpStats { fed, dropped }`), and two
  seeded fixture sources: NEST-shaped `MultiResolutionSource` (per-series
  `step_ms`, global timestamp merge) and tauhokohoko-shaped
  `SensorEventSource` (changepoint mean shift, SPRT-suitable). Forex is now
  the instantiation: `MarketMonitor = SampleMonitor<Tick>`,
  `TickUpdate = SampleUpdate<Tick>` (**breaking**: fields `pair` → `key`,
  `quote` → `view`). Acceptance: `tests/ingestion_integration.rs` +
  `examples/synthetic_ingestion.rs` run coalition formation on synthetic
  non-financial data. (A feature-gated databento adapter also consumed
  `ingest::Pacing` at K5; removed in v0.10.0 — see the top Done entry.)
- **Evaluator hot path (K6, issue #14) — magnitude arm on
  `CoalitionEvaluator`**: dep `catgraph-magnitude` bumped to `v0.2.0`
  (catgraph#31); `MagnitudePolicy`/`MagnitudeValueCalculator` hold a
  membership-keyed evaluator cache (`Arc<Mutex<…>>`, keyed
  `(required, member masks)`, `REGISTRY_CAP = 256` candidate registry
  retained across rebuilds). Decisions bit-frozen via the knife-edge fresh
  fallback (`KNIFE_EDGE_REL_BAND = 1e-6`); K4 re-run
  (`docs/ab-report-K4-catgraph-evaluator.md`) seed-for-seed identical on
  quality columns, latency 3.915 → 3.658 µs — Path A missed, dual verdict
  unchanged (`FALSIFIED (latency)` / `VALIDATED (B)`), per-decision profile
  committed as the catgraph#33 evidence. Both types lost `Copy` (use `new`).
  See §"Worth flagging" entry 15 and §K6 below.
- **Messaging swap (K3, issue #6) — kameo GONE**: the runtime layer is pure
  `tokio::sync` (hybrid — hot seams never touch a DB). Workers
  (`MarketMonitor`/`ArbitrageCoordinator`/`AlertSink`) are tasks over mpsc
  command enums with oneshot-correlated replies + `*Handle` wrappers;
  `tick_bus`/`alert_bus` are `broadcast` (cap 1024, `Lagged` skips overflow);
  `Ping`/`GetQuotes`/`GetAlerts` drain-then-reply as deterministic flush
  barriers. `CoalitionActor` → `CoalitionService` (same file). Thin restart
  layer `core::supervision::spawn_supervised` (factory rebuild on panic,
  sliding-window restart budget). Remote gateway ported to raw libp2p
  `request-response` (`/koalisi/alerts/1`, CBOR; `RemoteAlertClient`; no
  `init_global`). NEW `durable` feature (off by default):
  `surrealdb-live-message` v0.2.0 two-tier bus; `CoalitionService` decision
  tap (`DecisionRecord`, feature-independent) → `subsystems::durable`
  forwarder → restart-durable decision log; container-backed restart test.
  Bench: every hot-path metric improved (`.claude/docs/k3-hot-path-bench.md`,
  alert RTT median 22.5 → 9.0 µs, throughput +56%). See §"Worth flagging"
  entries 13–14; gotchas 1/2/6/9/10 are obsolete.
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
  (git tag `v0.1.1` then; HTTPS URL since the 2026-07-27 release sweep —
  catgraph is public now; pinned `t = 1`). Capabilities map to directed substitutability couplings
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
  `ValueCalculator` trait + 4 base calculators (Additive, Synergistic,
  Multiplicative, Weighted) + the `FeedbackCalculator<C>` feedback-weighting
  wrapper over a shared `FeedbackStore` (#41), `DCVCDistributor`, AIPA
  partition search
- **Runtime layer** (domain-neutral since v0.11.0 — the forex swarm was
  removed, #37):
  - **`CoalitionService`** (`subsystems::coalition_actor`) — the policy-gated
    coalition-membership seam: a tokio task over mpsc/oneshot command handles
    (`CoalitionServiceHandle` with `join`/`leave`/`members`) that consults a
    `Box<dyn CoalitionDecisionPolicy>` + `DecisionContext` before mutating the
    `CoalitionManager` hypergraph. Optional decision tap → `DecisionRecord`
    (feeds the `durable` forwarder). This is the runtime demonstration now.
  - **Task-restart layer** (`core::supervision::spawn_supervised`) — factory
    rebuild on panic, sliding-window restart budget.
  - **`durable`** (feature, off by default) — `DecisionRecord` tap →
    `subsystems::durable` forwarder → restart-durable SurrealDB decision log.
  - **Examples (domain-neutral)**:
    - `synthetic_ingestion` — **flagship**: two synthetic sources
      (NEST-shaped `MultiResolutionSource`, tauhokohoko-shaped
      `SensorEventSource`) → generic `SampleMonitor`s → coalition formation
      over the ingested sensor agents via `CoalitionService` (default features).
    - `supervised_monitor` — `spawn_supervised` restart demo over a
      `SampleMonitor<SensorEvent>` (panic → factory rebuild → liveness proof).
    - `topology_coalition`, `algorithm_values` — layer demos.
    - `strategy_comparison` *(decision,magnitude)*, `durable_decisions`
      *(durable)*.
  - `src/main.rs` — a domain-neutral reference daemon: `CoalitionRuntime` +
    seed coalition + bounded policy-gated join loop through `CoalitionService`,
    ctrl-c → three-step shutdown.
- **Tests passing**:
  | Suite | Tests | Command |
  |---|---|---|
  | Default | 103 | `cargo test` |
  | `--features decision` | 159 | `cargo test --features decision` |
  | `--features magnitude` | 125 | `cargo test --features magnitude` |
  | `--features decision,magnitude` | 181 | `cargo test --features decision,magnitude` |
  | `--features persistence` | 123 | `cargo test --features persistence` |
  | `--features persistence,magnitude` | 146 (incl. the #18/#30 replay parity gate) | `cargo test --features persistence,magnitude` |
  | `--features durable` | +1 container-backed restart test; needs Docker | `cargo test --features durable` |
  | All examples | exit 0 | see Reproducers below |

### File inventory

```
koalisi/
├── Cargo.toml                              git tag deps: catgraph-applied + catgraph-magnitude v0.6.0 in lockstep (one checkout — K6); aif, surrealdb-live-message (optional); no path deps since K3; MSRV 1.93
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
│   │   └── magnitude_policy.rs             MagnitudePolicy + MagnitudeValueCalculator + CouplingModel + CoalitionEvaluator cache (K6) (feature `magnitude`); relevant_masks/magnitude_or_zero pub(crate) for #18
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
│       ├── coalition_actor.rs              CoalitionService + handle (policy-gated membership seam, #1) + DecisionRecord tap (K3) — THE runtime seam
│       ├── outcome.rs                      #55 (v0.14.0): TaskOutcome + OutcomeSink fan-out + emit_outcome tap + spawn_outcome_forwarder (always compiled; the L2 outcome seam)
│       └── durable.rs                      DecisionEvent + DurableDecisionBus + forwarder (feature `durable`, K3)
├── examples/
│   ├── topology_coalition.rs               coalition lifecycle + time-travel queries
│   ├── algorithm_values.rs                 value calculators + DCVC + AIPA
│   ├── synthetic_ingestion.rs              FLAGSHIP (v0.11.0): NEST + sensor fixtures → generic monitors → coalition formation via CoalitionService (default features)
│   ├── supervised_monitor.rs               spawn_supervised restart demo over SampleMonitor<SensorEvent> (v0.11.0; was supervised_swarm)
│   ├── population_search.rs                #42: TaskCoverage-driven structure search + record/replay (default features; was missing from this inventory — added v0.16.0)
│   ├── population_reliability.rs           #57 (v0.16.0): outcome stream → world-model snapshot → ReliabilityCoverage → search + replay (feature decision)
│   ├── strategy_comparison.rs              divergence demo + K4 A/B battery (features decision,magnitude)
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
│   └── ab-report-K4-routing-corrected.md   #63 run — FALSIFIED (block-routing), mechanism-scoped (window vs lattice); leg C DEGENERATE (4th gotcha-21 mechanism); leg L ordering 30/30 (gotcha 26)
└── tests/
    ├── topology_test.rs                    12 tests
    ├── algorithms_test.rs                  18 tests (incl. 3 feedback-loop/seeding tests, #41)
    ├── decision_integration.rs             4–6 tests (feature-dependent)
    ├── durable_integration.rs              1 container-backed restart test (feature `durable`)
    ├── ingestion_integration.rs            3 tests (K5: synthetic sources → monitors → coalition formation; default features)
    ├── magnitude_trajectory.rs             6 tests (#18: hand-computed trajectory semantics; feature `magnitude`)
    ├── persistence_integration.rs          7 tests (#29: roundtrip, rotation+reopen, tamper, torn tail, sealed opaque, writer drain, bounds; feature `persistence`)
    ├── topology_replay.rs                  3 tests (#30: 13-variant round-trip, reconstruction equality, schema/Sealed rejection; feature `persistence`)
    └── replay_parity.rs                    1 test (#30: magnitude_history live == replayed — THE parity gate; features `persistence,magnitude`)
```

## Worth flagging (gotchas)

These cost time during the build; future-me should not relearn them.

1. **~~kameo supervised actors keep the same `ActorId` across restart.~~ OBSOLETE since K3 (#6)** — kameo is gone; restarts are `core::supervision::spawn_supervised` factory rebuilds (no id identity at all). The demo moved from `supervised_swarm` to `examples/supervised_monitor.rs` (v0.11.0, over a `SampleMonitor<SensorEvent>`).

2. **~~`anyhow::Context` shadows `kameo::Context`.~~ OBSOLETE since K3 (#6)** — kameo is gone. Kept for history:
   - kameo's prelude exports `Context<Self, Reply>` (the actor handler parameter type).
   - anyhow exports a `Context` trait for `.context("…")?` on `Result`.
   - `use anyhow::{Context, ...}` + `use kameo::prelude::*` → compile error "expected a type, found a trait" on `_: &mut Context<Self, Self::Reply>`.
   - Fix: `use anyhow::Context as _;` brings the trait into scope for the extension method without binding the name.
   - (Was applied in the deleted `examples/databento_live_replay.rs`; watch for it whenever both crates appear in the same example.)

3. **~~Bundled DBN test fixture is futures, not forex.~~ OBSOLETE since v0.10.0** — the databento adapter was removed (moved to `biome`); no DBN fixture is referenced any more.

4. **~~Path dependencies on kameo.~~ OBSOLETE since K3 (#6)** — both kameo deps removed. Kept for history:
   - `Cargo.toml` references `../../agentics/kameo` and `../../agentics/kameo/actors`.
   - This breaks if either repo moves. When the upstream `kameo 0.20.0` stabilises on crates.io with the API we're using, switch to a version dep.

5. **~~DBN file discovery for examples.~~ OBSOLETE since v0.10.0** — the databento adapter and its `$DBN_TEST_PATH`/`databento-rs` fixture probing were removed (moved to `biome`).

6. **~~PubSub `Subscribe` requires the subscriber to be alive.~~ OBSOLETE since K3 (#6)** — buses were `tokio::sync::broadcast` (`subscribe()` synchronous, order-free); the whole forex swarm carrying those buses was removed in v0.11.0 (#37).

7. **Cargo target dir + timeout convention (project-wide).**
   - We use `--manifest-path Cargo.toml --target-dir /tmp/koalisi-target` to avoid contention with the IDE's own `cargo check`. Run from inside the `koalisi` worktree.
   - Wrap with `timeout 30s` (or 60s, 120s as appropriate) so a hang in a freshly-built binary is killed cleanly, not just the shell wrapper.
   - Pattern: `timeout 30s cargo run --manifest-path Cargo.toml --target-dir /tmp/… --example foo 2>/dev/null ; echo "exit=$?"`. Exit 124 = unix `timeout` fired.

8. **~~libp2p remote RPC: hybrid, NOT hot-path.~~ OBSOLETE since v0.11.0 (#37)** — the `remote` feature (raw libp2p `request-response` gateway on `subsystems/distributed.rs`) was deleted with the forex swarm it subscribed to. Re-introduction as a domain-neutral coalition-event gateway is tracked in [#38](https://github.com/sustia-llc/koalisi/issues/38); the "publish-to-outside-world boundary, never the hot path" design rationale is recoverable from the git history at `v0.10.0` and #38.

9. **~~`kameo::remote::Behaviour::init_global()` is process-wide.~~ OBSOLETE since K3 (#6)** — no global init; producer + client coexist in one process (the remote test does exactly that). Kept for history:
   - Called once inside `enable_remote_alerts`. Calling it twice in the same process (e.g., from two integration tests in a single binary) will conflict.
   - For now: one `remote_integration` test only. Future remote tests need to share the libp2p swarm, OR use `serial_test` + tear-down hooks.

10. **~~`ActorRef::register` / registry names~~ OBSOLETE since K3 (#6)** — no registry; the rr protocol name is the service identity. Kept for history:
    - Signature is `register(impl Into<Arc<str>>)`. Passing `&String` doesn't work — `&String` does not impl `Into<Arc<str>>`. Pass `&str` (via `.as_str()` or `&literal`).
    - Without `remote`: sync, just returns `Result<(), RegistryError>` (no `.await`).
    - With `remote`: returns a future that resolves once libp2p propagates the registration.

11. **~~libp2p `#[derive(NetworkBehaviour)]` requires the `macros` feature.~~ OBSOLETE since v0.11.0 (#37)** — the `libp2p` dep left with the `remote` gateway. Relevant again only when #38 re-adds a gateway.

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

20. **Feedback-arm K4 battery (#46) — why it FALSIFIED, rely on this.**
    - **Balanced weights cancel in the full-join regime.** With
      `ThresholdPolicy<Synergistic>` at threshold 0 every marginal is positive, so
      every agent joins and none leaves (churn 0). Each member then accrues history
      and failures together (`history ≈ failures`), and `hw=fw` makes the feedback
      term `hw·25·history − fw·25·failures ≈ 0` — `fb` never diverges from `thr`
      (0/30 seeds). Feedback can only bite by declining an agent, which needs the
      *failure* term to dominate (`fw > hw`): the E1 sweep's failure-heavy cells
      move `PRIMARY_B` (best 0.0730) but none reach magnitude (0.2818). Magnitude
      wins by *selectivity* (small high-`cov_eff` coalitions), which a full-join base
      can't produce. Lesson for any rematch: use a **selective base** (positive
      `join_threshold`, #48) and/or a **failure-weighted** point (#49) — and
      register it fresh. (#48 did exactly this — `PARTIAL (mechanism only)`: the
      selective base makes feedback bite, but magnitude still wins; see gotcha 22.)
    - **Harness contract** (`examples/strategy_comparison.rs` Part 3): `run_instance`
      takes `Scope` + `Option<&FeedbackStore>`; write-back is `record_outcome` ONCE
      per task AFTER the leave sweep (within-task decisions see a constant store);
      the store is FRESH PER SEED (via the `Arm { policy, store }` factory) so the 30
      instances stay independent. Scope A shares a byte-identical prefix with the
      frozen Part 2 (regression gate: Scope-A `mag` == committed baseline); Scope B
      appends reliability + `perf[t][i]` draws off the SAME stream, after the prefix.

21. **Population search (#42) — rely on these.**
    - **Built-in calculators are degenerate for STRUCTURE search.** `search`
      maximises `Σ over blocks of ValueCalculator(block)`, but summed over a
      set-partition the `AdditiveCalculator` total is CONSTANT (its size /
      capability / trust terms sum to the same value for every partition), and
      `Synergistic`/`Multiplicative` are split-favouring (all-singletons optimal).
      So with those the answer is degenerate and the lineage is one epoch — NOT a
      bug. The search only does real work for a value model with an interior
      optimum: coverage-style (see `examples/population_search.rs`'s `TaskCoverage`)
      or the magnitude / EFE / #41-feedback arms. Documented in the module docs;
      don't "fix" it by changing the built-ins.
    - **Determinism**: one `SplitMix64` seeded from `PopulationConfig::seed`;
      `canonicalize` assigns block ids by first-occurrence traversal (NOT HashMap
      iteration), so `search` is a pure function of the seed. `blocks()` filters
      empties, so it returns a true set-partition even for a hand-built
      non-canonical `assignment` (the field is `pub`).
    - **`record_trajectory` is fresh-manager-only** and adds `agents`
      unconditionally; every `lineage` structure must be over the same `agents`
      (`assignment.len() == agents.len()`) or `vmap[i]` indexes out of bounds. It
      records the gbest lineage as form/dissolve epochs; the final epoch is live
      and replays via `TemporalQueries::coalition_members_at` (the #42 parity gate).

22. **Selective-base feedback arm (#48) — rely on these.** Extends gotcha 20.
    - **A positive `join_threshold` makes feedback bite, but doesn't beat magnitude.**
      On Scope B with base `ThresholdPolicy(Synergistic, join=100, 0)` and
      `FeedbackCalculator(hw=0, fw=1)`, `fb-selective` (0.0512) beats `thr-selective`
      (0.0301) on 21/30 seeds (H2 PASS) — so magnitude's edge is **NOT** pure
      selectivity; failure-weighting adds a genuine reliability signal. But mag
      (0.2818) is ~5.5× ahead (H1 FAIL) ⇒ `PARTIAL (mechanism only)`. Don't read the
      H2 pass as "feedback wins".
    - **The feedback increment is NON-MONOTONE in `join_threshold`.** E1 sweep: fb > thr
      at `join ∈ {50,75,100}` but pure selectivity **overtakes** at `{125,150}`
      (thr 0.0906/0.0937 vs fb 0.0451/0.0413). A tight base already forms small
      coalitions, and the 0/1 `fw=1` penalty then evicts merely-*unlucky* good agents (a
      reliable agent that missed a covered task still accrues a failure). So a *tighter*
      base is not *better* for feedback — it helps in a middle band. The registered
      `join=100` was fixed before the sweep (not threshold-shopped).
    - **Part 4 is additive; Part 3 (#46) is the byte-identical regression gate.**
      `make_fb`/`run_feedback_scope` gained a leading `join_threshold` param; Part 3
      passes its original `(0.0, 0.5, 0.5)` so its output — and the committed
      `docs/ab-report-feedback-arm-k4.md` — is unchanged.

23. **E1 outcome-signal fidelity (#54 Part 4e, unregistered) — rely on these.**
    - **The per-bit oracle signal is NOT load-bearing.** Feeding the registered
      `aif-e1` arm the DEGRADED signal — whole-coalition `success` smeared across
      required bits, `observe_outcome(req, &[success; 8])` — retains the effect on
      30..60: degraded 0.4381 vs oracle 0.4406 median (bit-identical 17/30 seeds),
      scalar 0.1267, churn 143 vs 136. A runtime therefore needs only ONE
      domain-emitted task-completion event `(required, members, success)` — the
      same information contract as #41's `record_outcome`. Full analysis:
      `docs/per-bit-outcome-plumbing-design.md` (fidelity ladder; the event is NOT
      a `TemporalEvent` — durable home is the P7.4 #32 streams).
    - **`run_seed_b`'s hook is 3-arg since Part 4e**
      (`FnMut(u32, &[bool; 8], bool)` — third = whole-coalition `success`,
      which is NOT derivable from `per_bit` alone). Behavior of all frozen
      parts unchanged (byte-identity reproduced 2026-07-18).
    - **mag churn on 30..60 is 8.00 median** (Part 4e table) — "historically
      near-zero" was 0..30 folklore; the real out-of-sample number is small but
      nonzero. e1 churn correlates weakly POSITIVELY with its quality (+0.22)
      and its margin over mag (+0.17): e1 wins *with* thrash, not despite it —
      churn mitigation (Step 3) is plausible but not free.
    - **Cheap reliability-gating on mag is WORSE than bare mag (Part 4g).**
      `RelFilteredMag` (mag structure + #41 `FeedbackStore` veto on the
      whole-task L2 signal): every grid cell < 0.2720 (best 0.2582; eviction
      cells collapse to 0.03–0.12). The veto is an ABSORBING exclusion — smeared
      failure zeroes a good agent's r̂ and a vetoed agent never re-earns
      evidence. Reliability is only usable via a learned model with an
      epistemic term (e1's Dirichlet+novelty; cf. v5 X1). Also rejected on
      theory: reliability-weighted couplings RAISE an unreliable agent's Möbius
      weight (magnitude = diversity, not dependability) — don't retry either
      form. `run_seed_b`'s hook is 4-arg since Part 4g (adds final `members`).
    - **Score-space margins/hysteresis on the e1 arm are INERT (Part 4f).** The
      fixed-γ=16 query posteriors saturate at ±0.5 (all join/leave score
      quantiles from p25 up are exactly 0.5000), so `p > 0.5 + δ` thresholds
      and leave hysteresis in score units change NOTHING for any δ, h < 0.5
      (frontier flat at 0.4406/136 oracle, 0.4381/143–142 degraded). Don't
      re-try this lever; churn work must target membership STATE (dwell-time /
      cooldown / rejoin-lockout), which is a new arm design + registration.

24. **`ReliabilityCoverage` contracts (#57, v0.16.0) — rely on these.**
    All three were measured by the semantics review lens, not inferred.
    - **The belief read is RECENCY-dominated, not an aggregate.**
      `beliefs[b][0]` = the smoothed posterior at the last node of the 2-step
      MMP window: the same 19-success stream reads 0.88 or 0.28 depending on
      where the single failure sits, and one truncated task moves the number
      3.1×. Cross-bit ORDERING is the robust signal; never present the value
      as a success rate (the example prints last-3-tasks context, not
      success counts, for exactly this reason).
    - **Reliability RESCALES the coverage optimum; it does not route around
      weak bits.** For `|required| ≤ 6`, full-coverage weight per unit
      reliability (`100/|required| ≥ 16.7`) > partial weight (15), so
      skipping a bit never pays at equal member count — the example's best
      structure is IDENTICAL to unweighted `TaskCoverage`'s (fitness rescaled
      152.0 → −2.50). Re-ranking happens only via the number of full-coverage
      blocks + which bits partial blocks cover. (At `|required| ≥ 7` skipping
      can win, but that's a TaskCoverage-coefficient artifact present at
      r ≡ 1 too.) Member cost is CONSTANT (−8·N) across partitions of a fixed
      pool — it never affects `search()`'s argmax.
    - **The 0.5 unknown-bit prior is OPTIMISTIC**: learned posteriors in the
      example read 0.07–0.28, so an unobserved bit outranks every measured
      one. Also: the belief read is identical under `query_dynamics`
      true/false (dynamics shape only the query POMDP) — the example pins
      `false` so its "E1 (K4-v5)" label is honest; `default()` is the
      FALSIFIED v4 config, don't relabel it v5.

25. **Battery-v2 contracts (#61, v0.18.0) — rely on these.**
    - **The e1 join rail is margin-proof.** Under MeanField queries the
      join-score quantiles sit at exactly +0.5 (p = 1.0, certainty) in EVERY
      measured (γ, regime) cell — γ ∈ {1, 4, 16} × both draws. γ de-saturates
      only the LEAVE stream. Any future churn lever in score space must act
      on leaves (hysteresis h) or membership state (#56 lineage); join
      margins are a dead lever twice over (Part 4f + Part 5a).
    - **Never write a "skips a bit" predicate over `search()` output.**
      `search()` returns a partition of the ENTIRE pool, so the union over
      blocks == the pool union: "no block covers b" ⟺ "no pool agent has b",
      independent of the calculator. Likewise the per-member cost sums to a
      constant across partitions of a fixed pool — member-savings arguments
      cannot express at partition level. Routing questions must be asked at
      BLOCK level (which bits do value-bearing blocks cover) — see #63.
    - **`TaskCoverageV2`-style reliability weighting cannot flip full→skip
      at equal size**: full pays 100/m per unit r per bit vs partial's 80/m —
      full wins per covered bit at ANY reliability. Reliability re-ranks only
      through which bits partial blocks cover. A routing-capable model needs
      a reliability-INsensitive full bonus or a flip via member savings
      (near-zero r), per the #63 requirements.
    - **Random pools don't cover big requirements**: n ∈ 8..=16 × caps 1..=4
      bits misses a 7–8-bit requirement ~1/6 of the time — any battery whose
      legs assume pool coverage must GUARANTEE it in the draw.
    - `query_gamma` is inert under `query_dynamics: true` (PrecisionDynamics
      owns γ); `Some(16.0)` ≡ `None` is asserted — don't "simplify" the
      Option away.
    - **Part 5c extensions (v0.19.0, all exploratory/context).** The join
      rail is margin-proof at 12 bits too (`n_bits = 12`, γ = 4: every join
      quantile exactly +0.5) — universe width doesn't free it. Leave-side
      hysteresis h IS live (the one stream γ frees) but expensive: h = 0.30
      cut churn to 0.85× at −24% `PRIMARY_B` — don't re-try it as a cheap
      fix; state levers remain the churn axis. The expected-outcome value
      model (per-block REAL as fitness) is gotcha-21 degenerate by a THIRD
      mechanism — the per-block partial term double-counts a bit covered by
      two blocks, so splitting weakly dominates; don't use per-block
      expected-payoff fitness for structure search without an
      interior-optimum term. Learned reliability posteriors rank a planted
      weak bit correctly 30/30 (levels wildly uncalibrated per gotcha 24) —
      the #63 corrected routing test can use the learned input.
    - `PersistentAifConfig::n_bits` (v0.19.0): identity default 8 is the
      registered arm bit-for-bit (asserted); values clamp to 1..=16 with a
      warn, `observe_outcome` takes `&[bool]` and skips on width mismatch.
      The 12-bit query joint is `2^(|required|+1)` ≤ 8192 ⇒ ~2 ms/decision
      — budget batteries accordingly.

26. **Corrected routing test (#63, Part 6) — rely on these.**
    - **Block-level routing needs window > lattice.** The b\* planting opens
      a value window of width `100·Δr/m` at the full-coverage block; the
      competing blocks are leftover singletons on a value lattice of spacing
      `w(m)·r_others`. At the registered planting (0.02/0.35) the window
      (4.71/4.13) barely exceeds the spacing (4.00/3.50) ⇒ at most one
      leftover capability count can fire per cover size, and the skip leg
      cannot reach a 60% bar on any draw distribution like this one. Any
      future routing design must widen the window-to-lattice ratio BY
      DESIGN — new registration required.
    - **Per-block multiplicative (success-probability) full bonuses are
      gotcha-21 degenerate for structure search** — the FOURTH mechanism:
      `100·Π r` collapses at any materially-sub-1 planting and stops paying
      for the per-block partial-term overlap a merge destroys
      (all-singletons ties-or-beats 30/30 at 0.15/0.98). Do NOT describe
      `TaskCoverageV2P` as "reliability-INsensitive" (the prereg's leg-C
      interpretation clause mischaracterizes it — report ledger item 6); its
      bonus is maximally reliability-sensitive and routes by collapse.
    - **Uniform-rescale counterfactuals do not hold the partition fixed.**
      Exact arithmetic says a uniform reliability is an increasing affine
      map of the unweighted total (same argmax), but the fitness landscape
      is saturated with exact ties and independently-rounded scaled sums
      split them at ±1 ulp — PSO trajectories diverged on 27/30 seeds.
      Treat same-seed counterfactual contrasts as intent-to-treat, and
      never assert partition coincidence (count it).
    - The min-multiplicity-cover bound `Σ|caps∩required| < 1.25·m` is the
      feasibility condition for a full-coverage TOP block (strictly stronger
      than pool coverage, which the corrected draw guarantees); print it
      per-seed so sanity failures attribute to draw-vs-search immediately
      (seed 196 = draw-infeasible, seed 188 = search miss).
    - Learned posteriors stay ordering-robust at near-floor reliabilities
      (30/30 at 0.02-vs-0.35, both bits failing most tasks) with levels as
      uncalibrated as ever (spread median 0.12) — gotcha 24's split holds at
      its second measured point.

## Reproducers

All assume `cwd = koalisi/`.

```sh
# === default features (103 tests) ===
timeout 60s  cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target
timeout 30s  cargo run  --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --example topology_coalition
timeout 30s  cargo run  --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --example algorithm_values
timeout 30s  cargo run  --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --example synthetic_ingestion   # FLAGSHIP
timeout 30s  cargo run  --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --example supervised_monitor
timeout 30s  cargo run  --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --example population_search   # P5.2 (#42)

# === decision-layer feature combos (152 / 125 / 174 tests) ===
timeout 120s cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features decision
timeout 120s cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features magnitude
timeout 120s cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features decision,magnitude
timeout 120s cargo run --release --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features decision,magnitude --example strategy_comparison
timeout 60s  cargo run --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features decision --example population_reliability   # #57 (v0.16.0)

# === with persistence feature (P7.1 store + P7.2 replay, 123 tests) ===
timeout 120s cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features persistence
timeout 120s cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features persistence,magnitude   # 146, incl. the replay parity gate

# === with durable feature (needs Docker; container-backed restart test) ===
timeout 300s cargo test --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features durable
timeout 120s cargo run  --manifest-path Cargo.toml --target-dir /tmp/koalisi-target --features durable --example durable_decisions
```

## Next steps

> **GATE — RESOLVED 2026-07-03 (2 of 2 design inputs received).**
> Input #1 (2026-05-27): integrate SwarmAgentic-style optimisation as
> the new Phase 5 (was Persistence); reordering Phase 5 = SwarmAgentic,
> Phase 6 = Decision layer (moved up, since SHIPPED + K2/K4/K6), Phase 7
> = Persistence (moved last so the dynamics that *generate* the events
> settle before committing to a durable storage format).
> Input #2 (2026-07-03): the driver-derived design-goals payload from
> the tauhokohoko + NEST requirement surveys — recorded in full on
> [#20](https://github.com/sustia-llc/koalisi/issues/20) (Phase 5
> reframe: NEST-H4 slow-loop calibration copilot; koalisi-AIF ≠
> NEST-AIF pinned) and
> [#21](https://github.com/sustia-llc/koalisi/issues/21) (Phase 7
> requirements: append-only + crypto-deletion + bilateral federation +
> portable format + EffectLog-compatible traces + FAIR provenance).
> **Status: the Phase 7 RE-PLAN (#21) is DONE and CLOSED — design doc
> shipped v0.7.0 (2026-07-04, `.claude/docs/phase7-persistence-design.md`), signed
> off, implementation filed as #29–#33 (P7.1–P7.5). Phase 5 DE-GATED
> 2026-07-15 (owner decision: NEST is slow — first session was
> administrative — so Phase 5 no longer blocks on the Year-1 ownership
> assignment). LLM-free slices promoted to standalone issues:
> [#41](https://github.com/sustia-llc/koalisi/issues/41) (idea 3,
> feedback-weight ValueCalculator) and
> [#42](https://github.com/sustia-llc/koalisi/issues/42) (idea 4, AIPA
> population search). #20 keeps the LLM-dependent remainder
> (configurator + velocity-rewrite loop + transferability); the only
> still-NEST-dependent piece is the input-#2 NEST-H4 calibration-copilot
> deployment framing, which activates whenever ownership lands.**

### Phase 5: SwarmAgentic-style optimisation  *(DE-GATED 2026-07-15 — LLM-free slices: [#41](https://github.com/sustia-llc/koalisi/issues/41) idea 3 **DONE v0.12.0**, [#42](https://github.com/sustia-llc/koalisi/issues/42) idea 4 **DONE v0.13.0**; LLM meta-layer remainder tracked: [#20](https://github.com/sustia-llc/koalisi/issues/20))*

Lift the SwarmAgentic framework (Zhang et al., 2025 — see
`.claude/docs/SwarmAgentic-summary.md` for full paper digest) into koalisi as a
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

3. **`ValueCalculator` extension with feedback weights** (promoted →
   [#41](https://github.com/sustia-llc/koalisi/issues/41), **DONE v0.12.0
   2026-07-16** — shipped as the `FeedbackCalculator<C>` wrapper +
   shared `FeedbackStore`, see §Current state and gotcha 19)**.**
   **K4 rematch as an arm: [#46](https://github.com/sustia-llc/koalisi/issues/46)
   — FALSIFIED (feedback), 2026-07-16.** The feedback arm was wired into the
   battery (`examples/strategy_comparison.rs` Part 3, Scope A i.i.d. null + Scope
   B reliability-structured contest; pre-reg `docs/prereg-feedback-arm-k4.md`,
   report `docs/ab-report-feedback-arm-k4.md`). Registered `hw=fw=0.5` cancels in
   the full-join `ThresholdPolicy`-at-0 regime (`history≈failures`); the E1 sweep
   shows failure-dominant cells bite but never reach magnitude (mag 0.2818 vs best
   fb 0.0730). Example-only, no library/version change. **Rematch on a selective
   base: [#48](https://github.com/sustia-llc/koalisi/issues/48) — `PARTIAL
   (mechanism only)`, 2026-07-17** (`join_threshold=100`, `hw=0/fw=1`;
   `fb-selective` beats `thr-selective` 21/30 ⇒ feedback adds a real reliability
   signal, but neither closes the ~5.5× gap to magnitude; the increment is
   non-monotone in the threshold — see gotcha 22). #49 **absorbed** as the
   registered weighting; prereg/report `docs/{prereg,ab-report}-feedback-arm-k4-v2.md`.
   SwarmAgentic's
   three coefficients (`c_f` failure / `c_p` personal-best / `c_g`
   global-best) are direct analogues of `WeightedCalculator`'s
   `size`/`capability`/`trust`/`synergy` weights. Add `history_weight`
   (derived from `CoalitionManager::agent_coalition_history`) and
   `failure_weight` (derived from past low-value outcomes) so the
   value-calculation feedback loop closes inside Rust without LLM
   round-trips for every score.

4. **Population-based search atop AIPA** (promoted →
   [#42](https://github.com/sustia-llc/koalisi/issues/42), **DONE v0.13.0
   2026-07-16** — `src/algorithms/population.rs`, see §Current state + gotcha 21)**.**
   AIPA enumerates integer partitions deterministically; SwarmAgentic maintains a
   *population* of full system designs. Shipped: AIPA integer-partition shapes seed
   a diverse population, a deterministic (LLM-free) `SplitMix64` PSO swarm evolves
   the assignments, and the improving global-best lineage is recorded into a
   `TemporalHypergraph` (`record_trajectory`) so good lineages replay via
   `TemporalQueries`. Fitness = the existing `ValueCalculator` trait (any arm). The
   LLM-driven *language* velocity rewrites (per-particle trajectory recording,
   collaboration-policy evolution) remain #20's meta-layer.

5. **Cross-model transferability as a koalisi value-prop.** SwarmAgentic
   shows discovered systems transfer across LLMs. If the runtime layer
   (the `tokio::sync` task seams + `CoalitionService`) stays
   provider-agnostic, a
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

- **Dependency:** `aif = { git = "https://github.com/sustia-llc/tira", tag =
  "aif-v0.12.0", optional = true }` (shipped at `aif-v0.4.0`; → `v0.5.0` by follow-up #2;
  → `v0.9.0` by #43 Part 1, 2026-07-16 — decision suite passed unchanged;
  → `v0.11.0` by the #44 K4-v4 cycle; → `v0.12.0` by the 2026-08-01 pre-EQ4
  adoption slice, v0.20.0 — drift-free), behind `[features] decision = ["dep:aif"]`.
  Originally an SSH URL (tira was private; cargo's libgit2 HTTPS fetch can't
  authenticate) — re-pinned to HTTPS in the 2026-07-27 release sweep after
  tira went public. Feature-off builds compile **no `aif` and no
  `nalgebra`**. (`aif` uses `nalgebra` internally — NOT `ndarray`; the old "adds
  ndarray dependency" note was wrong. Since the K4-v3 `aif-mm` arm, koalisi carries a
  **direct optional `nalgebra` dep under `decision`** — the multimodal bridge
  constructs `GenerativeModel` matrices itself; version unified with aif's transitive
  0.35, so no extra compile unit. The *scalar* bridge boundary remains plain
  `u32`/`f64` via `competence_efe`.)
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
  via `POMDPAgent::new` directly (NOT via the since-removed `aif::CoalitionEvaluator`,
  whose `observation_probs` couldn't see members). Higher coverage ⇒ sharper `A` ⇒ lower
  `G` (verified monotone: `G(0)=1.204 > G(0.5)=0.710 > G(1)=0.215` — the engine's
  default-parameter anchors, pinned in aif's `test_competence_efe_regression_anchors`
  since 0.6.0; the `0.511/0.121/0.017` figures previously recorded here were stale
  v0.4.0-era measurements of the pre-bridge `efe_for_coverage`). Non-degeneracy is
  unit-tested: an agent covering a new required bit lowers `G` (joins); a redundant
  clone does not. `BridgeParams` (`max_precision` 0.95, `success_preference` 0.9,
  `alpha` 8.0) tunes the mapping.
- **K4-v3 rematch: `FALSIFIED (multimodality)`** (2026-07-16,
  `docs/ab-report-K4v3-multimodal-aif.md`; pre-registered in
  `docs/prereg-K4v3-multimodal-aif.md` before implementation). The registered
  multimodal arm (`AifMmDecisionPolicy`, one modality per required bit, binary union
  coverage) is **decision-equivalent to the scalar arm** — G is affine in the
  covered-bit count and margin-0 decisions see only sign(ΔG), so all 30 seeds match
  scalar seed-for-seed (theorem characterized by the committed
  `mm_and_scalar_agree_on_acts` test). The v2 magnitude-quality verdict stands.
  E3 resolved analytically (same theorem); E1/E2 (learning / stochastic-B +
  PrecisionDynamics) deferred — they need a *persistent-agent* design (fresh-POMDP-per-
  decision makes learning/β-persistence meaningless) and their own registration. E2 is
  the identified lever: a live info-gain term is non-monotone in per-bit structure.
- **K4-v4 persistent arm: `FALSIFIED (persistence)`** (2026-07-17,
  `docs/ab-report-K4-v4-persistent-aif.md`; pre-registered in
  `docs/prereg-K4-v4-persistent-aif.md` + Amendments 1–2, all posted to #44 before
  implementation/run; engine `aif-v0.11.0` — three tira releases cut for this arm:
  0.10.0 seed+B-novelty, 0.10.1 read accessors, 0.11.0 `initial_pa`/`initial_pb`
  count injection after the row-uniform-pA/learn_a write-back gap made structured
  query A's decision-dead). `PersistentAifArm` (`aif_persistent_policy.rs`): per-seed
  persistent 8-bit reliability world model + membership-factor query POMDPs (exact
  coverage-masked count injection, `PrecisionDynamics`, deterministic posterior
  decisions). Registered medians: pers 0.0326 vs scalar 0.1035 vs mag 0.2818 — ¬H2,
  though S1 act-divergence 30/30 proves the arm **genuinely escapes the v3
  equivalence theorem** (first arm to do so; the falsification is performance, not
  collapse). Ablations: E5 learning-off reproduces scalar exactly (theorem-recovery
  sanity); **E6 dynamics-off (learned precisions + fixed-γ MeanField queries) posts
  0.4042 > mag's 0.2818** (churn 210) — exploratory only, no verdict; an E1-only v5
  would need a fresh registration. The γ₀ = 1 dynamics flattening + novelty join-bias
  are the leading (uninstrumented) explanations for the registered arm's collapse.
  Magnitude's v2 quality verdict now stands against four successive challengers.
- **K4-v5 E1-only arm: `VALIDATED (gap closed)`** (2026-07-17,
  `docs/ab-report-K4-v5-e1-persistent-aif.md`; pre-registered in
  `docs/prereg-K4-v5-e1-persistent-aif.md` posted to #53 before the run). **The
  first arm in the K4 lineage (v1→v5) to beat magnitude on quality**, on
  **out-of-sample seeds 30..60**: `aif-e1` (the #44 `PersistentAifArm` at 943d139,
  zero code changes, in the v4 E6 configuration — learned per-bit precisions +
  fixed-γ = 16 MeanField queries, no `PrecisionDynamics`) posts median 0.4406 vs
  mag 0.2720 (1.62×) and scalar 0.1267; strictly superior to scalar 30/30; H1∧H2.
  X2 determinism gate reproduced the 0..30 E6 numbers exactly (0.4042/210.00,
  asserted). **X1: novelty-off collapses to 0.1308 ≈ scalar** — the winning
  mechanism is learned precisions + novelty-driven epistemic joining, neither
  suffices alone (ablation inference). Costs: churn 136 vs scalar 79.5 (mag's is
  historically near-zero — the tradeoff to address), latency ~63 µs/decision vs
  µs-scale stateless arms. The v2 magnitude verdict is not overturned (scoped to
  the arms then registered); arm choice is now a cost-quality tradeoff — tracked as
  a follow-up issue per the prereg's pre-committed interpretation.
- **aif 0.9.0 is released and pinned** (tag `aif-v0.9.0`, 2026-07-16; bump landed via
  [#43](https://github.com/sustia-llc/koalisi/issues/43) Part 1). **tira's
  canonical-AIF parity roadmap (#12–#16) is complete**: 0.6.0 generalized generative
  model (multi-factor/multi-modality/injectable B), 0.7.0 marginal message passing +
  surfaced F, 0.8.0 full Dirichlet learning (pA/pB/pD/pE, η/ω, novelty EFE term,
  Fa/Fb/Fd/Fe), 0.9.0 opt-in γ/β precision dynamics (Smith Table 2). Migration when
  bumping straight to 0.9.0: `ObsPrecisionParams` gained a `transition_noise` field (add
  `transition_noise: 0.0` or use `..Default::default()`; default preserves current
  values byte-for-byte); `CoalitionEvaluator`/`CapabilityProvider` are gone (koalisi
  never used them); the `competence_efe` anchors above are unchanged through 0.9.0
  (bit-identical defaults every release); `AgentParams` grew ten fields across
  0.7.0–0.9.0 — only relevant to the multimodal arm, use `..Default::default()`.
  Design facts shaping the K4 v3 rematch: (1) `transition_noise` makes the info-gain
  term live but **raises** net `G` across most of the competence range (pragmatic
  blurring dominates) — it is a modeling choice, not an exploration bonus, and
  competence monotonicity is preserved; (2) noise alone cannot make the AIF arm
  diversity-sensitive (competence stays a scalar) — the diversity-aware arm is a
  **multi-modality bridge**: one observation modality per required capability bit via
  `GenerativeModel`/`from_model`, per-bit coverage driving that modality's precision,
  so member-overlap structure enters `G` directly; (3) since 0.8.0/0.9.0 the multimodal
  arm can additionally learn its observation model online (`learn_a` + novelty term
  drives information-seeking toward uncertain members) and run dynamic policy precision
  (`PrecisionDynamics`, requires MMP + stochastic B) — both opt-in, both optional
  extensions to the v3 arm design, not prerequisites.
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
  (policy-gated, `where V: AgentCapabilities`) + `subsystems::coalition_actor::CoalitionService` (renamed from `CoalitionActor` in K3; same file)
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

Cross-project plan (upstream `aif` + this Phase B): tracked in tira's
private planning notes; the public record is the issue trail on
`sustia-llc/tira` + `sustia-llc/koalisi` (#1/#2 here).

**K2 — magnitude decision arm ([#5](https://github.com/sustia-llc/koalisi/issues/5), DONE 2026-07-02).**
Part of the coalition semantic-layer roadmap (Phase K, planned upstream in
the catgraph project's notes).
The categorical A/B mirror of the AIF arm, behind feature `magnitude`
(independent of `decision` — either, both, or neither):
- Dep: `catgraph-magnitude` by git tag (shipped at `v0.1.0`; bumped to
  `v0.1.1` for the catgraph#29 triangle-tolerance fix; **K6 (#14) bumped
  again to `v0.2.0`** for the catgraph#31 `CoalitionEvaluator`; re-pinned
  `v0.5.0` 2026-07-30, EQ1 pin-first; re-pinned `v0.6.0` 2026-08-02, EQ3
  pin-first, both drift-free — see Current state). Originally
  an SSH URL (catgraph was private then; cargo's libgit2 can't authenticate
  HTTPS) — re-pinned to HTTPS in the 2026-07-27 release sweep after catgraph
  went public. `coalition_value` = magnitude at pinned `t = 1`;
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

**K6 — evaluator hot path ([#14](https://github.com/sustia-llc/koalisi/issues/14), DONE 2026-07-03).**
Adopted catgraph `v0.2.0`'s `CoalitionEvaluator` (catgraph#31) in the magnitude
arm: membership-keyed cache in `MagnitudePolicy`/`MagnitudeValueCalculator`
(interior mutability — the seams are `&self`), `should_join` = cached
`base_value()` + one `value_with(x)`, leave stays fresh (upstream non-goal;
variant B measured slower, ships opt-in via `with_evaluator_leave`).
Decisions bit-frozen: knife-edge fresh fallback (see gotcha 15 — the #31
rank-order contract is insufficient at a zero threshold; found by the
pre-registered parity gate, 16/8068 flips before the fix). Re-run
`docs/ab-report-K4-catgraph-evaluator.md`: quality columns seed-for-seed
identical to the K1 parity report; latency 3.915 → 3.658 µs vs AIF 1.387 —
**Path A missed** (dual verdict unchanged: v1 `FALSIFIED (latency)`, v2
`VALIDATED (B)`), residual-cost profile committed as the pre-registered
catgraph#33 evidence (construction ~30 µs ≈ 10–15× fresh; knife-edge tax
5.05 µs on ~62% of hits; pure hit path 1.35 µs ≈ AIF parity — the mechanism
works where it applies).

**K5 — domain-neutral ingestion ([#8](https://github.com/sustia-llc/koalisi/issues/8), DONE 2026-07-03).**
De-financed the ingestion layer: `src/ingest/` is the real implementation
(`Sample`, `SampleMonitor<S>` — the MarketMonitor logic verbatim-generic with
the K3 contracts intact — `DataSource`, domain-neutral `Pacing`,
`pump_source`), forex is the instantiation (`MarketMonitor =
SampleMonitor<Tick>`, `TickUpdate = SampleUpdate<Tick>`; fields `pair` → `key`,
`quote` → `view` — the one breaking rename). (At K5 a feature-gated databento
adapter also remained, consuming only `ingest::Pacing`; **removed in v0.10.0**
— see the top Current-state Done entry.) Two seeded fixture sources anchor the
roadmap drivers: NEST-shaped `MultiResolutionSource` (per-series `step_ms`,
global timestamp-order merge — the planning-period ↔ hourly gap) and
tauhokohoko-shaped `SensorEventSource` (per-sensor changepoint mean shift —
the SPRT-suitable stream; SPRT itself stays downstream). Acceptance held:
coalition formation runs on synthetic non-financial data
(`tests/ingestion_integration.rs`, `examples/synthetic_ingestion.rs`).

**Post-K salvage — magnitude trajectory over the event log
([#18](https://github.com/sustia-llc/koalisi/issues/18), DONE 2026-07-04, v0.7.0).**
Shipped as `TemporalAnalytics::magnitude_history` + `MagnitudePoint`
(feature `magnitude`) — see §Current state and gotcha 16 for the contracts.
Original salvage note kept below for provenance:
Fold-in salvage from a superseded catgraph-coalition prototype (decision
2026-07-03; salvage split across catgraph#53 / catgraph#36-addendum / this):
a temporal-analytics query that replays coalition membership at sample points
along the event-sourced history and evaluates the pinned t=1
`coalition_value` (or `CoalitionEvaluator` where the membership delta allows
the incremental path) — a diversity-over-time series per coalition.
Re-express against koalisi's own `TemporalQueries`/`TemporalAnalytics` API;
do NOT copy the legacy impl (it was bound to the old SurrealDB live-query
transport). Natural affinity with the Phase 7 persistence re-plan (both
consume the event log); sequence at the user's call. Anchors: BV 2025 §3.5;
catgraph #22/#23/#31.

**K3 — messaging swap ([#6](https://github.com/sustia-llc/koalisi/issues/6), DONE 2026-07-02).**
Hybrid per the pin: hot seams on `tokio::sync` (broadcast buses, mpsc/oneshot
handles, drain-based flush barriers), kameo + kameo_actors REMOVED, thin
restart layer (`core::supervision`), remote gateway on raw libp2p
`request-response`, and the `durable` feature (off by default) putting the
decision-event stream on `surrealdb-live-message` v0.2.0's two-tier
restart-durable bus (tag cut for K3 — the #6 pin's "v0.1.0" was stale; the
restart-durability acceptance needs the v0.2.0 cursor-replay bus). Acceptance
evidence: `.claude/docs/k3-hot-path-bench.md` (every hot-path metric improved) +
`tests/durable_integration.rs` (container-backed restart replay). Gotchas 13–14.
The durable decision log seeds Phase 7's message-event stream (retention sweep
is a bounded window, NOT a full event store — Phase 7 still owns real
durability).

### Phase 7: Persistence  *(DESIGN SHIPPED 2026-07-04, v0.7.0 — [#21](https://github.com/sustia-llc/koalisi/issues/21); implementation = follow-up issues P7.1–P7.5)*

**The design of record is `.claude/docs/phase7-persistence-design.md`** — it
supersedes both the original pre-design sketch (graph-snapshot half
dead since K1; `EventStore` idea survived) and the rmp-serde default (CBOR
won on the RFC 8949 deterministic-encoding profile; user-confirmed).
Summary: layered (portable CBOR append-only hash-chained log = source of
truth; `durable` bus = optional projection); envelope records with
`Plain | Sealed` payloads and causal `parents` over six independent streams;
crypto-deletion = per-subject KEK destruction (keystore outside the log);
revocation = appended Registry events; bilateral manifest-gated federation;
FAIR provenance; Lineage stream schema-reserved for Phase 5 (#20).
`TemporalQueries`/analytics plug in via `replay_into_event_log` — one query
path; P7.2's parity gate is `magnitude_history` over a replayed log ==
in-memory series.

Implementation phasing (**signed off + FILED 2026-07-04; #21 CLOSED**):
[#29](https://github.com/sustia-llc/koalisi/issues/29) P7.1 core chained
log (feature `persistence`, deps ciborium + sha2) — **DONE v0.8.0
(2026-07-04; ciborium picked over minicbor per §17)** ·
[#30](https://github.com/sustia-llc/koalisi/issues/30) P7.2 topology
projection + replay — **DONE v0.9.0 (2026-07-04; the pre-registered #18
`magnitude_history` parity gate held)** ·
[#31](https://github.com/sustia-llc/koalisi/issues/31) P7.3 sealing +
revocation registry ·
[#32](https://github.com/sustia-llc/koalisi/issues/32) P7.4 decision/belief
streams · [#33](https://github.com/sustia-llc/koalisi/issues/33) P7.5
federation manifests + FAIR provenance. Remaining sequencing: #31 next
(**blocked on the tauhokohoko KEK-granularity answer for belief sealing**),
then #32, then #33. Open calls in §17 (SHA-256 vs BLAKE3; ciphertext
reclamation; cross-federation EventRef addressing — resolve at #33;
ciborium-vs-minicbor RESOLVED at #29).

### Downstream: nautilus_trader bridge  *(separate project)*

IB adapter patterns from the nautilus_trader glean analysis inform a
separate `koalisi-nautilus` bridge project. Not a koalisi feature.

### Downstream: tauhokohoko integration  *(separate project)*

**Framing corrected 2026-07-03** (requirements survey for design input #2):
the earlier note here ("Salmon PD simulator using koalisi's coalition
primitives") overstated koalisi's role. tauhokohoko's actual M2 design frames
the salmon domain as **causal-model validation on DeepCausality directly**
(CausaloidGraph with temporal lagged effects + CEL access gates + Teloid
deontics) — no coalition formation, AIF-EFE, or magnitude requirement appears
anywhere in its specs. koalisi's earliest possible entry is the post-M2,
currently-unfunded Phase A demo. What tauhokohoko DOES impose on koalisi is
the Phase 7 persistence constraint set (IDSov: append-only + crypto-deletion
+ bilateral federation + portability — recorded on
[#21](https://github.com/sustia-llc/koalisi/issues/21)). See
`~/Documents/tauhokohoko/tauhokohoko/requirements/causal-context-architecture.md`
and `deliverables/m2-onchain-governance/design-doc.md`.

### Removed: Databento work — moved to `biome`  *(koalisi #22 + #23 CLOSED, v0.10.0)*

The databento (DBN) adapter was removed from koalisi in v0.10.0 (it is a
domain-agnostic coalition runtime). All market-data work — including the
planned `LiveClient` real-time subscriber ([#22](https://github.com/sustia-llc/koalisi/issues/22))
and the synthetic-DBN arb-signal demo
([#23](https://github.com/sustia-llc/koalisi/issues/23)) — now belongs to the sibling
[`biome`](https://github.com/sustia-llc/biome) project, which owns dbn
decode (biome #5/#6/#7). koalisi issues #22 and #23 were closed as
out-of-scope; do not re-file databento work here.

### Removed: forex-coupled backlog — moot since v0.11.0 (#37)

The forex swarm removal retired several tracked-but-forex-specific items.
Their GitHub issues are candidates to close:

- **[#24] Remote gateway hardening** — the gateway itself was deleted (#37);
  its hardening ideas (bounded/cursor buffer, stable wire schema `V1`, QUIC,
  multi-protocol) fold into **[#38]** (re-introduce a domain-neutral remote
  coalition-event gateway). Close #24 → #38.
- **[#26] Multi-triangle stress** and **[#27] bid/ask execution model** — pure
  forex (`coordinator.triangles`, `Triangle` spread math); no coordinator
  exists any more. Moot.
- **[#25] Metrics example** — still valid, but reframe: instrument the
  `CoalitionService` decision path / topology events, not the deleted
  `tick_bus`/`alert_bus`.

## Open questions (jot anything here as it comes up)

> The former forex open questions (coordinator hysteresis per-direction; the
> databento feature split) are moot — the forex swarm (v0.11.0, #37) and the
> databento adapter (v0.10.0) are both gone. Nothing open here right now.
