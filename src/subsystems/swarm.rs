//! `Swarm` — top-level orchestrator that ties the forex actors together.
//!
//! Wraps a [`CoalitionRuntime`] (TaskTracker + CancellationToken + three-step
//! shutdown) with forex-specific actors: monitors per pair, coordinator, sink,
//! and two pubsub buses.

use std::collections::{HashMap, HashSet};

use anyhow::{Result, anyhow};
use kameo::prelude::*;
use kameo_actors::{
    DeliveryStrategy,
    pubsub::{PubSub, Subscribe},
};
use tokio_util::sync::CancellationToken;

use crate::core::config::SETTINGS;
use crate::core::runtime::CoalitionRuntime;
use crate::market::{ArbitrageOpportunity, Pair, Tick, TickUpdate, Triangle};
use crate::subsystems::coordinator::{self, ArbitrageCoordinator, ArbitrageCoordinatorArgs};
use crate::subsystems::monitor::{self, MarketMonitor, MarketMonitorArgs};
use crate::subsystems::sink::{self, AlertSink};

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Builder-style config passed to [`Swarm::new`].
#[derive(Debug, Clone)]
pub struct SwarmConfig {
    pub triangles: Vec<Triangle>,
    pub threshold_bps: f64,
    pub history_capacity: usize,
    pub delivery_strategy: DeliveryStrategy,
}

impl SwarmConfig {
    /// Build a config from a list of triangles, pulling threshold / history
    /// capacity / delivery strategy from `config/*.toml`.
    pub fn from_settings(triangles: Vec<Triangle>) -> Self {
        let s = &SETTINGS.coalition;
        Self {
            triangles,
            threshold_bps: s.threshold_bps,
            history_capacity: s.history_capacity,
            delivery_strategy: parse_delivery_strategy(&s.delivery_strategy),
        }
    }
}

fn parse_delivery_strategy(s: &str) -> DeliveryStrategy {
    match s.trim().to_ascii_lowercase().as_str() {
        "guaranteed" => DeliveryStrategy::Guaranteed,
        "best_effort" | "best-effort" | "besteffort" => DeliveryStrategy::BestEffort,
        other => {
            tracing::warn!(
                value = other,
                "unknown delivery strategy in config; defaulting to guaranteed"
            );
            DeliveryStrategy::Guaranteed
        }
    }
}

// ---------------------------------------------------------------------------
// SwarmFeeder
// ---------------------------------------------------------------------------

/// A clone-able handle that knows how to route a tick to the right
/// `MarketMonitor`. Cheap to clone (`ActorRef`s are mpsc-sender + Arc).
///
/// Use this from background tasks that need to push ticks into the swarm
/// without holding a borrow on `Swarm`.
#[derive(Clone)]
pub struct SwarmFeeder {
    monitors: HashMap<Pair, ActorRef<MarketMonitor>>,
}

impl SwarmFeeder {
    /// Route a single tick. Same FIFO-deterministic semantics as
    /// `Swarm::feed_tick`.
    pub async fn feed_tick(&self, tick: Tick) -> Result<()> {
        let monitor = self
            .monitors
            .get(&tick.pair)
            .ok_or_else(|| anyhow!("no monitor for pair {}", tick.pair))?;
        monitor.ask(tick).await?;
        Ok(())
    }

    /// Iterate over the pairs this feeder can route to.
    pub fn pairs(&self) -> impl Iterator<Item = &Pair> {
        self.monitors.keys()
    }
}

// ---------------------------------------------------------------------------
// Swarm
// ---------------------------------------------------------------------------

pub struct Swarm {
    monitors: HashMap<Pair, ActorRef<MarketMonitor>>,
    coordinator: ActorRef<ArbitrageCoordinator>,
    sink: ActorRef<AlertSink>,
    tick_bus: ActorRef<PubSub<TickUpdate>>,
    alert_bus: ActorRef<PubSub<ArbitrageOpportunity>>,
    runtime: CoalitionRuntime,
}

impl Swarm {
    /// Build the swarm: spawn the two pubsubs, then the alert sink (and
    /// subscribe it), then the coordinator (and subscribe it to the tick
    /// pubsub), then one monitor per unique pair across all triangles.
    pub async fn new(config: SwarmConfig) -> Result<Self> {
        if config.triangles.is_empty() {
            return Err(anyhow!("SwarmConfig.triangles must be non-empty"));
        }

        let runtime = CoalitionRuntime::new();

        // ---- pubsubs ----
        let tick_bus = PubSub::spawn(PubSub::<TickUpdate>::new(config.delivery_strategy));
        let alert_bus = PubSub::spawn(PubSub::<ArbitrageOpportunity>::new(
            config.delivery_strategy,
        ));

        // ---- alert sink ----
        let sink = AlertSink::spawn(AlertSink::default());
        alert_bus.ask(Subscribe(sink.clone())).await?;

        // ---- coordinator ----
        let coordinator = ArbitrageCoordinator::spawn(ArbitrageCoordinatorArgs {
            triangles: config.triangles.clone(),
            threshold_bps: config.threshold_bps,
            alert_bus: alert_bus.clone(),
        });
        tick_bus.ask(Subscribe(coordinator.clone())).await?;

        // ---- monitors: one per unique pair ----
        let mut unique_pairs: HashSet<Pair> = HashSet::new();
        for triangle in &config.triangles {
            for p in triangle.pairs() {
                unique_pairs.insert(p.clone());
            }
        }

        let mut monitors = HashMap::with_capacity(unique_pairs.len());
        for pair in unique_pairs {
            let monitor_ref = MarketMonitor::spawn(MarketMonitorArgs {
                pair: pair.clone(),
                history_capacity: config.history_capacity,
                tick_bus: tick_bus.clone(),
            });
            monitors.insert(pair, monitor_ref);
        }

        tracing::info!(
            triangles = config.triangles.len(),
            monitors = monitors.len(),
            threshold_bps = config.threshold_bps,
            "Swarm assembled"
        );

        Ok(Self {
            monitors,
            coordinator,
            sink,
            tick_bus,
            alert_bus,
            runtime,
        })
    }

