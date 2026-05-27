//! Reference daemon: assemble a swarm, attach a scripted live feed, wait
//! for ctrl-c, then drain cleanly.

use anyhow::Result;
use std::time::Duration;
use tokio::time::interval;
use tokio_util::sync::CancellationToken;

use koalisi::core::config::setup_logging;
use koalisi::market::{Pair, Tick, Triangle};
use koalisi::subsystems::swarm::{Swarm, SwarmConfig};

#[tokio::main]
async fn main() -> Result<()> {
    setup_logging();

    let triangle = Triangle::new(
        "EUR/USD".parse()?,
        "GBP/USD".parse()?,
        "EUR/GBP".parse()?,
    )?;
    let config = SwarmConfig::from_settings(vec![triangle]);

    let swarm = Swarm::new(config).await?;

    // Spawn a scripted "live" feeder. In a real deployment this would be a
    // websocket subscriber pumping ticks into `swarm.feed_tick(...)`.
    let live_token = swarm.cancellation_token().child_token();
    let tick_bus_handle = spawn_demo_feed(&swarm, live_token);

    tokio::signal::ctrl_c().await?;
    tracing::info!("ctrl-c received, shutting down.");

    // The token is owned by `swarm.cancellation_token()`; cancelling here
    // (or letting `shutdown` cancel it) tells the feeder task to stop.
    swarm.shutdown().await;
    let _ = tick_bus_handle.await;
    Ok(())
}

/// Spawns a scripted feeder that walks the EUR/USD, GBP/USD, EUR/GBP
/// triangle around a slowly-drifting baseline. Yields control on the swarm
/// cancellation token.
fn spawn_demo_feed(swarm: &Swarm, token: CancellationToken) -> tokio::task::JoinHandle<()> {
    // Capture only what we need for the spawn — concrete refs, no `&self`.
    let eur_usd: Pair = "EUR/USD".parse().unwrap();
    let gbp_usd: Pair = "GBP/USD".parse().unwrap();
    let eur_gbp: Pair = "EUR/GBP".parse().unwrap();

    let m_eur_usd = swarm.monitor(&eur_usd).unwrap().clone();
    let m_gbp_usd = swarm.monitor(&gbp_usd).unwrap().clone();
    let m_eur_gbp = swarm.monitor(&eur_gbp).unwrap().clone();

    tokio::spawn(async move {
        let mut clock = interval(Duration::from_millis(250));
        let mut t = 0u64;
        loop {
            tokio::select! {
                _ = token.cancelled() => {
                    tracing::info!("demo feeder cancelled");
                    return;
                }
                _ = clock.tick() => {}
            }

            // Drift the synthetic equilibrium and occasionally let the
            // cross drift away from it.
            let drift = (t as f64 * 0.0001).sin() * 0.0002;
            let eu = 1.1000 + drift;
            let gu = 1.3000 - drift * 0.5;
            // synthetic EUR/GBP = eu / gu — start aligned, drift slightly
            // every ~12 ticks to give the coordinator something to do.
            let syn = eu / gu;
            let cross_drift = if t % 12 == 11 { 0.0007 } else { 0.0 };
            let eg = syn + cross_drift;
            let now_ms = (t as i64) * 250;

            let _ = m_eur_usd
                .tell(Tick::new(eur_usd.clone(), eu - 0.0002, eu + 0.0002, now_ms))
                .await;
            let _ = m_gbp_usd
                .tell(Tick::new(gbp_usd.clone(), gu - 0.0002, gu + 0.0002, now_ms))
                .await;
            let _ = m_eur_gbp
                .tell(Tick::new(eur_gbp.clone(), eg - 0.0001, eg + 0.0001, now_ms))
                .await;
            t = t.wrapping_add(1);
        }
    })
}
