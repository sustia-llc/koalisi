//! Coalition join/leave decision policies.
//!
//! A [`CoalitionDecisionPolicy`] answers two questions for a single agent:
//! should it *join* a coalition it is not yet a member of, and should it
//! *leave* a coalition it currently belongs to. The trait is object-safe so
//! policies can be swapped behind a `Box<dyn CoalitionDecisionPolicy>`.
//!
//! Policy implementations:
//!
//! - [`ThresholdPolicy`] (always available) — decides on the *marginal value*
//!   an agent contributes, measured by any existing
//!   [`ValueCalculator`].
//! - `AifDecisionPolicy` (feature `decision`) — decides via expected free
//!   energy from the Active Inference engine, where coalition membership
//!   changes the agent's *observation model* (capability coverage of the
//!   required capabilities). Higher coverage lowers expected free energy `G`.
//!   Two further AIF arms live beside it: `AifMmDecisionPolicy` (the K4-v3
//!   multimodal bridge) and `PersistentAifArm` (the K4-v4/v5 persistent
//!   world-model arm).
//! - `MagnitudePolicy` (feature `magnitude`) — the categorical A/B mirror of the
//!   AIF arm: it scores a coalition by its *magnitude* (effective-member
//!   diversity, via `catgraph`'s enriched-category magnitude) instead of `−G`,
//!   mapping capability coverage onto directed substitutability couplings. The
//!   two feature arms are independent — either, both, or neither may be enabled.
//!
//! The module also hosts one non-policy item: `ReliabilityCoverage` (feature
//! `decision`, issue #57) — a [`ValueCalculator`] derived from the persistent
//! arm's world-model snapshot, giving the population structure-search
//! (`crate::algorithms::search`) a reliability-weighted fitness. It lives here,
//! not in `algorithms`, because its snapshot constructor needs the
//! feature-gated AIF types.
//!
//! # Membership conventions
//!
//! The `coalition` slice argument has a different meaning per method:
//!
//! - [`CoalitionDecisionPolicy::should_join`] — `coalition` is the set of
//!   current members **excluding** `agent`. `act == true` means *join*.
//! - [`CoalitionDecisionPolicy::should_leave`] — `coalition` is the set of
//!   current members **including** `agent`. `act == true` means *leave*.

use crate::algorithms::{AgentCapabilities, ValueCalculator};
use std::future::Future;
use std::pin::Pin;

/// Outcome of a join/leave decision.
///
/// `act` is the boolean recommendation (join, or leave, depending on which
/// method produced it). `score` is the underlying scalar the decision was made
/// from (a marginal value, an expected-free-energy margin, or a
/// coalition-magnitude margin) — exposed so callers can rank candidates or
/// apply their own thresholds. Scores are only comparable *within* one policy;
/// the decision arms are deliberately not calibrated against each other.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Decision {
    pub act: bool,
    pub score: f64,
}

/// Context for a decision, independent of the candidate agent or coalition.
///
/// `required_capabilities` is a capability bitmask describing what the task /
/// coalition needs covered. Policies that reason about capability coverage use
/// it (`AifDecisionPolicy`, feature `decision`; `MagnitudePolicy`, feature
/// `magnitude`); value-only policies such as [`ThresholdPolicy`] currently
/// ignore it (the base [`ValueCalculator`]s do not consult it).
#[derive(Debug, Clone, Copy, Default)]
pub struct DecisionContext {
    pub required_capabilities: u32,
}

/// A strategy for deciding whether an agent should join or leave a coalition.
///
/// See the [module docs](self) for the per-method membership conventions.
pub trait CoalitionDecisionPolicy: Send + Sync {
    /// Decide whether `agent` should join `coalition`.
    ///
    /// `coalition` = current members **excluding** `agent`. `act == true`
    /// means the agent should join.
    fn should_join(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        ctx: &DecisionContext,
    ) -> Decision;

    /// Decide whether `agent` should leave `coalition`.
    ///
    /// `coalition` = current members **including** `agent`. `act == true`
    /// means the agent should leave.
    fn should_leave(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        ctx: &DecisionContext,
    ) -> Decision;

