//! Active Inference coalition-decision strategy (feature `decision`).
//!
//! The bridge from coalition membership to expected free energy `G` runs
//! through *capability coverage*: how much of the task's
//! `required_capabilities` bitmask the coalition's unioned capabilities cover.
//! Coverage maps to the precision `p` of a 2-state / 2-observation POMDP
//! observation model `A`; higher precision yields a lower `G` (the engine's
//! standard sign convention: lower `G` is better). Because coverage — and thus
//! `A` — changes with membership, the join/leave decision is *non-degenerate*:
//! adding an agent that covers a previously-uncovered required bit strictly
//! lowers `G`, while adding a redundant clone leaves `G` unchanged.
//!
//! The coverage→`G` math lives upstream in [`aif::competence_efe`] (the
//! reusable coalition-value primitive): coverage *is* the scalar competence it
//! takes, and it varies the *observation model* by competence. We use it rather
//! than `aif::CoalitionEvaluator`, whose `observation_probs` cannot see coalition
//! members and so can only vary *preferences* by membership — which collapses to
//! `G ≈ 0` for every coalition.

use std::future::Future;
use std::pin::Pin;

use crate::algorithms::{AgentCapabilities, ValueCalculator};

use super::{CoalitionDecisionPolicy, Decision, DecisionContext};

// Supporting belief structures (issue #2). Re-exported so koalisi callers reach
// them as `koalisi::decision::{TrustBeliefs, CompatibilityBeliefs,
// CoalitionHistory}` without depending on `aif` directly. They are plain
// `f64`/`HashMap` data (no extra deps). `belief_weighted_preference` aggregates
// them into a single alignment scalar in `[0.05, 0.95]`.
use aif::{AgentId, belief_weighted_preference};
pub use aif::{CoalitionHistory, CompatibilityBeliefs, TrustBeliefs};

/// Tunable parameters mapping capability coverage onto a POMDP and its `G`.
#[derive(Debug, Clone, Copy)]
pub struct BridgeParams {
    /// Observation-model precision at full coverage. In `(0.5, 1.0)`.
    pub max_precision: f64,
    /// Preference mass placed on observation 1 ("success"). In `(0.5, 1.0)`.
    pub success_preference: f64,
    /// Action precision passed to the POMDP agent.
    pub alpha: f64,
    /// How much the belief signal (trust / compatibility / history) modulates
    /// the competence that drives the observation model, in `[0, 1]` (issue #2).
    ///
    /// The competence fed to the POMDP is
    /// `(1 - belief_weight)·coverage + belief_weight·alignment`, where
    /// `alignment ∈ [0.05, 0.95]` is the belief aggregate from
    /// [`belief_weighted_preference`]. At the default `0.0` the policy is pure
    /// capability coverage — identical to the pre-issue-#2 behavior. Crucially
    /// the blend modulates the *observation model* (via competence), not just
    /// preferences, so membership still alters achievable `G` and the decision
    /// stays non-degenerate (the B2/B4 requirement).
    pub belief_weight: f64,
}

impl Default for BridgeParams {
    fn default() -> Self {
        Self {
            max_precision: 0.95,
            success_preference: 0.9,
            alpha: 8.0,
            belief_weight: 0.0,
        }
    }
}

/// Fraction of `required` capability bits covered by `caps`.
///
/// Returns `1.0` when `required == 0` (nothing required ⇒ already fully
/// covered, so no coalition can improve coverage). Otherwise the ratio of
/// covered required bits to total required bits, in `[0.0, 1.0]`.
fn coverage(caps: u32, required: u32) -> f64 {
    if required == 0 {
        return 1.0;
    }
    f64::from((caps & required).count_ones()) / f64::from(required.count_ones())
}

/// Union of the capability bitmasks of `agents`.
fn union_caps(agents: &[&dyn AgentCapabilities]) -> u32 {
    agents.iter().fold(0u32, |acc, a| acc | a.capabilities())
}

/// The capability→EFE bridge. Public so the mapping can be tested directly.
#[derive(Debug, Clone, Copy, Default)]
pub struct CapabilityModel;

impl CapabilityModel {
    /// Map capability coverage `cov ∈ [0, 1]` to expected free energy `G`
    /// (lower = better) via the upstream [`aif::competence_efe`] primitive:
    /// precision `p = 0.5 + (max_precision - 0.5) * cov`, so `cov == 0` gives an
    /// uninformative observation model (`p = 0.5`) and `cov == 1` the most
    /// informative one (`p = max_precision`). The AIF math lives in `aif`; this
    /// only maps koalisi's `BridgeParams` onto [`aif::ObsPrecisionParams`].
    ///
    /// # Errors
    ///
    /// Returns [`aif::AifError`] if `cov` is outside `[0, 1]` or the resulting
    /// POMDP parameters are rejected by the engine.
    pub fn efe_for_coverage(cov: f64, params: BridgeParams) -> Result<f64, aif::AifError> {
        aif::competence_efe(
            cov,
            aif::ObsPrecisionParams {
                max_precision: params.max_precision,
                success_preference: params.success_preference,
                alpha: params.alpha,
                // Deterministic-B bridge: no stochastic-transition info-gain term
                // (0.6.0's opt-in; noise RAISES net G — see tira CHANGELOG 0.6.0).
                transition_noise: 0.0,
            },
        )
    }
}

