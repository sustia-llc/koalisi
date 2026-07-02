//! Same bundled DBN file as `databento_historical`, but the adapter is
//! spawned on the swarm's `TaskTracker` with `Pacing::Realtime`, simulating
//! a live feed.
//!
//! What this example demonstrates beyond the historical one:
//! 1. `spawn_dbn_pump` — fire-and-forget spawn on `swarm.task_tracker()`
//!    with a child of `swarm.cancellation_token()`. The pump joins the
//!    swarm's library-first lifecycle: `Swarm::shutdown()` cancels and
//!    drains it.
//! 2. `Pacing::Realtime { speed_factor }` paces records by their
//!    `ts_recv` deltas. The bundled file's two records are ~nanoseconds
//!    apart so we crank the factor up to keep total runtime sub-second.
//! 3. A custom alert listener task subscribed to `swarm.alert_bus()` so the
//!    "live" semantics surface even without writing to the AlertSink.
//!
//! Run with:
//! ```ignore
//! cargo run --features databento --example databento_live_replay
//! ```

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};

use koalisi::market::{ArbitrageOpportunity, Pair, Triangle};
use koalisi::subsystems::databento::{Pacing, mapper_from_fn, spawn_dbn_pump};
use koalisi::subsystems::swarm::{Swarm, SwarmConfig};

fn locate_test_data() -> Result<PathBuf> {
    let candidates = [
        "../trade/databento-rs/tests/data/test_data.mbp-1.dbn.zst",
        "../../trade/databento-rs/tests/data/test_data.mbp-1.dbn.zst",
        "../databento-rs/tests/data/test_data.mbp-1.dbn.zst",
    ];
    for c in candidates {
        let p = PathBuf::from(c);
        if p.exists() {
            return Ok(p);
        }
    }
    anyhow::bail!("could not locate test_data.mbp-1.dbn.zst");
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_target(false).try_init().ok();

    let path: PathBuf = std::env::var("DBN_TEST_PATH")
        .map(PathBuf::from)
        .or_else(|_| locate_test_data())
        .context("locating DBN test data")?;
    println!("loading DBN file: {}", path.display());

    let triangle = Triangle::new("EUR/USD".parse()?, "GBP/USD".parse()?, "EUR/GBP".parse()?)?;
    let swarm = Swarm::new(SwarmConfig {
        triangles: vec![triangle],
        threshold_bps: 5.0,
        history_capacity: 256,
    })
    .await?;

    // Subscribe a custom listener task (printf-only) to the alert broadcast so
    // we see opportunities the moment they're published, in addition to the
    // swarm's built-in `AlertSink`.
    let mut alert_rx = swarm.alert_bus().subscribe();
    let listener_token = swarm.cancellation_token().child_token();
    swarm.task_tracker().spawn(async move {
        loop {
            tokio::select! {
                biased;
                _ = listener_token.cancelled() => break,
                r = alert_rx.recv() => match r {
                    Ok(opp) => {
                        let opp: ArbitrageOpportunity = opp;
                        println!("[live listener] {opp}");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    });

    // Mapper: round-robin between the three pairs so all three monitors
    // get a tick from the bundled 2-record file (which won't be enough
    // for an arb signal, but exercises the entire fan-out path).
    let pairs: Vec<Pair> = vec!["EUR/USD".parse()?, "GBP/USD".parse()?, "EUR/GBP".parse()?];
    let pairs_clone = pairs.clone();
    let counter = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mapper = mapper_from_fn(move |instrument_id, symbol| {
        let n = counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let pair = pairs_clone[n % pairs_clone.len()].clone();
        println!(
            "  record #{n} → instrument_id={instrument_id} symbol={symbol:?} mapped to {pair}"
        );
        Some(pair)
    });

    // Spawn the pump on the swarm's TaskTracker — Swarm::shutdown() will
    // cancel + drain it.
    let pump_handle = spawn_dbn_pump(
        &swarm,
        &path,
        mapper,
        // Realtime pacing with a high speed factor so the demo finishes
        // fast even if the file were spread across hours.
        Pacing::Realtime {
            speed_factor: 1_000_000.0,
        },
    );

    // Wait briefly for the pump to finish on its own (the test file has
    // only 2 records).
    let stats = tokio::time::timeout(Duration::from_secs(2), pump_handle)
        .await
        .context("pump did not finish within 2s")???;
    println!("pump complete: {stats:?}");

    let alerts = swarm.alerts().await?;
    println!("alerts emitted: {}", alerts.len());

    swarm.shutdown().await;
    Ok(())
}
