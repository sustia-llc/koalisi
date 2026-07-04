//! Public envelope types for the persistence layer.
//!
//! These are the signed-off §6 surface of the Phase 7 design doc. They are
//! deliberately **serde-free**: the wire projection (a private `FrameV1`) lives
//! in [`chain`](super::chain), so no `Serialize`/`Deserialize` bound leaks into
//! the public API and the on-disk schema can be versioned independently of
//! these in-memory types.

/// Logical stream within the store. Each stream hash-chains independently, so
/// crypto-deletion classification, federation manifests, and revocation can
/// operate per-stream without coupling one chain to another.
///
/// The `u8` mapping (`to_u8`/`from_u8`) and directory names are load-bearing:
/// they are written into frames and name the on-disk subdirectories. Never
/// renumber the byte mapping or rename a directory without a format migration;
/// reordering the variant *declaration* is harmless (the mapping is an explicit
/// match, not the discriminant).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum StreamId {
    /// `WireTopologyEvent` mirror of `TemporalEvent` — anonymous structure,
    /// always plain (P7.2 producer).
    Topology,
    /// Decision traces with causal parents (P7.4 producer).
    Decisions,
    /// EFE belief snapshots — sealed (P7.3/P7.4 producer).
    Beliefs,
    /// **RESERVED** for `SwarmAgentic` particle lineages. Schema-only until #20
    /// unholds; no producer exists yet.
    Lineage,
    /// Grants, revocations, key destruction, manifest revocation. Never sealed,
    /// never revoked — it is the record revocation is made of (P7.3/P7.5).
    Registry,
    /// FAIR run manifests and output-pack digests (P7.5 producer).
    Provenance,
}

impl StreamId {
    /// Every stream, in stable byte order — used to enumerate on-disk streams.
    pub(crate) const ALL: [StreamId; 6] = [
        StreamId::Topology,
        StreamId::Decisions,
        StreamId::Beliefs,
        StreamId::Lineage,
        StreamId::Registry,
        StreamId::Provenance,
    ];

    /// The stable on-wire byte for this stream.
    #[must_use]
    pub(crate) const fn to_u8(self) -> u8 {
        match self {
            StreamId::Topology => 0,
            StreamId::Decisions => 1,
            StreamId::Beliefs => 2,
            StreamId::Lineage => 3,
            StreamId::Registry => 4,
            StreamId::Provenance => 5,
        }
    }

    /// Inverse of [`StreamId::to_u8`]; `None` for an unknown byte (a decode
    /// error at the call site rather than a silent default).
    #[must_use]
    pub(crate) const fn from_u8(byte: u8) -> Option<StreamId> {
        match byte {
            0 => Some(StreamId::Topology),
            1 => Some(StreamId::Decisions),
            2 => Some(StreamId::Beliefs),
            3 => Some(StreamId::Lineage),
            4 => Some(StreamId::Registry),
            5 => Some(StreamId::Provenance),
            _ => None,
        }
    }

    /// Lowercase subdirectory name for this stream under the store root.
    #[must_use]
    pub(crate) const fn as_dir_name(self) -> &'static str {
        match self {
            StreamId::Topology => "topology",
            StreamId::Decisions => "decisions",
            StreamId::Beliefs => "beliefs",
            StreamId::Lineage => "lineage",
            StreamId::Registry => "registry",
            StreamId::Provenance => "provenance",
        }
    }
}

/// Per-stream dense monotonic cursor. Sequence numbers start at 0 and are
/// gap-free; hashes are for integrity and anchoring, not addressing.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SequenceNo(pub u64);

/// Algorithm tag on the *in-memory* [`RecordHash`]. It is not carried on the
/// wire: a `FrameV1` stores only the bare 32-byte digest and pins SHA-256
/// implicitly, so changing the on-disk hash algorithm is a frame-version bump
/// (see the frame version constant in `chain`), not a tag flip.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HashAlgorithm {
    /// SHA-256 — the v1 archival-ubiquity and KERI-interop choice.
    Sha256,
}

