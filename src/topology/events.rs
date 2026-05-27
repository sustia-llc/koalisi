//! Event types for the temporal hypergraph event log.

use super::timestamp::Timestamp;
use hypergraph::{HyperedgeIndex, VertexIndex};
use std::fmt::Debug;

/// A unique identifier for a snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct SnapshotId(pub(crate) u64);

impl SnapshotId {
    /// Create a new snapshot ID.
    pub const fn new(id: u64) -> Self {
        Self(id)
    }
}

impl std::fmt::Display for SnapshotId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Snapshot#{}", self.0)
    }
}

/// Events that can occur in a temporal hypergraph.
///
/// Each event captures a mutation to the graph along with its timestamp.
/// Events are stored immutably and can be replayed to reconstruct past states.
#[derive(Clone, Debug)]
pub enum TemporalEvent<V, HE>
where
    V: Clone + Debug,
    HE: Clone + Debug,
{
    /// A vertex was added to the graph.
    VertexAdded {
        timestamp: Timestamp,
        index: VertexIndex,
        weight: V,
    },

    /// A vertex was removed from the graph.
    VertexRemoved {
        timestamp: Timestamp,
        index: VertexIndex,
        weight: V,
    },

    /// A vertex's weight was updated.
    VertexWeightUpdated {
        timestamp: Timestamp,
        index: VertexIndex,
        old_weight: V,
        new_weight: V,
    },

    /// A hyperedge was added to the graph.
    HyperedgeAdded {
        timestamp: Timestamp,
        index: HyperedgeIndex,
        vertices: Vec<VertexIndex>,
        weight: HE,
    },

    /// A hyperedge was removed from the graph.
    HyperedgeRemoved {
        timestamp: Timestamp,
        index: HyperedgeIndex,
        vertices: Vec<VertexIndex>,
        weight: HE,
    },

    /// A hyperedge's weight was updated.
    HyperedgeWeightUpdated {
        timestamp: Timestamp,
        index: HyperedgeIndex,
        old_weight: HE,
        new_weight: HE,
    },

    /// A hyperedge's vertices were updated.
    HyperedgeVerticesUpdated {
        timestamp: Timestamp,
        index: HyperedgeIndex,
        old_vertices: Vec<VertexIndex>,
        new_vertices: Vec<VertexIndex>,
    },

    /// A hyperedge was reversed.
    HyperedgeReversed {
        timestamp: Timestamp,
        index: HyperedgeIndex,
        old_vertices: Vec<VertexIndex>,
        new_vertices: Vec<VertexIndex>,
    },

    /// Multiple hyperedges were joined into one.
    HyperedgesJoined {
        timestamp: Timestamp,
        target_index: HyperedgeIndex,
        source_indices: Vec<HyperedgeIndex>,
        new_vertices: Vec<VertexIndex>,
    },

    /// Vertices were contracted within a hyperedge.
    VerticesContracted {
        timestamp: Timestamp,
        hyperedge_index: HyperedgeIndex,
        contracted_vertices: Vec<VertexIndex>,
        target_vertex: VertexIndex,
        old_vertices: Vec<VertexIndex>,
        new_vertices: Vec<VertexIndex>,
    },

    /// All hyperedges were cleared from the graph.
    HyperedgesCleared {
        timestamp: Timestamp,
        count: usize,
    },

    /// The entire graph was cleared.
    GraphCleared {
        timestamp: Timestamp,
        vertex_count: usize,
        hyperedge_count: usize,
    },

    /// A snapshot marker (for indexing purposes).
    SnapshotMarker {
        timestamp: Timestamp,
        snapshot_id: SnapshotId,
    },
}

