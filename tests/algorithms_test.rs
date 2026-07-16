//! Algorithm layer integration tests — value calculators, DCVC, AIPA.

mod common;

use common::algorithms::{Agent, as_caps, test_agents};
use koalisi::algorithms::{
    AdditiveCalculator, AgentCapabilities, CapabilityAgent, DCVCDistributor, FeedbackCalculator,
    FeedbackStore, MultiplicativeCalculator, SynergisticCalculator, ValueCalculator,
    WeightedCalculator, compute_all_partition_bounds, find_best_partition,
    generate_integer_partitions, partition_count, verify_partition,
};
use std::collections::HashMap;

// =========================================================================
// Value calculator tests
// =========================================================================

#[test]
fn additive_calculator() {
    let calc = AdditiveCalculator;
    let agents = test_agents();
    let refs = as_caps(&agents);

    let value = calc.calculate_value(&refs);
    assert!(value > 0.0);
    assert_eq!(calc.calculate_value(&[]), 0.0);
}

#[test]
fn synergistic_values_diversity() {
    let calc = SynergisticCalculator;

    let diverse = [Agent::new(1, 0b001, 80), Agent::new(2, 0b010, 80)];
    let duplicate = [Agent::new(1, 0b001, 80), Agent::new(2, 0b001, 80)];

    let diverse_value = calc.calculate_value(&as_caps(&diverse));
    let duplicate_value = calc.calculate_value(&as_caps(&duplicate));

    assert!(diverse_value > duplicate_value);
}

#[test]
fn multiplicative_scales_with_agents() {
    let calc = MultiplicativeCalculator::new(1.0);
    let agents = test_agents();
    let refs = as_caps(&agents);

    let two_value = calc.calculate_value(&refs[..2]);
    let three_value = calc.calculate_value(&refs);

    assert!(three_value > two_value);
}

#[test]
fn weighted_calculator_presets() {
    let agents = test_agents();
    let refs = as_caps(&agents);

    let balanced = WeightedCalculator::balanced();
    let cap_focused = WeightedCalculator::capability_focused();
    let trust_focused = WeightedCalculator::trust_focused();

    assert!(balanced.calculate_value(&refs) > 0.0);
    assert!(cap_focused.calculate_value(&refs) > 0.0);
    assert!(trust_focused.calculate_value(&refs) > 0.0);
}

#[test]
fn synergistic_beats_additive_for_diverse() {
    let agents = test_agents();
    let refs = as_caps(&agents);

    let additive = AdditiveCalculator;
    let synergistic = SynergisticCalculator;

    assert!(synergistic.calculate_value(&refs) > additive.calculate_value(&refs));
}

// =========================================================================
// Feedback-weighted calculator tests (issue #41)
// =========================================================================

/// End-to-end feedback loop through `ThresholdPolicy`: a candidate with a heavy
/// failure record is declined where an otherwise-identical clean candidate joins.
#[test]
fn feedback_failures_close_the_decision_loop() {
    use koalisi::decision::{CoalitionDecisionPolicy, DecisionContext, ThresholdPolicy};

    let a1 = CapabilityAgent::new(1, 0b0011, 60);
    let coalition = [&a1 as &dyn AgentCapabilities];
    let ctx = DecisionContext::default();

    // Five failing outcomes for the candidate id 9 (value 0.0 < threshold 100).
    let store = FeedbackStore::new(100.0);
    for _ in 0..5 {
        store.record_outcome(&[9], 0.0);
    }
    assert_eq!(store.history(9), 5);
    assert_eq!(store.failures(9), 5);

    // hw=0, fw=1: the candidate's additive marginal (110) is dragged to
    // 110 − 25·5 = −15, below the join_threshold of 0.0.
    let policy = ThresholdPolicy::new(
        FeedbackCalculator::new(AdditiveCalculator, 0.0, 1.0, store),
        0.0,
        0.0,
    );

    let failing = CapabilityAgent::new(9, 0b1000, 50);
    let decision = policy.should_join(&failing, &coalition, &ctx);
    assert!(!decision.act, "heavy failure record blocks the join");
    assert_eq!(decision.score, -15.0);

    // A clean candidate with identical caps/trust but no failure record joins.
    let clean = CapabilityAgent::new(10, 0b1000, 50);
    let clean_decision = policy.should_join(&clean, &coalition, &ctx);
    assert!(clean_decision.act, "clean candidate joins on positive marginal");
    assert_eq!(clean_decision.score, 110.0);
}

