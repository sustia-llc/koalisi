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
//! Contrast with `aif::CoalitionEvaluator`, whose `observation_probs` cannot
//! see coalition members and so can only vary *preferences* by membership —
//! which collapses to `G ≈ 0` for every coalition. We therefore build
//! [`aif::POMDPAgent`] directly here.

use crate::algorithms::{AgentCapabilities, ValueCalculator};

use super::{CoalitionDecisionPolicy, Decision, DecisionContext};

/// Tunable parameters mapping capability coverage onto a POMDP and its `G`.
#[derive(Debug, Clone, Copy)]
pub struct BridgeParams {
    /// Observation-model precision at full coverage. In `(0.5, 1.0)`.
    pub max_precision: f64,
    /// Preference mass placed on observation 1 ("success"). In `(0.5, 1.0)`.
    pub success_preference: f64,
    /// Action precision passed to the POMDP agent.
    pub alpha: f64,
}

impl Default for BridgeParams {
    fn default() -> Self {
        Self {
            max_precision: 0.95,
            success_preference: 0.9,
            alpha: 8.0,
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
    /// Build a 2-state / 2-observation POMDP at the given coverage and return
    /// its expected free energy `G` (lower = better).
    ///
    /// Precision `p = 0.5 + (max_precision - 0.5) * cov`, so `cov == 0` gives an
    /// uninformative observation model (`p = 0.5`) and `cov == 1` gives the most
    /// informative one (`p = max_precision`). State 0 ("success") emits
    /// observation 1 with probability `p`; state 1 ("fail") emits observation 1
    /// with probability `1 - p`.
    ///
    /// # Errors
    ///
    /// Returns [`aif::OneManyError`] if the constructed POMDP parameters are
    /// rejected by the engine.
    pub fn efe_for_coverage(cov: f64, params: BridgeParams) -> Result<f64, aif::OneManyError> {
        let p = 0.5 + (params.max_precision - 0.5) * cov;
        let obs = vec![p, 1.0 - p];
        let prefs = vec![params.success_preference, 1.0 - params.success_preference];
        let agent = aif::POMDPAgent::new(2, Some(obs), None, prefs, None, params.alpha, false)?;
        Ok(agent.expected_free_energy())
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
#[derive(Debug, Clone, Copy)]
pub struct AifDecisionPolicy {
    pub params: BridgeParams,
    pub join_margin: f64,
}

impl Default for AifDecisionPolicy {
    fn default() -> Self {
        Self {
            params: BridgeParams::default(),
            join_margin: 0.0,
        }
    }
}

impl AifDecisionPolicy {
    /// Compute `G` at the given coverage, treating an engine error as the
    /// worst possible outcome (`+∞`).
    fn g(&self, cov: f64) -> f64 {
        match CapabilityModel::efe_for_coverage(cov, self.params) {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!(error = %e, coverage = cov, "EFE computation failed");
                f64::INFINITY
            }
        }
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
        let cov_alone = coverage(agent.capabilities(), ctx.required_capabilities);

        let mut with: Vec<&dyn AgentCapabilities> = coalition.to_vec();
        with.push(agent);
        let cov_coalition = coverage(union_caps(&with), ctx.required_capabilities);

        let g_alone = self.g(cov_alone);
        let g_coalition = self.g(cov_coalition);
        let margin = g_alone - g_coalition;
        Decision {
            act: margin > self.join_margin,
            score: margin,
        }
    }

    /// Leave iff staying does not lower `G`.
    ///
    /// `coalition` includes `agent`. `g_in` is `G` with the agent present,
    /// `g_out` is `G` after removing it. Removing the agent that covers a
    /// required bit *raises* `G` (`g_out > g_in`), so the agent stays; if
    /// removing it does not raise `G` (`g_out - g_in <= 0`), the agent is
    /// redundant and should leave.
    fn should_leave(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        ctx: &DecisionContext,
    ) -> Decision {
        let cov_in = coverage(union_caps(coalition), ctx.required_capabilities);

        let without: Vec<&dyn AgentCapabilities> = coalition
            .iter()
            .filter(|a| a.agent_id() != agent.agent_id())
            .copied()
            .collect();
        let cov_out = coverage(union_caps(&without), ctx.required_capabilities);

        let g_in = self.g(cov_in);
        let g_out = self.g(cov_out);
        // `g_out - g_in` is how much removing the agent raises G. If it does not
        // raise G (<= 0), the agent contributes no coverage and should leave.
        let delta = g_out - g_in;
        Decision {
            act: delta <= 0.0,
            score: delta,
        }
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
}
