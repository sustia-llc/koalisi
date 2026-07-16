//! Multi-modality Active Inference coalition-decision strategy (feature
//! `decision`), the `aif-mm` arm of pre-registration K4-v3
//! (`docs/prereg-K4v3-multimodal-aif.md`).
//!
//! Where [`AifDecisionPolicy`](super::AifDecisionPolicy) collapses capability
//! coverage to a single scalar competence and feeds it to
//! [`aif::competence_efe`], this arm builds a **multi-modality** POMDP directly:
//! one 2×2 observation modality per required capability bit, each modality's
//! precision set by whether the coalition covers *that specific bit*. The
//! coverage structure therefore enters the expected free energy `G` per bit
//! rather than as one aggregate fraction (the K4-v3 hypothesis: the scalar
//! bridge is diversity-blind).
//!
//! # Registered specification (no degrees of freedom)
//!
//! For required bits `R` and a candidate coalition `S`, the POMDP is:
//! - one 2×2 modality per required bit `b`, `A[b]` column `j = [p_b, 1−p_b]`
//!   (the exact `competence_efe` A-shape);
//! - per-bit precision `p_b = 0.5 + (max_precision − 0.5)·cov_b`, with `cov_b = 1`
//!   iff any member of `S` covers bit `b` (binary union coverage), else `0`;
//! - a single 2-state hidden factor, deterministic 2-control `B` (the
//!   `transition_noise = 0` form), uniform `D`, per-modality preferences
//!   `[success_preference, 1 − success_preference]`;
//! - `AgentParams::default()` plus the arm's `α` (MeanField, depth 1, no learning,
//!   no precision dynamics).
//!
//! `G` is the **raw sum over the `r` modalities** (deliberately not normalized by
//! `r` — decisions are within-task comparisons, so the scale cancels). The value
//! is `−G`; the join/leave rule is identical to the scalar arm.

use std::future::Future;
use std::pin::Pin;

use nalgebra::DMatrix;

use crate::algorithms::{AgentCapabilities, ValueCalculator};

use super::{BridgeParams, CoalitionDecisionPolicy, Decision, DecisionContext};

/// Union of the capability bitmasks of `agents`.
fn union_caps(agents: &[&dyn AgentCapabilities]) -> u32 {
    agents.iter().fold(0u32, |acc, a| acc | a.capabilities())
}

/// The multi-modality capability→EFE bridge. Public so the mapping can be tested
/// directly, mirroring [`super::CapabilityModel`].
#[derive(Debug, Clone, Copy, Default)]
pub struct MultiModalModel;

impl MultiModalModel {
    /// Expected free energy `G` (lower = better) of the multi-modality POMDP for
    /// task-required bits `required` under union coverage mask `covered`.
    ///
    /// One 2×2 modality per set bit of `required`, each at precision
    /// `0.5 + (max_precision − 0.5)·cov_b` where `cov_b ∈ {0, 1}` is that bit's
    /// binary union coverage. `G` is the raw sum over modalities (the engine's
    /// `expected_free_energy`, which already accumulates pragmatic value across
    /// modalities). `required == 0` yields a single fully-covered modality — a
    /// constant `G` regardless of membership — mirroring the scalar arm's
    /// `coverage(_, 0) == 1.0`.
    ///
    /// # Errors
    ///
    /// Returns [`aif::AifError`] if the constructed POMDP is rejected by the
    /// engine (e.g. a degenerate parameter in `params`).
    pub fn efe_for_coverage(
        required: u32,
        covered: u32,
        params: BridgeParams,
    ) -> Result<f64, aif::AifError> {
        let mp = params.max_precision;
        let sp = params.success_preference;

        // Per-modality precision, one entry per required bit (little-endian bit
        // order; the ordering is immaterial — preferences are identical across
        // modalities and `G` is a symmetric sum).
        let precisions: Vec<f64> = if required == 0 {
            vec![mp]
        } else {
            (0..u32::BITS)
                .filter(|i| required & (1u32 << i) != 0)
                .map(|i| {
                    let cov_b = f64::from(u32::from(covered & (1u32 << i) != 0));
                    0.5 + (mp - 0.5) * cov_b
                })
                .collect()
        };

        // A[b]: column j = [p_b, 1 − p_b] (state 0 has precision p_b, state 1 has
        // 1 − p_b) — the exact `competence_efe` A-shape.
        let a: Vec<DMatrix<f64>> = precisions
            .iter()
            .map(|&p| DMatrix::from_vec(2, 2, vec![p, 1.0 - p, 1.0 - p, p]))
            .collect();
        let n_mod = a.len();

        // Deterministic 2-control B (the `transition_noise = 0` form): control `u`
        // sends all mass to state `u` regardless of source, matching the B that
        // `POMDPAgent::new(2, …)` / `competence_efe` build.
        let b: Vec<DMatrix<f64>> = (0..2)
            .map(|u| {
                let mut m = DMatrix::zeros(2, 2);
                m.row_mut(u).fill(1.0);
                m
            })
            .collect();

        let prefs = vec![sp, 1.0 - sp];
        let model = aif::GenerativeModel {
            a,
            b: vec![b],
            c: vec![prefs; n_mod],
            d: vec![vec![0.5, 0.5]],
        };
        let agent = aif::POMDPAgent::from_model(
            model,
            aif::AgentParams {
                alpha: params.alpha,
                ..Default::default()
            },
        )?;
        Ok(agent.expected_free_energy())
    }
}

