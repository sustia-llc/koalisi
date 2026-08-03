//! Categorical-magnitude coalition-decision strategy (feature `magnitude`).
//!
//! This is the categorical A/B mirror of the Active Inference arm
//! ([`super::aif_policy`], feature `decision`). Where the AIF arm scores a
//! coalition by the (negated) expected free energy `−G` of a capability-coverage
//! POMDP, this arm scores it by its **coalition magnitude** at the pinned scale
//! `t = 1` — the "effective-member diversity" of the coalition read as a
//! cospan-weighted subgraph of an enriched category. The two arms are fully
//! independent: the `magnitude` feature pulls in `catgraph-magnitude` and never
//! references `aif`, and vice versa.
//!
//! The magnitude math lives upstream in
//! [`catgraph_magnitude::coalition`](https://github.com/sustia-llc/catgraph) —
//! see that module's docs for the full citation chain. In brief: Bradley–Vigneaux
//! 2025 (*Magnitude of Language Models*, arXiv:2501.06662) §3.5 Eq (7) defines
//! magnitude as the Möbius sum `Mag(tM) = Σ_{x,y} ζ_t⁻¹(x, y)` over the induced
//! Lawvere metric space; Bradley–Terilla–Vlassopoulos 2021 (arXiv:2106.07890)
//! supply the `[0, 1]` enrichment. koalisi only *maps* coalition membership onto
//! the couplings that upstream consumes — it does not re-derive the algebra.
//!
//! # The capability→coupling mapping (the semantic heart)
//!
//! [`catgraph_magnitude::coalition_value`] takes directed couplings
//! `(from, to, prob ∈ [0, 1])` — the hom-objects `A(i, j)` of the enriched
//! category (gemini-spec §IV.5 mapping). This arm derives them as a **directed
//! substitutability / containment** relation over the *task-relevant*
//! capabilities. Writing `rel_x = capabilities_x & required_capabilities`:
//!
//! ```text
//! A(i → j) = popcount(rel_i & rel_j) / popcount(rel_i)   if rel_i != 0
//! A(i → j) = 0.0                                         if rel_i == 0
//! ```
//!
//! Read `A(i → j)` as "the fraction of `i`'s task-relevant capabilities that `j`
//! also has" — the probability that `j` can stand in for `i` on this task.
//! Agents with `rel = 0` never actually reach the coupling: they are **excluded
//! from the member set** before the magnitude computation (see rationale
//! point 4). The resulting design choices (all hand-verified against the
//! upstream Möbius math):
//!
//! 1. **Clones merge (deliberate skeletalization).** Two agents with identical
//!    relevant masks are mutually coupled at exactly `1.0`, so upstream
//!    quotients them into ONE effective agent and the magnitude is unchanged.
//!    This deliberately mirrors the AIF arm's clone degeneracy (a redundant
//!    clone leaves `G` unchanged): task-interchangeable agents *are* one
//!    effective agent.
//! 2. **Subsumption is directional.** If `rel_j ⊃ rel_i` then `A(i → j) = 1.0`
//!    but `A(j → i) < 1.0`, so the pair is *not* merged; instead the subsumed
//!    agent receives Möbius weight `0` and contributes zero diversity (hand
//!    check: `ζ = [[1, 1], [0.5, 1]] ⇒ w = (0, 1) ⇒ Mag = 1.0`).
//! 3. **Complementary specialists count fully.** Disjoint relevant masks ⇒ zero
//!    couplings ⇒ `Mag = m` (the member count).
//! 4. **Irrelevant agents are excluded.** An agent with `rel = 0` is dropped
//!    from the member set before the magnitude computation, mirroring the AIF
//!    arm (where such an agent leaves capability coverage unchanged): as a
//!    candidate it produces join margin `0` ⇒ decline; as a member it produces
//!    leave delta `0` ⇒ leave. Exclusion — not a vacuous `1.0` coupling — is
//!    load-bearing: a one-way `1.0` coupling to every member would drive the
//!    irrelevant agent's Möbius weight *negative* and collapse coalition
//!    diversity (three disjoint specialists plus one irrelevant bystander would
//!    score `1.0` instead of `3.0`, and the bystander's presence would eject a
//!    unique specialist on leave).
//! 5. **`required == 0` ⇒ every agent is irrelevant** ⇒ every coalition scores
//!    `0` ⇒ the join margin is `0` ⇒ decline, and every member's leave delta is
//!    `0` ⇒ leave (mirrors the AIF arm, whose coverage is pinned at `1.0` both
//!    sides when nothing is required: join margin `0`, leave delta `0`).
//! 6. **No cross-arm calibration.** The score is *raw* magnitude; only the
//!    within-arm rank order is meaningful. The A/B metric that compares this arm
//!    to `−G` is pre-registered on koalisi #7 — this policy does not attempt to
//!    put magnitude and free energy on a common scale.
//!
//! # Execution
//!
//! Coalition magnitude is `O(m³)` (max-product transitive closure + Gaussian
//! elimination on the `m × m` zeta table). Unlike the AIF arm — where only the
//! final EFE eval is offloaded — here the *entire* magnitude computation is the
//! CPU-heavy part, so [`should_join_async`](MagnitudePolicy::should_join_async)
//! and [`should_leave_async`](MagnitudePolicy::should_leave_async) snapshot the
//! capability masks to owned `Vec<u32>` in a sync prologue (the `&dyn` borrows
//! are not `'static`) and run the whole closure on the rayon pool via
//! [`tokio_rayon::spawn`], leaving the tokio worker unblocked.
//!
//! Upstream errors ([`CatgraphError`]) are treated as **policy-level outcomes**,
//! never panics: a value calculation logs and returns [`f64::NEG_INFINITY`]; a
//! policy decision logs and declines with score `0.0`.
//!
//! # Incremental evaluation (the K6 hot path, koalisi #14)
//!
//! A coalition-decision loop sweeps every candidate agent `x` against a fixed
//! coalition `S`, scoring `Mag(S ∪ {x})` once per candidate. Naively that is a
//! full `O(m³)` magnitude per candidate even though every `S ∪ {x}` differs from
//! `S` by a single member. Upstream #31 caches the expensive parts of `S` once
//! in a [`CoalitionEvaluator`] and answers each `Mag(S ∪ {x})` with an
//! `O(m²)`-ish bordered (Schur) update (BV 2025 §3.5 Eq 7; the Schur/bordered
//! derivation is documented on [`CoalitionEvaluator::value_with`], not
//! re-derived here).
//!
//! Both [`MagnitudePolicy`] and [`MagnitudeValueCalculator`] hold a
//! membership-keyed evaluator cache behind interior mutability (the
//! [`CoalitionDecisionPolicy`] / [`ValueCalculator`] seams take `&self`). The
//! cache entry also carries one [`catgraph_magnitude::EvalScratch`] (EQ3 lever
//! L1, upstream #33): the seven per-query border/Schur `Vec`s are drawn from it
//! instead of being heap-allocated per candidate. The scratch holds **no
//! cross-call state** (upstream reuse contract) — results are bit-identical to a
//! fresh scratch — so it is retained across evaluator rebuilds exactly like the
//! candidate registry. The cache keys on `(required, member_masks)`:
//!
//! - [`CoalitionEvaluator::base_value`] is **bit-identical** (`==`) to a fresh
//!   [`catgraph_magnitude::coalition_value`] on the same members, so every
//!   value-level result (and the `Mag(S)` side of each margin) is unchanged.
//! - [`CoalitionEvaluator::value_with`] scores `Mag(S ∪ {x})` within
//!   [`catgraph_magnitude::INCREMENTAL_REL_TOL`] (`1e-9` relative) of fresh, and
//!   ranks candidates identically — so the *decisions* are preserved.
//!
//! # Decision-freeze guarantee (the knife-edge band)
//!
//! The join/leave rules compare the margin against a threshold with a strict
//! test (`margin > join_margin`, `delta <= 0`). Rank-order identity does
//! **not** protect a margin-vs-threshold comparison: a candidate whose *true*
//! margin sits exactly at the threshold (for the default `join_margin = 0`,
//! any subsumed or redundant agent) is decided entirely by float noise, and
//! the incremental path's noise (often exactly `0.0`) can disagree with the
//! pre-evaluator fresh noise (`±1e-16`-scale). To keep decisions bit-frozen to
//! the committed fresh behaviour, any incremental margin that falls inside a
//! relative band around **the threshold the decision actually uses** —
//! `join_margin` on joins (caller-settable, not necessarily `0`), `0` on
//! leaves — has its `Mag(S ∪ {x})` / `Mag(S)` side recomputed **fresh**
//! ([`KNIFE_EDGE_REL_BAND`], `1e-6` — comfortably wider than the `1e-9`
//! incremental tolerance yet far below any genuine diversity margin of the
//! discrete capability-mask domain). Because `base_value` is already
//! bit-identical to fresh `Mag(S)`, the recomputed margin equals the pre-K6
//! fresh margin bit-exactly, so the threshold test reproduces it. Margins
//! **outside** the band have contract-guaranteed agreement (the incremental
//! error cannot span a `1e-6`-wide gap), so they keep the fast incremental
//! value.
//!
//! This band logic is **frozen**: it is exactly what the default policy does,
//! before and after EQ3.
//!
//! # The opt-in EQ3 arm (koalisi #69, feature `magnitude-fast`)
//!
//! [`MagnitudePolicy::with_eq3_levers`] turns on two levers together. It
//! defaults **off**, and off is the frozen decision surface above.
//!
//! ## L2 — the exact zero-diversity proof branch (upstream #153)
//!
//! Part of the knife-edge population is *structurally* decidable, so it does
//! not need the fresh recompute at all. On this arm the candidate query is
//! [`CoalitionEvaluator::value_with_report_scratch`], and when the returned
//! [`catgraph_magnitude::JoinReport`] carries a
//! [`catgraph_magnitude::ZeroDiversityProof`] the candidate's **real**
//! increment is exactly `0` (a mutual-`1.0` skeletal merge without interior
//! improvement, or an incoming/outgoing closed-profile duplicate — each decided
//! by exact `f64` comparison, no tolerance anywhere). Those candidates take
//! `with := base`, so the margin is exactly `0.0` and the fresh recompute is
//! skipped. Per the upstream contract the branch is on the **proof**, never on
//! `increment() == 0.0` — a proven-zero candidate can still return a computed
//! increment of order `1e-14`; and a proof of `None` means *not proven*, never
//! "nonzero", so those candidates keep the band logic.
//!
//! **This lever changes decisions, deliberately** — which is why it is opt-in
//! (prereg Amendment 1 / A1.1). Where a certified-zero candidate's *fresh*
//! magnitude lands on positive roundoff (`+2e-16 … +7e-16` measured), the
//! frozen path joined on that noise and this arm declines on the exact zero:
//! 0.77 % of certified candidates on the module's 60-seed corpus, all in the
//! two profile-duplicate classes (skeletal merge measured 0/465 — it is
//! bit-identical to fresh). The default path is therefore left untouched, and
//! the K6 knife-edge regression fixture keeps its original assertions.
//!
//! ## L3 — `f64` routing of fresh evaluations (upstream #165)
//!
//! The same toggle routes the policy's **fresh** full-coalition evaluations —
//! the unprovable-knife-edge recompute, the leave-side fresh pair, and the
//! empty / sole-member guards — through upstream's `f64` factorization handle
//! (see `magnitude_of_masks_f64`) rather than calling
//! [`catgraph_magnitude::coalition_value`] directly. What that handle does is
//! **conditional**, and the condition matters here: it factors ζ with Cholesky
//! (or LBLT when symmetric-indefinite) only if ζ is *exactly* symmetric, and
//! otherwise falls back to the rig-generic Gauss–Jordan route.
//!
//! On koalisi traffic ζ is usually **not** symmetric. The substitutability
//! coupling `A(i → j) = |rel_i ∩ rel_j| / |rel_i|` is asymmetric whenever
//! `|rel_i| ≠ |rel_j|`, so any coalition mixing agents of different relevant
//! widths lands on the fallback — which re-enters the generic path *after*
//! paying the dense-matrix build and the symmetry scan, i.e. as net overhead.
//! Measured on v2-shaped draws (8753 evaluations): Cholesky 29.1 % (about 45 %
//! of those the trivial `k = 1` case), LBLT 0.0 %, Gauss–Jordan 70.9 %, with
//! the fast-route share decaying monotonically as coalitions grow. Any latency
//! claim about L3 must therefore be read against the per-run
//! `FactorizationPath` instrumentation row, which discloses that split for the
//! run being reported.
//!
//! Where the fast route *is* taken, agreement with the generic path is
//! *tolerance*, not bit-identity (upstream's ULP-identity contract covers the ζ
//! entries, not the inversion) — measured, not assumed. The evaluator-cached
//! values (`base_value` / `value_with_scratch`) are never routed: only fresh
//! evaluations are.
//!
//! # The typed-roles arm (koalisi #72, EQ4)
//!
//! [`MagnitudePolicy::with_role_modulation`] attaches a role assignment
//! (`agent_id → RoleId`) and a role-interaction table `ρ : R × R → [0, 1]`. The
//! arm then modulates every substitutability coupling before evaluation —
//! `A′(i → j) = ρ(role_i, role_j) · A(i → j)`, via
//! [`catgraph_magnitude::modulate`] (upstream #211 T2) — and evaluates magnitude
//! on the modulated table. Under the BTV lift `d = −ln π` the modulation is
//! additive in the metric, so a `ρ ≤ 1` entry can only *lengthen* a distance:
//! cross-role substitutability is weakened, never invented.
//!
//! What the arm does **not** do: the relevance masks stay **untyped**
//! (`rel = caps & required`; no role enters the mask), so roles reach a decision
//! only through the valuation layer. That is the registered lever, not an
//! omission — the arm tests whether this one multiplication converts role
//! structure into decisions.
//!
//! The registered contract (prereg §4, `docs/prereg-K4-eq4-typed-roles.md`):
//!
//! 1. **Identity default.** With no typed configuration,
//!    [`should_join`](CoalitionDecisionPolicy::should_join) /
//!    [`should_leave`](CoalitionDecisionPolicy::should_leave) and the async
//!    overrides route *structurally* to the untyped path described above — same
//!    evaluator cache, same knife-edge band, same code. Identity is by
//!    construction, not by measurement.
//! 2. **Fresh evaluation, no cache, no band.** The typed path evaluates *both*
//!    sides of every decision with a fresh
//!    [`catgraph_magnitude::coalition_value`] over the modulated couplings. The
//!    K6 evaluator cache is deliberately not reused: it keys on
//!    `(required, member_masks)`, which cannot separate two agents that share a
//!    capability mask but carry different roles — such a pair would hit an entry
//!    built for a different coupling table (the gotcha-16 precedent, where
//!    change-driven sampling likewise bypasses the evaluator). With no
//!    incremental value there is also nothing for the knife-edge band to
//!    reconcile: the band exists to make an incremental margin reproduce the
//!    fresh one, and here both sides are already fresh. Latency is the price of
//!    that, reported and never gating.
//! 3. **Exclusion precedes modulation.** Task-irrelevant agents (`rel == 0`) are
//!    dropped by [`relevant_masks`]'s contract *before* any role is read, so `ρ`
//!    can never resurrect a bystander — such an agent's role is never consulted.
//! 4. **Decline, never panic.** A *participating* agent (one that survived
//!    relevance filtering) with no role assignment declines the decision
//!    (`act = false`, `score = 0.0`) after a `tracing::warn!`, exactly as an
//!    upstream [`CatgraphError`] does; a role outside `ρ`'s range is an upstream
//!    [`catgraph_magnitude::modulate`] error and declines the same way. On the
//!    [`MagnitudeValueCalculator`] surface both cases return
//!    [`f64::NEG_INFINITY`], mirroring that surface's existing error convention.
//!
//! Interaction with the other opt-ins: [`MagnitudePolicy::with_eq3_levers`] and
//! [`MagnitudePolicy::with_evaluator_leave`] both govern the *untyped* cached /
//! fresh evaluation sites, and neither has an effect while a typed configuration
//! is set — the typed path has no cached candidate query for the L2 proof branch
//! to fire on, no reduced-set evaluator for leave variant B to use, and takes the
//! generic `coalition_value` route (never the L3 `f64` factorization) on every
//! evaluation.
//!
//! `t = 1` stays pinned (never exposed). Removal is an upstream non-goal
//! (max-product closures do not downdate), so `should_leave` defaults to the
//! fresh two-evaluation path; an opt-in reduced-set-evaluator leave variant
//! ([`MagnitudePolicy::with_evaluator_leave`]) is gated behind the #14 parity
//! test because its `Mag(S)`-with-leaver side is only tolerance-equal, not
//! bit-equal, to fresh. For a one-shot `(Mag(S), Mag(S ∪ {x}))` pair where
//! holding an evaluator handle does not fit, see
//! [`catgraph_magnitude::coalition_value_delta`].

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use catgraph_magnitude::{CatgraphError, CoalitionEvaluator, EvalScratch, RoleId, RoleModulation};
#[cfg(feature = "magnitude-fast")]
use catgraph_magnitude::{Coalition, HomMap, LawvereMetricSpace, UnitInterval, ZeroDiversityProof};

use crate::algorithms::{AgentCapabilities, ValueCalculator};

use super::{CoalitionDecisionPolicy, Decision, DecisionContext};

/// Relative half-width of the knife-edge band around the decision threshold.
/// When the incremental margin `value_with − base_value` falls inside the band
/// around the threshold the decision compares against (`join_margin` on the
/// join path, `0` on the leave path), the `Mag(S ∪ {x})` side is recomputed
/// fresh so the margin-vs-threshold comparison reproduces the pre-evaluator
/// float noise bit-exactly. The band only needs to exceed the incremental
/// error (`INCREMENTAL_REL_TOL` = 1e-9 relative); a wider band never changes a
/// decision — it only trades speed — so it is set well wide of the tolerance
/// and well below any genuine diversity margin of the discrete
/// capability-mask domain.
const KNIFE_EDGE_REL_BAND: f64 = 1e-6;

/// Whether the incremental margin `with − base` is within
/// [`KNIFE_EDGE_REL_BAND`] of `threshold` — a knife-edge whose
/// margin-vs-threshold comparison is float-noise-dominated and must be
/// reproduced by a fresh evaluation rather than the incremental one. The
/// threshold is anchored to what the decision actually compares against
/// (`join_margin` for joins, `0` for leaves), so the decision-freeze guarantee
/// holds for any caller-set `join_margin`, not only the default `0.0`.
fn is_knife_edge(with: f64, base: f64, threshold: f64) -> bool {
    (with - base - threshold).abs() <= KNIFE_EDGE_REL_BAND * with.abs().max(base.abs()).max(1.0)
}

/// A base coalition `S` cached as a membership-keyed [`CoalitionEvaluator`] plus
/// the registry of distinct candidate capability masks scored against it.
///
/// Keyed by `(required, member_masks)`: a hit (same requirement, same ordered
/// relevant member masks) answers `Mag(S ∪ {x})` for any already-registered
/// candidate mask with an `O(m²)` [`CoalitionEvaluator::value_with`]; a new
/// candidate mask, a membership change, or a `required` change rebuilds the
/// evaluator (≈ one fresh evaluation). The `candidate_masks` registry is
/// **retained across rebuilds** — including `required` changes, since couplings
/// are recomputed from raw masks at build time, so a previously-seen raw-caps
/// mask stays a valid pool slot and maximizes `value_with` reuse (measured on
/// the K4 battery: scoping the registry to one `required` degenerates the cache
/// to rebuild-per-decision, because each task draws a fresh requirement and each
/// candidate arrives once — the retained registry is what makes post-warm-up
/// joins O(m²) hits). Growth is bounded by [`REGISTRY_CAP`]: when a rebuild
/// would carry a full-cap registry, the registry is reset — to just the
/// current candidate on the join path, or to empty where no candidate is in
/// play ([`cached_base`]) — a rare full re-registration burst, amortized
/// across the stream, keeping both memory and the O(registry × m) build cost
/// bounded.
#[derive(Debug, Clone)]
struct EvaluatorCache {
    /// The task requirement the cached evaluator was built for.
    required: u32,
    /// The coalition's deduped, task-relevant member masks, in exactly the order
    /// [`relevant_masks`] produced them (order is load-bearing: it fixes the
    /// member-index → local-index map that keeps `base_value()` bit-identical to
    /// the fresh path).
    member_masks: Vec<u32>,
    /// Distinct raw candidate capability masks seen so far; candidate `s`
    /// occupies evaluator agent slot `member_masks.len() + s`.
    candidate_masks: Vec<u32>,
    /// The upstream incremental evaluator over agents
    /// `0..(member_masks.len() + candidate_masks.len())`, members `0..m`.
    evaluator: CoalitionEvaluator,
    /// Reusable border/Schur working buffers for
    /// [`CoalitionEvaluator::value_with_report_scratch`] (EQ3 L1, upstream #33).
    /// Carries **no** cross-call state — it is an allocation pool, not cache
    /// state — so it is kept across evaluator rebuilds (where the whole point of
    /// the pool would otherwise be lost on rebuild-heavy streams) and yields
    /// results bit-identical to a fresh scratch.
    scratch: EvalScratch,
}

