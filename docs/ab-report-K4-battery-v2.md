# A/B report: K4 battery v2 — de-saturated regime (koalisi #61 / stack EQ1)

> **RESULTS (2026-07-31):**
> **Lever 2 (de-saturation): `FALSIFIED (de-saturation)`** — no (γ ∈ {1, 4},
> v2-draw, δ > 0) cell reaches the churn ≤ 0.5× bar (best: 173.00 vs a 175.00
> baseline; the bar was ≤ 87.5). Mechanism measured: γ de-saturates only the
> **leave** stream; the **join** stream stays saturated at p = 1.0 in every
> cell, and the registered margin lever acts on joins.
> **Lever 1 (routing): `RUN-INVALID (sanity leg)`** — the unweighted argmax
> covers all required bits on 25/30 < 27. No verdict; two structural
> formulation flaws in the registered criterion are documented below.
> **Part 5c (registered exploratory): four of its five items did not run**
> (the 12-bit widened slice, leave-side hysteresis, expected-outcome model,
> learned-posterior twins) — deferred by owner decision 2026-07-31 to a
> follow-up session; their results will be recorded as an appended
> exploratory addendum, never by editing the sections above. The fifth item
> — oracle-vs-degraded pricing — ran as the Part 5a oracle twins and is
> reported in the Lever 3 section below.

