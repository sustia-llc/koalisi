//! Two-role demo of the libp2p-based remote alert gateway.
//!
//! Run the **producer** in one terminal:
//! ```sh
//! ROLE=producer cargo run --features remote --example distributed_alert_consumer
//! ```
//!
//! Run one or more **consumers** in other terminals:
//! ```sh
//! ROLE=consumer cargo run --features remote --example distributed_alert_consumer
//! ```
//!
//! The producer assembles a swarm with a scripted feeder, enables the
//! remote alert gateway, and stays up until `ctrl-c`. Consumers come up
//! with their own libp2p peer (mDNS for local discovery), wait for the
//! producer's gateway to appear in the registry, then poll it every few
//! seconds and print everything new.
//!
//! What's exercised:
//! - kameo's `remote::Behaviour` composed with `mdns` for discovery
//! - `Swarm::shutdown()` cancelling the libp2p event-loop task on the
//!   producer side (via the child cancellation token)
//! - The full wire round-trip: gateway's `#[remote_message] PollOpportunities`
//!   serialised over libp2p request-response → consumer deserialises into
//!   `Vec<ArbitrageOpportunity>`
//!
//! For single-host development, the producer's gateway is also
//! findable from within its own process — this example doesn't exercise
//! that path. See `tests/remote_integration.rs` for the in-process
//! round-trip verification (skipping the two-terminal setup).
//!
//! Note: the producer/consumer roles share a single binary so the example
//! list stays short. A production layout would split them.

use std::time::Duration;

use anyhow::{Result, anyhow};
use futures::TryStreamExt;
use kameo::prelude::*;
use kameo_actors::DeliveryStrategy;
use tokio::time::{interval, sleep};

use forex_arbitrage_swarm::market::{Pair, Tick, Triangle};
use forex_arbitrage_swarm::subsystems::distributed::{
    PollOpportunities, RemoteAlertGateway, RemoteConfig, enable_remote_alerts,
};
use forex_arbitrage_swarm::subsystems::swarm::{Swarm, SwarmConfig};

const GATEWAY_NAME: &str = "forex_swarm_alerts";

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,libp2p_mdns=warn".parse().unwrap()),
        )
        .with_target(false)
        .try_init()
        .ok();

    let role = std::env::var("ROLE").unwrap_or_else(|_| "producer".into());
    match role.as_str() {
        "producer" => run_producer().await,
        "consumer" => run_consumer().await,
        other => Err(anyhow!(
            "unknown ROLE={other:?}, expected \"producer\" or \"consumer\""
        )),
    }
}

// ---------------------------------------------------------------------------
// Producer
// ---------------------------------------------------------------------------

async fn run_producer() -> Result<()> {
    let triangle = Triangle::new("EUR/USD".parse()?, "GBP/USD".parse()?, "EUR/GBP".parse()?)?;
    let swarm = Swarm::new(SwarmConfig {
        triangles: vec![triangle],
        threshold_bps: 5.0,
        history_capacity: 256,
        delivery_strategy: DeliveryStrategy::Guaranteed,
    })
    .await?;

    let handle = enable_remote_alerts(
        &swarm,
        RemoteConfig {
            gateway_name: GATEWAY_NAME.into(),
            ..RemoteConfig::default()
        },
    )
    .await?;
    println!("✓ producer up");
    println!("  peer id : {}", handle.local_peer_id);
    println!("  gateway : '{GATEWAY_NAME}' registered");
    println!("  feeding scripted triangle (one arb every ~3s); ctrl-c to stop");

    // Scripted feeder: alternates aligned and dislocated cross prices so
    // an arbitrage opportunity fires every ~12 ticks. Same hysteresis
    // dance as the original `examples/triangular_arbitrage.rs`.
    let feeder = swarm.feeder();
    let feed_token = swarm.cancellation_token().child_token();
    swarm.task_tracker().spawn(async move {
        let eu: Pair = "EUR/USD".parse().unwrap();
        let gu: Pair = "GBP/USD".parse().unwrap();
        let eg: Pair = "EUR/GBP".parse().unwrap();
        let mut clock = interval(Duration::from_millis(250));
        let mut t: u64 = 0;

        loop {
            tokio::select! {
                _ = feed_token.cancelled() => return,
                _ = clock.tick() => {}
            }
            let now = (t as i64) * 250;
            let eu_mid = 1.1000;
            let gu_mid = 1.3000;
            let synthetic = eu_mid / gu_mid;
            let cross_mid = if t % 12 == 4 {
                synthetic + 0.0010
            } else {
                synthetic
            };

            let _ = feeder
                .feed_tick(Tick::new(eu.clone(), eu_mid - 0.0001, eu_mid + 0.0001, now))
                .await;
            let _ = feeder
                .feed_tick(Tick::new(gu.clone(), gu_mid - 0.0001, gu_mid + 0.0001, now))
                .await;
            let _ = feeder
                .feed_tick(Tick::new(
                    eg.clone(),
                    cross_mid - 0.0001,
                    cross_mid + 0.0001,
                    now,
                ))
                .await;
            t = t.wrapping_add(1);
        }
    });

    tokio::signal::ctrl_c().await?;
    println!("\n✓ ctrl-c received, shutting down producer");
    swarm.shutdown().await;
    Ok(())
}