/// Build a [`CoalitionEvaluator`] whose members are `member_masks` (agent slots
/// `0..m`) and whose candidate pool is `candidate_masks` (slots `m..`), at the
/// pinned scale `t = 1`.
///
/// The member↔member couplings are emitted in the **same** `i`-outer / `j`-inner
/// order and with the same `p > 0.0` filter as [`magnitude_of_masks`], so the
/// evaluator's base coalition is built from an identical coupling set by an
/// identical route — the contract that keeps [`CoalitionEvaluator::base_value`]
/// bit-identical to a fresh [`magnitude_of_masks`] on `member_masks`. Each
/// candidate then contributes its member↔candidate couplings in both directions
/// (same [`CouplingModel::coupling`], same filter); candidate↔candidate
/// couplings are never read by [`CoalitionEvaluator::value_with`] and are
/// omitted.
///
/// `member_masks` must be non-empty (callers guard the empty case up front).
///
/// # Errors
///
/// Propagates any [`CatgraphError`] from [`CoalitionEvaluator::new`]. Its
/// validation and singular-`ζ` checks depend only on the members and their
/// couplings, so construction errors **iff** a fresh
/// [`magnitude_of_masks`] on `member_masks` would (the candidate couplings are
/// always valid `[0, 1]` off-diagonal probabilities and cannot introduce a new
/// error).
fn build_evaluator(
    required: u32,
    member_masks: &[u32],
    candidate_masks: &[u32],
) -> Result<CoalitionEvaluator, CatgraphError> {
    let m = member_masks.len();
    let total = m + candidate_masks.len();
    let agents: Vec<usize> = (0..total).collect();

    let mut couplings: Vec<(usize, usize, f64)> = Vec::new();

    // Member↔member — identical order/filter to `magnitude_of_masks`.
    for (i, &from) in member_masks.iter().enumerate() {
        for (j, &to) in member_masks.iter().enumerate() {
            if i == j {
                continue;
            }
            let p = CouplingModel::coupling(from, to, required);
            if p > 0.0 {
                couplings.push((i, j, p));
            }
        }
    }

    // Member↔candidate, both directions (candidate slot = m + s). A registry
    // mask with nothing task-relevant under the CURRENT `required` yields 0.0
    // in both directions for every member — skip its inner loop entirely (the
    // registry is retained across `required` changes, so stale-irrelevant masks
    // are common; the slot still exists for index stability, it just carries no
    // couplings, exactly as if each 0.0 had been filtered).
    for (s, &cand) in candidate_masks.iter().enumerate() {
        let cs = m + s;
        if cand & required == 0 {
            continue;
        }
        for (i, &mem) in member_masks.iter().enumerate() {
            let p_out = CouplingModel::coupling(mem, cand, required); // member → candidate
            if p_out > 0.0 {
                couplings.push((i, cs, p_out));
            }
            let p_in = CouplingModel::coupling(cand, mem, required); // candidate → member
            if p_in > 0.0 {
                couplings.push((cs, i, p_in));
            }
        }
    }

    let members: Vec<usize> = (0..m).collect();
    CoalitionEvaluator::new(&agents, &couplings, &members, 1.0)
}

/// Maximum candidate-mask registry size. A rebuild that would carry more masks
/// than this resets the registry instead (see [`EvaluatorCache`]): the pool and
/// the O(registry × m) coupling emission stay bounded, at the price of a rare
/// full re-registration burst. Sized comfortably above the distinct-mask count
/// of any realistic pool (the K4 battery's whole 30-seed stream sees < 162).
const REGISTRY_CAP: usize = 256;

/// Take the candidate-mask registry and the [`EvalScratch`] pool out of `guard`
/// for a rebuild, enforcing [`REGISTRY_CAP`] on the registry: an over-cap
/// registry is dropped and re-registration starts empty. The registry is
/// retained across `required` changes (couplings are recomputed from raw masks
/// at each build), which is what keeps post-warm-up joins on the O(m²) hit path
/// (see [`EvaluatorCache`]).
///
/// The scratch is retained **unconditionally** — including when the registry
/// resets and when the coalition changes shape. It is stateless by upstream
/// contract (reusable across evaluators and across coalitions of different
/// sizes), so carrying it over is pure allocation reuse and cannot affect a
/// value.
fn take_registry_and_scratch(guard: &mut Option<EvaluatorCache>) -> (Vec<u32>, EvalScratch) {
    let (registry, scratch) = guard.take().map_or_else(
        || (Vec::new(), EvalScratch::new()),
        |c| (c.candidate_masks, c.scratch),
    );
    // `>=` so a full-cap registry resets BEFORE the caller's potential push —
    // the cap is a hard bound, never exceeded even by one.
    if registry.len() >= REGISTRY_CAP {
        (Vec::new(), scratch)
    } else {
        (registry, scratch)
    }
}

/// Ensure `cache` holds an evaluator keyed on `(required, member_masks)`,
/// rebuilding it if the key changed (retaining the candidate-mask registry, up
/// to [`REGISTRY_CAP`]), and return
/// `Mag(member_masks)` = [`CoalitionEvaluator::base_value`].
///
/// `member_masks` must be non-empty. The returned value is bit-identical to a
/// fresh [`magnitude_of_masks`] on `member_masks`.
///
/// # Errors
///
/// Propagates a [`build_evaluator`] failure (same conditions as a fresh
/// [`magnitude_of_masks`] on `member_masks`).
fn cached_base(
    cache: &Mutex<Option<EvaluatorCache>>,
    required: u32,
    member_masks: &[u32],
) -> Result<f64, CatgraphError> {
    let mut guard = cache.lock().expect("invariant: no panic while holding the magnitude cache lock (guarded section only builds/queries the evaluator)");
    if !guard
        .as_ref()
        .is_some_and(|c| c.required == required && c.member_masks == *member_masks)
    {
        let (registry, scratch) = take_registry_and_scratch(&mut guard);
        let evaluator = build_evaluator(required, member_masks, &registry)?;
        *guard = Some(EvaluatorCache {
            required,
            member_masks: member_masks.to_vec(),
            candidate_masks: registry,
            evaluator,
            scratch,
        });
    }
    Ok(guard
        .as_ref()
        .expect("cache populated above")
        .evaluator
        .base_value())
}

/// One candidate query against the cached base coalition: the `(Mag(S),
/// Mag(S ∪ {candidate}))` pair, plus — on the EQ3 arm only — the exact
/// zero-diversity certificate the upstream report carries (L2).
///
/// `base` is bit-identical to fresh `Mag(S)`; `with` is the incremental
/// `Mag(S ∪ {x})` (tolerance-equal to fresh, `Err` exactly when fresh would
/// err). The certificate field exists only under `magnitude-fast`, so the
/// default build has no proof surface at all.
#[derive(Debug)]
struct CandidateEval {
    /// `Mag(S)` — [`CoalitionEvaluator::base_value`], bit-identical to fresh.
    base: f64,
    /// `Mag(S ∪ {candidate})` from the incremental path, or the upstream error.
    with: Result<f64, CatgraphError>,
    /// The exact certificate, when one of upstream's three decidable classes
    /// fired: `Some(_)` **iff** the REAL increment is exactly `0`. `None` means
    /// *not proven*, never "nonzero" — so it is a decision input while
    /// `with - base` on a proven candidate may still be roundoff-nonzero.
    /// Always `None` on the `Err` arm, and always `None` on the toggle-OFF
    /// query (which never asks for a report).
    #[cfg(feature = "magnitude-fast")]
    zero_proof: Option<ZeroDiversityProof>,
}

/// Ensure `guard` holds an evaluator keyed on `(required, member_masks)` whose
/// candidate pool contains `candidate_caps`, and return that candidate's
/// registry slot (its evaluator agent index is `member_masks.len() + slot`).
///
/// Rebuilds the base evaluator when `(required, member_masks)` changed (retaining
/// the candidate registry, up to [`REGISTRY_CAP`], and the [`EvalScratch`] pool),
/// registers `candidate_caps` if unseen (a rebuild that extends the pool), and
/// otherwise leaves the cached evaluator untouched so the caller's query is a
/// pure `O(m²)` hit.
///
/// `member_masks` must be non-empty and must NOT contain `candidate_caps` as a
/// genuinely-merged member for the `with` side to be a real join — callers
/// dispatch the excluded / empty cases before reaching here. (A candidate mask
/// that merely *equals* a member mask is a distinct pool slot and is handled by
/// the evaluator's skeletal-merge slow path.)
///
/// # Errors
///
/// Propagates a [`build_evaluator`] failure (base or pool-extension rebuild).
fn ensure_cached_candidate(
    guard: &mut Option<EvaluatorCache>,
    required: u32,
    member_masks: &[u32],
    candidate_caps: u32,
) -> Result<usize, CatgraphError> {
    // Rebuild the base evaluator on a membership / requirement miss, retaining
    // the accumulated candidate registry (capped — see
    // `take_registry_and_scratch`) and
    // ensuring this candidate is in it.
    if !guard
        .as_ref()
        .is_some_and(|c| c.required == required && c.member_masks == *member_masks)
    {
        let (mut registry, scratch) = take_registry_and_scratch(guard);
        if !registry.contains(&candidate_caps) {
            registry.push(candidate_caps);
        }
        let evaluator = build_evaluator(required, member_masks, &registry)?;
        *guard = Some(EvaluatorCache {
            required,
            member_masks: member_masks.to_vec(),
            candidate_masks: registry,
            evaluator,
            scratch,
        });
    }

    let existing_pos = guard
        .as_ref()
        .expect("cache populated above")
        .candidate_masks
        .iter()
        .position(|&c| c == candidate_caps);

    // Register the candidate if unseen (extends the pool ⇒ one rebuild); a
    // repeated candidate against unchanged `S` is a pure O(m²) `value_with` hit.
    // The entry is TAKEN out and re-inserted only after a successful rebuild:
    // if `build_evaluator` fails mid-registration, the cache is left empty (a
    // clean full rebuild next call) instead of holding a `candidate_masks` list
    // that no longer matches the evaluator's actual pool — a desync would make
    // the next hit index the wrong slot (wrong value, or an out-of-bounds panic
    // that poisons the shared lock).
    if let Some(p) = existing_pos {
        Ok(p)
    } else {
        let mut entry = guard.take().expect("cache populated above");
        if entry.candidate_masks.len() >= REGISTRY_CAP {
            // Cap enforcement on the extension path too (a long-lived key could
            // otherwise grow past the cap one push at a time).
            entry.candidate_masks.clear();
        }
        entry.candidate_masks.push(candidate_caps);
        entry.evaluator = build_evaluator(required, &entry.member_masks, &entry.candidate_masks)?;
        let p = entry.candidate_masks.len() - 1;
        *guard = Some(entry);
        Ok(p)
    }
}

/// The `(Mag(S), Mag(S ∪ {candidate}))` pair for `S = member_masks` — **the
/// default (frozen) query**.
///
/// The `with` component comes from [`CoalitionEvaluator::value_with_scratch`]:
/// same arithmetic, same branch selection and **bit-identical** result to the
/// pre-EQ3 [`CoalitionEvaluator::value_with`] (upstream #33 contract), the only
/// change being that the seven per-query border/Schur `Vec`s are drawn from the
/// cache entry's reusable [`EvalScratch`] (EQ3 lever L1). No report is
/// requested, so upstream's proof scan is not even compiled into this path.
/// `base` is bit-identical to fresh; `with` is within
/// [`catgraph_magnitude::INCREMENTAL_REL_TOL`] of fresh and its `Ok`/`Err`
/// tracks fresh.
///
/// # Errors
///
/// Propagates a [`ensure_cached_candidate`] failure (base or pool-extension
/// rebuild). A [`CoalitionEvaluator::value_with_scratch`] failure is returned in
/// the inner `Result` ([`CandidateEval::with`]), not this outer one, so callers
/// can decline on it exactly as the fresh path declines on a failed
/// `Mag(S ∪ {x})`.
fn cached_base_and_with(
    cache: &Mutex<Option<EvaluatorCache>>,
    required: u32,
    member_masks: &[u32],
    candidate_caps: u32,
) -> Result<CandidateEval, CatgraphError> {
    let mut guard = cache.lock().expect("invariant: no panic while holding the magnitude cache lock (guarded section only builds/queries the evaluator)");
    let pos = ensure_cached_candidate(&mut guard, required, member_masks, candidate_caps)?;

    // `as_mut` (not `as_ref`): the scratch pool is borrowed mutably while the
    // evaluator is borrowed shared — disjoint fields of the same entry, so the
    // two borrows split. Nothing else in the entry is touched, so the
    // take/reinsert desync discipline in `ensure_cached_candidate` is
    // unaffected.
    let entry = guard.as_mut().expect("cache populated above");
    let m = entry.member_masks.len();
    let base = entry.evaluator.base_value();
    let with = entry
        .evaluator
        .value_with_scratch(m + pos, &mut entry.scratch);
    Ok(CandidateEval {
        base,
        with,
        #[cfg(feature = "magnitude-fast")]
        zero_proof: None,
    })
}

/// [`cached_base_and_with`] through the **reporting** query — the EQ3 arm's
/// candidate evaluation (levers L1 × L2, feature `magnitude-fast` + toggle ON).
///
/// [`CoalitionEvaluator::value_with_report_scratch`] returns a value
/// bit-identical to [`CoalitionEvaluator::value_with_scratch`] on the same
/// candidate (upstream contract: same arithmetic, same accumulation order, same
/// branch selection) and additionally carries the [`ZeroDiversityProof`] when
/// one of the three exactly-decidable classes fires. It is a separate function
/// — not a flag on the default one — so the frozen path is textually free of
/// any report/proof call.
///
/// # Errors
///
/// Identical to [`cached_base_and_with`].
#[cfg(feature = "magnitude-fast")]
fn cached_base_and_with_report(
    cache: &Mutex<Option<EvaluatorCache>>,
    required: u32,
    member_masks: &[u32],
    candidate_caps: u32,
) -> Result<CandidateEval, CatgraphError> {
    let mut guard = cache.lock().expect("invariant: no panic while holding the magnitude cache lock (guarded section only builds/queries the evaluator)");
    let pos = ensure_cached_candidate(&mut guard, required, member_masks, candidate_caps)?;

    let entry = guard.as_mut().expect("cache populated above");
    let m = entry.member_masks.len();
    let base = entry.evaluator.base_value();
    let report = entry
        .evaluator
        .value_with_report_scratch(m + pos, &mut entry.scratch);
    let (with, zero_proof) = match report {
        Ok(r) => (Ok(r.value()), r.zero_proof()),
        Err(e) => (Err(e), None),
    };
    Ok(CandidateEval {
        base,
        with,
        zero_proof,
    })
}

/// The candidate query the policy dispatch uses, selected by `route`:
/// [`cached_base_and_with`] (the frozen default — no report, no proof scan) or,
/// with the EQ3 toggle ON, [`cached_base_and_with_report`].
///
/// # Errors
///
/// Whatever the selected query propagates.
fn cached_candidate_eval(
    cache: &Mutex<Option<EvaluatorCache>>,
    required: u32,
    member_masks: &[u32],
    candidate_caps: u32,
    route: FreshRoute,
) -> Result<CandidateEval, CatgraphError> {
    // [`FreshRoute::proofs`] is a compile-time `false` without `magnitude-fast`
    // (the flag has no representation there), so the frozen build reduces to the
    // plain query below and never mentions the reporting one.
    if route.proofs() {
        #[cfg(feature = "magnitude-fast")]
        return cached_base_and_with_report(cache, required, member_masks, candidate_caps);
    }
    cached_base_and_with(cache, required, member_masks, candidate_caps)
}

/// The capability→coupling map. Public so the mapping can be tested directly
/// (mirror of `CapabilityModel` in the AIF arm).
#[derive(Debug, Clone, Copy, Default)]
pub struct CouplingModel;

impl CouplingModel {
    /// Directed substitutability coupling `A(from → to) ∈ [0, 1]`.
    ///
    /// With `rel_x = caps_x & required`, returns
    /// `popcount(rel_from & rel_to) / popcount(rel_from)` — the fraction of
    /// `from`'s task-relevant capabilities that `to` also has. Returns `0.0`
    /// when `rel_from == 0`: an agent with nothing task-relevant has no
    /// substitutability relation to express, and the mask pipeline excludes such
    /// agents from the member set anyway (see rationale point 4 in the module
    /// docs — a vacuous `1.0` here would collapse coalition diversity). Because
    /// `rel_x` masks with `required`, capability bits outside the task
    /// requirement are ignored, and `required == 0` yields `0.0` for every pair
    /// (rationale point 5).
    #[must_use]
    pub fn coupling(from_caps: u32, to_caps: u32, required: u32) -> f64 {
        let rel_from = from_caps & required;
        let rel_to = to_caps & required;
        if rel_from == 0 {
            return 0.0;
        }
        f64::from((rel_from & rel_to).count_ones()) / f64::from(rel_from.count_ones())
    }
}

/// Deduplicate `agents` by [`AgentCapabilities::agent_id`] (first occurrence
/// wins), drop agents with no task-relevant capabilities
/// (`capabilities & required == 0`), and return the survivors' capability masks.
///
/// Upstream [`catgraph_magnitude::coalition_value`] errors on duplicate members,
/// and a double-listed agent must not double-count toward diversity, so callers
/// dedup before building couplings. Irrelevant agents are excluded — not coupled
/// vacuously — because a one-way `1.0` coupling to every member drives their
/// Möbius weight negative and collapses the coalition's diversity (module docs,
/// rationale point 4).
///
/// `pub(crate)` because [`crate::topology::TemporalAnalytics::magnitude_history`]
/// reuses the exact bystander-exclusion / dedup contract when it snapshots a
/// coalition's members at a trajectory sample point.
pub(crate) fn relevant_masks(agents: &[&dyn AgentCapabilities], required: u32) -> Vec<u32> {
    let mut seen = std::collections::HashSet::new();
    let mut masks = Vec::with_capacity(agents.len());
    for a in agents {
        let caps = a.capabilities();
        if caps & required != 0 && seen.insert(a.agent_id()) {
            masks.push(caps);
        }
    }
    masks
}

/// Coalition magnitude of the capability `masks` under the substitutability
/// coupling, at the pinned scale `t = 1`.
///
/// Builds `agents = 0..masks.len()` (local `usize` indices), emits a directed
/// coupling for every ordered pair `i != j` whose [`CouplingModel::coupling`] is
/// strictly positive (a zero coupling equals an absent one upstream, and a
/// self-coupling `i == j` is rejected upstream), and calls
/// [`catgraph_magnitude::coalition_value`] with all indices as members.
///
/// `masks` must be non-empty: upstream errors on an empty member set, so callers
/// guard the empty case (via [`magnitude_or_zero`]) rather than reaching here.
///
/// # Errors
///
/// Propagates any [`CatgraphError`] from the upstream magnitude computation
/// (e.g. a singular `t`-scaled zeta from rare parametric coincidences).
fn magnitude_of_masks(masks: &[u32], required: u32) -> Result<f64, CatgraphError> {
    let agents: Vec<usize> = (0..masks.len()).collect();

    let mut couplings: Vec<(usize, usize, f64)> = Vec::new();
    for (i, &from) in masks.iter().enumerate() {
        for (j, &to) in masks.iter().enumerate() {
            if i == j {
                continue;
            }
            let p = CouplingModel::coupling(from, to, required);
            if p > 0.0 {
                couplings.push((i, j, p));
            }
        }
    }

    catgraph_magnitude::coalition_value(&agents, &couplings, &agents)
}

/// Magnitude of `masks`, or `Ok(0.0)` when empty.
///
/// `Mag(∅) = 0` (Leinster's convention, and consistent with every other koalisi
/// value calculator returning `0.0` for an empty coalition). This is the
/// call-site guard that keeps an empty member set from reaching upstream, which
/// would error. "Empty" includes a coalition whose members were *all* excluded
/// as task-irrelevant by [`relevant_masks`] (e.g. when `required == 0`).
///
/// `pub(crate)` because [`crate::topology::TemporalAnalytics::magnitude_history`]
/// evaluates each trajectory sample point through this same empty-guarded
/// fresh-magnitude path.
pub(crate) fn magnitude_or_zero(masks: &[u32], required: u32) -> Result<f64, CatgraphError> {
    if masks.is_empty() {
        return Ok(0.0);
    }
    magnitude_of_masks(masks, required)
}

/// The typed-roles configuration (koalisi #72, EQ4): who carries which role, and
/// how strongly two roles substitute for one another.
///
/// Held behind an [`Arc`] by [`MagnitudePolicy`] / [`MagnitudeValueCalculator`]
/// so a clone shares it and the async offload can move an owned handle into the
/// rayon closure. See the
/// [module docs](self#the-typed-roles-arm-koalisi-72-eq4) for the registered
/// contract.
#[derive(Debug, Clone)]
struct TypedConfig {
    /// `agent_id → RoleId`. Consulted **only** for agents that survive relevance
    /// filtering, so a task-irrelevant agent needs no entry (contract point 3).
    roles: HashMap<usize, RoleId>,
    /// The role-interaction table `ρ`, validated at construction by
    /// [`RoleModulation::new`].
    rho: RoleModulation,
}

/// One side of a typed decision: the relevance-filtered member capability masks
/// and the roles of exactly those members, positionally aligned
/// (`roles[i]` is the role of the agent whose mask is `masks[i]`).
///
/// Produced by [`typed_relevant_masks`], which is the sole constructor — the
/// positional alignment is the invariant every typed evaluation rests on, since
/// [`catgraph_magnitude::modulate`] indexes its `roles` slice by agent index.
#[derive(Debug, Clone, Default)]
struct TypedSide {
    masks: Vec<u32>,
    roles: Vec<RoleId>,
}

/// [`relevant_masks`] with each survivor's [`RoleId`] carried alongside its mask.
///
/// Applies exactly the same dedup-by-`agent_id` / drop-`rel == 0` filter, in the
/// same order, so the `masks` component is identical to what [`relevant_masks`]
/// would produce on the same input — the role lookup happens **after** the
/// filter, which is what keeps `ρ` from resurrecting a task-irrelevant agent
/// (module docs, contract point 3).
///
/// Returns `None` — after a `tracing::warn!` naming the agent — when a
/// *surviving* agent carries no role assignment. Callers turn that into a
/// decline (or [`f64::NEG_INFINITY`] on the value surface); it is never a panic.
fn typed_relevant_masks(
    agents: &[&dyn AgentCapabilities],
    required: u32,
    roles: &HashMap<usize, RoleId>,
) -> Option<TypedSide> {
    let mut seen = std::collections::HashSet::new();
    let mut side = TypedSide {
        masks: Vec::with_capacity(agents.len()),
        roles: Vec::with_capacity(agents.len()),
    };
    for a in agents {
        let caps = a.capabilities();
        let id = a.agent_id();
        if caps & required == 0 || !seen.insert(id) {
            continue;
        }
        let Some(&role) = roles.get(&id) else {
            tracing::warn!(
                agent_id = id,
                "typed magnitude: participating agent has no role assignment; declining"
            );
            return None;
        };
        side.masks.push(caps);
        side.roles.push(role);
    }
    Some(side)
}