/// A [`ValueCalculator`] that scores a coalition by its (negated) expected free
/// energy under the capability-coverage bridge.
///
/// Because [`ValueCalculator::calculate_value`] has no context argument, the
/// task's `required_capabilities` and the [`BridgeParams`] are stored as fields.
/// The returned value is `-G` so that, like the other calculators, higher is
/// better. On engine error it logs and returns [`f64::NEG_INFINITY`].
#[derive(Debug, Clone, Copy)]
pub struct EfeValueCalculator {
    pub required_capabilities: u32,
    pub params: BridgeParams,
}

impl ValueCalculator for EfeValueCalculator {
    fn calculate_value(&self, agents: &[&dyn AgentCapabilities]) -> f64 {
        let cov = coverage(union_caps(agents), self.required_capabilities);
        match CapabilityModel::efe_for_coverage(cov, self.params) {
            Ok(g) => -g,
            Err(e) => {
                tracing::warn!(error = %e, coverage = cov, "EFE value calculation failed");
                f64::NEG_INFINITY
            }
        }
    }
}

/// Active Inference join/leave policy driven by [`DecisionContext`].
///
/// The agent joins when doing so *lowers* expected free energy by more than
/// `join_margin`; it leaves when staying does not lower `G`. This mirrors the
/// engine's `decide_join` rule (`g_coalition < g_alone`) at `join_margin == 0`.
/// `belief_weight` defaults to `0.0`, so a default-constructed policy is pure
/// capability coverage and the (empty) belief structures are never consulted.
/// Use [`AifDecisionPolicy::with_beliefs`] (and a `params.belief_weight > 0`) to
/// fold trust / compatibility / history into the decision (issue #2).
#[derive(Debug, Clone, Default)]
pub struct AifDecisionPolicy {
    pub params: BridgeParams,
    pub join_margin: f64,
    /// Per-agent dynamic trust (EMA, `[0, 1]`). Distinct from the *static*
    /// [`AgentCapabilities::trust_level`] baseline — this is the learned signal.
    pub trust: TrustBeliefs,
    /// Symmetric pairwise compatibility (`[0, 1]`).
    pub compat: CompatibilityBeliefs,
    /// Observed past performance keyed by (sorted) membership.
    pub history: CoalitionHistory,
}

impl AifDecisionPolicy {
    /// Construct a belief-aware policy. The beliefs only take effect when
    /// `params.belief_weight > 0`; at `0` they are ignored and this behaves
    /// exactly like the pure-coverage policy.
    #[must_use]
    pub fn with_beliefs(
        params: BridgeParams,
        join_margin: f64,
        trust: TrustBeliefs,
        compat: CompatibilityBeliefs,
        history: CoalitionHistory,
    ) -> Self {
        Self {
            params,
            join_margin,
            trust,
            compat,
            history,
        }
    }

