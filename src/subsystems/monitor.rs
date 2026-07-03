//! `MarketMonitor` — the forex instantiation of the domain-neutral ingestion
//! monitor ([`SampleMonitor`], issue #8).
//!
//! Since K5 the monitor logic lives generically in [`crate::ingest::monitor`];
//! this module is the thin forex binding: `MarketMonitor = SampleMonitor<Tick>`
//! (key = `Pair`, view = `Quote`), and [`spawn_monitor`] delegates to
//! [`spawn_sample_monitor`] so the swarm wiring in `swarm.rs` stays readable.
//!
//! ## Runtime shape (post-K3, issue #6)
//!
//! Unchanged from the original monitor: a spawned task owns the state struct and
//! callers talk to it through a [`MonitorHandle`]. `feed` is an acknowledged
//! ingest-then-publish (the deterministic-flush primitive); `tell` is
//! fire-and-forget; `ping` is a mailbox barrier; `snapshot` reads the state. See
//! the generic [`SampleMonitor`] for details — the
//! contracts and the wrong-key drop behaviour are identical.

use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::ingest::{SampleMonitor, SampleMonitorHandle, SampleSnapshot, spawn_sample_monitor};
use crate::market::{Tick, TickUpdate};

/// Per-pair market monitor: the forex instantiation of [`SampleMonitor`].
pub type MarketMonitor = SampleMonitor<Tick>;

/// Clone-able handle to a spawned [`MarketMonitor`] task.
pub type MonitorHandle = SampleMonitorHandle<Tick>;

/// Read-only view of a monitor's state (was a struct with `pair`/`latest`/
/// `history`; now the generic `key`/`latest`/`history`).
pub type MonitorSnapshot = SampleSnapshot<Tick>;

/// Spawn a monitor task on `tracker` under a child of `token`, publishing
/// [`TickUpdate`]s to `tick_bus`. Returns a [`MonitorHandle`].
///
/// Thin wrapper over [`spawn_sample_monitor`] fixed to the forex `Tick`.
pub fn spawn_monitor(
    tracker: &TaskTracker,
    token: CancellationToken,
    pair: crate::market::Pair,
    history_capacity: usize,
    tick_bus: broadcast::Sender<TickUpdate>,
) -> MonitorHandle {
    spawn_sample_monitor::<Tick>(tracker, token, pair, history_capacity, tick_bus)
}
