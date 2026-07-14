//! Integration test for the domain-neutral ingestion layer (issue #8, K5).
//!
//! Proves two things end-to-end with **no financial data**:
//!
//! 1. Both synthetic sources (NEST-shaped `MultiResolutionSource` and
//!    tauhokohoko-shaped `SensorEventSource`) pump through generic
//!    `SampleMonitor`s on a `broadcast` bus, with updates arriving in timestamp
//!    order per key and snapshots holding the ring-buffer window.
//! 2. The coalition/topology layer runs on synthetic non-financial data:
//!    capability masks derived from sensor ids drive `CoalitionManager`
//!    coalition formation and a `ThresholdPolicy` join decision through the
//!    `CoalitionDecisionPolicy` trait (the #8 acceptance line).

use std::collections::HashMap;
use std::time::Duration;

use tokio::sync::broadcast;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use koalisi::algorithms::{AdditiveCalculator, AgentCapabilities};
use koalisi::decision::{CoalitionDecisionPolicy, DecisionContext, ThresholdPolicy};
use koalisi::ingest::{
    MultiResolutionSource, NumericSample, Pacing, SampleUpdate, SensorEvent, SensorEventSource,
    SensorSpec, SeriesSpec, spawn_sample_monitor, spawn_source_pump,
};
use koalisi::topology::CoalitionManager;

// ---------------------------------------------------------------------------
// 1. Pump synthetic sources through generic monitors
// ---------------------------------------------------------------------------

#[tokio::test]
async fn multi_resolution_pump_orders_per_key_and_windows_snapshots() {
    timeout(Duration::from_secs(10), async {
        let tracker = TaskTracker::new();
        let root = CancellationToken::new();
        let (bus, mut listener) = broadcast::channel::<SampleUpdate<NumericSample>>(1024);

        // Two resolutions: hourly load vs coarse 5-hourly planning periods —
        // the NEST resolution gap.
        let specs = vec![
            SeriesSpec {
                key: "hourly".into(),
                start_ms: 0,
                step_ms: 3_600_000,
                count: 12,
                base: 100.0,
                amplitude: 10.0,
                period: 24,
            },
            SeriesSpec {
                key: "planning".into(),
                start_ms: 0,
                step_ms: 18_000_000,
                count: 4,
                base: 50.0,
                amplitude: 5.0,
                period: 4,
            },
        ];
        let source = MultiResolutionSource::new(&specs, 2026);

        // One monitor per series key, all publishing to the shared bus.
        let mut router = HashMap::new();
        for key in ["hourly", "planning"] {
            let h = spawn_sample_monitor::<NumericSample>(
                &tracker,
                root.child_token(),
                key.into(),
                8, // small window so we can prove eviction
                bus.clone(),
            );
            router.insert(key.to_string(), h);
        }

        // Collect published updates in-order per key from a bus subscriber.
        let collector = tokio::spawn(async move {
            let mut last_ts: HashMap<String, i64> = HashMap::new();
            let mut counts: HashMap<String, usize> = HashMap::new();
            while let Ok(update) = listener.recv().await {
                if let Some(prev) = last_ts.get(&update.key) {
                    assert!(
                        update.view.timestamp_ms >= *prev,
                        "per-key updates must be time-ordered ({} then {})",
                        prev,
                        update.view.timestamp_ms
                    );
                }
                last_ts.insert(update.key.clone(), update.view.timestamp_ms);
                *counts.entry(update.key).or_default() += 1;
            }
            counts
        });

        let stats = spawn_source_pump(
            &tracker,
            root.child_token(),
            source,
            router.clone(),
            Pacing::Asap,
        )
        .await
        .expect("pump task joined")
        .expect("pump ok");
        assert_eq!(stats.fed, 16, "12 + 4 samples routed to their monitors");
        assert_eq!(stats.dropped, 0, "every key had a monitor");

        // Flush + snapshot each monitor: the window holds the last `capacity`.
        let hourly = router["hourly"].snapshot().await.unwrap();
        assert_eq!(hourly.key, "hourly");
        assert_eq!(
            hourly.history.len(),
            8,
            "12 hourly samples evict to the 8-cap window"
        );
        assert!(hourly.latest.is_some());
        // Window holds the most-recent 8 (indices 4..=11).
        let stamps: Vec<i64> = hourly.history.iter().map(|s| s.timestamp_ms).collect();
        assert_eq!(stamps.first().copied(), Some(4 * 3_600_000));
        assert_eq!(stamps.last().copied(), Some(11 * 3_600_000));

        let planning = router["planning"].snapshot().await.unwrap();
        assert_eq!(
            planning.history.len(),
            4,
            "4 planning samples fit the window"
        );

        // Drop senders → collector's `recv` closes → task returns its tallies.
        drop(bus);
        drop(router);
        let counts = collector.await.unwrap();
        assert_eq!(counts.get("hourly").copied(), Some(12));
        assert_eq!(counts.get("planning").copied(), Some(4));

        root.cancel();
        tracker.close();
        tracker.wait().await;
    })
    .await
    .expect("multi-resolution pump test timed out");
}

