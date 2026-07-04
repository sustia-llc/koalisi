//! Frame encoding/decoding, hashing, and chain-verification helpers.
//!
//! The on-disk frame is a private, serde-derived `FrameV1` carrying only
//! primitives, keeping the public envelope types (in [`envelope`](super::envelope))
//! serde-free. Frames are CBOR-encoded via `ciborium`.
//!
//! **Hash contract:** a record's [`RecordHash`] is SHA-256 over the exact frame
//! bytes as stored (excluding the 4-byte length prefix). `prev_hash` lives
//! *inside* the frame, so each record's hash covers its predecessor's digest —
//! that is the chain. Verification re-hashes stored bytes and never re-encodes a
//! decoded frame.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::envelope::{
    EventRef, HashAlgorithm, KeyId, Payload, Record, RecordHash, SequenceNo, StoredRecord, StreamId,
};
use super::errors::PersistenceError;

/// Current on-disk frame-format version.
///
/// Bumped only when the *frame* layout changes — including a change of hash
/// algorithm, which `FrameV1` encodes implicitly as SHA-256 (the frame stores
/// only the 32-byte digest). A payload's own `schema_version` is independent.
pub(crate) const FRAME_VERSION: u16 = 1;

/// Private wire frame. Primitives only; the public envelope types never derive
/// serde.
#[derive(Serialize, Deserialize)]
struct FrameV1 {
    frame_version: u16,
    seq: u64,
    timestamp: u64,
    schema_version: u16,
    parents: Vec<(u8, u64)>,
    prev_hash: Option<[u8; 32]>,
    payload: FramePayloadV1,
}

/// Private wire payload — mirrors [`Payload`] with primitive fields.
#[derive(Serialize, Deserialize)]
enum FramePayloadV1 {
    Plain(Vec<u8>),
    Sealed {
        key_id: String,
        nonce: [u8; 24],
        ciphertext: Vec<u8>,
    },
}

/// SHA-256 over the exact frame bytes (the hash contract).
pub(crate) fn hash_frame(bytes: &[u8]) -> RecordHash {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let out = hasher.finalize();
    let mut digest = [0u8; 32];
    digest.copy_from_slice(out.as_slice());
    RecordHash {
        algorithm: HashAlgorithm::Sha256,
        digest,
    }
}

/// Encode a record into its stored frame bytes (no length prefix).
///
/// # Errors
///
/// Returns [`PersistenceError::Encode`] if CBOR serialization fails.
pub(crate) fn encode_frame(
    record: &Record,
    seq: SequenceNo,
    prev_hash: Option<RecordHash>,
) -> Result<Vec<u8>, PersistenceError> {
    let parents = record
        .parents
        .iter()
        .map(|p| (p.stream.to_u8(), p.seq.0))
        .collect();
    let payload = match &record.payload {
        Payload::Plain(bytes) => FramePayloadV1::Plain(bytes.clone()),
        Payload::Sealed {
            key_id,
            nonce,
            ciphertext,
        } => FramePayloadV1::Sealed {
            key_id: key_id.0.clone(),
            nonce: *nonce,
            ciphertext: ciphertext.clone(),
        },
    };
    let frame = FrameV1 {
        frame_version: FRAME_VERSION,
        seq: seq.0,
        timestamp: record.timestamp,
        schema_version: record.schema_version,
        parents,
        prev_hash: prev_hash.map(|h| h.digest),
        payload,
    };
    let mut buf = Vec::new();
    ciborium::into_writer(&frame, &mut buf).map_err(|e| PersistenceError::Encode(e.to_string()))?;
    Ok(buf)
}

/// Decode stored frame bytes back into a [`StoredRecord`]. `seq` is the
/// store-known position, used for error context and cross-checked against the
/// frame's own recorded sequence.
///
/// # Errors
///
/// - [`PersistenceError::Decode`] if the bytes are not a valid `FrameV1`, carry
///   an unknown parent-stream byte, or disagree with `seq`.
/// - [`PersistenceError::SchemaVersionUnsupported`] if the frame version is not
///   readable by this build.
pub(crate) fn decode_frame(
    bytes: &[u8],
    stream: StreamId,
    seq: SequenceNo,
) -> Result<StoredRecord, PersistenceError> {
    let frame: FrameV1 = ciborium::from_reader(bytes).map_err(|e| PersistenceError::Decode {
        stream,
        seq,
        message: e.to_string(),
    })?;

    if frame.frame_version != FRAME_VERSION {
        return Err(PersistenceError::SchemaVersionUnsupported {
            found: frame.frame_version,
            max_supported: FRAME_VERSION,
        });
    }
    if frame.seq != seq.0 {
        return Err(PersistenceError::Decode {
            stream,
            seq,
            message: format!("frame seq {} does not match expected {}", frame.seq, seq.0),
        });
    }

    let mut parents = Vec::with_capacity(frame.parents.len());
    for (raw, pseq) in frame.parents {
        let pstream = StreamId::from_u8(raw).ok_or_else(|| PersistenceError::Decode {
            stream,
            seq,
            message: format!("unknown parent stream byte {raw}"),
        })?;
        parents.push(EventRef {
            stream: pstream,
            seq: SequenceNo(pseq),
        });
    }

    let payload = match frame.payload {
        FramePayloadV1::Plain(bytes) => Payload::Plain(bytes),
        FramePayloadV1::Sealed {
            key_id,
            nonce,
            ciphertext,
        } => Payload::Sealed {
            key_id: KeyId(key_id),
            nonce,
            ciphertext,
        },
    };

    let prev_hash = frame.prev_hash.map(|digest| RecordHash {
        algorithm: HashAlgorithm::Sha256,
        digest,
    });

    Ok(StoredRecord {
        seq,
        prev_hash,
        hash: hash_frame(bytes),
        record: Record {
            timestamp: frame.timestamp,
            schema_version: frame.schema_version,
            parents,
            payload,
        },
    })
}

