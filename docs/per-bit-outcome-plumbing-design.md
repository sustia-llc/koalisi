# Design note — per-bit outcome plumbing for the E1-persistent AIF arm (koalisi #54, Step 2)

_2026-07-18. The load-bearing Step 2 deliverable from the #54 sequencing comment
(2026-07-17): can the runtime feed [`PersistentAifArm`] the outcome signal the
K4-v5 battery feeds it, at what fidelity, and does a degraded (runtime-feasible)
signal preserve the #53 `VALIDATED (gap closed)` effect? Unregistered analysis —
no verdict is claimed for any quality-affecting variant; a quality-claimed L2 arm
would need a fresh registration on seeds 60..90 per the standing prereg
discipline. All code claims verified against the working tree at `1353605`._

[`PersistentAifArm`]: ../src/decision/aif_persistent_policy.rs

## 1. The signal contract the battery satisfies

`run_seed_b` (`examples/strategy_comparison.rs`) computes, once per task after
the leave sweep:

- `success` — whole-coalition: `completed && members.iter().all(|&i| perf[t][i])`;
- `per_bit[b]` — **bit b succeeds iff some final member providing b performed**
  (`(agents[i].caps >> b) & 1 == 1 && perf[t][i]`);

and calls `arm.observe_outcome(task.required, &per_bit)`. The arm maps each bit:
not required → `no_obs`, required ∧ `per_bit[b]` → `success`, else `failure`
(three-outcome modality, `aif_persistent_policy.rs`).

Three properties of this signal matter, in decreasing order of how hard they are
to reproduce outside the battery:

- **P1 — provider-conditioned truth.** `per_bit[b]` is derived from the hidden
  per-agent performance draw `perf[t][i]` (reliability `ρ_i`). It answers "did a
  *provider of b* actually perform", not "was b covered". This is oracle
  information: the battery reads the instance's hidden reliability structure.
- **P2 — per-required-bit resolution.** Bits succeed and fail independently
  within one task; a partially-failed task still yields positive evidence on the
  bits whose providers performed.
- **P3 — availability on failed tasks.** Partial credit exists: the arm learns
  from failures *which* bits failed, not merely that the task failed.

## 2. What the runtime actually has (verified inventory)

- **`CoalitionService`** (`subsystems/coalition_actor.rs`): policy-gated
  join/leave over a `Box<dyn CoalitionDecisionPolicy>`. Its optional decision tap
  emits `DecisionRecord { coalition, agent_id, kind, act, score }` — a record of
  *decisions*, not outcomes.
- **Topology event log** (`topology/events.rs`): 13 variants, all
  vertex/hyperedge lifecycle. **No task, outcome, or performance event exists.**
  This is the same fact behind gotcha 19: `seed_feedback_history` can seed
  membership episodes but "failures aren't seedable — the log has no outcomes".
- **The #41 scalar seam** (`algorithms/feedback.rs`):
  `FeedbackStore::record_outcome(&members, value)` — a *caller-supplied* scalar
  per coalition episode. Grep confirms no runtime component calls it: the only
  call sites are the library itself, the battery example, and tests. The seam is
  a socket with nothing plugged into its producing side.
- **`CoalitionDecisionPolicy`** (`decision/mod.rs`): `should_join`/`should_leave`
  (+ async variants) only — **no outcome-feedback method**. Wiring e1 into
  `CoalitionService` does not require a trait change: the battery's pattern
  (hold the concrete `PersistentAifArm` handle outside the policy seam and call
  `observe_outcome` from the outcome source) is the same side-channel shape as
  the #46/#48 `Arm { policy, store }` factory. `PersistentAifArm` is `&self`
  throughout and already exposes `state_snapshot()` (the catgraph #72/#73
  persistence boundary seam).
- **Statically known at decision time**: capability masks
  (`AgentCapabilities`), `DecisionContext.required_capabilities`, and the final
  member set. Therefore per-bit *coverage* attribution is computable at runtime
  for free. What is **not** observable anywhere: per-member *performance* — the
  `ρ_i`/`perf[t][i]` analogue. That is domain telemetry koalisi's domain-agnostic
  core cannot synthesize.

## 3. Fidelity ladder

| Level | Signal | Reproduces | Runtime requirement |
|---|---|---|---|
| **L0** (battery oracle) | per-member per-task performance → exact `per_bit` | P1+P2+P3 | per-agent task-performance telemetry from the domain + a task-outcome event carrying per-member performed flags. Domain-specific; does not exist; cannot be synthesized generically. |
| **L1** | per-bit success scored by an external per-task evaluator | P2+P3 (P1 approximated) | a task-outcome event with a per-bit payload; feasible only where the domain can score sub-outcomes per capability bit (e.g. sub-task decomposition mirrors bits). |
| **L2** (degraded) | one whole-task success/failure bool, smeared across required bits: `observe_outcome(required, &[success; 8])` | none of P1–P3 (on failure, all required bits read as failed; on success, all read as succeeded) | exactly **one** new signal: a task-completion event `(required_mask, final_members, success)`. Members and mask are already known; only the boolean is new. Same information content as the #41 scalar seam. |

L2 is the honest runtime floor: it needs only what #41 already assumed the
domain could provide (a scalar episode outcome), re-aimed at the arm instead of
a `FeedbackStore`.

## 4. Where a task-outcome event would live

