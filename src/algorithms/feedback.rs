//! Feedback-weighted coalition value calculation (issue #41, Phase 5 idea 3).
//!
//! This is the LLM-free slice of the SwarmAgentic velocity coefficients closed
//! inside Rust: SwarmAgentic's failure (`c_f`) and personal-best (`c_p`)
//! coefficients become two calculator weights that reshape coalition scores from
//! observed history, with no LLM round-trips.
//!
//! - [`FeedbackCalculator::history_weight`] ≈ `c_p` (personal-best guidance):
//!   agents that have participated in many coalitions are rewarded, biasing the
//!   swarm toward proven members.
//! - [`FeedbackCalculator::failure_weight`] ≈ `c_f` (failure-driven repulsion):
//!   agents whose past outcomes fell short are penalised, pushing the swarm away
//!   from repeatedly-failing members.
//!
//! Both signals live in a [`FeedbackStore`] — a shared, cheaply-cloneable
//! counter table. `Clone` **shares** the underlying data (an `Arc`), so a
//! [`FeedbackCalculator`] and every clone of its store observe each other's
//! [`record_outcome`](FeedbackStore::record_outcome) calls. Use one store per
//! battery / run and clone the handle wherever an outcome is recorded.
//!
//! # Feedback loop
//!
//! [`record_outcome`](FeedbackStore::record_outcome) closes the loop: after a
//! coalition produces a value, feed the member ids and that value back into the
//! store. Every member's `history` counter increments; members whose outcome
//! fell strictly below the store's failure threshold also have their `failures`
//! counter incremented. The next [`calculate_value`](ValueCalculator::calculate_value)
//! then reflects the accumulated history without any LLM call.
//!
//! # Marginal decomposition
//!
//! Under [`ThresholdPolicy`](crate::decision::ThresholdPolicy), the join
//! marginal for a candidate `x` decomposes cleanly:
//!
//! ```text
//! marginal(x) = base_marginal(x)
//!             + history_weight · HISTORY_UNIT · history(x)
//!             − failure_weight · FAILURE_UNIT · failures(x)
//! ```
//!
//! because the existing members' counters appear identically in the `with` and
//! `without` sums and cancel — only the candidate's own history/failure
//! counters move the marginal.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use super::{AgentCapabilities, ValueCalculator};

/// Score contributed per recorded membership episode (before `history_weight`).
pub const HISTORY_UNIT: f64 = 25.0;
/// Score removed per recorded failure (before `failure_weight`).
pub const FAILURE_UNIT: f64 = 25.0;

/// Per-agent feedback counters accumulated from observed coalition outcomes.
#[derive(Debug, Clone, Copy, Default)]
struct AgentFeedback {
    /// Number of coalition episodes this agent has participated in.
    history: u64,
    /// Number of those episodes whose value fell below the failure threshold.
    failures: u64,
}

/// Interior counter table shared behind the [`FeedbackStore`]'s lock.
#[derive(Debug)]
struct StoreInner {
    /// Outcomes strictly below this value count as failures.
    failure_threshold: f64,
    /// Per-agent counters, keyed by `agent_id`.
    records: HashMap<usize, AgentFeedback>,
}

/// A shared, cheaply-cloneable table of per-agent feedback counters.
///
/// `Clone` **shares** the underlying data: every clone observes recordings made
/// through any other clone. Construct one per battery / run and clone the handle
/// to wherever outcomes are reported.
#[derive(Debug, Clone)]
pub struct FeedbackStore {
    inner: Arc<RwLock<StoreInner>>,
}

impl FeedbackStore {
    /// Create an empty store. Outcomes strictly below `failure_threshold` count
    /// as failures.
    #[must_use]
    pub fn new(failure_threshold: f64) -> Self {
        Self {
            inner: Arc::new(RwLock::new(StoreInner {
                failure_threshold,
                records: HashMap::new(),
            })),
        }
    }

    /// Record a coalition outcome: bump each member's history, and its failure
    /// count if `value` is strictly below the failure threshold.
    ///
    /// A non-finite `value` (NaN or ±∞) is a signal that an upstream calculator
    /// failed; it is logged and **ignored** — the store is left completely
    /// unchanged so a bad value can never poison the counters.
    ///
    /// Ids are counted **per occurrence**: a duplicated id in `members` bumps
    /// that agent's counters twice. Hyperedge member lists in this codebase may
    /// legitimately contain duplicates (they are ordered `Vec`s, duplicates
    /// allowed) — dedup before recording if you want at-most-once-per-agent
    /// semantics.
    pub fn record_outcome(&self, members: &[usize], value: f64) {
        if !value.is_finite() {
            tracing::warn!(
                value,
                member_count = members.len(),
                "feedback: ignoring non-finite outcome value"
            );
            return;
        }

        // The store holds plain counters that are consistent at every point, so
        // recovering a poisoned lock is safe.
        let mut inner = self.inner.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        // Strict `<`: an outcome exactly at the threshold is not a failure.
        let is_failure = value < inner.failure_threshold;
        for &id in members {
            let record = inner.records.entry(id).or_default();
            record.history += 1;
            if is_failure {
                record.failures += 1;
            }
        }
    }

