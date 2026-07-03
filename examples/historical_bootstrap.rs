//! Bootstrap a single EUR/USD monitor with a small history vector, then
//! ask it for its snapshot. Demonstrates the per-pair ring buffer and the
//! "history-only" lifecycle (no live ticks, no triangle resolved).

use anyhow::Result;

use koalisi::market::{Pair, Tick, Triangle};
use koalisi::subsystems::monitor::MonitorSnapshot;
use koalisi::subsystems::swarm::{Swarm, SwarmConfig};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_target(false).try_init().ok();

    // Build the smallest meaningful swarm (one triangle's worth of monitors).
    // We only feed one pair here; the other two monitors remain idle.
    let triangle = Triangle::new(
        "EUR/USD".parse()?,
        "GBP/USD".parse()?,
        "EUR/GBP".parse()?,
    )?;
    let swarm = Swarm::new(SwarmConfig {
        triangles: vec![triangle],
        threshold_bps: 5.0,
        history_capacity: 8,
    })
    .await?;

    let pair: Pair = "EUR/USD".parse()?;
    let history = (0..10).map(|i| {
        let mid = 1.0900 + (i as f64) * 0.0001;
        Tick::new(pair.clone(), mid - 0.0001, mid + 0.0001, (i as i64) * 100)
    });
    swarm.replay_history(history.collect()).await?;

    // Inspect the monitor.
    let monitor = swarm.monitor(&pair).expect("EUR/USD monitor exists");
    let snapshot: MonitorSnapshot = monitor.snapshot().await?;
    println!(
        "{} → latest mid = {:.5}, history len = {} (capped at 8)",
        snapshot.key,
        snapshot.latest.unwrap().mid,
        snapshot.history.len()
    );
    println!("history ticks:");
    for t in &snapshot.history {
        println!("  t={} mid={:.5}", t.timestamp_ms, t.mid());
    }

    // No alerts should have fired — only one of three pairs has data.
    let alerts = swarm.alerts().await?;
    assert!(
        alerts.is_empty(),
        "no alerts expected when only one leg is fed, got {}",
        alerts.len()
    );
    println!("alerts fired: {} (expected 0)", alerts.len());

    swarm.shutdown().await;
    Ok(())
}