/// A [`ValueCalculator`] scoring a coalition by its (negated) multi-modality
/// expected free energy. Mirrors [`super::EfeValueCalculator`]; the value is `−G`
/// (higher is better) and engine error logs and returns [`f64::NEG_INFINITY`].
#[derive(Debug, Clone, Copy)]
pub struct MmEfeValueCalculator {
    pub required_capabilities: u32,
    pub params: BridgeParams,
}

impl ValueCalculator for MmEfeValueCalculator {
    fn calculate_value(&self, agents: &[&dyn AgentCapabilities]) -> f64 {
        let covered = union_caps(agents);
        match MultiModalModel::efe_for_coverage(self.required_capabilities, covered, self.params) {
            Ok(g) => -g,
            Err(e) => {
                tracing::warn!(error = %e, covered, "multi-modality EFE value calculation failed");
                f64::NEG_INFINITY
            }
        }
    }
}

/// Multi-modality Active Inference join/leave policy (`aif-mm`, prereg K4-v3).
///
/// The decision rule is **identical** to [`super::AifDecisionPolicy`]: join iff
/// joining lowers `G` by more than `join_margin`; leave iff staying does not lower
/// `G`. Only the value model differs — per-bit modalities instead of a scalar
/// competence. Pure capability coverage: unlike the scalar arm this carries no
/// belief blend (the registered arm uses `belief_weight = 0`).
#[derive(Debug, Clone, Copy)]
pub struct AifMmDecisionPolicy {
    pub params: BridgeParams,
    pub join_margin: f64,
}

impl Default for AifMmDecisionPolicy {
    /// The registered `aif-mm` arm: `BridgeParams::default()`, `join_margin = 0`.
    fn default() -> Self {
        Self {
            params: BridgeParams::default(),
            join_margin: 0.0,
        }
    }
}

