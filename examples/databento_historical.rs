//! Decode Databento's bundled MBP-1 test DBN file and pump it through the
//! swarm at full speed (the "historical bootstrap" path).
//!
//! What this example demonstrates:
//! 1. `dbn::AsyncDbnDecoder::from_zstd_file` reading a real `.dbn.zst`.
//! 2. The `Mbp1Msg → Tick` translation in the adapter.
//! 3. `koalisi::subsystems::databento::pump_dbn_file`
//!    routing records into the swarm via the FIFO-deterministic
//!    `SwarmFeeder::feed_tick`.
//! 4. The mapper from DBN's `instrument_id` to our `Pair` type.
//!
//! Notes on the bundled data
//! -------------------------
//! `databento-rs/tests/data/test_data.mbp-1.dbn.zst` is a 2-record decoder
//! fixture — not a real session. We pretend its instrument(s) are
//! `EUR/USD` for the demo so the wiring is exercised end-to-end. A real
//! deployment would supply a mapper that knows the actual symbology of
//! the data file in hand.
//!
//! Run with:
//! ```ignore
//! cargo run --features databento --example databento_historical
//! ```

use std::path::PathBuf;

use anyhow::{Context, Result};
use kameo_actors::DeliveryStrategy;
use tokio_util::sync::CancellationToken;

use koalisi::market::{Pair, Triangle};
use koalisi::subsystems::databento::{Pacing, mapper_from_fn, pump_dbn_file};
use koalisi::subsystems::swarm::{Swarm, SwarmConfig};

fn locate_test_data() -> Result<PathBuf> {
    // Walk a few common locations: relative to the binary, relative to
    // the workspace root, and relative to a sibling `databento-rs` repo.
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
    anyhow::bail!(
        "could not locate test_data.mbp-1.dbn.zst — checked: {candidates:?}\n\
         set DBN_TEST_PATH or place the file next to the example."
    );
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
        delivery_strategy: DeliveryStrategy::Guaranteed,
    })
    .await?;

    // Demo mapper: route every record to EUR/USD. A production mapper
    // would pattern-match on the symbol string returned by the file's
    // symbol map.
    let eur_usd: Pair = "EUR/USD".parse()?;
    let mapper = mapper_from_fn(move |instrument_id, symbol| {
        println!("  record → instrument_id={instrument_id}, symbol={symbol:?} (mapped to EUR/USD)");
        Some(eur_usd.clone())
    });

    let stats = pump_dbn_file(
        swarm.feeder(),
        &path,
        mapper,
        Pacing::Asap,
        CancellationToken::new(),
    )
    .await?;
    println!("pump complete: {stats:?}");

    // Inspect the EUR/USD monitor to confirm ticks landed.
    let snap = swarm
        .monitor(&"EUR/USD".parse()?)
        .unwrap()
        .ask(koalisi::subsystems::monitor::GetSnapshot)
        .await?;
    println!(
        "EUR/USD monitor: {} ticks in history (latest mid = {:.6})",
        snap.history.len(),
        snap.latest.map(|q| q.mid).unwrap_or(f64::NAN),
    );

    // Because we only fed one leg, no triangular opportunities will have
    // fired. Verify and shut down.
    let alerts = swarm.alerts().await?;
    println!("alerts emitted: {}", alerts.len());

    swarm.shutdown().await;
    Ok(())
}
