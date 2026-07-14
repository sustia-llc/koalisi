//! Domain-neutral ingestion demo (issue #8, K5) — no market data, no
//! credentials, default features only.
//!
//! Builds the two synthetic sources that mirror koalisi's real downstream
//! drivers, pumps them through generic `SampleMonitor`s on a `broadcast` bus,
//! and prints a short summary:
//!
//! - **NEST-shaped** `MultiResolutionSource`: two numeric series at very
//!   different resolutions (hourly vs 6-hourly) merged into one time-ordered
//!   stream — showing the resolution gap.
//! - **tauhokohoko-shaped** `SensorEventSource`: two ecological sensors at a
//!   fixed cadence, one with a mid-stream mean changepoint that shows up as a
//!   before/after mean difference.
//!
//! Runs quickly and exits 0 (bounded counts, `Pacing::Asap`).

use std::collections::HashMap;

use anyhow::Result;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use koalisi::ingest::{
    MultiResolutionSource, NumericSample, Pacing, SampleUpdate, SensorEvent, SensorEventSource,
    SensorSpec, SeriesSpec, spawn_sample_monitor, spawn_source_pump,
};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_target(false).try_init().ok();

    let tracker = TaskTracker::new();
    let root = CancellationToken::new();

    // =====================================================================
    // 1. NEST-shaped multi-resolution numeric series.
    // =====================================================================
    let numeric_specs = vec![
        SeriesSpec {
            key: "urbanopt_load".into(), // hourly
            start_ms: 0,
            step_ms: 3_600_000,
            count: 12,
            base: 100.0,
            amplitude: 15.0,
            period: 24,
        },
        SeriesSpec {
            key: "messageix_plan".into(), // 6-hourly — the resolution gap
            start_ms: 0,
            step_ms: 21_600_000,
            count: 3,
            base: 500.0,
            amplitude: 40.0,
            period: 4,
        },
    ];
    let numeric_source = MultiResolutionSource::new(&numeric_specs, 2026);
    println!(
        "NEST-shaped source: {} merged samples across 2 resolutions",
        numeric_source.remaining()
    );

    let (num_bus, _num_rx) = broadcast::channel::<SampleUpdate<NumericSample>>(1024);
    let mut num_router = HashMap::new();
    for key in ["urbanopt_load", "messageix_plan"] {
        num_router.insert(
            key.to_string(),
            spawn_sample_monitor::<NumericSample>(
                &tracker,
                root.child_token(),
                key.into(),
                8,
                num_bus.clone(),
            ),
        );
    }

    let num_stats = spawn_source_pump(
        &tracker,
        root.child_token(),
        numeric_source,
        num_router.clone(),
        Pacing::Asap,
    )
    .await??;
    println!(
        "  pumped: fed={} dropped={}",
        num_stats.fed, num_stats.dropped
    );
    for key in ["urbanopt_load", "messageix_plan"] {
        let snap = num_router[key].snapshot().await?;
        let latest = snap.latest.expect("series produced samples");
        println!(
            "  {key:<15} window={:>2}  latest value={:8.3} @ t={}ms",
            snap.history.len(),
            latest.value,
            latest.timestamp_ms
        );
    }

    // =====================================================================
    // 2. tauhokohoko-shaped sensor streams with a changepoint.
    // =====================================================================
    let sensor_specs = vec![
        SensorSpec {
            sensor: "salinity".into(),
            baseline_mean: 10.0,
            noise_sd: 0.4,
            shift_at: 60, // mid-stream ecological shift
            shift: 3.0,
        },
        SensorSpec {
            sensor: "turbidity".into(),
            baseline_mean: 2.0,
            noise_sd: 0.2,
            shift_at: usize::MAX, // stable
            shift: 0.0,
        },
    ];
    let sensor_count = 120usize;
    let sensor_source = SensorEventSource::new(&sensor_specs, sensor_count, 0, 1_000, 7);
    println!(
        "\ntauhokohoko-shaped source: {} sensor readings across 2 sensors",
        sensor_source.remaining()
    );

    let (sensor_bus, _sensor_rx) = broadcast::channel::<SampleUpdate<SensorEvent>>(1024);
    let mut sensor_router = HashMap::new();
    for key in ["salinity", "turbidity"] {
        sensor_router.insert(
            key.to_string(),
            spawn_sample_monitor::<SensorEvent>(
                &tracker,
                root.child_token(),
                key.into(),
                sensor_count + 1,
                sensor_bus.clone(),
            ),
        );
    }

    let sensor_stats = spawn_source_pump(
        &tracker,
        root.child_token(),
        sensor_source,
        sensor_router.clone(),
        Pacing::Asap,
    )
    .await??;
    println!(
        "  pumped: fed={} dropped={}",
        sensor_stats.fed, sensor_stats.dropped
    );

    let sal = sensor_router["salinity"].snapshot().await?;
    let split = 60usize;
    let before: f64 = sal.history[..split].iter().map(|e| e.reading).sum::<f64>() / split as f64;
    let after: f64 = sal.history[split..].iter().map(|e| e.reading).sum::<f64>()
        / (sal.history.len() - split) as f64;
    println!(
        "  salinity changepoint: mean before={before:.3}, after={after:.3} (shift ≈ {:.3})",
        after - before
    );

    // =====================================================================
    // Shutdown.
    // =====================================================================
    drop(num_bus);
    drop(sensor_bus);
    root.cancel();
    tracker.close();
    tracker.wait().await;
    println!("\ndone.");
    Ok(())
}
