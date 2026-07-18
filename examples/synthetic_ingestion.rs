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
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::Result;
use tokio::sync::{broadcast, mpsc};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use koalisi::algorithms::{AdditiveCalculator, CapabilityAgent, FeedbackStore};
use koalisi::decision::{DecisionContext, ThresholdPolicy};
use koalisi::ingest::{
    MultiResolutionSource, NumericSample, Pacing, SampleUpdate, SensorEvent, SensorEventSource,
    SensorSpec, SeriesSpec, spawn_sample_monitor, spawn_source_pump,
};
use koalisi::subsystems::coalition_actor::CoalitionService;
use koalisi::subsystems::outcome::{OutcomeSink, TaskOutcome, emit_outcome, spawn_outcome_forwarder};
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

    // One agent per ingested sensor. Topology `VertexIndex` and domain
    // `agent_id` are DISTINCT id spaces (they coincide here only because the
    // manager is fresh and agents are added in id order) — keep the mapping so
    // downstream consumers keyed by agent_id are wired honestly.
    let mut sensor_vertices = Vec::new();
    let mut vertex_to_agent = HashMap::new();
    for (id, _) in sensors.iter().enumerate() {
        let agent = CapabilityAgent::new(id, 1u32 << id, 50);
        let vertex = manager.add_agent(agent).await?;
        sensor_vertices.push(vertex);
        vertex_to_agent.insert(vertex, id);
    }
    // One fresh candidate covering a new capability bit. It must be added before
    // the manager moves into the service (the service seam mutates by index).
    let candidate_agent = CapabilityAgent::new(sensors.len(), 1u32 << sensors.len(), 50);
    let candidate = manager.add_agent(candidate_agent).await?;
    vertex_to_agent.insert(candidate, sensors.len());

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
    // 4. Task outcomes (issue #55): arm-agnostic completion events.
    //
    // The domain (this example) emits one `TaskOutcome` per completed task over
    // the coalition's final members. A forwarder fans each record out to two
    // learned consumers: a `FeedbackStore` (#41, scalarized) and a counting
    // closure sink. The success/failure pattern is deterministic — no
    // randomness — and the seam never gates a decision (Part 4g caveat).
    // =====================================================================
    // Resolve topology vertices to DOMAIN agent ids — `TaskOutcome.members` and
    // `FeedbackStore` are keyed by `agent_id`, not by `VertexIndex` (the raw
    // index only coincides with the id in a fresh, in-order, no-removal graph).
    let member_ids: Vec<usize> = after_members
        .iter()
        .map(|v| {
            vertex_to_agent
                .get(v)
                .copied()
                .expect("invariant: every coalition member vertex was added by this example")
        })
        .collect();

    let store = FeedbackStore::new(0.5);
    let outcome_count = Arc::new(AtomicUsize::new(0));

    let store_sink: Box<dyn OutcomeSink> = Box::new(store.clone());
    let counter = Arc::clone(&outcome_count);
    let closure_sink: Box<dyn OutcomeSink> = Box::new(move |_o: &TaskOutcome| {
        counter.fetch_add(1, Ordering::SeqCst);
    });

    let (outcome_tx, outcome_rx) = mpsc::channel::<TaskOutcome>(64);
    // Lossless shutdown discipline ("pick one; don't mix" — module docs): the
    // forwarder gets a dedicated, never-cancelled token, so ONLY the
    // drop-sender → drain path can end it; every buffered outcome reaches the
    // sinks. It is still on `tracker`, so the final drain covers it too.
    let outcome_shutdown = CancellationToken::new();
    let forwarder = spawn_outcome_forwarder(
        outcome_rx,
        vec![store_sink, closure_sink],
        &tracker,
        outcome_shutdown,
    );

    // Five deterministic outcomes over the coalition members: succeed unless
    // `t % 3 == 2` (⇒ four successes, one failure).
    for t in 0..5 {
        emit_outcome(
            Some(&outcome_tx),
            TaskOutcome {
                required,
                members: member_ids.clone(),
                success: t % 3 != 2,
            },
        );
    }

    // Drop the sender ⇒ the forwarder drains every buffered outcome, then exits.
    // The await is the drain-before-print barrier; the task is ALSO on
    // `tracker` (inherent to spawn_outcome_forwarder), so the final
    // tracker.wait() covers it uniformly — by then it has already finished.
    drop(outcome_tx);
    forwarder.await?;

    println!(
        "\ntask outcomes: {} fanned out over members {member_ids:?}",
        outcome_count.load(Ordering::SeqCst)
    );
    for &id in member_ids.iter().take(2) {
        println!(
            "  agent {id}: history={} failures={}",
            store.history(id),
            store.failures(id)
        );
    }

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
