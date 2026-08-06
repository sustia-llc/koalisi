//! The unstaffable-residual decision policy (`EQ5a` Amendment A3.1, promoted to
//! the library by the `#80` registration, prereg §7 / lock D6).
//!
//! A coalition is usually valued by what it *is* — magnitude over the members
//! that matter to the task. This wrapper values it against what the **process
//! still cannot execute**:
//!
//! ```text
//! value(S) = Mag(S) − λ · Σ per_gen(g)   over demand elements g of the declared
//!                          writing whose (bit, role) is NOT covered by S
//! ```
//!
//! [`ResidualPolicy`] wraps a [`MagnitudePolicy`] and folds that penalty into
//! both the join and the leave margin. Admitting an agent that covers
//! previously-unstaffable steps improves the score by `λ` times those steps'
//! price; a member holding otherwise-unstaffable steps becomes less likely to be
//! swept out.
//!
//! ## The four properties that are load-bearing
//!
//! **1. The penalty depends on `S`, so it does not cancel.** `EQ5a` §4 D3b
//! originally scored `Mag(S) − λ · cost_of(writing, per_gen)` while ALSO fixing
//! the declared writing to be independent of the coalition. That term is a
//! per-task CONSTANT: it cancelled exactly from every join/leave margin, and the
//! cell measured **bit-identical to its control at every λ** — a lever that could
//! not move, passing every test it had because it was a no-op. The residual form
//! is the fix, and the identity at λ = 0 (below) is what proves the difference is
//! caused by the residual rather than by the wrapper.
//!
//! **2. Spiders are excluded.** `μ`/`η`/`δ`/`ε` occurrences are priced by
//! `cost_of` — they are hyperedges like any other — but they name no
//! `(bit, role)`, so "uncovered" is undefined for them and no coalition could
//! ever discharge them. Counting them would add a constant to every side of every
//! margin: exactly the cancelling term property 1 removed. [`Demand`] carries
//! `User` occurrences only, so the exclusion is structural here rather than a
//! filter that could be dropped.
//!
//! **3. A zero score is NOT a decline.** [`MagnitudePolicy`] reports an upstream
//! evaluation failure as `Decision { act: false, score: 0.0 }` — **the same value
//! a legitimate exact-zero margin carries**, and this lineage measured exact-zero
//! margins at ~43 % of the decision stream (`EQ3`). Inferring a decline from the
//! score would therefore suppress a great many real corrections, and treating a
//! decline as a margin could fold a correction into it and flip it to an accept.
//! The ambiguity is resolved BEFORE the fold: [`ResidualPolicy`] runs its own
//! evaluation of both member sets a decision compares and, on `Err`, declines and
//! counts (see [`ResidualPolicy::declines`]).
//!
//! **4. At λ = 0 the wrapper is its inner policy, bit for bit.** Acts and raw
//! score bits, on every decision. This is the causality pin for the whole lever;
//! it is asserted by this module's `lambda_zero_reproduces_the_inner_policy`
//! test, and the `#80` battery promotes it to a run gate (X-identity).
//!
//! ## Roles
//!
//! Coverage is **role-matched**: a step `s_{b,r}` is staffed iff some member
//! whose role is `r` holds bit `b`. The wrapper therefore carries the same
//! `agent_id → RoleId` map its inner typed policy does
//! ([`MagnitudePolicy::with_role_modulation`]) — hand it the same map. An agent
//! the map does not cover cannot have its coverage decided, so the wrapper
//! **forwards the inner policy's decision unchanged** rather than inventing one;
//! that is the same condition the library declines on, and a systematically
//! incomplete map freezes membership (gotcha 28).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use catgraph_syntax::frobenius::FrobeniusOr;

use crate::algorithms::AgentCapabilities;
use crate::decision::{
    CoalitionDecisionPolicy, Decision, DecisionContext, MagnitudePolicy, RoleId, magnitude_or_zero,
    relevant_masks,
};

use super::demand::Demand;
use super::signature::{Step, WorkflowGen};

