//! Demonstrates the topology layer: temporal hypergraph with event-sourced
//! coalition management and time-travel queries.
//!
//! ```sh
//! cargo run --example topology_coalition
//! ```

use koalisi::topology::{
    CoalitionManager, TemporalAnalytics, TemporalQueries, TimeRange, Timestamp,
};
use std::fmt::{Display, Formatter};

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
struct Agent {
    id: &'static str,
    capabilities: u32,
}

impl Agent {
    const fn new(id: &'static str, capabilities: u32) -> Self {
        Self { id, capabilities }
    }
}

impl Display for Agent {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Agent({})", self.id)
    }
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
struct Coalition {
    name: &'static str,
    cost: usize,
}

impl Coalition {
    const fn new(name: &'static str, cost: usize) -> Self {
        Self { name, cost }
    }
}

impl Display for Coalition {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Coalition({})", self.name)
    }
}

impl From<Coalition> for usize {
    fn from(c: Coalition) -> Self {
        c.cost
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let manager = CoalitionManager::<Agent, Coalition>::empty();

    // --- Phase 1: build the team ---
    println!("=== Phase 1: Adding agents ===");
    let alice = manager.add_agent(Agent::new("alice", 0b001)).await?;
    let bob = manager.add_agent(Agent::new("bob", 0b010)).await?;
    let carol = manager.add_agent(Agent::new("carol", 0b100)).await?;
    let dave = manager.add_agent(Agent::new("dave", 0b011)).await?;
    println!("  agents: {} total", manager.count_agents().await);

    // --- Phase 2: form coalitions ---
    println!("\n=== Phase 2: Forming coalitions ===");
    let alpha = manager
        .form_coalition(vec![alice, bob], Coalition::new("alpha", 10))
        .await?;
    let beta = manager
        .form_coalition(vec![carol, dave], Coalition::new("beta", 8))
        .await?;
    println!("  coalitions: {}", manager.count_coalitions().await);

    let alpha_members = manager.coalition_members(alpha).await?;
    println!("  alpha members: {}", alpha_members.len());

    // --- Phase 3: agent joins ---
    println!("\n=== Phase 3: Carol joins alpha ===");
    manager.join_coalition(carol, alpha).await?;
    let alpha_members = manager.coalition_members(alpha).await?;
    println!("  alpha now has {} members", alpha_members.len());

    // --- Phase 4: time-travel queries ---
    println!("\n=== Phase 4: Time-travel queries ===");
    let events_ref = manager.graph().events_ref();

    let agents_at_t0 = TemporalQueries::count_vertices_at(events_ref, Timestamp::new(0)).await;
    println!("  agents at T=0: {}", agents_at_t0);

    let agents_at_t3 = TemporalQueries::count_vertices_at(events_ref, Timestamp::new(3)).await;
    println!("  agents at T=3: {}", agents_at_t3);

    let coalitions_at_t4 =
        TemporalQueries::count_hyperedges_at(events_ref, Timestamp::new(4)).await;
    println!("  coalitions at T=4 (before formation): {}", coalitions_at_t4);

    // --- Phase 5: merge coalitions ---
    println!("\n=== Phase 5: Merging alpha + beta ===");
    manager.merge_coalitions(vec![alpha, beta]).await?;
    println!("  coalitions after merge: {}", manager.count_coalitions().await);

    // --- Phase 6: analytics ---
    println!("\n=== Phase 6: Analytics ===");
    let range = TimeRange::all();
    let event_count =
        TemporalAnalytics::event_count_in_range(events_ref, &range).await;
    println!("  total events recorded: {}", event_count);

    let stats = manager.graph().event_stats().await;
    println!("  vertex adds: {}", stats.vertex_added);
    println!("  hyperedge adds: {}", stats.hyperedge_added);
    println!("  transforms: {}", stats.transforms);

    println!("\ndone.");
    Ok(())
}
