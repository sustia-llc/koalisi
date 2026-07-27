//! Reliability-weighted coverage value model (issue #57, feature `decision`).
//!
//! [`ReliabilityCoverage`] is a [`ValueCalculator`] that scores a block of agents
//! by how much of a required capability mask it covers, **weighted by how reliable
//! the providers of each covered bit have proven to be**. The reliability vector is
//! read out of the persistent Active Inference world model
//! ([`PersistentAifState`]) shared by the K4-v4/v5 arms — the belief read is
//! identical under both, since `query_dynamics` shapes only the per-decision query
//! POMDP, never the persistent beliefs. The slow loop (population
//! coalition-structure search, issue #42) is thereby driven by a signal the fast
//! loop learns — the persistent per-bit beliefs, which enter each E1 query as its
//! prior — without paying the per-decision AIF query cost. (The fast loop's full
//! decision signal also folds in the coverage-masked Dirichlet `pA` blocks; this
//! calculator reads the beliefs only.)
//!
//! Shape of the score, for a block `S` against `required`:
//!
//! - full coverage (`union(S) ⊇ required`): `100 · mean(r_b : b ∈ required)`
//! - partial coverage: `15 · Σ(r_b : b ∈ union(S) ∩ required)`
//! - minus `8` per member
//!
//! With `r ≡ 1` this is exactly the `TaskCoverage` calculator of
//! `examples/population_search.rs` (for `required` within the low 8 bits — higher
//! bits are masked off, see [`ReliabilityCoverage::new`]): an *interior* optimum
//! that groups complementary specialists into minimal full-coverage teams. That
//! matters — the built-in calculators are degenerate for structure search (see the
//! [`search`] module docs and gotcha 21), so a coverage-shaped model is what gives
//! the swarm real work.
//!
//! Reliability then *rescales* that optimum: an unreliable required bit lowers the
//! mean and so the value of every full-coverage block. It does not, at these
//! coefficients, make the search route *around* the bit — for `|required| ≤ 6` the
//! full-coverage weight per unit reliability (`100/|required| ≥ 16.7`) exceeds the
//! partial-coverage weight (`15`), so skipping a bit never pays at equal member
//! count. Reliability re-ranks structures only via the number of full-coverage
//! blocks and via which bits the leftover partial blocks cover.
//!
//! # Scope constraint: reliability is per BIT, not per agent
//!
//! The E1 world model carries **8 two-state factors, one per capability bit**
//! ("is provision of bit `b` reliable?"). It has no per-agent factor. So this
//! calculator cannot distinguish two agents that provide the same bits: two blocks
//! covering identical bits score identically apart from the member-count term,
//! however differently their members have historically performed. Agent-level
//! discrimination would need a different world model (an agent-indexed factor set)
//! and is explicitly out of scope for issue #57.
//!
//! [`search`]: crate::algorithms::search

use super::PersistentAifState;
use crate::algorithms::{AgentCapabilities, ValueCalculator};

/// Capability bits the reliability vector covers — the world model's factor count.
const N_BITS: usize = 8;

/// Low-[`N_BITS`] mask. Bits outside the world model's universe carry no
/// reliability estimate and are ignored by the value model.
const BIT_MASK: u32 = (1u32 << N_BITS) - 1;

/// Flat bonus for a block that fully covers `required`, scaled by mean reliability.
const FULL_COVERAGE_UNIT: f64 = 100.0;

/// Per-covered-bit bonus for a block that covers `required` only partially.
const PARTIAL_BIT_UNIT: f64 = 15.0;

/// Per-member cost — penalises redundant members in a block's *standalone* value.
/// Constant (`8·N`) summed across the blocks of any partition of a fixed pool, so
/// under [`search`](crate::algorithms::search) over-large blocks lose by starving
/// other blocks of members, not by this term.
const MEMBER_COST: f64 = 8.0;

/// Reliability assumed for a bit a snapshot cannot supply (the uniform two-state
/// prior). Note this typically sits *above* learned posteriors (the example's
/// measured bits read 0.07–0.28), so a missing bit reads as more reliable than a
/// measured one.
const UNKNOWN_RELIABILITY: f64 = 0.5;