/// What the residual charges for: every step **occurrence**, or every
/// **distinct** step.
///
/// The two coincide on a process with no repetition, and diverge exactly as much
/// as the writing repeats steps. Which one carries decision-relevant signal
/// beyond the distinct uncovered set is the `#80` question; the enum exists so
/// both can be built from one code path rather than two policies that could
/// drift.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ResidualBasis {
    /// One entry per step OCCURRENCE. Multiplicity is deliberately NOT
    /// collapsed: two occurrences of an unstaffable step are twice the penalty,
    /// which is the only way occurrence multiplicity reaches a decision at all
    /// (magnitude sees the OR-mask of distinct demand and nothing else).
    Occurrences,
    /// One entry per DISTINCT step, ascending. Prices *what* is unstaffable and
    /// ignores *how often* it is asked for.
    Distinct,
}

/// The priced elements a [`ResidualPolicy`] charges for when they are uncovered.
///
/// Built once per task from the declared writing's [`Demand`] and the cost
/// model's `per_gen` weight — the same closure
/// [`uniform_cost`](super::cost::uniform_cost) /
/// [`staffing_price`](super::cost::staffing_price) produce, so the residual and
/// the process cost are priced by one function rather than two that could
/// disagree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Residual {
    priced: Vec<(Step, u64)>,
    basis: ResidualBasis,
}

impl Residual {
    /// Price a declared writing's demand under `basis`.
    ///
    /// `per_gen` is called on `FrobeniusOr::User(step)` for each element, never
    /// on a spider — a spider names no `(bit, role)` and so can never be
    /// uncovered (module docs, property 2).
    #[must_use]
    pub fn new(
        demand: &Demand,
        basis: ResidualBasis,
        per_gen: impl Fn(&WorkflowGen) -> u64,
    ) -> Self {
        let steps: Vec<Step> = match basis {
            ResidualBasis::Occurrences => demand.occurrences().to_vec(),
            ResidualBasis::Distinct => demand.distinct().collect(),
        };
        let priced = steps
            .into_iter()
            .map(|step| {
                let price = per_gen(&FrobeniusOr::User(step));
                (step, price)
            })
            .collect();
        Self { priced, basis }
    }

    /// Which basis this residual was built on.
    #[must_use]
    pub fn basis(&self) -> ResidualBasis {
        self.basis
    }

    /// The priced elements, in demand order (content order for
    /// [`Occurrences`](ResidualBasis::Occurrences), ascending `Step` order for
    /// [`Distinct`](ResidualBasis::Distinct)).
    #[must_use]
    pub fn entries(&self) -> &[(Step, u64)] {
        &self.priced
    }

    /// How many priced elements there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.priced.len()
    }

    /// Whether the residual prices nothing — a pure-wiring writing, or a demand
    /// with no steps.
    ///
    /// Worth checking at a call site that expects the lever to be able to move:
    /// an empty residual makes the wrapper exactly its inner policy.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.priced.is_empty()
    }

    /// `Σ per_gen(g)` over every priced element — the residual at an EMPTY
    /// coalition, i.e. the largest the penalty can be.
    ///
    /// Saturating: a sum this large means a pathological cost model, and a
    /// clamped total is a better outcome than an overflow panic in a decision
    /// path.
    #[must_use]
    pub fn total_price(&self) -> u64 {
        self.priced
            .iter()
            .fold(0u64, |acc, &(_, price)| acc.saturating_add(price))
    }
}

/// A shared count of decisions declined because the independent evaluation probe
/// reported an upstream failure (module docs, property 3).
///
/// `Clone` **shares** the count (it is an `Arc` inside) — the `FeedbackStore` and
/// evaluator-cache precedent. Hand one counter to every per-task policy of a run
/// and read a single number at the end; a fresh [`ResidualPolicy`] that is never
/// given one carries its own.
#[derive(Clone, Debug, Default)]
pub struct DeclineCounter(Arc<AtomicUsize>);

impl DeclineCounter {
    /// A counter at zero.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many declines have been recorded.
    #[must_use]
    pub fn get(&self) -> usize {
        self.0.load(Ordering::Relaxed)
    }

