# Pre-registration: K4 EQ5b — typed two-engine contest (`GroupAgent`-over-e1 vs the process arm)

**Status: REGISTERED.** Committed BEFORE implementation. Design-lock of
record: [koalisi #78](https://github.com/sustia-llc/koalisi/issues/78) — the
owner lock of 2026-08-08 (D1–D10), plus **D4a** (commit discipline), the
**S-learn** gate, the **S-learn (i) correction**, and **D2a** + the
sub-parameter pins, all posted to #78 before this document. Pre-run amendments
are legitimate per the standing protocol (committed here + posted to #78 BEFORE
any affected code); after the official run this document is immutable.

## 1. Registration statement

- **Hypothesis (EQ5, stack E-queue entry 5; EQ5b = the two-engine half).** On
  the workflow-structured world EQ5a defines, an **active-inference** engine
  that decides as a *group of role specialists* — `aif::GroupAgent` with
  role-slotted internal agents over arm-E1's persistent world model —
  outperforms the **enriched-magnitude** engine staffing the same workflows.
- **Split.** EQ5a registered the process arm and reported
  `FALSIFIED (process structure)`. This document is the separate two-engine
  contest promised there, on its own fresh seed block. **No EQ5a leg depends
  on it, and it revises no EQ5a verdict.**
- **Born on:** koalisi **`v0.29.0`** — the EQ5b pin-first re-pin
  ([PR #82](https://github.com/sustia-llc/koalisi/pull/82), merge `5fbe9f3`,
  tag pushed): `aif-v0.13.0`, catgraph `v0.8.0` ×3 (deliberately not moved),
  slm `v0.2.1`, MSRV 1.93. Drift check clean — ten suites at baseline, default
  clippy `--all-targets` clean, frozen Parts 1–10 battery reproduced with
  **zero non-latency diffs**; reviewer pass 4 findings, all applied.
- **Seeds:** `330..360`, fresh. **90..120 (lockout) and 150..180 (replication)
  remain reserved — no release implied.**
- **Battery placement:** `examples/strategy_comparison.rs` **Part 11**; frozen
  Parts 1–10 are the byte-identity regression gate (§5 X-battery).
- **Standing constraint.** This result **cannot** move the koa#54 default-arm
  decision (mag = demonstrated default, **FINAL** — owner's standing record,
  restated at the EQ4 and EQ5a locks).

### Why the gate moved, and what that cost

The lock's D4 requires a **deterministic** group decision. Verification at the
then-current `aif-v0.12.0` found **no RNG-free path to a group action
distribution**: `Agent::act` samples throughout; the nested
`action_probabilities` replaces only the final draw and still samples each
member's action to `record_action`; `VotingAgent::weighted_mixture` was
private. #78's original "no tira release is needed" claim was correct about
`with_slots`/`Aggregator` and about nesting, and **wrong** about this surface.
Filed as [tira#53](https://github.com/sustia-llc/tira/issues/53) → shipped as
`aif-v0.13.0` → re-pinned here. Recorded because the registration's own build
gate was mis-assessed once, and a reader should be able to see that.

### Falsifiability posture

This lineage has repeatedly planted signals that failed to convert (gotcha 24
rescale-not-reroute; gotcha 25 unsatisfiable predicate; gotcha 26
window-vs-lattice; gotcha 31 coverage-proxy). Four structural choices here
exist so the test can fail:

1. The control is the **EQ4-validated typed arm** (§3), so EQ5b cannot
   re-harvest EQ4's margin.
2. The arm's decision criterion is **identical in form** to arm-E1's (SP3), so
   a win is attributable to the *engine shape*, not to a re-tuned threshold.
3. `wf-val-p` — the strongest cell EQ5a measured — is reported alongside, so a
   win against the control that fails to reach it **says so** (§6).
4. Every sub-parameter is pinned to an **existing engine semantic** (§4), so
   there is no coefficient available to tune toward a pass.

## 2. World

The **v2w** regime exactly as EQ5a §2 defines it (the Part 9 draw), **verbatim
and unmodified**, on seeds 330..360. Not re-derived and not re-tuned: EQ5a's
world is a fixed, reported quantity, and holding it constant is what makes the
two documents comparable.

Demand semantics carry over unchanged, including the property EQ5a established
and #80 confirmed: **multiplicity prices a process but adds no coverage
demand** — coverage is per distinct `(bit, role)`. That is precisely the
asymmetry SP2 exploits, and precisely what magnitude is structurally blind to.

## 3. Arms

| arm | engine | role |
|---|---|---|
| `wf-asis` | typed magnitude (`with_role_modulation`, oracle `ρ = δ`) staffing the as-written workflow | **control** |
| `grp-role` | `GroupAgent`, R = 3 **persistent** role internals, role-restricted precision | **confirmatory** |
| `grp-mult` | as `grp-role`, plus multiplicity-weighted precision | **confirmatory** |
| `grp-role-fresh` | `grp-role` with internals rebuilt per decision (arm-E1 shape) | registered reference, non-gating |
| `grp-mult-fresh` | `grp-mult` with internals rebuilt per decision | registered reference, non-gating |
| `wf-val-p` | the library `ResidualPolicy` at EQ5a's pinned λ — EQ5a's strongest cell | registered reference, non-gating |
| `mag` / `scalar` / `arm-E1` | frozen untyped magnitude / scalar bridge / v5 E1 config | context, non-gating |

All arms decide over the same arrival stream under the standing battery
protocol (bootstrap first arrival, one leave sweep per task, churn = all
removals, latency record-only).

**Why the fresh legs exist (D2a).** arm-E1's query POMDP is documented *"fresh
per decision"*. Had EQ5b's internals inherited that, `record_group_action`
would have had nothing to advance and **D4a's commit would have been a silent
no-op**. Rather than settle it by implication, both lifetimes are measured: the
contrast isolates the members' **own accumulated trajectory** from the shared
persistent model — "a group of role specialists" versus "three views of one
model, aggregated". The fresh reference runs at **both** channels deliberately;
a single-channel reference would break D3's attribution symmetry and invite
post-hoc pairing with whichever confirmatory cell carried.

## 4. The group arm (D2, D2a, D3, D4, D4a + sub-parameter pins)

**Engine shape.** `aif::GroupAgent` with a `CopyAgent` sensory slot, **R = 3**
role internals, and a `VotingAgent` active slot in **`CertaintyWeighted`**
mode.

**CW is forced, not chosen.** With 3 voters over a 2-action space,
`Deterministic` returns uniform-over-winners — and a tie is arithmetically
impossible, counts summing to 3 — so it is always a delta; `Probabilistic`
returns counts/3 ∈ {0, ⅓, ⅔, 1}. Neither carries a usable score gradient for
PRIMARY, churn or superiority. Under CW the read is **exactly the mixture
`act` would sample from**.

**Decision surface (D4).** `GroupAgent::group_distribution` — no RNG draw, no
`last_action` advance. `aif-v0.13.0` or later is therefore a hard requirement,
not a convenience.

**Commit discipline (D4a).** Pure read → decide → `record_group_action`.
Members advance to the **group's resolved action**, which is what a coalition
decision is. **Never combined with `group_distribution_recording`**: two
`record_action`s with no observation between them leave the action history one
long, so `last_action` and the smoother's next transition disagree until the
window slides — no panic, silent drift.

**Sub-parameter pins.** Each reuses an existing engine semantic. This lineage
does not grant free parameters, and the cheapest way to honour that is to
introduce none.

- **SP1 — precision from role demand.** Internal agent `r` is built by
  arm-E1's **existing** query construction with `required_r` substituted for
  `required`, where `required_r = { b : (b, r) ∈ distinct demand }`. The
  coverage-masked `initial_pa` injection is reused verbatim.
- **SP2 — multiplicity weighting (`grp-mult` only).** Injected Dirichlet counts
  for `(b, r)` are scaled by the occurrence multiplicity `m(b, r)` of that step
  in the declared writing. Justification is the engine's own semantics —
  Dirichlet counts **are** observation counts, so a step occurring three times
  carries three observations' worth of evidence demand. Not a tuned
  coefficient.
- **SP3 — decision rule.** `n_actions = 2` (`0` = decline, `1` = act); act iff
  `p(act) > 0.5`; **ties → decline**. Identical in form to arm-E1's
  deterministic rule on the marginal `p(control 1)`, so EQ5b changes the
  *engine*, not the criterion.
- **Base configuration.** arm-E1's registered v5 E1 config
  (`persistent_learning: true`, `query_dynamics: false`, `query_novelty: true`,
  fixed γ = 16, `n_bits = 8`), unchanged. EQ5b contests the group *shape* over
  that model, not a re-tuned model.

**Placement (D8).** Library surface behind `decision` + `process` (the arm
needs both). Battery scaffolding — world draw, scorers, tables — stays
example-side. Upstream error handling unchanged: an `AifError` ⇒ decline and
count, never a panic, never a silent fallback.

## 5. Registered legs and gates

### Confirmatory

- **H-G (family-wise, D6).** On the v2w world, seeds 330..360, each of
  `grp-role` and `grp-mult` against `wf-asis`: PRIMARY median ≥ **1.25×** the
  control's **AND** strictly superior on ≥ **18/30** seeds. Both conjuncts, in
  the same cell. Either cell may carry the verdict. The bar follows the
  cell-count rule pinned at lock time, before the cells were fixed: ≤ 2
  confirmatory cells ⇒ the lineage's standing 1.25× / ≥ 18-of-30; ≥ 3 ⇒ EQ5a's
  1.4× / ≥ 21-of-30. **All cells report regardless of outcome** — no
  cell-shopping, no post-hoc bar movement.

### Registered exploratory / disclosure (all non-gating)

- **E-life.** `grp-*` versus `grp-*-fresh` at both channels: how much of any
  margin comes from the members' own accumulated trajectory rather than the
  shared persistent model.
- **E-seed.** A seeded-sampling cell deciding by the shipped `act()` draw
  instead of the deterministic read — what the draw costs against argmax.
- **E-lat.** Per-arm µs/decision. Record-only, **never gating**.
- **E-agree.** Rate at which the three role internals agree, and the
  distribution of the CW mixture's margin — the mechanism read behind any
  H-G result.

### Gates (any failure ⇒ RUN-INVALID)

- **X-battery.** Frozen Parts 1–10 byte-identical against a pre-change
  baseline; latency-only diffs are the standing exclusion. Checked **outside**
  the binary.
- **X-identity.** At the identity configuration the arm reproduces its
  reference bit-for-bit on acts and raw score bits.
- **S-determinism.** Same seed ⇒ same result, asserted per seed. Load-bearing
  here in a way it has not been for any prior arm: every path through
  `GroupAgent` other than `group_distribution` consumes randomness.
- **S-learn.** Three conjuncts; **(i) and (ii) are RUN-INVALID conditions, not
  disclosures** (the #80 S-probe precedent).
  - **(i) — the world model advances the correct number of times.** Per seed,
    the persistent world model records **exactly one** update per task
    (`observe_outcome`), **and** on the persistent legs each role internal
    records **exactly one** advance per decision it participated in. **Both a
    deficit and a surplus are RUN-INVALID.** On the fresh legs only the
    persistent-model count applies — internals are single-use by construction,
    so no member-advance count exists; stated explicitly so the gate is not
    silently vacuous there. A non-vacuity guard is retained (end-of-stream
    state differs from initialization beyond a pinned tolerance) — necessary,
    **and explicitly not sufficient**.
  - **(ii) — every scored read is committed.** Counted, not asserted by
    construction: committed group actions on the decision path equal decisions
    taken, and uncommitted reads on that path are **0**.
  - **(iii) — the learning is load-bearing** (causal pin, non-gating). A
    learning-suppressed configuration collapses toward its degenerate
    reference, in the shape of v5's X1 novelty-off ablation and #80's λ = 0
    identity pin.
- **S-live.** The arm's decisions genuinely diverge from the control, reported
  as **act-vs-score-bit divergence**. The #80 lesson (gotcha 31): a term can be
  live in the score and dead at the decision, and a leg reading only PRIMARY
  sees a flat null where the truth is "it reached the margin and never crossed
  a threshold".

**Why S-learn (i) counts rather than merely detects movement.** It was first
drafted as "the world model moves", against the `MeanField` fixed point: a read
with no commit leaves `last_action` at `None`, the member ignores observations,
and nothing moves. But koalisi's world model is **MMP**
(`aif_persistent_policy.rs:568`), where the identical missing commit fails in
the **opposite direction** — per `aif-v0.13.0`, an observation with no action
recorded since the previous one **supersedes** it rather than opening a new
window node, so two commit-free reads are two looks at the same timestep and
the belief update, plus the Dirichlet update under `learn_*`, **applies
twice**. MeanField freezes; MMP silently over-learns. A double-updating arm
moves briskly and would have **passed the gate written to catch it**. Caught by
the v0.29.0 re-pin review before this document existed.

**Standing disclosure.** A zero score is **not** a decline. Evaluation failure
is detected independently, never inferred from the score (EQ5a A5.3 / gotcha
30–31).

## 6. Verdict labels (pre-committed)

- **`VALIDATED (two-engine)`** — H-G passes both conjuncts in at least one
  confirmatory cell, all gates hold.
- **`FALSIFIED (two-engine)`** — gates hold, no confirmatory cell passes both
  conjuncts.
- **`RUN-INVALID`** — any gate fails.

**Pre-committed interpretation, binding, fixed before any number is visible.**
The bar is computed against `wf-asis` and **only** `wf-asis`. A PASS that does
**not** also exceed `wf-val-p`'s median is reported as *"beats the typed
control, not the strongest process cell"*. Making `wf-val-p` a second conjunct
was considered and rejected: #80 measured that lever as a **coverage proxy, not
process-specific** (`lift_wf == lift_flat`), so hanging a verdict on it would
gate EQ5b on a mechanism already partly discredited.

A `VALIDATED` here would mean *"on this world, a role-structured active-inference
group outperforms the typed magnitude arm"* — **not** that aif beats magnitude
generally, and **not** anything about koa#54, which stays FINAL.

## 7. What this cannot settle

- The v1/v2 K4 verdicts, EQ3's, EQ4's, EQ5a's, and #80's — all untouched.
- The koa#54 arm question — closed, and no EQ5b outcome reopens it.
- Whether any advantage transfers off the v2w world. One world, one draw
  family, 30 seeds.
- Whether the deterministic read is the *right* group decision rule. It is the
  one that preserves this lineage's determinism discipline; the seeded-sampling
  contrast (E-seed) is exploratory and carries no verdict.

## 8. Report

`docs/ab-report-K4-eq5b-typed-two-engine.md`, with a mandatory
implementation/deviation ledger. This document is immutable after the official
run; follow-ups land as an appended addendum, never as edits to registered
sections.
## Amendment 1 (pre-run, 2026-08-08) — the registered arm was unimplementable

Posted to #78 **before** any implementation code exists. The prereg
(`a2ca328`) was committed first, the implementation dispatch read it as the
spec, and it **stopped without writing code** because three structural facts
about `aif-v0.13.0` make the arm as registered unbuildable. All three verified
against the pinned rev `74c8b12`. Owner resolutions below; the prereg is
amended to match before re-dispatch.

This is the process working as designed — the same shape as EQ5a's five
pre-run amendments. Recording it in full because two registered elements
**weaken** as a result, and that must not read as an oversight later.

### The three blockers (verified, not asserted)

- **B1 — fatal.** SP1 pins each internal as an arm-E1 query POMDP, but that
  construction bakes the **per-decision candidate** into the member's
  matrices: `aif_persistent_policy.rs:613`'s `pa_counts` builder selects
  `cover_mask` from `cfg0`/`cfg1`, the *this-candidate* coverage masks, and the
  model shape (`r` modalities, `r+1` factors) comes from `required`. Meanwhile
  `POMDPAgent` exposes **no setter** for `a`/`b`/`c`/`d`, and `GroupAgent`
  exposes only `internal_agents(&self) -> &[I]` (`group.rs:718`) — no mut
  accessor, no consuming accessor, no member swap. An SP1-built member is
  therefore candidate-specific, cannot be re-parameterized, and cannot be
  extracted. **D2a-persistent and SP1 were mutually exclusive.**
- **B2.** `group_distribution(&mut self, observation: usize)` (`group.rs:838`)
  is single-modality all the way down, while an SP1 member carries
  `|required_r|` modalities. Belief updates and `update_a` iterate
  `obs.iter().enumerate()`, so **only modality 0 would ever learn**, and an
  out-of-range observation panics rather than erroring.
- **B3.** `required_r` is frequently **empty** — `draw_tags` assigns each
  required bit one of 3 roles independently, so P(a given role draws nothing)
  = `(2/3)^|required|`, which at `|required| = 2` is ~44 %. But the group
  rejects any member not returning a length-2 distribution
  (`InvalidLength`, `group.rs:930`).

### A1.1 (B1) — role-specialised world models, fresh query members

**The confirmatory legs are rebuilt as:** each of the R = 3 roles owns its own
**persistent world model** (arm-E1's persistent agent, restricted to that
role's outcomes); the group's internals are **fresh SP1 queries per decision**
built from those per-role models.

**D2a's contrast is not lost — it relocates.** It is no longer
member-continuity but **world-model topology**:

| leg | world models | role |
|---|---|---|
| `grp-role`, `grp-mult` | **R = 3 role-specialised** persistent models | **confirmatory** |
| `grp-role-fresh`, `grp-mult-fresh` | **one shared** persistent model, three role-restricted views | registered reference, non-gating |

That is a more faithful reading of the prereg's own gloss — "a group of role
specialists" versus "three views of one model, aggregated" — than member
continuity ever was. The specialisation now lives where learning lives.

**D4a is STRUCK.** With members discarded after each decision,
`record_group_action` advances nothing. The arm uses the pure read
(`group_distribution`) only, and commits nothing. Retaining the commit would
be ceremony that reads as meaningful and is not.

### A1.2 (B2) — a koalisi `InternalAgent` wrapper carrying the replay vector

Members are a koalisi-side `InternalAgent` wrapper over `POMDPAgent` holding a
per-decision **multi-modality** observation vector and calling
`action_probabilities_multi`. Legal — `GroupAgent<S, I, X>` is generic in `I` —
and RNG-free, since gotcha 32's carve-out concerns nested `GroupAgent`s and
sampling sensory slots, not custom leaf wrappers.

This preserves arm-E1's **A1.4 two-task replay**, which the scalar
`group_distribution` observation would have silently discarded. Since EQ5b's
whole premise is carrying v5's validated mechanism into the workflow world,
dropping the replay would have hollowed out the claim while leaving it
superficially intact.

### A1.3 (B3) — empty-demand roles leave the roster

A role with no `(bit, role)` demand in a task **does not vote in that task**:
it is dropped from the roster, so R varies 3 → 2 → 1 per task. Semantically
right — a role with no stake has no opinion — and it avoids the alternative's
bias, since under `CertaintyWeighted` an abstaining member at `[0.5, 0.5]`
still carries weight `exp(−ln 2) = 0.5` and would systematically drag `p(act)`
toward the SP3 threshold on every affected slot.

**Mandatory disclosure:** the per-seed distribution of realised roster size,
and the rate of empty-demand role-slots. A task where **every** role is empty
cannot occur (`|required| ≥ 2`, so some role holds demand).

### A1.4 — S-learn is re-scoped, and two conjuncts weaken

Stated plainly rather than quietly edited, because S-learn was strengthened
two days' worth of decisions ago and now gives ground:

- **S-learn (i) — retained, re-targeted.** Each of the R per-role persistent
  world models records **exactly one** update per task **in which that role
  had demand** (tasks where the role was dropped per A1.3 are excluded from
  its expected count). Deficit or surplus is **RUN-INVALID**. The non-vacuity
  guard is retained.
- **S-learn (i)'s member-advance conjunct — STRUCK as vacuous.** Members are
  single-use on every leg now, so no member-advance count exists to check.
- **S-learn (ii) — STRUCK as vacuous.** There is no commit on the decision
  path, so "every scored read is committed" has no referent.
- **S-learn (iii) — unchanged.**

**The MMP double-update hazard is now designed out, not gated.** It required a
member read twice without a commit between; single-use members cannot be read
twice. That is a strictly better outcome than gating it — but the reasoning is
recorded here so that a future reader finding a thinner S-learn than the
2026-08-08 correction described does not mistake it for drift. The hazard and
its mechanism remain documented in koalisi **gotcha 32** and still apply to any
future arm that gives its members continuity.

### Unchanged

D1 (v2w world verbatim), D2 (`GroupAgent`, R = 3, CW forced), D3 (both
precision channels), D5 (control `wf-asis`, `wf-val-p` non-gating reference
under the scoped-claim language), D6 (**1.25× / ≥ 18-of-30**, 2 confirmatory
cells), D7 (seeds **330..360**), D8 (Part 11, library behind `decision` +
`process`), D10. SP1, SP2, SP3 unchanged. X-battery / X-identity /
S-determinism / S-live unchanged. koa#54 stays FINAL.
