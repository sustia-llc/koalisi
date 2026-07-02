//! `MarketMonitor` — a per-pair worker that maintains a ring buffer of
//! historical ticks, tracks the latest quote, and publishes a `TickUpdate`
//! to the swarm's tick broadcast on every incoming tick.
//!
//! Monitors don't decide on opportunities; that's the coordinator's job.
//! They're the "scout" workers of the swarm — each owns one pair.
//!
//! ## Runtime shape (post-K3, issue #6)
//!
//! A monitor is a plain [`MarketMonitor`] state struct driven by a spawned
//! task that owns it. Callers talk to the task through a [`MonitorHandle`]
//! (a clone-able wrapper over a `tokio::sync::mpsc` command sender). Ask-style
//! requests (`snapshot`, `ping`) carry a `oneshot::Sender` for the reply;
//! fire-and-forget ticks (`tell`) are a plain mpsc send; `feed` is an
//! acknowledged tick that returns only after the monitor has ingested it and
//! published the resulting `TickUpdate` (the deterministic-flush primitive the
//! old kameo `ask(tick)` provided).

use std::collections::VecDeque;

use anyhow::{Result, anyhow};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::market::{Pair, Quote, Tick, TickUpdate};

/// Command-channel capacity for a monitor. Generous so `feed`/`tell` rarely
/// block on backpressure.
const COMMAND_CAPACITY: usize = 1024;

// ---------------------------------------------------------------------------
// State core (reusable, task-agnostic)
// ---------------------------------------------------------------------------

/// The monitor's mutable state. Kept separate from the task loop so it can be
/// unit-tested (and reused by the supervised-worker example) without a runtime.
pub struct MarketMonitor {
    pair: Pair,
    history: VecDeque<Tick>,
    history_capacity: usize,
    latest: Option<Quote>,
}

impl MarketMonitor {
    /// Build an empty monitor for `pair` with the given ring-buffer capacity.
    pub fn new(pair: Pair, history_capacity: usize) -> Self {
        Self {
            pair,
            history: VecDeque::with_capacity(history_capacity),
            history_capacity,
            latest: None,
        }
    }

    /// Ingest a tick. Returns the `TickUpdate` to publish, or `None` if the
    /// tick was for the wrong pair (logged + dropped, matching the old actor).
    pub fn ingest(&mut self, tick: Tick) -> Option<TickUpdate> {
        if tick.pair != self.pair {
            tracing::warn!(
                expected = %self.pair,
                got = %tick.pair,
                "MarketMonitor received tick for the wrong pair; dropping"
            );
            return None;
        }

        // Ring-buffer push.
        if self.history.len() == self.history_capacity {
            self.history.pop_front();
        }
        let quote = tick.quote();
        self.history.push_back(tick);
        self.latest = Some(quote);

        Some(TickUpdate {
            pair: self.pair.clone(),
            quote,
        })
    }

    /// Snapshot the current state (latest quote + a copy of the ring buffer).
    pub fn snapshot(&self) -> MonitorSnapshot {
        MonitorSnapshot {
            pair: self.pair.clone(),
            latest: self.latest,
            history: self.history.iter().cloned().collect(),
        }
    }
}

/// Read-only view of a monitor's state.
#[derive(Debug, Clone)]
pub struct MonitorSnapshot {
    pub pair: Pair,
    pub latest: Option<Quote>,
    pub history: Vec<Tick>,
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

enum MonitorCommand {
    /// Fire-and-forget tick (was `ActorRef::tell(tick)`).
    Tick(Tick),
    /// Acknowledged tick: reply once ingested + published (was `ask(tick)`).
    Feed(Tick, oneshot::Sender<()>),
    /// Snapshot request.
    Snapshot(oneshot::Sender<MonitorSnapshot>),
    /// Flush barrier: reply after all prior commands are processed.
    Ping(oneshot::Sender<()>),
}

// ---------------------------------------------------------------------------
// Handle
// ---------------------------------------------------------------------------

/// Clone-able handle to a spawned monitor task.
#[derive(Clone)]
pub struct MonitorHandle {
    tx: mpsc::Sender<MonitorCommand>,
}

impl MonitorHandle {
    /// Acknowledged tick feed: returns only after the monitor has ingested the
    /// tick and published its `TickUpdate`. Mirrors the old `ask(tick)`
    /// deterministic-flush semantics.
    pub async fn feed(&self, tick: Tick) -> Result<()> {
        let (ack_tx, ack_rx) = oneshot::channel();
        self.tx
            .send(MonitorCommand::Feed(tick, ack_tx))
            .await
            .map_err(|_| anyhow!("monitor task is gone"))?;
        ack_rx.await.map_err(|_| anyhow!("monitor task dropped ack"))
    }

    /// Fire-and-forget tick (mirrors the old `tell(tick)`).
    pub async fn tell(&self, tick: Tick) -> Result<()> {
        self.tx
            .send(MonitorCommand::Tick(tick))
            .await
            .map_err(|_| anyhow!("monitor task is gone"))
    }

