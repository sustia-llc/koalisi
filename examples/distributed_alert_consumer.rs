//! Two-role demo of the raw-libp2p remote alert gateway.
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
//! The producer assembles a swarm with a scripted feeder, enables the remote
//! alert gateway, and stays up until `ctrl-c`. Consumers come up with their own
//! libp2p peer (mDNS for local discovery), wait for a gateway to appear, then
//! poll it every few seconds and print everything new.
//!
//! What's exercised:
//! - `request_response::cbor::Behaviour` composed with `mdns` for discovery
//! - `Swarm::shutdown()` cancelling the libp2p gateway task on the producer
//!   side (via the child cancellation token)
//! - The full wire round-trip: `AlertRequest::Poll` serialised over libp2p
//!   request-response → producer replies with `AlertResponse::Alerts` →
//!   consumer deserialises into `Vec<ArbitrageOpportunity>`
//!
//! Note: the service identity is the protocol name `/koalisi/alerts/1`, not an
//! actor-registry name, so any peer speaking the protocol answers. In the
//! usual one-producer/one-consumer run the consumer polls the producer; if two
//! consumers discover each other, a poll to a non-gateway peer simply fails and
//! is skipped. The producer/consumer roles share one binary so the example list
//! stays short; a production layout would split them.

use std::time::Duration;

use anyhow::{Result, anyhow};
use tokio::time::interval;

use koalisi::market::{Pair, Tick, Triangle};
use koalisi::subsystems::distributed::{
    PROTOCOL_NAME, RemoteAlertClient, RemoteConfig, enable_remote_alerts,
};
use koalisi::subsystems::swarm::{Swarm, SwarmConfig};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

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
    })
    .await?;

    let handle = enable_remote_alerts(&swarm, RemoteConfig::default()).await?;
    println!("✓ producer up");
    println!("  peer id : {}", handle.local_peer_id);
    println!("  gateway : '{PROTOCOL_NAME}' serving alerts");
    println!("  feeding scripted triangle (one arb every ~3s); ctrl-c to stop");

    // Scripted feeder: alternates aligned and dislocated cross prices so an
    // arbitrage opportunity fires every ~12 ticks. Same hysteresis dance as the
    // original `examples/triangular_arbitrage.rs`.
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
    let mut client = RemoteAlertClient::connect(RemoteConfig::default()).await?;
    println!("✓ consumer up");
    println!("  peer id : {}", client.local_peer_id());
    println!("  hunting for a gateway on the local network via mDNS ...");

    let mut last_seen = 0usize;
    loop {
        // Drive discovery + keep connections healthy for a few seconds.
        client.pump(Duration::from_secs(3)).await;

        let peers = client.connected_peers();
        if peers.is_empty() {
            println!("(no gateway found yet — make sure ROLE=producer is running)");
            continue;
        }

        for peer in peers {
            match client.poll(peer, REQUEST_TIMEOUT).await {
                Ok(opps) => {
                    if opps.len() > last_seen {
                        println!(
                            "[peer {}] {} opportunities (prev {}):",
                            peer.to_base58(),
                            opps.len(),
                            last_seen,
                        );
                        for opp in opps.iter().skip(last_seen) {
                            println!("    {opp}");
                        }
                        last_seen = opps.len();
                    }
                }
                Err(err) => eprintln!("poll to {} failed (non-gateway peer?): {err}", peer.to_base58()),
            }
        }
    }
}