    /// Async, runtime-friendly variant of [`should_join`](Self::should_join).
    ///
    /// The default impl runs the sync method inline and returns a ready future —
    /// correct for cheap policies. Policies whose decision is CPU-heavy (e.g.
    /// the AIF expected-free-energy bridge) override this to offload the work to a
    /// thread pool so the async runtime is not blocked. Dyn-compatible: returns a
    /// boxed future rather than using `async fn` in trait position.
    ///
    /// `coalition` excludes `agent` (same convention as
    /// [`should_join`](Self::should_join)).
    fn should_join_async<'a>(
        &'a self,
        agent: &'a dyn AgentCapabilities,
        coalition: &'a [&'a dyn AgentCapabilities],
        ctx: &'a DecisionContext,
    ) -> Pin<Box<dyn Future<Output = Decision> + Send + 'a>> {
        // Compute the decision in the synchronous prologue and move only the
        // owned (Copy) `Decision` into the future. The `&dyn` borrows are
        // `Send` (`AgentCapabilities: Send + Sync`) but not `'static`, so an
        // overriding impl that offloads to a thread pool (see `AifDecisionPolicy`)
        // must still snapshot to owned values rather than capture the borrows.
        let decision = self.should_join(agent, coalition, ctx);
        Box::pin(async move { decision })
    }

    /// Async, runtime-friendly variant of [`should_leave`](Self::should_leave).
    ///
    /// The default impl runs the sync method inline and returns a ready future —
    /// correct for cheap policies. Policies whose decision is CPU-heavy override
    /// this to offload the work to a thread pool. Dyn-compatible: returns a boxed
    /// future rather than using `async fn` in trait position.
    ///
    /// `coalition` includes `agent` (same convention as
    /// [`should_leave`](Self::should_leave)).
    fn should_leave_async<'a>(
        &'a self,
        agent: &'a dyn AgentCapabilities,
        coalition: &'a [&'a dyn AgentCapabilities],
        ctx: &'a DecisionContext,
    ) -> Pin<Box<dyn Future<Output = Decision> + Send + 'a>> {
        // See `should_join_async`: compute synchronously, move only the owned
        // `Decision` into the future to keep it `Send`.
        let decision = self.should_leave(agent, coalition, ctx);
        Box::pin(async move { decision })
    }
}

/// Marginal-value decision policy backed by a
/// [`ValueCalculator`].
///
/// The agent joins when the *marginal value* it adds to the coalition is at
/// least `join_threshold`, and leaves when the marginal value it currently
/// contributes drops below `leave_threshold`.
///
/// `DecisionContext` is accepted for trait conformance but ignored — the base
/// value calculators do not consult `required_capabilities`.
#[derive(Debug, Clone)]
pub struct ThresholdPolicy<C: ValueCalculator + Send + Sync> {
    pub calculator: C,
    pub join_threshold: f64,
    pub leave_threshold: f64,
}

impl<C: ValueCalculator + Send + Sync> ThresholdPolicy<C> {
    /// Construct a policy from a value calculator and the join/leave thresholds.
    pub fn new(calculator: C, join_threshold: f64, leave_threshold: f64) -> Self {
        Self {
            calculator,
            join_threshold,
            leave_threshold,
        }
    }
}

impl<C: ValueCalculator + Send + Sync> CoalitionDecisionPolicy for ThresholdPolicy<C> {
    /// Join iff `V(coalition + agent) - V(coalition) >= join_threshold`.
    ///
    /// `coalition` excludes `agent` (see [module docs](self)).
    fn should_join(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        _ctx: &DecisionContext,
    ) -> Decision {
        let value_without = self.calculator.calculate_value(coalition);

        let mut with: Vec<&dyn AgentCapabilities> = coalition.to_vec();
        with.push(agent);
        let value_with = self.calculator.calculate_value(&with);

        let marginal = value_with - value_without;
        // A non-finite marginal means a calculator failed (e.g. EfeValueCalculator
        // returning ±∞ on engine error); never join on a value we can't trust, and
        // never propagate NaN/±∞ as a score.
        if !marginal.is_finite() {
            return Decision {
                act: false,
                score: 0.0,
            };
        }
        Decision {
            act: marginal >= self.join_threshold,
            score: marginal,
        }
    }

    /// Leave iff the agent's marginal contribution to staying is below
    /// `leave_threshold`, i.e.
    /// `V(coalition) - V(coalition - agent) < leave_threshold`.
    ///
    /// `coalition` includes `agent` (see [module docs](self)).
    fn should_leave(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        _ctx: &DecisionContext,
    ) -> Decision {
        let value_with = self.calculator.calculate_value(coalition);

        // Removes the agent by id; assumes coalition members have distinct
        // `agent_id`s (duplicates would drop every match).
        let without: Vec<&dyn AgentCapabilities> = coalition
            .iter()
            .filter(|a| a.agent_id() != agent.agent_id())
            .copied()
            .collect();
        let value_without = self.calculator.calculate_value(&without);

        let marginal_of_staying = value_with - value_without;
        // Non-finite ⇒ a calculator failed; don't eject on an untrustworthy value
        // and don't propagate NaN/±∞.
        if !marginal_of_staying.is_finite() {
            return Decision {
                act: false,
                score: 0.0,
            };
        }
        Decision {
            act: marginal_of_staying < self.leave_threshold,
            score: marginal_of_staying,
        }
    }
}