/// Coverage value model weighted by learned per-bit provider reliability
/// (issue #57).
///
/// Scores a block of agents `S` against a required capability mask by how much of
/// it they cover, weighted by how reliable each covered bit's providers have proven
/// to be:
///
/// - full coverage (`union(S) ⊇ required`): `100 · mean(r_b : b ∈ required)`
/// - partial coverage: `15 · Σ(r_b : b ∈ union(S) ∩ required)`
/// - minus `8` per member
///
/// With `r ≡ 1` this reduces exactly to the `TaskCoverage` model of
/// `examples/population_search.rs` (for `required` within the low 8 bits — higher
/// bits are masked off, see [`new`](Self::new)) — an *interior* optimum that groups
/// complementary specialists into minimal full-coverage teams, which is what gives
/// [`search`](crate::algorithms::search) real work to do (the built-in calculators
/// are degenerate for structure search). Reliability then *rescales* that optimum —
/// an unreliable required bit lowers the value of every full-coverage block; see
/// the module docs for why, at these coefficients, it re-ranks structures rather
/// than routing the search around a weak bit.
///
/// Construct from an explicit vector with [`new`](Self::new), or from a persistent
/// AIF world-model snapshot with [`from_state`](Self::from_state).
///
/// # Scope constraint: reliability is per BIT, not per agent
///
/// The world model behind [`PersistentAifState`] carries 8 two-state factors, one
/// per capability *bit* ("is provision of bit `b` reliable?"), and no per-agent
/// factor. So this model cannot distinguish two agents providing the same bits:
/// two blocks covering identical bits score identically apart from the
/// member-count term, however differently their members have performed.
/// Agent-level discrimination needs a different world model and is out of scope
/// for issue #57.
#[derive(Debug, Clone)]
pub struct ReliabilityCoverage {
    /// Required capability mask, restricted to the low [`N_BITS`] bits.
    required: u32,
    /// `reliability[b]` = the world model's posterior that bit `b`'s providers are
    /// in the reliable state, clamped to `[0, 1]`. A model-relative belief, not a
    /// calibrated success rate (an unobserved bit reads 0.5 while a
    /// frequently-succeeding observed one can read lower) — see
    /// [`from_state`](Self::from_state).
    reliability: [f64; N_BITS],
}

impl ReliabilityCoverage {
    /// Build a value model for `required` from an explicit reliability vector.
    ///
    /// Each entry is clamped to `[0, 1]`; a non-finite entry is logged and becomes
    /// `0.0` (maximally unreliable), so an upstream failure can never NaN-poison
    /// the search — stricter than
    /// [`FeedbackStore::record_outcome`](crate::algorithms::FeedbackStore::record_outcome),
    /// which drops a non-finite value entirely.
    /// `required` is masked to the low 8 bits — bits above the world model's
    /// universe carry no reliability estimate and are dropped.
    #[must_use]
    pub fn new(required: u32, reliability: [f64; N_BITS]) -> Self {
        let mut sanitized = [0.0f64; N_BITS];
        for (bit, (slot, &raw)) in sanitized.iter_mut().zip(reliability.iter()).enumerate() {
            *slot = if raw.is_finite() {
                raw.clamp(0.0, 1.0)
            } else {
                tracing::warn!(bit, value = raw, "reliability: non-finite entry, using 0.0");
                0.0
            };
        }
        Self {
            required: required & BIT_MASK,
            reliability: sanitized,
        }
    }

    /// Build a value model from a persistent AIF world-model snapshot.
    ///
    /// `reliability[b]` is the snapshot's smoothed marginal belief **at the last
    /// node of the arm's 2-step MMP window** that factor `b` sits in state 0 — the
    /// [`PersistentAifArm`](super::PersistentAifArm) world model encodes each
    /// capability bit as a two-state factor with state 0 = providers reliable,
    /// state 1 = unreliable. Being a windowed smoothed posterior, the value is
    /// recent-outcome-weighted, NOT a running average over the outcome history —
    /// the same success record read after different final outcomes yields
    /// materially different numbers. Cross-bit *ordering* is the robust signal.
    ///
    /// Defensive by construction: a snapshot with fewer than 8 factors, or an
    /// empty belief vector for some factor, contributes a uniform `0.5` prior for
    /// the missing bits and logs a warning rather than panicking. Only `beliefs`
    /// is read, so snapshots taken
    /// from an arm with `persistent_learning: false` (where `pa`/`pb`/`pd` are
    /// `None`) are read without error — though with weaker cross-bit
    /// discrimination, since a non-learning arm's `A` never adapts.
    #[must_use]
    pub fn from_state(required: u32, state: &PersistentAifState) -> Self {
        let mut reliability = [UNKNOWN_RELIABILITY; N_BITS];
        if state.beliefs.len() < N_BITS {
            tracing::warn!(
                factors = state.beliefs.len(),
                expected = N_BITS,
                "reliability: short belief snapshot, missing bits default to uniform"
            );
        }
        for (bit, slot) in reliability.iter_mut().enumerate() {
            if let Some(&p) = state.beliefs.get(bit).and_then(|b| b.get(0)) {
                *slot = p;
            } else {
                tracing::warn!(bit, "reliability: no belief for bit, using uniform prior");
            }
        }
        Self::new(required, reliability)
    }