    /// Bulk-add `episodes` membership episodes to an agent's history.
    ///
    /// Used to seed the personal-best signal from the event-sourced membership
    /// log (see [`CoalitionManager::seed_feedback_history`]); failures cannot be
    /// seeded this way (the log records memberships, not outcomes).
    ///
    /// This **accumulates** (it never replaces): adding the same episode count
    /// twice doubles it. Seed a store at most once — see the contract on
    /// [`CoalitionManager::seed_feedback_history`].
    ///
    /// [`CoalitionManager::seed_feedback_history`]: crate::topology::CoalitionManager::seed_feedback_history
    pub fn add_history(&self, agent_id: usize, episodes: u64) {
        // See `record_outcome` for the poison-recovery rationale.
        let mut inner = self.inner.write().unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.records.entry(agent_id).or_default().history += episodes;
    }

    /// The recorded episode count for `agent_id` (0 for an unknown agent).
    #[must_use]
    pub fn history(&self, agent_id: usize) -> u64 {
        let inner = self.inner.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.records.get(&agent_id).map_or(0, |r| r.history)
    }

    /// The recorded failure count for `agent_id` (0 for an unknown agent).
    #[must_use]
    pub fn failures(&self, agent_id: usize) -> u64 {
        let inner = self.inner.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.records.get(&agent_id).map_or(0, |r| r.failures)
    }
}

/// A [`ValueCalculator`] that adds accumulated per-agent history and failure
/// signals on top of a wrapped base calculator.
///
/// The score is
///
/// ```text
/// base(agents)
///   + history_weight · HISTORY_UNIT · Σ history(a)
///   − failure_weight · FAILURE_UNIT · Σ failures(a)
/// ```
///
/// summed over the coalition's members. As with
/// [`WeightedCalculator`](super::WeightedCalculator), the weights carry no
/// enforced invariants — negative, zero, or non-finite weights are used as-is.
/// A non-finite weight simply propagates into the returned value, where the
/// decision layer's existing non-finite guard (e.g.
/// [`ThresholdPolicy`](crate::decision::ThresholdPolicy)) declines the action.
///
/// `Clone` shares the underlying [`FeedbackStore`] (see its docs), so cloning a
/// calculator does not fork its feedback history.
#[derive(Debug, Clone)]
pub struct FeedbackCalculator<C: ValueCalculator> {
    base: C,
    /// Personal-best coefficient (`c_p` analogue): weight on recorded history.
    pub history_weight: f64,
    /// Failure coefficient (`c_f` analogue): weight on recorded failures.
    pub failure_weight: f64,
    store: FeedbackStore,
}

impl<C: ValueCalculator> FeedbackCalculator<C> {
    /// Wrap `base`, weighting the history and failure signals drawn from `store`.
    #[must_use]
    pub fn new(base: C, history_weight: f64, failure_weight: f64, store: FeedbackStore) -> Self {
        Self {
            base,
            history_weight,
            failure_weight,
            store,
        }
    }

    /// Borrow the feedback store. Clone the returned handle (it shares the data)
    /// to record outcomes from elsewhere.
    #[must_use]
    pub fn store(&self) -> &FeedbackStore {
        &self.store
    }
}