// ---------------------------------------------------------------------------
// Consumer
// ---------------------------------------------------------------------------

async fn run_consumer() -> Result<()> {
    // The consumer still needs a libp2p swarm of its own to do
    // discovery + RPC. The easiest way to spin one up with the same
    // shape is to build an empty Swarm (no triangles) and reuse
    // `enable_remote_alerts` — its libp2p side-effect (init_global +
    // listen + event-loop task) is what we actually want here.
    let placeholder_triangle =
        Triangle::new("EUR/USD".parse()?, "GBP/USD".parse()?, "EUR/GBP".parse()?)?;
    let swarm = Swarm::new(SwarmConfig {
        triangles: vec![placeholder_triangle],
        threshold_bps: 1_000_000.0, // never fire locally
        history_capacity: 1,
        delivery_strategy: DeliveryStrategy::BestEffort,
    })
    .await?;

    let handle = enable_remote_alerts(
        &swarm,
        RemoteConfig {
            // Different gateway name so consumers don't shadow the producer.
            gateway_name: "forex_swarm_consumer_loopback".into(),
            ..RemoteConfig::default()
        },
    )
    .await?;
    println!("✓ consumer up");
    println!("  peer id : {}", handle.local_peer_id);
    println!("  hunting for '{GATEWAY_NAME}' on the local network via mDNS ...");

    // Poll every 3s. The first few iterations may show no producer
    // (mDNS discovery has a small delay).
    let mut last_seen = 0usize;
    let mut clock = interval(Duration::from_secs(3));
    loop {
        clock.tick().await;
        let mut found_any = false;
        let mut peers = RemoteActorRef::<RemoteAlertGateway>::lookup_all(GATEWAY_NAME);
        while let Some(peer) = peers.try_next().await? {
            // Skip ourselves — our `enable_remote_alerts` call registered
            // a loopback gateway too (under a different name, but
            // belt-and-suspenders).
            if peer.id().peer_id() == Some(&handle.local_peer_id) {
                continue;
            }
            found_any = true;
            match peer.ask(&PollOpportunities).send().await {
                Ok(opps) => {
                    if opps.len() > last_seen {
                        println!(
                            "[peer {}] {} opportunities (prev {}):",
                            peer.id()
                                .peer_id()
                                .map(|p| p.to_base58())
                                .unwrap_or_else(|| "?".into()),
                            opps.len(),
                            last_seen,
                        );
                        for opp in opps.iter().skip(last_seen) {
                            println!("    {opp}");
                        }
                        last_seen = opps.len();
                    }
                }
                Err(err) => eprintln!("ask failed: {err}"),
            }
        }
        if !found_any {
            println!("(no producer found yet — make sure ROLE=producer is running)");
            // Give mDNS a moment on first iterations.
            sleep(Duration::from_millis(500)).await;
        }
    }
}
