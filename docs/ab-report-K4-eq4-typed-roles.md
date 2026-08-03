# A/B report: K4 EQ4 — typed roles. VERDICT: `VALIDATED (typed roles)`

**Official run 2026-08-03**, seeds `240..270`, `--release`, koalisi
`v0.24.0`-to-be (catgraph `v0.7.0` ×2, `aif-v0.12.0`, MSRV 1.93).
Governed by `docs/prereg-K4-eq4-typed-roles.md` (registered pre-implementation
at `e2221c8`; pre-run **Amendment 1** `cd46c9d` — typed Scope-A, E-ρq
anti-alignment note, disclosure clauses; pre-run **Amendment 2** `f53a60a` —
§2 gloss erratum, re-draw/cap pins, owner-approved `E-ρq-inv` cell, E-T3
counters, interpretation corrections). 3-lens review (correctness /
registration-conformance / modeling-semantics) ran BEFORE this run:
**1 blocking + 5 important + ~9 minor, ALL applied or ledgered** (§7);
correctness lens returned zero findings. Owner design-lock D1–D9 on
[#72](https://github.com/sustia-llc/koalisi/issues/72), posted before the
prereg.

**This is the first `VALIDATED` verdict in the K4 lineage since v5, and the
first typed-vs-untyped contrast.** Scope (pre-committed, prereg §7): it
speaks to typed-vs-untyped valuation on a role-structured world — the v1/v2
K4 verdicts, EQ3's verdict, and the #54 arm question (mag = demonstrated
default, FINAL) are untouched.

## 1. Registered result (H-T + gates)

World **v2t** (prereg §2 as amended): the frozen `draw_prefix_v2` prefix
(pool `n ∈ 4..=16`, caps `1..=4`, `|required| ∈ 2..=8`, 8-bit universe —
A2.1), R = 3 worker roles + per-required-bit role tags appended off the same
SplitMix64 stream, role-matched feasibility rejection re-draw (≤ 1000 total
attempts per task; worst observed 220 = 22.0 % of budget). Ground truth =
role-matched coverage; `PRIMARY = success_rate × mean cov_eff`
(`success ≡ completed`, A1.1).

| arm | median PRIMARY | vs `mag` | superior seeds | median churn | median µs/decision |
|-----|---------------:|---------:|---------------:|-------------:|-------------------:|
| `mag` (frozen control) | 0.0501 | 1.00× | 0/30 | 6.00 | 3.819 |
| **`mag-typed` (T2, oracle ρ = δ)** | **0.1810** | **3.61×** | **30/30** | 6.00 | 11.720 |
| `scalar` (context) | 0.0204 | 0.41× | 6/30 | 102.50 | 2.690 |
| `arm-E1` (context) | 0.0565 | 1.13× | 18/30 | 153.00 | 62.735 |

- **Conjunct 1 PASS**: 0.1810 ≥ 1.25 × 0.0501 = 0.0626.
- **Conjunct 2 PASS**: strictly superior **30/30** (bar 18/30).
- **X-identity PASS** (cell 1: identity config bit-identical to `mag`, acts
  + score bits, 6830 decisions, plus the R = 1 metric-reduction check on
  600 tasks; cell 2: typed path at ρ ≡ 1 — acts + per-seed PRIMARY
  (bit-identical) + churn, 30/30; scores excluded by registration, the EQ3
  H-par′ lesson).
- **S-fib PASS**: three `role_grid` shapes vs
  `RoleFibrationProof::expected_magnitude()`, rel Δ 1.24e-16 / 3.79e-16 /
  3.10e-16 against the upstream-documented 1e-9 tolerance.
- **X-battery PASS** (checked outside the binary): this run's Parts 1–7
  diffed against the fresh `v0.23.0` pre-implementation baseline —
  **every quality/churn/verdict line byte-identical**; the only diffs are
  latency lines (standing exclusion) and the appended Part-8 section
  boundary.

Verdict grammar and per-seed H-T table are in the committed run output
(the example prints both); success-rate context: `mag` 0.2117 →
`mag-typed` 0.7950 mean role-matched completion.

## 2. Mechanism (what won, and what did not)

`mag-typed` never sees a tag — relevance masks stay UNTYPED by registration
(prereg §4). It cannot route coverage toward the role a bit asks for. Its
one lever is refusing to treat cross-role members as substitutes: the
oracle `ρ = δ` zeroes cross-role couplings, so same-mask/different-role
agents never skeletalize into one effective agent and each keeps its own
diversity weight. **The win is role-diverse redundancy retained, not
coverage routing** (A2.5.iv). The E-ceil contrast (§3) quantifies the
remainder: the ρ-modulation lever converts **43.5 %** of the tag-informed
reference margin; the rest requires tag knowledge the registered lever
deliberately withholds.