/// The join marginal decomposes exactly as
/// `base_marginal + hw·HISTORY_UNIT·history(x) − fw·FAILURE_UNIT·failures(x)`;
/// the existing members' counters cancel in the with/without difference.
#[test]
fn feedback_marginal_decomposition() {
    use koalisi::decision::{CoalitionDecisionPolicy, DecisionContext, ThresholdPolicy};

    let a1 = CapabilityAgent::new(1, 0b0011, 60);
    let x = CapabilityAgent::new(9, 0b1000, 50);
    let coalition = [&a1 as &dyn AgentCapabilities];
    let ctx = DecisionContext::default();

    // Record outcomes touching BOTH the existing member and the candidate so the
    // cancellation of the member's counters is actually exercised.
    let store = FeedbackStore::new(100.0);
    store.record_outcome(&[1, 9], 150.0); // success for 1 and 9
    store.record_outcome(&[1, 9], 50.0); // failure for 1 and 9
    store.record_outcome(&[9], 50.0); // extra failure for 9
    // history(1)=2 failures(1)=1 ; history(9)=3 failures(9)=2.

    let (hw, fw) = (1.0_f64, 1.0_f64);

    // Base (feedback-free) additive marginal for x joining [a1].
    let base_without = AdditiveCalculator.calculate_value(&coalition);
    let mut with = coalition.to_vec();
    with.push(&x);
    let base_with = AdditiveCalculator.calculate_value(&with);
    let base_marginal = base_with - base_without;

    let expected = base_marginal
        + hw * koalisi::algorithms::HISTORY_UNIT * store.history(9) as f64
        - fw * koalisi::algorithms::FAILURE_UNIT * store.failures(9) as f64;

    let policy = ThresholdPolicy::new(
        FeedbackCalculator::new(AdditiveCalculator, hw, fw, store),
        0.0,
        0.0,
    );
    let decision = policy.should_join(&x, &coalition, &ctx);
    assert_eq!(decision.score, expected);
    assert_eq!(decision.score, 135.0);
}

/// `seed_feedback_history` folds the event-sourced membership episode count into
/// the store: an agent that joined two coalitions (one dissolved, one ongoing)
/// seeds a history of 2 and no failures.
#[tokio::test]
async fn feedback_seeding_from_event_log() {
    use koalisi::topology::CoalitionManager;

    let manager = CoalitionManager::<CapabilityAgent, &'static str>::empty();
    let agent = manager
        .add_agent(CapabilityAgent::new(7, 0b0001, 50))
        .await
        .expect("add agent");

    // Episode 1: form a coalition with the agent, then dissolve it (closed range).
    let c1 = manager
        .form_coalition(vec![agent], "alpha")
        .await
        .expect("form c1");
    manager.dissolve_coalition(c1).await.expect("dissolve c1");

    // Episode 2: form another coalition, left open (ongoing membership).
    let _c2 = manager
        .form_coalition(vec![agent], "beta")
        .await
        .expect("form c2");

    let store = FeedbackStore::new(100.0);
    manager
        .seed_feedback_history(&[agent], &store)
        .await
        .expect("seed history");

    assert_eq!(store.history(7), 2, "two membership episodes seeded");
    assert_eq!(store.failures(7), 0, "log seeds no failures");
}

