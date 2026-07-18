# A/B report: K4-v6 — never-evict E1 arm, dual-signal, out-of-sample (koalisi #56)

_Registered run 2026-07-18, `examples/strategy_comparison.rs` Part 4h (release,
`--features decision,magnitude`), governed by `docs/prereg-K4-v6-never-evict.md`
(committed 68ba8fc + posted to #56 before implementation). Registered arm:
the #53 `aif-e1` (E6) config + `eviction_cap: Some(0)` (never-evict; skip-query
leave semantics). Confirmatory: Scope B, fresh seeds **60..90**, BOTH outcome
signals gating; all thresholds from this run's own 60..90 medians._

## VERDICT: `FALSIFIED (never-evict)`

- **H1 FAIL (both signals)** — ne-oracle **0.0143** / ne-degraded **0.0141**
  vs the bar 1.25 × mag 0.2841 = 0.3551.
- **H3 FAIL (both signals)** — ne strictly superior to scalar on **3/30**
  seeds (needed 18), per signal.
- **H2 PASS** — churn 0.00 / 0.00 (by construction at c = 0).

**Run validity: PASS** — X-A reproduced the #53 numbers exactly (e1-k0 on
30..60 = 0.4406 / 136.00, asserted in-code); all frozen Parts 1–4g
byte-identical (only record-only latency lines differ vs the Part 4g run);
X2 and the Part 4f identity gate held.

## Result detail (seeds 60..90 medians)

| arm | PRIMARY_B | churn |
|---|---:|---:|
| ne-oracle (registered) | 0.0143 | 0.00 |
| ne-degraded (registered) | 0.0141 | 0.00 |
| e1-k0 (context, oracle) | **0.3840** | 186.50 |
| scalar | 0.1443 | — |
| mag | 0.2841 | — |

Context worth recording: **e1-k0 replicates its quality edge on a third seed
range** (0.3840 = 1.35 × mag's 0.2841; 30/30-range was 1.62×) — the #53
mechanism is robust across ranges, just thinner here, while its churn rose to
186.5.

## Exploratory (non-gating; degraded signal, 60..90)

| condition | median PRIMARY_B | churn median |
|---|---:|---:|
| cap c=1 | 0.0201 | 20.00 |
| cap c=2 | 0.0252 | 40.00 |
| cap c=4 | 0.0467 | 80.00 |
| lockout k=1 | 0.3222 | 104.50 |
| lockout k=2 | 0.2094 | 74.00 |

**The cap series is the mechanistic headline: quality is monotone in allowed
evictions** (0 → 0.014, 1 → 0.020, 2 → 0.025, 4 → 0.047, unlimited → 0.384).
Within-task eviction volume is not overhead — it IS the mechanism by which the
arm converts its reliability beliefs into small, high-`cov_eff` coalitions.
This closes the loop with #54 Step 1b (churn–quality correlation +0.22, "wins
WITH thrash") and hardens it to a causal reading through the lever.

The **rejoin-lockout axis is the gentler tradeoff** (it never limits
within-task trimming): k=1 holds 0.3222 (1.13 × mag) at churn 104.5 — a 44%
churn reduction from k0's 186.5 for a 16% quality cost — but sits below the
1.25× family bar. Per the prereg, no interior-point registration is implied by
this run; if one is ever attempted, the lockout axis (not the cap axis) is the
candidate, on seeds 90..120.

## Interpretation (per the prereg's pre-committed commitments)

`FALSIFIED (never-evict)` finalizes memo option B's parked state for this
lineage (`docs/k4-arm-choice-memo.md` §6): **magnitude remains the
demonstrated fast-loop default; e1 stays parked as capability evidence** —
its quality mechanism (learned per-bit reliability + novelty, #53) is real and
range-robust, but it is inseparable from high eviction churn: the E1 lineage
now has both directions measured (score-space damping inert, Part 4f;
state-space damping destructive, this run). #57 (slow-loop e1-derived
`ValueCalculator`) proceeds — it does not depend on this verdict and is where
the mechanism's costs don't bite.

## Provenance

- Prereg: `docs/prereg-K4-v6-never-evict.md` (68ba8fc, posted pre-impl;
  owner-locked lever/signals/bar 2026-07-18).
- Lever: `PersistentAifConfig::{eviction_cap, rejoin_lockout_tasks}` (identity
  defaults; this PR). Battery: Part 4h (this PR). Engine `aif-v0.11.0`
  unchanged.
- Evidence chain: #53 (`VALIDATED (gap closed)`), #54 Steps 1–3 + Parts 4e–4g,
  `docs/k4-arm-choice-memo.md`.
