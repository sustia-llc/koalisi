//! Integration tests for the population-based coalition-structure search
//! (issue #42, P5.2). Default features only.

use std::collections::BTreeSet;

use koalisi::algorithms::{
    CapabilityAgent, PopulationConfig, SynergisticCalculator, ValueCalculator, search,
};
use koalisi::algorithms::population::record_trajectory;
use koalisi::topology::{CoalitionManager, Timestamp};

/// A modest, capability-diverse agent pool.
fn agents() -> Vec<CapabilityAgent> {
    vec![
        CapabilityAgent::new(0, 0b0001, 80),
        CapabilityAgent::new(1, 0b0010, 70),
        CapabilityAgent::new(2, 0b0100, 90),
        CapabilityAgent::new(3, 0b1000, 60),
        CapabilityAgent::new(4, 0b0011, 75),
        CapabilityAgent::new(5, 0b1100, 85),
    ]
}

/// Partition an assignment into a canonical set-of-sets for order-free comparison.
fn partition_set(blocks: &[Vec<usize>]) -> BTreeSet<BTreeSet<usize>> {
    blocks
        .iter()
        .map(|b| b.iter().copied().collect::<BTreeSet<usize>>())
        .collect()
}

#[test]
fn same_seed_is_deterministic_different_seed_may_differ() {
    let agents = agents();
    let cfg = PopulationConfig::default().with_seed(42);

    let a = search(&agents, &SynergisticCalculator, &cfg);
    let b = search(&agents, &SynergisticCalculator, &cfg);
    assert_eq!(a, b, "same seed must reproduce the entire SearchOutcome");

    // A different seed is *allowed* to differ; we only assert both are valid
    // partitions of all agents (no panic, full coverage).
    let c = search(&agents, &SynergisticCalculator, &cfg.with_seed(99));
    assert_eq!(
        c.best.blocks().iter().map(Vec::len).sum::<usize>(),
        agents.len()
    );
}

#[test]
fn gbest_is_monotone_and_beats_baselines() {
    let agents = agents();
    let outcome = search(
        &agents,
        &SynergisticCalculator,
        &PopulationConfig::default().with_seed(7),
    );

    // Strictly increasing lineage, best == last entry.
    for pair in outcome.lineage.windows(2) {
        assert!(pair[1].fitness > pair[0].fitness);
    }
    assert_eq!(outcome.best, *outcome.lineage.last().unwrap());

    // Baselines computed directly. The population seeds BOTH the grand-coalition
    // shape [n] and the all-singleton shape [1^n] (population 32 >> partition
    // count), so the initial gbest already dominates both — and search never
    // regresses the gbest.
    let calc = SynergisticCalculator;
    let all: Vec<&dyn koalisi::algorithms::AgentCapabilities> =
        agents.iter().map(|a| a as &dyn koalisi::algorithms::AgentCapabilities).collect();
    let single_block = calc.calculate_value(&all);
    let all_singletons: f64 = agents
        .iter()
        .map(|a| calc.calculate_value(&[a as &dyn koalisi::algorithms::AgentCapabilities]))
        .sum();

    assert!(
        outcome.best.fitness >= single_block,
        "best {} < single-block baseline {single_block}",
        outcome.best.fitness
    );
    assert!(
        outcome.best.fitness >= all_singletons,
        "best {} < all-singleton baseline {all_singletons}",
        outcome.best.fitness
    );
}

#[test]
#[allow(clippy::float_cmp)] // fitness of an empty structure is exactly 0.0 by construction
fn config_edge_cases() {
    // n = 0 ⇒ trivial outcome.
    let empty: Vec<CapabilityAgent> = Vec::new();
    let o0 = search(&empty, &SynergisticCalculator, &PopulationConfig::default());
    assert!(o0.best.assignment.is_empty());
    assert_eq!(o0.best.fitness, 0.0);
    assert_eq!(o0.lineage.len(), 1);
    assert_eq!(o0.iterations_run, 0);

    // n = 1 ⇒ single block.
    let one = vec![CapabilityAgent::new(0, 0b1, 50)];
    let o1 = search(&one, &SynergisticCalculator, &PopulationConfig::default());
    assert_eq!(o1.best.assignment, vec![0]);

    // population = 1 ⇒ still valid, full coverage.
    let cfg1 = PopulationConfig::new(1, 50, 0.4, 0.3, 3);
    let op = search(&agents(), &SynergisticCalculator, &cfg1);
    assert_eq!(op.best.blocks().iter().map(Vec::len).sum::<usize>(), 6);

    // iterations = 0 ⇒ outcome is the initial gbest, lineage length 1.
    let cfg0 = PopulationConfig::new(16, 0, 0.4, 0.3, 5);
    let oi = search(&agents(), &SynergisticCalculator, &cfg0);
    assert_eq!(oi.iterations_run, 0);
    assert_eq!(oi.lineage.len(), 1);
    assert_eq!(oi.best, oi.lineage[0]);
}

/// The #42 acceptance gate: search → `record_trajectory` → replay the final epoch
/// through the temporal-hypergraph query layer and confirm it reproduces the best
/// coalition structure.
#[tokio::test]
async fn replay_reproduces_best_structure() {
    let agents = agents();
    let outcome = search(
        &agents,
        &SynergisticCalculator,
        &PopulationConfig::default().with_seed(2024),
    );

    // `u32` is a valid HyperedgeTrait (Copy + Eq + Debug + Send + Sync) and Default.
    let manager: CoalitionManager<CapabilityAgent, u32> = CoalitionManager::empty();
    record_trajectory(&manager, &agents, &outcome.lineage)
        .await
        .expect("recording the lineage into a fresh manager cannot fail");

    // Reconstruct the live partition at the final recorded state. Agents were added
    // to a FRESH manager in order, so agent index i is VertexIndex(i) (catgraph's
    // monotonic-from-zero index contract).
    let hidxs: Vec<_> = {
        let events = manager.graph().events_ref().read().await;
        events.hyperedge_indices().collect()
    };

    let mut reconstructed: Vec<Vec<usize>> = Vec::new();
    for h in hidxs {
        if let Some(members) = manager.coalition_members_at(h, Timestamp::MAX).await {
            reconstructed.push(members.iter().map(|v| v.get()).collect());
        }
    }

    assert_eq!(
        partition_set(&reconstructed),
        partition_set(&outcome.best.blocks()),
        "replayed final epoch must equal the best coalition structure"
    );
}
