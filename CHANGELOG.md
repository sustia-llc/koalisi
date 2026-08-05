# Changelog

All notable changes to **koalisi** will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

Planned work — issue-tracked (details in [`CLAUDE.md`](./CLAUDE.md)
§"Next steps"):
- **Phase 5 — SwarmAgentic meta-layer remainder** ([#20]): configurator,
  velocity-rewrite loop, transferability (needs an LLM backend behind
  `src/llm/mod.rs`).
- **Phase 7 — Persistence implementation**: [#31] sealing + revocation
  registry (*blocked on the tauhokohoko KEK-granularity answer*), [#32]
  decision/belief streams (the `TaskOutcome` durable home), [#33] federation
  manifests + FAIR provenance.
- **[#25]** metrics example, reframed onto the `CoalitionService` decision
  path / topology events.

## [0.27.0] — 2026-08-05

The **EQ5a** registration ([#76](https://github.com/sustia-llc/koalisi/issues/76))
— process-structured tasks, workflows as string diagrams. Run verdict:
**`FALSIFIED (process structure)`**
(`docs/ab-report-K4-eq5a-process-structured.md`; prereg
`docs/prereg-K4-eq5a-process-structured.md`, Amendments 1–5 all pre-run).

### Added
- **New gated feature `process = ["magnitude", "dep:catgraph-syntax"]`**
  and `src/process/` — the library surface a runtime would use for
  process-structured coalition tasks: `Role`/`Step` (`s_{b,r} : r → r`,
  role-preserving), `Workflow = ColoredExpr<FrobeniusOr<Step>>`,
  `demand()`, `rule_theory()`, `uniform_cost`/`StaffingTable` +
  `staffing_price()`, `optimize_workflow()`, and `verify_optimization()`
  (replay + `content_eq` — the S-sound helper). Default builds pull
  neither `catgraph-syntax` nor its transitive `deep_causality_haft`.
- **`catgraph-syntax v0.8.0`** as an optional dep, in lockstep with the
  other two catgraph deps (three now — the K6 rule).
- `examples/strategy_comparison.rs` **Part 9** — the EQ5a battery
  (v2w world, control + four confirmatory cells, E-fuel / E-conc /
  E-dedup / E-ceil, gates X-reduce / S-sound / S-dedup). Requires
  `--features decision,magnitude,process`, so the example — and the
  frozen Parts 1–8 battery with it — now needs all three.

### Result
- No H-P cell clears the family-wise bar (≥ 1.4× and ≥ 21/30):
  `wf-rw-u` 0.2104 (1.06×, 17/30) · `wf-rw-p` 0.2112 (1.06×, 19/30) ·
  `wf-val-u`/`wf-val-p` 0.2484 (**1.25×, 30/30**) · control 0.1989.
  Both valuation cells clear the lineage's *standing* 1.25×/60 % bar and
  miss the registered one; **the bar does not move** (pre-committed,
  Amendment A4.3).
- **The signal that converts is valuation, not rewriting** — the
  valuation cells change no demand at all yet post the strongest paired
  consistency since v5. The rewriting arm converted **100 %** of its
  achievable margin (E-ceil identical at 0.2112) and the fuel sweep is
  flat, so the ceiling is low rather than the optimizer weak. See
  **gotcha 30**.

### Notes
- Suites: `process` **152**, `decision,magnitude,process` **208**; all
  pre-existing suites unchanged (106 / 162 / 135 / 191 / 143 / 126 /
  156 / 112).

## [0.26.0] — 2026-08-05

The EQ5a pin-first release: both catgraph deps re-pinned one minor ahead
with a clean drift check, so the EQ5a process-structured registration
([#76](https://github.com/sustia-llc/koalisi/issues/76)) is born on the
final pins. The `v0.8.0` surface koalisi will consume in EQ5a is cg#214
`prop::presentation::rewrite` — the W2 process cost functional
(`cost_of`, defined on content so it is a function of the morphism) and
the W3 bounded convex-DPO engine (`RewriteRule`, `optimize`, `replay`,
`RewriteOutcome`) — the sole delta over `v0.7.0`, purely additive
(a new module plus one example; no existing signature changed).

**The pin stops at `v0.8.0`** by owner decision (2026-08-04): a
pre-registration keeps the tag its design targeted, so catgraph `v0.9.0`
waits for a later re-pin koalisi wants for its own reasons.

### Changed
- **`catgraph-applied` + `catgraph-magnitude` deps re-pinned `v0.7.0` →
  `v0.8.0` in lockstep** (one checkout — the K6 rule). Drift check
  CLEAN: all eight feature suites at baseline counts
  (106/162/135/191/143/126/156 incl. the #18/#30 replay parity gate, and
  112 `remote`), default clippy `--all-targets` clean, and the frozen K4
  battery (`strategy_comparison` Parts 1–8, `--release`) reproduced
  against a fresh `v0.7.0` baseline with every quality median, ratio,
  superiority count, churn column, seed table and verdict
  byte-identical — the only differing lines are latency measurements,
  the column the K4 protocol has always excluded from determinism.
- Lockfile: the three catgraph workspace packages moved (`catgraph`,
  `catgraph-applied`, `catgraph-magnitude` 0.7.0 → 0.8.0, commit
  `9eae951b` → `5ee51f3d`) plus koalisi's own version line. Re-resolution
  also shifted **two unrelated transitive edges** to already-present
  sibling versions — `prost-derive`'s `itertools` 0.14.0 → 0.11.0 and
  `tempfile`'s `getrandom` 0.4.3 → 0.3.4. Both are ordinary resolver
  churn (no package added or removed, all suites green); recorded because
  prior re-pin entries in this file claim an *exactly-three-packages*
  lockfile delta and this one is not that.
- README catgraph tag citations updated `v0.7.0` → `v0.8.0` (checked on
  every re-pin since the v0.21.0 and v0.25.0 misses).

### Notes
- MSRV stays **1.93**: catgraph `v0.8.0` still carries the
  `deep_causality_num =0.4.1` pin that forces the floor. That crate is
  removed in catgraph `v0.9.0`, so the floor is re-testable at the *next*
  re-pin, not this one.
- The `catgraph-syntax` dependency EQ5a's design-lock calls for (D6 —
  Frobenius spiders + the presentation text surface) does **not** land
  here: it arrives with the gated feature it belongs to, in the
  implementation PR, so this PR's drift check keeps meaning what it
  means.

## [0.25.0] — 2026-08-03

The [#38] gap-filler release: the domain-neutral remote coalition-event
gateway, re-introducing (and hardening) the off-process publish boundary
deleted with the forex swarm in v0.11.0. Design owner-locked on [#38]
(D1–D6) before implementation; the old [#24] hardening list is folded in
(bounded buffer, cursor polling, stable wire schema); QUIC, mDNS-expiry
changes, and a topology-event protocol are deliberately deferred.

### Added
- **`subsystems::remote`** (new feature `remote`, gated `libp2p 0.56`
  dep — feature-off builds pull none of it): raw-libp2p
  `request-response` gateway on `/koalisi/coalition-events/1`
  (TCP + noise + yamux, CBOR; mDNS via `Toggle`, genuinely off when
  disabled). `RemoteCoalitionEventV1` stable wire schema
  (`DecisionRecord` stays serde-free; conversion at the boundary, the
  P7.2 `WireTopologyEvent` precedent), bounded `EventBuffer` (cap
  default 1024, oldest evicted; cap 0 clamps to 1 — unclamped it would
  grow unbounded), gateway-stamped monotonic `seq` (from 1),
  `EventRequest::{PollSince, Head}` cursor polling (no `Clear` —
  destructive under multiple consumers), `enable_remote_gateway`
  (tracker + token runtime handles), `RemoteCoalitionClient`
  (`dial`/`discover`/`pump`/`poll_since`/`head`; refuses events with a
  `schema_version` newer than `REMOTE_WIRE_SCHEMA_VERSION`).
- **`subsystems::coalition_actor::spawn_decision_tee`** (always
  compiled): fans the single-consumer decision tap into N sinks with
  per-sink `try_send` drop-with-warn — `durable` and `remote` compose
  on one tap. At-most-once gains one more lossy hop; documented.
- `examples/remote_coalition_consumer.rs` — gateway + client in one
  process over a live policy-gated `CoalitionService`.
- `tests/remote_integration.rs` — loopback round-trip (mDNS off,
  explicit dial) through service → tee → gateway → client, cursor
  deltas + seq ordering asserted.

### Fixed
- README drift: test counts were stale (v0.16.0-era) and the
  `magnitude-fast` suite line was missing; catgraph tag citations said
  `v0.6.0` after the v0.23.0 re-pin to `v0.7.0`.

Suites: 106 default / 162 decision / 135 magnitude / 191
decision,magnitude / 143 magnitude-fast / 126 persistence / 156
persistence,magnitude (all +3 tee unit tests) / **112 remote** (new).

## [0.24.0] — 2026-08-03

The EQ4 run release ([#72]): the registered typed-roles battery, executed
under full discipline (owner design-lock D1–D9 → prereg + pre-run
Amendments 1–2 → 3-lens review with every finding applied → official run
on fresh seeds 240..270). **Verdict: `VALIDATED (typed roles)`** — the
first VALIDATED registration in the K4 lineage since v5, and the first
typed-vs-untyped contrast: `mag-typed` (T2 ρ-modulation at the oracle
identity table) posts median PRIMARY 0.1810 vs frozen `mag` 0.0501
(3.61×, bar 1.25×), strictly superior 30/30 (bar 18/30), with every gate
holding (X-identity ×2, S-fib ≤ 3.8e-16 vs 1e-9 tol, X-battery
byte-identical). Mechanism: the arm never sees tags — its one lever is
refusing to skeletalize cross-role members (role-diverse redundancy
retained, not coverage routing); it converts 43.5 % of the tag-informed
E-ceil reference margin. Report `docs/ab-report-K4-eq4-typed-roles.md`
(15-item ledger); prereg + Amendments
`docs/prereg-K4-eq4-typed-roles.md`.

### Added
- **`MagnitudePolicy::with_role_modulation` /
  `MagnitudeValueCalculator::with_role_modulation`** (feature `magnitude`,
  no new deps): opt-in typed arm — substitutability couplings modulated by
  `ρ(role_i, role_j)` via catgraph `coalition_typed::modulate`, evaluated
  FRESH both sides (no evaluator cache — the K6 key `(required,
  member_masks)` would collide same-mask/different-role agents); identity
  default routes structurally to the untyped path (bit-identical by
  construction); irrelevant agents excluded BEFORE modulation; missing
  role ⇒ decline-with-warn; `with_eq3_levers`/`with_evaluator_leave`
  documented inert under a typed config. `RoleId`/`RoleModulation`
  re-exported from `koalisi::decision`. 6 unit tests.
- `examples/strategy_comparison.rs` **Part 8** — the registered EQ4
  battery (v2t role-matched world + feasibility re-draw, four arms,
  X-identity/S-fib gates in-code, E-deg/E-ceil/E-ρq(+inv)/E-T3
  exploratory legs with measured caveat counters, T1 `role_shares`
  instrumentation). Parts 1–7 byte-identical (X-battery gate held against
  a fresh v0.23.0 baseline).

### Notes
- The typed arm is opt-in; the demonstrated default arm is unchanged (the
  #54 decision stands — adopting a typed default would be a new
  registration).
- Suites: 103 default / 159 decision / **132** magnitude / **188**
  decision,magnitude / **140** magnitude-fast / 123 persistence / **153**
  persistence,magnitude; example binary **32 (+1 ignored)**.

## [0.23.0] — 2026-08-03

The EQ4 pin-first release: both catgraph deps re-pinned one minor ahead
with a clean drift check, so the EQ4 typed-roles registration
([#72](https://github.com/sustia-llc/koalisi/issues/72)) is born on the
final pins. The v0.7.0 surface koalisi will consume in EQ4 is cg#211
`coalition_typed` — T1 `role_shares` diagnostics, T2
`RoleModulation`/`modulate` + `role_grid`/`RoleFibrationProof`, T3
`ChannelCouplings::collapse(θ)` — the sole delta over v0.6.0, purely
additive (magnitude-only; no existing signature changed).

### Changed
- **`catgraph-applied` + `catgraph-magnitude` deps re-pinned `v0.6.0` →
  `v0.7.0` in lockstep** (one checkout — the K6 rule). Drift check
  CLEAN: all seven feature suites at baseline counts
  (103/159/126/182/134/123/147, incl. the #18/#30 replay parity gate),
  default clippy `--all-targets` clean, and the frozen K4 battery
  (`strategy_comparison` Parts 1–7, `--release`) reproduced with every
  quality median, churn column, seed table, and verdict byte-identical
  to a fresh `v0.6.0` baseline — the only diff lines are latency
  measurements, the column the K4 protocol has always excluded from
  determinism.
- Lockfile: exactly the three catgraph workspace packages moved
  (`catgraph`, `catgraph-applied`, `catgraph-magnitude` 0.6.0 → 0.7.0);
  catgraph's `deep_causality_num =0.4.1` pin is unchanged, so MSRV stays
  1.93.

### Fixed
- Suite-count record: `persistence,magnitude` is **147**, not the 146
  carried since v0.21.0 — EQ3's +1 `magnitude`-feature lib test lands in
  this suite too and the count was never re-measured at v0.22.0.
  Verified identical on the pre-change tree (documentation correction,
  not drift).

## [0.22.0] — 2026-08-02

The EQ3 run release ([#69]): the registered cg latency re-match, executed
under full discipline (owner design-lock → prereg + pre-run Amendment 1 →
3-lens review with every finding applied → official run on fresh seeds
210..240). **Verdict: `FALSIFIED (latency re-match)`** — quality parity
held (H-par′ both conjuncts PASS; the 49 first divergences are all
certified exact-zero declines, mildly quality-positive), but the strict
Path-A analogue failed (mag-eq3 4.830 µs vs scalar 2.675 µs; the ratio
shrank 2.48× → 1.81×, no crossing). Headline instrumentation: **the
cg#153 [1e-13, 1e-6) empty-band hypothesis is CONFIRMED on koalisi
traffic** (0 decisions in the band; knife-edge population 43.0 % of
probed joins, 99.6 % of the frozen arm's band recomputes
certificate-retired). Report `docs/ab-report-K4-eq3-latency-rematch.md`
(10-item deviation ledger); prereg + Amendment 1
`docs/prereg-K4-eq3-latency-rematch.md`.

### Added
- **`magnitude-fast` cargo feature** (off-default; pass-through to
  `catgraph-magnitude/f64-fast`) + `MagnitudePolicy::with_eq3_levers`
  (identity default OFF). Toggle ON = the `mag-eq3` arm: **L2** the
  cg#153 zero-diversity proof branch (all three classes; deliberately
  decision-changing on certified exact-zeros — the frozen arm joins on
  +2e-16 roundoff, the certificate declines) + **L3** `f64-fast` fresh
  evaluation via the skeletal-space rebuild (A1.3). L3's fast route
  engages only on exactly-symmetric ζ (substitutability couplings mostly
  are not — see the report's FactorizationPath section).
- Feature-gated read-only instrumentation: `MagnitudePolicy::probe_join`
  (`JoinProbe`) + `probe_fresh_factorization` (answers off throw-away
  caches; no decision-path change).
- `examples/strategy_comparison.rs` **Part 7** — the registered EQ3
  battery (paired H-par′ walk, H-lat, context rows, four instrumentation
  blocks). Parts 1–6 byte-identical (X-A feature-off and X-B feature-on
  gates both held against a fresh pre-change baseline).

### Changed
- **L1 ships as the library default**: `CoalitionEvaluator` calls go
  through `value_with_scratch` with the scratch retained across evaluator
  rebuilds — bit-identical to pre-EQ3 (pinned by a 60-seed stream gate on
  acts AND score bits, both leave variants), and a mild win even on the
  frozen v1 battery (Part 2 mag median 3.55 → 3.44 µs).
- CLAUDE.md's K6 latency citations corrected to the report-of-record
  numbers (3.552/1.435; previously 3.658/1.387 — review ledger item 9).

### Tests
- Suites: 103 default / **126** magnitude / **182** decision,magnitude /
  **134** magnitude-fast (new suite) / 123 persistence / 146
  persistence,magnitude; example binary 26 (+1 ignored) under
  decision,magnitude and 31 (+1) with magnitude-fast.

## [0.21.0] — 2026-08-02

The EQ3 pin-first release: both catgraph deps re-pinned one minor ahead
with a clean drift check, so the EQ3 latency-re-match registration
([#69]) is born on the final pins. The v0.6.0 surfaces koalisi will
consume in EQ3: `value_with_scratch` (cg#33 allocation-free sweeps),
`value_with_report` zero-diversity proofs (cg#153), and the off-default
`f64-fast` factorization path (cg#165).

### Changed
- **`catgraph-applied` + `catgraph-magnitude` deps re-pinned `v0.5.0` →
  `v0.6.0` in lockstep** (one checkout — the K6 rule). The v0.6.0
  breaking set is applied-side (cg#202 CC-metric re-pin, cg#185
  symmetric cuts) and does not touch koalisi's consumption (the
  Hypergraph container + the magnitude evaluation path); magnitude's new
  EQ3 surfaces are additive and `value_with` is byte-identical upstream.
  Drift check CLEAN: all six feature suites at baseline counts
  (103/159/125/181/123/146, incl. the #18/#30 replay parity gate),
  default clippy `--all-targets` clean, and the frozen K4 battery
  (`strategy_comparison` Parts 1–6, `--release`) reproduced with every
  quality median, churn column, seed table, and verdict byte-identical
  to a fresh `v0.5.0` baseline — the only diff lines are latency
  measurements, the column the K4 protocol has always excluded from
  determinism.
- Lockfile: exactly the three catgraph workspace packages moved
  (`catgraph`, `catgraph-applied`, `catgraph-magnitude` 0.5.0 → 0.6.0);
  catgraph's `deep_causality_num =0.4.1` pin is unchanged, so MSRV stays
  1.93. Rider: the lockfile's own `koalisi` version line was stale at
  0.19.0 (the 0.20.0 bump never regenerated it) — refreshed here.

## [0.20.0] — 2026-08-01

The pre-EQ4 adoption release: `aif` re-pinned one minor ahead with a clean
drift check, clearing the AIF-side substrate (generic agent slots +
nesting) the EQ4 typed-roles registration will build on.

### Changed
- **`aif` dep re-pinned `aif-v0.11.0` → `aif-v0.12.0`** (tira Phase 3:
  #39 generic blanket slots, #41 `GroupAgent` nesting, #11 hardening; the
  one breaking rider — #9 serde feature-gating on
  `Message`/`MessageContent`/`InfoRequestType` — is a no-op for koalisi,
  which never touches those types). Drift check CLEAN: all six feature suites at baseline
  counts (103/159/125/181/123/146, incl. the #18/#30 replay parity gate),
  default clippy `--all-targets` clean, and the frozen K4 battery
  (`strategy_comparison` Parts 1–6, `--release`) reproduced with every
  quality median, churn column, seed table, and verdict byte-identical to
  the `aif-v0.11.0` baseline — the only diff lines are latency
  measurements, the column the K4 protocol has always excluded from
  determinism. Expected per the stack file: 0.12.0's additive surface
  (generic slots + nesting) never touches the `POMDPAgent` path koalisi's
  scalar/mm/persistent arms consume.
- Lockfile: `aif 0.11.0 → 0.12.0` — the only package moved; MSRV
  unchanged at 1.93.

## [0.19.0] — 2026-07-31

The EQ1 ([#61]) Part 5c close-out: the four exploratory items deferred from
the v0.18.0 battery-v2 run, delivered as an appended addendum to the
immutable report (`docs/ab-report-K4-battery-v2.md`) — nothing registered
was edited. Headlines (all context, non-gating): the join rail is
margin-proof at 12 bits too and the v2-regime quality inversion widens
there (mag 0.0607 < scalar 0.1062 < e1-degraded 0.1657); leave-side
hysteresis is the first score-space lever measured to move churn on the E1
lineage (0.85× at h = 0.30, 29/30 paired) but pays −24% quality; the
expected-outcome value model is gotcha-21 degenerate by a third mechanism
(per-block double-counting); learned reliability posteriors rank the
planted weak bit correctly on 30/30 seeds.

### Added
- **`PersistentAifConfig::n_bits: usize`** (feature `decision`): the
  world-model bit-width, identity default **8** — at the default the arm
  is bit-for-bit the registered arm (asserted in-code + by the X-A/X-C
  gates). Out-of-range values clamp to `1..=16` with a warning. +5 unit
  tests (identity, 12-bit construction/decisions, snapshot width, mismatch
  skip, clamp).
- **`examples/strategy_comparison.rs` Part 5c** (`part5c_addendum()`,
  additive; every pre-existing printed line byte-identical — X-C
  re-verified against a fresh pre-change baseline): the four deferred
  registered-exploratory items — 12-bit `w12-draw` slice (`Regime::W12`,
  `|required|` 2..=12, caps 1..=6), leave-side hysteresis sweep with an
  asserted in-line baseline, expected-outcome value model with its
  degeneracy analysis, learned-posterior routing twins on a salted
  side-stream. +9 example tests (one `#[ignore]`d release-only battery
  smoke).
- **Report addendum** appended to `docs/ab-report-K4-battery-v2.md` with a
  7-entry implementation & deviation ledger (clamp-not-error, hook
  widening, the latent out-of-universe-mask fix, test-cost concessions,
  both item-2 cell-selection interpretations, the item-3 gating reading,
  the item-4 stream discipline).

### Changed
- **`PersistentAifArm::observe_outcome`** takes `&[bool]` (was
  `&[bool; 8]`; **breaking**) — `&[bool; 8]` coerces, so existing call
  sites compile unchanged; a length mismatch warns and skips the update.
  The shared example outcome hook widened the same way (output-neutral,
  proven by X-C).
- `decide()` masks `required` to the low `n_bits` bits — a no-op for every
  previously-reachable input, load-bearing once `n_bits` is configurable.

## [0.18.0] — 2026-07-31

The EQ1 ([#61]) battery-v2 release: the registered de-saturated-regime run.
Both confirmatory levers landed negative — `FALSIFIED (de-saturation)` and
`RUN-INVALID (sanity leg)` — with the mechanism and the criterion flaws
measured and documented; the v2-regime context rows invert the v1 quality
ordering (mag < scalar < e1-degraded). Falsification discipline unchanged:
nothing was tuned, everything is reported.

### Added
- **`PersistentAifConfig::query_gamma: Option<f64>`** (feature `decision`):
  the EQ1 de-saturation lever — fixed query-POMDP softmax temperature γ on
  the MeanField path (`query_dynamics: false`). Identity default `None` =
  engine γ 16, the registered arm-E1/K4-v5 value; `Some(16.0)` is asserted
  bit-identical. Ignored under `query_dynamics: true` (`PrecisionDynamics`
  owns γ there). Battery-v2 arm-config labels: `arm-E1g1` / `arm-E1g4` /
  `arm-E1g16`; arm-E1 itself stays `None`. +2 unit tests (identity,
  non-degeneracy).
- **`examples/strategy_comparison.rs` Parts 5a + 5b** (additive; every
  pre-existing printed line byte-identical, gate X-C): the registered
  battery-v2 parts — 5a: γ × regime × margin factorial on seeds 120..150
  with the degraded/L2 signal, score-quantile mechanism observable, H-S
  evaluation; 5b: `TaskCoverageV2` reliability-routing test with planted
  weak bit, closed-form `REAL`, H-R evaluation + structural notes.
  `run_seed_b` became a thin `Regime::V1` wrapper over a new
  `run_seed_b_regime` (the regime parameter selects the v1/v2 instance
  draw); the 4-arg outcome hook is UNCHANGED — it has carried the member
  list since Part 4g (gotcha 23). Call-site refactor output-neutral,
  proven by gate X-C.
- **`docs/prereg-K4-battery-v2.md`** (registered pre-implementation,
  immutable) and **`docs/ab-report-K4-battery-v2.md`** (the run's immutable
  report: lever 2 `FALSIFIED (de-saturation)` — γ frees only the leave
  stream, the join rail at p = 1.0 defeats any margin; lever 1
  `RUN-INVALID (sanity leg)` — corrected registration filed as [#63];
  lever 3 exploratory: the oracle–degraded gap widens ~2% → ~16% in the v2
  regime). Of Part 5c's five registered exploratory items, the
  oracle-vs-degraded pricing ran (as the Part 5a oracle twins, the lever-3
  rows above); the other four (12-bit slice, leave-side hysteresis,
  expected-outcome model, learned-posterior twins) are deferred to a
  follow-up session and will land as an appended addendum.

### Changed
- Suites: decision 152 → **154**, decision,magnitude 174 → **176** (the two
  `query_gamma` tests). All other feature suites unchanged.
- `Cargo.lock`: koalisi's own entry synced to the crate version (was stale
  at 0.16.0 since the v0.17.0 release commit).

## [0.17.0] — 2026-07-30

The EQ1 ([#61]) pin-first release: catgraph re-pinned two majors ahead with
a clean drift check, clearing the instrument's substrate before the
battery-v2 registrations.

### Changed
- **catgraph deps re-pinned `v0.2.0` → `v0.5.0`** (`catgraph-applied` +
  `catgraph-magnitude`, lockstep per K6). Drift check CLEAN: all six
  feature suites at baseline counts (103/152/125/174/123/146, incl. the
  #18/#30 replay parity gate) and the frozen K4 battery
  (`strategy_comparison` Parts 1–4h, `--release`) reproduced with every
  quality median, churn column, seed table, and verdict byte-identical to
  the `v0.2.0` baseline — the only diff lines are latency measurements,
  the column the K4 protocol has always excluded from determinism.
  Expected per the stack file: the in-hop catgraph revisions (column
  pass / eq_mod / worded surface / display) never touch the magnitude
  evaluation path koalisi consumes.
- **MSRV `1.88.0` → `1.93.0`** — forced by catgraph `v0.5.0`'s
  `deep_causality_num 0.4.1` floor (recorded finding; not a koalisi
  code change).
- Lockfile: `deep_causality_num 0.3.3 → 0.4.1`; the `primal`/`hamming`
  transitive tree dropped upstream.

## [0.16.0] — 2026-07-27

The public-release version: the repo went PUBLIC 2026-07-27, and per-release
tagging resumed with this release (`v0.7.0`–`v0.15.0` backfilled onto their
release merges the same day).

### Added

- **`ReliabilityCoverage`** (feature `decision`,
  [#57](https://github.com/sustia-llc/koalisi/issues/57)) — a
  `ValueCalculator` derived from the persistent AIF world model
  (`PersistentAifState`), giving the population structure-search (#42) a
  reliability-weighted fitness: the `TaskCoverage` interior-optimum shape
  with each required bit weighted by the world model's per-bit reliability
  posterior (`beliefs[b][0]`, state 0 = reliable). Constructors `new`
  (explicit vector; clamps, sanitizes non-finite entries to 0.0) and
  `from_state` (snapshot read; short/ragged snapshots fall back to the
  uniform prior per bit). With reliability ≡ 1 it reduces exactly to
  `TaskCoverage` (within the low 8 bits). Reliability *rescales* the
  coverage optimum rather than routing around weak bits (for
  `|required| ≤ 6` skipping never pays at equal member count) — documented,
  with the BIT-level scope constraint (no agent-level discrimination; the
  world model has no agent-indexed factor). 5 unit tests including the
  gotcha-21 non-degeneracy discipline.
- **`examples/population_reliability.rs`** (requires `decision`) — feeds a
  `PersistentAifArm` (E1 configuration, `query_dynamics: false`) a
  deterministic 20-task outcome stream with one flaky bit, snapshots it,
  runs `search()` under the derived calculator, and records + replays the
  best structure (the #42 acceptance pattern). Output prints the per-bit
  posteriors with a recency caveat (the belief is a 2-step-MMP-window
  smoothed posterior, not a success-rate average).

### Changed — public-release prep (2026-07-27, no behavior change)

- All git-tag dependencies re-pinned SSH → HTTPS (`aif`,
  `catgraph-applied`, `catgraph-magnitude`, `surrealdb-live-message` —
  every upstream is public now), so outside builds work for every feature.
- `LICENSE-MIT` + `LICENSE-APACHE` added (dual MIT OR Apache-2.0, matching
  the existing `Cargo.toml` `license` field); `repository` + `readme`
  metadata added.
- `Cargo.lock` is now tracked (was gitignored) — pins the reproducibility
  story the A/B reports rely on.
- README: new "The A/B process" section (the pre-registered
  K4 v1→v6 verdict trail); aif dep tag drift fixed (0.9.0 → 0.11.0);
  test counts re-measured.
- `docs/` reorg: now purely the A/B showcase trail (pre-registrations,
  reports, decision memo, evidence). Internal design docs + paper
  references moved to the tracked `.claude/docs/` (phase7 persistence
  design, K3 bench, SwarmAgentic digest + CC0 paper copy); all
  references updated — registered prereg/report docs untouched.
- `decision` module docs: implementation list brought current
  (`AifMmDecisionPolicy`, `PersistentAifArm`, `ReliabilityCoverage`).

## [0.15.0] — 2026-07-18

### Added

- **K4-v6 state levers on the persistent AIF arm**
  ([#56](https://github.com/sustia-llc/koalisi/issues/56)):
  `PersistentAifConfig` gains `eviction_cap: Option<u32>` (per-task leave-act
  cap; `Some(0)` = never-evict, skip-query leave semantics) and
  `rejoin_lockout_tasks: u64` (bar a just-evicted agent from rejoining for k
  tasks) — both **identity-default** (`None` / `0` reproduce the #53
  registered arm bit-for-bit, gated by the X-A/X2 asserts).
  `examples/strategy_comparison.rs` gains Part 4h, the registered K4-v6
  battery (fresh seeds 60..90, dual-signal).

### Registered result

- **`FALSIFIED (never-evict)`** (`docs/ab-report-K4-v6-never-evict.md`,
  prereg `docs/prereg-K4-v6-never-evict.md`): the never-evict point collapses
  to 0.0143/0.0141 vs the 1.25×-mag bar 0.3551 under both signals. The
  exploratory cap series is monotone (quality rises with allowed evictions) —
  **eviction churn is the arm's mechanism, not overhead**. Memo option B's
  parked state is final: magnitude stays the demonstrated default; e1 stays
  parked as capability evidence (its quality edge replicated on a third seed
  range: e1-k0 0.3840 vs mag 0.2841 on 60..90).

## [0.14.0] — 2026-07-18

### Added

- **Arm-agnostic task-completion event seam**
  ([#55](https://github.com/sustia-llc/koalisi/issues/55), decided by the #54
  arm-choice memo, option B): new always-compiled `src/subsystems/outcome.rs`
  (zero new deps) — `TaskOutcome { required, members, success }` (the L2
  contract: whole-task success is sufficient, per the #54 Step 2 measurement
  0.4381 ≈ oracle 0.4406), `OutcomeSink` fan-out trait with `FeedbackStore`
  (scalarized #41 consumer) and closure (arm side-channel) impls,
  `emit_outcome` (non-blocking `try_send`, drop-with-warn — the decision-tap
  contract), and `spawn_outcome_forwarder` (biased cancel vs drain-to-`None`,
  mirroring the `durable` forwarder's shutdown disciplines). Emission is the
  embedding domain's responsibility; koalisi never synthesizes outcomes. NOT a
  `TemporalEvent` — the durable home is the P7.4 (#32) streams.
- `examples/synthetic_ingestion.rs`: deterministic "task outcomes" section
  demonstrating the seam (forwarder fan-out to a `FeedbackStore` + counter
  sink, drain-then-print).

### Notes

- The module docs carry the Part 4g caveat: this seam feeds **learned**
  consumers; raw ratio-gating on the signal measured strictly worse than no
  gating (absorbing exclusion).

## [Unreleased]

Planned work — issue-tracked (details in [`CLAUDE.md`](./CLAUDE.md)
§"Next steps"):

- **Phase 5 — SwarmAgentic-style optimisation** (de-gated 2026-07-15):
  ~~[#41] ValueCalculator feedback weights~~ (DONE 0.12.0), ~~[#42] AIPA
  population search~~ (DONE 0.13.0), [#20] the LLM meta-layer remainder
  (configurator, velocity-rewrite loop, transferability).
  - ~~[#46] feedback-weighted arm in the K4 battery~~ — **FALSIFIED
    (feedback), 2026-07-16** (example + report only, no version bump):
    `examples/strategy_comparison.rs` Part 3 + `docs/prereg-feedback-arm-k4.md`
    + `docs/ab-report-feedback-arm-k4.md`. The balanced `hw=fw` weighting
    cancels in the full-join base; a selective base / failure-weighted point
    would each need a fresh registration.
- **Phase 7 — Persistence implementation** ([#29]–[#33] from the [#21]
  design doc): ~~[#29] core chained log~~ (DONE 0.8.0), ~~[#30] topology
  projection + replay~~ (DONE 0.9.0 — #18 parity gate held), [#31]
  sealing + revocation registry (*blocked on the tauhokohoko
  KEK-granularity answer*), [#32] decision/belief streams, [#33]
  federation manifests + FAIR provenance.
- **[#38]** domain-neutral remote coalition-event gateway (successor to
  the removed `remote` feature; folds in the [#24] hardening ideas).
- **[#44]** persistent-agent AIF design (E1/E2 learning + precision
  dynamics deferred from the K4-v3 rematch; unscheduled).
- **[#25]** metrics example, reframed onto the `CoalitionService`
  decision path / topology events.

## [0.13.0] — 2026-07-16

Phase 5 idea 4 ships as the second LLM-free slice ([#42]): population-based
coalition-structure search atop AIPA.

### Added

- **Population search** ([#42]): new `algorithms::population` module (always
  compiled, zero new deps). `search(agents, calc, cfg) -> SearchOutcome` runs a
  deterministic (SplitMix64-seeded, `rand`-free) SwarmAgentic-style particle
  swarm over coalition structures (set-partitions), maximising `Σ over blocks of
  ValueCalculator(block)`. AIPA integer-partition shapes seed a diverse
  population; per-agent global-best / personal-best pulls plus random mutation
  evolve it; the strictly-improving global-best `lineage` is returned. `search`
  is pure and synchronous; the separate async `record_trajectory(manager, agents,
  lineage)` writes the lineage into a `CoalitionManager` as form/dissolve epochs,
  replayable through `TemporalQueries` (the `tests/population_test.rs` parity
  gate). New `examples/population_search.rs` (a `TaskCoverage` value model with a
  non-trivial interior optimum, since the built-in calculators are degenerate for
  structure search — see CLAUDE.md gotcha 21). `CoalitionStructure`,
  `PopulationConfig`, `SearchOutcome`, `search`, `record_trajectory` re-exported
  from `algorithms`.

### Notes

- **Value models for structure search**: `search` maximises `Σ over blocks of
  ValueCalculator(block)`, but the built-in calculators are *degenerate* for
  *structure* search — `AdditiveCalculator` is **constant** across every
  set-partition, and `SynergisticCalculator` / `MultiplicativeCalculator` favour
  all-singletons (trivial optimum, one-epoch lineage). The `population_search`
  example therefore ships a `TaskCoverage` value model with a genuine interior
  optimum; real work needs an interior-optimum calculator (coverage-style, or the
  `magnitude` / EFE / `FeedbackCalculator` arms). See CLAUDE.md gotcha 21.
- Tests: 98 default / 129 decision / 120 magnitude / 151 decision,magnitude /
  118 persistence / 141 persistence,magnitude (+10 each vs 0.12.0: 5 lib unit +
  4 integration + 1 doctest; population is default-compiled and un-gated).

## [0.12.0] — 2026-07-16

Phase 5 idea 3 ships as an LLM-free slice ([#41]), plus the previously
uncut [#43] decision-layer work (aif 0.9.0 + the K4-v3 multimodal
rematch) that landed on `main` after 0.11.0.

### Added

- **Feedback-weighted value calculation** ([#41]): new
  `algorithms::feedback` module (always compiled, zero new deps).
  `FeedbackCalculator<C: ValueCalculator>` wraps any base calculator and
  adds two SwarmAgentic velocity-coefficient analogues, closing the
  feedback loop inside Rust with no LLM round-trips:
  `history_weight` (≈ `c_p`, personal-best guidance — rewards recorded
  coalition-membership episodes) and `failure_weight` (≈ `c_f`,
  failure-driven repulsion — penalises outcomes strictly below a
  threshold), each scaled by `HISTORY_UNIT`/`FAILURE_UNIT` (= 25.0).
  Signals accumulate in a shared `FeedbackStore` (`Clone` shares;
  `record_outcome` ignores + warns on non-finite values so the counters
  can't be NaN-poisoned; duplicate member ids count per occurrence —
  dedup first for at-most-once semantics). Zero weights reproduce the
  base calculator exactly. Under `ThresholdPolicy` the join marginal
  decomposes as `base_marginal + hw·25·history(x) − fw·25·failures(x)`
  (existing members' counters cancel), so the feedback acts directly on
  the candidate's margin — and doubles as a third baseline arm for any
  future K4 rematch.
- **Event-log history seeding** ([#41]):
  `CoalitionManager::agent_coalition_history` promoted to `pub` (the
  Phase 5 anchor activating) and new
  `CoalitionManager::seed_feedback_history` folds per-agent
  membership-episode counts from the event-sourced log into a
  `FeedbackStore`. Seed a given store at most once, before recording
  begins — seeding accumulates and is not idempotent (CLAUDE.md
  gotcha 19). Failures are not seedable (the log has no outcomes).
- **Multimodal AIF arm** ([#43] Part 2, landed 2026-07-16 pre-cut):
  `AifMmDecisionPolicy`/`MmEfeValueCalculator` (feature `decision`) —
  one observation modality per required capability bit via
  `GenerativeModel`; direct optional `nalgebra` dep under `decision`.
  K4-v3 registered run: **FALSIFIED (multimodality)** — decision-
  equivalent to the scalar arm on all 30 seeds
  (`docs/ab-report-K4v3-multimodal-aif.md`); the v2 magnitude-quality
  verdict stands.

### Changed

- **aif pin bumped `v0.5.0` → `v0.9.0`** ([#43] Part 1): tira's
  canonical-AIF parity roadmap complete upstream; decision suite passed
  unchanged (anchors bit-identical across four upstream minors).

### Tests

- Suites: 88 default / 119 `decision` / 110 `magnitude` /
  141 `decision,magnitude` / 108 `persistence` /
  131 `persistence,magnitude` (+12 everywhere from [#41]: 9 unit +
  3 integration, incl. a `ThresholdPolicy` loop-closure test and an
  event-log seeding test).

[#18]: https://github.com/sustia-llc/koalisi/issues/18
[#20]: https://github.com/sustia-llc/koalisi/issues/20
[#21]: https://github.com/sustia-llc/koalisi/issues/21
[#22]: https://github.com/sustia-llc/koalisi/issues/22
[#23]: https://github.com/sustia-llc/koalisi/issues/23
[#24]: https://github.com/sustia-llc/koalisi/issues/24
[#25]: https://github.com/sustia-llc/koalisi/issues/25
[#26]: https://github.com/sustia-llc/koalisi/issues/26
[#27]: https://github.com/sustia-llc/koalisi/issues/27
[#29]: https://github.com/sustia-llc/koalisi/issues/29
[#30]: https://github.com/sustia-llc/koalisi/issues/30
[#31]: https://github.com/sustia-llc/koalisi/issues/31
[#32]: https://github.com/sustia-llc/koalisi/issues/32
[#33]: https://github.com/sustia-llc/koalisi/issues/33
[#37]: https://github.com/sustia-llc/koalisi/issues/37
[#38]: https://github.com/sustia-llc/koalisi/issues/38
[#41]: https://github.com/sustia-llc/koalisi/issues/41
[#42]: https://github.com/sustia-llc/koalisi/issues/42
[#43]: https://github.com/sustia-llc/koalisi/issues/43
[#44]: https://github.com/sustia-llc/koalisi/issues/44
[#61]: https://github.com/sustia-llc/koalisi/issues/61
[#63]: https://github.com/sustia-llc/koalisi/issues/63
[#69]: https://github.com/sustia-llc/koalisi/issues/69

## [0.11.0] — 2026-07-14

Second (and final) step of the de-financialisation pass begun in 0.10.0:
koalisi is now a purely domain-agnostic coalition runtime. The forex
triangular-arbitrage domain — which survived only as the runtime
*demonstration* — is gone; the demonstrated runtime is now a synthetic,
non-financial coalition-formation pipeline. ([#37])

### Removed

- **Forex domain deleted.** `src/market.rs` (`Pair`/`Tick`/`Quote`/`Triangle`/
  `ArbitrageOpportunity`/`Direction`) and the arbitrage "swarm"
  (`subsystems/{coordinator,sink,swarm,monitor}.rs`) are removed, along with
  the `historical_bootstrap`, `live_pubsub`, `triangular_arbitrage`, and
  `hot_path_bench` examples and `tests/integration_test.rs`. The domain-neutral
  coalition core — `topology` (`CoalitionManager`), `algorithms`, `decision`,
  `ingest`, and `subsystems::coalition_actor::CoalitionService` — is unchanged.
- **`remote` feature deleted.** The raw-libp2p alert gateway
  (`subsystems/distributed.rs`), its `distributed_alert_consumer` example, the
  `remote_integration` test, and the `libp2p` dependency are removed. The
  gateway subscribed to the (now-deleted) swarm's `alert_bus` and published
  `ArbitrageOpportunity`; reframing it needs a domain-neutral coalition-event
  broadcast surface that doesn't exist yet, so it was deferred to [#38] (a
  domain-neutral remote coalition-event gateway) rather than reworked here.

### Changed

- **`examples/synthetic_ingestion.rs` is now the flagship demo**: after pumping
  the two synthetic sources through generic `SampleMonitor`s, it forms a
  coalition over the ingested sensor agents and drives a policy-gated join
  through the `CoalitionService` seam — end-to-end coalition formation on
  synthetic, non-financial data.
- **`src/main.rs` rewritten** as a domain-neutral reference daemon: a
  `CoalitionRuntime` forms a seed coalition of bit-capability agents and runs a
  bounded, policy-gated join loop through `CoalitionService`, then shuts down on
  ctrl-c.
- **`examples/supervised_swarm.rs` → `examples/supervised_monitor.rs`**: the
  `core::spawn_supervised` restart demo, ported from a forex `MarketMonitor` to
  a generic `SampleMonitor<SensorEvent>` over synthetic data.

### Migration

The `databento` (0.10.0) and now `remote` cargo features and the public
`subsystems::{swarm, coordinator, sink, monitor, distributed}` and `market`
modules are gone. Market/trading work lives in the sibling
[`biome`](https://github.com/sustia-llc/biome) project; the deleted forex code
is recoverable from the `v0.10.0` tag.

## [0.10.0] — 2026-07-14

### Removed

- **Databento (DBN) adapter dropped.** koalisi is a domain-agnostic
  coalition runtime; the market-data decode path is now owned entirely by
  the sibling [`biome`](https://github.com/sustia-llc/biome) project (which
  carries its own `load_prices_dbn`). Deleted: `subsystems/databento.rs`
  (the MBP-1 → `Tick` `SwarmFeeder` pump), the `databento_historical` and
  `databento_live_replay` examples, and `tests/databento_integration.rs`.
  Dropped the `databento` feature and its `dbn` + `time` dependencies
  (`time` was databento-only). The domain-neutral `ingest::{Pacing,
  PumpStats, DataSource, pump_source}` layer (issue #8) is unaffected — it
  was already the generalisation and no longer name-drops the removed
  adapter. Follow-up databento ideas (issues [#22] LiveClient, [#23]
  synthetic DBN arb) are closed here as out-of-scope; that work belongs in
  `biome`. **Breaking** for anyone building `--features databento`.
- First step of a broader de-financialisation pass; forex (`market`,
  `subsystems::{coordinator,sink,swarm}`, triangular arbitrage) removal is
  planned separately (`.claude/plans/`).

## [0.9.0] — 2026-07-04

### Added (issue [#30] P7.2 topology projection + replay)

- **Event tap on `TemporalHypergraph`** (always compiled, inert by default):
  `with_event_tap(mpsc::Sender<TemporalEvent<V,HE>>)` — non-blocking
  `try_send`, drop-with-warn (the K3 tap contract); `Clone` shares the tap;
  every append site taps **under the events write guard**, so tap order is
  always identical to log order even with concurrent mutators. Zero change
  to untapped behavior (default/frozen suites byte-identical).
- **Wire projection** ([#30], feature `persistence`):
  `WireTopologyEvent<VW, HW>` — serde mirror of all 13 `TemporalEvent`
  variants with raw `u64` indices/timestamps and
  `WIRE_TOPOLOGY_SCHEMA_VERSION = 1`; inherent
  `from_event`/`try_into_event` conversions (`V: Into<VW>` /
  `VW: TryInto<V>`; identity projection `VW = V` for serde-capable weight
  types). Domain types still gain no serde (§4 discipline).
- **Topology forwarder** `spawn_topology_forwarder`: tap → wire → CBOR
  payload → `Record` on the `Topology` stream via the P7.1 store writer.
  Backpressured internal hop; documented lossless (drop-tap-then-drain) vs
  prompt (token-cancel, may drop in-flight) shutdown disciplines.
- **`replay_into_event_log(store, from)`**: batched Topology-stream read to
  head → decode → rebuild a fresh `EventLog` that all existing consumers
  (`TemporalQueries`, `TemporalAnalytics`, `magnitude_history`) run on
  unchanged — the §7 one-query-path guarantee. Rejects future
  `schema_version` and `Sealed` payloads; requires a quiescent pipeline
  (documented).
- **Acceptance held**: all-13-variant round-trip through the store;
  point-in-time reconstruction equality live-vs-replayed; and the
  pre-registered #18 **parity gate** — `magnitude_history` over a replayed
  log is exactly equal to the live series (`tests/replay_parity.rs`,
  features `persistence,magnitude`). Suites: 87 default / 107 `persistence`
  / 130 `persistence,magnitude` / 128 `decision,magnitude`.

## [0.8.0] — 2026-07-04

### Added (issue [#29] P7.1 persistence core — first Phase 7 implementation phase)

- **New `src/persistence/` module behind feature `persistence`** ([#29];
  deps `ciborium` + `sha2`, both gated — default build unchanged; ciborium
  picked over minicbor per design-doc §17, the §3 hash-over-stored-bytes
  contract makes encoder determinism non-load-bearing):
  - The signed-off §6 surface verbatim: `EventStore` trait
    (`append`/`read_from`/`head`/`verify`), envelope types (`StreamId` ×6
    incl. reserved `Lineage`, `SequenceNo`, `RecordHash`, `Payload::{Plain,
    Sealed}`, `EventRef`, `Record`, `StoredRecord`, `StreamHead`),
    `PersistenceError` (hand-rolled, incl. review-added `StreamWedged`).
  - `FileEventStore`: CBOR frames via a private `FrameV1` mirror (public
    types stay serde-free), per-stream segment directories, u32-LE
    length-prefixed frames, SHA-256 chain over the exact stored frame bytes
    (length prefix excluded — verification never re-encodes), segment
    rotation with cross-segment chain continuity, torn-tail truncation at
    the last segment's tail only, tail re-verification on open,
    **wedge-on-write-failure** (append after a failed write returns
    `StreamWedged`; reopening re-scans and recovers), bounded random-access
    frame reads (corrupted length prefix cannot force a giant allocation).
  - `spawn_store_writer`: mpsc-fed writer task — `spawn_blocking` appends,
    biased cancellation, drain-on-cancel. **At-most-once from the tee
    onward** (a failed append is logged and dropped; K3's cursor-replay
    at-least-once has no P7.1 analogue).
  - `Payload::Sealed` is schema-only until P7.3 (round-tripped opaquely);
    `Lineage` waits for [#20].
- Design doc truth-sync in the same PR: §3 hash-input clarification (prefix
  excluded), §6 gains `StreamWedged` + `FileStoreConfig`, writer-seam
  delivery corrected to at-most-once.
- Tests: +8 unit (envelope/chain/store wedge contract) and +7 integration
  (`tests/persistence_integration.rs`: roundtrip, rotation + reopen,
  tamper detection, torn-tail recovery, sealed opaque round-trip, writer
  drain-on-cancel, empty/bounds semantics). Suites: 87 default / 102
  `persistence` / 128 `decision,magnitude` (default and existing suites
  unchanged).

## [0.7.0] — 2026-07-04

### Added (issue [#21] Phase 7 persistence design + issue [#18] magnitude trajectory)

- **`.claude/docs/phase7-persistence-design.md`** ([#21], the Phase 7 RE-PLAN
  deliverable — design only, no `EventStore` code ships in 0.7.0): layered
  architecture with a portable CBOR append-only hash-chained log as source
  of truth (the K3 `durable` bus becomes an optional, rebuildable live
  projection); envelope record model over six independently chained streams
  (`Topology`/`Decisions`/`Beliefs`/`Lineage`(reserved, [#20])/`Registry`/
  `Provenance`); crypto-deletion via per-subject KEK destruction with the
  keystore outside the log (Stroh §7.4 data-layer pattern only); revocation
  as appended `Registry` events; bilateral manifest-gated federation
  (explicitly not the buses / libp2p gateway); EPP-`EffectLog`-shaped causal
  parents on the envelope; FAIR run-provenance stream. Implementation is
  phased P7.1–P7.5 as follow-up issues; open calls recorded in §17.
- **`TemporalAnalytics::magnitude_history` + `MagnitudePoint`** ([#18],
  feature `magnitude`): the magnitude-trajectory salvage — change-driven
  replay of a coalition's membership + member weights along the
  event-sourced history, evaluating the pinned `t = 1`
  `catgraph_magnitude::coalition_value` fresh at each sample point
  (`CoalitionEvaluator` deliberately not used: the trajectory access pattern
  misses its base key at every sample, and a rebuild costs ≈10–15× one
  fresh eval). Single chronological pass under one read guard; magnitude
  evaluations in one `tokio_rayon` offload; upstream errors become
  `NEG_INFINITY` points, never panics. `HyperedgesCleared`/`GraphCleared`
  dissolve the trajectory (documented divergence from
  `hyperedge_vertices_at`, which cannot see clear events). `relevant_masks`
  and `magnitude_or_zero` promoted to `pub(crate)` (visibility + docs only —
  decision-arm behavior untouched). 6 hand-computed seeded tests in
  `tests/magnitude_trajectory.rs`; suites 87 default / 109 `magnitude` /
  128 `decision,magnitude`.

### Added/Changed (issue [#8] domain-neutral ingestion, K5)

- **New `src/ingest/` module (always compiled, zero new deps)** ([#8]): the
  ingestion layer is now domain-neutral; forex is one instantiation.
  - `Sample` trait (`Key` routing + `timestamp_ms` + distilled `View`) and
    `SampleMonitor<S>` — the former `MarketMonitor` logic ported
    verbatim-generic (ring-buffer window, latest view, publish-on-ingest,
    wrong-key drop-with-warn). The K3 runtime contracts carry over unchanged:
    `feed` = acknowledged ingest-then-publish flush primitive, `tell` =
    fire-and-forget, `Ping` = mailbox barrier, broadcast publish errors
    ignored.
  - `DataSource` trait (time-ordered pull; async-fn-in-trait,
    dyn-incompatible by design), domain-neutral `Pacing::{Asap, Realtime}`
    (moved out of the databento adapter, re-exported there), and
    `pump_source`/`spawn_source_pump` — key-routed feeding with
    `PumpStats { fed, dropped }` and token cancellation.
  - **Synthetic fixture sources** (deterministic inline SplitMix64, no `rand`,
    no credentials): `MultiResolutionSource` — NEST-shaped multi-resolution
    numeric series (per-series `step_ms`, merged in global timestamp order;
    models the planning-period ↔ hourly resolution gap) — and
    `SensorEventSource` — tauhokohoko-shaped sensor-event streams with a
    configurable changepoint (mean shift after `shift_at`), the SPRT-suitable
    shape. Fixtures only; SPRT itself stays downstream.
- **Forex re-expressed atop the generic core (breaking)**: `MarketMonitor` /
  `MonitorHandle` / `MonitorSnapshot` are now type aliases of the generic
  types; `TickUpdate` is `SampleUpdate<Tick>` — field renames `pair` → `key`,
  `quote` → `view` ripple through consumers. Runtime behaviour, method names
  and error strings are identical; the triangular-arbitrage domain example
  stays fully working (its examples + integration tests are the guard).
  databento remains a feature-gated adapter; its DBN pump is unchanged (its
  `PumpStats` keeps the decode-oriented fields its tests assert) and now
  shares `ingest::Pacing`.
- **Acceptance (#8)**: coalition/topology tests and the A/B harness run on
  synthetic non-financial data with no databento dependency —
  `tests/ingestion_integration.rs` (default features) pumps both fixture
  shapes through generic monitors and drives `CoalitionManager` +
  `ThresholdPolicy` from sensor-derived capabilities;
  `examples/synthetic_ingestion.rs` demos both shapes end-to-end.
- **Hot-path no-regression evidence** (review-driven): `Sample::key` returns
  `&Self::Key` — an earlier owned-key draft cloned the key per ingested tick
  (two `String` allocs for `Pair`) and measurably bumped the K3 bench's alert
  RTT median to ~11.9 µs; the by-ref design restores exact pre-K5 allocation
  parity, re-measured at 8.90 µs / 118.7k ticks/s vs the committed K3 baseline
  9.02 µs / 120.2k (`.claude/docs/k3-hot-path-bench.md`) — within run noise.
  Signature notes: `pump_source`/`spawn_source_pump` are hasher-generic
  (`H: BuildHasher`), the synthetic sources take `&[…Spec]`, and
  `ArbitrageCoordinator::ingest` takes `&TickUpdate`.
- Suite: 87 default / 106 `decision` / 103 `magnitude` / 122 both (+10:
  5 synthetic-source, 2 pump, 3 ingestion-integration; the 2 monitor unit
  tests moved to the generic module).

[#8]: https://github.com/sustia-llc/koalisi/issues/8

### Changed (issue [#14] evaluator hot path, K6)

- **Magnitude decision hot path adopts `catgraph_magnitude::CoalitionEvaluator`**
  ([#14], downstream of catgraph#31, dep bumped `v0.1.1` → `v0.2.0`;
  `catgraph-applied` bumped in lockstep — the tag split made cargo compile the
  catgraph repo twice, and the upstream `v0.1.1 → v0.2.0` delta touches only
  `catgraph-magnitude`, so the topology crate is source-identical).
  `MagnitudePolicy` and `MagnitudeValueCalculator` hold a membership-keyed
  evaluator cache behind interior mutability (`Arc<Mutex<…>>` — the
  `CoalitionDecisionPolicy`/`ValueCalculator` seams take `&self`): a join
  against an unchanged coalition answers `Mag(S ∪ {x})` via the `O(m²)`
  incremental `value_with` instead of two fresh `O(m³)` evaluations, keyed on
  `(required, ordered member masks)` with a `REGISTRY_CAP`-bounded (256)
  candidate-mask registry retained across rebuilds (measured: scoping the
  registry to one requirement degenerates to rebuild-per-decision and regresses
  below the pre-K6 baseline).
- **Decision behavior is bit-frozen** — the K4 battery re-run
  (`docs/ab-report-K4-catgraph-evaluator.md`) matches
  `ab-report-K4-catgraph.md` seed-for-seed on every quality column, and a
  lockstep probe found 0/8068 `act` divergences. This required a **knife-edge
  fresh fallback** (`KNIFE_EDGE_REL_BAND = 1e-6` relative): mathematically-zero
  margins (subsumed/redundant candidates) are decided by float noise under the
  fresh path, and the upstream rank-order-identity contract does not protect a
  sign-vs-zero threshold comparison — in-band margins recompute the `with` side
  fresh, reproducing the committed noise bit-exactly (16/8068 decisions flipped
  without it). `base_value()` is bit-identical by the upstream contract, pinned
  by a populated-registry test.
- **Latency**: mag median 3.915 → 3.658 µs (AIF 1.387 µs) — Path A (v1 speed
  route) still missed, dual verdict `FALSIFIED (latency)` / `VALIDATED (B)`
  unchanged. Per-decision profile committed in the report (the pre-registered
  catgraph#33 evidence): the pure incremental hit path is at AIF parity
  (1.35 µs) but ~62% of cache hits are knife-edge (decision-freeze tax, 5.05 µs)
  and evaluator construction costs ~10–15× a plain fresh evaluation (~30 µs,
  the #33 scratch-buffer target). Leave path stays fresh (upstream non-goal;
  the reduced-set-evaluator variant B measured slower and ships opt-in via
  `MagnitudePolicy::with_evaluator_leave`).
- **API**: `MagnitudePolicy` / `MagnitudeValueCalculator` lose `Copy` (the
  cache is an `Arc`; `Clone` shares it) and gain `new` constructors — struct
  literals no longer work outside the crate. `t = 1` stays pinned; the
  capability→coupling mapping, join/leave rules, and battery protocol are
  untouched.
- Suite: 77 default / 96 `decision` / 93 `magnitude` / 112 both (+7 magnitude
  tests: seeded decision-equality guards for both leave variants, cache
  invalidation, shared-clone-cache, populated-registry bit-identity, knife-edge
  flip regression, error-path plumbing).

[#14]: https://github.com/sustia-llc/koalisi/issues/14

### Changed (issue [#6] messaging swap, K3)

- **In-process actor seams swapped from kameo to `tokio::sync`** (hybrid, hot
  seams never touch a DB). The forex workers (`MarketMonitor`,
  `ArbitrageCoordinator`, `AlertSink`) are now plain state structs driven by
  spawned tasks:
  - **Pub/sub buses** (`tick_bus`, `alert_bus`) → `tokio::sync::broadcast`
    (capacity 1024). Subscription is synchronous/immediate, so the old
    "subscriber must be spawned before `Subscribe`" ordering gotcha is gone. A
    subscriber that falls > capacity behind gets `RecvError::Lagged` and skips
    the overflow (acceptable for ticks; alert consumers stay ahead).
  - **Ask/tell** → `tokio::sync::mpsc` command enums with `oneshot`-correlated
    replies; fire-and-forget ticks are plain mpsc sends. `Ping`/`GetQuotes`/
    `GetAlerts` first *drain* the buffered broadcast so they remain deterministic
    flush barriers (reproducing the old kameo FIFO-mailbox guarantee across the
    two channels). `flush()` pings monitors → coordinator → sink in order.
  - Handles (`MonitorHandle`, `CoordinatorHandle`, `SinkHandle`) wrap the mpsc
    sender and expose typed async methods; `Swarm` wires the tasks under its
    existing `CoalitionRuntime` `TaskTracker` + child cancellation tokens, so
    the three-step shutdown (cancel → close → wait) drains the whole swarm in
    one call.
- **`CoalitionActor` → `CoalitionService`** (file keeps its `coalition_actor.rs`
  name): the decision seam (issue #1) is now a task + mpsc/oneshot handle
  (`join`/`leave`/`members`) instead of a kameo actor. Behavior identical — it
  still owns the `CoalitionManager` + `Box<dyn CoalitionDecisionPolicy>` +
  `DecisionContext` and consults the policy via its async offload before
  mutating membership. The `CoalitionDecisionPolicy` surface is untouched.
- **Thin task-restart layer** (`core::supervision::spawn_supervised`), replacing
  kameo's `OneForOne`: a supervisor task rebuilds a fresh task instance from a
  factory on panic, up to `restart_limit` restarts within a sliding `window`
  (cancellation is not a failure). `examples/supervised_swarm.rs` is rewritten
  on it — the old ActorId-reuse gotcha (gotcha #1) is obsolete; a restart is now
  simply the factory building a new instance.
- **API deltas**: `SwarmConfig` drops its `delivery_strategy` field (broadcast
  has no selectable strategy; the `config` key is retained but ignored).
  `Swarm::{monitor,coordinator,sink}` return `*Handle`s (not kameo `ActorRef`s);
  `Swarm::{tick_bus,alert_bus}` return `&broadcast::Sender<_>` (call `.subscribe()`
  for a `Receiver`). `MonitorSnapshot` moved onto `MarketMonitor::snapshot`.
- **Remote gateway ported to raw libp2p; `kameo` + `kameo_actors` REMOVED**
  (stage 2): `RemoteAlertGateway` is now a plain tokio task on the swarm's
  `TaskTracker` speaking libp2p `request-response` (CBOR codec, protocol
  `/koalisi/alerts/1`; wire enums `AlertRequest::{Poll, PeekCount, Clear}` /
  `AlertResponse`), consuming the alert bus via `broadcast::Receiver`. mDNS
  discovery unchanged; the protocol name is the service identity (no kameo
  registry / `gateway_name`). New `RemoteAlertClient` (mDNS or explicit dial)
  replaces the kameo `RemoteActorRef` lookup. Deltas: `Poll` still clones
  without draining; `Clear` returns a bare ack (was a dropped count);
  `RemoteHandle` is `{ local_peer_id, listen_addrs }`; the process-wide
  `init_global()` constraint is gone (producer + client can share a process);
  mDNS expiry no longer force-disconnects the peer (deliberate — an active
  polling client keeps its connection; idle ones close via the 300s idle
  timeout).
  `remote = ["dep:libp2p"]`; libp2p features gain `request-response, cbor`.
- **Durable decision messaging behind a new `durable` feature, off by default**
  (stage 3): dep `surrealdb-live-message` git tag `v0.2.0` (SSH; cargo key
  remapped via `package = "surrealdb_live_message"`) — the two-tier durable bus
  (CHANGEFEED log + LIVE wake-up + `SHOW CHANGES` versionstamp-cursor catch-up;
  at-least-once, restart-durable).
  - Feature-independent tap: `CoalitionService::spawn_with_tap` emits plain
    `DecisionRecord { coalition, agent_id, kind: Join|Leave, act, score }` on
    every policy-consulted decision via non-blocking `try_send` (a full/closed
    tap never stalls the decision path). No surrealdb types in core.
  - `subsystems::durable` (gated): `DecisionEvent` (`SurrealValue` derive,
    crate path pinned via `#[surreal(crate = "::surrealdb_types")]` — needs the
    small `surrealdb-types` value crate), `DurableDecisionBus` wrapping the
    upstream `Coalition<DecisionEvent>`, `spawn_decision_forwarder` (tap →
    durable log-sink agent). Dynamic koalisi membership is recorded IN the
    event stream (upstream agent sets are fixed at construction) — the event
    log is the membership record, consistent with koalisi's event sourcing.
  - Upstream `SETTINGS` resolves from the consumer's cwd (`config/default.toml`
    + `RUN_MODE` overlay + env): koalisi's `config/default.toml` gains `[sdb]`
    and `[docker]` sections (inert feature-off).
  - `tests/durable_integration.rs`: container-backed (bollard/Docker) proof
    that a decision published while the consumer is down is replayed on
    restart — **the pre-registered "durable messaging survives restart"
    acceptance**. `examples/durable_decisions.rs` shows the end-to-end story.
- **Hot-path bench** (`examples/hot_path_bench.rs`, run on both runtimes —
  see `.claude/docs/k3-hot-path-bench.md`): alert round-trip median 22.5 → 9.0 µs,
  p99 56.1 → 26.0 µs; ask round-trip median 7.6 → 7.5 µs, p99 17.3 → 13.3 µs;
  throughput 77.2k → 120.2k ticks/sec. **Not regressed** (every metric
  improved) — the pre-registered acceptance.

### Changed (issue [#4] catgraph backend, K1)

- **Topology backend swapped**: `TemporalHypergraph`/`SharedGraph` re-backed
  from yamafaktory `hypergraph` v4.2.0 onto `catgraph_applied::Hypergraph`
  (git tag `v0.1.1` — the catgraph#23 container, purpose-built from koalisi's
  call-site survey). Direct swap, no feature flag (staging deviation approved
  on [#4]); the yamafaktory dep is **dropped**. `CoalitionManager` / decision
  seams unchanged; tokio-rayon executor path intact.
- **API deltas**:
  - `TemporalError`/`TemporalResult` drop their `<V, HE>` generics (they
    existed solely for yamafaktory's generic error; catgraph's
    `HypergraphError` is non-generic).
  - `topology::{VertexTrait, HyperedgeTrait}` are now koalisi-local
    blanket-impl aliases: `Copy + Eq + Debug + Send + Sync` (relaxed from the
    old backend's additional `Display + Hash` and `Into<usize>`; `Send + Sync`
    carried over, now explicit).
  - Behavior: no-op updates return `Ok` (yamafaktory errored `…Unchanged`) —
    `CoalitionManager::try_join_coalition`'s documented idempotency is now
    true, guarded by `rejoin_existing_member_is_idempotent`; clears are
    infallible.
- **K4 backend-parity re-run (pre-registered on [#7])**:
  `docs/ab-report-K4-catgraph.md` — every per-seed quality number, churn,
  oracle, t-sweep value and both verdicts byte-identical to the yamafaktory
  report; only the backend header + machine-varying latency lines differ.
  Parity: PASS (as predicted — the decision path never touches the topology
  backend).
- Suite grows by the idempotency guard test: 68 default / 87 `decision` /
  77 `magnitude` / 96 both / +4 `databento` / +1 `remote`. Also fixed a
  pre-existing `clippy::derivable_impls` on `databento::Pacing` (0-warning
  parity restored under current clippy).

[#4]: https://github.com/sustia-llc/koalisi/issues/4

### Added (issue [#7] A/B harness, K4)

- **Pre-registered A/B harness** ([#7], `examples/strategy_comparison.rs`, now
  `required-features = ["decision", "magnitude"]`): Part 1 keeps the original
  Threshold-vs-AIF divergence demo unchanged; Part 2 benchmarks
  `AifDecisionPolicy` vs `MagnitudePolicy` over the #7-pre-registered battery —
  30 SplitMix64-seeded instances (no `rand` dep), pools of 4–16 agents over an
  8-bit capability universe, 20-task streams with seeded arrival orders,
  unconditional first-arrival bootstrap (AIF cannot self-start from empty),
  one leave sweep. PRIMARY = completion-rate × coverage-efficiency; oracle
  regret (brute force, pools ≤ 8); churn + warm sync-path latency secondaries;
  exploratory t-sweep (t ∈ {0.5, 1, 2, 10}) via an example-local policy — the
  library arm stays pinned at t = 1.
- **Committed report** `docs/ab-report-K4-yamafaktory.md` (yamafaktory backend,
  pre-K1, release build; deterministic except latency). Result: magnitude arm
  superior on the primary metric in **30/30 seeds** (median 0.4469 vs 0.1898),
  churn 8 vs 113, oracle regret 0.1156 vs 0.3757 — but median per-decision
  latency 4.37 µs vs 1.48 µs, so the pre-committed **verdict is
  `FALSIFIED (latency)`** (criterion 1 pass, criterion 2 fail; nothing tuned).
- **Criterion amendment v2** (#7 comment 2026-07-02, posted before any re-run;
  run 1's recorded v1 verdict stands): VALIDATED iff Path A (original v1 speed
  route) OR Path B (quality dominance — median ≥ 1.25× AIF, superior in ≥ 60%
  of seeds, latency ≤ 10× AIF). The harness now prints **both** verdicts; run 1
  under v2 is `VALIDATED (B)`. Latency follow-up filed as catgraph#31
  (incremental/paired magnitude evaluation — non-gating, strengthens Path A).
- **Upstream find (resolved):** the battery surfaced a debug-only panic in
  `catgraph-magnitude v0.1.0` (over-strict triangle-inequality `debug_assert`,
  ULP noise on non-dyadic couplings) — filed catgraph#29, fixed by catgraph
  PR #30, tagged `v0.1.1`. **koalisi dep bumped `v0.1.0` → `v0.1.1`**; debug
  builds run clean. The harness runs `--release` for the latency criterion.

[#7]: https://github.com/sustia-llc/koalisi/issues/7

### Added (issue [#5] magnitude decision arm, K2)

- **`MagnitudePolicy` — the categorical A/B mirror of the AIF arm** ([#5],
  new feature `magnitude`, independent of `decision`): coalition join/leave
  decisions scored by **coalition magnitude** (effective-member diversity)
  instead of expected free energy.
  - New dep `catgraph-magnitude` (git tag `v0.1.0`, SSH URL — catgraph is
    private, same rationale as the `aif` dep; never a path dep). Feature-off
    builds compile none of it.
  - `src/decision/magnitude_policy.rs`: `MagnitudeValueCalculator` (impl
    `ValueCalculator`; value = `catgraph_magnitude::coalition_value` at the
    pinned `t = 1` Shannon-diversity arm) + `MagnitudePolicy` (impl
    `CoalitionDecisionPolicy`, sync + rayon-offloaded async — the whole
    `O(m³)` magnitude computation runs on the rayon pool) + `CouplingModel`
    (the mapping, public for direct testing).
  - **The capabilities→coupling mapping** (the semantic heart): directed
    substitutability `A(i→j) = |rel_i ∩ rel_j| / |rel_i|` over
    required-masked capability bits. Capability clones are mutually coupled
    at `1.0` and **skeletalize** into one effective agent (deliberate mirror
    of the AIF arm's clone degeneracy); subsumed agents get Möbius weight 0;
    disjoint specialists count fully. **Task-irrelevant agents
    (`rel == 0`) are excluded from the member set** — a vacuous `1.0`
    coupling would drive their Möbius weight negative and collapse coalition
    diversity (code-review finding, hand-verified: 3 specialists + 1
    bystander scored 1.0 instead of 3.0 and ejected a unique specialist on
    leave).
  - Upstream `CatgraphError`s are policy-level outcomes (decline / `-∞`
    value), never panics. Dedup by `agent_id` before every upstream call.
  - Tests: 9 unit (upstream seam pins — chain `0.7/0.5 ⇒ Mag(1) = 1.80`,
    mutual-1.0 skeletalization ⇒ `1.0`; mapping table; hand-computed
    calculator values `3.0`/`4/3`/`1.0`; join/leave mirrors; bystander
    regression; Err path; async ≡ sync incl. through
    `Box<dyn CoalitionDecisionPolicy>`). Suite: 67 default / 76 `magnitude`
    / 86 `decision` (unchanged) / 95 both.

[#5]: https://github.com/sustia-llc/koalisi/issues/5

### Added (issue [#1] decision wiring)

- **Live decision call site** ([#1]): the `AifDecisionPolicy` / async-offload
  primitives shipped in v0.6.0 now have a real seam.
  - `CoalitionManager::{try_join_coalition, try_leave_coalition}`
    (`src/topology/coalitions.rs`, gated `where V: AgentCapabilities`):
    consult a `&dyn CoalitionDecisionPolicy` over the coalition's current
    membership and a real `DecisionContext`, then apply the mutation iff the
    policy returns `act`. The CPU-bound part runs through the policy's async
    offload (`should_{join,leave}_async`), so the runtime worker is not blocked
    even for the AIF policy.
  - `subsystems::coalition_actor::CoalitionActor<V, HE>` — kameo actor owning a
    `CoalitionManager`, a `Box<dyn CoalitionDecisionPolicy>`, and a
    `DecisionContext`. `JoinRequest`/`LeaveRequest`/`Members` messages drive
    policy-gated membership. `AifDecisionPolicy` is never named here — the AIF
    strategy and `ThresholdPolicy` are interchangeable behind the trait object.
  - `tests/decision_integration.rs` (4 feature-off + 1 feature-on): apply /
    decline / force-leave through the actor, the manager primitive in isolation,
    and AIF non-degeneracy through the live actor (an agent covering a new
    required bit joins; a redundant clone does not).
- **`AgentCapabilities: Send + Sync`** (`src/algorithms/mod.rs`): lets
  `&dyn AgentCapabilities` capability views cross `.await` points / threads, as
  the async decision seam requires. All concrete agent types are `Copy` data, so
  the bound is satisfied for free.

[#1]: https://github.com/sustia-llc/koalisi/issues/1

### Added (issue [#2] belief-aware scoring)

- **Trust / compatibility / history beliefs in the AIF decision path** ([#2],
  feature `decision`): the decision now reflects more than capability coverage.
  - `koalisi::decision::{TrustBeliefs, CompatibilityBeliefs, CoalitionHistory}`
    re-exported from `aif` (plain `f64`/`HashMap`, no new deps).
  - `BridgeParams.belief_weight` (default `0.0`) blends a belief *alignment*
    scalar (from `aif::belief_weighted_preference`) into the **competence** that
    drives the POMDP observation model:
    `competence = (1 - belief_weight)·coverage + belief_weight·alignment`. At the
    default `0.0` the policy is pure coverage — behavior is byte-for-byte
    unchanged. Because beliefs modulate the *observation model* (not just
    preferences), the decision stays **non-degenerate** (the B2/B4 requirement):
    membership still alters achievable `G`.
  - `AifDecisionPolicy` now carries `trust`/`compat`/`history` (constructible via
    `AifDecisionPolicy::with_beliefs`). With `belief_weight > 0`: a
    capability-redundant but well-trusted agent can now join; a coverage-improving
    but badly-distrusted partnership can be declined; recorded coalition history
    shifts the margin. The async (rayon) path is preserved — belief lookups happen
    in the sync prologue, only the EFE math is offloaded.
  - Trust reconciliation: `AgentCapabilities::trust_level()` is the *static*
    baseline; `TrustBeliefs` is the *dynamic* EMA-learned signal. They are
    complementary, not duplicated — koalisi has no `TrustGraph`.
  - Tests: 6 new policy unit tests (belief-weight-zero parity, high-trust join,
    low-trust block, history flip, leave control, sync/async equivalence) + 1
    integration test driving a belief-aware join through the live `CoalitionActor`.

[#2]: https://github.com/sustia-llc/koalisi/issues/2

### Added (Phase 5 prep)

- **`koalisi::llm` stub module** (`src/llm/mod.rs`): defines the
  `LlmProvider` trait (`fn complete(&self, prompt: &str) -> impl
  Future<Output = anyhow::Result<String>> + Send`) plus
  `StubLlmProvider` that returns `"no LLM backend configured yet"`.
  Plan documents and future Phase 5/6 code reference `LlmProvider`;
  real backends (OpenAI / Anthropic / Ollama / local llama.cpp) land
  later behind a future `llm` feature flag with per-backend
  sub-features. One unit test covers the stub error path (default
  test count: 26 → 27).

## [0.6.0] — 2026-05-29

Phase 6 — a **pluggable, optional** Active Inference coalition-decision
layer, built on the code-reviewed `aif` reference engine (the `tira`
repo, tag `aif-v0.4.0`). AIF is never forced on all swarms: it coexists
with a non-AIF baseline behind a trait, selectable per swarm. This does
**not** port the retired `coalition_aif` prototype (its AIF math was
buggy); it bridges koalisi capabilities to the correct engine.

### Added

- **`decision` module** (`src/decision/`, `mod.rs` always compiled):
  - `CoalitionDecisionPolicy` trait — object-safe; `should_join` /
    `should_leave` plus dyn-compatible `should_join_async` /
    `should_leave_async` (boxed-future variants) over
    `&dyn AgentCapabilities` + `&[&dyn AgentCapabilities]` +
    `&DecisionContext { required_capabilities: u32 }`, returning
    `Decision { act: bool, score: f64 }`.
  - `ThresholdPolicy<C: ValueCalculator>` — always available, non-AIF
    baseline; joins when a candidate's marginal coalition value clears a
    threshold. Reuses the existing `ValueCalculator` impls unchanged.
- **Optional `decision` feature** (`decision = ["dep:aif"]`):
  - `aif = { git = "ssh://git@github.com/sustia-llc/tira", tag =
    "aif-v0.4.0", optional = true }` (SSH because `tira` is private and
    git is SSH-only). Feature-off builds compile **no `aif` and no
    `nalgebra`**; koalisi itself adds no matrix library.
  - `EfeValueCalculator` — impl of the EXISTING `ValueCalculator`;
    coalition value = `−G` (negated expected free energy). Slots
    alongside Additive/Synergistic/Multiplicative/Weighted.
  - `AifDecisionPolicy` — impl of `CoalitionDecisionPolicy`; joins iff
    coalition membership lowers `G`. The capability→EFE bridge maps
    coverage of `required_capabilities` to a 2-state POMDP
    observation-model precision (built via `aif::POMDPAgent` directly),
    so membership alters `G` (not just preferences) and the decision is
    non-degenerate (verified monotone; unit-tested). CPU-bound EFE is
    offloaded to the rayon pool via `tokio-rayon`; the async trait
    methods keep `Box<dyn CoalitionDecisionPolicy>` non-blocking.
- **`examples/strategy_comparison.rs`** (`required-features =
  ["decision"]`) — one join scenario under both `ThresholdPolicy` and
  `AifDecisionPolicy`, printing where they diverge.
- Tests: 30 default / 40 with `--features decision` (incl. monotonicity,
  non-degeneracy + degeneracy guards, sync/async equivalence,
  async-via-trait-object). Non-finite (NaN/±∞) margins are guarded.

### Notes

- Deferred (tracked as GitHub issues #1, #2): wiring `AifDecisionPolicy`
  into a live kameo / `CoalitionManager` call site, and recovering aif's
  `TrustBeliefs`/`CompatibilityBeliefs`/`CoalitionHistory` for richer
  scoring. See `CLAUDE.md` §"Phase 6".

## [0.5.0] — 2026-05-27

Max-effort `/code-review` sweep across the topology and algorithms
layers. Three reviewers (reuse, quality, efficiency) flagged 33
distinct findings; all applied or explicitly deferred. Includes real
API breakage — see "Breaking" below.

### Fixed

- **`TemporalHypergraph::create_snapshot` race**: the marker append and
  the snapshots-table insert now happen under a single events-log write
  guard. `Snapshot::event_index` correctly points at the marker
  (previously off-by-one even single-threaded). `temporal.rs:507-531`.
- **`TemporalQueries::{vertex,hyperedge}_lifespan` double-lock race**:
  `created` and `removed` are now sampled under a single read lock via
  private `_impl` helpers, eliminating the window where a concurrent
  writer could yield an inconsistent lifespan. `queries.rs`.
- **`TemporalAnalytics::{vertex,hyperedge}_count_series` O(samples × |V|)
  re-locking**: replaced with a single-pass chronological walk that
  maintains a `HashSet<VertexIndex>` of live entities and emits counts
  at sample boundaries. One lock acquisition per call instead of one
  per sample. `analytics.rs`.
- **`TemporalAnalytics::delta` silently dropped clears**: `HyperedgesCleared`
  and `GraphCleared` events are now recorded in two new `GraphDelta`
  fields (`hyperedges_cleared`, `graph_cleared`) instead of being
  swallowed.
- **`EventLog::events_until` ignored its own index**: now uses
  `time_index.range(..=ts)`, O(log n + k) instead of O(n).
- **`EventLog::snapshot_before` linear HashMap scan**: snapshot index
  is now a `BTreeMap<Timestamp, (SnapshotId, usize)>` so
  `snapshot_before` is O(log n) via `.range(..=t).next_back()`.
- **`CoalitionManager::{join,leave}_coalition` TOCTOU window**: replaced
  the two-round-trip read-then-write with a new atomic
  `TemporalHypergraph::update_hyperedge_vertices_try` mutator helper
  that holds the graph write lock across the membership check + update.
- **`agent_coalition_history` lock-holding scan**: now snapshots the
  events vec under the read lock, then processes outside. The linear
  scan no longer blocks concurrent writers for its duration.
- **AIPA combinatorial wastefulness**:
  - `partition_count(n)` switched from full Vec enumeration to the
    O(n²) dynamic-programming recurrence (no per-partition allocation;
    n=50 went from ~204k Vec allocations to 51 usize cells).
  - `find_best_partition` replaced sort-then-take-first with `max_by`;
    O(p(n)) instead of O(p(n) log p(n)).
  - `compute_all_partition_bounds` pre-aggregates max/avg per coalition
    size once before the partition loop instead of rescanning each
    bucket per partition.

### Changed

- **Atomics**: `Clock` and `TemporalHypergraph::snapshot_counter`
  switched from `Ordering::SeqCst` to `Relaxed` (`tick`) /
  `Acquire`/`AcqRel` (compare-exchange) — same correctness, no x86_64
  `MFENCE` on every clock tick.
- **`TemporalAnalytics::events_by_type`**: returns `HashMap<&'static str,
  usize>` instead of `HashMap<String, usize>` and routes through the
  existing `TemporalEvent::event_type()` method — eliminates one
  `String` allocation per event in range and deduplicates the 13-arm
  match.
- **`DCVCDistributor::calculate_statistics`**: returns a typed
  `DistributionStats` struct (`min`, `max`, `avg`, `total`) instead of
  an unnamed 4-tuple. Computed in a single fold (no per-share Vec
  allocation, no three-pass min/max/sum).
- **`ValueCalculator` impls**: shared helpers (`size_bonus`,
  `capability_bonus`, `trust_sum`, `combined_capabilities`) and named
  constants (`SIZE_UNIT`, `CAP_UNIT`, `SYNERGY_UNIT`, `TEAM_UNIT`,
  `TEAM_THRESHOLD`) replace the three duplicated implementations'
  magic numbers.
- **`EventLog`**: `stats()` is now O(1) (incremental on `append`)
  instead of O(total events). All five `.or_insert_with(Vec::new)`
  call sites use `.or_default()` (clippy::pedantic alignment).
- **`HypergraphExecutor`**: gained `with_num_threads(n)` constructor
  for downstream runtimes that already partition CPUs across rayon
  pools. Module + struct documentation added.
- **CLAUDE.md**: new §"Available tooling for this project" section
  pointing at the `graph` plugin v2.0.1 (hypergraph agent + six
  hypergraph skills tracking hypergraph v4.2.0 HEAD). New gate notice
  on Phases 5 and 6 noting they are pending two user design inputs.
- **Test fixtures**: extracted shared `Agent`/`Coalition`/`as_caps`
  scaffolding from `tests/{algorithms,topology}_test.rs` into
  `tests/common/{algorithms,topology}.rs`. Examples stay standalone
  per the cargo-examples idiom.

### Breaking

- `Timestamp` and `SnapshotId` inner `u64` fields are now `pub(crate)`.
  External callers must use `Timestamp::new(n)` / `SnapshotId::new(n)`
  and `Timestamp::value()` / `SnapshotId::value()` instead of tuple
  construction or field access.
- `EventStats::new()` removed — use `EventStats::default()`.
- `SynergisticCalculator::new()` removed — use
  `SynergisticCalculator::default()` or the unit-struct literal
  `SynergisticCalculator`.
- `DCVCDistributor::calculate_statistics()` return type changed from
  `(usize, usize, f64, usize)` to `DistributionStats { min, max, avg,
  total }`. Callers must replace `let (min, max, avg, total) = ...`
  with field access.
- `koalisi::logger` and `koalisi::settings` modules removed — use
  `koalisi::core::config::{setup_logging, Settings, CoalitionSettings,
  SETTINGS}` directly.
- `koalisi::topology::EXEC` no longer re-exported at the topology
  module root (now `pub(crate)` in `executor.rs`). External callers
  can construct their own pool via `HypergraphExecutor::with_num_threads`.
- 6 `TemporalAnalytics` methods and 7 `CoalitionManager` temporal-query
  methods are now `pub(crate)` (no in-crate or external callers; will
  be re-promoted when Phase 5/6 wires them up): `vertex_count_series`,
  `hyperedge_count_series`, `mutation_rate`, `most_active_vertices`,
  `most_active_hyperedges`, `events_by_type`; `was_member_at`,
  `coalition_formed_at`, `coalition_dissolved_at`, `coalition_lifespan`,
  `count_agents_at`, `count_coalitions_at`, `agent_coalition_history`.
- `GraphDelta` gained two new fields (`hyperedges_cleared`,
  `graph_cleared`). Exhaustive pattern matches on `GraphDelta` will
  need to cover them.

### Verified

- `cargo test` (default): 26 lib + 15 algorithms + 11 topology + 5
  integration + 2 doctests = **59 pass, 0 fail, 0 warnings**.
- `cargo test --features databento`: + 4 integration tests pass.
- `cargo test --features remote`: + 1 integration test passes.
- `cargo check --all-targets` across all three feature configurations:
  clean.
- All seven examples (`topology_coalition`, `algorithm_values`,
  `triangular_arbitrage`, `historical_bootstrap`, `live_pubsub`,
  `supervised_swarm`, `distributed_alert_consumer`) plus the two
  databento examples exit 0 with expected output.

## [0.4.0] — 2026-05-26

Rename + topology and algorithm layer integration. Consolidates three
prior projects into a single layered crate; `forex-arbitrage-swarm`
becomes the runtime adapter.

### Added

- **Rename**: `forex-arbitrage-swarm` → `koalisi`. The original crate
  name survives as the runtime adapter's nickname; everything else
  re-aligned under `koalisi::`.
- **`core` layer** — `CoalitionRuntime` (domain-agnostic three-step
  shutdown: cancel → close → drain) extracted from the old `Swarm`'s
  lifecycle plumbing. `core::config` consolidates `Settings`,
  `CoalitionSettings`, and `setup_logging` behind one module.
- **`topology` layer** (ported from dynamo, on top of hypergraph
  v4.2.0):
  - `TemporalHypergraph<V, HE>` — event-sourced wrapper around
    `Hypergraph` with full per-mutation event recording.
  - `EventLog<V, HE>` — chronological `Vec<TemporalEvent>` plus
    `BTreeMap` time index, `HashMap` vertex/hyperedge indices,
    `HashMap` snapshot index.
  - `TemporalEvent<V, HE>` — 13-variant enum covering every
    hypergraph mutation plus snapshot markers.
  - `CoalitionManager<V, HE>` — agent (vertex) + coalition (hyperedge)
    API: `add_agent`, `form_coalition`, `join_coalition`,
    `leave_coalition`, `dissolve_coalition`, `merge_coalitions`,
    `coalition_members`, etc.
  - `TemporalQueries` — point-in-time state reconstruction via event
    replay (`vertex_exists_at`, `hyperedge_vertices_at`,
    `count_vertices_at`, etc.).
  - `TemporalAnalytics` — delta computation, time-series sampling,
    activity ranking, mutation-rate aggregation.
  - `HypergraphExecutor` (+ `EXEC` singleton) — rayon ↔ tokio bridge
    so the std `RwLock` inside the upstream hypergraph crate doesn't
    leak across `.await`.
  - `Timestamp`, `TimeRange`, `Clock` — logical-time primitives with
    thread-safe monotonic counter.
  - `examples/topology_coalition.rs` — form/join/merge + time-travel
    walkthrough.
  - `tests/topology_test.rs` — 11 integration tests.
- **`algorithms` layer** (ported from coalesce):
  - `AgentCapabilities` trait — abstract interface (`agent_id`,
    `capabilities`, `trust_level`) so calculators stay
    domain-agnostic.
  - `ValueCalculator` trait + four implementations:
    `AdditiveCalculator`, `SynergisticCalculator`,
    `MultiplicativeCalculator`, `WeightedCalculator` (with
    `balanced` / `capability_focused` / `trust_focused` presets).
  - `DCVCDistributor` — Distributed Coalitional Value Calculation
    workload split (Rahwan & Jennings, 2007).
  - `aipa` module — Anytime Integer-Partition Algorithm (Rahwan et
    al., 2007): `generate_integer_partitions`, `compute_*_bound`,
    `compute_all_partition_bounds`, `find_best_partition`,
    `partition_count`, `verify_partition`.
  - `examples/algorithm_values.rs` — calculator comparison + DCVC
    distribution + AIPA bound enumeration walkthrough.
  - `tests/algorithms_test.rs` — 15 integration tests.
- README rewrite documenting the four-layer architecture with the
  forex domain as a working adapter, the new examples, and the four
  origin projects.

### Changed

- `forex-arbitrage-swarm`'s public surface migrated under
  `koalisi::subsystems::{coordinator, monitor, sink, swarm}` (forex
  domain) and `koalisi::market` (value types). Examples and tests
  updated to the new paths.
- `CLAUDE.md` reorganised: four-layer mission statement, new file
  inventory with topology + algorithms additions, all 11 "worth
  flagging" gotchas updated for the koalisi naming.

## [0.3.0] — 2026-05-24

### Added

- **Remote alert gateway** (`subsystems::distributed`, feature `remote`):
  - `RemoteAlertGateway` actor (`#[derive(Actor, RemoteActor)]`), locally
    subscribed to `Swarm::alert_bus()`, remotely registered via
    `kameo::remote::Behaviour` running on a libp2p swarm.
  - `#[remote_message]` asks `PollOpportunities`, `PeekOpportunityCount`,
    `ClearOpportunities` exposed over the wire.
  - `enable_remote_alerts(&swarm, RemoteConfig) -> Result<RemoteHandle>`
    one-shot entry point: builds libp2p swarm (TCP + noise + yamux +
    mDNS + `kameo::remote::Behaviour`), calls `init_global()`,
    `listen_on(...)`, spawns the event loop on `swarm.task_tracker()`
    with a child of `swarm.cancellation_token()`, registers the gateway.
- **`Serialize + Deserialize` derives on wire-bound market types**:
  `Tick`, `Quote`, `Triangle`, `Direction`, `ArbitrageOpportunity`. These
  types now double as the on-wire payload for the remote gateway.
  *(POC trade-off — a production deployment would split internal and
  wire types behind a conversion boundary.)*
- `examples/distributed_alert_consumer.rs` — single binary with
  `ROLE=producer` / `ROLE=consumer` env-var dispatch. Producer runs a
  scripted feeder + registers the gateway; consumer mDNS-discovers and
  polls remote peers every 3 s.
- `tests/remote_integration.rs` — in-process wire round-trip:
  `enable_remote_alerts` → trigger an arb locally →
  `RemoteActorRef::<RemoteAlertGateway>::lookup_all(...)` finds self →
  `ask(&PollOpportunities)` round-trips the opportunity through full
  rmp-serde + libp2p `request-response`. Asserts edge_bps and cross pair
  survive the wire.
- New `[features]` entry: `remote = ["kameo/remote", "dep:libp2p"]`.
- `libp2p = { version = "0.56", features = ["macros", "noise", "mdns",
  "tcp", "tokio", "yamux"], optional = true }` direct dependency.
- README + `CLAUDE.md` document the hybrid architecture (local mpsc hot
  path + libp2p publish-to-outside-world boundary), the 4 new "worth
  flagging" entries (#8–#11 in `CLAUDE.md`) covering libp2p quirks, and
  the canonical `cd forex-arbitrage-swarm && cargo … --features remote`
  invocation.

### Changed

- README architecture diagram now shows the optional libp2p layer
  branching off the alert pubsub.
- `CLAUDE.md` reorganised: §"Next steps" reordered (A = LiveClient,
  B = synthetic DBN, C = remote gateway hardening, D = nice-to-haves);
  done-list and file inventory updated for the new files.

### Verified

- `cargo test --features remote` — 1 new test passes (16 tests total
  with `--features 'databento remote'`).
- `cargo run --features remote --example distributed_alert_consumer` —
  producer comes up with a real libp2p peer ID, gateway registers, mDNS
  starts listening.

## [0.2.0] — 2026-05-23

### Added

- **Databento DBN-file adapter** (`subsystems::databento`, feature
  `databento`):
  - `Pacing::{Asap, Realtime { speed_factor }}` — historical-bootstrap
    vs live-replay pacing.
  - `SymbolMapper = Arc<dyn Fn(u32, Option<&str>) -> Option<Pair> + Send + Sync>`
    closure for mapping DBN `instrument_id` / symbol strings to swarm
    `Pair` values.
  - `pump_dbn_file(feeder, path, mapper, pacing, token) -> Result<PumpStats>` —
    decode + route loop with FIFO-deterministic `SwarmFeeder::feed_tick`.
  - `spawn_dbn_pump(swarm, path, mapper, pacing) -> JoinHandle<...>` —
    fire-and-forget on `swarm.task_tracker()` with a child of
    `swarm.cancellation_token()`. Joins `Swarm::shutdown()` lifecycle.
  - `mbp1_to_tick(msg, pair)` helper — converts DBN fixed-point prices
    (1e-9 scale) to f64 and nanoseconds to milliseconds.
- **`SwarmFeeder`** — clone-able feed handle exposed via `Swarm::feeder()`.
  Captures only the `MarketMonitor` `ActorRef` map; lets background
  tasks call `feed_tick` without borrowing the `Swarm` itself.
- `examples/databento_historical.rs` — decode bundled `.dbn.zst` with
  `Pacing::Asap`, inspect the monitor snapshot.
- `examples/databento_live_replay.rs` — same file but spawned via
  `spawn_dbn_pump` with `Pacing::Realtime { speed_factor: 1e6 }` +
  custom alert listener subscribed to `alert_bus`.
- `tests/databento_integration.rs` — 4 tests: fixed-point conversion,
  file decode + routing, lifecycle join, cancellation honour.
- New `[features]` entry: `databento = ["dep:dbn", "dep:time"]`.
- Optional dependencies: `dbn = { version = "0.58", features = ["async"] }`,
  `time = { version = "0.3", features = ["macros"] }`.

### Changed

- `Swarm` gained a `feeder()` accessor returning the new `SwarmFeeder`
  type.

## [0.1.0] — 2026-05-23

Initial POC release.

### Added

- **Core actor swarm** (Coalition-shaped, modeled on
  `surrealdb-live-message`'s `Coalition<T>`):
  - `MarketMonitor` — one per pair, holds a `VecDeque<Tick>` ring buffer
    (capped at `history_capacity`), the latest `Quote`, and publishes a
    `TickUpdate` to the swarm's tick pubsub on every tick. Exposes
    `GetSnapshot`, `Ping`.
  - `ArbitrageCoordinator` — subscribed to `PubSub<TickUpdate>`. Walks
    `Vec<Triangle>` on every update; when `|edge_bps|` crosses
    `threshold_bps`, publishes an `ArbitrageOpportunity` to the alert
    pubsub. Per-triangle hysteresis: requires the edge to dip back
    below threshold before the same triangle can re-fire. Exposes
    `GetQuotes`, `Ping`.
  - `AlertSink` — subscribed to `PubSub<ArbitrageOpportunity>`. Buffers
    every received opportunity; exposes `GetAlerts`, `DrainAlerts`,
    `Ping`.
  - `Swarm` — top-level orchestrator owning all actor refs, both
    pubsubs, a `TaskTracker`, and a root `CancellationToken`. Library-
    first three-step shutdown (cancel → close → drain). Public API:
    `new`, `feed_tick`, `replay_history`, `flush`, `alerts`, `shutdown`,
    plus accessor methods.
- **Market value types** (`market` module): `Pair` (with `FromStr` for
  `"EUR/USD"`-style parsing), `Tick`, `Quote`, `Triangle` (with
  validation that legs share a common quote currency), `TickUpdate`,
  `ArbitrageOpportunity`, `Direction`.
- **Reference daemon** (`src/main.rs`) — assembles a swarm with the
  EUR/USD-GBP/USD-EUR/GBP triangle, attaches a scripted live feeder,
  waits for `ctrl-c`, drains cleanly.
- **Examples (4)**:
  - `historical_bootstrap` — single-pair history replay + ring-buffer
    snapshot inspection.
  - `live_pubsub` — scripted live feed + custom listener actor
    subscribed to the alert bus.
  - `triangular_arbitrage` — full triangle replay producing +45.25 bps
    and −72.06 bps signals, exercises hysteresis-rearm-flip.
  - `supervised_swarm` — kameo `OneForOne` supervisor with
    `restart_limit(3, 5s)` on a `MarketMonitor`; uses a `oneshot`
    readiness handshake (mirror of `surrealdb-live-message`'s
    `sdb_server::SurrealDBContainer::start_and_wait`) to race-freely
    obtain the supervised child's `ActorRef` from `main`.
- **Test suite (11 tests)**:
  - 6 unit tests in `market.rs` (pair parsing, triangle validation,
    edge_bps math).
  - 5 integration tests in `tests/integration_test.rs` (end-to-end
    triangular arb wiring, hysteresis, rearm + flip, ring-buffer
    eviction, empty-triangle rejection, unknown-pair rejection).
- **Configuration**: `config/default.toml` + `config/{development,test}.toml`,
  loaded via the `config` crate with `RUN_MODE` env override. Settings:
  `logger.level`, `swarm.threshold_bps`, `swarm.history_capacity`,
  `swarm.delivery_strategy`.
- **Logging**: idempotent `tracing_subscriber` setup via
  `logger::setup()` (uses `std::sync::Once` so tests sharing the
  subscriber don't race).
- README, `CLAUDE.md` working-state document, gitignore.

### Project conventions established

- Library-first lifecycle: callers wire their own top-level shutdown
  using `swarm.cancellation_token()` + `swarm.task_tracker()`. No
  `tokio-graceful-shutdown`, no `SubsystemHandle`.
- Path dependencies on local `kameo` checkout
  (`../../agentics/kameo{,/actors}`) until the upstream API stabilises
  on crates.io.
- `--manifest-path Cargo.toml --target-dir /tmp/forex-arbitrage-swarm-target`
  convention to avoid contention with Zed's own `cargo check`.
- `timeout 30s …` wrapper on `cargo run` invocations to bound runaway
  binaries cleanly.

[Unreleased]: #unreleased
[0.9.0]: #090--2026-07-04
[0.8.0]: #080--2026-07-04
[0.7.0]: #070--2026-07-04
[0.6.0]: #060--2026-05-29
[0.5.0]: #050--2026-05-27
[0.4.0]: #040--2026-05-26
[0.3.0]: #030--2026-05-24
[0.2.0]: #020--2026-05-23
[0.1.0]: #010--2026-05-23
