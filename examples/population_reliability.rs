//! Population coalition-structure search driven by a *learned* value model
//! (issue #57) — the slow loop reading the fast loop's world model.
//!
//! `examples/population_search.rs` searches against a hand-written `TaskCoverage`
//! value model. This example replaces it with [`ReliabilityCoverage`]: the same
//! coverage shape, but each capability bit weighted by how reliable its providers
//! have proven to be, read straight out of the E1 persistent Active Inference world
//! model (the K4-v5 arm). A short synthetic outcome stream teaches the arm that
//! bit 2 is a problem, and the search then ranks structures under a fitness that
//! knows it (rescaling their values — see the [`ReliabilityCoverage`] docs for
//! why, at these coefficients, it re-ranks rather than routes around a weak bit).
//!
//! Reliability lives per *bit*, not per agent (see the [`ReliabilityCoverage`]
//! module docs) — the world model has no agent-indexed factor.
//!
//! ```sh
//! cargo run --features decision --example population_reliability
//! ```

use anyhow::Result;

use koalisi::algorithms::{
    AgentCapabilities, CapabilityAgent, PopulationConfig, ValueCalculator, record_trajectory,
    search,
};
use koalisi::decision::{PersistentAifArm, PersistentAifConfig, ReliabilityCoverage};
use koalisi::topology::{CoalitionManager, Timestamp};

/// The 4-bit task the coalition is formed for.
const REQUIRED: u32 = 0b1111;
/// Task outcomes fed into the persistent world model.
const N_TASKS: usize = 20;

/// Deterministic outcome pattern: bits 0/1/3 are dependable (they fail only on
/// every tenth task), bit 2's providers succeed only on every fifth.
fn per_bit_success(task: usize) -> [bool; 8] {
    let dependable = task % 10 != 9;
    let flaky = task.is_multiple_of(5);
    [dependable, dependable, flaky, dependable, false, false, false, false]
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_target(false).try_init().ok();

    println!("=== Reliability-weighted population search (issue #57) ===\n");

    // --- 1. Teach the persistent world model which bits are dependable ---
    // query_dynamics off = the E1 (K4-v5) configuration; the persistent belief
    // read is identical either way (dynamics shape only the query POMDP).
    let config = PersistentAifConfig {
        query_dynamics: false,
        ..Default::default()
    };
    let arm = PersistentAifArm::new(27, config)?;
    for task in 0..N_TASKS {
        arm.observe_outcome(REQUIRED, &per_bit_success(task));
    }
    let snapshot = arm.state_snapshot();
    let calc = ReliabilityCoverage::from_state(REQUIRED, &snapshot);

    // `beliefs[b][0]` is the world model's smoothed posterior at the last node of
    // its 2-step MMP window, so it tracks RECENT outcomes, not the 20-task
    // average, and its absolute scale is not a success rate. What the value model
    // consumes is the ordering across bits.
    println!("Per-bit reliability posterior after {N_TASKS} task outcomes:");
    for (bit, &r) in calc.reliability().iter().take(4).enumerate() {
        let recent: Vec<bool> = ((N_TASKS - 3)..N_TASKS).map(|t| per_bit_success(t)[bit]).collect();
        println!("  bit {bit}: reliability {r:.4}   (last 3 tasks: {recent:?})");
    }
    println!("  (bit 2 ranks lowest, as its outcome stream implies)");

    // --- 2. Search coalition structures against the learned model ---
    let agents = vec![
        CapabilityAgent::new(0, 0b0001, 80), // sensing
        CapabilityAgent::new(1, 0b0010, 70), // processing
        CapabilityAgent::new(2, 0b0100, 90), // comms
        CapabilityAgent::new(3, 0b1000, 60), // storage
        CapabilityAgent::new(4, 0b0011, 75), // sensing + processing
        CapabilityAgent::new(5, 0b1100, 85), // comms + storage
    ];
    let cfg = PopulationConfig::default().with_seed(27);
    let outcome = search(&agents, &calc, &cfg);

    println!("\nBest coalition structure (fitness = {:.2}):", outcome.best.fitness);
    for (id, block) in outcome.best.blocks().iter().enumerate() {
        let ids: Vec<usize> = block.iter().map(|&i| agents[i].agent_id()).collect();
        let union = block.iter().fold(0u32, |acc, &i| acc | agents[i].capabilities());
        let members: Vec<&dyn AgentCapabilities> = block
            .iter()
            .map(|&i| &agents[i] as &dyn AgentCapabilities)
            .collect();
        println!(
            "  block {id}: agents {ids:?}  covers 0b{union:04b}  value {:.2}",
            calc.calculate_value(&members)
        );
    }
    println!(
        "\nImprovement lineage: {} epochs over {} iterations",
        outcome.lineage.len(),
        outcome.iterations_run
    );
    println!(
        "(reliability is per capability BIT, not per agent — blocks covering the \
         same bits score identically beyond member count)"
    );

    // --- 3. Execute the discovered structure on the runtime ---
    let manager: CoalitionManager<CapabilityAgent, u32> = CoalitionManager::empty();
    record_trajectory(&manager, &agents, &outcome.lineage).await?;

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
