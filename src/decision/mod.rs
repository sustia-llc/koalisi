//! Coalition join/leave decision policies.
//!
//! A [`CoalitionDecisionPolicy`] answers two questions for a single agent:
//! should it *join* a coalition it is not yet a member of, and should it
//! *leave* a coalition it currently belongs to. The trait is object-safe so
//! policies can be swapped behind a `Box<dyn CoalitionDecisionPolicy>`.
//!
//! Two implementations are provided:
//!
//! - [`ThresholdPolicy`] (always available) — decides on the *marginal value*
//!   an agent contributes, measured by any existing
//!   [`ValueCalculator`](crate::algorithms::ValueCalculator).
//! - [`AifDecisionPolicy`] (feature `decision`) — decides via expected free
//!   energy from the Active Inference engine, where coalition membership
//!   changes the agent's *observation model* (capability coverage of the
//!   required capabilities). Higher coverage lowers expected free energy `G`.
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

/// Outcome of a join/leave decision.
///
/// `act` is the boolean recommendation (join, or leave, depending on which
/// method produced it). `score` is the underlying scalar the decision was made
/// from (a marginal value, or an expected-free-energy margin) — exposed so
/// callers can rank candidates or apply their own thresholds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Decision {
    pub act: bool,
    pub score: f64,
}

/// Context for a decision, independent of the candidate agent or coalition.
///
/// `required_capabilities` is a capability bitmask describing what the task /
/// coalition needs covered. Policies that reason about capability coverage
/// (e.g. [`AifDecisionPolicy`]) use it; value-only policies such as
/// [`ThresholdPolicy`] currently ignore it (the base
/// [`ValueCalculator`](crate::algorithms::ValueCalculator)s do not consult it).
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
}

/// Marginal-value decision policy backed by a
/// [`ValueCalculator`](crate::algorithms::ValueCalculator).
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

        let without: Vec<&dyn AgentCapabilities> = coalition
            .iter()
            .filter(|a| a.agent_id() != agent.agent_id())
            .copied()
            .collect();
        let value_without = self.calculator.calculate_value(&without);

        let marginal_of_staying = value_with - value_without;
        Decision {
            act: marginal_of_staying < self.leave_threshold,
            score: marginal_of_staying,
        }
    }
}

#[cfg(feature = "decision")]
mod aif_policy;
#[cfg(feature = "decision")]
pub use aif_policy::{AifDecisionPolicy, BridgeParams, CapabilityModel, EfeValueCalculator};

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
}