    /// Competence→`G` over owned params (no `&self` borrow), so it can run inside
    /// a `'static` rayon closure. The competence is capability coverage,
    /// optionally blended with belief alignment (see [`Self::blended_competence`]).
    /// Engine error ⇒ `+∞` (worst outcome).
    fn g_at(competence: f64, params: BridgeParams) -> f64 {
        match CapabilityModel::efe_for_coverage(competence, params) {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!(error = %e, competence, "EFE computation failed");
                f64::INFINITY
            }
        }
    }

    /// Blend capability `coverage` with belief `alignment` into the competence
    /// that drives the observation model:
    /// `(1 - belief_weight)·coverage + belief_weight·alignment`. At
    /// `belief_weight == 0` this is exactly `coverage`, preserving the original
    /// non-degenerate coverage→`G` behavior. Both inputs are in `[0, 1]`, so the
    /// result is too (clamped defensively).
    fn blended_competence(coverage: f64, alignment: f64, belief_weight: f64) -> f64 {
        let w = belief_weight.clamp(0.0, 1.0);
        ((1.0 - w) * coverage + w * alignment).clamp(0.0, 1.0)
    }

    /// Belief alignment scalar in `[0.05, 0.95]` for `agent` among `members`
    /// (mean trust + compatibility with its partners, blended with any recorded
    /// history). Returns the neutral `0.5` when beliefs are disabled
    /// (`belief_weight == 0`) or the agent has no partners in `members`.
    fn alignment(&self, agent: AgentId, members: &[AgentId]) -> f64 {
        if self.params.belief_weight <= 0.0 {
            return 0.5;
        }
        belief_weighted_preference(agent, members, &self.trust, &self.compat, &self.history)[0]
    }

    /// Pure join decision over owned competence scalars.
    ///
    /// `comp_alone` / `comp_coalition` are the blended competences for the agent
    /// acting alone vs. in the coalition. Shared by the sync
    /// [`CoalitionDecisionPolicy::should_join`] and the async
    /// [`CoalitionDecisionPolicy::should_join_async`] override: both reduce their
    /// borrowed inputs to these two owned `f64` and call this.
    fn join_decision_from_competences(
        comp_alone: f64,
        comp_coalition: f64,
        params: BridgeParams,
        join_margin: f64,
    ) -> Decision {
        let g_alone = Self::g_at(comp_alone, params);
        let g_coalition = Self::g_at(comp_coalition, params);
        let margin = g_alone - g_coalition;
        // `g_at` returns +∞ on engine error, so `∞ - ∞` can be NaN. Never join on
        // an untrustworthy margin, and never propagate NaN/±∞ as a score.
        if !margin.is_finite() {
            return Decision { act: false, score: 0.0 };
        }
        Decision {
            act: margin > join_margin,
            score: margin,
        }
    }

    /// Pure leave decision over owned competence scalars.
    ///
    /// `comp_in` is the agent-present competence (coverage of the coalition
    /// including the agent, blended with the agent's belief alignment to its
    /// partners); `comp_out` is the agent-absent competence (coverage without the
    /// agent, neutral alignment). If removing the agent does not raise `G`
    /// (`g_out - g_in <= 0`) it contributes nothing — by coverage *or* belief —
    /// and should leave.
    fn leave_decision_from_competences(
        comp_in: f64,
        comp_out: f64,
        params: BridgeParams,
    ) -> Decision {
        let g_in = Self::g_at(comp_in, params);
        let g_out = Self::g_at(comp_out, params);
        let delta = g_out - g_in;
        // `g_at` returns +∞ on engine error, so `∞ - ∞` can be NaN. Don't eject on
        // an untrustworthy value and don't propagate NaN/±∞ as a score.
        if !delta.is_finite() {
            return Decision { act: false, score: 0.0 };
        }
        Decision {
            act: delta <= 0.0,
            score: delta,
        }
    }

    /// Collect the `AgentId`s of `coalition`, appending `extra` if `Some`.
    /// Used to build the membership list `belief_weighted_preference` consumes.
    fn member_ids(coalition: &[&dyn AgentCapabilities], extra: Option<AgentId>) -> Vec<AgentId> {
        coalition
            .iter()
            .map(|a| a.agent_id())
            .chain(extra)
            .collect()
    }

    /// Blended `(comp_alone, comp_coalition)` competences for a join decision.
    /// `coalition` excludes `agent`. Acting alone has no partners ⇒ neutral
    /// alignment; the coalition's alignment is the agent's belief aggregate over
    /// its prospective partners. Returns owned `f64` so the async path can move
    /// them into the rayon closure.
    fn join_competences(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        ctx: &DecisionContext,
    ) -> (f64, f64) {
        let required = ctx.required_capabilities;
        let w = self.params.belief_weight;
        let agent_caps = agent.capabilities();

        let cov_alone = coverage(agent_caps, required);
        let cov_coalition = coverage(union_caps(coalition) | agent_caps, required);

        let members = Self::member_ids(coalition, Some(agent.agent_id()));
        let align_coalition = self.alignment(agent.agent_id(), &members);

        (
            Self::blended_competence(cov_alone, 0.5, w),
            Self::blended_competence(cov_coalition, align_coalition, w),
        )
    }

    /// Blended `(comp_in, comp_out)` competences for a leave decision.
    /// `coalition` includes `agent`. Agent-present competence carries the agent's
    /// belief alignment to its partners; agent-absent competence uses neutral
    /// alignment (the leaving agent's belief no longer applies). Returns owned
    /// `f64` for the async path.
    fn leave_competences(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        ctx: &DecisionContext,
    ) -> (f64, f64) {
        let required = ctx.required_capabilities;
        let w = self.params.belief_weight;
        let agent_id = agent.agent_id();

        let union_in = union_caps(coalition);
        // Removes the agent by id; assumes coalition members have distinct
        // `agent_id`s (duplicates would drop every match and understate `union_out`).
        let without: Vec<&dyn AgentCapabilities> = coalition
            .iter()
            .filter(|a| a.agent_id() != agent_id)
            .copied()
            .collect();
        let union_out = union_caps(&without);

        let cov_in = coverage(union_in, required);
        let cov_out = coverage(union_out, required);

        let members_in = Self::member_ids(coalition, None);
        let align_in = self.alignment(agent_id, &members_in);

        (
            Self::blended_competence(cov_in, align_in, w),
            Self::blended_competence(cov_out, 0.5, w),
        )
    }
}

