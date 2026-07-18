# Pre-registration: K4-v6 — never-evict E1-persistent AIF arm, dual-signal, out-of-sample (koalisi #56)

> **RESULT (2026-07-18): `FALSIFIED (never-evict)`** — ne 0.0143/0.0141 vs the
> 0.3551 bar, superior to scalar 3/30 (both signals); churn 0 by construction.
> X-A held (0.4406/136.00 reproduced). See
> `docs/ab-report-K4-v6-never-evict.md`. The exploratory cap series shows
> quality monotone in allowed evictions — churn IS the mechanism.

_Registered 2026-07-18, BEFORE any v6 implementation or run (posted to #56).
Owner-locked design decisions (2026-07-18, in-session): lever = **never-evict
(eviction cap c = 0)**; outcome signal = **BOTH oracle and degraded, gating**;
bar = **hybrid** (relative quality gates + absolute churn ceiling). Fresh seeds
**60..90** — never used by any prior registration (0..30: v1–v4; 30..60: v5 +
the #54 Parts 4d–4g)._

## Motivation (evidence chain)

#53 (K4-v5) validated `aif-e1` on quality (0.4406 vs mag 0.2720, 30..60) with
churn 136 vs mag 8 as the adoption blocker (#54 Step 1a). #54 Step 3 (Part 4f)
proved churn is not tunable in score space — the fixed-γ=16 posteriors saturate
at ±0.5 — so mitigation must act on membership state. The owner selected the
boldest state point: **remove eviction entirely**. Churn is then **0 by
construction** and the entire registration risk sits on quality: never-evict
removes the arm's ability to trim members, so `cov_eff = coverage/|members|`
(size penalty) and `success = completed ∧ all members perform` (one flaky
member kills the task) both push against the arm. This bar is genuinely at
risk; a FALSIFIED outcome finalizes memo option B's parked state
(`docs/k4-arm-choice-memo.md` §6).

## Registered arm

`aif-e1-ne` = the #53 registered `aif-e1` configuration (E6: persistent per-bit
learning + novelty, MeanField queries at fixed γ = 16, no `PrecisionDynamics`,
engine `aif-v0.11.0`) plus ONE new lever:

- `PersistentAifConfig.eviction_cap: Option<u32>` — **identity default
  `None`** (unlimited; the #53 arm bit-for-bit, asserted). Registered point
  **`Some(0)`**: `should_leave` returns `Decision { act: false, score: 0.0 }`
  WITHOUT constructing a query (no engine call, no decision-counter
  increment on the leave path). The arm still learns normally via
  `observe_outcome`. (Fixed here, before implementation: the skip-query form
  is the registered semantics.)

Two gating instantiations on the same seeds, fresh arm + per-seed factory as
always:

- **`ne-oracle`** — outcome hook = per-bit oracle signal (the #53 contract);
- **`ne-degraded`** — outcome hook = whole-task success smeared
  (`observe_outcome(required, &[success; 8])`, the #55/L2 runtime contract).

## Battery

`examples/strategy_comparison.rs` new Part (additive; every existing printed
line byte-identical; release build). Scope B, seeds **60..90** (warm-up on
seed 60 discarded, per the range-battery convention). In-run baselines on the
same instances: `mag` (frozen `MagnitudePolicy::default()`), `scalar` (frozen
`AifDecisionPolicy::default()`), and `e1-k0` (unmodified #53 arm, oracle
signal) as the churn/quality reference — context rows, non-gating except X-A.

## Confirmatory criteria (all from THIS run's 60..90 medians)

Per signal S ∈ {oracle, degraded}:

- **H1(S) — quality vs mag**: `ne-S` median PRIMARY_B ≥ **1.25 ×** mag median.
- **H3(S) — mechanism vs scalar**: `ne-S` strictly superior to scalar on
  ≥ **18/30** seeds.

- **H2 — churn ceiling (absolute)**: `ne` churn median ≤ **68**. At c = 0 this
  holds **by construction** (churn ≡ 0); it is stated so the hybrid bar is
  explicit, and it becomes live only if implementation reality diverges from
  the never-evict semantics (which would itself be a run-invalidating bug).

**Verdict rule (pre-committed):**
- `VALIDATED (v6)` = H1 ∧ H3 under **BOTH** signals (∧ H2).
- `PARTIAL (signal-limited)` = H1 ∧ H3 under exactly ONE signal (report which;
  if oracle-only, the runtime-credibility claim fails and e1 stays
  battery-only per the Step-2 decision table).
- `FALSIFIED (never-evict)` = anything less.

Thresholds (1.25×, 18/30) inherit the v2→v5 family. Nothing is tuned to flip
the verdict; the lever, signal set, and bar were locked before implementation.

## Run-validity gates (run-invalidating)

- **X-A**: `e1-k0` (cap `None`, oracle) on seeds **30..60** reproduces the #53
  registered numbers exactly (median 0.4406 / churn 136.00, asserted in-code —
  the determinism/comparability gate).
- **X-B**: `eviction_cap: None` is bit-identity — the existing Part 4c/4d/4e/4f
  frozen output and asserts (scalar 0.1035, mag 0.2818, X2 0.4042/210.00, the
  Part 4f identity gate) all still pass unchanged.
- Latency is record-only, never gated.

## Exploratory (non-gating, printed after the verdict)

- Eviction cap c ∈ {1, 2, 4} under the degraded signal, 60..90 (the
  interpolation between never-evict and the k0 arm).
- Rejoin-lockout k ∈ {1, 2} tasks (with unlimited evictions) under the
  degraded signal, 60..90 (the across-task state alternative).

## Pre-committed interpretation

- `VALIDATED (v6)` ⇒ v6 becomes the adoption candidate: quality ≥ 1.25× mag at
  zero churn under the runtime-feasible signal; remaining adoption questions
  reduce to latency (~µs-vs-64µs) and the #55 event wiring. Follow-up issue:
  runtime integration of the v6 arm behind the seam.
- `PARTIAL (oracle-only)` ⇒ signal fidelity matters at c = 0 after all;
  e1 remains battery-only; memo option B state stands.
- `FALSIFIED (never-evict)` ⇒ option B's parked state is final for this
  lineage; the exploratory cap/lockout cells inform whether any interior point
  merits a future registration, but none is implied. #57 (slow-loop) proceeds
  regardless — it does not depend on this verdict.

## Provenance

Owner decisions recorded in-session 2026-07-18 (lever / signals / bar).
Evidence chain: `docs/ab-report-K4-v5-e1-persistent-aif.md` (#53),
`docs/k4-arm-choice-memo.md` + #54 comments (Steps 1–3, Parts 4e/4f/4g),
`docs/per-bit-outcome-plumbing-design.md` (Step 2). Engine pin `aif-v0.11.0`
unchanged. Depends on #55 only conceptually (the degraded contract); the
battery needs no new plumbing.