    /// The sanitized per-bit reliabilities this model scores with (index = bit).
    #[must_use]
    pub fn reliability(&self) -> &[f64; N_BITS] {
        &self.reliability
    }

    /// Sum of the reliabilities of the bits set in `mask` (already `BIT_MASK`ed).
    fn sum_reliability(&self, mask: u32) -> f64 {
        (0..N_BITS)
            .filter(|b| mask & (1u32 << b) != 0)
            .map(|b| self.reliability[b])
            .sum()
    }
}

impl ValueCalculator for ReliabilityCoverage {
    /// Reliability-weighted coverage minus member cost; see
    /// [`ReliabilityCoverage`] for the formula.
    #[allow(clippy::cast_precision_loss)] // bounded team sizes
    fn calculate_value(&self, agents: &[&dyn AgentCapabilities]) -> f64 {
        if agents.is_empty() {
            return 0.0;
        }
        let union = agents.iter().fold(0u32, |acc, a| acc | a.capabilities());
        let covered = union & self.required;

        let coverage = if covered == self.required {
            // A vacuous requirement is trivially, perfectly covered.
            let mean = if self.required == 0 {
                1.0
            } else {
                self.sum_reliability(self.required) / f64::from(self.required.count_ones())
            };
            FULL_COVERAGE_UNIT * mean
        } else {
            PARTIAL_BIT_UNIT * self.sum_reliability(covered)
        };

        coverage - agents.len() as f64 * MEMBER_COST
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algorithms::{CapabilityAgent, PopulationConfig, search};
    use crate::decision::{PersistentAifArm, PersistentAifConfig, TrialBoundary};
    use nalgebra::DVector;

    /// The `examples/population_search.rs` value model, replicated verbatim as the
    /// reference the all-ones case must reduce to.
    struct TaskCoverage {
        required: u32,
    }

    impl ValueCalculator for TaskCoverage {
        #[allow(clippy::cast_precision_loss)]
        fn calculate_value(&self, agents: &[&dyn AgentCapabilities]) -> f64 {
            if agents.is_empty() {
                return 0.0;
            }
            let union = agents.iter().fold(0u32, |acc, a| acc | a.capabilities());
            let covered = (union & self.required).count_ones();
            let required = self.required.count_ones();

            let coverage = if covered == required {
                100.0
            } else {
                f64::from(covered) * 15.0
            };
            coverage - agents.len() as f64 * 8.0
        }
    }

    /// The six-agent pool of `examples/population_search.rs`: four pure specialists
    /// plus two 2-capability generalists over a 4-bit task.
    fn pool() -> Vec<CapabilityAgent> {
        vec![
            CapabilityAgent::new(0, 0b0001, 80),
            CapabilityAgent::new(1, 0b0010, 70),
            CapabilityAgent::new(2, 0b0100, 90),
            CapabilityAgent::new(3, 0b1000, 60),
            CapabilityAgent::new(4, 0b0011, 75),
            CapabilityAgent::new(5, 0b1100, 85),
        ]
    }

    fn block<'a>(agents: &'a [CapabilityAgent], idx: &[usize]) -> Vec<&'a dyn AgentCapabilities> {
        idx.iter().map(|&i| &agents[i] as &dyn AgentCapabilities).collect()
    }

    #[test]
    fn reliable_bits_valued_above_unreliable_at_equal_structure() {
        let agents = pool();
        // Agents 0..4 fully cover 0b1111 — the same structure under both models.
        let members = block(&agents, &[0, 1, 2, 3]);

        let reliable = ReliabilityCoverage::new(0b1111, [1.0; N_BITS]);
        let unreliable = ReliabilityCoverage::new(0b1111, [0.2; N_BITS]);
        assert!(
            reliable.calculate_value(&members) > unreliable.calculate_value(&members),
            "reliable providers must be worth strictly more at equal structure"
        );

        // Same for a partially-covering block (agents 0,1 cover 0b0011).
        let partial = block(&agents, &[0, 1]);
        assert!(
            reliable.calculate_value(&partial) > unreliable.calculate_value(&partial),
            "partial coverage must also be reliability-weighted"
        );
    }

    #[test]
    fn all_ones_reduces_to_task_coverage() {
        let agents = pool();
        let reference = TaskCoverage { required: 0b1111 };
        let calc = ReliabilityCoverage::new(0b1111, [1.0; N_BITS]);

        let cases: [&[usize]; 4] = [
            &[],           // empty
            &[0, 1],       // partial (0b0011)
            &[4, 5],       // full coverage, minimal team
            &[0, 1, 2, 3, 4, 5], // full coverage, oversized team
        ];
        for idx in cases {
            let members = block(&agents, idx);
            assert_eq!(
                calc.calculate_value(&members),
                reference.calculate_value(&members),
                "all-ones reliability must reduce exactly to TaskCoverage for {idx:?}"
            );
        }
    }

    #[test]
    fn interior_optimum_search_nondegenerate() {
        let agents = pool();
        // Bit 2's providers are unreliable; bits 0/1/3 are dependable.
        let mut reliability = [0.9f64; N_BITS];
        reliability[2] = 0.3;
        let calc = ReliabilityCoverage::new(0b1111, reliability);

        let cfg = PopulationConfig::default().with_seed(27);
        let outcome = search(&agents, &calc, &cfg);

        let blocks = outcome.best.blocks();
        assert!(
            blocks.len() > 1 && blocks.len() < agents.len(),
            "optimum must be interior (not one block, not all singletons), got {blocks:?}"
        );
        assert!(
            outcome.lineage.len() > 1,
            "the swarm must actually improve on its seeded global best"
        );
    }

    #[test]
    fn from_state_defensive() {
        // A fresh arm has uniform beliefs — every bit reads as 0.5, all finite.
        let arm = PersistentAifArm::new(7, PersistentAifConfig::default())
            .expect("invariant: the default persistent config builds a valid engine model");
        let calc = ReliabilityCoverage::from_state(0b1111, &arm.state_snapshot());
        for (bit, &r) in calc.reliability().iter().enumerate() {
            assert!(r.is_finite(), "bit {bit} reliability must be finite");
            assert!((0.0..=1.0).contains(&r), "bit {bit} reliability out of range: {r}");
        }

        // A short / ragged snapshot falls back to the uniform prior, never panics.
        let ragged = PersistentAifState {
            beliefs: vec![
                DVector::from_vec(vec![0.8, 0.2]),
                DVector::from_vec(Vec::new()),
            ],
            pa: None,
            pb: None,
            pd: None,
            beta: None,
            trial_boundary: TrialBoundary::PerStream,
        };
        let calc = ReliabilityCoverage::from_state(0b1111, &ragged);
        assert!((calc.reliability()[0] - 0.8).abs() < 1e-12, "present belief is used");
        assert!(
            calc.reliability()[1..]
                .iter()
                .all(|&r| (r - UNKNOWN_RELIABILITY).abs() < 1e-12),
            "empty and missing factors fall back to the uniform prior"
        );
    }

    #[test]
    fn non_finite_reliability_sanitized() {
        let agents = pool();
        let mut reliability = [1.0f64; N_BITS];
        reliability[1] = f64::NAN;
        reliability[3] = f64::INFINITY;
        let calc = ReliabilityCoverage::new(0b1111, reliability);

        assert_eq!(calc.reliability()[1], 0.0, "NaN sanitizes to 0.0");
        assert_eq!(calc.reliability()[3], 0.0, "±∞ sanitizes to 0.0");

        let members = block(&agents, &[0, 1, 2, 3]);
        let value = calc.calculate_value(&members);
        assert!(value.is_finite(), "a sanitized model can never emit NaN");
        // Full coverage: mean of [1.0, 0.0, 1.0, 0.0] = 0.5 ⇒ 50 − 4·8 = 18.
        assert!((value - 18.0).abs() < 1e-12, "unexpected value {value}");
    }
}
