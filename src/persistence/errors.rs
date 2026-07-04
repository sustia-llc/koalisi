//! Error type for the persistence layer.
//!
//! Hand-rolled `Display` + [`std::error::Error`] + `From<std::io::Error>`, in
//! the same style as [`topology::errors`](crate::topology::errors) — no
//! `thiserror`, no `anyhow`.

use super::envelope::{KeyId, SequenceNo, StreamId};

/// Errors that can occur during persistence operations.
#[derive(Debug)]
pub enum PersistenceError {
    /// An underlying filesystem / I/O error.
    Io(std::io::Error),

    /// A frame could not be encoded (CBOR serialization or length overflow).
    Encode(String),

    /// A stored frame could not be decoded back into a record.
    Decode {
        /// Stream the bad frame belongs to.
        stream: StreamId,
        /// Sequence position of the bad frame.
        seq: SequenceNo,
        /// Human-readable detail.
        message: String,
    },

    /// The hash chain is broken: the record at `seq` no longer hashes to what
    /// its successor recorded as `prev_hash`.
    ChainBroken {
        /// Stream whose chain is broken.
        stream: StreamId,
        /// Sequence position of the record whose content mismatches.
        seq: SequenceNo,
    },

    /// A frame declared a frame-format version this build cannot read.
    SchemaVersionUnsupported {
        /// The version found on disk.
        found: u16,
        /// The highest version this build supports.
        max_supported: u16,
    },

    /// A sealed record whose key was destroyed — crypto-deleted.
    ///
    /// Reserved: raised from the P7.3 sealing layer; unused in P7.1.
    KeyUnavailable(KeyId),

    /// A federation manifest failed validation.
    ///
    /// Reserved: raised from the P7.5 federation layer; unused in P7.1.
    ManifestInvalid(String),

    /// A federation manifest has been revoked.
    ///
    /// Reserved: raised from the P7.5 federation layer; unused in P7.1.
    ManifestRevoked(String),

    /// An operation requiring records (e.g. `verify`) ran against an empty
    /// stream.
    StreamEmpty(StreamId),

    /// The stream is wedged after a failed write.
    ///
    /// A write-path I/O error can leave the physical segment file grown (the
    /// append-mode fd / a partial `BufWriter` flush) while the in-memory
    /// offsets, `active_len`, and `next_seq` stayed at their pre-write values —
    /// a desync that would misalign every subsequent frame. To avoid silently
    /// corrupting the on-disk chain, the stream refuses further appends until
    /// the store is reopened. Reopening re-scans the segments and truncates the
    /// torn tail, recovering to the last complete frame.
    StreamWedged(StreamId),
}

impl std::fmt::Display for PersistenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Encode(m) => write!(f, "frame encode error: {m}"),
            Self::Decode {
                stream,
                seq,
                message,
            } => write!(
                f,
                "frame decode error in {stream:?} at seq {}: {message}",
                seq.0
            ),
            Self::ChainBroken { stream, seq } => {
                write!(f, "hash chain broken in {stream:?} at seq {}", seq.0)
            }
            Self::SchemaVersionUnsupported {
                found,
                max_supported,
            } => write!(
                f,
                "unsupported frame version {found} (max supported {max_supported})"
            ),
            Self::KeyUnavailable(k) => {
                write!(f, "encryption key unavailable (crypto-deleted): {}", k.0)
            }
            Self::ManifestInvalid(m) => write!(f, "invalid federation manifest: {m}"),
            Self::ManifestRevoked(m) => write!(f, "revoked federation manifest: {m}"),
            Self::StreamEmpty(stream) => write!(f, "stream {stream:?} is empty"),
            Self::StreamWedged(stream) => write!(
                f,
                "stream {stream:?} is wedged after a failed write; reopen the store to recover"
            ),
        }
    }
}

impl std::error::Error for PersistenceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for PersistenceError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