    // -----------------------------------------------------------------------
    // Accessors
    // -----------------------------------------------------------------------

    pub fn monitor(&self, pair: &Pair) -> Option<&ActorRef<MarketMonitor>> {
        self.monitors.get(pair)
    }

    pub fn coordinator(&self) -> &ActorRef<ArbitrageCoordinator> {
        &self.coordinator
    }

    pub fn sink(&self) -> &ActorRef<AlertSink> {
        &self.sink
    }

    /// The tick pubsub. Exposed so callers can subscribe additional
    /// consumers (e.g., a metrics exporter actor).
    pub fn tick_bus(&self) -> &ActorRef<PubSub<TickUpdate>> {
        &self.tick_bus
    }

    /// The alert pubsub.
    pub fn alert_bus(&self) -> &ActorRef<PubSub<ArbitrageOpportunity>> {
        &self.alert_bus
    }

    pub fn cancellation_token(&self) -> &CancellationToken {
        self.runtime.cancellation_token()
    }

    pub fn task_tracker(&self) -> &tokio_util::task::TaskTracker {
        self.runtime.task_tracker()
    }

    /// A clone-able, owned feed handle that captures the monitor refs and
    /// nothing else. Pass it into background tasks (e.g., the databento
    /// adapter's pump) so they can call `feed_tick` without holding a
    /// borrow on the `Swarm` itself.
    pub fn feeder(&self) -> SwarmFeeder {
        SwarmFeeder {
            monitors: self.monitors.clone(),
        }
    }

    // -----------------------------------------------------------------------
    // Feeds
    // -----------------------------------------------------------------------

    /// Route a single tick to the right monitor. Uses `ask`, so the call
    /// returns only after the monitor has accepted the tick *and* published
    /// the resulting `TickUpdate` to the tick pubsub (which, with
    /// `Guaranteed` delivery, blocks until the coordinator's mailbox has
    /// accepted it). This is what makes `feed_tick(...)` a deterministic
    /// flushing primitive in tests.
    pub async fn feed_tick(&self, tick: Tick) -> Result<()> {
        let monitor = self
            .monitors
            .get(&tick.pair)
            .ok_or_else(|| anyhow!("no monitor for pair {}", tick.pair))?;
        monitor.ask(tick).await?;
        Ok(())
    }

    /// Bootstrap historical data: feed every tick in `ticks` into the right
    /// monitor sequentially. Returns once all ticks have been ingested.
    pub async fn replay_history(&self, ticks: Vec<Tick>) -> Result<()> {
        for tick in ticks {
            self.feed_tick(tick).await?;
        }
        // Coordinator could be still processing the last few TickUpdates;
        // a Ping flush ensures it's drained before we hand back.
        self.coordinator.ask(coordinator::Ping).await?;
        Ok(())
    }

    /// Deterministically drain every relevant mailbox in dependency order:
    /// monitors → coordinator → sink. Useful right after a burst of ticks
    /// when the caller is about to inspect alerts.
    pub async fn flush(&self) -> Result<()> {
        for m in self.monitors.values() {
            m.ask(monitor::Ping).await?;
        }
        self.coordinator.ask(coordinator::Ping).await?;
        self.sink.ask(sink::Ping).await?;
        Ok(())
    }

    /// Snapshot of every opportunity the sink has captured so far.
    /// Implicitly flushes the pipeline first.
    pub async fn alerts(&self) -> Result<Vec<ArbitrageOpportunity>> {
        self.flush().await?;
        Ok(self.sink.ask(sink::GetAlerts).await?)
    }

    // -----------------------------------------------------------------------
    // Lifecycle
    // -----------------------------------------------------------------------

    /// Three-step shutdown: drain the CoalitionRuntime (cancel token →
    /// close tracker → wait), then stop all kameo actors gracefully.
    pub async fn shutdown(self) {
        self.runtime.shutdown().await;

        // Stop actors in reverse-dependency order so no actor publishes
        // into a bus that's already stopped.
        for monitor_ref in self.monitors.values() {
            let _ = monitor_ref.stop_gracefully().await;
        }
        let _ = self.coordinator.stop_gracefully().await;
        let _ = self.tick_bus.stop_gracefully().await;
        let _ = self.alert_bus.stop_gracefully().await;
        let _ = self.sink.stop_gracefully().await;

        for monitor_ref in self.monitors.values() {
            monitor_ref.wait_for_shutdown().await;
        }
        self.coordinator.wait_for_shutdown().await;
        self.tick_bus.wait_for_shutdown().await;
        self.alert_bus.wait_for_shutdown().await;
        self.sink.wait_for_shutdown().await;

        tracing::info!("Swarm shut down cleanly.");
    }
}