    fn record(&self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

/// A [`MagnitudePolicy`] whose join and leave margins are corrected by the
/// price of the demand the coalition cannot cover.
///
/// See the [module docs](self) for the formula, the four load-bearing
/// properties, and the role-map contract.
///
/// # Why the inner policy is concrete
///
/// The wrapper is not generic over [`CoalitionDecisionPolicy`], for two reasons
/// that are both about honesty rather than convenience: the act predicate it
/// re-derives is *magnitude's* (`margin > join_margin` on joins, `delta <= 0` on
/// leaves, read off the very policy being wrapped rather than mirrored as a
/// constant), and the evaluation probe of property 3 is the magnitude evaluation
/// path. A generic wrapper would have to guess both.
pub struct ResidualPolicy {
    inner: MagnitudePolicy,
    /// `agent_id → RoleId` — the same map the inner typed policy carries.
    roles: HashMap<usize, RoleId>,
    /// The coefficient λ. Finite and non-negative (clamped at construction).
    lambda: f64,
    residual: Residual,
    declines: DeclineCounter,
}

impl ResidualPolicy {
    /// Wrap `inner` with the residual penalty at coefficient `lambda`.
    ///
    /// `roles` must be the same `agent_id → RoleId` map `inner` was given by
    /// [`MagnitudePolicy::with_role_modulation`]; coverage is role-matched, and
    /// an agent absent from the map makes the wrapper forward `inner`'s decision
    /// unchanged.
    ///
    /// A non-finite or negative `lambda` is **clamped to `0.0` with a warning**
    /// (the `PersistentAifConfig::n_bits` precedent): a negative coefficient
    /// would make the correction negative and turn "covers more of the process"
    /// into a penalty, and a `NaN` would poison every score. Clamping keeps a
    /// misconfiguration inert rather than silently inverted.
    #[must_use]
    pub fn new(
        inner: MagnitudePolicy,
        roles: HashMap<usize, RoleId>,
        lambda: f64,
        residual: Residual,
    ) -> Self {
        let lambda = if lambda.is_finite() && lambda >= 0.0 {
            lambda
        } else {
            tracing::warn!(
                lambda,
                "residual policy: lambda must be finite and non-negative; clamping to 0.0 \
                 (the wrapper is then exactly its inner policy)"
            );
            0.0
        };
        Self {
            inner,
            roles,
            lambda,
            residual,
            declines: DeclineCounter::new(),
        }
    }

    /// Report declines into a shared [`DeclineCounter`] instead of this policy's
    /// own.
    #[must_use]
    pub fn with_declines(mut self, declines: DeclineCounter) -> Self {
        self.declines = declines;
        self
    }

    /// The (clamped) coefficient λ actually in force.
    #[must_use]
    pub fn lambda(&self) -> f64 {
        self.lambda
    }

    /// The priced residual this policy charges against.
    #[must_use]
    pub fn residual(&self) -> &Residual {
        &self.residual
    }

    /// How many decisions this policy declined because its evaluation probe
    /// reported an upstream failure (module docs, property 3).
    ///
    /// A number to **report**, not to assert on: an upstream rejection is a
    /// legitimate outcome of a member set, and a run that hides it behind a zero
    /// score is the defect this counter exists to make visible.
    #[must_use]
    pub fn declines(&self) -> usize {
        self.declines.get()
    }

    /// `λ · Σ per_gen(g)` at an EMPTY coalition — the largest the penalty can be
    /// for this task.
    #[must_use]
    pub fn full_term(&self) -> f64 {
        self.lambda * price_as_f64(self.residual.total_price())
    }

    /// The `(role, capabilities)` of one participant, or `None` when the role map
    /// has no entry for it.
    fn staff_of(&self, agent: &dyn AgentCapabilities) -> Option<(RoleId, u32)> {
        Some((*self.roles.get(&agent.agent_id())?, agent.capabilities()))
    }

    fn staff(&self, members: &[&dyn AgentCapabilities]) -> Option<Vec<(RoleId, u32)>> {
        members.iter().map(|a| self.staff_of(*a)).collect()
    }

    /// `Σ per_gen(g)` over residual elements no member of `staff` can perform.
    fn uncovered(&self, staff: &[(RoleId, u32)]) -> u64 {
        self.residual
            .entries()
            .iter()
            .filter(|&&(step, _)| !step_staffed(staff, step))
            .fold(0u64, |acc, &(_, price)| acc.saturating_add(price))
    }

    /// A decline in the inner policy's own shape, counted.
    fn decline(&self) -> Decision {
        self.declines.record();
        Decision {
            act: false,
            score: 0.0,
        }
    }

    /// Fold a non-negative residual correction into a base decision.
    ///
    /// The act predicate is re-derived from the corrected score unconditionally.
    /// That is safe **because** [`evaluation_failed`] has already ruled out a
    /// decline: every base score reaching here is a genuine margin, so at
    /// `correction == 0` the re-derivation reproduces the inner policy's own
    /// decision exactly (`margin > join_margin` / `delta <= 0` over an unchanged
    /// score), and at `correction > 0` it applies the registered value rule. The
    /// non-finite guard stays — a non-finite base has no correction that means
    /// anything.
    fn fold(base: Decision, correction: f64, act: impl Fn(f64) -> bool) -> Decision {
        if !base.score.is_finite() {
            return base;
        }
        let score = base.score + correction;
        Decision {
            act: act(score),
            score,
        }
    }
}

/// A residual price as an `f64`, so the one lossy conversion in this module has
/// one site and one justification.
///
/// Both shipped cost models are small: [`uniform_cost`](super::cost::uniform_cost)
/// weighs every element `1`, and [`staffing_price`](super::cost::staffing_price)
/// weighs it `1 + scarcity`, bounded by the pool size. Summed over a task's demand
/// elements, either stays far below `2^53`, where `u64 → f64` is exact. A caller
/// whose own `per_gen` reached the mantissa boundary would have already saturated
/// [`Residual::total_price`], and a rounded penalty is a better outcome in a
/// decision path than a panic.
fn price_as_f64(price: u64) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    {
        price as f64
    }
}

/// Whether any member of `staff` can perform `step` — role-matched.
fn step_staffed(staff: &[(RoleId, u32)], step: Step) -> bool {
    step.capability_mask().is_some_and(|mask| {
        staff
            .iter()
            .any(|&(role, caps)| role == RoleId::from(step.role.index()) && caps & mask != 0)
    })
}

/// Independently detect an upstream evaluation failure over the two member sets a
/// decision compares (module docs, property 3).
///
/// It is the library's own [`relevant_masks`] + [`magnitude_or_zero`] path at the
/// pinned `t = 1` — the same dedup-and-relevance filter, the same member sets, the
/// same restrict-then-close Möbius pipeline the arm runs on. It differs in ONE
/// respect: its couplings are UNTYPED where a typed inner policy's are
/// ρ-modulated, so it detects the structural failure modes (a member set the
/// upstream metric space rejects) exactly, while a hypothetical failure that only
/// the ρ-modulated coupling matrix could trigger would slip past it. That residual
/// is disclosed rather than assumed away; upstream exposes no typed-evaluation
/// probe to close it.
///
/// Free rather than a method: it reads nothing off the policy, and saying so in
/// the signature is worth more than the call-site brevity.
fn evaluation_failed(
    left: &[&dyn AgentCapabilities],
    right: &[&dyn AgentCapabilities],
    required: u32,
) -> bool {
    let probe = |members: &[&dyn AgentCapabilities]| {
        magnitude_or_zero(&relevant_masks(members, required), required).is_err()
    };
    probe(left) || probe(right)
}

impl CoalitionDecisionPolicy for ResidualPolicy {
    /// `Δvalue = ΔMag + λ · (pen(S) − pen(S ∪ {x}))`.
    ///
    /// The correction is non-negative — admitting an agent can only cover more
    /// steps — so it can turn a declined join into an accepted one, never the
    /// reverse.
    fn should_join(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        ctx: &DecisionContext,
    ) -> Decision {
        let (Some(without), Some(joined)) = (self.staff(coalition), self.staff_of(agent)) else {
            // No role for a participant: the inner policy declines for exactly
            // this reason, so forward ITS decision rather than inventing one.
            return self.inner.should_join(agent, coalition, ctx);
        };
        let mut with_view: Vec<&dyn AgentCapabilities> = coalition.to_vec();
        with_view.push(agent);
        if evaluation_failed(coalition, &with_view, ctx.required_capabilities) {
            return self.decline();
        }
        let base = self.inner.should_join(agent, coalition, ctx);
        let mut with = without.clone();
        with.push(joined);
        // `saturating_sub` states the monotonicity rather than trusting it: a
        // larger staff can never leave more uncovered.
        let correction = self.lambda
            * price_as_f64(
                self.uncovered(&without)
                    .saturating_sub(self.uncovered(&with)),
            );
        // The inner policy's own `margin > join_margin` rule, read off the very
        // policy being wrapped rather than mirrored as a constant — the score
        // changed, so the predicate has to be restated, but the threshold does
        // not.
        let margin = self.inner.join_margin;
        Self::fold(base, correction, |score| score > margin)
    }