// =========================================================================
// DCVC tests
// =========================================================================

#[test]
fn dcvc_proportional_distribution() {
    let agents = [Agent::new(1, 0b001, 90), Agent::new(2, 0b010, 60)];
    let refs = as_caps(&agents);

    let dist = DCVCDistributor::distribute_workload(&refs, 100);

    let fast = dist.get_agent_share(1).unwrap();
    let slow = dist.get_agent_share(2).unwrap();

    assert!(fast.share_size >= slow.share_size);
    assert_eq!(fast.share_size + slow.share_size, 100);
}

#[test]
fn dcvc_equal_speeds() {
    let agents = [
        Agent::new(1, 0b001, 80),
        Agent::new(2, 0b010, 80),
        Agent::new(3, 0b100, 80),
    ];
    let refs = as_caps(&agents);

    let dist = DCVCDistributor::distribute_workload(&refs, 90);

    let s1 = dist.get_agent_share(1).unwrap().share_size;
    let s2 = dist.get_agent_share(2).unwrap().share_size;
    let s3 = dist.get_agent_share(3).unwrap().share_size;

    assert!((s1 as i32 - s2 as i32).abs() <= 1);
    assert!((s2 as i32 - s3 as i32).abs() <= 1);
}

#[test]
fn dcvc_single_agent() {
    let agents = [Agent::new(1, 0b111, 95)];
    let refs = as_caps(&agents);

    let dist = DCVCDistributor::distribute_workload(&refs, 50);
    assert_eq!(dist.get_agent_share(1).unwrap().share_size, 50);
}

#[test]
fn dcvc_verification() {
    let agents = [
        Agent::new(1, 0b001, 90),
        Agent::new(2, 0b010, 70),
        Agent::new(3, 0b100, 80),
    ];
    let refs = as_caps(&agents);

    let dist = DCVCDistributor::distribute_workload(&refs, 100);
    assert!(dist.verify_distribution(100));
}

#[test]
fn dcvc_empty() {
    let dist = DCVCDistributor::distribute_workload(&[], 100);
    assert!(dist.get_all_shares().is_empty());
}

// =========================================================================
// AIPA tests
// =========================================================================

#[test]
fn integer_partitions() {
    // Explicit element type keeps inference unambiguous when a `serde_json`
    // (`usize: PartialEq<Value>`) impl is linked under the `durable` feature.
    assert_eq!(generate_integer_partitions(0), vec![Vec::<usize>::new()]);
    assert_eq!(generate_integer_partitions(1).len(), 1);
    assert_eq!(generate_integer_partitions(4).len(), 5);
    assert_eq!(partition_count(5), 7);
}

#[test]
fn partitions_sum_to_n() {
    for n in 1..=8 {
        for p in generate_integer_partitions(n) {
            assert_eq!(p.iter().sum::<usize>(), n);
        }
    }
}

#[test]
fn partition_bounds() {
    let mut values = HashMap::new();
    values.insert(2, vec![100.0, 150.0, 120.0]);
    values.insert(1, vec![50.0, 60.0]);

    let bounds = compute_all_partition_bounds(2, &values);
    assert_eq!(bounds.len(), 2);
    assert!(bounds[0].upper_bound >= bounds[1].upper_bound);
}

#[test]
fn best_partition() {
    let mut values = HashMap::new();
    values.insert(2, vec![100.0, 150.0]);
    values.insert(1, vec![50.0]);

    let best = find_best_partition(2, &values).unwrap();
    assert_eq!(best.partition, vec![2]);
    assert_eq!(best.upper_bound, 150.0);
}

#[test]
fn verify_partition_validity() {
    assert!(verify_partition(&vec![3, 2, 1], 6));
    assert!(verify_partition(&vec![4], 4));
    assert!(!verify_partition(&vec![3, 2], 6));
    assert!(!verify_partition(&vec![1, 3, 2], 6));
}