/// Check that `record`'s back-pointer matches the actual hash of its
/// predecessor. `predecessor_hash` is `None` iff `record` is a stream genesis.
///
/// On mismatch the *predecessor* is the tampered record (its content no longer
/// hashes to what `record` recorded), so the error reports `record.seq - 1`
/// (saturating to 0 for a corrupted genesis).
///
/// # Errors
///
/// Returns [`PersistenceError::ChainBroken`] if the link does not hold.
pub(crate) fn check_back_link(
    stream: StreamId,
    record: &StoredRecord,
    predecessor_hash: Option<RecordHash>,
) -> Result<(), PersistenceError> {
    if record.prev_hash == predecessor_hash {
        Ok(())
    } else {
        Err(PersistenceError::ChainBroken {
            stream,
            seq: SequenceNo(record.seq.0.saturating_sub(1)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{FramePayloadV1, FrameV1, decode_frame, encode_frame, hash_frame};
    use crate::persistence::envelope::{EventRef, KeyId, Payload, Record, SequenceNo, StreamId};
    use crate::persistence::errors::PersistenceError;

    fn plain_record() -> Record {
        Record {
            timestamp: 42,
            schema_version: 7,
            parents: vec![EventRef {
                stream: StreamId::Topology,
                seq: SequenceNo(3),
            }],
            payload: Payload::Plain(vec![1, 2, 3, 4, 5]),
        }
    }

    #[test]
    fn plain_frame_round_trips() {
        let record = plain_record();
        let bytes = encode_frame(&record, SequenceNo(9), None).unwrap();
        let stored = decode_frame(&bytes, StreamId::Topology, SequenceNo(9)).unwrap();

        assert_eq!(stored.seq, SequenceNo(9));
        assert_eq!(stored.prev_hash, None);
        assert_eq!(stored.hash, hash_frame(&bytes));
        assert_eq!(stored.record.timestamp, 42);
        assert_eq!(stored.record.schema_version, 7);
        assert_eq!(stored.record.parents.len(), 1);
        assert_eq!(stored.record.parents[0].seq, SequenceNo(3));
        match stored.record.payload {
            Payload::Plain(b) => assert_eq!(b, vec![1, 2, 3, 4, 5]),
            Payload::Sealed { .. } => panic!("expected plain payload"),
        }
    }

    #[test]
    fn sealed_frame_round_trips() {
        let record = Record {
            timestamp: 1,
            schema_version: 1,
            parents: vec![],
            payload: Payload::Sealed {
                key_id: KeyId("subject-key".to_owned()),
                nonce: [9u8; 24],
                ciphertext: vec![0xAA, 0xBB, 0xCC],
            },
        };
        let prev = hash_frame(b"genesis-bytes");
        let bytes = encode_frame(&record, SequenceNo(1), Some(prev)).unwrap();
        let stored = decode_frame(&bytes, StreamId::Beliefs, SequenceNo(1)).unwrap();

        assert_eq!(stored.prev_hash, Some(prev));
        match stored.record.payload {
            Payload::Sealed {
                key_id,
                nonce,
                ciphertext,
            } => {
                assert_eq!(key_id, KeyId("subject-key".to_owned()));
                assert_eq!(nonce, [9u8; 24]);
                assert_eq!(ciphertext, vec![0xAA, 0xBB, 0xCC]);
            }
            Payload::Plain(_) => panic!("expected sealed payload"),
        }
    }

    #[test]
    fn unknown_parent_stream_byte_is_a_decode_error() {
        // Hand-build a frame referencing an out-of-range parent-stream byte.
        let frame = FrameV1 {
            frame_version: super::FRAME_VERSION,
            seq: 0,
            timestamp: 0,
            schema_version: 1,
            parents: vec![(99, 0)],
            prev_hash: None,
            payload: FramePayloadV1::Plain(vec![]),
        };
        let mut bytes = Vec::new();
        ciborium::into_writer(&frame, &mut bytes).unwrap();

        let err = decode_frame(&bytes, StreamId::Topology, SequenceNo(0)).unwrap_err();
        match err {
            PersistenceError::Decode { seq, .. } => assert_eq!(seq, SequenceNo(0)),
            other => panic!("expected Decode, got {other:?}"),
        }
    }

    #[test]
    fn future_frame_version_is_unsupported() {
        let frame = FrameV1 {
            frame_version: super::FRAME_VERSION + 1,
            seq: 0,
            timestamp: 0,
            schema_version: 1,
            parents: vec![],
            prev_hash: None,
            payload: FramePayloadV1::Plain(vec![]),
        };
        let mut bytes = Vec::new();
        ciborium::into_writer(&frame, &mut bytes).unwrap();

        let err = decode_frame(&bytes, StreamId::Topology, SequenceNo(0)).unwrap_err();
        match err {
            PersistenceError::SchemaVersionUnsupported {
                found,
                max_supported,
            } => {
                assert_eq!(found, super::FRAME_VERSION + 1);
                assert_eq!(max_supported, super::FRAME_VERSION);
            }
            other => panic!("expected SchemaVersionUnsupported, got {other:?}"),
        }
    }
}
