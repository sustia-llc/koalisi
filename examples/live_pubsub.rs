//! Drive the swarm with a "live" tick stream (scripted, deterministic) and
//! print the resulting opportunity stream.
//!
//! Subscribes a custom listener actor to the alert pubsub so the example
//! shows the standard kameo pubsub subscription pattern.

use std::time::Duration;

use anyhow::Result;
use kameo::error::Infallible;
use kameo::prelude::*;
use kameo_actors::{DeliveryStrategy, pubsub::Subscribe};

use forex_arbitrage_swarm::market::{ArbitrageOpportunity, Pair, Tick, Triangle};
use forex_arbitrage_swarm::subsystems::swarm::{Swarm, SwarmConfig};

#[derive(Default)]
struct PrintListener;

impl Actor for PrintListener {
    type Args = Self;
    type Error = Infallible;
    async fn on_start(s: Self::Args, _r: ActorRef<Self>) -> Result<Self, Self::Error> {
        Ok(s)
    }
}

impl Message<ArbitrageOpportunity> for PrintListener {
    type Reply = ();
    async fn handle(
        &mut self,
        opp: ArbitrageOpportunity,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        println!("[live listener] {opp}");
    }
}

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
        delivery_strategy: DeliveryStrategy::Guaranteed,
    })
    .await?;

    // Wire up a custom listener to the alert pubsub.
    let listener = PrintListener::spawn(PrintListener);
    swarm.alert_bus().ask(Subscribe(listener.clone())).await?;

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

    // Tidy up.
    let _ = listener.stop_gracefully().await;
    listener.wait_for_shutdown().await;
    swarm.shutdown().await;
    Ok(())
}
