//! Durable, portable, hash-chained event log (feature `persistence`).
//!
//! This is the Phase 7 (issue #21) source-of-truth store: a segmented,
//! append-only, per-stream hash-chained frame log. Everything else — the K3
//! `durable` `SurrealDB` bus, future dashboards, search indexes — is a projection
//! rebuildable from this log. [`spawn_topology_forwarder`] writes `Topology`
//! records; [`spawn_decision_store_forwarder`] (and, with feature `durable`,
//! `subsystems::durable::spawn_decision_log_bus_tee`) writes `Decisions`
//! records, each carrying at most one causal parent into `Topology`. Sealing +
//! crypto-deletion (P7.3), belief snapshots, and federation (P7.5) are later
//! phases.
//!
//! ## Streams are independent
//!
//! A [`StreamId`] names one of six logical streams
//! ([`Topology`](StreamId::Topology), [`Decisions`](StreamId::Decisions),
//! [`Beliefs`](StreamId::Beliefs), [`Lineage`](StreamId::Lineage) — reserved for
//! #20, [`Registry`](StreamId::Registry), [`Provenance`](StreamId::Provenance)).
//! Each hash-chains **independently** and occupies its own on-disk subdirectory,
//! so crypto-deletion classification, federation manifests, and revocation
//! operate per-stream without coupling one chain to another.
//!
//! ## Hash contract (load-bearing)
//!
//! A record's [`RecordHash`] is SHA-256 over the **exact stored frame bytes**
//! (excluding the 4-byte length prefix). Each frame carries the predecessor's
//! digest in its `prev_hash` field, so a record's hash covers its predecessor's
//! — that is the chain. Verification re-reads stored bytes and re-hashes; it
//! never re-encodes a decoded frame. Consequently chain integrity does not
//! depend on encoder determinism across implementations or decades, and (once
//! P7.3 lands) destroying a sealed record's key cannot break verification —
//! the chain hashes ciphertext bytes.
//!
//! ## Durability policy
//!
//! [`FileEventStore`] `append` encodes, length-prefixes, writes, and `flush`es
//! the buffered writer — so records survive a process kill (they reach the OS).
//! It does **not** `fsync` per append; a segment is `sync_data`'d on rotation
//! only. Power-loss durability (fsync-per-append) is a future config knob, out
//! of scope for P7.1. On [`FileEventStore::open`] an incomplete final frame (a
//! torn write) is truncated and the tail is re-verified against its predecessor.
//!
//! ## Sealing is schema-only in P7.1
//!
//! [`Payload::Sealed`] exists as a schema and the store round-trips its
//! `key_id`/`nonce`/`ciphertext` opaquely, but no keystore, encryption, or
//! crypto-deletion exists yet — that is P7.3. Likewise
//! [`PersistenceError::KeyUnavailable`], [`PersistenceError::ManifestInvalid`],
//! and [`PersistenceError::ManifestRevoked`] are reserved for later phases.

mod chain;
mod decision;
mod envelope;
mod errors;
mod replay;
mod store;
mod tee;
mod wire;
mod writer;

pub use decision::spawn_decision_store_forwarder;
#[cfg(feature = "durable")]
pub(crate) use decision::{forward_decision_trace, next_trace};
pub use envelope::{
    EventRef, HashAlgorithm, KeyId, Payload, Record, RecordHash, SequenceNo, StoredRecord,
    StreamHead, StreamId,
};
pub use errors::PersistenceError;
pub use replay::replay_into_event_log;
pub use store::{EventStore, FileEventStore, FileStoreConfig};
pub use tee::spawn_topology_forwarder;
pub use wire::{
    WIRE_DECISION_SCHEMA_VERSION, WIRE_TOPOLOGY_SCHEMA_VERSION, WireConversionError, WireDecision,
    WireLineage, WireTopologyEvent,
};
pub use writer::spawn_store_writer;