Non-tautology (semantics lens, verified): both arms are scored by the same
typed metric; the typed arm receives only worker roles, never tags; three
in-run falsifiability witnesses show the design could have failed —
E-ρq (the same lever at 0.36× when the world rewards cohesion), E-T3
(a different typed lever at 0.83× on the same world the confirmatory lever
wins on), and the #63 precedent that planted signal often fails to convert.
"Size alone wins" is refuted in-run (`scalar` joins promiscuously and
scores 0.41×).

## 3. Exploratory legs (all non-gating)

- **E-deg (ρ mis-specification)**: oracle 0.1810 (3.61×, 30/30) →
  ρ_off = 0.25: 0.1674 (3.34×, 30/30) → ρ_off = 0.5: 0.1174 (2.34×,
  29/30); churn 6 → 9 → 14.5. The margin degrades gracefully — the lever
  does not need the exact table, but a blunter table both scores lower and
  thrashes more.
- **E-ceil (typed-relevance reference arm)**: 0.3513 (7.01×, 30/30) at
  3.729 µs — a **fully-informed reference arm within the magnitude family,
  NOT a supremum** (A2.5.i). Conversion fraction
  (0.1810 − 0.0501)/(0.3513 − 0.0501) = **43.5 %** (medians; a summary
  contrast, not a per-seed decomposition).
- **E-ρq (ρ-structured quality world)**: `mag` 0.2451 ·
  `mag-typed (ρ_world)` 0.0875 (**0.36×**, 0/30) ·
  **`mag-typed-inv` (ρ flipped, A2.3) 0.1926 (0.79×, 9/30)**. Per A1.2 the
  ρ_world configuration is structurally ANTI-aligned: ρ < 1 weakens
  cross-role couplings, magnitude reads weak coupling as diversity, so the
  arm admits MORE role-mixing while this world pays for cohesion —
  gotcha 23 restated (magnitude scores diversity, not dependability). The
  inverse cell shows alignment is only **partially recoverable inside T2**
  (0.36× → 0.79×, still under untyped `mag`): the modulation axis can
  redirect diversity accounting, but a cohesion-rewarding world is
  fundamentally mismatched with a diversity-scoring functional.
- **E-T3 (channel-valued couplings, uniform θ)**: 0.0416 (0.83×, 9/30).
  **Caveat incidence (A2.4, measured)**: 207 480 channel entries, 104 570
  (50.4 %) neutral-1.0 on an empty denominator; exact-1.0 collapsed
  couplings split **0** all-channels-neutral vs **5 265** `powf`-rounded.
  The measurement CORRECTS the registered caveat's account: the pure
  "no evidence anywhere" case is structurally impossible on the real
  stream (relevance survivors have non-empty `rel ⊆ required` and
  `∪_c tagged(c) = required`), so **every forced skeletal merge in this
  leg is the upstream `powf`-rounds-to-1.0 trap, not the neutral-element
  convention** (§7 item 13).

## 4. T1 instrumentation (role shares)

600 final `mag-typed` coalitions decomposed off the decision path
(0 skips, 0 errors). **Mixed classes: 0/600** — expected under ρ = δ
(a cross-role coupling modulated to exactly 0 can never reach mutual
closure 1.0, so only same-role clones merge; the count would print either
way). `Σ_r share(r)` vs `base_value()` max rel gap 3.77e-16 (upstream
contracts equality up to float re-association, not bit-identity). Role
share medians 1.00 / 1.50 / 1.57 (p25 all 1.0, p75 all 2.0).

## 5. Disclosures (registered clauses)

- **Task-size shift (A1.3.i)**: rejection is not size-neutral — realized
  `|required|` mean 3.27 / median 3.0 vs pure-v2 4.94 / 5.0 on the same
  seeds. The registered world IS the post-rejection distribution;
  cross-part comparisons carry this clause.
- **Tag conditioning (A2.5.iii)**: 19.6 % of required bits are
  single-role-held (tag forced; contest-dead — typed ≡ untyped coverage
  there); per-seed mean spans 0.0 %–71.9 %; re-draw intensity
  anti-correlates with pool size. Direction is CONSERVATIVE for H-T.
- **Fresh policy per seed for every arm (A1.3.ii)** — structural for
  `mag-typed`; latency-only effect on the control (cache warmth;
  decisions knife-edge-frozen per gotcha 15).
- **Re-draws**: 6 908 total across all 30 seeds (per-seed column in the
  run output), worst seed 1 013.
