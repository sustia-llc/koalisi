//! Domain-neutral ingestion demo (issue #8, K5) — no market data, no
//! credentials, default features only.
//!
//! Builds the two synthetic sources that mirror koalisi's real downstream
//! drivers, pumps them through generic `SampleMonitor`s on a `broadcast` bus,
//! then forms a coalition over the ingested data — all domain-neutral:
//!
//! - **NEST-shaped** `MultiResolutionSource`: two numeric series at very
//!   different resolutions (hourly vs 6-hourly) merged into one time-ordered
//!   stream — showing the resolution gap.
//! - **tauhokohoko-shaped** `SensorEventSource`: two ecological sensors at a
//!   fixed cadence, one with a mid-stream mean changepoint that shows up as a
//!   before/after mean difference.
//! - **coalition formation** (the flagship demo): each ingested sensor becomes
//!   an agent whose capability mask is a distinct bit, a `CoalitionManager`
//!   forms a coalition over them, and a fresh candidate joins through the
//!   policy-gated `CoalitionService` seam.
//!
//! Runs quickly and exits 0 (bounded counts, `Pacing::Asap`).

use std::collections::HashMap;

use anyhow::Result;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use koalisi::algorithms::{AdditiveCalculator, CapabilityAgent};
use koalisi::decision::{DecisionContext, ThresholdPolicy};
use koalisi::ingest::{
    MultiResolutionSource, NumericSample, Pacing, SampleUpdate, SensorEvent, SensorEventSource,
    SensorSpec, SeriesSpec, spawn_sample_monitor, spawn_source_pump,
};
use koalisi::subsystems::coalition_actor::CoalitionService;
use koalisi::topology::CoalitionManager;

/// A minimal coalition label (satisfies the topology `HyperedgeTrait` bound).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SensorCoalition(u32);

// Demo `main`: three sequential sections read top-to-bottom; the mean-window
// casts are on bounded fixture counts and lose no meaningful precision.
#[allow(clippy::too_many_lines, clippy::cast_precision_loss)]
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
    // 3. Coalition formation on the synthetic data (the flagship demo).
    //
    // Each ingested sensor becomes an agent covering one distinct capability
    // bit, so a coalition's value is coverage diversity — no domain types
    // involved. We form a coalition over the sensor agents, then offer a fresh
    // candidate (covering a not-yet-covered bit) through the policy-gated
    // `CoalitionService` seam; the additive marginal value clears the threshold
    // and the candidate joins.
    // =====================================================================
    let sensors = ["salinity", "turbidity"];

    let manager: CoalitionManager<CapabilityAgent, SensorCoalition> = CoalitionManager::empty();

    // One agent per ingested sensor.
    let mut sensor_vertices = Vec::new();
    for (id, _) in sensors.iter().enumerate() {
        let agent = CapabilityAgent::new(id, 1u32 << id, 50);
        sensor_vertices.push(manager.add_agent(agent).await?);
    }
    // One fresh candidate covering a new capability bit. It must be added before
    // the manager moves into the service (the service seam mutates by index).
    let candidate_agent = CapabilityAgent::new(sensors.len(), 1u32 << sensors.len(), 50);
    let candidate = manager.add_agent(candidate_agent).await?;

    let coalition = manager
        .form_coalition(sensor_vertices.clone(), SensorCoalition(1))
        .await?;

    // Require every sensor bit plus the candidate's bit.
    let required = (1u32 << (sensors.len() + 1)) - 1;
    let service = CoalitionService::spawn(
        manager,
        Box::new(ThresholdPolicy::new(AdditiveCalculator, 0.0, 0.0)),
        DecisionContext {
            required_capabilities: required,
        },
    );

    let before_members = service.members(coalition).await?;
    println!(
        "\ncoalition formed over {} synthetic sensor agents {sensors:?}",
        before_members.len()
    );

    let decision = service.join(candidate, coalition).await?;
    println!(
        "  policy-gated candidate join: act={} score={:.3}",
        decision.act, decision.score
    );

    let after_members = service.members(coalition).await?;
    println!(
        "  coalition size: {} → {}",
        before_members.len(),
        after_members.len()
    );

    // Release the service handle so its task exits on the closed command channel.
    drop(service);

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
