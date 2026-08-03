# Pre-registration: K4 EQ4 — typed roles (typed coalition valuation)

**Status: REGISTERED.** Committed BEFORE implementation. Design-lock of
record: [koalisi #72](https://github.com/sustia-llc/koalisi/issues/72)
(owner design-lock comment, 2026-08-03, D1–D9). Pre-run amendments are
legitimate per the standing protocol (committed to this doc + posted to
#72 BEFORE any affected code); after the official run this document is
immutable.

## 1. Registration statement

- **Hypothesis (EQ4, stack E-queue entry 4):** role-typed tasks + typed
  coalition valuation add signal beyond untyped bit-coverage. On a
  role-structured world (role-matched coverage), a magnitude arm whose
  substitutability couplings are ρ-modulated through catgraph
  `coalition_typed` (T2) outperforms the frozen untyped magnitude arm;
  on the identity world it reproduces it exactly.
- **Born on:** koalisi `v0.23.0` (catgraph `v0.7.0` ×2 — PR #73,
  drift-free; `aif-v0.12.0`; MSRV 1.93). Regime: v2-style draw (the
  standing E-queue D6 decision).
- **Seeds:** `240..270`, fresh (90..120 + 150..180 remain reserved; no
  release implied).
- **Battery placement:** `examples/strategy_comparison.rs` **Part 8**;
  frozen Parts 1–7 are the byte-identity regression gate (§6 X-battery).
- **Falsifiability posture:** planted signals have repeatedly failed to
  convert into decision advantage in this lineage (gotcha 24
  rescale-not-reroute; gotcha 26 window-vs-lattice). The registered
  lever keeps relevance masks UNTYPED (§4) precisely so the test can
  fail: roles enter valuation only through coupling modulation.

## 2. World: the v2t regime (role-matched coverage)

Per-seed, the v2 prefix (`draw_prefix_v2`: pool `n ∈ 4..=16`, worker
caps `1..=6` bits, `|required| ∈ 2..=12`, seeded arrival shuffle) is
drawn first, then role draws are APPENDED off the same SplitMix64
stream (the #46/#48 shared-prefix discipline — the untyped prefix of
the stream is bit-identical to a pure-v2 draw of the same seed):

- **R = 3 roles.** Worker roles: uniform `next() % 3`, one draw per
  worker in worker order.
- **Required-bit role tags:** uniform `next() % 3` per required bit, in
  ascending bit order, per task.
- **Typed ground truth:** a required bit `b` tagged role `r` is
  **covered** iff some coalition member with role `r` holds bit `b`.
  `completed(task)` = every required bit role-matched-covered.
  `cov_eff(task)` = (role-matched-covered bits / required bits) /
  member count — the untyped formula with typed coverage.
  **PRIMARY = success_rate × mean_cov_eff** (stream-level product,
  unchanged shape).
- **Feasibility guarantee (gotcha-25 / #63 lesson):** after the role
  draws, each task is checked for role-matched feasibility in the pool
  (for every required bit `b` tagged `r`: some pool worker of role `r`
  holds `b`). Infeasible tasks get a rejection re-draw (required bits +
  tags, same stream, ≤ 1000 attempts; a still-infeasible task after
  1000 attempts is a RUN-INVALID condition — expected never at these
  draws). Per-seed re-draw counts print in the run output.
- **Identity world (for §6 X-identity):** R = 1 — every worker one
  role, every tag that role. The draw consumes the same stream draws
  (`next() % 1 == 0`), and the typed metric reduces exactly to the
  untyped one.

## 3. Arms

| arm | policy | role in this registration |
|---|---|---|
| `mag` | frozen `MagnitudePolicy` (untyped, K6 cache + knife-edge) | control — sees bit masks only |
| `mag-typed` | T2 ρ-modulated magnitude (library, §4) | **confirmatory** |
| `aif` | scalar bridge (`AifDecisionPolicy`) | untyped context baseline, non-gating |
| `arm-E1` | `PersistentAifArm`, v5 E1 config (oracle signal) | untyped context baseline, non-gating |

All arms decide over the same arrival stream with the standing battery
protocol (bootstrap first arrival, one leave sweep per task, churn =
all removals, latency record-only).

## 4. The `mag-typed` arm (D2/D4/D8, locked)

- **Lever:** koalisi's substitutability couplings
  `A(i→j) = |rel_i ∩ rel_j| / |rel_i|` (rel = caps & required,
  **untyped** — deliberately NOT re-typed; the registered question is
  whether the VALUATION layer converts role signal) are modulated
  `A′(i→j) = ρ(role_i, role_j) · A(i→j)` via
  `coalition_typed::modulate` before magnitude evaluation. Same
  knife-edge-free fresh evaluation path for base and candidate sides.
- **Oracle ρ (design-lock interpretation note):** the locked D1 world
  is role-matched coverage — the world is not drawn *with* a ρ table;
  the oracle table is DERIVED from the world semantics:
  **ρ = identity (`ρ(r,r′) = δ_rr′`)** — cross-role substitutability
  exactly 0 (wrong-role coverage counts for nothing), same-role
  substitutability unchanged. Symmetric, satisfying D4's letter; the
  planted *non-trivial* table appears only in the exploratory ρ-quality
  world (§5 E-ρq).
- **Placement (D8):** library, behind the existing `magnitude` feature
  (no new deps). Identity default (no roles supplied) **routes
  structurally to the untyped path** — bit-identity by construction.
  The typed path evaluates **fresh per decision** (no evaluator cache,
  no knife-edge surface — the gotcha-16 precedent; the K6 cache key
  `(required, member_masks)` would collide same-mask/different-role
  agents). Latency is a non-gating secondary, reported honestly.
- Task-irrelevant exclusion (`rel == 0`, gotcha K2) unchanged and
  applied BEFORE modulation — ρ never resurrects an irrelevant agent.
- Upstream error handling unchanged: `CatgraphError` ⇒ decline / `-∞`,
  never a panic.

## 5. Registered legs

- **H-T (confirmatory):** on the v2t world, seeds 240..270 —
  `mag-typed` PRIMARY median ≥ **1.25×** `mag` PRIMARY median, AND
  `mag-typed` strictly superior on ≥ **60 %** of seeds (≥ 18/30).
  Both conjuncts required.
- **E-deg (registered context, non-gating):** degraded-ρ cells —
  `ρ_off ∈ {0.25, 0.5}` (off-diagonal lifted from the oracle 0):
  how much of any H-T margin survives table mis-specification (#54
  oracle-vs-degraded discipline).
- **E-ceil (registered exploratory, non-gating, example-side):**
  typed-relevance ceiling — a variant that re-types the relevance masks
  themselves (`rel_i = caps_i ∩ bits-tagged(role_i)`), i.e. an arm that
  fully understands role-matching. Measures the total convertible
  signal; the gap between E-ceil and `mag-typed` mechanism-scopes any
  H-T failure (valuation-layer limit vs no signal present).
- **E-ρq (registered exploratory, non-gating):** the ρ-structured
  quality world (D1c) — world drawn with a planted symmetric table
  (`ρ(r,r) = 1`, `ρ_off = 0.25`); task quality =
  `cov_eff × mean pairwise ρ(role_i, role_j)` over final members
  (singleton ⇒ 1.0); untyped coverage. `mag` vs `mag-typed` (oracle =
  the planted table) on this world's PRIMARY.
- **E-T3 (registered exploratory, non-gating, example-side):**
  channel-valued couplings — `C = R = 3` channels, channel `c` carrying
  the role-`c`-restricted substitutability
  `A_c(i→j) = |rel_i ∩ rel_j ∩ tagged(c)| / |rel_i ∩ tagged(c)|`
  (empty denominator ⇒ neutral `1.0` — REGISTERED CAVEAT: the product
  collapse makes "no evidence" neutral-high, biasing cross-role
  coupling upward; one reason this leg is exploratory), collapsed with
  fixed uniform `θ = (1/3, 1/3, 1/3)` (D3) via
  `ChannelCouplings::collapse`.
- **Instrumentation (non-gating):** T1 `role_shares` sampled on each
  task's final coalition (constructed evaluator, off the decision
  path); per-run summary of role-share distribution + mixed-class
  counts in the report.

## 6. Gates (any failure ⇒ RUN-INVALID)

- **X-identity, two cells, all 30 seeds:**
  1. Identity configuration (no roles supplied): structurally the
     untyped path — acts + scores bit-identical to `mag`.
  2. Typed path, R = 3, **ρ ≡ 1** (exact 1.0 entries; `1.0·π == π` in
     IEEE): **acts + per-seed PRIMARY + churn bit-identical** to `mag`.
     Raw scores may differ in low bits (fresh vs cached evaluation
     re-associates; an out-of-band margin cannot flip and the in-band
     population is recomputed fresh on both routes — the EQ3 H-par′
     lesson: parity gates on decisions, not float bit patterns).
- **X-battery:** frozen Parts 1–7 byte-identical on every
  quality/churn/verdict line vs a fresh pre-change baseline
  (latency-only diffs permitted, standing exclusion).
- **S-fib (sanity):** `role_grid`-constructed instances (3 role-space ×
  fiber shapes, deterministic): the harness-evaluated magnitude of the
  grid coalition matches `RoleFibrationProof::expected_magnitude()`
  within the upstream-documented relative tolerance, all instances.

## 7. Verdict labels (pre-committed)

- `VALIDATED (typed roles)` — H-T passes both conjuncts, all §6 gates
  hold.
- `FALSIFIED (typed roles)` — §6 gates hold, H-T fails either conjunct.
  Mechanism-scoping via E-ceil/E-deg is reported but cannot upgrade the
  verdict.
- `RUN-INVALID` — any §6 gate fails (a corrected registration would be
  a new document, #63 precedent).

Pre-committed interpretations: the v1/v2 K4 verdicts, EQ3's verdict,
and the #54 arm question (mag = demonstrated default, FINAL) are
UNTOUCHED regardless of outcome — a `VALIDATED (typed roles)` result
speaks to the typed-vs-untyped contrast, not to the mag-vs-aif arm
question. No post-hoc bar movement. Latency is never gating in this
registration.

## 8. Report

`docs/ab-report-K4-eq4-typed-roles.md` — registered sections mirror
§5/§6; implementation/deviation ledger mandatory; appended-addendum
convention applies to any follow-up.
