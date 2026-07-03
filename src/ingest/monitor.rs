//! [`SampleMonitor`] — a per-key worker that maintains a ring buffer of
//! historical [`Sample`]s, tracks the latest [`view`](Sample::view), and
//! publishes a [`SampleUpdate`] to a `broadcast` bus on every ingested sample.
//!
//! This is the domain-neutral generalisation of the forex `MarketMonitor`
//! (issue #8): `MarketMonitor` is now `SampleMonitor<Tick>`. The runtime shape
//! and every semantic below are ported verbatim from the K3 monitor — only the
//! concrete forex types became the `Sample` associated types.
//!
//! ## Runtime shape (post-K3, issue #6)
//!
//! A monitor is a plain [`SampleMonitor`] state struct driven by a spawned task
//! that owns it. Callers talk to the task through a [`SampleMonitorHandle`]
//! (a clone-able wrapper over a `tokio::sync::mpsc` command sender). Ask-style
//! requests (`snapshot`, `ping`) carry a `oneshot::Sender` for the reply;
//! fire-and-forget samples (`tell`) are a plain mpsc send; `feed` is an
//! acknowledged sample that returns only after the monitor has ingested it and
//! published the resulting [`SampleUpdate`] (the deterministic-flush primitive
//! the old actor `ask(tick)` provided).

use std::collections::VecDeque;

use anyhow::{Result, anyhow};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use super::sample::Sample;

/// Command-channel capacity for a monitor. Generous so `feed`/`tell` rarely
/// block on backpressure.
const COMMAND_CAPACITY: usize = 1024;

// ---------------------------------------------------------------------------
// State core (reusable, task-agnostic)
// ---------------------------------------------------------------------------

/// The monitor's mutable state. Kept separate from the task loop so it can be
/// unit-tested (and reused by the supervised-worker example) without a runtime.
pub struct SampleMonitor<S: Sample> {
    key: S::Key,
    history: VecDeque<S>,
    history_capacity: usize,
    latest: Option<S::View>,
}

impl<S: Sample> SampleMonitor<S> {
    /// Build an empty monitor for `key` with the given ring-buffer capacity.
    pub fn new(key: S::Key, history_capacity: usize) -> Self {
        Self {
            key,
            history: VecDeque::with_capacity(history_capacity),
            history_capacity,
            latest: None,
        }
    }

    /// The key this monitor is bound to.
    pub fn key(&self) -> &S::Key {
        &self.key
    }

    /// Ingest a sample. Returns the [`SampleUpdate`] to publish, or `None` if
    /// the sample was for the wrong key (logged + dropped, matching the old
    /// actor's wrong-pair behaviour).
    pub fn ingest(&mut self, sample: S) -> Option<SampleUpdate<S>> {
        // Zero-alloc key comparison: `key()` borrows, so the wrong-key check
        // never clones (matching the pre-K5 `tick.pair != self.pair`). The only
        // key clone is the one below, when constructing the published update.
        if sample.key() != &self.key {
            tracing::warn!(
                expected = %self.key,
                got = %sample.key(),
                "SampleMonitor received sample for the wrong key; dropping"
            );
            return None;
        }

        // Ring-buffer push.
        if self.history.len() == self.history_capacity {
            self.history.pop_front();
        }
        let view = sample.view();
        self.history.push_back(sample);
        self.latest = Some(view.clone());

        Some(SampleUpdate {
            key: self.key.clone(),
            view,
        })
    }

    /// Snapshot the current state (latest view + a copy of the ring buffer).
    pub fn snapshot(&self) -> SampleSnapshot<S> {
        SampleSnapshot {
            key: self.key.clone(),
            latest: self.latest.clone(),
            history: self.history.iter().cloned().collect(),
        }
    }
}

// ---------------------------------------------------------------------------
// SampleUpdate — what a monitor publishes on every ingested sample
// ---------------------------------------------------------------------------

