//! The decision forwarder: an mpsc-fed task that persists tapped
//! [`DecisionTrace`]s on the `Decisions` stream (Phase 7 / P7.4,
//! sustia-llc/koalisi#32). It sits between the trace tap of
//! [`CoalitionService::spawn_with_trace_tap`](crate::subsystems::coalition_actor::CoalitionService::spawn_with_trace_tap)
//! and [`spawn_store_writer`](super::spawn_store_writer); see
//! [`spawn_decision_store_forwarder`] for the record it builds and its
//! delivery and shutdown contracts.

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::subsystems::coalition_actor::DecisionTrace;

use super::envelope::{EventRef, Payload, Record, SequenceNo, StreamId};
use super::errors::PersistenceError;
use super::wire::{WIRE_DECISION_SCHEMA_VERSION, WireDecision};

/// Build the `Decisions` stream [`Record`] for `trace` (the shape is listed on
/// [`spawn_decision_store_forwarder`]); [`PersistenceError::Encode`] if the
/// CBOR encoding of the [`WireDecision`] fails.
fn decision_store_record(trace: &DecisionTrace) -> Result<Record, PersistenceError> {
    let wire = WireDecision::from_record(&trace.record);
    let mut buf = Vec::new();
    ciborium::into_writer(&wire, &mut buf).map_err(|e| PersistenceError::Encode(e.to_string()))?;
    let parents = trace
        .event_log_len
        .checked_sub(1)
        .map(|seq| EventRef {
            stream: StreamId::Topology,
            seq: SequenceNo(seq),
        })
        .into_iter()
        .collect();
    Ok(Record {
        timestamp: trace.timestamp,
        schema_version: WIRE_DECISION_SCHEMA_VERSION,
        parents,
        payload: Payload::Plain(buf),
    })
}

/// Build `trace`'s `Decisions` stream record and send it on `writer_tx`,
/// waiting for capacity.
///
/// An encode failure or a closed writer channel is logged and the trace
/// dropped.
pub(crate) async fn forward_decision_trace(
    trace: &DecisionTrace,
    writer_tx: &mpsc::Sender<(StreamId, Record)>,
) {
    let record = match decision_store_record(trace) {
        Ok(record) => record,
        Err(e) => {
            tracing::warn!("decision forwarder: {e}; dropping trace");
            return;
        }
    };
    if let Err(e) = writer_tx.send((StreamId::Decisions, record)).await {
        tracing::warn!("decision forwarder: writer channel closed, dropping record: {e}");
    }
}

/// Spawn a task that persists every [`DecisionTrace`] on `rx` to the
/// `Decisions` stream via `writer_tx`.
///
/// ## The record
///
/// - `timestamp` = [`DecisionTrace::timestamp`].
/// - `schema_version` = [`WIRE_DECISION_SCHEMA_VERSION`].
/// - `parents` = `[]` when [`DecisionTrace::event_log_len`] is 0, else one
///   [`EventRef`] to `Topology` sequence number `event_log_len − 1`.
/// - `payload` = [`Payload::Plain`] of the CBOR-encoded [`WireDecision`].
///
/// **Parent precondition.** `Topology` record `event_log_len − 1` is the
/// in-memory event log's entry at index `event_log_len − 1` only when the
/// `Topology` stream was empty and was fed by a graph event tap installed on
/// an empty event log (before the graph's first mutation) that dropped no
/// event. Otherwise the parent names whichever `Topology` record holds that
/// sequence number, or a sequence number no record holds.
///
/// ## Delivery
///
/// The trace tap is lossy (`try_send`, drop-with-warn). This task forwards
/// with `writer_tx.send().await`, so it waits for writer-channel capacity
/// instead of dropping. An encode failure or a closed writer channel is warned
/// and the trace dropped, and the store writer makes a single `append` attempt
/// per record, so delivery from the tap onward is at-most-once.
///
/// ## Shutdown
///
/// Ends when either `token` is cancelled or `rx` closes (the trace tap and all
/// its clones dropped). On cancellation it first forwards whatever is already
/// buffered in `rx`, then stops — as
/// [`spawn_topology_forwarder`](super::spawn_topology_forwarder). Spawned on
/// `tracker` so the caller's shutdown covers it; the returned [`JoinHandle`]
/// may be dropped.
#[allow(clippy::must_use_candidate)]
pub fn spawn_decision_store_forwarder(
    mut rx: mpsc::Receiver<DecisionTrace>,
    writer_tx: mpsc::Sender<(StreamId, Record)>,
    tracker: &TaskTracker,
    token: CancellationToken,
) -> JoinHandle<()> {
    tracker.spawn(async move {
        while let Some(trace) = next_trace(&mut rx, &token).await {
            forward_decision_trace(&trace, &writer_tx).await;
        }
        tracing::debug!("decision store forwarder stopped");
    })
}

/// The next trace a draining task handles: the next one received on `rx`, or
/// `None` once `rx` is closed (the trace tap and all its clones dropped) and
/// empty. After `token` is cancelled, only traces already buffered in `rx`
/// are returned, then `None`.
pub(crate) async fn next_trace(
    rx: &mut mpsc::Receiver<DecisionTrace>,
    token: &CancellationToken,
) -> Option<DecisionTrace> {
    tokio::select! {
        biased;
        () = token.cancelled() => rx.try_recv().ok(),
        maybe = rx.recv() => maybe,
    }
}