- **Emission**: by the *domain* embedding koalisi (exactly as `record_outcome`
  is caller-supplied today) — e.g. a `TaskOutcome { required: u32, members:
  Vec<VertexIndex>, success: bool }` handed to a small forwarder that fans out
  to (a) `FeedbackStore::record_outcome` (the #41 consumer, scalarized), and
  (b) `PersistentAifArm::observe_outcome` (L2-smeared, or L1 per-bit where the
  domain can supply it).
- **Not** a `TemporalEvent`: the topology log is lifecycle-only by design and
  replay (`WireTopologyEvent`, P7.2) is pinned at 13 variants +
  `WIRE_TOPOLOGY_SCHEMA_VERSION`. The natural durable home is the **P7.4
  Decisions/Beliefs streams (#32)** — outcome records are decision-adjacent,
  not topology. Until #32, the tap-style non-durable channel (the
  `DecisionRecord` pattern) suffices for a live arm.
- **Durable arm state** is a separate concern: `state_snapshot()` exists for
  the catgraph #72/#73 boundary; runtime persistence proper stays gated on
  Phase-7 #31 (tauhokohoko KEK reply).

## 5. The empirical fork — degraded signal in the battery

Part 4e (`part4e_arm_choice_addendum`, unregistered exploratory) runs the
registered `aif-e1` configuration over the out-of-sample seeds 30..60 with the
L2 hook (`observe_outcome(required, &[success; 8])`) against the same-run
oracle-signal e1 and scalar rows.

**Result (run of 2026-07-18; run validity: all frozen sections byte-identical,
Part 4d reproduced 0.4406/0.1267/0.2720 and the X2 gate reproduced
0.4042/210.00):**

| signal | median PRIMARY_B | churn median |
|---|---:|---:|
| e1 oracle (per-bit, Part 4d ≡ 4e re-run) | 0.4406 | 136.00 |
| **e1 degraded (L2, whole-task success smeared)** | **0.4381** | 143.00 |
| scalar | 0.1267 | 79.50 |
| mag | 0.2720 | 8.00 (Part 4e — first time measured) |

Per-seed: the degraded arm is **bit-identical to oracle on 17/30 seeds** and
slightly lower on the rest; the two visible exceptions are seed 42 (0.0643 vs
0.2246) and seed 59 (0.0168 vs 0.0576) — both already e1's worst instances
under the oracle signal. Degraded churn (143) is statistically indistinguishable
from oracle churn (136).

Decision table (written before the run):

- **Degraded holds near oracle** (informally: still ≥ 1.25× scalar and within
  ~sight of 0.44): the runtime needs only the L2 task-completion event — the
  thread lives; churn mitigation (Step 3) is worth pursuing; any adoption-bound
  variant registers fresh on 60..90.
- **Degraded collapses to scalar** (~0.13): the per-bit oracle signal (P1/P2)
  is load-bearing. Per-bit attribution is generically infeasible (L0/L1 are
  domain-conditional), so **e1 is battery-only** for the domain-agnostic
  runtime: the memo's fast-loop answer is magnitude, with e1 parked as
  capability evidence and as a slow-loop candidate where oracle-ish fitness
  exists (the #42 population-search `ValueCalculator` seam evaluates synthetic
  fitness functions — no outcome plumbing needed there).
- **In between**: judgment call for the memo; the exploratory number bounds
  what an L2 runtime arm could deliver, and only a fresh registration can
  quality-claim it.

## 6. Conclusion

**The first branch of the decision table fires: degraded ≈ oracle.** The
per-bit oracle signal (P1/P2/P3) is **not** load-bearing for the E1 effect on
these instances — the whole-task success bool, smeared across required bits,
retains 99.4% of the oracle median (0.4381 vs 0.4406) and the full separation
from scalar (3.5×) and magnitude (1.6×). The thread does **not** die here:

- **The runtime requirement collapses to L2** — exactly one new signal, a
  domain-emitted task-completion event `(required_mask, final_members,
  success)`. That is the same information contract the #41 `FeedbackStore`
  seam already assumed; no per-member performance telemetry, no per-bit
  sub-task scoring, no trait change, no topology-event-schema change.
- **Why so little is lost (mechanistic reading, unverified per-decision):**
  under the smeared signal a required bit is marked failed whenever *any*
  member underperforms, which mislabels the bits whose providers did perform —
  but the arm's per-bit Dirichlet learning averages over many tasks, and
  provider sets vary task-to-task, so the mislabelling decorrelates and the
  reliable/unreliable provider distinction survives. The two seeds where
  degraded visibly lags (42, 59) are the low-signal instances where e1 was
  already weakest.
- **Caveats:** unregistered exploratory, one configuration, seeds 30..60. Any
  runtime-bound or quality-claimed L2 arm registers fresh on seeds 60..90 per
  the standing discipline (the natural home is the Step-3/v6 registration if a
  churn-mitigation point is also adopted).

The remaining #54 questions are therefore the ones Step 1 quantified — churn
(mag 8.00 vs e1 136 median, now both measured on 30..60) and latency — not
feasibility. Companion Step-1b evidence (issue comment): e1's churn correlates
weakly *positively* with its quality and with its margin over mag (Spearman
+0.22 / +0.17), so mitigation is not obviously free, but the coupling is weak
enough that a quality-vs-churn frontier sweep (Step 3) is worth running.