/// Coalition magnitude of a typed side under the **ρ-modulated**
/// substitutability coupling, at the pinned scale `t = 1`.
///
/// Builds the coupling list exactly as [`magnitude_of_masks`] does (same
/// [`CouplingModel::coupling`] map, same `i`-outer / `j`-inner order, same
/// `p > 0.0` filter, no self-couplings), applies
/// [`catgraph_magnitude::modulate`] to it, and evaluates the modulated table
/// through the same [`catgraph_magnitude::coalition_value`] entry point. At
/// `ρ ≡ 1` the modulation is the exact identity on every entry (`1.0 * p == p`
/// in IEEE), so this reduces to [`magnitude_of_masks`] bit-for-bit.
///
/// A `ρ` entry of `0` yields an explicit `0.0` coupling, which upstream treats
/// exactly as an absent one.
///
/// `side.masks` must be non-empty (callers guard the empty case via
/// [`typed_magnitude_or_zero`]).
///
/// # Errors
///
/// Propagates a [`catgraph_magnitude::modulate`] failure (a role outside `ρ`'s
/// range) and any [`CatgraphError`] from the upstream magnitude computation.
fn typed_magnitude_of_masks(
    side: &TypedSide,
    required: u32,
    rho: &RoleModulation,
) -> Result<f64, CatgraphError> {
    let agents: Vec<usize> = (0..side.masks.len()).collect();

    let mut couplings: Vec<(usize, usize, f64)> = Vec::new();
    for (i, &from) in side.masks.iter().enumerate() {
        for (j, &to) in side.masks.iter().enumerate() {
            if i == j {
                continue;
            }
            let p = CouplingModel::coupling(from, to, required);
            if p > 0.0 {
                couplings.push((i, j, p));
            }
        }
    }

    let modulated = catgraph_magnitude::modulate(&couplings, &side.roles, rho)?;
    catgraph_magnitude::coalition_value(&agents, modulated.couplings(), &agents)
}

/// [`typed_magnitude_of_masks`], or `Ok(0.0)` when the side is empty
/// (`Mag(∅) = 0` — the same guard [`magnitude_or_zero`] applies to the untyped
/// path, including the all-irrelevant and `required == 0` cases).
///
/// # Errors
///
/// Whatever [`typed_magnitude_of_masks`] propagates.
fn typed_magnitude_or_zero(
    side: &TypedSide,
    required: u32,
    rho: &RoleModulation,
) -> Result<f64, CatgraphError> {
    if side.masks.is_empty() {
        return Ok(0.0);
    }
    typed_magnitude_of_masks(side, required, rho)
}

/// The decline a typed decision returns when the configuration cannot be applied
/// to it (a participating agent has no role) — the same shape as the
/// upstream-error decline in [`MagnitudePolicy::join_decision_from_values`].
const TYPED_DECLINE: Decision = Decision {
    act: false,
    score: 0.0,
};

/// [`magnitude_of_masks`] over the upstream `f64` factorization (EQ3 lever L3,
/// feature `magnitude-fast`, upstream #165).
///
/// Reproduces [`magnitude_of_masks`]'s semantics step for step — same
/// [`CouplingModel::coupling`] map, same `i`-outer / `j`-inner emission order,
/// same `p > 0.0` filter, no self-couplings — and hands the couplings to the
/// same [`Coalition::from_enriched`] pipeline
/// [`catgraph_magnitude::coalition_value`] uses (restrict-then-close +
/// skeletalize). Only the final Möbius inversion differs: the coalition's
/// **skeletal** metric space is re-derived from the public surface
/// (closed cospan weights indexed by class representatives) and inverted by
/// [`catgraph_magnitude::magnitude_f64::magnitude_f64`] at the pinned `t = 1`.
///
/// Skeletalization is load-bearing, not incidental: the coalition's *full*
/// cospan-derived space keeps perfectly-coupled members apart, whose ζ has
/// identical rows and is singular at every `t` (a clone candidate would error
/// instead of scoring the base magnitude). Taking one representative per
/// [`Coalition::member_classes`] class is exactly the quotient the generic path
/// applies, so both routes invert the same **skeletal** matrix up to the
/// documented ζ construction — upstream's ULP-identity contract covers the ζ
/// *entries*, not the inversion that follows.
///
/// Agreement with [`magnitude_of_masks`] is by *tolerance*, not bit-identity —
/// the factorization route differs (Cholesky / LBLT when ζ is exactly
/// symmetric, Gauss–Jordan otherwise) — which is why the toggle is measured
/// (koalisi #69) and defaults off. See the module docs for how rarely the
/// symmetric route is actually taken on this traffic.
///
/// `masks` must be non-empty (same contract as [`magnitude_of_masks`]).
///
/// # Errors
///
/// Propagates any [`CatgraphError`] from category construction, coalition
/// formation, or a singular `t`-scaled zeta.
///
/// **Failure-surface scope.** `Err`-parity with [`magnitude_of_masks`] is
/// upstream-guaranteed only on the Gauss–Jordan route, which *is* the generic
/// computation. On the Cholesky/LBLT route the two can in principle disagree:
/// a near-singular ζ that Cholesky accepts would return a finite value where
/// the generic path errors (or vice versa). That would surface as an act
/// divergence with no certificate behind it — exactly what the registered
/// H-par′ (i) gate is designed to catch — rather than as a silent wrong answer.
#[cfg(feature = "magnitude-fast")]
fn magnitude_of_masks_f64(masks: &[u32], required: u32) -> Result<f64, CatgraphError> {
    let space = skeletal_space_of_masks(masks, required)?;
    catgraph_magnitude::magnitude_f64::magnitude_f64(&space, 1.0)
}

/// The **skeletal** Lawvere metric space of `masks` under the substitutability
/// coupling — the matrix both `f64` entry points invert (the fresh route in
/// [`magnitude_of_masks_f64`], the instrumentation route in
/// [`probe_fresh_factorization`]).
///
/// See [`magnitude_of_masks_f64`] for why the skeleton, not the full member
/// cospan, is the right space.
///
/// `masks` must be non-empty.
///
/// # Errors
///
/// Propagates any [`CatgraphError`] from category construction or coalition
/// formation.
#[cfg(feature = "magnitude-fast")]
fn skeletal_space_of_masks(
    masks: &[u32],
    required: u32,
) -> Result<LawvereMetricSpace<usize>, CatgraphError> {
    let agents: Vec<usize> = (0..masks.len()).collect();

    let mut cat = HomMap::<usize, UnitInterval>::new(agents.clone());
    for (i, &from) in masks.iter().enumerate() {
        for (j, &to) in masks.iter().enumerate() {
            if i == j {
                continue;
            }
            let p = CouplingModel::coupling(from, to, required);
            if p > 0.0 {
                cat.set_hom(i, j, UnitInterval::new(p)?);
            }
        }
    }

    let coalition = Coalition::from_enriched(&cat, &agents)?;

    // Skeleton: one representative (the first member of the class) per
    // equivalence class, mirroring upstream's `build_skeletal_space`. Indexing
    // `reps` by class id rather than by first-seen order makes this independent
    // of upstream's class numbering (any permutation leaves the magnitude
    // unchanged).
    let classes = coalition.member_classes();
    let k = classes.iter().copied().max().map_or(0, |c| c + 1);
    let mut reps = vec![usize::MAX; k];
    for (member, &c) in classes.iter().enumerate() {
        if reps[c] == usize::MAX {
            reps[c] = member;
        }
    }

    let cospan = coalition.as_weighted_cospan();
    Ok(LawvereMetricSpace::from_distance_fn(k, |a, b| {
        let p = cospan.weight(reps[a], reps[b]).value();
        if p > 0.0 { -p.ln() } else { f64::INFINITY }
    }))
}

/// **EQ3 instrumentation** (koalisi #69, feature `magnitude-fast`): what
/// [`MagnitudePolicy::probe_join`] observed about one join decision.
///
/// A read-only view of the evaluator layer, produced off a private cache — see
/// that method for the no-behaviour-change contract.
#[cfg(feature = "magnitude-fast")]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JoinProbe {
    /// `Mag(S)` — bit-identical to a fresh evaluation of the coalition.
    pub base: f64,
    /// `Mag(S ∪ {x})` from the **incremental** path, before any knife-edge
    /// substitution.
    pub with: f64,
    /// The exact zero-diversity certificate, when one of the three decidable
    /// classes fired: `Some(_)` ⇒ the real increment is exactly `0`.
    pub zero_proof: Option<ZeroDiversityProof>,
    /// Whether `with − base` lands inside [`KNIFE_EDGE_REL_BAND`] of the
    /// policy's `join_margin` — i.e. whether the frozen arm pays a fresh
    /// recompute here.
    pub knife_edge: bool,
}

/// **EQ3 instrumentation** (koalisi #69, feature `magnitude-fast`): the
/// factorization route the `f64` fresh path takes on `agents`, together with the
/// magnitude that route produces, at the pinned `t = 1`.
///
/// One factorization total — the handle that reports
/// [`catgraph_magnitude::magnitude_f64::FactorizationPath`] is the same handle
/// that answers the magnitude, so nothing extra is computed. (`t = 1` is what
/// makes this exact: upstream's `t`-scaling multiplies every distance by `t`,
/// and `d * 1.0 == d` bitwise for finite and infinite `d` alike, so factoring
/// the unscaled space is factoring the scaled one. The returned magnitude is
/// asserted equal to [`magnitude_of_masks_f64`]'s in the module tests.)
///
/// The path is reported **independently of whether the magnitude succeeds** —
/// the route is decided at construction, before any solve — so a singular ζ
/// still tells you which route it was singular on. Counting only the `Ok`
/// cases would silently undercount the Gauss–Jordan share, since that is the
/// route an exact singularity surfaces on.
///
/// Returns `None` only when nothing task-relevant survives [`relevant_masks`]
/// (an empty coalition has no ζ to factor) or when the coalition itself cannot
/// be built.
#[cfg(feature = "magnitude-fast")]
#[must_use]
pub fn probe_fresh_factorization(
    agents: &[&dyn AgentCapabilities],
    required: u32,
) -> Option<(
    catgraph_magnitude::magnitude_f64::FactorizationPath,
    Result<f64, CatgraphError>,
)> {
    let masks = relevant_masks(agents, required);
    if masks.is_empty() {
        return None;
    }
    let space = skeletal_space_of_masks(&masks, required).ok()?;
    let factorization = catgraph_magnitude::magnitude_f64::ZetaFactorization::new(&space);
    Some((factorization.path(), factorization.magnitude()))
}

/// The EQ3 arm selector: one flag driving **both** opt-in levers — L2 (the
/// exact zero-diversity proof branch on the cached candidate query) and L3 (the
/// `f64` factorization for fresh full-coalition evaluations).
///
/// Amendment 1 (A1.1) parks L2 here alongside L3 because the proof branch is
/// decision-changing on the profile-duplicate classes: OFF is the frozen
/// pre-EQ3 decision surface (plus L1, which is invisible), ON is the `mag-eq3`
/// arm.
///
/// Zero-sized when `magnitude-fast` is not compiled in — the flag then has no
/// representation, no branch, and no cost, so the feature-off build is the
/// pre-EQ3 code path exactly.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct FreshRoute {
    /// `true` ⇒ the EQ3 arm: proof branch on, fresh evaluations through
    /// [`magnitude_of_masks_f64`]. `false` (the default) ⇒ no proof scan and
    /// the generic [`magnitude_of_masks`].
    #[cfg(feature = "magnitude-fast")]
    fast: bool,
}

impl FreshRoute {
    /// Whether the EQ3 L2 proof branch is active. A compile-time `false`
    /// without `magnitude-fast`.
    fn proofs(self) -> bool {
        #[cfg(feature = "magnitude-fast")]
        return self.fast;
        #[cfg(not(feature = "magnitude-fast"))]
        return false;
    }

    /// Fresh `Mag(masks)` through the selected route. `masks` must be non-empty.
    fn magnitude(self, masks: &[u32], required: u32) -> Result<f64, CatgraphError> {
        #[cfg(feature = "magnitude-fast")]
        if self.fast {
            return magnitude_of_masks_f64(masks, required);
        }
        magnitude_of_masks(masks, required)
    }

    /// Fresh `Mag(masks)` through the selected route, or `Ok(0.0)` when empty
    /// (the [`magnitude_or_zero`] guard — `Mag(∅) = 0`).
    fn magnitude_or_zero(self, masks: &[u32], required: u32) -> Result<f64, CatgraphError> {
        if masks.is_empty() {
            return Ok(0.0);
        }
        self.magnitude(masks, required)
    }
}

/// A [`ValueCalculator`] that scores a coalition by its magnitude (effective-
/// member diversity) under the capability-substitutability coupling.
///
/// Because [`ValueCalculator::calculate_value`] has no context argument, the
/// task's `required_capabilities` is stored as a field. Agents are deduplicated
/// by `agent_id` (first occurrence wins) and task-irrelevant agents
/// (`capabilities & required == 0`) are excluded before scoring, so a
/// double-listed agent does not double-count and a bystander with nothing
/// task-relevant does not distort diversity. A coalition that is empty — or
/// whose members are all task-irrelevant — scores `0.0`; on an upstream error
/// it logs and returns [`f64::NEG_INFINITY`] (exact mirror of the AIF arm's
/// `EfeValueCalculator`).
///
/// # Caching
///
/// Holds a membership-keyed [`CoalitionEvaluator`] cache behind interior
/// mutability (the [`ValueCalculator`] seam takes `&self`). It is **exact-hit
/// only**: a hit on `(required, masks)` returns the cached
/// [`CoalitionEvaluator::base_value`]; any miss rebuilds the evaluator once and
/// returns its `base_value`. Because `base_value` is bit-identical to a fresh
/// magnitude evaluation, the returned value is unchanged from the pre-cache
/// implementation in every case (only `base_value`, never `value_with`, is used
/// here). [`Clone`] shares the cache (via [`Arc`]); [`Default`] / [`new`] start
/// empty. The calculator no longer derives [`Copy`].
///
/// # Typed roles
///
/// [`with_role_modulation`](Self::with_role_modulation) switches the calculator
/// to the EQ4 typed arm: couplings are `ρ`-modulated and every value is computed
/// **fresh** (the cache above is bypassed entirely — the module's *typed-roles
/// arm* docs explain why the `(required, member_masks)` key cannot serve a typed
/// evaluation). Without a typed configuration the calculator is exactly the
/// untyped one described above.
///
/// [`new`]: MagnitudeValueCalculator::new
#[derive(Debug, Clone, Default)]
pub struct MagnitudeValueCalculator {
    pub required_capabilities: u32,
    cache: Arc<Mutex<Option<EvaluatorCache>>>,
    /// The EQ4 typed-roles configuration, or `None` for the untyped default.
    typed: Option<Arc<TypedConfig>>,
}

impl MagnitudeValueCalculator {
    /// A calculator for the given task requirement, with an empty evaluator
    /// cache and no typed configuration (the untyped arm).
    #[must_use]
    pub fn new(required_capabilities: u32) -> Self {
        Self {
            required_capabilities,
            cache: Arc::default(),
            typed: None,
        }
    }

    /// Switch the calculator to the **EQ4 typed arm** (koalisi #72): score
    /// coalitions on `ρ`-modulated substitutability couplings,
    /// `A′(i → j) = ρ(role_i, role_j) · A(i → j)`.
    ///
    /// `roles` maps `AgentCapabilities::agent_id` to a
    /// [`RoleId`](catgraph_magnitude::RoleId); only agents that survive
    /// relevance filtering (`capabilities & required != 0`) are looked up, so a
    /// task-irrelevant agent needs no entry. A *participating* agent with no
    /// entry — or with a role outside `ρ`'s range — makes the value
    /// [`f64::NEG_INFINITY`] with a warning, this surface's existing
    /// cannot-evaluate convention.
    ///
    /// The typed arm evaluates **fresh**: the membership-keyed evaluator cache
    /// is bypassed, since its `(required, member_masks)` key cannot distinguish
    /// two agents that share a mask and differ in role. The module's
    /// *typed-roles arm* docs carry the full contract.
    #[must_use]
    pub fn with_role_modulation(
        mut self,
        roles: HashMap<usize, RoleId>,
        rho: RoleModulation,
    ) -> Self {
        self.typed = Some(Arc::new(TypedConfig { roles, rho }));
        self
    }
}

impl ValueCalculator for MagnitudeValueCalculator {
    fn calculate_value(&self, agents: &[&dyn AgentCapabilities]) -> f64 {
        if let Some(cfg) = self.typed.as_deref() {
            let Some(side) = typed_relevant_masks(agents, self.required_capabilities, &cfg.roles)
            else {
                return f64::NEG_INFINITY;
            };
            return match typed_magnitude_or_zero(&side, self.required_capabilities, &cfg.rho) {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!(error = %e, "typed magnitude value calculation failed");
                    f64::NEG_INFINITY
                }
            };
        }
        let masks = relevant_masks(agents, self.required_capabilities);
        if masks.is_empty() {
            return 0.0;
        }
        match cached_base(&self.cache, self.required_capabilities, &masks) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!(error = %e, "magnitude value calculation failed");
                f64::NEG_INFINITY
            }
        }
    }
}

/// How [`MagnitudePolicy::should_leave`] evaluates the `Mag(S ∖ {x})` magnitudes.
///
/// Removal is an upstream incremental non-goal (a max-product closure cannot be
/// downdated), so the default is the fresh two-evaluation path. The
/// evaluator-backed variant reuses the membership-keyed cache but its
/// with-leaver magnitude is only tolerance-equal to fresh, so it is opt-in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum LeaveVariant {
    /// Variant A: two fresh [`magnitude_or_zero`] evaluations. Bit-identical to
    /// the pre-K6 behaviour; the default.
    #[default]
    Fresh,
    /// Variant B: a reduced-set [`CoalitionEvaluator`] over `S ∖ {x}` with the
    /// leaver as the single pool candidate — `Mag(S ∖ {x})` from `base_value`
    /// (bit-identical) and `Mag(S)` from `value_with` (tolerance-equal).
    Evaluator,
}

/// Categorical-magnitude join/leave policy driven by [`DecisionContext`].
///
/// The agent joins when adding it *raises* coalition magnitude (diversity) by
/// more than `join_margin`, and leaves when removing it does not *lower*
/// magnitude. This mirrors the AIF arm's join/leave rule with `−G` replaced by
/// magnitude. `join_margin` defaults to `0.0`.
///
/// # Caching
///
/// Holds a membership-keyed [`CoalitionEvaluator`] cache behind interior
/// mutability (the [`CoalitionDecisionPolicy`] seam takes `&self`), so a
/// candidate sweep against a fixed coalition pays the `O(m³)` magnitude once and
/// answers each subsequent join in `O(m²)` (see the module-level
/// "Incremental evaluation" docs).
/// [`Clone`] shares the cache (via [`Arc`]); [`Default`] / [`new`] start empty.
/// The policy no longer derives [`Copy`].
///
/// # Typed roles
///
/// [`with_role_modulation`](Self::with_role_modulation) switches the policy to
/// the EQ4 typed arm (koalisi #72). Without it the policy is exactly the untyped
/// one described above — the identity default is structural, not measured.
///
/// [`new`]: MagnitudePolicy::new
#[derive(Debug, Clone, Default)]
pub struct MagnitudePolicy {
    pub join_margin: f64,
    leave_variant: LeaveVariant,
    /// Which route the policy's *fresh* full-coalition evaluations take (EQ3
    /// L3). Zero-sized — and the toggle absent — without `magnitude-fast`.
    fresh_route: FreshRoute,
    cache: Arc<Mutex<Option<EvaluatorCache>>>,
    /// The EQ4 typed-roles configuration, or `None` for the untyped default.
    /// `None` is what routes every decision structurally to the untyped path.
    typed: Option<Arc<TypedConfig>>,
}

impl MagnitudePolicy {
    /// A policy with the given `join_margin`, the default (fresh) leave variant,
    /// an empty evaluator cache, and no typed configuration.
    #[must_use]
    pub fn new(join_margin: f64) -> Self {
        Self {
            join_margin,
            leave_variant: LeaveVariant::Fresh,
            fresh_route: FreshRoute::default(),
            cache: Arc::default(),
            typed: None,
        }
    }

    /// Switch the policy to the **EQ4 typed arm** (koalisi #72): modulate every
    /// substitutability coupling by the role-interaction table,
    /// `A′(i → j) = ρ(role_i, role_j) · A(i → j)`
    /// ([`catgraph_magnitude::modulate`]), then decide on **fresh** magnitudes
    /// of the modulated coalition — join iff
    /// `Mag′(S ∪ {x}) − Mag′(S) > join_margin`, leave iff
    /// `Mag′(S) − Mag′(S ∖ {x}) <= 0`, the same rules as the untyped arm over a
    /// different coupling table.
    ///
    /// `roles` maps [`AgentCapabilities::agent_id`] to a
    /// [`RoleId`](catgraph_magnitude::RoleId). Only agents that survive
    /// relevance filtering (`capabilities & required != 0`) are looked up, so a
    /// task-irrelevant agent needs no entry — `ρ` cannot resurrect one. A
    /// *participating* agent with no entry, or with a role outside `ρ`'s range,
    /// declines the decision (`act = false`, `score = 0.0`) with a warning;
    /// nothing on this path panics.
    ///
    /// Not composable with the cache-based opt-ins: the typed path evaluates
    /// fresh on both sides of every decision, so `with_eq3_levers` (feature
    /// `magnitude-fast`) and
    /// [`with_evaluator_leave`](Self::with_evaluator_leave) have no effect while
    /// a typed configuration is set. The module's *typed-roles arm* docs carry
    /// the registered contract and the reasons behind it.
    #[must_use]
    pub fn with_role_modulation(
        mut self,
        roles: HashMap<usize, RoleId>,
        rho: RoleModulation,
    ) -> Self {
        self.typed = Some(Arc::new(TypedConfig { roles, rho }));
        self
    }

