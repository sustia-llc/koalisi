//! End-to-end triangular arbitrage: realistic EUR/USD, GBP/USD, EUR/GBP
//! prices that produce a clear ~45 bps opportunity. Demonstrates the full
//! "swarm collaborates and fires collectively" path on a hand-picked
//! market state.

use anyhow::Result;
use kameo_actors::DeliveryStrategy;

use koalisi::market::{Pair, Tick, Triangle};
use koalisi::subsystems::swarm::{Swarm, SwarmConfig};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_target(false).try_init().ok();

    let triangle = Triangle::new(
        "EUR/USD".parse()?,
        "GBP/USD".parse()?,
        "EUR/GBP".parse()?,
    )?;
    let swarm = Swarm::new(SwarmConfig {
        triangles: vec![triangle.clone()],
        threshold_bps: 10.0,
        history_capacity: 32,
        delivery_strategy: DeliveryStrategy::Guaranteed,
    })
    .await?;

    let eu: Pair = "EUR/USD".parse()?;
    let gu: Pair = "GBP/USD".parse()?;
    let eg: Pair = "EUR/GBP".parse()?;

    // Phase 1 — historical bootstrap aligned to synthetic.
    // EUR/USD=1.10, GBP/USD=1.30 → synthetic EUR/GBP = 1.10/1.30 ≈ 0.84615
    // Cross trades exactly at 0.84615 → no edge.
    let synthetic = 1.10 / 1.30;
    let history = vec![
        Tick::new(eu.clone(), 1.0998, 1.1002, 0),
        Tick::new(gu.clone(), 1.2998, 1.3002, 0),
        Tick::new(eg.clone(), synthetic - 0.0001, synthetic + 0.0001, 0),
    ];
    swarm.replay_history(history).await?;
    let alerts_phase1 = swarm.alerts().await?;
    println!("phase 1 (aligned market): {} alerts", alerts_phase1.len());
    assert!(alerts_phase1.is_empty(), "phase 1 should produce no alerts");

    // Phase 2 — cross dislocates to 0.85, ~45 bps above synthetic.
    let dislocate = vec![Tick::new(eg.clone(), 0.8499, 0.8501, 1_000)];
    swarm.replay_history(dislocate).await?;
    let alerts_phase2 = swarm.alerts().await?;
    println!(
        "phase 2 (cross at 0.85, synthetic ≈ 0.84615): {} alerts",
        alerts_phase2.len()
    );
    for opp in &alerts_phase2 {
        println!("  → {opp}");
    }
    assert_eq!(alerts_phase2.len(), 1, "expected exactly one alert in phase 2");
    let opp = &alerts_phase2[0];
    assert!(opp.edge_bps > 40.0 && opp.edge_bps < 50.0);
    assert_eq!(opp.triangle.cross, eg);

    // Phase 3 — cross re-aligns and the triangle should rearm without
    // firing again. Followed by a new dislocation on the *opposite* side
    // (synthetic > actual) to demonstrate direction-flipping.
    let realign_and_flip = vec![
        Tick::new(eg.clone(), synthetic - 0.0001, synthetic + 0.0001, 2_000),
        Tick::new(eg.clone(), 0.8400, 0.8402, 3_000), // ~73 bps below synthetic
    ];
    swarm.replay_history(realign_and_flip).await?;
    let alerts_phase3 = swarm.alerts().await?;
    println!("phase 3 (re-align, then flip): {} alerts", alerts_phase3.len());
    assert_eq!(
        alerts_phase3.len(),
        2,
        "expected exactly two cumulative alerts (the original + the flipped)"
    );
    let flipped = alerts_phase3.last().unwrap();
    assert!(flipped.edge_bps < -50.0, "edge should be strongly negative");
    println!("  flipped opportunity → {flipped}");

    swarm.shutdown().await;
    Ok(())
}
