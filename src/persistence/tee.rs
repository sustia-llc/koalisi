//! The topology tee: an mpsc-fed task that projects tapped
//! [`TemporalEvent`]s onto the `Topology` stream (Phase 7 / P7.2,
//! sustia-llc/koalisi#30).
//!
//! It sits between the [`TemporalHypergraph`](crate::topology::TemporalHypergraph)
//! event tap (installed via `with_event_tap`) and
//! [`spawn_store_writer`](super::writer::spawn_store_writer): it converts each
//! event to a [`WireTopologyEvent`], CBOR-encodes the payload, wraps it in a
//! [`Record`], and forwards it to the writer channel.
//!
//! ## Delivery
//!
//! The lossy point is the tap (`try_send`, drop-with-warn — the at-most-once
//! contract). This internal hop uses `writer_tx.send().await`, so under normal
//! operation it applies backpressure to the tap-drain rather than dropping.
//! An encode failure is warned and the record dropped (at-most-once). Payload
//! encoding lives here; the store frames and hash-chains the record — this
//! never duplicates `chain.rs` framing.
//!
//! ## Shutdown discipline (lossless vs prompt)
//!
//! For a **lossless** shutdown, drop the tap sender and let the pipeline
//! drain: the forwarder exits when the tap channel closes (after forwarding
//! everything buffered), dropping `writer_tx`; the store writer then drains
//! and exits; `tracker.wait()` joins both. Cancelling a **shared** token
//! instead stops both tasks promptly, and the writer may close while this
//! forwarder has a record in flight — that record is warned and dropped
//! (still at-most-once, but lost after acceptance). Prompt cancellation is
//! for teardown-now; use the drop-to-drain order when the log must be
//! complete.

use std::fmt::Debug;

use serde::Serialize;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::topology::TemporalEvent;

use super::envelope::{Payload, Record, StreamId};
use super::wire::{WIRE_TOPOLOGY_SCHEMA_VERSION, WireTopologyEvent};

/// Project one event and forward it to the writer channel.
///
/// An encode failure is logged and the record dropped (at-most-once); a closed
/// writer channel is logged (the store writer has stopped).
async fn forward_one<V, HE, VW, HW>(
    event: &TemporalEvent<V, HE>,
    writer_tx: &mpsc::Sender<(StreamId, Record)>,
) where
    V: Clone + Debug + Sync + Into<VW>,
    HE: Clone + Debug + Sync + Into<HW>,
    VW: Serialize,
    HW: Serialize,
{
    let wire = WireTopologyEvent::<VW, HW>::from_event(event);
    let mut buf = Vec::new();
    if let Err(e) = ciborium::into_writer(&wire, &mut buf) {
        tracing::warn!("topology forwarder: payload encode failed, dropping record: {e}");
        return;
    }
    let record = Record {
        timestamp: event.timestamp().value(),
        schema_version: WIRE_TOPOLOGY_SCHEMA_VERSION,
        parents: Vec::new(),
        payload: Payload::Plain(buf),
    };
    if let Err(e) = writer_tx.send((StreamId::Topology, record)).await {
        tracing::warn!("topology forwarder: writer channel closed, dropping record: {e}");
    }
}

/// Spawn a task that projects every tapped [`TemporalEvent`] on `rx` onto the
/// `Topology` stream via `writer_tx`.
///
/// Ends when either `token` is cancelled or `rx` closes (the event tap and all
/// its clones dropped). On cancellation it first drains whatever is already
/// buffered in `rx` (mirroring [`spawn_store_writer`](super::writer::spawn_store_writer)),
/// then stops. Spawned on `tracker` so the caller's shutdown covers it.
///
/// The returned [`JoinHandle`] is intentionally droppable — the task is owned by
/// `tracker`, matching [`spawn_store_writer`](super::writer::spawn_store_writer).
#[allow(clippy::must_use_candidate)]
pub fn spawn_topology_forwarder<V, HE, VW, HW>(
    mut rx: mpsc::Receiver<TemporalEvent<V, HE>>,
    writer_tx: mpsc::Sender<(StreamId, Record)>,
    tracker: &TaskTracker,
    token: CancellationToken,
) -> JoinHandle<()>
where
    V: Clone + Debug + Send + Sync + Into<VW> + 'static,
    HE: Clone + Debug + Send + Sync + Into<HW> + 'static,
    VW: Serialize + Send + 'static,
    HW: Serialize + Send + 'static,
{
    tracker.spawn(async move {
        loop {
            tokio::select! {
                biased;
                () = token.cancelled() => {
                    // Drain events already buffered before the cancel.
                    while let Ok(event) = rx.try_recv() {
                        forward_one::<V, HE, VW, HW>(&event, &writer_tx).await;
                    }
                    break;
                }
                maybe = rx.recv() => match maybe {
                    Some(event) => forward_one::<V, HE, VW, HW>(&event, &writer_tx).await,
                    None => break, // event tap dropped
                },
            }
        }
        tracing::debug!("topology forwarder stopped");
    })
}