    /// Switch the policy to the **EQ3 arm** (koalisi #69, feature
    /// `magnitude-fast`): both opt-in levers at once — see the
    /// [module docs](self#the-opt-in-eq3-arm-koalisi-69-feature-magnitude-fast).
    ///
    /// - **L2**, the exact zero-diversity proof branch on the cached candidate
    ///   query. **Decision-changing by design** on the profile-duplicate
    ///   classes, where the frozen path joined on positive roundoff and this arm
    ///   declines on the certified exact zero.
    /// - **L3**, the `f64` factorization for the fresh evaluation sites (the
    ///   unprovable-knife-edge recompute, the leave-side fresh pair, the empty /
    ///   sole-member guards). Tolerance-equal to the generic route, not
    ///   bit-identical.
    ///
    /// `false` is the identity and the default: the policy then behaves exactly
    /// as a `magnitude`-only build — no report is requested, no proof scan runs,
    /// and the knife-edge band logic is the frozen one (asserted bit-exactly by
    /// the toggle-identity and default-path unit gates).
    #[cfg(feature = "magnitude-fast")]
    #[must_use]
    pub fn with_eq3_levers(mut self, enabled: bool) -> Self {
        self.fresh_route = FreshRoute { fast: enabled };
        self
    }

    /// **EQ3 instrumentation** (koalisi #69, feature `magnitude-fast`): the
    /// evaluator-level facts behind one join decision — the cached
    /// `(Mag(S), Mag(S ∪ {x}))` pair, the exact zero-diversity certificate, and
    /// whether the incremental margin lands in the knife-edge band.
    ///
    /// **No behaviour change, no state change.** The probe answers from a
    /// PRIVATE, throw-away evaluator cache: `self`'s cache is neither read nor
    /// written, so probing cannot perturb a measured run's rebuild pattern or
    /// its latency. It is not on any decision path.
    ///
    /// That purity is **load-bearing, not merely tidy**: besides the non-gating
    /// instrumentation rows, this probe is consumed by the registered
    /// **H-par′ (i)** paired walk, which is a GATING leg. The walk reads the
    /// certificate at a decision the two arms disagree on, so a probe that
    /// mutated cache state could change the very decisions it is adjudicating.
    /// The walk is untimed, so the private cache's rebuild cost is irrelevant
    /// there; keep it private regardless.
    ///
    /// Returns `None` when the decision does not reach the incremental branch
    /// (empty coalition, or a candidate excluded as task-irrelevant / duplicate
    /// `agent_id`), and on an upstream evaluator-construction error.
    ///
    /// The reported [`JoinProbe::with`] is the **incremental** value, before any
    /// knife-edge substitution — i.e. what `Mag(S ∪ {x})` costs on the fast
    /// path, which is the quantity the cg#153 empty-band hypothesis is about.
    /// [`Decision::score`] on the shipped path is the *post*-substitution
    /// margin; the two differ exactly on `knife_edge` decisions.
    #[cfg(feature = "magnitude-fast")]
    #[must_use]
    pub fn probe_join(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        ctx: &DecisionContext,
    ) -> Option<JoinProbe> {
        let required = ctx.required_capabilities;
        let (masks_with, masks_without) = Self::join_masks(agent, coalition, required);
        if masks_without.is_empty() || masks_with.len() == masks_without.len() {
            return None;
        }
        let private = Mutex::new(None);
        let eval =
            cached_base_and_with_report(&private, required, &masks_without, agent.capabilities())
                .ok()?;
        let with = eval.with.ok()?;
        Some(JoinProbe {
            base: eval.base,
            with,
            zero_proof: eval.zero_proof,
            knife_edge: is_knife_edge(with, eval.base, self.join_margin),
        })
    }

    /// A policy that scores leaves through the reduced-set incremental evaluator
    /// (leave variant B) instead of the default fresh two-evaluation path.
    ///
    /// Opt-in and **not** the default: variant B's `Mag(S)`-with-leaver side
    /// comes from [`CoalitionEvaluator::value_with`], which is tolerance-equal
    /// (within [`catgraph_magnitude::INCREMENTAL_REL_TOL`]) rather than
    /// bit-identical to fresh. Provided so the #14 A/B battery can measure it;
    /// prefer [`MagnitudePolicy::new`] / [`MagnitudePolicy::default`] until
    /// measurement says otherwise.
    #[must_use]
    pub fn with_evaluator_leave(join_margin: f64) -> Self {
        Self {
            join_margin,
            leave_variant: LeaveVariant::Evaluator,
            fresh_route: FreshRoute::default(),
            cache: Arc::default(),
            typed: None,
        }
    }

    /// Snapshot the deduped, task-relevant `(with, without)` capability-mask
    /// lists for a join decision. `coalition` excludes `agent`; `with` is
    /// `coalition + agent`, `without` is `coalition`, both deduped by `agent_id`
    /// and with task-irrelevant agents excluded (see [`relevant_masks`]).
    /// Returns owned `Vec<u32>` so the async path can move them into the rayon
    /// closure.
    fn join_masks(
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        required: u32,
    ) -> (Vec<u32>, Vec<u32>) {
        let masks_without = relevant_masks(coalition, required);
        let mut with: Vec<&dyn AgentCapabilities> = coalition.to_vec();
        with.push(agent);
        let masks_with = relevant_masks(&with, required);
        (masks_with, masks_without)
    }

    /// Snapshot the deduped, task-relevant `(in, out)` capability-mask lists for
    /// a leave decision. `coalition` includes `agent`; `in` is the coalition,
    /// `out` is the coalition with `agent` removed by `agent_id`, both deduped
    /// and with task-irrelevant agents excluded (see [`relevant_masks`]).
    /// Returns owned `Vec<u32>` for the async path.
    fn leave_masks(
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        required: u32,
    ) -> (Vec<u32>, Vec<u32>) {
        let masks_in = relevant_masks(coalition, required);
        let agent_id = agent.agent_id();
        let without: Vec<&dyn AgentCapabilities> = coalition
            .iter()
            .filter(|a| a.agent_id() != agent_id)
            .copied()
            .collect();
        let masks_out = relevant_masks(&without, required);
        (masks_in, masks_out)
    }

    /// The typed analogue of [`join_masks`](Self::join_masks): the `(with,
    /// without)` [`TypedSide`]s for a join decision, roles included.
    ///
    /// `None` (⇒ [`TYPED_DECLINE`]) when a participating agent has no role — see
    /// [`typed_relevant_masks`], which also emits the warning.
    fn typed_join_sides(
        cfg: &TypedConfig,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        required: u32,
    ) -> Option<(TypedSide, TypedSide)> {
        let without = typed_relevant_masks(coalition, required, &cfg.roles)?;
        let mut with_view: Vec<&dyn AgentCapabilities> = coalition.to_vec();
        with_view.push(agent);
        let with = typed_relevant_masks(&with_view, required, &cfg.roles)?;
        Some((with, without))
    }

    /// The typed analogue of [`leave_masks`](Self::leave_masks): the `(in, out)`
    /// [`TypedSide`]s for a leave decision, roles included. `coalition` includes
    /// `agent`, which is removed from the `out` side by `agent_id`.
    ///
    /// `None` (⇒ [`TYPED_DECLINE`]) when a participating agent has no role.
    fn typed_leave_sides(
        cfg: &TypedConfig,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        required: u32,
    ) -> Option<(TypedSide, TypedSide)> {
        let side_in = typed_relevant_masks(coalition, required, &cfg.roles)?;
        let agent_id = agent.agent_id();
        let without: Vec<&dyn AgentCapabilities> = coalition
            .iter()
            .filter(|a| a.agent_id() != agent_id)
            .copied()
            .collect();
        let side_out = typed_relevant_masks(&without, required, &cfg.roles)?;
        Some((side_in, side_out))
    }

    /// Typed join decision: two **fresh** `ρ`-modulated magnitudes through the
    /// shared [`join_decision_from_values`](Self::join_decision_from_values)
    /// rule.
    ///
    /// No cache and no knife-edge band (module docs, contract point 2), so the
    /// three-way dispatch the untyped
    /// [`join_dispatch`](Self::join_dispatch) needs collapses: an empty side is
    /// handled by [`typed_magnitude_or_zero`], and an excluded candidate makes
    /// the two sides *identical inputs* — hence identical values and an exactly
    /// `0.0` margin, the pre-K6 fresh behaviour.
    fn typed_join_dispatch(
        cfg: &TypedConfig,
        required: u32,
        with: &TypedSide,
        without: &TypedSide,
        join_margin: f64,
    ) -> Decision {
        let mag_with = typed_magnitude_or_zero(with, required, &cfg.rho);
        let mag_without = typed_magnitude_or_zero(without, required, &cfg.rho);
        Self::join_decision_from_values(mag_with, mag_without, join_margin)
    }

    /// Typed leave decision: the dual of
    /// [`typed_join_dispatch`](Self::typed_join_dispatch), mirroring the untyped
    /// default (variant A) leave path — two fresh magnitudes through
    /// [`leave_decision_from_values`](Self::leave_decision_from_values).
    fn typed_leave_dispatch(
        cfg: &TypedConfig,
        required: u32,
        side_in: &TypedSide,
        side_out: &TypedSide,
    ) -> Decision {
        let mag_in = typed_magnitude_or_zero(side_in, required, &cfg.rho);
        let mag_out = typed_magnitude_or_zero(side_out, required, &cfg.rho);
        Self::leave_decision_from_values(mag_in, mag_out)
    }

    /// Pure join decision over owned magnitude results.
    ///
    /// `with` / `without` are the coalition magnitudes with and without the
    /// candidate (the empty "without" side is pre-mapped to `Ok(0.0)`). Shared
    /// by the sync [`CoalitionDecisionPolicy::should_join`] and the async
    /// [`CoalitionDecisionPolicy::should_join_async`] override. Any upstream
    /// [`CatgraphError`] is a policy-level decline (`act = false`, `score = 0.0`)
    /// — never an unwrap/panic. A non-finite margin is guarded the same way.
    fn join_decision_from_values(
        with: Result<f64, CatgraphError>,
        without: Result<f64, CatgraphError>,
        join_margin: f64,
    ) -> Decision {
        let (with, without) = match (with, without) {
            (Ok(w), Ok(wo)) => (w, wo),
            (Err(e), _) | (_, Err(e)) => {
                tracing::warn!(error = %e, "magnitude join computation failed");
                return Decision {
                    act: false,
                    score: 0.0,
                };
            }
        };
        let margin = with - without;
        if !margin.is_finite() {
            return Decision {
                act: false,
                score: 0.0,
            };
        }
        Decision {
            act: margin > join_margin,
            score: margin,
        }
    }

    /// Pure leave decision over owned magnitude results.
    ///
    /// `mag_in` is the coalition magnitude with the agent present, `mag_out` the
    /// magnitude after removing it (the empty remainder is pre-mapped to
    /// `Ok(0.0)`). If removing the agent does not lower magnitude
    /// (`mag_in - mag_out <= 0`) it contributes no diversity and should leave
    /// (exact mirror of the AIF arm's `g_out - g_in <= 0`). Shared by the sync
    /// and async leave paths; upstream error or non-finite delta ⇒ decline.
    fn leave_decision_from_values(
        mag_in: Result<f64, CatgraphError>,
        mag_out: Result<f64, CatgraphError>,
    ) -> Decision {
        let (mag_in, mag_out) = match (mag_in, mag_out) {
            (Ok(i), Ok(o)) => (i, o),
            (Err(e), _) | (_, Err(e)) => {
                tracing::warn!(error = %e, "magnitude leave computation failed");
                return Decision {
                    act: false,
                    score: 0.0,
                };
            }
        };
        let delta = mag_in - mag_out;
        if !delta.is_finite() {
            return Decision {
                act: false,
                score: 0.0,
            };
        }
        Decision {
            act: delta <= 0.0,
            score: delta,
        }
    }

    /// Join decision over the membership-keyed evaluator `cache`.
    ///
    /// Splits the three cases the pre-cache fresh path handled implicitly:
    /// - **empty coalition** (`masks_without` empty, incl. `required == 0` or an
    ///   all-irrelevant coalition): fresh verbatim — trivial cost, and the
    ///   evaluator cannot key on an empty member set;
    /// - **candidate excluded** (`masks_with.len() == masks_without.len()`: the
    ///   candidate is task-irrelevant or a duplicate `agent_id`): fresh computed
    ///   two identical magnitudes ⇒ margin exactly `0.0`; reproduced here as
    ///   `Mag(S) − Mag(S)` from a single [`cached_base`], preserving error parity
    ///   (the evaluator build errs iff the fresh evaluation would);
    /// - **normal**: `Mag(S ∪ {x}) − Mag(S)` via [`cached_base_and_with`].
    ///
    /// `route` selects the arithmetic used by the *fresh* evaluations only (the
    /// empty-coalition branch and the knife-edge recompute) — EQ3 lever L3.
    fn join_dispatch(
        cache: &Mutex<Option<EvaluatorCache>>,
        required: u32,
        masks_with: &[u32],
        masks_without: &[u32],
        candidate_caps: u32,
        join_margin: f64,
        route: FreshRoute,
    ) -> Decision {
        if masks_without.is_empty() {
            let mag_with = route.magnitude_or_zero(masks_with, required);
            let mag_without = route.magnitude_or_zero(masks_without, required);
            return Self::join_decision_from_values(mag_with, mag_without, join_margin);
        }
        if masks_with.len() == masks_without.len() {
            // Candidate excluded: margin is exactly 0 on success, decline on a
            // build error (parity with the fresh double evaluation).
            return match cached_base(cache, required, masks_without) {
                Ok(base) => Self::join_decision_from_values(Ok(base), Ok(base), join_margin),
                Err(e) => {
                    tracing::warn!(error = %e, "magnitude join computation failed");
                    Decision {
                        act: false,
                        score: 0.0,
                    }
                }
            };
        }
        match cached_candidate_eval(cache, required, masks_without, candidate_caps, route) {
            Ok(eval) => {
                let base = eval.base;
                #[cfg(feature = "magnitude-fast")]
                let zero_proof = eval.zero_proof;
                // Proof branch (EQ3 L2, toggle ON only — the arm the certificate
                // was even requested on): a certificate means the REAL increment
                // is exactly 0, so `Mag(S ∪ {x}) = Mag(S)` — take `base` and
                // skip the recompute. Branch on the proof, never on the computed
                // increment (which can be roundoff-nonzero).
                //
                // Otherwise (and always on the frozen default path) knife-edge:
                // when the incremental margin is within the band of the decision
                // threshold (`join_margin` — caller-settable, not necessarily
                // 0), the margin-vs-threshold comparison is
                // float-noise-dominated. Recompute `Mag(S ∪ {x})` fresh so it
                // reproduces the pre-evaluator behaviour bit-exactly (`base` is
                // already bit-identical to fresh `Mag(S)`).
                let with = match eval.with {
                    #[cfg(feature = "magnitude-fast")]
                    Ok(_) if zero_proof.is_some() => Ok(base),
                    Ok(w) if is_knife_edge(w, base, join_margin) => {
                        route.magnitude(masks_with, required)
                    }
                    other => other,
                };
                Self::join_decision_from_values(with, Ok(base), join_margin)
            }
            Err(e) => {
                tracing::warn!(error = %e, "magnitude join computation failed");
                Decision {
                    act: false,
                    score: 0.0,
                }
            }
        }
    }

    /// Leave decision over the membership-keyed evaluator `cache` (variant B).
    ///
    /// Mirrors [`join_dispatch`](Self::join_dispatch): `masks_out` empty (sole
    /// member) or leaver-excluded (`masks_in.len() == masks_out.len()`) reproduce
    /// the fresh guards exactly; the normal case scores `Mag(S ∖ {x})` from
    /// [`CoalitionEvaluator::base_value`] (bit-identical) and `Mag(S)` from
    /// [`CoalitionEvaluator::value_with`] (tolerance-equal) via
    /// [`cached_base_and_with`] with the leaver as the pool candidate.
    fn leave_dispatch_evaluator(
        cache: &Mutex<Option<EvaluatorCache>>,
        required: u32,
        masks_in: &[u32],
        masks_out: &[u32],
        leaver_caps: u32,
        route: FreshRoute,
    ) -> Decision {
        if masks_out.is_empty() {
            let mag_in = route.magnitude_or_zero(masks_in, required);
            let mag_out = route.magnitude_or_zero(masks_out, required);
            return Self::leave_decision_from_values(mag_in, mag_out);
        }
        if masks_in.len() == masks_out.len() {
            // Leaver excluded: delta exactly 0 ⇒ leave, decline on build error.
            return match cached_base(cache, required, masks_out) {
                Ok(base) => Self::leave_decision_from_values(Ok(base), Ok(base)),
                Err(e) => {
                    tracing::warn!(error = %e, "magnitude leave computation failed");
                    Decision {
                        act: false,
                        score: 0.0,
                    }
                }
            };
        }
        // `with` = Mag(S ∖ {x} ∪ {x}) = Mag(S) = mag_in; `base` = Mag(S ∖ {x}) = mag_out.
        match cached_candidate_eval(cache, required, masks_out, leaver_caps, route) {
            Ok(eval) => {
                let base = eval.base;
                #[cfg(feature = "magnitude-fast")]
                let zero_proof = eval.zero_proof;
                // Proof branch (EQ3 L2, toggle ON only): a certificate means the
                // leaver's real diversity contribution is exactly 0, i.e.
                // `Mag(S) = Mag(S ∖ {x})` — take `base`, skip the recompute, and
                // let the `delta <= 0` rule fire on an exact zero.
                //
                // Otherwise (and always on the frozen default path) knife-edge:
                // recompute `Mag(S)` fresh when the incremental delta is
                // band-close to zero (the leave threshold is hardcoded
                // `<= 0.0`), so the sign-vs-zero leave decision reproduces the
                // pre-evaluator behaviour bit-exactly.
                let with = match eval.with {
                    #[cfg(feature = "magnitude-fast")]
                    Ok(_) if zero_proof.is_some() => Ok(base),
                    Ok(w) if is_knife_edge(w, base, 0.0) => route.magnitude(masks_in, required),
                    other => other,
                };
                Self::leave_decision_from_values(with, Ok(base))
            }
            Err(e) => {
                tracing::warn!(error = %e, "magnitude leave computation failed");
                Decision {
                    act: false,
                    score: 0.0,
                }
            }
        }
    }
}

impl CoalitionDecisionPolicy for MagnitudePolicy {
    /// Join iff adding `agent` raises coalition magnitude by more than
    /// `join_margin`.
    ///
    /// `coalition` excludes `agent` (see [module docs](super)).
    /// `margin = Mag(coalition + agent) - Mag(coalition)` is positive when the
    /// candidate adds effective-member diversity (new task-relevant coverage or
    /// non-substitutable specialization).
    ///
    /// With a typed configuration ([`with_role_modulation`](Self::with_role_modulation))
    /// the margin is taken over `ρ`-modulated couplings, evaluated fresh on both
    /// sides; without one this is the untyped, evaluator-cached path verbatim.
    fn should_join(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        ctx: &DecisionContext,
    ) -> Decision {
        let required = ctx.required_capabilities;
        if let Some(cfg) = self.typed.as_deref() {
            let Some((with, without)) = Self::typed_join_sides(cfg, agent, coalition, required)
            else {
                return TYPED_DECLINE;
            };
            return Self::typed_join_dispatch(cfg, required, &with, &without, self.join_margin);
        }
        let (masks_with, masks_without) = Self::join_masks(agent, coalition, required);
        Self::join_dispatch(
            &self.cache,
            required,
            &masks_with,
            &masks_without,
            agent.capabilities(),
            self.join_margin,
            self.fresh_route,
        )
    }

    /// Leave iff removing `agent` does not lower coalition magnitude.
    ///
    /// `coalition` includes `agent` (see [module docs](super)).
    /// `delta = Mag(coalition) - Mag(coalition - agent)`; a redundant member
    /// (a clone, or one whose task-relevant capabilities are subsumed) leaves
    /// magnitude unchanged (`delta <= 0`) and should leave. A sole member has
    /// `delta = 1 - 0 > 0` and stays.
    ///
    /// With a typed configuration ([`with_role_modulation`](Self::with_role_modulation))
    /// the delta is taken over `ρ`-modulated couplings, evaluated fresh on both
    /// sides (the leave variant does not apply — see the module docs).
    fn should_leave(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        ctx: &DecisionContext,
    ) -> Decision {
        let required = ctx.required_capabilities;
        if let Some(cfg) = self.typed.as_deref() {
            let Some((side_in, side_out)) =
                Self::typed_leave_sides(cfg, agent, coalition, required)
            else {
                return TYPED_DECLINE;
            };
            return Self::typed_leave_dispatch(cfg, required, &side_in, &side_out);
        }
        let (masks_in, masks_out) = Self::leave_masks(agent, coalition, required);
        match self.leave_variant {
            LeaveVariant::Fresh => {
                let mag_in = self.fresh_route.magnitude_or_zero(&masks_in, required);
                let mag_out = self.fresh_route.magnitude_or_zero(&masks_out, required);
                Self::leave_decision_from_values(mag_in, mag_out)
            }
            LeaveVariant::Evaluator => Self::leave_dispatch_evaluator(
                &self.cache,
                required,
                &masks_in,
                &masks_out,
                agent.capabilities(),
                self.fresh_route,
            ),
        }
    }