- **arm-E1 context notes (A2.5.v)**: e1 is the only context arm receiving
  typed ground truth (its 1.13×/18-30 vs `mag` is not a role-blind
  contrast), and its per-bit posterior is a tag-marginal — it mixes
  capability coverage with tag luck and is non-stationary relative to the
  v5 world.
- **Latency (record-only, never gating)**: `mag-typed` 11.7 µs vs `mag`
  3.8 µs (~3.1×) — the registered fresh-both-sides/no-cache contract is
  the cause (prereg §4).

## 6. Review trail

3-lens review before the run; findings: **correctness 0**;
**registration-conformance** 1 blocking (E-ρq preamble contradicted A1.2 —
corrected), 1 important (§2 gloss transcribed the w12 draw's numbers —
A2.1 erratum), 6 minor (ledgered below); **modeling-semantics** 4
important (pass-side E-ceil scoping; reference-arm wording; tag-conditioning
beyond A1.3.i; E-T3 caveat unmeasured — all applied via A2.4/A2.5 +
printed lines), 3 minor. Apply-pass commit `f86a0da`; nothing contested,
nothing skipped.

## 7. Implementation / deviation ledger

1. `success ≡ completed` (A1.1): the v2t world is a typed Scope-A — no
   Scope-B reliability gate is layered on; the registered question is role
   conversion, not reliability.
2. §2's v2-prefix gloss transcribed `draw_prefix_w12` numbers (A2.1); the
   named frozen function governs and is what ran (caps 1..=4,
   |required| 2..=8, 8-bit). Verify-before-prescribe miss, recorded.
3. Re-draw scope = the FULL v2 task draw (size + bits + tags; never the
   arrival order); cap = 1000 TOTAL attempts (initial draw = attempt 1),
   so 999 re-draws (A2.2).
4. The E-ρq preamble shipped pre-review contradicting A1.2
   ("agree by construction") — blocking finding, corrected before this run
   to the registered anti-alignment framing.
5. E-ceil is a reference arm, not a supremum; prereg §5's "total
   convertible signal" is corrected by A2.5.i (registered text stands, the
   amendment governs — #63 ledger precedent). Conversion fraction
   reported per A2.5.ii.
6. Tag-distribution conditioning disclosed beyond the size clause
   (A2.5.iii): single-role-held bits are contest-dead; conservative for
   H-T.
7. Fresh policy per seed for every arm (A1.3.ii) — differs from Part 7's
   one-instance convention; latency-only.
8. X-identity cell 1 is identity-by-construction (A1.3.iii): asserted as a
   determinism check + the R = 1 metric-reduction check.
9. T1 decomposition re-runs the (deterministic) typed arm and samples the
   post-sweep hook rather than sampling inside the official pass —
   identical coalitions, off the decision path.
10. Additive unregistered instrumentation (registration does not forbid):
    cap-headroom metric, success-rate context line, `powf` caveat print,
    E-deg Δ column + context rows, report date constant.
11. The registered seeds were executed pre-review in a smoke run labeled
    PRELIMINARY throughout; the PRELIMINARY label was removed after the
    review completed, and THIS post-review run is the official one. The
    battery is deterministic, so smoke and official agree on every
    non-latency number.
12. Library missing-role handling (decline-with-warn) is not in the
    prereg; the battery forecloses it by asserting full-pool role-map
    coverage (a harness gap becomes a hard stop, not a silent decline).
13. E-T3 counter finding (A2.4): all-channels-neutral unit couplings are
    structurally impossible on the real stream — every exact-1.0 merge is
    the upstream `powf` trap. The registered caveat's "no evidence"
    mechanism does not occur; the measured account replaces it.
14. `RoleId`/`RoleModulation` re-exported from `koalisi::decision`
    (correctness-lens API-consistency note, aif-types precedent).
15. Suites at this commit: 103 default / 159 decision / 132 magnitude /
    188 decision,magnitude / 140 magnitude-fast / 123 persistence /
    153 persistence,magnitude; example binary 32 (+1 ignored).

## 8. Standing consequences

- **`with_role_modulation` ships as an opt-in library surface** (feature
  `magnitude`, identity default = the untyped arm, bit-identical by
  construction). The typed arm is NOT the demonstrated default — the #54
  decision (mag = default) stands; adopting a typed default would be a NEW
  registration.
- The EQ5 battery (workflows as string diagrams) builds on this typing per
  the E-queue; cg#57 a2 fires there.
- Upstream seams surfaced: the E-T3 `powf`-rounds-to-1.0 trap is now
  measured downstream (5 265 forced merges in one leg) — input for any
  future catgraph channel-collapse hardening; the E-ρq-inv result bounds
  what T2 modulation can do on cohesion-rewarding worlds.
