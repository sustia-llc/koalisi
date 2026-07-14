//! `Swarm` — top-level orchestrator that ties the forex workers together.
//!
//! Wraps a [`CoalitionRuntime`] (TaskTracker + CancellationToken + three-step
//! shutdown) with forex-specific workers: monitors per pair, coordinator, sink,
//! and two `tokio::sync::broadcast` buses.
//!
//! ## Runtime shape (post-K3, issue #6)
//!
//! The former actor-framework actors are now plain tasks spawned on the runtime's
//! `TaskTracker`, each under a child of the runtime's root `CancellationToken`.
//! The two pub/sub buses are `tokio::sync::broadcast` channels:
//!
//! - `tick_bus: broadcast::Sender<TickUpdate>` — monitors publish, the
//!   coordinator subscribes.
//! - `alert_bus: broadcast::Sender<ArbitrageOpportunity>` — the coordinator
//!   publishes, the sink (and any user-attached listener) subscribes.
//!
//! ### Broadcast semantics
//!
//! Subscription is synchronous and immediate: `bus.subscribe()` returns a
//! `Receiver` that sees every message sent *after* the call. The old "the
//! subscriber must be spawned before `Subscribe`" ordering gotcha is gone —
//! `Swarm::new` subscribes the coordinator/sink before any tick is fed, so no
//! early message is missed. A subscriber that falls more than the bus capacity
//! (`BUS_CAPACITY`) behind gets a `RecvError::Lagged` and skips the overflow;
//! this is acceptable for ticks (best-effort) and the alert consumers are fast,
//! so alerts are not expected to lag under normal load.

use std::collections::{HashMap, HashSet};

use anyhow::{Result, anyhow};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::core::runtime::CoalitionRuntime;
use crate::market::{ArbitrageOpportunity, Pair, Tick, TickUpdate, Triangle};
use crate::subsystems::coordinator::{self, CoordinatorHandle};
use crate::subsystems::monitor::{self, MonitorHandle};
use crate::subsystems::sink::{self, SinkHandle};

/// Broadcast ring-buffer capacity for both buses. Generous so short bursts of
/// ticks/alerts don't lag a momentarily-behind subscriber.
const BUS_CAPACITY: usize = 1024;

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Builder-style config passed to [`Swarm::new`].
#[derive(Debug, Clone)]
pub struct SwarmConfig {
    pub triangles: Vec<Triangle>,
    pub threshold_bps: f64,
    pub history_capacity: usize,
}

impl SwarmConfig {
    /// Build a config from a list of triangles, pulling threshold / history
    /// capacity from `config/*.toml`.
    ///
    /// The config's `delivery_strategy` key is retained for backward
    /// compatibility but no longer consulted: the tokio `broadcast` buses have
    /// best-effort (lossy-on-lag) delivery, not a selectable strategy.
    pub fn from_settings(triangles: Vec<Triangle>) -> Self {
        let s = &crate::core::config::SETTINGS.coalition;
        Self {
            triangles,
            threshold_bps: s.threshold_bps,
            history_capacity: s.history_capacity,
        }
    }
}

// ---------------------------------------------------------------------------
// SwarmFeeder
// ---------------------------------------------------------------------------

/// A clone-able handle that knows how to route a tick to the right monitor.
/// Cheap to clone (each [`MonitorHandle`] is an mpsc sender).
///
/// Use this from background tasks that need to push ticks into the swarm
/// without holding a borrow on `Swarm`.
#[derive(Clone)]
pub struct SwarmFeeder {
    monitors: HashMap<Pair, MonitorHandle>,
}