    /// Async, runtime-friendly override of [`should_join`](Self::should_join).
    ///
    /// Snapshots the deduped capability masks to owned `Vec<u32>` in the sync
    /// prologue (the `&dyn` borrows are not `'static`), then offloads the entire
    /// `O(m³)` magnitude computation to the rayon pool via [`tokio_rayon::spawn`]
    /// so the tokio worker thread is not blocked. Produces the same [`Decision`]
    /// as the sync [`should_join`](Self::should_join).
    ///
    /// `coalition` excludes `agent` (same convention as the sync method).
    fn should_join_async<'a>(
        &'a self,
        agent: &'a dyn AgentCapabilities,
        coalition: &'a [&'a dyn AgentCapabilities],
        ctx: &'a DecisionContext,
    ) -> Pin<Box<dyn Future<Output = Decision> + Send + 'a>> {
        let required = ctx.required_capabilities;
        let join_margin = self.join_margin;
        // Typed arm: snapshot the owned `(masks, roles)` sides and an owned
        // config handle in the sync prologue, exactly as the untyped path
        // snapshots masks, then offload the fresh magnitudes.
        if let Some(cfg) = self.typed.clone() {
            let sides = Self::typed_join_sides(&cfg, agent, coalition, required);
            return Box::pin(async move {
                let Some((with, without)) = sides else {
                    return TYPED_DECLINE;
                };
                tokio_rayon::spawn(move || {
                    Self::typed_join_dispatch(&cfg, required, &with, &without, join_margin)
                })
                .await
            });
        }
        let (masks_with, masks_without) = Self::join_masks(agent, coalition, required);
        let candidate_caps = agent.capabilities();
        let route = self.fresh_route;
        let cache = Arc::clone(&self.cache);

        Box::pin(async move {
            tokio_rayon::spawn(move || {
                Self::join_dispatch(
                    &cache,
                    required,
                    &masks_with,
                    &masks_without,
                    candidate_caps,
                    join_margin,
                    route,
                )
            })
            .await
        })
    }

    /// Async, runtime-friendly override of [`should_leave`](Self::should_leave).
    ///
    /// Snapshots the deduped capability masks to owned `Vec<u32>` in the sync
    /// prologue, then offloads the entire `O(m³)` magnitude computation to the
    /// rayon pool via [`tokio_rayon::spawn`]. Produces the same [`Decision`] as
    /// the sync [`should_leave`](Self::should_leave).
    ///
    /// `coalition` includes `agent` (same convention as the sync method).
    fn should_leave_async<'a>(
        &'a self,
        agent: &'a dyn AgentCapabilities,
        coalition: &'a [&'a dyn AgentCapabilities],
        ctx: &'a DecisionContext,
    ) -> Pin<Box<dyn Future<Output = Decision> + Send + 'a>> {
        let required = ctx.required_capabilities;
        // Typed arm: same prologue-snapshot / offload shape as the join path.
        if let Some(cfg) = self.typed.clone() {
            let sides = Self::typed_leave_sides(&cfg, agent, coalition, required);
            return Box::pin(async move {
                let Some((side_in, side_out)) = sides else {
                    return TYPED_DECLINE;
                };
                tokio_rayon::spawn(move || {
                    Self::typed_leave_dispatch(&cfg, required, &side_in, &side_out)
                })
                .await
            });
        }
        let (masks_in, masks_out) = Self::leave_masks(agent, coalition, required);
        let leaver_caps = agent.capabilities();
        let variant = self.leave_variant;
        let route = self.fresh_route;
        let cache = Arc::clone(&self.cache);

        Box::pin(async move {
            tokio_rayon::spawn(move || match variant {
                LeaveVariant::Fresh => {
                    let mag_in = route.magnitude_or_zero(&masks_in, required);
                    let mag_out = route.magnitude_or_zero(&masks_out, required);
                    Self::leave_decision_from_values(mag_in, mag_out)
                }
                LeaveVariant::Evaluator => Self::leave_dispatch_evaluator(
                    &cache,
                    required,
                    &masks_in,
                    &masks_out,
                    leaver_caps,
                    route,
                ),
            })
            .await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy)]
    struct TestAgent {
        id: usize,
        caps: u32,
        trust: u32,
    }

    impl AgentCapabilities for TestAgent {
        fn agent_id(&self) -> usize {
            self.id
        }
        fn capabilities(&self) -> u32 {
            self.caps
        }
        fn trust_level(&self) -> u32 {
            self.trust
        }
    }

    const EPS: f64 = 1e-9;

    // -----------------------------------------------------------------------
    // 1. Upstream seam pins — call `coalition_value` directly.
    // -----------------------------------------------------------------------

    #[test]
    fn upstream_chain_and_skeletalization_pins() {
        // Chain a→b→c with couplings 0.7, 0.5 over 3 members ⇒ Mag(1) = 1.80.
        let agents = [0usize, 1, 2];
        let chain = [(0usize, 1usize, 0.7f64), (1, 2, 0.5)];
        let mag = catgraph_magnitude::coalition_value(&agents, &chain, &agents).unwrap();
        assert!(
            (mag - 1.80).abs() < EPS,
            "chain Mag(1) = {mag}, expected 1.80"
        );

        // Mutual-1.0 pair ⇒ skeletalized to ONE effective agent ⇒ Mag = 1.0.
        let pair = [0usize, 1];
        let mutual = [(0usize, 1usize, 1.0f64), (1, 0, 1.0)];
        let mag = catgraph_magnitude::coalition_value(&pair, &mutual, &pair).unwrap();
        assert!(
            (mag - 1.0).abs() < EPS,
            "mutual-1.0 Mag = {mag}, expected 1.0"
        );
    }

    // -----------------------------------------------------------------------
    // 2. The capability→coupling mapping.
    // -----------------------------------------------------------------------

    #[test]
    fn coupling_mapping() {
        // Identical relevant masks ⇒ full substitutability.
        assert!((CouplingModel::coupling(0b011, 0b011, 0b111) - 1.0).abs() < EPS);
        // Subset (from ⊂ to): all of from's bits are in to ⇒ 1.0.
        assert!((CouplingModel::coupling(0b001, 0b011, 0b111) - 1.0).abs() < EPS);
        // Superset (from ⊃ to): only half of from's bits are in to ⇒ 0.5.
        assert!((CouplingModel::coupling(0b011, 0b001, 0b111) - 0.5).abs() < EPS);
        // Disjoint ⇒ 0.0.
        assert!(CouplingModel::coupling(0b001, 0b010, 0b111).abs() < EPS);
        // Required masking: the 0b001 bit lies outside required 0b100 and is
        // ignored, so both agents look identical on the task ⇒ 1.0.
        assert!((CouplingModel::coupling(0b101, 0b100, 0b100) - 1.0).abs() < EPS);
        // rel_from == 0 (nothing task-relevant) ⇒ no substitutability relation.
        assert!(CouplingModel::coupling(0b1000, 0b111, 0b111).abs() < EPS);
        // required == 0 ⇒ 0.0 for any pair (every agent is irrelevant).
        assert!(CouplingModel::coupling(0b001, 0b010, 0).abs() < EPS);
    }

    // -----------------------------------------------------------------------
    // 3. Calculator through the `ValueCalculator` surface (hand-computed).
    // -----------------------------------------------------------------------

    #[test]
    fn calculator_hand_computed_values_and_dedup() {
        let calc = MagnitudeValueCalculator::new(0b111);

        let s0 = TestAgent {
            id: 0,
            caps: 0b001,
            trust: 50,
        };
        let s1 = TestAgent {
            id: 1,
            caps: 0b010,
            trust: 50,
        };
        let s2 = TestAgent {
            id: 2,
            caps: 0b100,
            trust: 50,
        };

        // Disjoint specialists ⇒ zero couplings ⇒ Mag = 3.0.
        let specialists: [&dyn AgentCapabilities; 3] = [&s0, &s1, &s2];
        assert!((calc.calculate_value(&specialists) - 3.0).abs() < EPS);

        // Clone pair (distinct ids, same caps) ⇒ mutual-1.0 ⇒ 1 effective ⇒ 1.0.
        let c0 = TestAgent {
            id: 0,
            caps: 0b001,
            trust: 50,
        };
        let c1 = TestAgent {
            id: 1,
            caps: 0b001,
            trust: 50,
        };
        let clones: [&dyn AgentCapabilities; 2] = [&c0, &c1];
        assert!((calc.calculate_value(&clones) - 1.0).abs() < EPS);

        // Half-overlap pair {0b011, 0b110} ⇒ mutual-0.5 ⇒ Mag = 4/3.
        let h0 = TestAgent {
            id: 0,
            caps: 0b011,
            trust: 50,
        };
        let h1 = TestAgent {
            id: 1,
            caps: 0b110,
            trust: 50,
        };
        let half: [&dyn AgentCapabilities; 2] = [&h0, &h1];
        assert!((calc.calculate_value(&half) - 4.0 / 3.0).abs() < EPS);

        // Subsumption pair {0b001, 0b011}: A(0→1)=1.0, A(1→0)=0.5 ⇒ w=(0,1) ⇒ 1.0.
        let sub0 = TestAgent {
            id: 0,
            caps: 0b001,
            trust: 50,
        };
        let sub1 = TestAgent {
            id: 1,
            caps: 0b011,
            trust: 50,
        };
        let subsume: [&dyn AgentCapabilities; 2] = [&sub0, &sub1];
        assert!((calc.calculate_value(&subsume) - 1.0).abs() < EPS);

        // Ordering: disjoint (2.0) > half-overlap (4/3) > clones (1.0).
        let d0 = TestAgent {
            id: 0,
            caps: 0b001,
            trust: 50,
        };
        let d1 = TestAgent {
            id: 1,
            caps: 0b010,
            trust: 50,
        };
        let disjoint_pair: [&dyn AgentCapabilities; 2] = [&d0, &d1];
        let v_disjoint = calc.calculate_value(&disjoint_pair);
        let v_half = calc.calculate_value(&half);
        let v_clones = calc.calculate_value(&clones);
        assert!((v_disjoint - 2.0).abs() < EPS);
        assert!(v_disjoint > v_half && v_half > v_clones);

        // Empty slice ⇒ 0.0.
        assert!(calc.calculate_value(&[]).abs() < EPS);

        // Dedup: the same agent_id listed twice scores like once.
        let twice: [&dyn AgentCapabilities; 2] = [&s0, &s0];
        let once: [&dyn AgentCapabilities; 1] = [&s0];
        assert!((calc.calculate_value(&twice) - calc.calculate_value(&once)).abs() < EPS);

        // Object safety alongside the existing calculators.
        let _: &dyn ValueCalculator = &calc;
    }

    // -----------------------------------------------------------------------
    // 4. Policy join.
    // -----------------------------------------------------------------------

    #[test]
    fn policy_join() {
        let policy = MagnitudePolicy::default();
        let ctx = DecisionContext {
            required_capabilities: 0b111,
        };

        let cand = TestAgent {
            id: 0,
            caps: 0b001,
            trust: 50,
        };
        let m1 = TestAgent {
            id: 1,
            caps: 0b010,
            trust: 50,
        };
        let m2 = TestAgent {
            id: 2,
            caps: 0b100,
            trust: 50,
        };

        // Joining raises Mag 2.0 → 3.0 ⇒ act, score ≈ 1.0.
        let coalition: [&dyn AgentCapabilities; 2] = [&m1, &m2];
        let d = policy.should_join(&cand, &coalition, &ctx);
        assert!(d.act, "specialist should join (score={})", d.score);
        assert!(
            (d.score - 1.0).abs() < EPS,
            "join margin = {}, expected 1.0",
            d.score
        );

        // Clone candidate joining a single clone: Mag 1.0 → 1.0 ⇒ decline, ≈ 0.0.
        let clone_partner = TestAgent {
            id: 1,
            caps: 0b001,
            trust: 50,
        };
        let clone_coalition: [&dyn AgentCapabilities; 1] = [&clone_partner];
        let d = policy.should_join(&cand, &clone_coalition, &ctx);
        assert!(!d.act, "redundant clone must not join (score={})", d.score);
        assert!(
            d.score.abs() < EPS,
            "clone join margin = {}, expected 0.0",
            d.score
        );

        // required == 0 ⇒ Mag = 1 both sides ⇒ decline, ≈ 0.0.
        let ctx0 = DecisionContext {
            required_capabilities: 0,
        };
        let d = policy.should_join(&cand, &coalition, &ctx0);
        assert!(!d.act, "no requirements ⇒ no reason to join");
        assert!(
            d.score.abs() < EPS,
            "no-req join margin = {}, expected 0.0",
            d.score
        );

        // Object safety behind a trait object.
        let _: Box<dyn CoalitionDecisionPolicy> = Box::new(MagnitudePolicy::default());
    }

    // -----------------------------------------------------------------------
    // 5. Policy leave.
    // -----------------------------------------------------------------------

    #[test]
    fn policy_leave_when_redundant_else_stay() {
        let policy = MagnitudePolicy::default();
        let ctx = DecisionContext {
            required_capabilities: 0b111,
        };

        let a0 = TestAgent {
            id: 0,
            caps: 0b001,
            trust: 50,
        };
        let a1 = TestAgent {
            id: 1,
            caps: 0b010,
            trust: 50,
        };
        let a2 = TestAgent {
            id: 2,
            caps: 0b100,
            trust: 50,
        };

        // Unique contributor: removing a0 drops Mag 3.0 → 2.0 ⇒ delta ≈ 1.0 ⇒ stay.
        let full: [&dyn AgentCapabilities; 3] = [&a0, &a1, &a2];
        let stay = policy.should_leave(&a0, &full, &ctx);
        assert!(
            !stay.act,
            "unique contributor should stay (score={})",
            stay.score
        );
        assert!(
            (stay.score - 1.0).abs() < EPS,
            "stay delta = {}, expected 1.0",
            stay.score
        );

        // Redundant clone (id 3, caps 0b001) in the 4-member set: removing it
        // leaves Mag 3.0 → 3.0 ⇒ delta ≈ 0.0 ⇒ leave.
        let redundant = TestAgent {
            id: 3,
            caps: 0b001,
            trust: 50,
        };
        let with_clone: [&dyn AgentCapabilities; 4] = [&a0, &a1, &a2, &redundant];
        let leave = policy.should_leave(&redundant, &with_clone, &ctx);
        assert!(
            leave.act,
            "redundant clone should leave (score={})",
            leave.score
        );
        assert!(
            leave.score.abs() < EPS,
            "leave delta = {}, expected 0.0",
            leave.score
        );

        // Sole member: Mag 1.0 → 0.0 ⇒ delta = 1.0 > 0 ⇒ stay.
        let solo_coalition: [&dyn AgentCapabilities; 1] = [&a0];
        let d = policy.should_leave(&a0, &solo_coalition, &ctx);
        assert!(!d.act, "sole member should stay (score={})", d.score);
        assert!(
            (d.score - 1.0).abs() < EPS,
            "sole-member delta = {}, expected 1.0",
            d.score
        );
    }

    // -----------------------------------------------------------------------
    // 5b. Irrelevant agents are excluded, not vacuously coupled (review
    // regression): a bystander with no task-relevant capabilities must neither
    // collapse coalition diversity nor eject a unique specialist.
    // -----------------------------------------------------------------------

    #[test]
    fn irrelevant_bystander_neither_collapses_value_nor_corrupts_decisions() {
        let calc = MagnitudeValueCalculator::new(0b111);
        let policy = MagnitudePolicy::default();
        let ctx = DecisionContext {
            required_capabilities: 0b111,
        };

        let a0 = TestAgent {
            id: 0,
            caps: 0b001,
            trust: 50,
        };
        let a1 = TestAgent {
            id: 1,
            caps: 0b010,
            trust: 50,
        };
        let a2 = TestAgent {
            id: 2,
            caps: 0b100,
            trust: 50,
        };
        // All of x's capability bits lie outside required 0b111.
        let bystander = TestAgent {
            id: 3,
            caps: 0b1000,
            trust: 50,
        };

        // Value: the bystander is excluded, so diversity stays 3.0 (a vacuous
        // 1.0 coupling would collapse this to 1.0 via a negative Möbius weight).
        let with_bystander: [&dyn AgentCapabilities; 4] = [&a0, &a1, &a2, &bystander];
        assert!(
            (calc.calculate_value(&with_bystander) - 3.0).abs() < EPS,
            "bystander must not distort diversity"
        );

        // Join: the bystander adds no task-relevant diversity ⇒ decline, ≈ 0.0.
        let specialists: [&dyn AgentCapabilities; 3] = [&a0, &a1, &a2];
        let join = policy.should_join(&bystander, &specialists, &ctx);
        assert!(!join.act, "irrelevant candidate must not join");
        assert!(join.score.abs() < EPS);

        // Leave: a unique specialist stays despite the bystander's presence
        // (pre-fix, the collapsed magnitude made delta 0 and ejected it) ...
        let stay = policy.should_leave(&a0, &with_bystander, &ctx);
        assert!(
            !stay.act,
            "unique specialist must stay despite bystander (score={})",
            stay.score
        );
        assert!((stay.score - 1.0).abs() < EPS);

        // ... while the bystander itself is redundant and leaves.
        let leave = policy.should_leave(&bystander, &with_bystander, &ctx);
        assert!(leave.act, "irrelevant member should leave");
        assert!(leave.score.abs() < EPS);

        // All-irrelevant coalition (required == 0 makes everyone irrelevant):
        // value 0.0, and every member's leave delta is 0 ⇒ leave (AIF mirror).
        let calc0 = MagnitudeValueCalculator::new(0);
        assert!(calc0.calculate_value(&specialists).abs() < EPS);
        let ctx0 = DecisionContext {
            required_capabilities: 0,
        };
        let leave0 = policy.should_leave(&a0, &specialists, &ctx0);
        assert!(leave0.act, "required == 0 ⇒ every member is redundant");
        assert!(leave0.score.abs() < EPS);
    }

    // -----------------------------------------------------------------------
    // 6. Err path — a real CatgraphError flows through the pure decision fns.
    // -----------------------------------------------------------------------

    #[test]
    fn upstream_error_declines_without_panic() {
        // Self-coupling (0, 0, _) is rejected upstream ⇒ a genuine CatgraphError.
        let err =
            catgraph_magnitude::coalition_value(&[0usize], &[(0usize, 0usize, 0.5f64)], &[0usize])
                .unwrap_err();

        let join = MagnitudePolicy::join_decision_from_values(Err(err.clone()), Ok(1.0), 0.0);
        assert!(
            !join.act && join.score.abs() < EPS,
            "join: error ⇒ decline, score 0.0"
        );

        let leave = MagnitudePolicy::leave_decision_from_values(Ok(1.0), Err(err));
        assert!(
            !leave.act && leave.score.abs() < EPS,
            "leave: error ⇒ decline, score 0.0"
        );
    }

    // -----------------------------------------------------------------------
    // 7. Async == sync, and async reachable through the trait object.
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn async_matches_sync_join_and_leave() {
        let policy = MagnitudePolicy::default();
        let ctx = DecisionContext {
            required_capabilities: 0b111,
        };

        let a0 = TestAgent {
            id: 0,
            caps: 0b001,
            trust: 50,
        };
        let a1 = TestAgent {
            id: 1,
            caps: 0b010,
            trust: 50,
        };
        let a2 = TestAgent {
            id: 2,
            caps: 0b100,
            trust: 50,
        };

        // Join: candidate a0 into {a1, a2}.
        let coalition: [&dyn AgentCapabilities; 2] = [&a1, &a2];
        let sync_join = policy.should_join(&a0, &coalition, &ctx);
        let async_join = policy.should_join_async(&a0, &coalition, &ctx).await;
        assert_eq!(sync_join, async_join, "async join must equal sync");
        assert!(async_join.act && async_join.score > 0.0);

        // Leave: unique contributor stays; redundant clone leaves.
        let full: [&dyn AgentCapabilities; 3] = [&a0, &a1, &a2];
        let sync_stay = policy.should_leave(&a0, &full, &ctx);
        let async_stay = policy.should_leave_async(&a0, &full, &ctx).await;
        assert_eq!(sync_stay, async_stay, "async leave (stay) must equal sync");
        assert!(!async_stay.act);

        let redundant = TestAgent {
            id: 3,
            caps: 0b001,
            trust: 50,
        };
        let with_clone: [&dyn AgentCapabilities; 4] = [&a0, &a1, &a2, &redundant];
        let sync_leave = policy.should_leave(&redundant, &with_clone, &ctx);
        let async_leave = policy
            .should_leave_async(&redundant, &with_clone, &ctx)
            .await;
        assert_eq!(
            sync_leave, async_leave,
            "async leave (leave) must equal sync"
        );
        assert!(async_leave.act);
    }

    #[tokio::test]
    async fn async_path_reachable_through_trait_object() {
        let p: Box<dyn CoalitionDecisionPolicy> = Box::new(MagnitudePolicy::default());
        let ctx = DecisionContext {
            required_capabilities: 0b111,
        };

        let a0 = TestAgent {
            id: 0,
            caps: 0b001,
            trust: 50,
        };
        let a1 = TestAgent {
            id: 1,
            caps: 0b010,
            trust: 50,
        };
        let a2 = TestAgent {
            id: 2,
            caps: 0b100,
            trust: 50,
        };
        let coalition: [&dyn AgentCapabilities; 2] = [&a1, &a2];

        // The dynamic-dispatch async call must produce the non-degenerate join.
        let d = p.should_join_async(&a0, &coalition, &ctx).await;
        assert!(
            d.act,
            "trait-object async join should fire (score={})",
            d.score
        );
        assert!(d.score > 0.0, "join margin must be positive");

        // And the leave override is reachable too: a0 provides unique coverage.
        let full: [&dyn AgentCapabilities; 3] = [&a0, &a1, &a2];
        let stay = p.should_leave_async(&a0, &full, &ctx).await;
        assert!(
            !stay.act,
            "trait-object async leave: unique contributor stays"
        );
    }

    // -----------------------------------------------------------------------
    // 8. Cache invalidation — one policy instance across changing keys.
    // -----------------------------------------------------------------------

    /// Fresh reference join decision (the pre-K6 two-evaluation logic), used to
    /// check that the evaluator-backed cache never changes a decision.
    fn ref_join(
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        required: u32,
        join_margin: f64,
    ) -> Decision {
        let (mw, mo) = MagnitudePolicy::join_masks(agent, coalition, required);
        MagnitudePolicy::join_decision_from_values(
            magnitude_or_zero(&mw, required),
            magnitude_or_zero(&mo, required),
            join_margin,
        )
    }

    /// Fresh reference leave decision (the pre-K6 two-evaluation logic).
    fn ref_leave(
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        required: u32,
    ) -> Decision {
        let (mi, mo) = MagnitudePolicy::leave_masks(agent, coalition, required);
        MagnitudePolicy::leave_decision_from_values(
            magnitude_or_zero(&mi, required),
            magnitude_or_zero(&mo, required),
        )
    }

    /// `act` strictly equal, `score` within 1e-9 relative.
    fn decisions_match(a: Decision, b: Decision) -> bool {
        a.act == b.act && rel_close(a.score, b.score)
    }

    fn rel_close(x: f64, y: f64) -> bool {
        (x - y).abs() <= 1e-9 * x.abs().max(y.abs()).max(1.0)
    }

    #[test]
    fn cache_survives_membership_required_and_candidate_changes() {
        let policy = MagnitudePolicy::default();

        let a0 = TestAgent {
            id: 0,
            caps: 0b001,
            trust: 50,
        };
        let a1 = TestAgent {
            id: 1,
            caps: 0b010,
            trust: 50,
        };
        let a2 = TestAgent {
            id: 2,
            caps: 0b100,
            trust: 50,
        };
        let cand = TestAgent {
            id: 9,
            caps: 0b010,
            trust: 50,
        };
        let ctx = DecisionContext {
            required_capabilities: 0b111,
        };

        // (a) membership change: same instance, two different coalitions.
        let c1: [&dyn AgentCapabilities; 1] = [&a0];
        let d1 = policy.should_join(&cand, &c1, &ctx);
        assert!(decisions_match(d1, ref_join(&cand, &c1, 0b111, 0.0)));

        let c2: [&dyn AgentCapabilities; 2] = [&a0, &a2];
        let d2 = policy.should_join(&cand, &c2, &ctx);
        assert!(decisions_match(d2, ref_join(&cand, &c2, 0b111, 0.0)));

        // Back to c1 — a rebuild, still correct.
        let d1b = policy.should_join(&cand, &c1, &ctx);
        assert!(decisions_match(d1b, ref_join(&cand, &c1, 0b111, 0.0)));

        // (b) required change on the same coalition.
        let ctx2 = DecisionContext {
            required_capabilities: 0b011,
        };
        let d3 = policy.should_join(&cand, &c2, &ctx2);
        assert!(decisions_match(d3, ref_join(&cand, &c2, 0b011, 0.0)));

        // (c) candidate mask equal to a member mask (true clone): margin ~0,
        // decline. Exercises the evaluator's skeletal-merge slow path.
        let clone_of_a1 = TestAgent {
            id: 8,
            caps: 0b010,
            trust: 50,
        };
        let with_a1: [&dyn AgentCapabilities; 2] = [&a1, &a2];
        let dclone = policy.should_join(&clone_of_a1, &with_a1, &ctx);
        assert!(!dclone.act, "true clone must not join");
        assert!(dclone.score.abs() < EPS, "clone margin exactly 0");
        assert!(decisions_match(
            dclone,
            ref_join(&clone_of_a1, &with_a1, 0b111, 0.0)
        ));

        // (d) repeated candidate against unchanged S — the O(m²) hit path.
        let coalition: [&dyn AgentCapabilities; 2] = [&a0, &a2];
        let first = policy.should_join(&cand, &coalition, &ctx);
        let second = policy.should_join(&cand, &coalition, &ctx);
        assert_eq!(first, second, "repeated candidate must be identical");
        assert!(decisions_match(
            first,
            ref_join(&cand, &coalition, 0b111, 0.0)
        ));
    }

    #[test]
    fn clone_shares_cache_without_corrupting_decisions() {
        // (e) two clones of the policy interleave different coalitions; rebuilds
        // thrash the shared cache but every answer stays correct.
        let p1 = MagnitudePolicy::default();
        let p2 = p1.clone();
        let ctx = DecisionContext {
            required_capabilities: 0b111,
        };

        let a0 = TestAgent {
            id: 0,
            caps: 0b001,
            trust: 50,
        };
        let a1 = TestAgent {
            id: 1,
            caps: 0b010,
            trust: 50,
        };
        let a2 = TestAgent {
            id: 2,
            caps: 0b100,
            trust: 50,
        };
        let cand = TestAgent {
            id: 9,
            caps: 0b110,
            trust: 50,
        };

        let coa: [&dyn AgentCapabilities; 1] = [&a0];
        let cob: [&dyn AgentCapabilities; 2] = [&a1, &a2];

        for _ in 0..4 {
            let da = p1.should_join(&cand, &coa, &ctx);
            let db = p2.should_join(&cand, &cob, &ctx);
            assert!(decisions_match(da, ref_join(&cand, &coa, 0b111, 0.0)));
            assert!(decisions_match(db, ref_join(&cand, &cob, 0b111, 0.0)));
        }
    }

    #[test]
    fn value_with_out_of_pool_would_decline_not_panic() {
        // A bordered-ζ singularity (or any value_with error) must decline. We
        // cannot hand-construct a post-skeletal singular ζ, so assert the plumbing
        // directly: the inner Result on cached_base_and_with feeds
        // join_decision_from_values, which declines on Err. Here we confirm a
        // clean pool query is Ok and, symmetrically, that a synthetic Err on the
        // `with` side declines (mirrors upstream_error_declines_without_panic).
        let cache = Mutex::new(None);
        let CandidateEval { base, with, .. } =
            cached_base_and_with(&cache, 0b111, &[0b001, 0b010], 0b100).unwrap();
        assert!(with.is_ok(), "clean candidate query is Ok");
        let ok = MagnitudePolicy::join_decision_from_values(with, Ok(base), 0.0);
        assert!(ok.act, "disjoint specialist joins");

        let err =
            catgraph_magnitude::coalition_value(&[0usize], &[(0usize, 0usize, 0.5f64)], &[0usize])
                .unwrap_err();
        let declined = MagnitudePolicy::join_decision_from_values(Err(err), Ok(base), 0.0);
        assert!(!declined.act && declined.score.abs() < EPS);
    }

    // -----------------------------------------------------------------------
    // 8b. Knife-edge fresh fallback (koalisi #14 parity gate).
    // -----------------------------------------------------------------------

    /// The load-bearing invariant the knife-edge fallback relies on:
    /// `build_evaluator(...).base_value()` is bit-identical (`==` on raw bits) to
    /// fresh `magnitude_of_masks(member_masks)` even for a NON-EMPTY candidate
    /// registry — including a candidate mask equal to a member mask. (The pool
    /// slots must never perturb the base coalition's magnitude.)
    #[test]
    fn base_value_bit_identical_with_nonempty_registry() {
        let required = 0b1111u32;
        let member_sets: [&[u32]; 4] = [
            &[0b0001, 0b0010, 0b0100],
            &[0b0011, 0b0110, 0b1100],
            &[0b0001, 0b0011],
            &[0b0001, 0b0010, 0b0100, 0b1000],
        ];
        // Registry deliberately includes a mask equal to a member mask (0b0001).
        let registry = [0b0001u32, 0b0101, 0b1110, 0b1000];

        for masks in member_sets {
            let ev = build_evaluator(required, masks, &registry).unwrap();
            let fresh = magnitude_of_masks(masks, required).unwrap();
            assert_eq!(
                ev.base_value().to_bits(),
                fresh.to_bits(),
                "base_value must be bit-identical to fresh for members {masks:?} with a non-empty registry"
            );
        }
    }

    /// Regression for the specific parity flip the lockstep probe caught
    /// (seed 3, task 1): a subsumed candidate whose true join margin is
    /// mathematically zero was decided by float noise. The evaluator-backed
    /// `should_join` must agree in `act` with the freshly-computed reference
    /// (assert equality with the reference, not a hardcoded outcome, so the test
    /// survives any future upstream numeric shift).
    ///
    /// EQ3 keeps this frozen: the default arm carries L1 only, so the band logic
    /// and the decision are unchanged. (The EQ3 arm decides this fixture the
    /// other way, on the exact certificate — see
    /// `knife_edge_flip_fixture_declines_on_the_eq3_arm`, prereg Amendment 1.)
    #[test]
    fn knife_edge_flip_fixture_matches_fresh() {
        let required = 0b1110_0100u32; // 228
        let m0 = TestAgent {
            id: 0,
            caps: 0b0110_0001,
            trust: 50,
        }; // 97
        let m1 = TestAgent {
            id: 1,
            caps: 0b0101_0101,
            trust: 50,
        }; // 85
        let candidate = TestAgent {
            id: 2,
            caps: 0b1100_0110,
            trust: 50,
        }; // 198
        let coalition: [&dyn AgentCapabilities; 2] = [&m0, &m1];
        let ctx = DecisionContext {
            required_capabilities: required,
        };

        let policy = MagnitudePolicy::default();
        let got = policy.should_join(&candidate, &coalition, &ctx);
        let want = ref_join(&candidate, &coalition, required, 0.0);
        assert_eq!(
            got.act, want.act,
            "knife-edge candidate must match fresh reference act (got {got:?}, want {want:?})"
        );
        assert!(
            decisions_match(got, want),
            "and score within 1e-9 relative (got {got:?}, want {want:?})"
        );
    }

    /// The same K6 fixture on the **EQ3 arm** (prereg Amendment 1 / A1.1): the
    /// candidate is certified an `IncomingProfileDuplicate`, so the real
    /// increment is exactly `0`, the policy takes `with := base` and declines —
    /// while the fresh reference still joins on `+2.22e-16` of roundoff. This is
    /// the registered, deliberate divergence; it is confined to this arm, which
    /// is exactly why `knife_edge_flip_fixture_matches_fresh` above still holds
    /// for the default policy.
    #[cfg(feature = "magnitude-fast")]
    #[test]
    fn knife_edge_flip_fixture_declines_on_the_eq3_arm() {
        let required = 0b1110_0100u32; // 228
        let m0 = TestAgent {
            id: 0,
            caps: 0b0110_0001,
            trust: 50,
        }; // 97
        let m1 = TestAgent {
            id: 1,
            caps: 0b0101_0101,
            trust: 50,
        }; // 85
        let candidate = TestAgent {
            id: 2,
            caps: 0b1100_0110,
            trust: 50,
        }; // 198
        let coalition: [&dyn AgentCapabilities; 2] = [&m0, &m1];
        let ctx = DecisionContext {
            required_capabilities: required,
        };

        let (masks_with, masks_without) =
            MagnitudePolicy::join_masks(&candidate, &coalition, required);
        assert_eq!(masks_with.len(), masks_without.len() + 1);
        assert!(
            matches!(
                probe_proof(required, &masks_without, candidate.capabilities()),
                Some(ZeroDiversityProof::IncomingProfileDuplicate { .. })
            ),
            "the fixture must be certified as an incoming profile duplicate"
        );

        let eq3 = MagnitudePolicy::default().with_eq3_levers(true);
        let got = eq3.should_join(&candidate, &coalition, &ctx);
        assert!(
            !got.act && got.score.abs() < f64::EPSILON,
            "the EQ3 arm must decline at an exact-zero margin (got {got:?})"
        );

        let want = ref_join(&candidate, &coalition, required, 0.0);
        assert!(
            want.act && want.score > 0.0 && want.score < 1e-15,
            "and the fresh reference must differ by float noise only (want {want:?})"
        );
    }

    // -----------------------------------------------------------------------
    // 9. Acceptance gate (koalisi #14): the evaluator-backed policy must produce
    // decisions identical (act strict, score 1e-9-relative) to the fresh
    // reference over a seeded scenario stream, for both leave variants — and the
    // ValueCalculator surface must be bit-identical to fresh magnitude.
    // -----------------------------------------------------------------------

    /// `SplitMix64` (inline; same shape as the K4 battery — no `rand` dep).
    struct SplitMix64(u64);
    impl SplitMix64 {
        fn new(seed: u64) -> Self {
            SplitMix64(seed.wrapping_add(0x9E37_79B9_7F4A_7C15))
        }
        fn next_u64(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }
    }

    const UNIVERSE_BITS: u64 = 8;

    #[allow(clippy::cast_possible_truncation)] // bounded RNG scaffolding (values < 8)
    fn draw_distinct_bits(rng: &mut SplitMix64, k: u64) -> u32 {
        let mut mask = 0u32;
        let mut count = 0u64;
        while count < k {
            let b = 1u32 << (rng.next_u64() % UNIVERSE_BITS) as u32;
            if mask & b == 0 {
                mask |= b;
                count += 1;
            }
        }
        mask
    }

    #[allow(clippy::cast_possible_truncation)] // bounded RNG scaffolding (index < n)
    fn fisher_yates(rng: &mut SplitMix64, n: usize) -> Vec<usize> {
        let mut order: Vec<usize> = (0..n).collect();
        for i in (1..n).rev() {
            let j = (rng.next_u64() % (i as u64 + 1)) as usize;
            order.swap(i, j);
        }
        order
    }

    struct SeedTask {
        required: u32,
        order: Vec<usize>,
    }

    /// A seeded instance: an agent pool and a task stream (same generative shape
    /// as the K4 battery, scaled down for a fast unit test).
    #[allow(clippy::cast_possible_truncation)] // bounded RNG scaffolding
    fn generate_instance(seed: u64) -> (Vec<TestAgent>, Vec<SeedTask>) {
        let mut rng = SplitMix64::new(seed);
        let n = (4 + rng.next_u64() % 8) as usize;
        let agents: Vec<TestAgent> = (0..n)
            .map(|id| {
                let k = 1 + rng.next_u64() % 4;
                let caps = draw_distinct_bits(&mut rng, k);
                let trust = (20 + rng.next_u64() % 80) as u32;
                TestAgent { id, caps, trust }
            })
            .collect();
        let tasks: Vec<SeedTask> = (0..8)
            .map(|_| {
                let r = 1 + rng.next_u64() % 5;
                let required = draw_distinct_bits(&mut rng, r);
                let order = fisher_yates(&mut rng, n);
                SeedTask { required, order }
            })
            .collect();
        (agents, tasks)
    }

    fn view<'a>(agents: &'a [TestAgent], members: &[usize]) -> Vec<&'a dyn AgentCapabilities> {
        members
            .iter()
            .map(|&i| &agents[i] as &dyn AgentCapabilities)
            .collect()
    }

    /// Drive one seed under `policy` (evaluator-backed) and assert every decision
    /// equals the fresh reference. `eval_leave` selects whether the leave sweep
    /// exercises the evaluator (variant B) or fresh (variant A) — either way the
    /// reference is fresh, so both must match.
    fn assert_stream_matches_reference(policy: &MagnitudePolicy, seed: u64) {
        let (agents, tasks) = generate_instance(seed);

        for task in &tasks {
            let required = task.required;
            let ctx = DecisionContext {
                required_capabilities: required,
            };

            // Bootstrap: first arrival joins unconditionally (battery protocol).
            let mut members: Vec<usize> = vec![task.order[0]];

            for &idx in &task.order[1..] {
                let candidate: &dyn AgentCapabilities = &agents[idx];
                let coalition = view(&agents, &members);
                let got = policy.should_join(candidate, &coalition, &ctx);
                let want = ref_join(candidate, &coalition, required, policy.join_margin);
                assert!(
                    decisions_match(got, want),
                    "seed {seed} join idx {idx}: got {got:?} want {want:?}"
                );
                if got.act {
                    members.push(idx);
                }
            }

            // One leave sweep in arrival order.
            for &idx in &task.order {
                let Some(pos) = members.iter().position(|&m| m == idx) else {
                    continue;
                };
                let coalition = view(&agents, &members);
                let agent: &dyn AgentCapabilities = &agents[idx];
                let got = policy.should_leave(agent, &coalition, &ctx);
                let want = ref_leave(agent, &coalition, required);
                assert!(
                    decisions_match(got, want),
                    "seed {seed} leave idx {idx}: got {got:?} want {want:?}"
                );
                if got.act {
                    members.remove(pos);
                }
            }

            // ValueCalculator surface: bit-identical to fresh magnitude. The
            // calculator keys on `required`, so build one per requirement (and
            // score the same coalition twice to also exercise its exact-hit path).
            let coalition = view(&agents, &members);
            let calc = MagnitudeValueCalculator::new(required);
            let cached = calc.calculate_value(&coalition);
            let cached_hit = calc.calculate_value(&coalition);
            let fresh = {
                let masks = relevant_masks(&coalition, required);
                magnitude_or_zero(&masks, required).unwrap_or(f64::NEG_INFINITY)
            };
            // Bit-for-bit equality (compare raw bits so a NaN sentinel would also
            // have to match, and to sidestep the float-cmp lint on an exact check).
            assert_eq!(
                cached.to_bits(),
                cached_hit.to_bits(),
                "calculator exact-hit must be stable"
            );
            assert_eq!(
                cached.to_bits(),
                fresh.to_bits(),
                "seed {seed}: MagnitudeValueCalculator must equal fresh bit-for-bit"
            );
        }
    }

    #[test]
    fn seeded_decisions_equal_fresh_reference_variant_a() {
        let policy = MagnitudePolicy::default(); // Variant A leave (fresh).
        for seed in 0..5u64 {
            assert_stream_matches_reference(&policy, seed);
        }
    }

    #[test]
    fn seeded_decisions_equal_fresh_reference_variant_b() {
        let policy = MagnitudePolicy::with_evaluator_leave(0.0); // Variant B leave.
        for seed in 0..5u64 {
            assert_stream_matches_reference(&policy, seed);
        }
    }

    // -----------------------------------------------------------------------
    // 10. EQ3 unit gates (koalisi #69, prereg Amendment 1): (a) proof-branch
    // equivalence + divergence shape on the EQ3 arm, (b) f64 fresh-route
    // anchor, (c) toggle identity, (d) default-path identity (L1 is invisible).
    // -----------------------------------------------------------------------

    /// The exact zero-diversity certificate for one candidate query, read off a
    /// FRESH cache through the EQ3 arm's reporting query. The proof is a pure
    /// function of `(required, member_masks, candidate)` — cache state selects
    /// only which evaluator answers — so probing on a private cache observes
    /// exactly what a toggle-ON dispatch sees.
    #[cfg(feature = "magnitude-fast")]
    fn probe_proof(
        required: u32,
        member_masks: &[u32],
        candidate: u32,
    ) -> Option<ZeroDiversityProof> {
        let cache = Mutex::new(None);
        cached_base_and_with_report(&cache, required, member_masks, candidate)
            .unwrap()
            .zero_proof
    }

    /// (a, fixture half) Proof-branch equivalence on the **EQ3 arm**. Each of
    /// upstream's three exactly-decidable classes is hit by a hand-built
    /// fixture (assert the class actually fires, so the gate cannot silently go
    /// vacuous), and the toggle-ON decision on that fixture must equal the
    /// pre-change fresh-recompute reference — `act` strictly, `score` within
    /// 1e-9 relative.
    ///
    /// On these fixtures the fresh route also lands on an exact `0.0`, so the
    /// two agree. That is **not** universal: where fresh lands on a
    /// noise-positive margin instead, the proof branch decides differently by
    /// construction — the rate and shape of that divergence is the separate
    /// [`proof_branch_divergence_from_fresh_is_float_noise_only`] gate, which
    /// pins H-par′ conjunct (i).
    #[cfg(feature = "magnitude-fast")]
    #[test]
    fn proof_branch_decisions_equal_fresh_reference() {
        let required = 0b111u32;
        let ctx = DecisionContext {
            required_capabilities: required,
        };
        let policy = MagnitudePolicy::default().with_eq3_levers(true);

        // SkeletalMerge: the candidate is a mutual-1.0 clone of member 1 and
        // opens no interior shortcut (member 0 is disjoint from both).
        let m0 = TestAgent {
            id: 0,
            caps: 0b001,
            trust: 50,
        };
        let m1 = TestAgent {
            id: 1,
            caps: 0b010,
            trust: 50,
        };
        let clone_of_m1 = TestAgent {
            id: 2,
            caps: 0b010,
            trust: 50,
        };
        assert!(
            matches!(
                probe_proof(required, &[0b001, 0b010], 0b010),
                Some(ZeroDiversityProof::SkeletalMerge { .. })
            ),
            "clone candidate must carry the skeletal-merge certificate"
        );

        // IncomingProfileDuplicate: the candidate subsumes the sole member
        // (A(member → candidate) = 1.0, A(candidate → member) = 0.5), so its
        // incoming closed profile replicates that member's column.
        let sole = TestAgent {
            id: 0,
            caps: 0b001,
            trust: 50,
        };
        let superset = TestAgent {
            id: 1,
            caps: 0b011,
            trust: 50,
        };
        assert!(
            matches!(
                probe_proof(required, &[0b001], 0b011),
                Some(ZeroDiversityProof::IncomingProfileDuplicate { .. })
            ),
            "subsuming candidate must carry the incoming-duplicate certificate"
        );

        // OutgoingProfileDuplicate: the transpose — the candidate is subsumed
        // BY the sole member, so its outgoing profile replicates that row.
        let subsumed = TestAgent {
            id: 1,
            caps: 0b001,
            trust: 50,
        };
        assert!(
            matches!(
                probe_proof(required, &[0b011], 0b001),
                Some(ZeroDiversityProof::OutgoingProfileDuplicate { .. })
            ),
            "subsumed candidate must carry the outgoing-duplicate certificate"
        );

        let merge_coalition: [&dyn AgentCapabilities; 2] = [&m0, &m1];
        let incoming_coalition: [&dyn AgentCapabilities; 1] = [&sole];
        let outgoing_coalition: [&dyn AgentCapabilities; 1] = [&superset];
        let cases: [(&dyn AgentCapabilities, &[&dyn AgentCapabilities]); 3] = [
            (&clone_of_m1, &merge_coalition),
            (&superset, &incoming_coalition),
            (&subsumed, &outgoing_coalition),
        ];

        for (candidate, coalition) in cases {
            let got = policy.should_join(candidate, coalition, &ctx);
            let want = ref_join(candidate, coalition, required, 0.0);
            assert_eq!(
                got.act, want.act,
                "proof-fired join must match fresh act (got {got:?}, want {want:?})"
            );
            assert!(
                decisions_match(got, want),
                "proof-fired join must match fresh score (got {got:?}, want {want:?})"
            );
            assert!(
                !got.act && got.score.abs() < EPS,
                "a proven-zero increment is a decline at margin 0 (got {got:?})"
            );
        }
    }

    /// (a, corpus half) The measured relationship between the **EQ3 arm's**
    /// proof branch and the frozen arm's fresh-recompute decision, over a
    /// 60-seed stream. This is the unit-level statement of **H-par′ conjunct
    /// (i)** (prereg Amendment 1 / A1.2): a divergence is admissible only with
    /// the certified shape.
    ///
    /// **The gate records a DECISION CHANGE, not an identity** — which is why
    /// A1.1 parks L2 behind the toggle. Measured on this corpus: 2832
    /// evaluator-branch join queries, **911 proof-certified** (32.2%; 465
    /// skeletal-merge / 147 incoming-dup / 299 outgoing-dup), of which **7
    /// (0.77%) decide differently from the frozen arm**. Every divergence has
    /// the same shape: the real increment is exactly `0` (certified), the frozen
    /// path's margin there is a strictly-positive float-noise value
    /// (2.2e-16 … 6.7e-16) that cleared the strict `margin > 0` test. All 7 are
    /// duplicate-profile proofs; the skeletal-merge class never diverges
    /// (0/465 — matching upstream #153's "merge-only candidates are
    /// bit-identical to fresh").
    ///
    /// What is asserted (and stays true if upstream numerics shift): the proof
    /// is arithmetically sound — every certified candidate's *frozen-arm* margin
    /// is zero to within float noise — the skeletal-merge class never flips, and
    /// every flip is a noise-positive frozen margin, never a genuine one.
    #[cfg(feature = "magnitude-fast")]
    #[test]
    fn proof_branch_divergence_from_fresh_is_float_noise_only() {
        /// Margins this small are float noise around an exact zero, not
        /// diversity: the smallest genuine magnitude margin on the discrete
        /// capability-mask domain is many orders above it (same reasoning as
        /// [`KNIFE_EDGE_REL_BAND`], which is `1e-6`).
        const NOISE: f64 = 1e-15;

        let mut proofs = 0usize;
        let mut flips = 0usize;

        for seed in 0..60u64 {
            let (agents, tasks) = generate_instance(seed);
            // The two registered arms, scored on identical coalition states:
            // `mag` (frozen, L1 only) and `mag-eq3` (toggle ON = L2 + L3).
            let frozen = MagnitudePolicy::default();
            let eq3 = MagnitudePolicy::default().with_eq3_levers(true);
            for task in &tasks {
                let required = task.required;
                let ctx = DecisionContext {
                    required_capabilities: required,
                };
                let mut members: Vec<usize> = vec![task.order[0]];
                for &idx in &task.order[1..] {
                    let candidate: &dyn AgentCapabilities = &agents[idx];
                    let coalition = view(&agents, &members);
                    let (mw, mo) = MagnitudePolicy::join_masks(candidate, &coalition, required);
                    let want = frozen.should_join(candidate, &coalition, &ctx);
                    let got = eq3.should_join(candidate, &coalition, &ctx);

                    if !mo.is_empty()
                        && mw.len() != mo.len()
                        && let Some(proof) = probe_proof(required, &mo, candidate.capabilities())
                    {
                        proofs += 1;
                        assert!(
                            want.score.abs() <= NOISE,
                            "seed {seed} idx {idx}: {proof:?} certifies a zero increment, but \
                             the frozen arm's margin is {} — the certificate would be unsound",
                            want.score
                        );
                        assert!(
                            got.score.abs() < f64::EPSILON,
                            "seed {seed} idx {idx}: a certified candidate must score an \
                             exact-zero margin, got {got:?}"
                        );
                        if got.act != want.act {
                            flips += 1;
                            assert!(
                                !matches!(proof, ZeroDiversityProof::SkeletalMerge { .. }),
                                "seed {seed} idx {idx}: the skeletal-merge class is supposed to \
                                 be bit-identical to fresh, but it flipped ({want:?})"
                            );
                            assert!(
                                want.act && want.score > 0.0,
                                "seed {seed} idx {idx}: the only admissible divergence is a \
                                 noise-positive frozen margin, got {want:?}"
                            );
                        }
                    } else {
                        // H-par′ (i): outside the certified population the two
                        // arms must agree — a bare L3 act change would be a
                        // parity failure, not a characterized divergence.
                        assert_eq!(
                            got.act, want.act,
                            "seed {seed} idx {idx}: uncertified act divergence \
                             (got {got:?}, want {want:?})"
                        );
                    }

                    // Advance on the FROZEN arm so both are always scored on the
                    // same coalition state (membership cascade is out of scope
                    // for the shape check — A1.2 counts it separately).
                    if want.act {
                        members.push(idx);
                    }
                }
            }
        }

        assert!(proofs > 0, "the corpus must exercise the proof branch");
        // The count itself is the finding, not a threshold — the per-flip
        // asserts above are what constrain its shape (noise-positive fresh
        // margin, never the skeletal-merge class).
        assert!(
            flips <= proofs,
            "sanity: {flips} flips out of {proofs} certified candidates"
        );
    }

    /// The **pre-EQ3** join dispatch, transcribed: the same three branches
    /// [`MagnitudePolicy::join_dispatch`] has, but querying
    /// [`CoalitionEvaluator::value_with`] — no scratch, no report — on an
    /// evaluator built for this one candidate, and recomputing knife-edges with
    /// [`magnitude_of_masks`]. The reference for gate (d).
    fn pre_eq3_join(
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        required: u32,
        join_margin: f64,
    ) -> Decision {
        let (masks_with, masks_without) = MagnitudePolicy::join_masks(agent, coalition, required);
        if masks_without.is_empty() {
            return MagnitudePolicy::join_decision_from_values(
                magnitude_or_zero(&masks_with, required),
                magnitude_or_zero(&masks_without, required),
                join_margin,
            );
        }
        if masks_with.len() == masks_without.len() {
            return match build_evaluator(required, &masks_without, &[]) {
                Ok(ev) => {
                    let base = ev.base_value();
                    MagnitudePolicy::join_decision_from_values(Ok(base), Ok(base), join_margin)
                }
                Err(_) => Decision {
                    act: false,
                    score: 0.0,
                },
            };
        }
        match build_evaluator(required, &masks_without, &[agent.capabilities()]) {
            Ok(ev) => {
                let base = ev.base_value();
                let with = match ev.value_with(masks_without.len()) {
                    Ok(w) if is_knife_edge(w, base, join_margin) => {
                        magnitude_of_masks(&masks_with, required)
                    }
                    other => other,
                };
                MagnitudePolicy::join_decision_from_values(with, Ok(base), join_margin)
            }
            Err(_) => Decision {
                act: false,
                score: 0.0,
            },
        }
    }

    /// The **pre-EQ3** evaluator-backed leave dispatch (variant B), transcribed
    /// from [`MagnitudePolicy::leave_dispatch_evaluator`] the same way.
    fn pre_eq3_leave_evaluator(
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        required: u32,
    ) -> Decision {
        let (masks_in, masks_out) = MagnitudePolicy::leave_masks(agent, coalition, required);
        if masks_out.is_empty() {
            return MagnitudePolicy::leave_decision_from_values(
                magnitude_or_zero(&masks_in, required),
                magnitude_or_zero(&masks_out, required),
            );
        }
        if masks_in.len() == masks_out.len() {
            return match build_evaluator(required, &masks_out, &[]) {
                Ok(ev) => {
                    let base = ev.base_value();
                    MagnitudePolicy::leave_decision_from_values(Ok(base), Ok(base))
                }
                Err(_) => Decision {
                    act: false,
                    score: 0.0,
                },
            };
        }
        match build_evaluator(required, &masks_out, &[agent.capabilities()]) {
            Ok(ev) => {
                let base = ev.base_value();
                let with = match ev.value_with(masks_out.len()) {
                    Ok(w) if is_knife_edge(w, base, 0.0) => magnitude_of_masks(&masks_in, required),
                    other => other,
                };
                MagnitudePolicy::leave_decision_from_values(with, Ok(base))
            }
            Err(_) => Decision {
                act: false,
                score: 0.0,
            },
        }
    }

    /// (d) Default-path identity: **L1 is invisible**. The shipped default
    /// policy must reproduce the pre-EQ3 decision stream **bit-exactly** — acts
    /// and raw score bits — over the 60-seed corpus, for both leave variants.
    ///
    /// The reference ([`pre_eq3_join`] / [`pre_eq3_leave_evaluator`]) queries
    /// `value_with`, i.e. the arithmetic route the policy used before the
    /// scratch adoption; the policy under test queries `value_with_scratch`.
    /// Upstream #33 promises those are bit-identical, and this is koalisi's own
    /// check of that promise at the decision surface, which is what X-A rests
    /// on.
    #[test]
    fn default_path_reproduces_pre_eq3_stream_bit_exactly() {
        for seed in 0..60u64 {
            let (agents, tasks) = generate_instance(seed);
            let variant_a = MagnitudePolicy::default();
            let variant_b = MagnitudePolicy::with_evaluator_leave(0.0);

            for task in &tasks {
                let required = task.required;
                let ctx = DecisionContext {
                    required_capabilities: required,
                };

                for (policy, eval_leave, label) in [
                    (&variant_a, false, "variant A"),
                    (&variant_b, true, "variant B"),
                ] {
                    let mut members: Vec<usize> = vec![task.order[0]];

                    for &idx in &task.order[1..] {
                        let candidate: &dyn AgentCapabilities = &agents[idx];
                        let coalition = view(&agents, &members);
                        let got = policy.should_join(candidate, &coalition, &ctx);
                        let want = pre_eq3_join(candidate, &coalition, required, 0.0);
                        assert_eq!(
                            got.act, want.act,
                            "{label} seed {seed} join {idx}: act differs ({got:?} vs {want:?})"
                        );
                        assert_eq!(
                            got.score.to_bits(),
                            want.score.to_bits(),
                            "{label} seed {seed} join {idx}: score not bit-identical \
                             ({got:?} vs {want:?})"
                        );
                        if got.act {
                            members.push(idx);
                        }
                    }

                    for &idx in &task.order {
                        let Some(pos) = members.iter().position(|&m| m == idx) else {
                            continue;
                        };
                        let coalition = view(&agents, &members);
                        let agent: &dyn AgentCapabilities = &agents[idx];
                        let got = policy.should_leave(agent, &coalition, &ctx);
                        let want = if eval_leave {
                            pre_eq3_leave_evaluator(agent, &coalition, required)
                        } else {
                            ref_leave(agent, &coalition, required)
                        };
                        assert_eq!(
                            got.act, want.act,
                            "{label} seed {seed} leave {idx}: act differs ({got:?} vs {want:?})"
                        );
                        assert_eq!(
                            got.score.to_bits(),
                            want.score.to_bits(),
                            "{label} seed {seed} leave {idx}: score not bit-identical \
                             ({got:?} vs {want:?})"
                        );
                        if got.act {
                            members.remove(pos);
                        }
                    }
                }
            }
        }
    }

    /// (b) f64 fresh-route anchor: `magnitude_of_masks_f64` must reproduce
    /// `magnitude_of_masks` within 1e-9 relative on the K2 / gotcha fixtures —
    /// including the dedup, irrelevant-agent-exclusion and clone-skeletalization
    /// shapes — and on the seeded instance pools.
    #[cfg(feature = "magnitude-fast")]
    #[test]
    fn f64_fresh_route_matches_generic_within_tolerance() {
        fn assert_agrees(masks: &[u32], required: u32, label: &str) {
            let generic = magnitude_of_masks(masks, required);
            let fast = magnitude_of_masks_f64(masks, required);
            match (generic, fast) {
                (Ok(g), Ok(f)) => assert!(
                    rel_close(g, f),
                    "{label}: f64 route {f} vs generic {g} (masks {masks:?}, required {required:#b})"
                ),
                (Err(_), Err(_)) => {}
                (g, f) => panic!("{label}: Ok/Err disagreement — generic {g:?}, f64 {f:?}"),
            }
        }

        // Hand-checked K2 fixtures (the values pinned elsewhere in this module).
        assert_agrees(&[0b001], 0b111, "singleton");
        assert_agrees(&[0b001, 0b010, 0b100], 0b111, "disjoint specialists");
        assert_agrees(&[0b001, 0b001], 0b111, "clone pair (skeletalization)");
        assert_agrees(
            &[0b010, 0b010, 0b010, 0b001],
            0b111,
            "three clones + specialist",
        );
        assert_agrees(&[0b011, 0b110], 0b111, "half overlap");
        assert_agrees(&[0b001, 0b011], 0b111, "subsumption");
        assert_agrees(&[0b0011, 0b0110, 0b1100], 0b1111, "chain couplings");
        assert_agrees(&[97, 85, 198], 0b1110_0100, "knife-edge fixture");

        // Dedup + irrelevant exclusion happen in `relevant_masks`, so drive the
        // fixture through it and score whatever survives (gotcha 12 / point 4).
        let a0 = TestAgent {
            id: 0,
            caps: 0b001,
            trust: 50,
        };
        let a1 = TestAgent {
            id: 1,
            caps: 0b010,
            trust: 50,
        };
        let bystander = TestAgent {
            id: 3,
            caps: 0b1000,
            trust: 50,
        };
        let listed: [&dyn AgentCapabilities; 4] = [&a0, &a0, &a1, &bystander];
        let masks = relevant_masks(&listed, 0b111);
        assert_eq!(masks, vec![0b001, 0b010], "dedup + exclusion precondition");
        assert_agrees(&masks, 0b111, "deduped, bystander-excluded");

        // Seeded pools: whole-pool magnitudes per task requirement.
        for seed in 0..5u64 {
            let (agents, tasks) = generate_instance(seed);
            for task in &tasks {
                let pool: Vec<&dyn AgentCapabilities> =
                    agents.iter().map(|a| a as &dyn AgentCapabilities).collect();
                let masks = relevant_masks(&pool, task.required);
                if masks.is_empty() {
                    continue;
                }
                assert_agrees(&masks, task.required, "seeded pool");
            }
        }
    }

    /// Every decision one seeded instance produces under `policy`, in order
    /// (joins in arrival order, then one leave sweep) — the fixture battery the
    /// toggle-identity gate compares bit-for-bit.
    #[cfg(feature = "magnitude-fast")]
    fn decision_stream(policy: &MagnitudePolicy, seed: u64) -> Vec<Decision> {
        let (agents, tasks) = generate_instance(seed);
        let mut out = Vec::new();

        for task in &tasks {
            let required = task.required;
            let ctx = DecisionContext {
                required_capabilities: required,
            };
            let mut members: Vec<usize> = vec![task.order[0]];

            for &idx in &task.order[1..] {
                let candidate: &dyn AgentCapabilities = &agents[idx];
                let coalition = view(&agents, &members);
                let d = policy.should_join(candidate, &coalition, &ctx);
                out.push(d);
                if d.act {
                    members.push(idx);
                }
            }

            for &idx in &task.order {
                let Some(pos) = members.iter().position(|&m| m == idx) else {
                    continue;
                };
                let coalition = view(&agents, &members);
                let agent: &dyn AgentCapabilities = &agents[idx];
                let d = policy.should_leave(agent, &coalition, &ctx);
                out.push(d);
                if d.act {
                    members.remove(pos);
                }
            }
        }

        out
    }

    /// L3 in isolation, at the decision surface: on every **uncertified**
    /// candidate — where L2 cannot act, so any difference is the `f64`
    /// factorization alone — the EQ3 arm must agree with the frozen arm on
    /// `act` and to within 1e-9 relative on `score` (the K6 parity tolerance).
    ///
    /// Context measured while building this: before A1.1 moved L2 behind the
    /// toggle, an L3-only arm produced **0 act differences over 3970 decisions**
    /// on the 60-seed corpus, with 109 decisions differing in the last `f64`
    /// ulps of their score (max 6.7e-16 relative). So the factorization change
    /// is visible in the score column and, on this corpus, in no act. This is
    /// *not* the registered H-par′ gate — that one runs the Part 7 battery on
    /// seeds 210..240.
    #[cfg(feature = "magnitude-fast")]
    #[test]
    fn f64_route_alone_preserves_uncertified_decisions() {
        let frozen = MagnitudePolicy::default();
        let eq3 = MagnitudePolicy::default().with_eq3_levers(true);

        for seed in 0..20u64 {
            let (agents, tasks) = generate_instance(seed);
            for task in &tasks {
                let required = task.required;
                let ctx = DecisionContext {
                    required_capabilities: required,
                };
                let mut members: Vec<usize> = vec![task.order[0]];

                for &idx in &task.order[1..] {
                    let candidate: &dyn AgentCapabilities = &agents[idx];
                    let coalition = view(&agents, &members);
                    let (mw, mo) = MagnitudePolicy::join_masks(candidate, &coalition, required);
                    let certified = !mo.is_empty()
                        && mw.len() != mo.len()
                        && probe_proof(required, &mo, candidate.capabilities()).is_some();

                    let want = frozen.should_join(candidate, &coalition, &ctx);
                    let got = eq3.should_join(candidate, &coalition, &ctx);
                    if !certified {
                        assert_eq!(
                            got.act, want.act,
                            "seed {seed} idx {idx}: the f64 route changed an uncertified act \
                             ({got:?} vs {want:?})"
                        );
                        assert!(
                            rel_close(got.score, want.score),
                            "seed {seed} idx {idx}: uncertified score drift beyond 1e-9 relative \
                             ({got:?} vs {want:?})"
                        );
                    }

                    // Both arms advance on the frozen decision so they keep
                    // scoring identical states.
                    if want.act {
                        members.push(idx);
                    }
                }
            }
        }
    }

    /// The EQ3 instrumentation surface: `probe_join` reports the incremental
    /// pair and certificate for exactly the decisions that reach the evaluator
    /// (and `None` for the empty / excluded branches), without touching the
    /// policy's own cache; `probe_fresh_factorization`'s single-factorization
    /// shortcut at `t = 1` returns the same magnitude as the fresh `f64` route.
    #[cfg(feature = "magnitude-fast")]
    #[test]
    fn instrumentation_probes_report_without_changing_state() {
        let ctx = DecisionContext {
            required_capabilities: 0b111,
        };
        let a0 = TestAgent {
            id: 0,
            caps: 0b001,
            trust: 50,
        };
        let a1 = TestAgent {
            id: 1,
            caps: 0b010,
            trust: 50,
        };
        let cand = TestAgent {
            id: 2,
            caps: 0b100,
            trust: 50,
        };
        let bystander = TestAgent {
            id: 3,
            caps: 0b1000,
            trust: 50,
        };
        let policy = MagnitudePolicy::default().with_eq3_levers(true);
        let coalition: [&dyn AgentCapabilities; 2] = [&a0, &a1];

        // Empty coalition and excluded candidate: no incremental branch.
        assert!(policy.probe_join(&cand, &[], &ctx).is_none());
        assert!(policy.probe_join(&bystander, &coalition, &ctx).is_none());

        // A disjoint specialist: increment 1.0, no certificate, not knife-edge.
        let probe = policy
            .probe_join(&cand, &coalition, &ctx)
            .expect("the normal branch reports");
        assert!(rel_close(probe.base, 2.0) && rel_close(probe.with, 3.0));
        assert!(probe.zero_proof.is_none() && !probe.knife_edge);

        // A clone of a member: certified zero, and the band would have fired.
        let clone_of_a1 = TestAgent {
            id: 4,
            caps: 0b010,
            trust: 50,
        };
        let probe = policy
            .probe_join(&clone_of_a1, &coalition, &ctx)
            .expect("the normal branch reports");
        assert!(
            matches!(
                probe.zero_proof,
                Some(ZeroDiversityProof::SkeletalMerge { .. })
            ) && probe.knife_edge
        );

        // Probing leaves the policy's own decisions untouched (it answers off a
        // private cache), so a decision taken after probing still matches the
        // fresh reference.
        let after = policy.should_join(&cand, &coalition, &ctx);
        assert!(decisions_match(after, ref_join(&cand, &coalition, 0b111, 0.0)));

        // The factorization shortcut agrees with the fresh f64 route, and skips
        // task-irrelevant agents through `relevant_masks`.
        let listed: [&dyn AgentCapabilities; 3] = [&a0, &a1, &bystander];
        let (_path, mag) =
            probe_fresh_factorization(&listed, 0b111).expect("two relevant members remain");
        let direct = magnitude_of_masks_f64(&[0b001, 0b010], 0b111).unwrap();
        assert_eq!(
            mag.unwrap().to_bits(),
            direct.to_bits(),
            "the t = 1 single-factorization shortcut must equal the fresh f64 route"
        );
        let none: [&dyn AgentCapabilities; 1] = [&bystander];
        assert!(probe_fresh_factorization(&none, 0b111).is_none());
    }

    /// The Gauss–Jordan class — the route MOST koalisi traffic actually takes.
    ///
    /// `A(i → j) = |rel_i ∩ rel_j| / |rel_i|` is asymmetric as soon as the two
    /// agents have different relevant widths, so ζ is not exactly symmetric and
    /// upstream's `ZetaFactorization` falls back to the rig-generic route. This
    /// pins (a) that the probe reports that fallback rather than a symmetric
    /// route, and (b) that the magnitude it reports is bit-identical to what the
    /// arm's own fresh evaluation computes — so the instrumentation describes
    /// the arm, not a parallel computation.
    #[cfg(feature = "magnitude-fast")]
    #[test]
    fn gauss_jordan_fixture_probe_matches_the_arms_fresh_eval() {
        use catgraph_magnitude::magnitude_f64::FactorizationPath;

        // |rel| 1 vs 2 ⇒ A(0→1) = 1.0 but A(1→0) = 0.5 ⇒ ζ asymmetric.
        let narrow = TestAgent {
            id: 0,
            caps: 0b001,
            trust: 50,
        };
        let wide = TestAgent {
            id: 1,
            caps: 0b011,
            trust: 50,
        };
        let listed: [&dyn AgentCapabilities; 2] = [&narrow, &wide];
        let required = 0b111u32;

        let (path, mag) =
            probe_fresh_factorization(&listed, required).expect("both members are relevant");
        assert_eq!(
            path,
            FactorizationPath::GaussJordan,
            "an asymmetric ζ must take the generic fallback route"
        );

        let masks = relevant_masks(&listed, required);
        assert_eq!(masks, vec![0b001, 0b011]);
        let arm_fresh = magnitude_of_masks_f64(&masks, required).unwrap();
        assert_eq!(
            mag.unwrap().to_bits(),
            arm_fresh.to_bits(),
            "the probe must report the magnitude the arm's own fresh eval produces"
        );

        // And on this class the generic route IS the computation, so it also
        // matches the rig-generic path exactly.
        let generic = magnitude_of_masks(&masks, required).unwrap();
        assert!(
            rel_close(arm_fresh, generic),
            "Gauss–Jordan route: f64 {arm_fresh} vs generic {generic}"
        );
    }

    /// (c) Toggle identity: with the EQ3 levers explicitly OFF the policy is
    /// bit-identical to the plain default (which is what a `magnitude`-only
    /// build compiles), for both leave variants. This is the unit-level
    /// counterpart of the registered X-B gate.
    #[cfg(feature = "magnitude-fast")]
    #[test]
    fn eq3_levers_toggle_off_is_bit_identical_to_default() {
        let pairs = [
            (
                MagnitudePolicy::default(),
                MagnitudePolicy::default().with_eq3_levers(false),
                "variant A",
            ),
            (
                MagnitudePolicy::with_evaluator_leave(0.0),
                MagnitudePolicy::with_evaluator_leave(0.0).with_eq3_levers(false),
                "variant B",
            ),
        ];

        for (base, toggled, label) in pairs {
            for seed in 0..5u64 {
                let want = decision_stream(&base, seed);
                let got = decision_stream(&toggled, seed);
                assert_eq!(
                    want.len(),
                    got.len(),
                    "{label} seed {seed}: decision counts differ"
                );
                for (i, (w, g)) in want.iter().zip(got.iter()).enumerate() {
                    assert_eq!(
                        w.act, g.act,
                        "{label} seed {seed} decision {i}: act differs"
                    );
                    assert_eq!(
                        w.score.to_bits(),
                        g.score.to_bits(),
                        "{label} seed {seed} decision {i}: score not bit-identical ({w:?} vs {g:?})"
                    );
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // 11. EQ4 typed-roles gates (koalisi #72, prereg §4): the identity default,
    // ρ ≡ 1 agreement with the untyped arm, δ-ρ cross-role decoupling,
    // exclusion-before-modulation, and the decline-never-panic surface.
    // -----------------------------------------------------------------------

    /// A `ρ` of all-`1.0` entries over `n` roles — the exact identity of the
    /// modulation (`1.0 * p == p` in IEEE, and upstream's validator returns `p`
    /// unchanged).
    fn rho_ones(n: usize) -> RoleModulation {
        RoleModulation::new(vec![vec![1.0; n]; n]).unwrap()
    }

    /// The registered oracle table `ρ = δ`: same-role substitutability
    /// unchanged, cross-role substitutability exactly `0`.
    fn rho_identity(n: usize) -> RoleModulation {
        let rows: Vec<Vec<f64>> = (0..n)
            .map(|i| {
                (0..n)
                    .map(|j| if i == j { 1.0 } else { 0.0 })
                    .collect::<Vec<f64>>()
            })
            .collect();
        RoleModulation::new(rows).unwrap()
    }

    /// Round-robin role assignment over agent ids `0..n`.
    fn round_robin_roles(n: usize, n_roles: usize) -> HashMap<usize, RoleId> {
        (0..n).map(|id| (id, id % n_roles)).collect()
    }

    /// Every join/leave decision one policy makes over a seeded stream, in
    /// order; membership advances on the policy's own decisions.
    fn eq4_decision_stream(policy: &MagnitudePolicy, seed: u64) -> Vec<Decision> {
        let (agents, tasks) = generate_instance(seed);
        let mut out = Vec::new();

        for task in &tasks {
            let ctx = DecisionContext {
                required_capabilities: task.required,
            };
            let mut members: Vec<usize> = vec![task.order[0]];

            for &idx in &task.order[1..] {
                let candidate: &dyn AgentCapabilities = &agents[idx];
                let coalition = view(&agents, &members);
                let d = policy.should_join(candidate, &coalition, &ctx);
                out.push(d);
                if d.act {
                    members.push(idx);
                }
            }

            for &idx in &task.order {
                let Some(pos) = members.iter().position(|&m| m == idx) else {
                    continue;
                };
                let coalition = view(&agents, &members);
                let agent: &dyn AgentCapabilities = &agents[idx];
                let d = policy.should_leave(agent, &coalition, &ctx);
                out.push(d);
                if d.act {
                    members.remove(pos);
                }
            }
        }
        out
    }

    /// Drive `typed` alongside the untyped `reference` over a seeded stream,
    /// asserting act-equality and 1e-9-relative score agreement at every
    /// decision. Membership advances on the REFERENCE decision, so both arms
    /// always score identical states even if one had diverged.
    fn assert_typed_tracks_untyped(
        typed: &MagnitudePolicy,
        reference: &MagnitudePolicy,
        seed: u64,
    ) {
        let (agents, tasks) = generate_instance(seed);

        for task in &tasks {
            let required = task.required;
            let ctx = DecisionContext {
                required_capabilities: required,
            };
            let mut members: Vec<usize> = vec![task.order[0]];

            for &idx in &task.order[1..] {
                let candidate: &dyn AgentCapabilities = &agents[idx];
                let coalition = view(&agents, &members);
                let want = reference.should_join(candidate, &coalition, &ctx);
                let got = typed.should_join(candidate, &coalition, &ctx);
                assert!(
                    decisions_match(got, want),
                    "seed {seed} typed join idx {idx}: got {got:?} want {want:?}"
                );
                if want.act {
                    members.push(idx);
                }
            }

            for &idx in &task.order {
                let Some(pos) = members.iter().position(|&m| m == idx) else {
                    continue;
                };
                let coalition = view(&agents, &members);
                let agent: &dyn AgentCapabilities = &agents[idx];
                let want = reference.should_leave(agent, &coalition, &ctx);
                let got = typed.should_leave(agent, &coalition, &ctx);
                assert!(
                    decisions_match(got, want),
                    "seed {seed} typed leave idx {idx}: got {got:?} want {want:?}"
                );
                if want.act {
                    members.remove(pos);
                }
            }
        }
    }

    /// X-identity cell 1: no typed configuration ⇒ the untyped path, structurally.
    /// The policy still reproduces the fresh untyped reference over the seeded
    /// stream (cache path included), and is decision-for-decision **bit**-identical
    /// to a plain policy — the typed field adds a branch, never an arithmetic step.
    #[test]
    fn typed_identity_default_is_the_untyped_path() {
        for seed in 0..5u64 {
            assert_stream_matches_reference(&MagnitudePolicy::new(0.0), seed);
        }

        let plain = MagnitudePolicy::default();
        let no_roles = MagnitudePolicy::new(0.0);
        for seed in 0..5u64 {
            let want = eq4_decision_stream(&plain, seed);
            let got = eq4_decision_stream(&no_roles, seed);
            assert_eq!(want.len(), got.len(), "seed {seed}: decision counts differ");
            for (i, (w, g)) in want.iter().zip(got.iter()).enumerate() {
                assert_eq!(w.act, g.act, "seed {seed} decision {i}: act differs");
                assert_eq!(
                    w.score.to_bits(),
                    g.score.to_bits(),
                    "seed {seed} decision {i}: score not bit-identical ({w:?} vs {g:?})"
                );
            }
        }
    }

    /// X-identity cell 2: the typed path at `ρ ≡ 1` reproduces the untyped
    /// arm's **acts** over the seeded stream (scores within 1e-9 relative — the
    /// untyped arm answers joins incrementally, the typed one always fresh).
    /// On the value surface, where the untyped arm is itself bit-identical to
    /// fresh, the agreement is bit-exact.
    #[test]
    fn typed_rho_ones_agrees_with_the_untyped_arm() {
        for seed in 0..5u64 {
            let (agents, tasks) = generate_instance(seed);
            let roles = round_robin_roles(agents.len(), 3);
            let typed = MagnitudePolicy::new(0.0).with_role_modulation(roles.clone(), rho_ones(3));
            assert_typed_tracks_untyped(&typed, &MagnitudePolicy::default(), seed);

            // Value surface: `1.0 · p == p`, so the modulated coupling list is
            // the untyped one bit-for-bit and so is the magnitude.
            for task in &tasks {
                let coalition = view(&agents, &task.order);
                let untyped = MagnitudeValueCalculator::new(task.required);
                let typed_calc = MagnitudeValueCalculator::new(task.required)
                    .with_role_modulation(roles.clone(), rho_ones(3));
                assert_eq!(
                    untyped.calculate_value(&coalition).to_bits(),
                    typed_calc.calculate_value(&coalition).to_bits(),
                    "seed {seed}: ρ ≡ 1 must not move the value"
                );
            }
        }
    }

    /// The oracle table `ρ = δ` decouples cross-role clones: two agents with
    /// IDENTICAL relevant masks are mutually coupled at `1.0` untyped (⇒
    /// skeletalized into one effective agent ⇒ margin `0` ⇒ decline), but at
    /// `ρ(0, 1) = 0` the coupling vanishes and they count as two — a positive
    /// margin. Same-role clones still merge. This is the arm's whole mechanism,
    /// in one fixture.
    #[test]
    fn identity_rho_decouples_cross_role_clones() {
        let ctx = DecisionContext {
            required_capabilities: 0b111,
        };
        let member = TestAgent {
            id: 0,
            caps: 0b001,
            trust: 50,
        };
        let clone = TestAgent {
            id: 1,
            caps: 0b001,
            trust: 50,
        };
        let coalition: [&dyn AgentCapabilities; 1] = [&member];

        // Untyped control: the clone is redundant and declines.
        let untyped = MagnitudePolicy::default().should_join(&clone, &coalition, &ctx);
        assert!(!untyped.act, "untyped: a clone must not join");
        assert!(untyped.score.abs() < EPS);

        // Cross-role (member role 0, clone role 1) under ρ = δ: the coupling is
        // zeroed both ways ⇒ Mag 1.0 → 2.0 ⇒ join with margin ≈ 1.0.
        let cross: HashMap<usize, RoleId> = [(0usize, 0usize), (1usize, 1usize)].into();
        let typed_cross = MagnitudePolicy::new(0.0).with_role_modulation(cross, rho_identity(2));
        let d = typed_cross.should_join(&clone, &coalition, &ctx);
        assert!(
            d.act,
            "cross-role clone must join under ρ = δ (score={})",
            d.score
        );
        assert!(
            (d.score - 1.0).abs() < EPS,
            "cross-role join margin = {}, expected 1.0",
            d.score
        );

        // Same-role: ρ(0, 0) = 1 leaves the mutual-1.0 coupling intact ⇒ the
        // pair still skeletalizes ⇒ decline, margin ≈ 0.
        let same: HashMap<usize, RoleId> = [(0usize, 0usize), (1usize, 0usize)].into();
        let typed_same = MagnitudePolicy::new(0.0).with_role_modulation(same, rho_identity(2));
        let d = typed_same.should_join(&clone, &coalition, &ctx);
        assert!(!d.act, "same-role clone must not join (score={})", d.score);
        assert!(d.score.abs() < EPS);

        // The leave dual: with the cross-role pair seated, neither is redundant.
        let seated: [&dyn AgentCapabilities; 2] = [&member, &clone];
        let cross: HashMap<usize, RoleId> = [(0usize, 0usize), (1usize, 1usize)].into();
        let typed_cross = MagnitudePolicy::new(0.0).with_role_modulation(cross, rho_identity(2));
        let stay = typed_cross.should_leave(&clone, &seated, &ctx);
        assert!(!stay.act, "cross-role clone contributes ⇒ stays");
        assert!((stay.score - 1.0).abs() < EPS);
        // ... while under ρ ≡ 1 it is redundant again and leaves.
        let same_role_all: HashMap<usize, RoleId> = [(0usize, 0usize), (1usize, 1usize)].into();
        let typed_ones = MagnitudePolicy::new(0.0).with_role_modulation(same_role_all, rho_ones(2));
        let leave = typed_ones.should_leave(&clone, &seated, &ctx);
        assert!(leave.act, "ρ ≡ 1 ⇒ the clone is redundant and leaves");
        assert!(leave.score.abs() < EPS);
    }

    /// Contract point 3: task-irrelevant agents are excluded BEFORE any role is
    /// read, so `ρ` can never resurrect a bystander — and the bystander does not
    /// even need a role assignment. Mirrors the untyped
    /// `irrelevant_bystander_*` regression.
    #[test]
    fn typed_irrelevant_bystander_is_excluded_before_modulation() {
        let ctx = DecisionContext {
            required_capabilities: 0b111,
        };
        let a0 = TestAgent {
            id: 0,
            caps: 0b001,
            trust: 50,
        };
        let a1 = TestAgent {
            id: 1,
            caps: 0b010,
            trust: 50,
        };
        let a2 = TestAgent {
            id: 2,
            caps: 0b100,
            trust: 50,
        };
        // Every capability bit lies outside `required` — and it has NO role
        // entry, which must not matter.
        let bystander = TestAgent {
            id: 3,
            caps: 0b1000,
            trust: 50,
        };
        let roles: HashMap<usize, RoleId> =
            [(0usize, 0usize), (1usize, 1usize), (2usize, 2usize)].into();
        let policy = MagnitudePolicy::new(0.0).with_role_modulation(roles.clone(), rho_identity(3));
        let calc =
            MagnitudeValueCalculator::new(0b111).with_role_modulation(roles, rho_identity(3));

        let specialists: [&dyn AgentCapabilities; 3] = [&a0, &a1, &a2];
        let with_bystander: [&dyn AgentCapabilities; 4] = [&a0, &a1, &a2, &bystander];

        // Value is unmoved by the bystander (and the missing role is never hit).
        assert!((calc.calculate_value(&with_bystander) - 3.0).abs() < EPS);
        assert!((calc.calculate_value(&specialists) - 3.0).abs() < EPS);

        // Join: nothing task-relevant ⇒ margin exactly 0 ⇒ decline (NOT a
        // missing-role decline — the score would be 0.0 either way, but the
        // value assertion above pins that the agent never reached the lookup).
        let join = policy.should_join(&bystander, &specialists, &ctx);
        assert!(!join.act && join.score.abs() < EPS);

        // Leave: the bystander is redundant; a unique specialist still stays.
        let leave = policy.should_leave(&bystander, &with_bystander, &ctx);
        assert!(leave.act && leave.score.abs() < EPS);
        let stay = policy.should_leave(&a0, &with_bystander, &ctx);
        assert!(
            !stay.act,
            "unique specialist must stay (score={})",
            stay.score
        );
        assert!((stay.score - 1.0).abs() < EPS);

        // `required == 0` ⇒ every agent irrelevant ⇒ value 0.0, everyone leaves,
        // and again no role is consulted.
        let ctx0 = DecisionContext {
            required_capabilities: 0,
        };
        let calc0 =
            MagnitudeValueCalculator::new(0).with_role_modulation(HashMap::new(), rho_identity(3));
        assert!(calc0.calculate_value(&specialists).abs() < EPS);
        let policy0 =
            MagnitudePolicy::new(0.0).with_role_modulation(HashMap::new(), rho_identity(3));
        let leave0 = policy0.should_leave(&a0, &specialists, &ctx0);
        assert!(leave0.act && leave0.score.abs() < EPS);
    }

    /// Contract point 4: an unusable typed configuration declines — it never
    /// panics. Two ways to be unusable: a participating agent with no role
    /// assignment, and a role outside `ρ`'s range (an upstream `modulate`
    /// error). The value surface reports both as `-∞`, its cannot-evaluate
    /// convention.
    #[test]
    fn typed_missing_or_out_of_range_role_declines_without_panic() {
        let ctx = DecisionContext {
            required_capabilities: 0b111,
        };
        let member = TestAgent {
            id: 0,
            caps: 0b001,
            trust: 50,
        };
        let candidate = TestAgent {
            id: 1,
            caps: 0b010,
            trust: 50,
        };
        let coalition: [&dyn AgentCapabilities; 1] = [&member];
        let seated: [&dyn AgentCapabilities; 2] = [&member, &candidate];

        // (a) The candidate is task-relevant but carries no role.
        let partial: HashMap<usize, RoleId> = [(0usize, 0usize)].into();
        let policy = MagnitudePolicy::new(0.0).with_role_modulation(partial.clone(), rho_ones(2));
        let join = policy.should_join(&candidate, &coalition, &ctx);
        assert!(
            !join.act && join.score.abs() < EPS,
            "missing role ⇒ decline"
        );
        let leave = policy.should_leave(&candidate, &seated, &ctx);
        assert!(
            !leave.act && leave.score.abs() < EPS,
            "missing role ⇒ decline (a leave decline keeps the member seated)"
        );
        let calc = MagnitudeValueCalculator::new(0b111).with_role_modulation(partial, rho_ones(2));
        assert_eq!(
            calc.calculate_value(&seated).to_bits(),
            f64::NEG_INFINITY.to_bits(),
            "missing role ⇒ the value surface's cannot-evaluate sentinel"
        );

        // (b) The candidate's role is outside a 2-role ρ ⇒ upstream `modulate`
        // errors ⇒ the same decline, no panic.
        let out_of_range: HashMap<usize, RoleId> = [(0usize, 0usize), (1usize, 7usize)].into();
        let policy =
            MagnitudePolicy::new(0.0).with_role_modulation(out_of_range.clone(), rho_ones(2));
        let join = policy.should_join(&candidate, &coalition, &ctx);
        assert!(
            !join.act && join.score.abs() < EPS,
            "out-of-range role ⇒ decline"
        );
        let calc =
            MagnitudeValueCalculator::new(0b111).with_role_modulation(out_of_range, rho_ones(2));
        assert_eq!(
            calc.calculate_value(&seated).to_bits(),
            f64::NEG_INFINITY.to_bits(),
            "out-of-range role ⇒ the value surface's cannot-evaluate sentinel"
        );
    }

    /// The typed async overrides produce the same [`Decision`] as their sync
    /// counterparts, including through a trait object (the offload snapshots
    /// owned `(masks, roles)` sides plus an owned config handle).
    #[tokio::test]
    async fn typed_async_matches_sync() {
        let ctx = DecisionContext {
            required_capabilities: 0b111,
        };
        let member = TestAgent {
            id: 0,
            caps: 0b001,
            trust: 50,
        };
        let clone = TestAgent {
            id: 1,
            caps: 0b001,
            trust: 50,
        };
        let roles: HashMap<usize, RoleId> = [(0usize, 0usize), (1usize, 1usize)].into();
        let policy = MagnitudePolicy::new(0.0).with_role_modulation(roles, rho_identity(2));

        let coalition: [&dyn AgentCapabilities; 1] = [&member];
        let sync_join = policy.should_join(&clone, &coalition, &ctx);
        let async_join = policy.should_join_async(&clone, &coalition, &ctx).await;
        assert_eq!(sync_join, async_join, "typed async join must equal sync");
        assert!(async_join.act);

        let seated: [&dyn AgentCapabilities; 2] = [&member, &clone];
        let sync_leave = policy.should_leave(&clone, &seated, &ctx);
        let async_leave = policy.should_leave_async(&clone, &seated, &ctx).await;
        assert_eq!(sync_leave, async_leave, "typed async leave must equal sync");
        assert!(!async_leave.act);

        // Missing role through the async path: decline, no panic.
        let orphan = TestAgent {
            id: 9,
            caps: 0b010,
            trust: 50,
        };
        let d = policy.should_join_async(&orphan, &coalition, &ctx).await;
        assert!(!d.act && d.score.abs() < EPS);

        // Reachable behind the trait object.
        let boxed: Box<dyn CoalitionDecisionPolicy> = Box::new(policy);
        let d = boxed.should_join_async(&clone, &coalition, &ctx).await;
        assert!(d.act, "trait-object typed async join should fire");
    }
}