    /// Read the monitor's current state.
    pub async fn snapshot(&self) -> Result<MonitorSnapshot> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(MonitorCommand::Snapshot(tx))
            .await
            .map_err(|_| anyhow!("monitor task is gone"))?;
        rx.await.map_err(|_| anyhow!("monitor task dropped reply"))
    }

    /// Flush the monitor's mailbox (deterministic barrier used by tests).
    pub async fn ping(&self) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(MonitorCommand::Ping(tx))
            .await
            .map_err(|_| anyhow!("monitor task is gone"))?;
        rx.await.map_err(|_| anyhow!("monitor task dropped ping"))
    }
}

// ---------------------------------------------------------------------------
// Spawn
// ---------------------------------------------------------------------------

/// Spawn a monitor task on `tracker` under a child of `token`, publishing
/// `TickUpdate`s to `tick_bus`. Returns a [`MonitorHandle`].
pub fn spawn_monitor(
    tracker: &TaskTracker,
    token: CancellationToken,
    pair: Pair,
    history_capacity: usize,
    tick_bus: broadcast::Sender<TickUpdate>,
) -> MonitorHandle {
    let (tx, rx) = mpsc::channel(COMMAND_CAPACITY);
    let monitor = MarketMonitor::new(pair, history_capacity);
    tracker.spawn(monitor_loop(rx, token, monitor, tick_bus));
    MonitorHandle { tx }
}

async fn monitor_loop(
    mut rx: mpsc::Receiver<MonitorCommand>,
    token: CancellationToken,
    mut monitor: MarketMonitor,
    tick_bus: broadcast::Sender<TickUpdate>,
) {
    tracing::info!(pair = %monitor.pair, "MarketMonitor started");
    loop {
        tokio::select! {
            biased;
            _ = token.cancelled() => break,
            cmd = rx.recv() => match cmd {
                Some(MonitorCommand::Tick(tick)) => {
                    if let Some(update) = monitor.ingest(tick) {
                        // A send error just means no subscribers (e.g. the
                        // coordinator already stopped at teardown) — ignore.
                        let _ = tick_bus.send(update);
                    }
                }
                Some(MonitorCommand::Feed(tick, ack)) => {
                    if let Some(update) = monitor.ingest(tick) {
                        let _ = tick_bus.send(update);
                    }
                    let _ = ack.send(());
                }
                Some(MonitorCommand::Snapshot(reply)) => {
                    let _ = reply.send(monitor.snapshot());
                }
                Some(MonitorCommand::Ping(reply)) => {
                    let _ = reply.send(());
                }
                None => break,
            }
        }
    }
    tracing::debug!(pair = %monitor.pair, "MarketMonitor stopped");
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Pair {
        s.parse().unwrap()
    }

    #[test]
    fn ingest_ring_buffer_and_wrong_pair() {
        let mut m = MarketMonitor::new(p("EUR/USD"), 2);

        // Wrong pair → dropped, no update.
        assert!(m.ingest(Tick::new(p("GBP/USD"), 1.0, 1.0002, 0)).is_none());

        // Correct pairs fill + evict oldest.
        let u0 = m.ingest(Tick::new(p("EUR/USD"), 1.10, 1.1002, 0)).unwrap();
        assert_eq!(u0.pair, p("EUR/USD"));
        m.ingest(Tick::new(p("EUR/USD"), 1.11, 1.1102, 1)).unwrap();
        m.ingest(Tick::new(p("EUR/USD"), 1.12, 1.1202, 2)).unwrap();

        let snap = m.snapshot();
        assert_eq!(snap.history.len(), 2, "capacity 2 evicts oldest");
        let stamps: Vec<i64> = snap.history.iter().map(|t| t.timestamp_ms).collect();
        assert_eq!(stamps, vec![1, 2]);
        // mid = (bid + ask) / 2 = (1.12 + 1.1202) / 2 = 1.1201
        assert!((snap.latest.unwrap().mid - 1.1201).abs() < 1e-9);
    }

    #[tokio::test]
    async fn handle_feed_publishes_and_snapshot_flushes() {
        let tracker = TaskTracker::new();
        let token = CancellationToken::new();
        let (tick_bus, mut rx) = broadcast::channel::<TickUpdate>(16);

        let h = spawn_monitor(&tracker, token.child_token(), p("EUR/USD"), 8, tick_bus);

        // feed acks only after the update is published, so recv must succeed.
        h.feed(Tick::new(p("EUR/USD"), 1.10, 1.1002, 5)).await.unwrap();
        let update = rx.try_recv().expect("update published before feed ack");
        assert_eq!(update.pair, p("EUR/USD"));

        // tell + ping flush: after ping the tell must have been ingested.
        h.tell(Tick::new(p("EUR/USD"), 1.11, 1.1102, 6)).await.unwrap();
        h.ping().await.unwrap();
        let snap = h.snapshot().await.unwrap();
        assert_eq!(snap.history.len(), 2);

        token.cancel();
        tracker.close();
        tracker.wait().await;
    }
}