impl CoalitionDecisionPolicy for AifDecisionPolicy {
    /// Join iff joining lowers `G` by more than `join_margin`.
    ///
    /// `coalition` excludes `agent`. `margin = g_alone - g_coalition` is
    /// positive when the coalition's broader capability coverage lowers `G`.
    /// Equivalent to the engine's `decide_join` (`g_coalition < g_alone`) when
    /// `join_margin == 0`.
    fn should_join(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        ctx: &DecisionContext,
    ) -> Decision {
        let (comp_alone, comp_coalition) = self.join_competences(agent, coalition, ctx);
        Self::join_decision_from_competences(
            comp_alone,
            comp_coalition,
            self.params,
            self.join_margin,
        )
    }

    /// Leave iff staying does not lower `G`.
    ///
    /// `coalition` includes `agent`. `g_in` is `G` with the agent present,
    /// `g_out` is `G` after removing it. An agent that covers a required bit —
    /// or (with `belief_weight > 0`) is well-trusted/compatible with its partners
    /// — *raises* `G` when removed (`g_out > g_in`), so it stays; if removing it
    /// does not raise `G` (`g_out - g_in <= 0`) it is redundant and should leave.
    fn should_leave(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        ctx: &DecisionContext,
    ) -> Decision {
        let (comp_in, comp_out) = self.leave_competences(agent, coalition, ctx);
        Self::leave_decision_from_competences(comp_in, comp_out, self.params)
    }

