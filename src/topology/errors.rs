//! Error types for temporal hypergraph operations.

use super::events::SnapshotId;
use super::timestamp::Timestamp;
use catgraph_applied::HypergraphError;

/// Errors that can occur during temporal hypergraph operations.
///
/// K1 (sustia-llc/koalisi#4): the underlying [`HypergraphError`] is now
/// non-generic, so this type no longer carries the `<V, HE>` weight parameters
/// either — every variant is weight-independent.
#[derive(Debug)]
pub enum TemporalError {
    /// An error from the underlying hypergraph.
    Hypergraph(HypergraphError),

    /// The requested timestamp is in the future.
    TimestampInFuture {
        requested: Timestamp,
        current: Timestamp,
    },

    /// No snapshot exists at or before the requested timestamp.
    NoSnapshotBefore(Timestamp),

    /// The requested snapshot was not found.
    SnapshotNotFound(SnapshotId),

    /// Cannot replay events: inconsistent state.
    ReplayError {
        event_index: usize,
        message: String,
    },

    /// The time range is invalid (start > end).
    InvalidTimeRange {
        start: Timestamp,
        end: Timestamp,
    },

    /// Event log is corrupted or inconsistent.
    EventLogCorrupted(String),

    /// Concurrent modification detected.
    ConcurrentModification,

    /// Cannot leave a coalition that would become empty.
    EmptyCoalition,
}

impl std::fmt::Display for TemporalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Hypergraph(e) => write!(f, "Hypergraph error: {}", e),
            Self::TimestampInFuture { requested, current } => {
                write!(
                    f,
                    "Requested timestamp {} is in the future (current: {})",
                    requested, current
                )
            }
            Self::NoSnapshotBefore(ts) => {
                write!(f, "No snapshot exists at or before timestamp {}", ts)
            }
            Self::SnapshotNotFound(id) => write!(f, "Snapshot not found: {}", id),
            Self::ReplayError {
                event_index,
                message,
            } => {
                write!(f, "Error replaying event {}: {}", event_index, message)
            }
            Self::InvalidTimeRange { start, end } => {
                write!(
                    f,
                    "Invalid time range: start ({}) > end ({})",
                    start, end
                )
            }
            Self::EventLogCorrupted(msg) => write!(f, "Event log corrupted: {}", msg),
            Self::ConcurrentModification => write!(f, "Concurrent modification detected"),
            Self::EmptyCoalition => {
                write!(f, "Cannot leave coalition: would become empty (use dissolve instead)")
            }
        }
    }
}

impl std::error::Error for TemporalError {}

impl From<HypergraphError> for TemporalError {
    fn from(e: HypergraphError) -> Self {
        Self::Hypergraph(e)
    }
}

/// Result type for temporal operations.
pub type TemporalResult<T> = Result<T, TemporalError>;