/// What a [`SampleMonitor`] publishes to its broadcast bus on every fresh
/// sample: the routing key plus the distilled latest view.
///
/// The forex `TickUpdate` is `SampleUpdate<Tick>`.
pub struct SampleUpdate<S: Sample> {
    pub key: S::Key,
    pub view: S::View,
}

// Hand-written impls: a `#[derive]` would add a spurious `S: Clone`/`S: Debug`
// bound on the type parameter, but the fields are the associated types
// `S::Key`/`S::View`, so we bound those directly.
impl<S: Sample> Clone for SampleUpdate<S> {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            view: self.view.clone(),
        }
    }
}

impl<S: Sample> std::fmt::Debug for SampleUpdate<S>
where
    S::Key: std::fmt::Debug,
    S::View: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SampleUpdate")
            .field("key", &self.key)
            .field("view", &self.view)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// SampleSnapshot — read-only view of a monitor's state
// ---------------------------------------------------------------------------

/// Read-only view of a monitor's state.
pub struct SampleSnapshot<S: Sample> {
    pub key: S::Key,
    pub latest: Option<S::View>,
    pub history: Vec<S>,
}

impl<S: Sample> Clone for SampleSnapshot<S> {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            latest: self.latest.clone(),
            history: self.history.clone(),
        }
    }
}

impl<S: Sample> std::fmt::Debug for SampleSnapshot<S>
where
    S: std::fmt::Debug,
    S::Key: std::fmt::Debug,
    S::View: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SampleSnapshot")
            .field("key", &self.key)
            .field("latest", &self.latest)
            .field("history", &self.history)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

enum MonitorCommand<S: Sample> {
    /// Fire-and-forget sample (was `ActorRef::tell(tick)`).
    Sample(S),
    /// Acknowledged sample: reply once ingested + published (was `ask(tick)`).
    Feed(S, oneshot::Sender<()>),
    /// Snapshot request.
    Snapshot(oneshot::Sender<SampleSnapshot<S>>),
    /// Flush barrier: reply after all prior commands are processed.
    Ping(oneshot::Sender<()>),
}

// ---------------------------------------------------------------------------
// Handle
// ---------------------------------------------------------------------------

/// Clone-able handle to a spawned monitor task.
pub struct SampleMonitorHandle<S: Sample> {
    tx: mpsc::Sender<MonitorCommand<S>>,
}

// Hand-written `Clone` so the handle stays `Clone` regardless of `S` (a derive
// would wrongly require `S: Clone`; only the `mpsc::Sender` is cloned).
impl<S: Sample> Clone for SampleMonitorHandle<S> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
        }
    }
}

impl<S: Sample> SampleMonitorHandle<S> {
    /// Acknowledged sample feed: returns only after the monitor has ingested the
    /// sample and published its [`SampleUpdate`]. Mirrors the old `ask(tick)`
    /// deterministic-flush semantics.
    ///
    /// # Errors
    ///
    /// Returns an error if the monitor task has stopped (its command channel is
    /// closed) or dropped the acknowledgement before replying.
    pub async fn feed(&self, sample: S) -> Result<()> {
        let (ack_tx, ack_rx) = oneshot::channel();
        self.tx
            .send(MonitorCommand::Feed(sample, ack_tx))
            .await
            .map_err(|_| anyhow!("monitor task is gone"))?;
        ack_rx
            .await
            .map_err(|_| anyhow!("monitor task dropped ack"))
    }

    /// Fire-and-forget sample (mirrors the old `tell(tick)`).
    ///
    /// # Errors
    ///
    /// Returns an error if the monitor task has stopped (its command channel is
    /// closed).
    pub async fn tell(&self, sample: S) -> Result<()> {
        self.tx
            .send(MonitorCommand::Sample(sample))
            .await
            .map_err(|_| anyhow!("monitor task is gone"))
    }

