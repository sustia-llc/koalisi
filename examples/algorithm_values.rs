//! Demonstrates the algorithm layer: coalition value calculators, DCVC
//! workload distribution, and AIPA partition search.
//!
//! ```sh
//! cargo run --example algorithm_values
//! ```

use koalisi::algorithms::{
    AdditiveCalculator, AgentCapabilities, DCVCDistributor, SynergisticCalculator,
    ValueCalculator, WeightedCalculator, compute_all_partition_bounds, find_best_partition,
    generate_integer_partitions,
};
use std::collections::HashMap;

#[derive(Debug, Copy, Clone)]
struct Agent {
    id: usize,
    capabilities: u32,
    trust_level: u32,
}

impl Agent {
    fn new(id: usize, capabilities: u32, trust_level: u32) -> Self {
        Self { id, capabilities, trust_level }
    }
}

impl AgentCapabilities for Agent {
    fn agent_id(&self) -> usize { self.id }
    fn capabilities(&self) -> u32 { self.capabilities }
    fn trust_level(&self) -> u32 { self.trust_level }
}

fn as_caps(agents: &[Agent]) -> Vec<&dyn AgentCapabilities> {
    agents.iter().map(|a| a as &dyn AgentCapabilities).collect()
}

fn main() {
    let agents = vec![
        Agent::new(1, 0b001, 85),  // sensing
        Agent::new(2, 0b010, 90),  // processing
        Agent::new(3, 0b100, 75),  // communication
        Agent::new(4, 0b011, 80),  // sensing + processing
        Agent::new(5, 0b110, 95),  // processing + communication
    ];
    let refs = as_caps(&agents);

    // --- Value calculators ---
    println!("=== Coalition Value Calculators ===\n");

    let additive = AdditiveCalculator;
    let synergistic = SynergisticCalculator::new();
    let weighted = WeightedCalculator::balanced();

    let team_ab = as_caps(&agents[0..2]);
    let team_abc = as_caps(&agents[0..3]);
    let team_all = as_caps(&agents);

    println!("Team [1,2]   additive={:.0} synergistic={:.0} weighted={:.0}",
        additive.calculate_value(&team_ab),
        synergistic.calculate_value(&team_ab),
        weighted.calculate_value(&team_ab));

    println!("Team [1,2,3] additive={:.0} synergistic={:.0} weighted={:.0}",
        additive.calculate_value(&team_abc),
        synergistic.calculate_value(&team_abc),
        weighted.calculate_value(&team_abc));

    println!("Team [all]   additive={:.0} synergistic={:.0} weighted={:.0}",
        additive.calculate_value(&team_all),
        synergistic.calculate_value(&team_all),
        weighted.calculate_value(&team_all));

    // --- DCVC workload distribution ---
    println!("\n=== DCVC Workload Distribution (100 coalitions) ===\n");

    let dist = DCVCDistributor::distribute_workload(&refs, 100);
    for id in 1..=5 {
        let share = dist.get_agent_share(id).unwrap();
        println!("  Agent {} (trust={}): {} coalitions",
            id, agents[id - 1].trust_level, share.share_size);
    }

    let (min, max, avg, total) = dist.calculate_statistics();
    println!("\n  total={} min={} max={} avg={:.1}", total, min, max, avg);
    println!("  exhaustive+disjoint: {}", dist.verify_distribution(100));

    // --- AIPA partition search ---
    println!("\n=== AIPA Partition Search (n=5) ===\n");

    let partitions = generate_integer_partitions(5);
    println!("  {} integer partitions of 5", partitions.len());
    for p in &partitions {
        println!("    {:?}", p);
    }

    let mut values_by_size: HashMap<usize, Vec<f64>> = HashMap::new();
    values_by_size.insert(1, vec![85.0, 90.0, 75.0, 80.0, 95.0]);
    values_by_size.insert(2, vec![200.0, 250.0, 180.0]);
    values_by_size.insert(3, vec![400.0, 350.0]);
    values_by_size.insert(5, vec![800.0]);

    println!("\n  partition bounds (sorted by upper bound):");
    let bounds = compute_all_partition_bounds(5, &values_by_size);
    for b in bounds.iter().take(5) {
        println!("    {:?} upper={:.0} lower={:.0}", b.partition, b.upper_bound, b.lower_bound);
    }

    let best = find_best_partition(5, &values_by_size).unwrap();
    println!("\n  best partition: {:?} (upper bound={:.0})", best.partition, best.upper_bound);
}