impl AifMmDecisionPolicy {
    /// `G` for `covered` under `required`; engine error ⇒ `+∞` (worst outcome),
    /// so it can run inside a `'static` rayon closure without borrowing `&self`.
    fn g_at(required: u32, covered: u32, params: BridgeParams) -> f64 {
        match MultiModalModel::efe_for_coverage(required, covered, params) {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!(error = %e, required, covered, "multi-modality EFE computation failed");
                f64::INFINITY
            }
        }
    }

    /// Pure join decision over owned coverage masks (shared by the sync and async
    /// paths). Join iff joining lowers `G` by more than `join_margin`.
    fn join_decision(
        required: u32,
        alone: u32,
        coalition: u32,
        params: BridgeParams,
        join_margin: f64,
    ) -> Decision {
        let g_alone = Self::g_at(required, alone, params);
        let g_coalition = Self::g_at(required, coalition, params);
        let margin = g_alone - g_coalition;
        // `g_at` returns +∞ on engine error, so `∞ − ∞` can be NaN. Never join on
        // an untrustworthy margin, and never propagate NaN/±∞ as a score.
        if !margin.is_finite() {
            return Decision { act: false, score: 0.0 };
        }
        Decision {
            act: margin > join_margin,
            score: margin,
        }
    }

    /// Pure leave decision over owned coverage masks. Leave iff removing the agent
    /// does not raise `G` (`g_out − g_in <= 0`).
    fn leave_decision(required: u32, in_mask: u32, out_mask: u32, params: BridgeParams) -> Decision {
        let g_in = Self::g_at(required, in_mask, params);
        let g_out = Self::g_at(required, out_mask, params);
        let delta = g_out - g_in;
        if !delta.is_finite() {
            return Decision { act: false, score: 0.0 };
        }
        Decision {
            act: delta <= 0.0,
            score: delta,
        }
    }

    /// Coverage masks `(alone, coalition)` for a join decision. `coalition`
    /// excludes `agent`.
    fn join_masks(agent: &dyn AgentCapabilities, coalition: &[&dyn AgentCapabilities]) -> (u32, u32) {
        let alone = agent.capabilities();
        (alone, union_caps(coalition) | alone)
    }

    /// Coverage masks `(in, out)` for a leave decision. `coalition` includes
    /// `agent`; `out` removes the agent by id (distinct ids assumed, as in the
    /// scalar arm).
    fn leave_masks(agent: &dyn AgentCapabilities, coalition: &[&dyn AgentCapabilities]) -> (u32, u32) {
        let agent_id = agent.agent_id();
        let in_mask = union_caps(coalition);
        let without: Vec<&dyn AgentCapabilities> = coalition
            .iter()
            .filter(|a| a.agent_id() != agent_id)
            .copied()
            .collect();
        (in_mask, union_caps(&without))
    }
}

impl CoalitionDecisionPolicy for AifMmDecisionPolicy {
    /// Join iff joining lowers `G` by more than `join_margin`. `coalition`
    /// excludes `agent`.
    fn should_join(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        ctx: &DecisionContext,
    ) -> Decision {
        let (alone, coalition_mask) = Self::join_masks(agent, coalition);
        Self::join_decision(
            ctx.required_capabilities,
            alone,
            coalition_mask,
            self.params,
            self.join_margin,
        )
    }

    /// Leave iff staying does not lower `G`. `coalition` includes `agent`.
    fn should_leave(
        &self,
        agent: &dyn AgentCapabilities,
        coalition: &[&dyn AgentCapabilities],
        ctx: &DecisionContext,
    ) -> Decision {
        let (in_mask, out_mask) = Self::leave_masks(agent, coalition);
        Self::leave_decision(ctx.required_capabilities, in_mask, out_mask, self.params)
    }