    /// `Δvalue = ΔMag + λ · (pen(S \ {x}) − pen(S))`, leaving iff `Δvalue ≤ 0`
    /// (the inner policy's leave rule, restated over value instead of magnitude).
    ///
    /// The correction is again non-negative, so a member holding otherwise
    /// unstaffable steps becomes LESS likely to be swept out.
    fn should_leave(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        ctx: &DecisionContext,
    ) -> Decision {
        // `coalition` INCLUDES `agent` on the leave path (library convention).
        let id = agent.agent_id();
        let remaining: Vec<&dyn AgentCapabilities> = coalition
            .iter()
            .filter(|a| a.agent_id() != id)
            .copied()
            .collect();
        let (Some(inside), Some(outside)) = (self.staff(coalition), self.staff(&remaining)) else {
            return self.inner.should_leave(agent, coalition, ctx);
        };
        if evaluation_failed(coalition, &remaining, ctx.required_capabilities) {
            return self.decline();
        }
        let base = self.inner.should_leave(agent, coalition, ctx);
        let correction = self.lambda
            * price_as_f64(
                self.uncovered(&outside)
                    .saturating_sub(self.uncovered(&inside)),
            );
        Self::fold(base, correction, |score| score <= 0.0)
    }
}

#[cfg(test)]
mod tests {
    use catgraph_applied::prop::Free;
    use catgraph_applied::prop::colored::ColoredExpr;