impl<V, HE> TemporalEvent<V, HE>
where
    V: Clone + Debug,
    HE: Clone + Debug,
{
    /// Get the timestamp when this event occurred.
    pub fn timestamp(&self) -> Timestamp {
        match self {
            Self::VertexAdded { timestamp, .. } => *timestamp,
            Self::VertexRemoved { timestamp, .. } => *timestamp,
            Self::VertexWeightUpdated { timestamp, .. } => *timestamp,
            Self::HyperedgeAdded { timestamp, .. } => *timestamp,
            Self::HyperedgeRemoved { timestamp, .. } => *timestamp,
            Self::HyperedgeWeightUpdated { timestamp, .. } => *timestamp,
            Self::HyperedgeVerticesUpdated { timestamp, .. } => *timestamp,
            Self::HyperedgeReversed { timestamp, .. } => *timestamp,
            Self::HyperedgesJoined { timestamp, .. } => *timestamp,
            Self::VerticesContracted { timestamp, .. } => *timestamp,
            Self::HyperedgesCleared { timestamp, .. } => *timestamp,
            Self::GraphCleared { timestamp, .. } => *timestamp,
            Self::SnapshotMarker { timestamp, .. } => *timestamp,
        }
    }

    /// Get the vertex index affected by this event, if any.
    pub fn vertex_index(&self) -> Option<VertexIndex> {
        match self {
            Self::VertexAdded { index, .. } => Some(*index),
            Self::VertexRemoved { index, .. } => Some(*index),
            Self::VertexWeightUpdated { index, .. } => Some(*index),
            Self::VerticesContracted { target_vertex, .. } => Some(*target_vertex),
            _ => None,
        }
    }

    /// Get the hyperedge index affected by this event, if any.
    pub fn hyperedge_index(&self) -> Option<HyperedgeIndex> {
        match self {
            Self::HyperedgeAdded { index, .. } => Some(*index),
            Self::HyperedgeRemoved { index, .. } => Some(*index),
            Self::HyperedgeWeightUpdated { index, .. } => Some(*index),
            Self::HyperedgeVerticesUpdated { index, .. } => Some(*index),
            Self::HyperedgeReversed { index, .. } => Some(*index),
            Self::HyperedgesJoined { target_index, .. } => Some(*target_index),
            Self::VerticesContracted {
                hyperedge_index, ..
            } => Some(*hyperedge_index),
            _ => None,
        }
    }

    /// Check if this is a mutation event (not a marker).
    pub fn is_mutation(&self) -> bool {
        !matches!(self, Self::SnapshotMarker { .. })
    }

    /// Get an event type name for display/debugging.
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::VertexAdded { .. } => "VertexAdded",
            Self::VertexRemoved { .. } => "VertexRemoved",
            Self::VertexWeightUpdated { .. } => "VertexWeightUpdated",
            Self::HyperedgeAdded { .. } => "HyperedgeAdded",
            Self::HyperedgeRemoved { .. } => "HyperedgeRemoved",
            Self::HyperedgeWeightUpdated { .. } => "HyperedgeWeightUpdated",
            Self::HyperedgeVerticesUpdated { .. } => "HyperedgeVerticesUpdated",
            Self::HyperedgeReversed { .. } => "HyperedgeReversed",
            Self::HyperedgesJoined { .. } => "HyperedgesJoined",
            Self::VerticesContracted { .. } => "VerticesContracted",
            Self::HyperedgesCleared { .. } => "HyperedgesCleared",
            Self::GraphCleared { .. } => "GraphCleared",
            Self::SnapshotMarker { .. } => "SnapshotMarker",
        }
    }
}

/// Summary statistics about events.
#[derive(Clone, Debug, Default)]
pub struct EventStats {
    pub vertex_added: usize,
    pub vertex_removed: usize,
    pub vertex_updated: usize,
    pub hyperedge_added: usize,
    pub hyperedge_removed: usize,
    pub hyperedge_updated: usize,
    pub transforms: usize,
    pub clears: usize,
    pub snapshots: usize,
}

impl EventStats {
    /// Count an event.
    pub fn count<V, HE>(&mut self, event: &TemporalEvent<V, HE>)
    where
        V: Clone + Debug,
        HE: Clone + Debug,
    {
        match event {
            TemporalEvent::VertexAdded { .. } => self.vertex_added += 1,
            TemporalEvent::VertexRemoved { .. } => self.vertex_removed += 1,
            TemporalEvent::VertexWeightUpdated { .. } => self.vertex_updated += 1,
            TemporalEvent::HyperedgeAdded { .. } => self.hyperedge_added += 1,
            TemporalEvent::HyperedgeRemoved { .. } => self.hyperedge_removed += 1,
            TemporalEvent::HyperedgeWeightUpdated { .. } => self.hyperedge_updated += 1,
            TemporalEvent::HyperedgeVerticesUpdated { .. } => self.hyperedge_updated += 1,
            TemporalEvent::HyperedgeReversed { .. } => self.transforms += 1,
            TemporalEvent::HyperedgesJoined { .. } => self.transforms += 1,
            TemporalEvent::VerticesContracted { .. } => self.transforms += 1,
            TemporalEvent::HyperedgesCleared { .. } => self.clears += 1,
            TemporalEvent::GraphCleared { .. } => self.clears += 1,
            TemporalEvent::SnapshotMarker { .. } => self.snapshots += 1,
        }
    }

    /// Total number of events.
    pub fn total(&self) -> usize {
        self.vertex_added
            + self.vertex_removed
            + self.vertex_updated
            + self.hyperedge_added
            + self.hyperedge_removed
            + self.hyperedge_updated
            + self.transforms
            + self.clears
            + self.snapshots
    }
}
