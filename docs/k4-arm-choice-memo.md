# K4 arm-choice decision memo — magnitude vs E1-persistent AIF (koalisi #54)

_2026-07-18. The Step 4 deliverable from the #54 sequencing (2026-07-17): an
owner-decision memo compiling the full K4 evidence through Steps 1–3. This memo
decides nothing — it lays out the options, what each commits to, and a
recommendation. All numbers are committed, registered-or-labelled-exploratory
battery results; provenance in §7._

## 1. The question

Which coalition-decision arm does koalisi's runtime adopt as its demonstrated
default, now that K4-v5 (#53) made quality conditional: **`aif-e1`** wins raw
decision quality out-of-sample, **magnitude** wins every cost axis. Scope
honesty: koalisi is a domain-agnostic reference runtime — "adoption" means
which arm the reference implementation defaults to and demonstrates, and which
seams get built (task-outcome event, persistent-arm lifecycle), not a
production SLA call.

## 2. Evidence base

### The K4 lineage (five registrations, one lens)

| Reg. | Arm vs magnitude | Verdict | What it established |
|---|---|---|---|
| v1/v2 (#7) | scalar AIF | `VALIDATED (B)` for **mag** | mag superior 30/30 on quality (0.4469 vs 0.1898, 0..30), 14× less churn; fails only the v1 strict-latency gate |
| v3 (#43) | multimodal AIF | `FALSIFIED (multimodality)` | per-bit structure alone is decision-equivalent to scalar (affine-G theorem) |
| v4 (#44) | persistent AIF (full E1+E2 stack) | `FALSIFIED (persistence)` | first arm to escape the v3 theorem (act-divergence 30/30) but lost on performance (0.0326); E6 ablation flagged the winning config |
| v5 (#53) | **`aif-e1`** (E6: learned per-bit precisions + novelty, fixed γ=16, no PrecisionDynamics) | **`VALIDATED (gap closed)`** | **first arm to beat mag on quality**: 0.4406 vs 0.2720 median on out-of-sample seeds 30..60 (1.62×), superior to scalar 30/30; X1: novelty-off collapses to scalar (0.1308) — learning AND novelty jointly load-bearing |

Side branches: the #46/#48 feedback-calculator arms (`FALSIFIED` /
`PARTIAL (mechanism only)`) showed reliability-gating adds a real signal but
never approached magnitude — the E1 world model is the only reliability
mechanism that has. The v2 magnitude verdict is not overturned (scoped to the
arms then registered); v5 is a new arm winning a new registration.

### #54 Step 1 — the costs, measured (Part 4e, seeds 30..60)

- **Churn**: mag median **8.00** vs e1 **136** (17×); scalar 79.5. ("Near-zero"
  mag churn was 0..30 folklore; the out-of-sample number is small, not zero.)
- **Churn↔quality correlation**: weakly **positive** — e1 churn vs its own
  PRIMARY_B Spearman +0.22, vs its margin over mag +0.17; the high-churn half
  of seeds carries the *larger* margins (0.170 vs 0.085 median). e1 wins *with*
  thrash, not despite it. e1 beats mag on 28/30 seeds.
- **Latency** (record-only, this run): e1 **63.9 µs**/decision vs mag ~3.55 µs
  vs scalar ~2.85 µs — e1/mag ≈ **18×**. (Informal note: under the v2
  amendment's own ≤10× tolerance philosophy this would not pass; latency was
  never a gate in v4/v5.)

### #54 Step 2 — outcome plumbing (Part 4e + design note): the thread lives

- The battery's per-bit oracle signal is **not load-bearing**: feeding e1 only
  whole-coalition success smeared across required bits (the **degraded L2**
  signal) retains the effect — **0.4381 vs 0.4406** median, bit-identical on
  17/30 seeds, churn 143 vs 136.
- Therefore the runtime requirement is exactly **one new domain-emitted
  task-completion event `(required_mask, final_members, success)`** — the same
  information contract #41's `record_outcome` already assumed. No per-member
  telemetry, no per-bit sub-task scoring, no `CoalitionDecisionPolicy` trait
  change (side-channel handle pattern), no topology-event-schema change.
  Durable home when needed: the P7.4 (#32) streams. Today **no** outcome
  signal exists anywhere in the runtime — this event must be built for ANY
  outcome-driven arm, including #41's feedback calculators.
  Full analysis: `docs/per-bit-outcome-plumbing-design.md`.

### #54 Step 3 — churn is not tunable on the current arm (Part 4f): null

- The pre-fixed 5×4 margin/hysteresis grid (join `p > 0.5+δ`, evict only at
  `score ≥ h`) is **flat in every cell** under both signals.
- Mechanism: the fixed-γ=16 posteriors **saturate at ±0.5** (p25–max of all
  ~11k join/leave scores exactly 0.5000). Decisions are binary; no sub-0.5
  threshold separates them. e1's churn is *confident flip-flopping* driven by
  between-task belief updates (learned precisions + novelty), not marginal
  indecision.
- Consequence: score-space mitigation is a dead lever. A low-churn e1 variant
  requires a **state-based mechanism** (dwell-time / cooldown /
  rejoin-lockout) — a new arm design, a library change, and its own
  registration on fresh seeds 60..90.

## 3. Cost-quality ledger

| Axis | magnitude | `aif-e1` | Notes |
|---|---|---|---|
| Quality (PRIMARY_B, 30..60) | 0.2720 | **0.4406** (0.4381 runtime-feasible signal) | e1 1.62×, superior 28/30 vs mag |
| Churn | **8.00** | 136 (not tunable without a new design) | membership mutations → hypergraph ops, event-log growth, tap traffic |
| Latency / decision | **~3.6 µs** | ~64 µs (18×) | both fine for the reference daemon's cadence; matters at scale |
| State | **stateless** (evaluator cache only, decision-neutral) | persistent world model per context; lifecycle + (eventually) durability (`state_snapshot()`; Phase-7 #31 gate) | |
| Plumbing | **none** | one task-completion event (domain-emitted) | the event also unblocks #41 feedback arms |
| Evidence maturity | 3 registrations + K1/K6 parity gates; never beaten on quality until v5 | 1 registration (v5) + a falsified sibling (v4); X1/X2 ablation support | |
| Blind spot | reliability (structurally cannot see it) | diversity is learned, not structural; churn | the two arms see orthogonal signals |

## 4. Options and what each commits to

**A — Adopt `aif-e1` as the demonstrated fast-loop default.**
Commits to: building the task-completion event seam; long-lived-arm lifecycle
(one arm per `DecisionContext` stream; reset/reseed policy); accepting 17×
churn and 18× latency in the reference daemon; a follow-on durability story
(#31-gated). Honest label: adopting a once-registered arm whose churn is a
known, currently-unmitigable cost. Quality claim for the *runtime signal*
config (degraded) is exploratory — a fresh registration on 60..90 (trivial:
the config exists) would harden it.

**B — Keep magnitude as the default; park e1 as capability evidence; pursue
the state-based low-churn e1-v6 as the registered adoption path.**
Commits to: no runtime change now; a v6 design cycle (dwell/cooldown lever in
`PersistentAifConfig`, prereg on 60..90, H: hold ≥ ~0.40 at materially lower
churn). Optionally still build the task-completion event seam now — it is
cheap, arm-agnostic, and unblocks #41 arms and any v6 test-drive.

**C — Hybrid: magnitude structure + e1 reliability filter.**
Commits to: a new arm design (e.g. mag decides, e1's per-bit reliability
beliefs veto/penalize unreliable providers) and its own registration. Note the
#46/#48 precedent: reliability-gating on a non-learning base never closed the
gap; the hybrid bet is that mag's structural selectivity + e1's *learned*
reliability compose. Untested; highest design risk, plausibly best ceiling
(the two arms' blind spots are orthogonal).

**D — Slow-loop seam (orthogonal; combinable with A/B/C).**
The SwarmAgentic population search (#42/#20) consumes a `ValueCalculator`, not
a policy — e1 has no calculator form today. Deriving one from the persistent
world model (e.g. expected covered-reliability of a block) is design work, but
the slow loop is where e1's costs vanish: latency is irrelevant offline and
"churn" does not exist for search-space evaluation (no runtime mutations).
The world model still needs the same task-outcome feed to learn from. If e1 is
parked (B), this is its natural second life.

## 5. Recommendation

**B + the event seam + D-as-follow-up.** Reasoning: (1) the decisive
runtime-blocker measured in this cycle is churn, and Step 3 proved it is not
tunable on the registered arm — adopting now (A) means adopting the thrash;
(2) the task-completion event is the no-regret move — small, arm-agnostic,
required by every outcome-driven arm and by any v6; (3) a dwell/cooldown v6 is
a well-posed, cheap registration with a pre-stated bar (≥ ~0.40 at materially
lower churn on 60..90), and Step 1b's weak positive churn–quality coupling
says the bar is genuinely at risk — which is what makes it a real test, not
theater; (4) the hybrid (C) is the highest-ceiling idea but should wait for
the v6 answer — if v6 holds quality at low churn, C may be moot.

Counter-case for A, stated fairly: if quality-per-task dominates all cost axes
in the intended deployments, e1's 1.62× is decisive today, the plumbing is one
event, and 64 µs/136-churn may simply not matter at reference-daemon scale.
That is a legitimate owner call; the evidence does not force B.

## 6. Decision (owner)

- [ ] A — adopt e1 now
- [ ] B — keep mag; event seam now; v6 (state-based low-churn e1) next
- [ ] C — hybrid design cycle
- [ ] D — slow-loop e1 fitness derivation (with A/B/C: ____)
- [ ] other / amendments: ____

## 7. Provenance

- Registered: `docs/ab-report-K4-{yamafaktory,catgraph,catgraph-evaluator}.md`
  (v1/v2 + parity), `docs/ab-report-K4v3-multimodal-aif.md`,
  `docs/ab-report-K4-v4-persistent-aif.md`,
  `docs/ab-report-K4-v5-e1-persistent-aif.md` (+ preregs alongside).
- #54 exploratory (unregistered, labelled): Parts 4e/4f of
  `examples/strategy_comparison.rs` (commits e69eb62, 66ea144; runs of
  2026-07-18 — prefix byte-identity + identity/X2 gates all held in-code),
  `docs/per-bit-outcome-plumbing-design.md`, Step-1b correlation (posted to
  #54, 2026-07-18).
- Context: #46/#48 feedback-arm reports; CLAUDE.md gotchas 20–23.