_Registered design: `docs/prereg-K4-battery-v2.md` (committed d9881e1 +
posted to #61 BEFORE implementation). Run 2026-07-31, `--release`, seeds
**120..150** (150..180 held out — **unconsumed**: nothing validated, so no
replication run occurred; the range stays reserved per the prereg).
Pins: catgraph `v0.5.0` ×2 · `aif-v0.11.0` · surrealdb-live-message `v0.2.1`
(the registered born-on set; mid-arc freeze held). New arm-config lever:
`PersistentAifConfig::query_gamma: Option<f64>` (identity default `None` =
engine γ 16; live only on the MeanField / `query_dynamics: false` path)._

## Run-validity gates

| Gate | Registered requirement | Result |
|---|---|---|
| X-A | arm-E1 (frozen v5 config, oracle) reproduces 0.4406 / 136.00 on 30..60 | **PASS** (in-code assert, standing Part 4d gate) |
| X-B(a) | `query_gamma: Some(16.0)` ≡ `None` bit-for-bit on the X-A cell | **PASS** (asserted, all 30 seeds) |
| X-B(b) | `MarginE1(δ = 0, h = 0)` ≡ unwrapped arm per seed, all 6 (γ, regime) cells | **PASS** (asserted) |
| X-C | Every pre-existing printed line of Parts 1–4h byte-identical | **PASS** (full-run diff vs pre-implementation baseline: additions only; latency lines the sole permitted diff, per the standing exclusion) |
| X-D | H-R sanity leg ≥ 27/30 | **FAIL — 25/30 ⇒ Part 5b RUN-INVALID** |

Suites at the run commit: 154 `decision` / 176 `decision,magnitude`
(+2 `query_gamma` unit tests over the v0.17.0 baselines); default clippy
`--all-targets` clean.

## Part 5a — factorial battery (lever 2, CONFIRMATORY)

γ ∈ {1, 4, 16} × regime ∈ {v1-draw: `|required|` 1..=5, v2-draw:
`|required|` 2..=8} × join margin δ ∈ {0, 0.15, 0.30}; degraded/L2 signal;
Scope B; seeds 120..150.

### Factorial cell medians (degraded signal)

| regime | arm | δ | median PRIMARY_B | churn median |
|--------|-----|--:|-----------------:|-------------:|
| v1-draw | arm-E1g1 | 0.00 | 0.3860 | 181.00 |
| v1-draw | arm-E1g1 | 0.15 | 0.3860 | 179.50 |
| v1-draw | arm-E1g1 | 0.30 | 0.3734 | 172.50 |
| v1-draw | arm-E1g4 | 0.00 | 0.3860 | 181.00 |
| v1-draw | arm-E1g4 | 0.15 | 0.3860 | 180.50 |
| v1-draw | arm-E1g4 | 0.30 | 0.3860 | 180.50 |
| v1-draw | arm-E1g16 | 0.00 | 0.3860 | 181.00 |
| v1-draw | arm-E1g16 | 0.15 | 0.3860 | 181.00 |
| v1-draw | arm-E1g16 | 0.30 | 0.3860 | 180.50 |
| v2-draw | arm-E1g1 | 0.00 | 0.1621 | 175.00 |
| v2-draw | arm-E1g1 | 0.15 | 0.1573 | 174.50 |
| v2-draw | arm-E1g1 | 0.30 | 0.1573 | 173.00 |
| v2-draw | arm-E1g4 | 0.00 | 0.1621 | 175.00 |
| v2-draw | arm-E1g4 | 0.15 | 0.1621 | 174.50 |
| v2-draw | arm-E1g4 | 0.30 | 0.1586 | 174.50 |
| v2-draw | arm-E1g16 | 0.00 | 0.1621 | 175.00 |
| v2-draw | arm-E1g16 | 0.15 | 0.1621 | 175.00 |
| v2-draw | arm-E1g16 | 0.30 | 0.1621 | 175.00 |

### In-run baselines on the same instances (context, non-gating)

| regime | arm | median PRIMARY_B | churn median |
|--------|-----|-----------------:|-------------:|
| v1-draw | mag | 0.2991 | 9.00 |
| v1-draw | scalar | 0.1093 | 101.50 |
| v2-draw | mag | 0.1286 | 10.00 |
| v2-draw | scalar | 0.1332 | 141.00 |

**Context finding (non-gating, no verdict):** the v2 regime inverts the v1
quality ordering — `mag` 0.1286 < `scalar` 0.1332 < `e1-degraded` 0.1621.
This is the first measured regime in which magnitude does not lead on
quality under the runtime-feasible signal. It is a context row on 30 seeds,
registered as context only; any arm-choice consequence is its own owner
decision per the #61 non-goals (the #54 B+D decision is untouched).

### Decision-score quantiles at δ = 0 (the mechanism observable)

| regime | arm | stream | n | p25 | p50 | p75 |
|--------|-----|--------|--:|----:|----:|----:|
| v1-draw | arm-E1g1 | join | 5760 | 0.5000 | 0.5000 | 0.5000 |
| v1-draw | arm-E1g1 | leave | 6290 | 0.4361 | 0.4890 | 0.4981 |
| v1-draw | arm-E1g4 | join | 5760 | 0.5000 | 0.5000 | 0.5000 |
| v1-draw | arm-E1g4 | leave | 6290 | 0.5000 | 0.5000 | 0.5000 |
| v1-draw | arm-E1g16 | join | 5760 | 0.5000 | 0.5000 | 0.5000 |
| v1-draw | arm-E1g16 | leave | 6290 | 0.5000 | 0.5000 | 0.5000 |
| v2-draw | arm-E1g1 | join | 5760 | 0.5000 | 0.5000 | 0.5000 |
| v2-draw | arm-E1g1 | leave | 6343 | 0.1137 | 0.4521 | 0.4989 |
| v2-draw | arm-E1g4 | join | 5760 | 0.5000 | 0.5000 | 0.5000 |
| v2-draw | arm-E1g4 | leave | 6343 | 0.3644 | 0.5000 | 0.5000 |
| v2-draw | arm-E1g16 | join | 5760 | 0.4994 | 0.5000 | 0.5000 |
| v2-draw | arm-E1g16 | leave | 6343 | 0.4994 | 0.5000 | 0.5000 |

The battery **did** create de-saturation — but only on the **leave** stream
(γ = 1, v2-draw: leave p25 = 0.1137 vs Part 4f's wall of exact 0.5000s).
The **join** stream is pinned at score +0.5 (p = 1.0, certainty) in every
cell including γ = 1 × v2-draw. The registered margin lever acts on joins
(`p > 0.5 + δ`), so it faces the one stream γ does not free. Any follow-up
churn lever on this arm should target the leave stream (hysteresis h — the
Part 5c exploratory axis) or membership state (#56 lineage), not join
margins.

### H-S evaluation (γ ∈ {1, 4}, v2-draw, δ > 0 vs own δ = 0 baseline)

| arm | δ | churn med | base | ≤ 0.5× | PRIMARY_B med | base | ≥ 0.9× | paired churn↓ | ≥ 18/30 | cell |
|-----|--:|----------:|-----:|:------:|--------------:|-----:|:------:|--------------:|:-------:|:----:|
| arm-E1g1 | 0.15 | 174.50 | 175.00 | FAIL | 0.1573 | 0.1621 | PASS | 17/30 | FAIL | FAIL |
| arm-E1g1 | 0.30 | 173.00 | 175.00 | FAIL | 0.1573 | 0.1621 | PASS | 23/30 | PASS | FAIL |
| arm-E1g4 | 0.15 | 174.50 | 175.00 | FAIL | 0.1621 | 0.1621 | PASS | 7/30 | FAIL | FAIL |
| arm-E1g4 | 0.30 | 174.50 | 175.00 | FAIL | 0.1586 | 0.1621 | PASS | 10/30 | FAIL | FAIL |

**VERDICT (lever 2): `FALSIFIED (de-saturation)`.** Scoping per the
registered verdict rule: the δ = 0 scores DID de-saturate in at least one
(γ ∈ {1, 4}, v2-draw) row, so the falsification is about the **margin
lever**, not about reaching a de-saturated regime. Part 4f's inertness
claim survives its first out-of-regime test in the one place it was
registered to be re-tested: join-side score space.

## Part 5b — reliability-routing test (lever 1, CONFIRMATORY)

`TaskCoverageV2` (full 100 · partial `w(m) = 80/m` · member cost 8·N) vs its
reliability-weighted twin; pool n ∈ 8..=16 (caps 1..=4 bits), m ∈ {7, 8},
planted `r[b*] = 0.15` / others 0.9; both argmaxes from `search()` at the
same pinned `PopulationConfig` and seed; `REAL` = closed-form expected
payoff under per-bit Bernoulli(r).

### H-R legs (seeds 120..150)

- **Sanity leg (run-invalidating):** unweighted argmax covers every
  required bit on **25/30 < 27 → FAIL**.
- Skip leg: weighted argmax skips `b*` on **0/30** (moot).
- REAL leg: weighted strictly greater on **11/30**; medians **129.8003 vs
  129.8003** (tied; moot).

**VERDICT (lever 1): `RUN-INVALID (sanity leg)`** — per the prereg's
pre-committed rule, a sanity-leg failure invalidates the run rather than
producing a verdict. **No routing conclusion may be drawn from this run in
either direction.**

### Structural analysis (why the registered criterion could not work)

Two formulation flaws in the registered H-R, both discovered
post-registration (implementation-time and run-time), both now measured:

1. **The skip predicate is unsatisfiable under partition semantics.**
   `search()` returns a partition of the ENTIRE pool (`assignment[i]`
   defined for every agent), so the union over blocks equals the pool
   union. "No block covers b\*" therefore reduces to "no pool agent
   provides b\*" — independent of the calculator — and is mutually
   exclusive with the sanity leg. Whenever sanity holds, skip is 0/N by
   construction. Related: over a fixed pool the 8·N member cost is
   constant across partitions, so "skipping to save a member" cannot
   express at the partition level; and at the `TaskCoverageV2` rates the
   full branch pays 100/m per unit reliability per bit vs the partial
   branch's 80/m, so a same-size block never profits from skipping at any
   reliability. Reliability re-ranks structures only through *which* bits
   the partial blocks cover — visible in the per-seed `REAL` deltas (both
   directions, netting to a tied median).
2. **The instance draw does not guarantee pool coverage.** n ∈ 8..=16
   agents × 1..=4 random bits fails to cover a 7–8-bit requirement on 5/30
   seeds, which is what tripped the sanity bar.

**Attribution (pre-committed-interpretation discipline):** the registered
prereg's interpretation for `FALSIFIED (routing)` — "reliability weighting
does not route even where the coefficients permit it" — is NOT available
here: no verdict was produced, and the structural analysis shows the
registered instance was not in the coefficient-permitting regime in the
first place (the flip region needs member-count savings at near-zero
reliabilities, far from the 0.9/0.15 planting). Gotcha 24's
rescale-not-reroute is neither strengthened nor weakened by this run.
A corrected routing test — coverage-guaranteed pool draw, a block-level
skip predicate, flip-region planting/coefficients — requires a **new
registration** (follow-up issue filed from this report).

Non-registered diagnostic (exploratory, printed after the verdict): the
weighted argmax's single highest-value block omits `b*` where the
unweighted one includes it on 2/30 seeds.

## Lever 3 — oracle-vs-degraded pricing (EXPLORATORY, non-gating)

Oracle twins of the Part 5a cells. Headline medians at δ = 0:

| regime | signal | arm-E1g16 median PRIMARY_B |
|--------|--------|---------------------------:|
| v1-draw | degraded | 0.3860 |
| v1-draw | oracle | 0.3948 (+2.3%) |
| v2-draw | degraded | 0.1621 |
| v2-draw | oracle | 0.1881 (+16.0%) |

The oracle–degraded gap widens roughly 2% → 16% moving to the harder
regime: per-bit signal fidelity starts to price in exactly where the v1
battery could not see it (Part 4e's "degraded ≈ oracle" is regime-scoped).
Exploratory by registration; any confirmatory fidelity claim needs its own
registration.

## Pre-committed interpretation (as registered, applied)

- Lever 2 `FALSIFIED` ⇒ the inertness of score-space churn levers
  generalizes across this γ/regime grid — with the measured refinement
  that the join rail (certainty joins) is the blocker; leave-side and
  state-based levers remain the only live churn axes.
- Lever 1 `RUN-INVALID` ⇒ no update to gotcha 24 in either direction; the
  corrected test is a new registration.
- Lever 3 stays exploratory; the widened fidelity gap motivates (not
  licenses) a registered fidelity test in a future entry.
- EQ-queue: EQ1's landing is the fair-judgment baseline for EQ3/EQ4/EQ5;
  the v2-regime context inversion (mag < scalar < e1-degraded) is the
  strongest single argument that later entries must register on v2-style
  regimes, not v1.

## Provenance

Prereg `docs/prereg-K4-battery-v2.md` (d9881e1, immutable; its Part 5c
scope remains registered and pending). Design-lock + prereg + results
posted to #61. Implementation: `query_gamma` lever in
`src/decision/aif_persistent_policy.rs` (+2 unit tests); Parts 5a/5b
additive in `examples/strategy_comparison.rs` (`run_seed_b` became a thin
`Regime::V1` wrapper over a new `run_seed_b_regime` — the regime parameter
selects the v1/v2 instance draw; the 4-arg outcome hook is unchanged, it
has carried the member list since Part 4g — call-site refactor
output-neutral, proven by gate X-C).
Battery run: single-writer release run, byte-identity verified against the
pre-implementation baseline capture. Seeds ledger after this run: 0..30,
30..60, 60..90 consumed by v1-lineage registrations; **120..150 consumed by
this run**; 90..120 soft-reserved (lockout axis); 150..180 reserved
(unconsumed replication range).