#[tokio::test]
async fn sensor_pump_windows_and_changepoint_visible() {
    timeout(Duration::from_secs(10), async {
        let tracker = TaskTracker::new();
        let root = CancellationToken::new();
        let (bus, _rx) = broadcast::channel::<SampleUpdate<SensorEvent>>(1024);

        let specs = vec![
            SensorSpec {
                sensor: "salinity".into(),
                baseline_mean: 10.0,
                noise_sd: 0.4,
                shift_at: 100,
                shift: 4.0,
            },
            SensorSpec {
                sensor: "turbidity".into(),
                baseline_mean: 2.0,
                noise_sd: 0.2,
                shift_at: usize::MAX, // no changepoint
                shift: 0.0,
            },
        ];
        let source = SensorEventSource::new(&specs, 200, 0, 1_000, 7);

        let mut router = HashMap::new();
        for key in ["salinity", "turbidity"] {
            let h = spawn_sample_monitor::<SensorEvent>(
                &tracker,
                root.child_token(),
                key.into(),
                256, // window large enough to keep every reading
                bus.clone(),
            );
            router.insert(key.to_string(), h);
        }

        let stats = spawn_source_pump(
            &tracker,
            root.child_token(),
            source,
            router.clone(),
            Pacing::Asap,
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(stats.fed, 400);
        assert_eq!(stats.dropped, 0);

        // The salinity window holds all 200 readings; the changepoint is visible
        // as a before/after mean difference ≈ shift.
        let sal = router["salinity"].snapshot().await.unwrap();
        assert_eq!(sal.history.len(), 200);
        let before: f64 = sal.history[..100].iter().map(|e| e.reading).sum::<f64>() / 100.0;
        let after: f64 = sal.history[100..].iter().map(|e| e.reading).sum::<f64>() / 100.0;
        assert!(
            (after - before - 4.0).abs() < 0.3,
            "changepoint mean shift ≈ 4.0, observed {}",
            after - before
        );

        drop(bus);
        drop(router);
        root.cancel();
        tracker.close();
        tracker.wait().await;
    })
    .await
    .expect("sensor pump test timed out");
}

// ---------------------------------------------------------------------------
// 2. Coalition layer runs on synthetic non-financial (sensor) data
// ---------------------------------------------------------------------------

/// A test-local agent standing in for a sensor stream. Its capability mask is
/// derived from the sensor id, so coalition value comes from *coverage
/// diversity* — no financial types involved. Derives `Copy + Eq + Debug` so it
/// satisfies the topology `VertexTrait` blanket bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SensorAgent {
    id: usize,
    caps: u32,
    trust: u32,
}

impl AgentCapabilities for SensorAgent {
    fn agent_id(&self) -> usize {
        self.id
    }
    fn capabilities(&self) -> u32 {
        self.caps
    }
    fn trust_level(&self) -> u32 {
        self.trust
    }
}

/// A minimal coalition label (satisfies the topology `HyperedgeTrait` bound).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SensorCoalition(u32);

#[tokio::test]
async fn coalition_layer_runs_on_synthetic_sensor_agents() {
    timeout(Duration::from_secs(10), async {
        // Derive capability masks from sensor ids: each sensor covers a distinct
        // capability bit, so a coalition of them is genuinely diverse.
        let agents: Vec<SensorAgent> = (0..3)
            .map(|id| SensorAgent {
                id,
                caps: 1u32 << id,
                trust: 50,
            })
            .collect();

        // ---- topology: form a coalition of the sensor agents ----
        let manager: CoalitionManager<SensorAgent, SensorCoalition> = CoalitionManager::empty();
        let mut vertices = Vec::new();
        for a in &agents {
            vertices.push(manager.add_agent(*a).await.expect("add agent"));
        }
        let coalition = manager
            .form_coalition(vertices.clone(), SensorCoalition(1))
            .await
            .expect("form coalition");
        // The coalition holds all three agents.
        let members = manager
            .coalition_members(coalition)
            .await
            .expect("read members");
        assert_eq!(members.len(), 3, "coalition formed over 3 synthetic agents");

        // ---- decision: a new sensor agent covering a fresh capability joins ----
        let policy: Box<dyn CoalitionDecisionPolicy> =
            Box::new(ThresholdPolicy::new(AdditiveCalculator, 0.0, 0.0));
        let ctx = DecisionContext {
            required_capabilities: 0b1111,
        };
        let candidate = SensorAgent {
            id: 3,
            caps: 1u32 << 3,
            trust: 50,
        };
        let current: Vec<&dyn AgentCapabilities> =
            agents.iter().map(|a| a as &dyn AgentCapabilities).collect();
        let decision = policy.should_join_async(&candidate, &current, &ctx).await;
        assert!(
            decision.act,
            "a sensor adding a new capability should join (marginal value {} > 0)",
            decision.score
        );
        assert!(decision.score > 0.0);
    })
    .await
    .expect("coalition-on-synthetic-data test timed out");
}
