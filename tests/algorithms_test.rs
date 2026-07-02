//! Algorithm layer integration tests — value calculators, DCVC, AIPA.

mod common;

use common::algorithms::{Agent, as_caps, test_agents};
use koalisi::algorithms::{
    AdditiveCalculator, DCVCDistributor, MultiplicativeCalculator, SynergisticCalculator,
    ValueCalculator, WeightedCalculator, compute_all_partition_bounds, find_best_partition,
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