/// Algorithm-tagged digest over the exact stored frame bytes (see the module
/// docs' hash contract).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordHash {
    /// The algorithm that produced [`RecordHash::digest`].
    pub algorithm: HashAlgorithm,
    /// The raw 32-byte digest.
    pub digest: [u8; 32],
}

/// Opaque handle into the (destroyable) key registry. Unused in P7.1 beyond the
/// [`Payload::Sealed`] schema; the keystore lands in P7.3.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct KeyId(pub String);

/// Payload as it enters the store: already projected + CBOR-encoded, optionally
/// sealed. The store never sees the plaintext of a sealed payload — sealing
/// happens in the adapter before [`EventStore::append`](super::store::EventStore::append).
#[derive(Clone, Debug)]
pub enum Payload {
    /// Plaintext bytes (the producer's already-encoded projection).
    Plain(Vec<u8>),
    /// AEAD-sealed bytes. Schema-only in P7.1: the store round-trips these
    /// fields opaquely; the keystore and crypto-deletion land in P7.3.
    Sealed {
        /// Registry handle for the key that seals [`Payload::Sealed::ciphertext`].
        key_id: KeyId,
        /// 24-byte AEAD nonce (XChaCha20-Poly1305 shape).
        nonce: [u8; 24],
        /// The sealed ciphertext.
        ciphertext: Vec<u8>,
    },
}

/// Causal parent reference — one node-to-node edge of the cross-stream DAG (the
/// EPP `EffectLog` shape).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventRef {
    /// Stream the parent record lives in.
    pub stream: StreamId,
    /// Sequence number of the parent record.
    pub seq: SequenceNo,
}

/// What a producer hands to
/// [`EventStore::append`](super::store::EventStore::append); the store assigns
/// the sequence number and `prev_hash`.
#[derive(Clone, Debug)]
pub struct Record {
    /// Logical-clock value (a `Timestamp`'s raw `u64`).
    pub timestamp: u64,
    /// Wire-schema version of the payload; bump on a projection change.
    pub schema_version: u16,
    /// Causal antecedents (empty for topology events).
    pub parents: Vec<EventRef>,
    /// The record body, plain or sealed.
    pub payload: Payload,
}

/// A record as read back, carrying the envelope fields the store assigned.
#[derive(Clone, Debug)]
pub struct StoredRecord {
    /// The assigned sequence number.
    pub seq: SequenceNo,
    /// Predecessor digest; `None` only for a stream's genesis record.
    pub prev_hash: Option<RecordHash>,
    /// This record's own digest (over its exact stored frame bytes).
    pub hash: RecordHash,
    /// The original producer-supplied record.
    pub record: Record,
}

/// Stream head: the `(seq, hash)` a KERI controller anchors in a KEL `ixn`.
#[derive(Clone, Copy, Debug)]
pub struct StreamHead {
    /// Sequence number of the head record.
    pub seq: SequenceNo,
    /// Digest of the head record.
    pub hash: RecordHash,
}

#[cfg(test)]
mod tests {
    use super::StreamId;

    #[test]
    fn stream_id_u8_round_trips_for_all_six() {
        for stream in StreamId::ALL {
            assert_eq!(StreamId::from_u8(stream.to_u8()), Some(stream));
        }
    }

    #[test]
    fn unknown_stream_byte_is_none() {
        assert_eq!(StreamId::from_u8(6), None);
        assert_eq!(StreamId::from_u8(255), None);
    }

    #[test]
    fn dir_names_are_distinct_and_lowercase() {
        let mut names: Vec<&str> = StreamId::ALL.iter().map(|s| s.as_dir_name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 6);
        assert!(
            names
                .iter()
                .all(|n| n.chars().all(|c| c.is_ascii_lowercase()))
        );
    }
}
