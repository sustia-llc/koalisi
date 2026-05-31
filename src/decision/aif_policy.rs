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
    /// Coverage→`G` over owned params (no `&self` borrow), so it can run inside
    /// a `'static` rayon closure. Engine error ⇒ `+∞` (worst outcome).
    fn g_at(cov: f64, params: BridgeParams) -> f64 {
        match CapabilityModel::efe_for_coverage(cov, params) {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!(error = %e, coverage = cov, "EFE computation failed");
                f64::INFINITY
            }
        }
    }

    /// Pure join decision over owned capability masks.
    ///
    /// Shared by the sync [`CoalitionDecisionPolicy::should_join`] and the async
    /// [`CoalitionDecisionPolicy::should_join_async`] override: both reduce the
    /// borrowed agents to owned `u32` masks and call this. `union_with_agent` is
    /// the unioned capability mask of the coalition *including* the candidate
    /// agent.
    fn join_decision_from_masks(
        agent_caps: u32,
        union_with_agent: u32,
        required: u32,
        params: BridgeParams,
        join_margin: f64,
    ) -> Decision {
        let cov_alone = coverage(agent_caps, required);
        let cov_coalition = coverage(union_with_agent, required);
        let g_alone = Self::g_at(cov_alone, params);
        let g_coalition = Self::g_at(cov_coalition, params);
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

    /// Pure leave decision over owned capability masks.
    ///
    /// Shared by the sync [`CoalitionDecisionPolicy::should_leave`] and the async
    /// [`CoalitionDecisionPolicy::should_leave_async`] override: both reduce the
    /// borrowed agents to owned `u32` masks and call this. `union_in` is the
    /// unioned capability mask of the coalition *including* the agent; `union_out`
    /// is the union after removing the agent (by id).
    fn leave_decision_from_masks(
        union_in: u32,
        union_out: u32,
        required: u32,
        params: BridgeParams,
    ) -> Decision {
        let cov_in = coverage(union_in, required);
        let cov_out = coverage(union_out, required);
        let g_in = Self::g_at(cov_in, params);
        let g_out = Self::g_at(cov_out, params);
        // `g_out - g_in` is how much removing the agent raises G. If it does not
        // raise G (<= 0), the agent contributes no coverage and should leave.
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
        let agent_caps = agent.capabilities();
        let union_with_agent = union_caps(coalition) | agent_caps;
        Self::join_decision_from_masks(
            agent_caps,
            union_with_agent,
            ctx.required_capabilities,
            self.params,
            self.join_margin,
        )
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
        let union_in = union_caps(coalition);

        // Removes the agent by id; assumes coalition members have distinct
        // `agent_id`s (duplicates would drop every match and understate `union_out`).
        let without: Vec<&dyn AgentCapabilities> = coalition
            .iter()
            .filter(|a| a.agent_id() != agent.agent_id())
            .copied()
            .collect();
        let union_out = union_caps(&without);

        Self::leave_decision_from_masks(
            union_in,
            union_out,
            ctx.required_capabilities,
            self.params,
        )
    }

    /// Async, runtime-friendly override of
    /// [`should_join`](Self::should_join).
    ///
    /// Snapshots the borrowed agents' capability masks to owned `u32` *before*
    /// spawning, then runs the CPU-bound expected-free-energy compute on the
    /// rayon pool via [`tokio_rayon::spawn`], so the tokio worker thread is not
    /// blocked. Produces the same [`Decision`] as the sync
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
        let agent_caps = agent.capabilities();
        let union_with_agent = union_caps(coalition) | agent_caps;
        let required = ctx.required_capabilities;
        let params = self.params;
        let join_margin = self.join_margin;

        Box::pin(async move {
            tokio_rayon::spawn(move || {
                Self::join_decision_from_masks(
                    agent_caps,
                    union_with_agent,
                    required,
                    params,
                    join_margin,
                )
            })
            .await
        })
    }

    /// Async, runtime-friendly override of
    /// [`should_leave`](Self::should_leave).
    ///
    /// Snapshots the in/out unioned capability masks to owned `u32` *before*
    /// spawning, then runs the CPU-bound expected-free-energy compute on the
    /// rayon pool via [`tokio_rayon::spawn`], so the tokio worker thread is not
    /// blocked. Produces the same [`Decision`] as the sync
    /// [`should_leave`](Self::should_leave).
    ///
    /// `coalition` includes `agent` (same convention as the sync method).
    fn should_leave_async<'a>(
        &'a self,
        agent: &'a dyn AgentCapabilities,
        coalition: &'a [&'a dyn AgentCapabilities],
        ctx: &'a DecisionContext,
    ) -> Pin<Box<dyn Future<Output = Decision> + Send + 'a>> {
        // Snapshot owned masks in the sync prologue, mirroring the sync
        // `should_leave`: union with the agent, and union after removing it by id.
        let union_in = union_caps(coalition);
        let union_out = union_caps(
            &coalition
                .iter()
                .filter(|a| a.agent_id() != agent.agent_id())
                .copied()
                .collect::<Vec<_>>(),
        );
        let required = ctx.required_capabilities;
        let params = self.params;

        Box::pin(async move {
            tokio_rayon::spawn(move || {
                Self::leave_decision_from_masks(union_in, union_out, required, params)
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
}