#[cfg(feature = "decision")]
mod aif_policy;
#[cfg(feature = "decision")]
pub use aif_policy::{
    AifDecisionPolicy, BridgeParams, CapabilityModel, CoalitionHistory, CompatibilityBeliefs,
    EfeValueCalculator, TrustBeliefs,
};

#[cfg(feature = "decision")]
mod aif_mm_policy;
#[cfg(feature = "decision")]
pub use aif_mm_policy::{AifMmDecisionPolicy, MmEfeValueCalculator, MultiModalModel};

#[cfg(feature = "decision")]
mod aif_persistent_policy;
#[cfg(feature = "decision")]
pub use aif_persistent_policy::{
    PersistentAifArm, PersistentAifConfig, PersistentAifState, TrialBoundary,
};

#[cfg(feature = "decision")]
mod reliability_value;
#[cfg(feature = "decision")]
pub use reliability_value::ReliabilityCoverage;

#[cfg(feature = "magnitude")]
mod magnitude_policy;
#[cfg(feature = "magnitude")]
pub use magnitude_policy::{CouplingModel, MagnitudePolicy, MagnitudeValueCalculator};
#[cfg(feature = "magnitude")]
pub(crate) use magnitude_policy::{magnitude_or_zero, relevant_masks};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algorithms::AdditiveCalculator;

    /// Minimal `AgentCapabilities` impl for testing decision policies.
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

    #[test]
    fn threshold_policy_join_leave_and_object_safety() {
        // Object-safety: the policy must be usable behind a trait object.
        let _: Box<dyn CoalitionDecisionPolicy> =
            Box::new(ThresholdPolicy::new(AdditiveCalculator, 0.0, 0.0));

        let policy = ThresholdPolicy::new(AdditiveCalculator, 0.0, 0.0);
        let ctx = DecisionContext::default();

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

        // Joining a non-empty coalition adds positive additive value, so a
        // threshold of 0.0 means "join".
        let coalition: [&dyn AgentCapabilities; 1] = [&a1];
        let join = policy.should_join(&a0, &coalition, &ctx);
        assert!(join.act, "agent should join (positive marginal value)");
        assert!(join.score > 0.0, "marginal value must be positive");

        // Leaving: the coalition includes the agent. Its marginal contribution
        // is positive (> leave_threshold of 0.0), so it should NOT leave.
        let full: [&dyn AgentCapabilities; 2] = [&a0, &a1];
        let leave = policy.should_leave(&a0, &full, &ctx);
        assert!(!leave.act, "agent with positive contribution stays");
        assert!(leave.score > 0.0, "staying contribution must be positive");
    }

    #[test]
    fn threshold_policy_respects_high_threshold() {
        // A join_threshold above any achievable marginal blocks joining.
        let policy = ThresholdPolicy::new(AdditiveCalculator, 1.0e9, 0.0);
        let ctx = DecisionContext::default();

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
        let coalition: [&dyn AgentCapabilities; 1] = [&a1];
        let join = policy.should_join(&a0, &coalition, &ctx);
        assert!(!join.act, "unreachable threshold blocks join");

        // A high leave_threshold forces leaving even a valuable position.
        let leave_policy = ThresholdPolicy::new(AdditiveCalculator, 0.0, 1.0e9);
        let full: [&dyn AgentCapabilities; 2] = [&a0, &a1];
        let leave = leave_policy.should_leave(&a0, &full, &ctx);
        assert!(leave.act, "high leave threshold forces leave");
    }

    #[tokio::test]
    async fn threshold_default_async_matches_sync() {
        // The default (non-overridden) async methods must reproduce the sync
        // results exactly, including through a `Box<dyn ...>` trait object.
        let policy: Box<dyn CoalitionDecisionPolicy> =
            Box::new(ThresholdPolicy::new(AdditiveCalculator, 0.0, 0.0));
        let ctx = DecisionContext::default();

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

        let coalition: [&dyn AgentCapabilities; 1] = [&a1];
        let sync_join = policy.should_join(&a0, &coalition, &ctx);
        let async_join = policy.should_join_async(&a0, &coalition, &ctx).await;
        assert_eq!(sync_join, async_join, "default should_join_async == sync");

        let full: [&dyn AgentCapabilities; 2] = [&a0, &a1];
        let sync_leave = policy.should_leave(&a0, &full, &ctx);
        let async_leave = policy.should_leave_async(&a0, &full, &ctx).await;
        assert_eq!(
            sync_leave, async_leave,
            "default should_leave_async == sync"
        );
    }
}