    /// Read the monitor's current state.
    ///
    /// # Errors
    ///
    /// Returns an error if the monitor task has stopped (its command channel is
    /// closed) or dropped the reply before answering.
    pub async fn snapshot(&self) -> Result<SampleSnapshot<S>> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(MonitorCommand::Snapshot(tx))
            .await
            .map_err(|_| anyhow!("monitor task is gone"))?;
        rx.await.map_err(|_| anyhow!("monitor task dropped reply"))
    }

    /// Flush the monitor's mailbox (deterministic barrier used by tests).
    ///
    /// # Errors
    ///
    /// Returns an error if the monitor task has stopped (its command channel is
    /// closed) or dropped the reply before answering.
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
/// [`SampleUpdate`]s to `bus`. Returns a [`SampleMonitorHandle`].
pub fn spawn_sample_monitor<S: Sample>(
    tracker: &TaskTracker,
    token: CancellationToken,
    key: S::Key,
    capacity: usize,
    bus: broadcast::Sender<SampleUpdate<S>>,
) -> SampleMonitorHandle<S> {
    let (tx, rx) = mpsc::channel(COMMAND_CAPACITY);
    let monitor = SampleMonitor::new(key, capacity);
    tracker.spawn(monitor_loop(rx, token, monitor, bus));
    SampleMonitorHandle { tx }
}

async fn monitor_loop<S: Sample>(
    mut rx: mpsc::Receiver<MonitorCommand<S>>,
    token: CancellationToken,
    mut monitor: SampleMonitor<S>,
    bus: broadcast::Sender<SampleUpdate<S>>,
) {
    tracing::info!(key = %monitor.key, "SampleMonitor started");
    loop {
        tokio::select! {
            biased;
            () = token.cancelled() => break,
            cmd = rx.recv() => match cmd {
                Some(MonitorCommand::Sample(sample)) => {
                    if let Some(update) = monitor.ingest(sample) {
                        // A send error just means no subscribers (e.g. the
                        // coordinator already stopped at teardown) — ignore.
                        let _ = bus.send(update);
                    }
                }
                Some(MonitorCommand::Feed(sample, ack)) => {
                    if let Some(update) = monitor.ingest(sample) {
                        let _ = bus.send(update);
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
    tracing::debug!(key = %monitor.key, "SampleMonitor stopped");
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// These exercise the generic monitor through the forex `Tick` instantiation, so
// they double as the `SampleMonitor<Tick>` (= `MarketMonitor`) guard. Their
// assertions are ports of the old monitor tests: `ingest_ring_buffer_and_wrong_key`
// checks the state core, and the `#[tokio::test]` drives the spawned task through
// the same feed/flush checkpoints as the original.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::market::{Pair, Tick};

    fn p(s: &str) -> Pair {
        s.parse().unwrap()
    }

    #[test]
    fn ingest_ring_buffer_and_wrong_key() {
        let mut m = SampleMonitor::<Tick>::new(p("EUR/USD"), 2);

        // Wrong key → dropped, no update.
        assert!(m.ingest(Tick::new(p("GBP/USD"), 1.0, 1.0002, 0)).is_none());

        // Correct keys fill + evict oldest.
        let u0 = m.ingest(Tick::new(p("EUR/USD"), 1.10, 1.1002, 0)).unwrap();
        assert_eq!(u0.key, p("EUR/USD"));
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
        let (bus, mut rx) = broadcast::channel::<SampleUpdate<Tick>>(16);

        let h = spawn_sample_monitor::<Tick>(&tracker, token.child_token(), p("EUR/USD"), 8, bus);

        // feed acks only after the update is published, so recv must succeed.
        h.feed(Tick::new(p("EUR/USD"), 1.10, 1.1002, 5))
            .await
            .unwrap();
        let update = rx.try_recv().expect("update published before feed ack");
        assert_eq!(update.key, p("EUR/USD"));

        // tell + ping flush: after ping the tell must have been ingested.
        h.tell(Tick::new(p("EUR/USD"), 1.11, 1.1102, 6))
            .await
            .unwrap();
        h.ping().await.unwrap();
        let snap = h.snapshot().await.unwrap();
        assert_eq!(snap.history.len(), 2);

        token.cancel();
        tracker.close();
        tracker.wait().await;
    }
}
