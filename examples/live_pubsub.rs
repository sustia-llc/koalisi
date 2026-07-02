//! Drive the swarm with a "live" tick stream (scripted, deterministic) and
//! print the resulting opportunity stream.
//!
//! Subscribes a listener task to the alert broadcast so the example shows the
//! standard post-K3 subscription pattern: `swarm.alert_bus().subscribe()`
//! returns a `broadcast::Receiver` immediately — no actor to spawn first, and
//! no "subscribe before the publisher starts" ordering gotcha.

use std::time::Duration;

use anyhow::Result;
use tokio_util::sync::CancellationToken;

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
        triangles: vec![triangle],
        threshold_bps: 5.0,
        history_capacity: 64,
    })
    .await?;

    // Subscribe a listener task to the alert broadcast. It runs on the swarm's
    // TaskTracker under a child cancellation token, so `Swarm::shutdown()`
    // drains it.
    let mut alert_rx = swarm.alert_bus().subscribe();
    let listener_token = swarm.cancellation_token().child_token();
    swarm.task_tracker().spawn(async move {
        listen(&mut alert_rx, listener_token).await;
    });

    // Tick through a small scripted feed:
    //   t=0..3   aligned (no edge)
    //   t=4..7   cross drifts → arb opens
    //   t=8..11  re-aligned → arb closes (re-arms for next deviation)
    let eu: Pair = "EUR/USD".parse()?;
    let gu: Pair = "GBP/USD".parse()?;
    let eg: Pair = "EUR/GBP".parse()?;

    for t in 0..12 {
        let now = t as i64 * 100;
        let eu_mid = 1.1000;
        let gu_mid = 1.3000;
        let synthetic = eu_mid / gu_mid;
        let cross_mid = if (4..8).contains(&t) {
            synthetic + 0.0010 // ~118 bps deviation, well above threshold
        } else {
            synthetic
        };

        swarm
            .feed_tick(Tick::new(eu.clone(), eu_mid - 0.0001, eu_mid + 0.0001, now))
            .await?;
        swarm
            .feed_tick(Tick::new(gu.clone(), gu_mid - 0.0001, gu_mid + 0.0001, now))
            .await?;
        swarm
            .feed_tick(Tick::new(
                eg.clone(),
                cross_mid - 0.0001,
                cross_mid + 0.0001,
                now,
            ))
            .await?;
        // Give the listener task time to print before the next round.
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    let alerts = swarm.alerts().await?;
    println!("\ntotal alerts captured in sink: {}", alerts.len());

    swarm.shutdown().await;
    Ok(())
}

async fn listen(
    rx: &mut tokio::sync::broadcast::Receiver<koalisi::market::ArbitrageOpportunity>,
    token: CancellationToken,
) {
    loop {
        tokio::select! {
            biased;
            _ = token.cancelled() => break,
            r = rx.recv() => match r {
                Ok(opp) => println!("[live listener] {opp}"),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    }
}