    /// Async, runtime-friendly override of
    /// [`should_join`](Self::should_join).
    ///
    /// Reduces the borrowed agents (capability masks + agent ids) and beliefs to
    /// two owned competence `f64` in the sync prologue, then runs the CPU-bound
    /// expected-free-energy compute on the rayon pool via [`tokio_rayon::spawn`],
    /// so the tokio worker thread is not blocked. The belief lookups are cheap
    /// `HashMap` reads done in the prologue; only the EFE math is offloaded.
    /// Produces the same [`Decision`] as the sync
    /// [`should_join`](Self::should_join).
    ///
    /// `coalition` excludes `agent` (same convention as the sync method).
    fn should_join_async<'a>(
        &'a self,
        agent: &'a dyn AgentCapabilities,
        coalition: &'a [&'a dyn AgentCapabilities],
        ctx: &'a DecisionContext,
    ) -> Pin<Box<dyn Future<Output = Decision> + Send + 'a>> {
        // Snapshot to owned values in the sync prologue: the &dyn borrows are not
        // 'static and must not cross into the spawned closure or the future.
        let (comp_alone, comp_coalition) = self.join_competences(agent, coalition, ctx);
        let params = self.params;
        let join_margin = self.join_margin;

        Box::pin(async move {
            tokio_rayon::spawn(move || {
                Self::join_decision_from_competences(comp_alone, comp_coalition, params, join_margin)
            })
            .await
        })
    }

    /// Async, runtime-friendly override of
    /// [`should_leave`](Self::should_leave).
    ///
    /// Reduces the borrowed agents and beliefs to two owned competence `f64` in
    /// the sync prologue, then runs the CPU-bound expected-free-energy compute on
    /// the rayon pool via [`tokio_rayon::spawn`], so the tokio worker thread is
    /// not blocked. Produces the same [`Decision`] as the sync
    /// [`should_leave`](Self::should_leave).
    ///
    /// `coalition` includes `agent` (same convention as the sync method).
    fn should_leave_async<'a>(
        &'a self,
        agent: &'a dyn AgentCapabilities,
        coalition: &'a [&'a dyn AgentCapabilities],
        ctx: &'a DecisionContext,
    ) -> Pin<Box<dyn Future<Output = Decision> + Send + 'a>> {
        let (comp_in, comp_out) = self.leave_competences(agent, coalition, ctx);
        let params = self.params;

        Box::pin(async move {
            tokio_rayon::spawn(move || {
                Self::leave_decision_from_competences(comp_in, comp_out, params)
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

    /// B4 crux: verify the coverage→G mapping is monotone (higher coverage ⇒
    /// lower G) empirically, then check non-degeneracy and the degeneracy
    /// guards.
    #[test]
    fn efe_monotonic_in_coverage() {
        let params = BridgeParams::default();
        let g0 = CapabilityModel::efe_for_coverage(0.0, params).unwrap();
        let g_half = CapabilityModel::efe_for_coverage(0.5, params).unwrap();
        let g1 = CapabilityModel::efe_for_coverage(1.0, params).unwrap();

        // Print exact values for the report.
        tracing::info!(g0, g_half, g1, "EFE by coverage");
        eprintln!("efe_for_coverage: 0.0={g0} 0.5={g_half} 1.0={g1}");

        assert!(
            g1 < g_half && g_half < g0,
            "expected higher coverage ⇒ lower G, got 0.0={g0} 0.5={g_half} 1.0={g1}"
        );
    }

    #[test]
    fn coverage_helper() {
        assert!((coverage(0b001, 0b111) - 1.0 / 3.0).abs() < 1e-12);
        assert!((coverage(0b111, 0b111) - 1.0).abs() < 1e-12);
        assert!((coverage(0b000, 0b111) - 0.0).abs() < 1e-12);
        // Nothing required ⇒ fully covered.
        assert!((coverage(0b000, 0b000) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn join_is_non_degenerate() {
        let policy = AifDecisionPolicy::default();
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

        // Agent covers 1/3 alone; coalition union covers 3/3 ⇒ joining lowers G.
        let coalition: [&dyn AgentCapabilities; 2] = [&a1, &a2];
        let d = policy.should_join(&a0, &coalition, &ctx);
        assert!(
            d.act,
            "agent should join: coalition raises coverage 1/3 → 3/3 (score={})",
            d.score
        );
        assert!(d.score > 0.0, "join margin must be positive");
    }

    #[test]
    fn join_clone_is_degenerate_no_op() {
        let policy = AifDecisionPolicy::default();
        let ctx = DecisionContext {
            required_capabilities: 0b111,
        };

        let a0 = TestAgent {
            id: 0,
            caps: 0b001,
            trust: 50,
        };
        let clone = TestAgent {
            id: 1,
            caps: 0b001,
            trust: 50,
        };

        // Union coverage stays 1/3 — the clone adds no required coverage.
        let coalition: [&dyn AgentCapabilities; 1] = [&clone];
        let d = policy.should_join(&a0, &coalition, &ctx);
        assert!(
            !d.act,
            "joining a redundant clone must not help (score={})",
            d.score
        );
        assert!(d.score.abs() < 1e-9, "margin must be ≈ 0, got {}", d.score);
    }

    #[test]
    fn join_with_no_requirements_is_no_op() {
        let policy = AifDecisionPolicy::default();
        let ctx = DecisionContext {
            required_capabilities: 0,
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

        // required == 0 ⇒ coverage 1.0 both sides ⇒ no margin ⇒ no join.
        let coalition: [&dyn AgentCapabilities; 1] = [&a1];
        let d = policy.should_join(&a0, &coalition, &ctx);
        assert!(!d.act, "no requirements ⇒ no reason to join");
        assert!(d.score.abs() < 1e-9, "margin must be ≈ 0, got {}", d.score);
    }

    #[test]
    fn leave_when_redundant_else_stay() {
        let policy = AifDecisionPolicy::default();
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

        // a0 provides the only coverage of bit 0 ⇒ removing it raises G ⇒ stay.
        let full: [&dyn AgentCapabilities; 3] = [&a0, &a1, &a2];
        let stay = policy.should_leave(&a0, &full, &ctx);
        assert!(!stay.act, "agent providing unique coverage should stay");

        // A redundant clone of a0 contributes no new coverage ⇒ should leave.
        let redundant = TestAgent {
            id: 3,
            caps: 0b001,
            trust: 50,
        };
        let with_clone: [&dyn AgentCapabilities; 4] = [&a0, &a1, &a2, &redundant];
        let leave = policy.should_leave(&redundant, &with_clone, &ctx);
        assert!(leave.act, "redundant agent should leave (score={})", leave.score);
    }

    #[test]
    fn efe_value_calculator_orders_by_coverage() {
        let calc = EfeValueCalculator {
            required_capabilities: 0b111,
            params: BridgeParams::default(),
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

        let one: [&dyn AgentCapabilities; 1] = [&a0];
        let all: [&dyn AgentCapabilities; 3] = [&a0, &a1, &a2];
        // Higher coverage ⇒ lower G ⇒ higher -G value.
        assert!(calc.calculate_value(&all) > calc.calculate_value(&one));

        // Object-safety alongside the existing calculators.
        let _: &dyn ValueCalculator = &calc;
    }

    #[tokio::test]
    async fn should_join_async_matches_sync() {
        let policy = AifDecisionPolicy::default();
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

        // `should_join_async` is now the trait override; this still resolves on
        // the concrete type.
        let sync = policy.should_join(&a0, &coalition, &ctx);
        let asyncd = policy.should_join_async(&a0, &coalition, &ctx).await;

        assert_eq!(sync.act, asyncd.act, "act must match sync");
        assert!(
            (sync.score - asyncd.score).abs() < 1e-12,
            "score must match sync: sync={} async={}",
            sync.score,
            asyncd.score
        );
        // Sanity: this is the non-degenerate join case.
        assert!(asyncd.act && asyncd.score > 0.0);
    }

    #[tokio::test]
    async fn should_leave_async_matches_sync() {
        let policy = AifDecisionPolicy::default();
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

        // Unique-coverage stay case: a0 is the only provider of bit 0 ⇒ stay.
        let full: [&dyn AgentCapabilities; 3] = [&a0, &a1, &a2];
        let sync_stay = policy.should_leave(&a0, &full, &ctx);
        let async_stay = policy.should_leave_async(&a0, &full, &ctx).await;
        assert_eq!(sync_stay.act, async_stay.act, "stay: act must match sync");
        assert!(
            (sync_stay.score - async_stay.score).abs() < 1e-12,
            "stay: score must match sync: sync={} async={}",
            sync_stay.score,
            async_stay.score
        );
        assert!(!async_stay.act, "unique-coverage agent should stay");

        // Redundant-clone leave case: a clone of a0 adds no coverage ⇒ leave.
        let redundant = TestAgent {
            id: 3,
            caps: 0b001,
            trust: 50,
        };
        let with_clone: [&dyn AgentCapabilities; 4] = [&a0, &a1, &a2, &redundant];
        let sync_leave = policy.should_leave(&redundant, &with_clone, &ctx);
        let async_leave = policy.should_leave_async(&redundant, &with_clone, &ctx).await;
        assert_eq!(sync_leave.act, async_leave.act, "leave: act must match sync");
        assert!(
            (sync_leave.score - async_leave.score).abs() < 1e-12,
            "leave: score must match sync: sync={} async={}",
            sync_leave.score,
            async_leave.score
        );
        assert!(async_leave.act, "redundant agent should leave");
    }

    /// Regression test for the actual review finding: a caller holding a
    /// `Box<dyn CoalitionDecisionPolicy>` must reach the async (rayon-offloaded)
    /// path, not fall through to the blocking sync `should_join`.
    #[tokio::test]
    async fn async_path_reachable_through_trait_object() {
        let p: Box<dyn CoalitionDecisionPolicy> = Box::new(AifDecisionPolicy::default());
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

        // The dynamic-dispatch async call must produce the non-degenerate join
        // decision (coverage 1/3 → 3/3), proving the override is reached.
        let d = p.should_join_async(&a0, &coalition, &ctx).await;
        assert!(d.act, "trait-object async join should fire (score={})", d.score);
        assert!(d.score > 0.0, "join margin must be positive");

        // And the leave override is reachable too: a0 provides unique coverage.
        let full: [&dyn AgentCapabilities; 3] = [&a0, &a1, &a2];
        let stay = p.should_leave_async(&a0, &full, &ctx).await;
        assert!(!stay.act, "trait-object async leave: unique-coverage agent stays");
    }

    // -----------------------------------------------------------------------
    // Belief-aware behavior (issue #2): trust / compatibility / history fold
    // into the competence that drives the observation model.
    // -----------------------------------------------------------------------

    /// `BridgeParams` with the belief blend cranked up; everything else default.
    fn belief_params(belief_weight: f64) -> BridgeParams {
        BridgeParams {
            belief_weight,
            ..BridgeParams::default()
        }
    }

    /// Drive `agent`'s EMA trust toward `target` so the EMA settles near it.
    fn trust_toward(agent: AgentId, target: f64) -> TrustBeliefs {
        let mut t = TrustBeliefs::new();
        for _ in 0..200 {
            t.update(agent, target);
        }
        t
    }

    #[test]
    fn belief_weight_zero_matches_pure_coverage() {
        // A belief-aware policy with belief_weight == 0 must reproduce the
        // pure-coverage default exactly: a redundant clone does NOT join.
        let mut trust = TrustBeliefs::new();
        trust.update(1, 1.0);
        let mut compat = CompatibilityBeliefs::new();
        compat.set(0, 1, 1.0);
        let policy = AifDecisionPolicy::with_beliefs(
            belief_params(0.0),
            0.0,
            trust,
            compat,
            CoalitionHistory::new(),
        );
        let ctx = DecisionContext { required_capabilities: 0b111 };

        let a0 = TestAgent { id: 0, caps: 0b010, trust: 50 };
        let a1 = TestAgent { id: 1, caps: 0b010, trust: 50 };
        let coalition: [&dyn AgentCapabilities; 1] = [&a1];

        // Even with strong beliefs present, weight 0 ignores them ⇒ redundant
        // coverage ⇒ decline (identical to AifDecisionPolicy::default()).
        assert!(!policy.should_join(&a0, &coalition, &ctx).act);
        assert_eq!(
            policy.should_join(&a0, &coalition, &ctx),
            AifDecisionPolicy::default().should_join(&a0, &coalition, &ctx),
        );
    }

    #[test]
    fn high_trust_lets_capability_redundant_agent_join() {
        // a0 (caps 0b010) adds NO new coverage over partner a1 (caps 0b010), so
        // the pure-coverage policy declines. But high trust + compatibility makes
        // the partnership valuable, so the belief-aware policy joins.
        let policy = AifDecisionPolicy::with_beliefs(
            belief_params(0.9),
            0.0,
            trust_toward(1, 0.95),
            {
                let mut c = CompatibilityBeliefs::new();
                c.set(0, 1, 0.95);
                c
            },
            CoalitionHistory::new(),
        );
        let ctx = DecisionContext { required_capabilities: 0b111 };

        let a0 = TestAgent { id: 0, caps: 0b010, trust: 50 };
        let a1 = TestAgent { id: 1, caps: 0b010, trust: 50 };
        let coalition: [&dyn AgentCapabilities; 1] = [&a1];

        assert!(
            !AifDecisionPolicy::default().should_join(&a0, &coalition, &ctx).act,
            "pure coverage: redundant clone declines",
        );
        assert!(
            policy.should_join(&a0, &coalition, &ctx).act,
            "high trust+compat: redundant agent now joins",
        );
    }

    #[test]
    fn low_trust_blocks_a_coverage_improving_join() {
        // a0 (caps 0b001) DOES add new coverage (1/3 → 2/3), so pure coverage
        // joins. But the partner is badly distrusted/incompatible, so under a
        // high belief weight the agent prefers to act alone.
        let policy = AifDecisionPolicy::with_beliefs(
            belief_params(0.9),
            0.0,
            trust_toward(1, 0.05),
            {
                let mut c = CompatibilityBeliefs::new();
                c.set(0, 1, 0.05);
                c
            },
            CoalitionHistory::new(),
        );
        let ctx = DecisionContext { required_capabilities: 0b111 };

        let a0 = TestAgent { id: 0, caps: 0b001, trust: 50 };
        let a1 = TestAgent { id: 1, caps: 0b010, trust: 50 };
        let coalition: [&dyn AgentCapabilities; 1] = [&a1];

        assert!(
            AifDecisionPolicy::default().should_join(&a0, &coalition, &ctx).act,
            "pure coverage: new bit ⇒ join",
        );
        assert!(
            !policy.should_join(&a0, &coalition, &ctx).act,
            "low trust+compat blocks the otherwise-good join",
        );
    }

    #[test]
    fn recorded_history_flips_a_redundant_join() {
        // Neutral trust/compat (base alignment 0.5), redundant coverage ⇒ no
        // join. Recording strong past performance for {0,1} lifts alignment and
        // flips the decision to join.
        let ctx = DecisionContext { required_capabilities: 0b111 };
        let a0 = TestAgent { id: 0, caps: 0b010, trust: 50 };
        let a1 = TestAgent { id: 1, caps: 0b010, trust: 50 };
        let coalition: [&dyn AgentCapabilities; 1] = [&a1];

        let no_history = AifDecisionPolicy::with_beliefs(
            belief_params(0.9),
            0.0,
            TrustBeliefs::new(),
            CompatibilityBeliefs::new(),
            CoalitionHistory::new(),
        );
        assert!(
            !no_history.should_join(&a0, &coalition, &ctx).act,
            "neutral beliefs, redundant coverage ⇒ decline",
        );

        let mut hist = CoalitionHistory::new();
        hist.record(&[0, 1], 0.95);
        let with_history = AifDecisionPolicy::with_beliefs(
            belief_params(0.9),
            0.0,
            TrustBeliefs::new(),
            CompatibilityBeliefs::new(),
            hist,
        );
        assert!(
            with_history.should_join(&a0, &coalition, &ctx).act,
            "strong recorded history ⇒ join",
        );
    }

    #[test]
    fn trust_controls_whether_a_redundant_member_leaves() {
        // a0 is capability-redundant within {a0, a1} (both 0b010): pure coverage
        // says leave. High trust keeps it; low trust ejects it.
        let ctx = DecisionContext { required_capabilities: 0b111 };
        let a0 = TestAgent { id: 0, caps: 0b010, trust: 50 };
        let a1 = TestAgent { id: 1, caps: 0b010, trust: 50 };
        let full: [&dyn AgentCapabilities; 2] = [&a0, &a1];

        assert!(
            AifDecisionPolicy::default().should_leave(&a0, &full, &ctx).act,
            "pure coverage: redundant member leaves",
        );

        let trusted = AifDecisionPolicy::with_beliefs(
            belief_params(0.9),
            0.0,
            trust_toward(1, 0.95),
            {
                let mut c = CompatibilityBeliefs::new();
                c.set(0, 1, 0.95);
                c
            },
            CoalitionHistory::new(),
        );
        assert!(
            !trusted.should_leave(&a0, &full, &ctx).act,
            "high trust keeps the redundant member",
        );

        let distrusted = AifDecisionPolicy::with_beliefs(
            belief_params(0.9),
            0.0,
            trust_toward(1, 0.05),
            {
                let mut c = CompatibilityBeliefs::new();
                c.set(0, 1, 0.05);
                c
            },
            CoalitionHistory::new(),
        );
        assert!(
            distrusted.should_leave(&a0, &full, &ctx).act,
            "low trust ejects the redundant member",
        );
    }

    #[test]
    fn well_trusted_redundant_member_stays_in_larger_coalition() {
        // Design-intent guard (review finding, issue #2): the leave path is the
        // symmetric dual of join. `comp_out` uses neutral 0.5 ("agent not in the
        // coalition"), exactly as join's `comp_alone` does — so if high trust
        // lets a capability-redundant agent JOIN, the same trust must keep it
        // from being EJECTED. Here all three members share caps 0b010 (coverage
        // pinned at 1/3 regardless of size), so a pure-coverage policy would shed
        // any of them; high mutual trust keeps a0.
        let ctx = DecisionContext { required_capabilities: 0b111 };
        let a0 = TestAgent { id: 0, caps: 0b010, trust: 50 };
        let a1 = TestAgent { id: 1, caps: 0b010, trust: 50 };
        let a2 = TestAgent { id: 2, caps: 0b010, trust: 50 };
        let full: [&dyn AgentCapabilities; 3] = [&a0, &a1, &a2];

        // Pure coverage: a redundant member in a 3-clone coalition leaves.
        assert!(
            AifDecisionPolicy::default().should_leave(&a0, &full, &ctx).act,
            "pure coverage: redundant member of a 3-clone coalition leaves",
        );

        // a0 well-trusted by / compatible with both partners ⇒ stays.
        let mut trust = TrustBeliefs::new();
        for _ in 0..200 {
            trust.update(1, 0.95);
            trust.update(2, 0.95);
        }
        let mut compat = CompatibilityBeliefs::new();
        compat.set(0, 1, 0.95);
        compat.set(0, 2, 0.95);
        let trusted = AifDecisionPolicy::with_beliefs(
            belief_params(0.9),
            0.0,
            trust,
            compat,
            CoalitionHistory::new(),
        );
        assert!(
            !trusted.should_leave(&a0, &full, &ctx).act,
            "high mutual trust keeps the redundant member (dual of belief-aware join)",
        );

        // Low trust toward both partners ⇒ the redundant a0 is ejected.
        let mut low_trust = TrustBeliefs::new();
        for _ in 0..200 {
            low_trust.update(1, 0.05);
            low_trust.update(2, 0.05);
        }
        let mut low_compat = CompatibilityBeliefs::new();
        low_compat.set(0, 1, 0.05);
        low_compat.set(0, 2, 0.05);
        let distrusted = AifDecisionPolicy::with_beliefs(
            belief_params(0.9),
            0.0,
            low_trust,
            low_compat,
            CoalitionHistory::new(),
        );
        assert!(
            distrusted.should_leave(&a0, &full, &ctx).act,
            "low trust ejects the redundant member from the larger coalition",
        );
    }

    #[tokio::test]
    async fn belief_aware_sync_async_equivalent() {
        // The async (rayon-offloaded) path must reproduce the sync belief-aware
        // decision exactly, including through a trait object.
        let policy = AifDecisionPolicy::with_beliefs(
            belief_params(0.9),
            0.0,
            trust_toward(1, 0.95),
            {
                let mut c = CompatibilityBeliefs::new();
                c.set(0, 1, 0.95);
                c
            },
            CoalitionHistory::new(),
        );
        let ctx = DecisionContext { required_capabilities: 0b111 };
        let a0 = TestAgent { id: 0, caps: 0b010, trust: 50 };
        let a1 = TestAgent { id: 1, caps: 0b010, trust: 50 };
        let coalition: [&dyn AgentCapabilities; 1] = [&a1];

        let sync = policy.should_join(&a0, &coalition, &ctx);
        let r#async = policy.should_join_async(&a0, &coalition, &ctx).await;
        assert_eq!(sync, r#async, "belief-aware async join == sync");
    }
}
