//! Population-based coalition-structure search atop AIPA (issue #42, P5.2) —
//! default features, no credentials.
//!
//! Runs the deterministic `SwarmAgentic`-style swarm over a small pool of
//! capability agents, prints the best coalition structure and the global-best
//! improvement lineage, then **executes** that best structure on the runtime:
//! it records the lineage into a `CoalitionManager` and replays the final epoch
//! back out through the temporal-hypergraph query layer.
//!
//! The value model here is a pluggable `TaskCoverage` [`ValueCalculator`] that
//! rewards blocks which *fully cover* a required capability set with as few
//! members as possible. Its optimum is a genuinely non-trivial partition —
//! complementary specialists grouped into minimal full-coverage teams — so the
//! swarm has real work to do and the improvement lineage spans several epochs.
//!
//! ```sh
//! cargo run --example population_search
//! ```

use anyhow::Result;

use koalisi::algorithms::{
    AgentCapabilities, CapabilityAgent, PopulationConfig, ValueCalculator, record_trajectory,
    search,
};
use koalisi::topology::{CoalitionManager, Timestamp};

/// Rewards a block for covering `required` capabilities cheaply: a full-coverage
/// team earns a flat bonus, partial coverage a pro-rated one, and every member
/// costs a little — so redundant members and over-large teams are penalised. The
/// optimum groups complementary specialists into minimal full-coverage teams.
struct TaskCoverage {
    required: u32,
}

impl ValueCalculator for TaskCoverage {
    #[allow(clippy::cast_precision_loss)] // small bounded popcounts / team sizes
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
        let member_cost = agents.len() as f64 * 8.0;
        coverage - member_cost
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_target(false).try_init().ok();

    // Four pure specialists plus two 2-capability generalists over a 4-bit task.
    let agents = vec![
        CapabilityAgent::new(0, 0b0001, 80), // sensing
        CapabilityAgent::new(1, 0b0010, 70), // processing
        CapabilityAgent::new(2, 0b0100, 90), // comms
        CapabilityAgent::new(3, 0b1000, 60), // storage
        CapabilityAgent::new(4, 0b0011, 75), // sensing + processing
        CapabilityAgent::new(5, 0b1100, 85), // comms + storage
    ];
    let calc = TaskCoverage { required: 0b1111 };

    let cfg = PopulationConfig::default().with_seed(27);
    let outcome = search(&agents, &calc, &cfg);

    println!("=== Population search (SwarmAgentic idea #4, issue #42) ===\n");
    println!(
        "agents={}  required=0b{:04b}  population={}  iterations={}  seed={}",
        agents.len(),
        calc.required,
        cfg.population,
        cfg.iterations,
        cfg.seed
    );
    println!("\nBest coalition structure (fitness = {:.1}):", outcome.best.fitness);
    for (id, block) in outcome.best.blocks().iter().enumerate() {
        let ids: Vec<usize> = block.iter().map(|&i| agents[i].agent_id()).collect();
        let union = block.iter().fold(0u32, |acc, &i| acc | agents[i].capabilities());
        println!("  block {id}: agents {ids:?}  covers 0b{union:04b}");
    }

    println!(
        "\nGlobal-best improvement lineage ({} epochs, iterations_run={}):",
        outcome.lineage.len(),
        outcome.iterations_run
    );
    for (epoch, cs) in outcome.lineage.iter().enumerate() {
        println!("  epoch {epoch}: fitness {:.1}  blocks={:?}", cs.fitness, cs.blocks());
    }

    // --- Execute the discovered structure on the runtime ---
    println!("\n=== Executing best structure on the runtime ===\n");
    let manager: CoalitionManager<CapabilityAgent, u32> = CoalitionManager::empty();
    record_trajectory(&manager, &agents, &outcome.lineage).await?;
    println!(
        "recorded {} agents and {} lineage epochs into the temporal hypergraph",
        agents.len(),
        outcome.lineage.len()
    );

    // Replay the final epoch back out via the temporal-query layer.
    let hidxs: Vec<_> = {
        let events = manager.graph().events_ref().read().await;
        events.hyperedge_indices().collect()
    };
    println!("\nReplayed live coalitions at the final epoch:");
    for h in hidxs {
        if let Some(members) = manager.coalition_members_at(h, Timestamp::MAX).await {
            let ids: Vec<usize> = members.iter().map(|v| agents[v.get()].agent_id()).collect();
            println!("  coalition {}: agents {ids:?}", h.get());
        }
    }

    Ok(())
}