impl<C: ValueCalculator> ValueCalculator for FeedbackCalculator<C> {
    fn calculate_value(&self, agents: &[&dyn AgentCapabilities]) -> f64 {
        if agents.is_empty() {
            return 0.0;
        }

        let base = self.base.calculate_value(agents);

        // Take the read lock once for the whole sum rather than per agent.
        let inner = self
            .store
            .inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut history_total = 0.0_f64;
        let mut failure_total = 0.0_f64;
        for agent in agents {
            if let Some(record) = inner.records.get(&agent.agent_id()) {
                history_total += record.history as f64;
                failure_total += record.failures as f64;
            }
        }
        drop(inner);

        base + self.history_weight * HISTORY_UNIT * history_total
            - self.failure_weight * FAILURE_UNIT * failure_total
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algorithms::{AdditiveCalculator, CapabilityAgent};

    fn as_caps(agents: &[CapabilityAgent]) -> Vec<&dyn AgentCapabilities> {
        agents.iter().map(|a| a as &dyn AgentCapabilities).collect()
    }

    /// Two agents whose `AdditiveCalculator` value is exactly 270:
    /// size 2·50 + caps (2+1)·10 + trust (60+80) = 100 + 30 + 140 = 270.
    fn fixture_agents() -> [CapabilityAgent; 2] {
        [
            CapabilityAgent::new(1, 0b0011, 60),
            CapabilityAgent::new(2, 0b0100, 80),
        ]
    }

    #[test]
    fn additive_base_is_exact() {
        let agents = fixture_agents();
        assert_eq!(AdditiveCalculator.calculate_value(&as_caps(&agents)), 270.0);
    }

    #[test]
    fn record_outcome_counts_history_and_failures() {
        let store = FeedbackStore::new(100.0);
        store.record_outcome(&[1, 2], 150.0); // success for both
        store.record_outcome(&[1], 50.0); // failure for 1 only

        assert_eq!(store.history(1), 2);
        assert_eq!(store.history(2), 1);
        assert_eq!(store.failures(1), 1);
        assert_eq!(store.failures(2), 0);
    }

    #[test]
    fn zero_weights_equal_base() {
        let store = FeedbackStore::new(100.0);
        store.record_outcome(&[1, 2], 150.0);
        store.record_outcome(&[1], 50.0);

        let agents = fixture_agents();
        let calc = FeedbackCalculator::new(AdditiveCalculator, 0.0, 0.0, store);
        // Even with a populated store, zero weights reduce to the base value.
        assert_eq!(calc.calculate_value(&as_caps(&agents)), 270.0);
    }

    #[test]
    fn signals_move_scores() {
        let store = FeedbackStore::new(100.0);
        store.record_outcome(&[1, 2], 150.0);
        store.record_outcome(&[1], 50.0);
        // history: 1→2, 2→1 (Σ=3); failures: 1→1, 2→0 (Σ=1).

        let agents = fixture_agents();

        // hw=1, fw=1 → 270 + 25·3 − 25·1 = 320.
        let base = FeedbackCalculator::new(AdditiveCalculator, 1.0, 1.0, store.clone());
        let base_score = base.calculate_value(&as_caps(&agents));
        assert_eq!(base_score, 320.0);

        // History monotonicity: hw=2, fw=1 → 270 + 25·2·3 − 25·1 = 395.
        let more_history = FeedbackCalculator::new(AdditiveCalculator, 2.0, 1.0, store.clone());
        let more_history_score = more_history.calculate_value(&as_caps(&agents));
        assert_eq!(more_history_score, 395.0);
        assert!(more_history_score > base_score);

        // Failure monotonicity: hw=1, fw=2 → 270 + 25·3 − 25·2·1 = 295.
        let more_failure = FeedbackCalculator::new(AdditiveCalculator, 1.0, 2.0, store);
        let more_failure_score = more_failure.calculate_value(&as_caps(&agents));
        assert_eq!(more_failure_score, 295.0);
        assert!(more_failure_score < base_score);
    }

    #[test]
    fn threshold_boundary_is_not_a_failure() {
        let store = FeedbackStore::new(100.0);
        store.record_outcome(&[2], 100.0); // exactly at threshold, strict `<`
        assert_eq!(store.history(2), 1);
        assert_eq!(store.failures(2), 0);
    }

    #[test]
    fn non_finite_outcome_leaves_store_unchanged() {
        let store = FeedbackStore::new(100.0);
        store.record_outcome(&[1], f64::NAN);
        store.record_outcome(&[1], f64::INFINITY);
        store.record_outcome(&[1], f64::NEG_INFINITY);
        assert_eq!(store.history(1), 0);
        assert_eq!(store.failures(1), 0);
    }

    #[test]
    fn clone_shares_store() {
        let store = FeedbackStore::new(100.0);
        let handle = store.clone();
        handle.record_outcome(&[7], 10.0); // failure

        // The original observes the clone's recording.
        assert_eq!(store.history(7), 1);
        assert_eq!(store.failures(7), 1);

        // FeedbackCalculator::clone also shares the store.
        let calc = FeedbackCalculator::new(AdditiveCalculator, 1.0, 1.0, store);
        let calc2 = calc.clone();
        calc.store().record_outcome(&[7], 10.0);
        assert_eq!(calc2.store().history(7), 2);
    }

    #[test]
    fn empty_slice_is_zero_even_with_populated_store() {
        let store = FeedbackStore::new(100.0);
        store.record_outcome(&[1, 2], 10.0);
        let calc = FeedbackCalculator::new(AdditiveCalculator, 1.0, 1.0, store);
        assert_eq!(calc.calculate_value(&[]), 0.0);
    }

    #[test]
    fn unknown_agent_contributes_nothing() {
        let store = FeedbackStore::new(100.0);
        // Nothing recorded for these agents.
        assert_eq!(store.history(42), 0);
        assert_eq!(store.failures(42), 0);

        let agents = fixture_agents();
        let calc = FeedbackCalculator::new(AdditiveCalculator, 1.0, 1.0, store);
        // Unrecorded agents add zero on top of the base.
        assert_eq!(calc.calculate_value(&as_caps(&agents)), 270.0);
    }
}
