//! The async writer seam: an mpsc-fed task that drains `(StreamId, Record)`
//! pairs into an [`EventStore`].
//!
//! Shaped like the K3 decision tap
//! ([`spawn_decision_forwarder`](crate::subsystems::durable::spawn_decision_forwarder)),
//! with two deliberate differences: this task **drains** whatever is buffered in
//! `rx` when cancelled (the forwarder breaks immediately), and its sink is a
//! synchronous [`EventStore::append`] (blocking file I/O), offloaded to a
//! blocking thread via [`tokio::task::spawn_blocking`] so it never stalls a
//! tokio worker.
//!
//! ## Delivery is at-most-once from the tee onward
//!
//! Producers `try_send` (non-blocking); a full or closed channel drops the
//! record — the tap contract. This task then makes a **single** `append`
//! attempt per record: on failure it logs and drops, with no retry or replay
//! (and the failing stream wedges — see [`PersistenceError::StreamWedged`]).
//! There is no cursor-replay analogue to the K3 durable bus here, so end-to-end
//! delivery is at-most-once; durable retry/replay is a later-phase concern.
//!
//! [`PersistenceError::StreamWedged`]: super::errors::PersistenceError::StreamWedged

use std::sync::Arc;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use super::envelope::{Record, StreamId};
use super::store::EventStore;

/// Offload one blocking `append` to a blocking thread; log (never propagate) a
/// store error or a panicked append task.
async fn offload_append(store: &Arc<dyn EventStore>, stream: StreamId, record: Record) {
    let store = Arc::clone(store);
    match tokio::task::spawn_blocking(move || store.append(stream, record)).await {
        Ok(Ok(_head)) => {}
        Ok(Err(e)) => tracing::warn!("store append failed: {e}"),
        Err(e) => tracing::warn!("store append task panicked: {e}"),
    }
}

/// Spawn a task that appends every `(StreamId, Record)` received on `rx` to
/// `store`.
///
/// The task ends when either `token` is cancelled or `rx` closes (all senders
/// dropped). On cancellation it first drains whatever is already buffered in
/// `rx` (a `try_recv` loop) so records accepted before the cancel are still
/// written, then stops. Each `append` runs on a blocking thread; a store error
/// or a panicked append is logged and skipped. Spawned on `tracker` so the
/// caller's shutdown covers it.
pub fn spawn_store_writer(
    store: Arc<dyn EventStore>,
    mut rx: mpsc::Receiver<(StreamId, Record)>,
    tracker: &TaskTracker,
    token: CancellationToken,
) -> JoinHandle<()> {
    tracker.spawn(async move {
        loop {
            tokio::select! {
                biased;
                () = token.cancelled() => {
                    // Drain records already buffered before the cancel.
                    while let Ok((stream, record)) = rx.try_recv() {
                        offload_append(&store, stream, record).await;
                    }
                    break;
                }
                maybe = rx.recv() => match maybe {
                    Some((stream, record)) => offload_append(&store, stream, record).await,
                    None => break, // all senders dropped
                },
            }
        }
        tracing::debug!("store writer stopped");
    })
}