    /// Async override: snapshot the coverage masks in the sync prologue, then run
    /// the CPU-bound multi-modality EFE compute on the rayon pool via
    /// [`tokio_rayon::spawn`] so the tokio worker is not blocked. Produces the same
    /// [`Decision`] as [`should_join`](Self::should_join). `coalition` excludes
    /// `agent`.
    fn should_join_async<'a>(
        &'a self,
        agent: &'a dyn AgentCapabilities,
        coalition: &'a [&'a dyn AgentCapabilities],
        ctx: &'a DecisionContext,
    ) -> Pin<Box<dyn Future<Output = Decision> + Send + 'a>> {
        let (alone, coalition_mask) = Self::join_masks(agent, coalition);
        let required = ctx.required_capabilities;
        let params = self.params;
        let join_margin = self.join_margin;
        Box::pin(async move {
            tokio_rayon::spawn(move || {
                Self::join_decision(required, alone, coalition_mask, params, join_margin)
            })
            .await
        })
    }

    /// Async override of [`should_leave`](Self::should_leave). `coalition` includes
    /// `agent`.
    fn should_leave_async<'a>(
        &'a self,
        agent: &'a dyn AgentCapabilities,
        coalition: &'a [&'a dyn AgentCapabilities],
        ctx: &'a DecisionContext,
    ) -> Pin<Box<dyn Future<Output = Decision> + Send + 'a>> {
        let (in_mask, out_mask) = Self::leave_masks(agent, coalition);
        let required = ctx.required_capabilities;
        let params = self.params;
        Box::pin(async move {
            tokio_rayon::spawn(move || Self::leave_decision(required, in_mask, out_mask, params))
                .await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decision::AifDecisionPolicy;

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

    /// More required bits covered ⇒ lower `G` (monotone), and the modality count
    /// tracks the required-bit count.
    #[test]
    fn mm_efe_monotonic_in_covered_count() {
        let params = BridgeParams::default();
        let req = 0b111;
        let g0 = MultiModalModel::efe_for_coverage(req, 0b000, params).unwrap();
        let g1 = MultiModalModel::efe_for_coverage(req, 0b001, params).unwrap();
        let g2 = MultiModalModel::efe_for_coverage(req, 0b011, params).unwrap();
        let g3 = MultiModalModel::efe_for_coverage(req, 0b111, params).unwrap();
        eprintln!("mm efe by covered count: 0={g0} 1={g1} 2={g2} 3={g3}");
        assert!(
            g3 < g2 && g2 < g1 && g1 < g0,
            "more covered required bits ⇒ lower G: {g0} {g1} {g2} {g3}"
        );
    }

    /// Which specific bit is covered is immaterial (symmetric modalities): equal
    /// covered count ⇒ equal `G`.
    #[test]
    fn mm_efe_symmetric_in_bit_identity() {
        let params = BridgeParams::default();
        let req = 0b111;
        let a = MultiModalModel::efe_for_coverage(req, 0b001, params).unwrap();
        let b = MultiModalModel::efe_for_coverage(req, 0b010, params).unwrap();
        let c = MultiModalModel::efe_for_coverage(req, 0b100, params).unwrap();
        assert!((a - b).abs() < 1e-12 && (b - c).abs() < 1e-12, "{a} {b} {c}");
    }

    #[test]
    fn mm_join_is_non_degenerate() {
        let policy = AifMmDecisionPolicy::default();
        let ctx = DecisionContext { required_capabilities: 0b111 };
        let a0 = TestAgent { id: 0, caps: 0b001, trust: 50 };
        let a1 = TestAgent { id: 1, caps: 0b010, trust: 50 };
        let a2 = TestAgent { id: 2, caps: 0b100, trust: 50 };
        let coalition: [&dyn AgentCapabilities; 2] = [&a1, &a2];
        let d = policy.should_join(&a0, &coalition, &ctx);
        assert!(d.act, "coverage 1/3 → 3/3 must join (score={})", d.score);
        assert!(d.score > 0.0);
    }

    #[test]
    fn mm_join_clone_is_degenerate_no_op() {
        let policy = AifMmDecisionPolicy::default();
        let ctx = DecisionContext { required_capabilities: 0b111 };
        let a0 = TestAgent { id: 0, caps: 0b001, trust: 50 };
        let clone = TestAgent { id: 1, caps: 0b001, trust: 50 };
        let coalition: [&dyn AgentCapabilities; 1] = [&clone];
        let d = policy.should_join(&a0, &coalition, &ctx);
        assert!(!d.act, "redundant clone must not help (score={})", d.score);
        assert!(d.score.abs() < 1e-9, "margin ≈ 0, got {}", d.score);
    }

    #[test]
    fn mm_leave_when_redundant_else_stay() {
        let policy = AifMmDecisionPolicy::default();
        let ctx = DecisionContext { required_capabilities: 0b111 };
        let a0 = TestAgent { id: 0, caps: 0b001, trust: 50 };
        let a1 = TestAgent { id: 1, caps: 0b010, trust: 50 };
        let a2 = TestAgent { id: 2, caps: 0b100, trust: 50 };
        let full: [&dyn AgentCapabilities; 3] = [&a0, &a1, &a2];
        assert!(!policy.should_leave(&a0, &full, &ctx).act, "unique coverage stays");

        let redundant = TestAgent { id: 3, caps: 0b001, trust: 50 };
        let with_clone: [&dyn AgentCapabilities; 4] = [&a0, &a1, &a2, &redundant];
        assert!(policy.should_leave(&redundant, &with_clone, &ctx).act, "redundant leaves");
    }

    #[test]
    fn mm_value_calculator_orders_by_coverage() {
        let calc = MmEfeValueCalculator {
            required_capabilities: 0b111,
            params: BridgeParams::default(),
        };
        let a0 = TestAgent { id: 0, caps: 0b001, trust: 50 };
        let a1 = TestAgent { id: 1, caps: 0b010, trust: 50 };
        let a2 = TestAgent { id: 2, caps: 0b100, trust: 50 };
        let one: [&dyn AgentCapabilities; 1] = [&a0];
        let all: [&dyn AgentCapabilities; 3] = [&a0, &a1, &a2];
        assert!(calc.calculate_value(&all) > calc.calculate_value(&one));
        let _: &dyn ValueCalculator = &calc;
    }

    #[test]
    fn mm_required_zero_is_no_op() {
        let policy = AifMmDecisionPolicy::default();
        let ctx = DecisionContext { required_capabilities: 0 };
        let a0 = TestAgent { id: 0, caps: 0b001, trust: 50 };
        let a1 = TestAgent { id: 1, caps: 0b010, trust: 50 };
        let coalition: [&dyn AgentCapabilities; 1] = [&a1];
        let d = policy.should_join(&a0, &coalition, &ctx);
        assert!(!d.act && d.score.abs() < 1e-9, "no requirements ⇒ no join, got {}", d.score);
    }

    /// Characterization (K4-v3 mechanism): with binary union coverage and
    /// symmetric per-modality preferences, the multi-modality `G` is a strictly
    /// monotone function of the covered-bit COUNT — the same information the scalar
    /// arm's coverage fraction carries. Both decision rules depend only on the sign
    /// of ΔG, so `aif-mm` and `aif-scalar` return identical join/leave *acts*
    /// (their scores differ in magnitude). This is the empirical crux of the
    /// registered H2 test; it is asserted here as an honest characterization, not
    /// tuned.
    #[test]
    fn mm_and_scalar_agree_on_acts() {
        let mm = AifMmDecisionPolicy::default();
        let scalar = AifDecisionPolicy::default();
        let a0 = TestAgent { id: 0, caps: 0b0001, trust: 50 };
        let a1 = TestAgent { id: 1, caps: 0b0010, trust: 50 };
        let a2 = TestAgent { id: 2, caps: 0b0001, trust: 50 }; // clone of a0's bit
        let a3 = TestAgent { id: 3, caps: 0b1000, trust: 50 };

        for &required in &[0b0001u32, 0b0011, 0b0111, 0b1011, 0b1111] {
            let ctx = DecisionContext { required_capabilities: required };
            // Join cases against a few coalitions.
            for coalition in [
                vec![&a1 as &dyn AgentCapabilities],
                vec![&a1, &a2],
                vec![&a1, &a3],
                vec![&a2],
            ] {
                assert_eq!(
                    mm.should_join(&a0, &coalition, &ctx).act,
                    scalar.should_join(&a0, &coalition, &ctx).act,
                    "join act must match scalar (required={required:#06b})"
                );
            }
            // Leave cases over a mixed coalition.
            let full: [&dyn AgentCapabilities; 4] = [&a0, &a1, &a2, &a3];
            let members: [&dyn AgentCapabilities; 4] = [&a0, &a1, &a2, &a3];
            for member in members {
                assert_eq!(
                    mm.should_leave(member, &full, &ctx).act,
                    scalar.should_leave(member, &full, &ctx).act,
                    "leave act must match scalar (required={required:#06b})"
                );
            }
        }
    }

    /// Anchor: a single-bit modality is the *same POMDP* `aif::competence_efe`
    /// builds, so covered ⇒ `competence_efe(1.0)` (the 0.215 anchor) and uncovered
    /// ⇒ `competence_efe(0.0)` (the 1.204 anchor), bit-for-bit.
    #[test]
    fn mm_single_modality_matches_competence_efe() {
        let bp = BridgeParams::default();
        let ce = |c: f64| {
            aif::competence_efe(
                c,
                aif::ObsPrecisionParams {
                    max_precision: bp.max_precision,
                    success_preference: bp.success_preference,
                    alpha: bp.alpha,
                    transition_noise: 0.0,
                },
            )
            .unwrap()
        };
        let g_cov = MultiModalModel::efe_for_coverage(0b1, 0b1, bp).unwrap();
        let g_unc = MultiModalModel::efe_for_coverage(0b1, 0b0, bp).unwrap();
        assert!((g_cov - ce(1.0)).abs() < 1e-12, "covered mm == competence_efe(1.0): {g_cov}");
        assert!((g_unc - ce(0.0)).abs() < 1e-12, "uncovered mm == competence_efe(0.0): {g_unc}");
        // Documented tira anchors.
        assert!((g_cov - 0.215).abs() < 1e-3, "covered anchor ≈ 0.215, got {g_cov}");
        assert!((g_unc - 1.204).abs() < 1e-3, "uncovered anchor ≈ 1.204, got {g_unc}");
    }

    /// Additivity characterization (brief deliverable — the actual relationship,
    /// pinned): at the registered params `G` is additive across modalities to
    /// floating precision, `G(⋃ modalities) ≈ Σ_b G(single bit b)`.
    ///
    /// Mechanism: `neg_g` sums across modalities and `G = −E_{q(π)}[neg_g]`; the
    /// γ-softmax weight `q(π)` couples the modalities, so `G` is not a *plain* sum
    /// in general. But at `α`-independent `γ = 16` the posterior is a near-delta on
    /// the preferred control (off-mass `ε·gap ≈ 1e-14`), so the coupling term is
    /// negligible and both a symmetric-uncovered add and an informative-covered add
    /// are additive to ≈ 1e-13. The residual is strictly **sub-additive** (more
    /// covered bits sharpen the posterior, lowering `G` by ~7e-14 vs the pure sum) —
    /// real but at floating noise. Consequence: the multimodal value is essentially
    /// `Σ_b competence_efe(cov_b)`, monotone in the covered-bit count, which is why
    /// `aif-mm` and `aif-scalar` decisions coincide (see `mm_and_scalar_agree_on_acts`).
    #[test]
    fn mm_additivity_relationship() {
        let bp = BridgeParams::default();
        let g_1cov = MultiModalModel::efe_for_coverage(0b1, 0b1, bp).unwrap();
        let g_1unc = MultiModalModel::efe_for_coverage(0b1, 0b0, bp).unwrap();
        let g_cov_unc = MultiModalModel::efe_for_coverage(0b11, 0b01, bp).unwrap();
        let g_2cov = MultiModalModel::efe_for_coverage(0b11, 0b11, bp).unwrap();

        // Additive to floating precision for both a covered+uncovered mix and two
        // covered modalities.
        assert!(
            (g_cov_unc - (g_1cov + g_1unc)).abs() < 1e-12,
            "covered+uncovered additive: {g_cov_unc} vs {}",
            g_1cov + g_1unc
        );
        assert!(
            (g_2cov - 2.0 * g_1cov).abs() < 1e-12,
            "two covered additive to ~1e-13: {g_2cov} vs {}",
            2.0 * g_1cov
        );
        // The tiny residual is (non-strictly) sub-additive: more coverage never
        // raises G above the pure per-bit sum.
        assert!(
            g_2cov <= 2.0 * g_1cov,
            "sub-additive residual (coupling ≥ 0): {g_2cov} vs {}",
            2.0 * g_1cov
        );
    }

    /// Registered-arm determinism: no RNG in the value path — the same inputs give
    /// bit-identical `G`.
    #[test]
    fn mm_value_is_deterministic() {
        let bp = BridgeParams::default();
        let a = MultiModalModel::efe_for_coverage(0b1011, 0b0011, bp).unwrap();
        let b = MultiModalModel::efe_for_coverage(0b1011, 0b0011, bp).unwrap();
        assert_eq!(a.to_bits(), b.to_bits(), "value path must be deterministic");
    }

    #[tokio::test]
    async fn mm_async_matches_sync() {
        let policy: Box<dyn CoalitionDecisionPolicy> = Box::new(AifMmDecisionPolicy::default());
        let ctx = DecisionContext { required_capabilities: 0b111 };
        let a0 = TestAgent { id: 0, caps: 0b001, trust: 50 };
        let a1 = TestAgent { id: 1, caps: 0b010, trust: 50 };
        let a2 = TestAgent { id: 2, caps: 0b100, trust: 50 };
        let coalition: [&dyn AgentCapabilities; 2] = [&a1, &a2];

        let sync = policy.should_join(&a0, &coalition, &ctx);
        let asyncd = policy.should_join_async(&a0, &coalition, &ctx).await;
        assert_eq!(sync.act, asyncd.act);
        assert!((sync.score - asyncd.score).abs() < 1e-12);

        let full: [&dyn AgentCapabilities; 3] = [&a0, &a1, &a2];
        let sync_leave = policy.should_leave(&a0, &full, &ctx);
        let async_leave = policy.should_leave_async(&a0, &full, &ctx).await;
        assert_eq!(sync_leave.act, async_leave.act);
        assert!((sync_leave.score - async_leave.score).abs() < 1e-12);
    }
}