    use super::super::cost::uniform_cost;
    use super::super::demand::demand;
    use super::super::signature::{Role, chain, spider_expr, step_expr};
    use super::*;

    /// A pool worker: `id`, capability mask, trust.
    #[derive(Clone, Copy, Debug)]
    struct Agent {
        id: usize,
        caps: u32,
    }

    impl AgentCapabilities for Agent {
        fn agent_id(&self) -> usize {
            self.id
        }
        fn capabilities(&self) -> u32 {
            self.caps
        }
        fn trust_level(&self) -> u32 {
            50
        }
    }

    fn role_map(entries: &[(usize, RoleId)]) -> HashMap<usize, RoleId> {
        entries.iter().copied().collect()
    }

    /// `δ_r ; (s_{1,r} ⊗ s_{1,r}) ; μ_r ; s_{2,r}` — one repeated step, one
    /// singleton, and two spiders that must never be priced.
    fn fanned_workflow() -> super::super::signature::Workflow {
        let r = Role::new(0);
        ColoredExpr::new(
            vec![r],
            chain(vec![
                spider_expr(FrobeniusOr::Delta(r)),
                Free::tensor(step_expr(Step::new(1, r)), step_expr(Step::new(1, r))),
                spider_expr(FrobeniusOr::Mu(r)),
                step_expr(Step::new(2, r)),
            ])
            .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn residual_prices_occurrences_or_distinct_and_never_spiders() {
        let d = demand(&fanned_workflow());
        let r = Role::new(0);

        let occ = Residual::new(&d, ResidualBasis::Occurrences, uniform_cost());
        assert_eq!(occ.basis(), ResidualBasis::Occurrences);
        assert_eq!(
            occ.len(),
            3,
            "two s1 occurrences + one s2; the spiders are not demand"
        );
        assert_eq!(occ.total_price(), 3);
        assert!(!occ.is_empty());

        let dis = Residual::new(&d, ResidualBasis::Distinct, uniform_cost());
        assert_eq!(dis.len(), 2, "s1 and s2, once each");
        assert_eq!(dis.total_price(), 2);
        assert_eq!(
            dis.entries().iter().map(|&(s, _)| s).collect::<Vec<_>>(),
            vec![Step::new(1, r), Step::new(2, r)],
            "the distinct walk is Step-ordered"
        );

        // A scarcity-style weight reaches every element, and multiplicity
        // multiplies it — the only route by which occurrence count enters a score.
        let heavy = Residual::new(&d, ResidualBasis::Occurrences, |g| match g {
            FrobeniusOr::User(s) if s.bit == 1 => 10,
            _ => 1,
        });
        assert_eq!(heavy.total_price(), 21);
    }

    /// Property 4 of the module docs, and the `#80` X-identity gate in miniature:
    /// at λ = 0 the wrapper is its inner policy on acts AND raw score bits.
    #[test]
    fn lambda_zero_reproduces_the_inner_policy() {
        let roles = role_map(&[(0, 0), (1, 0), (2, 0)]);
        let inner = MagnitudePolicy::default();
        let d = demand(&fanned_workflow());
        let policy = ResidualPolicy::new(
            inner.clone(),
            roles,
            0.0,
            Residual::new(&d, ResidualBasis::Occurrences, uniform_cost()),
        );
        assert_eq!(policy.lambda().to_bits(), 0.0f64.to_bits());
        assert_eq!(policy.full_term().to_bits(), 0.0f64.to_bits());

        let a = Agent {
            id: 0,
            caps: 0b0010,
        };
        let b = Agent {
            id: 1,
            caps: 0b0100,
        };
        let c = Agent {
            id: 2,
            caps: 0b0110,
        };
        let ctx = DecisionContext {
            required_capabilities: 0b0110,
        };

        let candidates: [&dyn AgentCapabilities; 3] = [&a, &b, &c];
        for candidate in candidates {
            let members: Vec<&dyn AgentCapabilities> = vec![&a, &b];
            let wrapped = policy.should_join(candidate, &members, &ctx);
            let bare = inner.should_join(candidate, &members, &ctx);
            assert_eq!(wrapped.act, bare.act, "join act at lambda = 0");
            assert_eq!(
                wrapped.score.to_bits(),
                bare.score.to_bits(),
                "join score BITS at lambda = 0"
            );

            let inside: Vec<&dyn AgentCapabilities> = vec![&a, &b, &c];
            let wrapped = policy.should_leave(candidate, &inside, &ctx);
            let bare = inner.should_leave(candidate, &inside, &ctx);
            assert_eq!(wrapped.act, bare.act, "leave act at lambda = 0");
            assert_eq!(
                wrapped.score.to_bits(),
                bare.score.to_bits(),
                "leave score BITS at lambda = 0"
            );
        }
        assert_eq!(
            policy.declines(),
            0,
            "no probe declines on a well-formed pool"
        );
    }

    /// The lever is LIVE: a coalition-dependent penalty, so a candidate that
    /// covers an otherwise-unstaffable step scores strictly higher than it does
    /// under the bare inner policy — the regression the cancelling formulation
    /// would have passed silently.
    #[test]
    fn the_residual_moves_a_score_and_can_flip_a_join() {
        let r = Role::new(0);
        // A one-step process on bit 3, which only agent 2 can perform.
        let workflow =
            ColoredExpr::new(vec![r], chain(vec![step_expr(Step::new(3, r))]).unwrap()).unwrap();
        let d = demand(&workflow);
        let roles = role_map(&[(0, 0), (1, 0), (2, 0)]);
        let inner = MagnitudePolicy::default();
        // A large λ against a heavy price, so the correction dominates the
        // magnitude margin rather than merely perturbing it.
        let policy = ResidualPolicy::new(
            inner.clone(),
            roles,
            1.0,
            Residual::new(&d, ResidualBasis::Occurrences, |_| 100),
        );
        assert_eq!(policy.full_term().to_bits(), 100.0f64.to_bits());

        let a = Agent {
            id: 0,
            caps: 0b0011,
        };
        let clone_of_a = Agent {
            id: 1,
            caps: 0b0011,
        };
        let specialist = Agent {
            id: 2,
            caps: 0b1000,
        };
        let ctx = DecisionContext {
            required_capabilities: 0b1011,
        };
        let members: Vec<&dyn AgentCapabilities> = vec![&a, &clone_of_a];

        let bare = inner.should_join(&specialist, &members, &ctx);
        let wrapped = policy.should_join(&specialist, &members, &ctx);
        assert!(
            wrapped.score > bare.score,
            "covering the unstaffable step must improve the score: {} vs {}",
            wrapped.score,
            bare.score
        );
        assert!(wrapped.act, "the correction carries the join");

        // …and it is the RESIDUAL, not the wrapper: the same decision at λ = 0
        // is the bare one, bit for bit.
        let inert = ResidualPolicy::new(
            inner,
            role_map(&[(0, 0), (1, 0), (2, 0)]),
            0.0,
            Residual::new(&d, ResidualBasis::Occurrences, |_| 100),
        );
        assert_eq!(
            inert
                .should_join(&specialist, &members, &ctx)
                .score
                .to_bits(),
            bare.score.to_bits()
        );
    }

    /// An agent the role map does not cover cannot have its coverage decided, so
    /// the wrapper forwards the inner policy's decision unchanged — the same
    /// condition the library declines on, never a decision invented here.
    #[test]
    fn a_missing_role_forwards_the_inner_decision() {
        let r = Role::new(0);
        let workflow =
            ColoredExpr::new(vec![r], chain(vec![step_expr(Step::new(1, r))]).unwrap()).unwrap();
        let d = demand(&workflow);
        // Agent 2 is absent from the map.
        let inner = MagnitudePolicy::default();
        let policy = ResidualPolicy::new(
            inner.clone(),
            role_map(&[(0, 0), (1, 0)]),
            1.0,
            Residual::new(&d, ResidualBasis::Occurrences, |_| 100),
        );

        let a = Agent {
            id: 0,
            caps: 0b0011,
        };
        let b = Agent {
            id: 1,
            caps: 0b0101,
        };
        let unmapped = Agent {
            id: 2,
            caps: 0b0010,
        };
        let ctx = DecisionContext {
            required_capabilities: 0b0111,
        };
        let members: Vec<&dyn AgentCapabilities> = vec![&a, &b];

        let wrapped = policy.should_join(&unmapped, &members, &ctx);
        let bare = inner.should_join(&unmapped, &members, &ctx);
        assert_eq!(wrapped.act, bare.act);
        assert_eq!(wrapped.score.to_bits(), bare.score.to_bits());
        assert_eq!(
            policy.declines(),
            0,
            "a missing role is a forward, not a probe decline"
        );
    }

    /// Role matching is real: a member of the WRONG role does not staff a step,
    /// so the penalty stays.
    #[test]
    fn coverage_is_role_matched_not_role_blind() {
        let r1 = Role::new(1);
        let workflow =
            ColoredExpr::new(vec![r1], chain(vec![step_expr(Step::new(2, r1))]).unwrap()).unwrap();
        let d = demand(&workflow);
        let inner = MagnitudePolicy::default();
        // Agent 0 holds bit 2 but is role 0; agent 1 holds bit 2 and is role 1.
        let policy = ResidualPolicy::new(
            inner,
            role_map(&[(0, 0), (1, 1)]),
            1.0,
            Residual::new(&d, ResidualBasis::Occurrences, |_| 7),
        );

        let wrong_role = Agent {
            id: 0,
            caps: 0b0100,
        };
        let right_role = Agent {
            id: 1,
            caps: 0b0100,
        };
        assert_eq!(policy.uncovered(&[(0, wrong_role.caps)]), 7);
        assert_eq!(policy.uncovered(&[(1, right_role.caps)]), 0);
        assert_eq!(policy.uncovered(&[]), 7, "an empty staff covers nothing");
    }

    /// λ is clamped, not trusted: a negative or non-finite coefficient would
    /// invert or poison every correction, so it is forced inert.
    #[test]
    fn a_bad_lambda_is_clamped_inert() {
        let r = Role::new(0);
        let workflow =
            ColoredExpr::new(vec![r], chain(vec![step_expr(Step::new(0, r))]).unwrap()).unwrap();
        let d = demand(&workflow);
        for bad in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let p = ResidualPolicy::new(
                MagnitudePolicy::default(),
                role_map(&[(0, 0)]),
                bad,
                Residual::new(&d, ResidualBasis::Occurrences, uniform_cost()),
            );
            assert_eq!(
                p.lambda().to_bits(),
                0.0f64.to_bits(),
                "lambda {bad} must clamp to 0"
            );
            assert_eq!(p.full_term().to_bits(), 0.0f64.to_bits());
        }
    }

    /// A [`DeclineCounter`] clone SHARES its count — one number per run, however
    /// many per-task policies were built.
    #[test]
    fn decline_counters_share_across_clones() {
        let shared = DeclineCounter::new();
        assert_eq!(shared.get(), 0);
        let other = shared.clone();
        shared.record();
        other.record();
        assert_eq!(shared.get(), 2);
        assert_eq!(other.get(), 2);

        let r = Role::new(0);
        let workflow =
            ColoredExpr::new(vec![r], chain(vec![step_expr(Step::new(0, r))]).unwrap()).unwrap();
        let d = demand(&workflow);
        let p = ResidualPolicy::new(
            MagnitudePolicy::default(),
            role_map(&[(0, 0)]),
            0.05,
            Residual::new(&d, ResidualBasis::Occurrences, uniform_cost()),
        )
        .with_declines(shared.clone());
        assert_eq!(p.declines(), 2, "the policy reads the shared count");
    }
}