impl SwarmFeeder {
    /// Route a single tick. Same deterministic-flush semantics as
    /// [`Swarm::feed_tick`] (acknowledged feed).
    pub async fn feed_tick(&self, tick: Tick) -> Result<()> {
        let monitor = self
            .monitors
            .get(&tick.pair)
            .ok_or_else(|| anyhow!("no monitor for pair {}", tick.pair))?;
        monitor.feed(tick).await
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
    monitors: HashMap<Pair, MonitorHandle>,
    coordinator: CoordinatorHandle,
    sink: SinkHandle,
    tick_bus: broadcast::Sender<TickUpdate>,
    alert_bus: broadcast::Sender<ArbitrageOpportunity>,
    runtime: CoalitionRuntime,
}

impl Swarm {
    /// Build the swarm: create the two broadcast buses, then spawn the sink
    /// (subscribed to the alert bus), the coordinator (subscribed to the tick
    /// bus), and one monitor per unique pair across all triangles. Every task
    /// runs on the runtime's `TaskTracker` under a child cancellation token.
    pub async fn new(config: SwarmConfig) -> Result<Self> {
        if config.triangles.is_empty() {
            return Err(anyhow!("SwarmConfig.triangles must be non-empty"));
        }

        let runtime = CoalitionRuntime::new();
        let tracker = runtime.task_tracker().clone();
        let root = runtime.cancellation_token().clone();

        // ---- buses ----
        let (tick_bus, tick_rx) = broadcast::channel::<TickUpdate>(BUS_CAPACITY);
        let (alert_bus, alert_rx) = broadcast::channel::<ArbitrageOpportunity>(BUS_CAPACITY);

        // ---- sink (subscribed to the alert bus) ----
        let sink = sink::spawn_sink(&tracker, root.child_token(), alert_rx);

        // ---- coordinator (subscribed to the tick bus, publishes alerts) ----
        let coordinator = coordinator::spawn_coordinator(
            &tracker,
            root.child_token(),
            config.triangles.clone(),
            config.threshold_bps,
            tick_rx,
            alert_bus.clone(),
        );

        // ---- monitors: one per unique pair ----
        let mut unique_pairs: HashSet<Pair> = HashSet::new();
        for triangle in &config.triangles {
            for p in triangle.pairs() {
                unique_pairs.insert(p.clone());
            }
        }

        let mut monitors = HashMap::with_capacity(unique_pairs.len());
        for pair in unique_pairs {
            let handle = monitor::spawn_monitor(
                &tracker,
                root.child_token(),
                pair.clone(),
                config.history_capacity,
                tick_bus.clone(),
            );
            monitors.insert(pair, handle);
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

    pub fn monitor(&self, pair: &Pair) -> Option<&MonitorHandle> {
        self.monitors.get(pair)
    }

    pub fn coordinator(&self) -> &CoordinatorHandle {
        &self.coordinator
    }

    pub fn sink(&self) -> &SinkHandle {
        &self.sink
    }

    /// The tick broadcast. Exposed so callers can `.subscribe()` additional
    /// consumers (e.g., a metrics exporter).
    pub fn tick_bus(&self) -> &broadcast::Sender<TickUpdate> {
        &self.tick_bus
    }

    /// The alert broadcast. Call `.subscribe()` to receive every alert emitted
    /// after the subscription.
    pub fn alert_bus(&self) -> &broadcast::Sender<ArbitrageOpportunity> {
        &self.alert_bus
    }

    pub fn cancellation_token(&self) -> &CancellationToken {
        self.runtime.cancellation_token()
    }

    pub fn task_tracker(&self) -> &tokio_util::task::TaskTracker {
        self.runtime.task_tracker()
    }

    /// A clone-able, owned feed handle that captures the monitor handles and
    /// nothing else. Pass it into background tasks (e.g., a source pump) so
    /// they can call `feed_tick` without holding a borrow on the `Swarm`
    /// itself.
    pub fn feeder(&self) -> SwarmFeeder {
        SwarmFeeder {
            monitors: self.monitors.clone(),
        }
    }

    // -----------------------------------------------------------------------
    // Feeds
    // -----------------------------------------------------------------------

    /// Route a single tick to the right monitor. Uses the acknowledged
    /// `feed` path, so the call returns only after the monitor has ingested
    /// the tick *and* published the resulting `TickUpdate` onto the tick bus.
    /// Combined with [`Swarm::flush`] (which pings monitors before the
    /// coordinator), this is a deterministic flushing primitive in tests.
    pub async fn feed_tick(&self, tick: Tick) -> Result<()> {
        let monitor = self
            .monitors
            .get(&tick.pair)
            .ok_or_else(|| anyhow!("no monitor for pair {}", tick.pair))?;
        monitor.feed(tick).await
    }

    /// Bootstrap historical data: feed every tick in `ticks` into the right
    /// monitor sequentially. Returns once all ticks have been ingested and the
    /// coordinator has drained them.
    pub async fn replay_history(&self, ticks: Vec<Tick>) -> Result<()> {
        for tick in ticks {
            self.feed_tick(tick).await?;
        }
        self.coordinator.ping().await?;
        Ok(())
    }

    /// Deterministically drain every relevant task in dependency order:
    /// monitors → coordinator → sink. Pinging the monitors first guarantees
    /// all `TickUpdate`s are on the tick bus before the coordinator's ping
    /// drains them (and, in turn, publishes any alerts the sink's ping then
    /// drains). Useful right after a burst of ticks when the caller is about to
    /// inspect alerts.
    pub async fn flush(&self) -> Result<()> {
        for m in self.monitors.values() {
            m.ping().await?;
        }
        self.coordinator.ping().await?;
        self.sink.ping().await?;
        Ok(())
    }

    /// Snapshot of every opportunity the sink has captured so far.
    /// Implicitly flushes the pipeline first.
    pub async fn alerts(&self) -> Result<Vec<ArbitrageOpportunity>> {
        self.flush().await?;
        self.sink.get_alerts().await
    }

    // -----------------------------------------------------------------------
    // Lifecycle
    // -----------------------------------------------------------------------

    /// Three-step shutdown: cancel the root token (every task's child token
    /// fires, so every task loop breaks), close the tracker, then drain all
    /// tracked tasks. Because the workers are all tracked tasks, this single
    /// call tears the whole swarm down — no separate actor-stop pass is needed.
    ///
    /// Ordering among tasks at teardown is unconstrained: a `broadcast::send`
    /// into a bus whose receivers have already stopped returns `Err` (which the
    /// senders ignore), so there is no "published into a stopped bus" hazard.
    pub async fn shutdown(self) {
        self.runtime.shutdown().await;
        tracing::info!("Swarm shut down cleanly.");
    }
}
