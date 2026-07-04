//! Replay the `Topology` stream back into an in-memory [`EventLog`]
//! (Phase 7 / P7.2, sustia-llc/koalisi#30).
//!
//! Per the §7 "one query path" decision, the durable store grows no query API
//! of its own: [`replay_into_event_log`] decodes `Topology`-stream frames back
//! into [`TemporalEvent`]s and appends them into a fresh [`EventLog`], so all
//! existing consumers — [`TemporalQueries`](crate::topology::TemporalQueries),
//! [`TemporalAnalytics`](crate::topology::TemporalAnalytics), and the #18
//! `magnitude_history` trajectory — run unchanged against a replayed log. The
//! pre-registered parity gate is that a query over a replayed log equals the
//! same query over the live in-memory series.

use std::fmt::{Debug, Display};

use serde::de::DeserializeOwned;

use crate::topology::{EventLog, TemporalEvent};

use super::envelope::{Payload, SequenceNo, StreamId};
use super::errors::PersistenceError;
use super::store::EventStore;
use super::wire::{WIRE_TOPOLOGY_SCHEMA_VERSION, WireTopologyEvent};

/// Records read per `read_from` batch during replay.
const REPLAY_BATCH: usize = 1024;

/// Replay `Topology`-stream records with sequence `>= from` into a fresh
/// [`EventLog`].
///
/// Reads the stream in `REPLAY_BATCH`-sized windows to its head, decoding each
/// [`Payload::Plain`] frame's CBOR into a [`WireTopologyEvent`] and converting it
/// back to a [`TemporalEvent`] (via the caller-chosen `VW`/`HW` projections;
/// pass the weight types themselves for the identity projection). The returned
/// log carries the same events, in the same order, as the live log that
/// produced them — **provided the pipeline is quiescent**: the tap has been
/// dropped and the forwarder + writer tasks joined (the lossless shutdown
/// order in the tee module docs). Replaying while a writer is still appending
/// silently yields a prefix of the stream, not an error.
///
/// This is synchronous (the [`EventStore`] trait and [`EventLog::append`] are
/// both sync); on an async path, wrap it in
/// [`tokio::task::spawn_blocking`](tokio::task::spawn_blocking) so the file I/O
/// does not stall a tokio worker.
///
/// # Errors
///
/// - [`PersistenceError::SchemaVersionUnsupported`] if a record's
///   `schema_version` exceeds [`WIRE_TOPOLOGY_SCHEMA_VERSION`].
/// - [`PersistenceError::Decode`] if a record is [`Payload::Sealed`] (the
///   `Topology` stream is always plain), if CBOR decoding fails, or if a weight
///   projection / index fails to convert back (the record's sequence number is
///   attached).
/// - Any [`PersistenceError`] surfaced by the underlying
///   [`EventStore::read_from`].
pub fn replay_into_event_log<V, HE, VW, HW>(
    store: &dyn EventStore,
    from: SequenceNo,
) -> Result<EventLog<V, HE>, PersistenceError>
where
    V: Clone + Debug,
    HE: Clone + Debug,
    VW: DeserializeOwned + TryInto<V>,
    <VW as TryInto<V>>::Error: Display,
    HW: DeserializeOwned + TryInto<HE>,
    <HW as TryInto<HE>>::Error: Display,
{
    let mut log = EventLog::new();
    let mut cursor = from;

    loop {
        let batch = store.read_from(StreamId::Topology, cursor, REPLAY_BATCH)?;
        let Some(last) = batch.last() else {
            break; // reached the head
        };
        let next = SequenceNo(last.seq.0 + 1);

        for stored in &batch {
            if stored.record.schema_version > WIRE_TOPOLOGY_SCHEMA_VERSION {
                return Err(PersistenceError::SchemaVersionUnsupported {
                    found: stored.record.schema_version,
                    max_supported: WIRE_TOPOLOGY_SCHEMA_VERSION,
                });
            }
            let bytes = match &stored.record.payload {
                Payload::Plain(bytes) => bytes,
                Payload::Sealed { .. } => {
                    return Err(PersistenceError::Decode {
                        stream: StreamId::Topology,
                        seq: stored.seq,
                        message: "Topology stream records are Plain; found Sealed".to_owned(),
                    });
                }
            };
            let wire: WireTopologyEvent<VW, HW> =
                ciborium::from_reader(bytes.as_slice()).map_err(|e| PersistenceError::Decode {
                    stream: StreamId::Topology,
                    seq: stored.seq,
                    message: e.to_string(),
                })?;
            let event: TemporalEvent<V, HE> =
                wire.try_into_event()
                    .map_err(|e| PersistenceError::Decode {
                        stream: StreamId::Topology,
                        seq: stored.seq,
                        message: e.message,
                    })?;
            log.append(event);
        }

        cursor = next;
    }

    Ok(log)
}
