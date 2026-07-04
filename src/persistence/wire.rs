//! Versioned wire projection of [`TemporalEvent`] for the `Topology` stream
//! (Phase 7 / P7.2, sustia-llc/koalisi#30).
//!
//! Per the §4 "wire projection, not serde on domain types" decision,
//! [`TemporalEvent`] itself does NOT gain serde derives. Instead this module
//! carries [`WireTopologyEvent`], a 13-variant serde-derived mirror whose fields
//! are primitives: indices and timestamps become raw `u64`, `Vec<VertexIndex>`
//! becomes `Vec<u64>`, and the `V`/`HE` weights become caller-chosen
//! serializable projections `VW`/`HW`. The wire schema is frozen and versioned
//! by [`WIRE_TOPOLOGY_SCHEMA_VERSION`], independently of the in-memory enum.
//!
//! Persisting raw `VertexIndex`/`HyperedgeIndex` values is legitimate: catgraph
//! indices are stable and never reused (CLAUDE.md gotcha 12), and the in-memory
//! event log already depends on this for replay.

use std::fmt::{Debug, Display};

use serde::{Deserialize, Serialize};

use crate::topology::{HyperedgeIndex, SnapshotId, TemporalEvent, Timestamp, VertexIndex};

/// Wire-schema version of the `Topology` stream payload. Bump on any change to
/// [`WireTopologyEvent`]'s shape; replay rejects a record whose recorded
/// `schema_version` exceeds this.
pub const WIRE_TOPOLOGY_SCHEMA_VERSION: u16 = 1;

/// A weight projection failed to convert back to its in-memory type during
/// [`WireTopologyEvent::try_into_event`].
///
/// Deliberately lightweight and seq-free:
/// [`replay_into_event_log`](super::replay_into_event_log) wraps it into
/// [`PersistenceError::Decode`](super::PersistenceError::Decode) with the
/// offending record's sequence number, which this layer does not know.
#[derive(Debug)]
pub struct WireConversionError {
    /// Human-readable detail (which field, and the underlying converter's error).
    pub message: String,
}

impl Display for WireConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "wire topology conversion failed: {}", self.message)
    }
}

impl std::error::Error for WireConversionError {}

/// Serde-derived wire mirror of [`TemporalEvent`], generic over serializable
/// weight projections `VW` (vertex) and `HW` (hyperedge).
///
/// The identity projection (`VW = V`, `HW = HE` where the weight type is itself
/// `Serialize + DeserializeOwned`) round-trips through the blanket
/// `From<T> for T` / `TryFrom<T> for T` impls; see
/// [`from_event`](Self::from_event) and [`try_into_event`](Self::try_into_event).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum WireTopologyEvent<VW, HW> {
    /// Mirror of [`TemporalEvent::VertexAdded`].
    VertexAdded {
        /// Logical-clock value (`Timestamp`'s raw `u64`).
        timestamp: u64,
        /// Vertex index (`VertexIndex`'s raw `u64`).
        index: u64,
        /// Projected vertex weight.
        weight: VW,
    },
    /// Mirror of [`TemporalEvent::VertexRemoved`].
    VertexRemoved {
        /// Logical-clock value.
        timestamp: u64,
        /// Vertex index.
        index: u64,
        /// Projected vertex weight at removal.
        weight: VW,
    },
    /// Mirror of [`TemporalEvent::VertexWeightUpdated`].
    VertexWeightUpdated {
        /// Logical-clock value.
        timestamp: u64,
        /// Vertex index.
        index: u64,
        /// Projected previous weight.
        old_weight: VW,
        /// Projected new weight.
        new_weight: VW,
    },
    /// Mirror of [`TemporalEvent::HyperedgeAdded`].
    HyperedgeAdded {
        /// Logical-clock value.
        timestamp: u64,
        /// Hyperedge index (`HyperedgeIndex`'s raw `u64`).
        index: u64,
        /// Member vertex indices.
        vertices: Vec<u64>,
        /// Projected hyperedge weight.
        weight: HW,
    },
    /// Mirror of [`TemporalEvent::HyperedgeRemoved`].
    HyperedgeRemoved {
        /// Logical-clock value.
        timestamp: u64,
        /// Hyperedge index.
        index: u64,
        /// Member vertex indices at removal.
        vertices: Vec<u64>,
        /// Projected hyperedge weight at removal.
        weight: HW,
    },
    /// Mirror of [`TemporalEvent::HyperedgeWeightUpdated`].
    HyperedgeWeightUpdated {
        /// Logical-clock value.
        timestamp: u64,
        /// Hyperedge index.
        index: u64,
        /// Projected previous weight.
        old_weight: HW,
        /// Projected new weight.
        new_weight: HW,
    },
    /// Mirror of [`TemporalEvent::HyperedgeVerticesUpdated`].
    HyperedgeVerticesUpdated {
        /// Logical-clock value.
        timestamp: u64,
        /// Hyperedge index.
        index: u64,
        /// Previous member vertex indices.
        old_vertices: Vec<u64>,
        /// New member vertex indices.
        new_vertices: Vec<u64>,
    },
    /// Mirror of [`TemporalEvent::HyperedgeReversed`].
    HyperedgeReversed {
        /// Logical-clock value.
        timestamp: u64,
        /// Hyperedge index.
        index: u64,
        /// Previous member vertex indices.
        old_vertices: Vec<u64>,
        /// Reversed member vertex indices.
        new_vertices: Vec<u64>,
    },
    /// Mirror of [`TemporalEvent::HyperedgesJoined`].
    HyperedgesJoined {
        /// Logical-clock value.
        timestamp: u64,
        /// Index of the hyperedge absorbing the sources.
        target_index: u64,
        /// Indices of the absorbed (now-removed) source hyperedges.
        source_indices: Vec<u64>,
        /// Member vertex indices after the join.
        new_vertices: Vec<u64>,
    },
    /// Mirror of [`TemporalEvent::VerticesContracted`].
    VerticesContracted {
        /// Logical-clock value.
        timestamp: u64,
        /// Hyperedge whose members were contracted.
        hyperedge_index: u64,
        /// Contracted vertex indices.
        contracted_vertices: Vec<u64>,
        /// Vertex the contraction merged into.
        target_vertex: u64,
        /// Previous member vertex indices.
        old_vertices: Vec<u64>,
        /// New member vertex indices.
        new_vertices: Vec<u64>,
    },
    /// Mirror of [`TemporalEvent::HyperedgesCleared`].
    HyperedgesCleared {
        /// Logical-clock value.
        timestamp: u64,
        /// Number of hyperedges cleared.
        count: u64,
    },
    /// Mirror of [`TemporalEvent::GraphCleared`].
    GraphCleared {
        /// Logical-clock value.
        timestamp: u64,
        /// Number of vertices cleared.
        vertex_count: u64,
        /// Number of hyperedges cleared.
        hyperedge_count: u64,
    },
    /// Mirror of [`TemporalEvent::SnapshotMarker`].
    SnapshotMarker {
        /// Logical-clock value.
        timestamp: u64,
        /// Snapshot identifier (`SnapshotId`'s raw `u64`).
        snapshot_id: u64,
    },
}

/// `VertexIndex` → raw `u64`. `usize::from` then widen (lossless: `usize` is
/// never wider than 64 bits on any supported target).
#[allow(clippy::cast_possible_truncation)]
fn v_u64(index: VertexIndex) -> u64 {
    usize::from(index) as u64
}

/// `HyperedgeIndex` → raw `u64` (see [`v_u64`]).
#[allow(clippy::cast_possible_truncation)]
fn he_u64(index: HyperedgeIndex) -> u64 {
    usize::from(index) as u64
}

/// `usize` → raw `u64` for count fields (see [`v_u64`]).
#[allow(clippy::cast_possible_truncation)]
fn usize_u64(value: usize) -> u64 {
    value as u64
}

fn v_vec(indices: &[VertexIndex]) -> Vec<u64> {
    indices.iter().copied().map(v_u64).collect()
}

fn he_vec(indices: &[HyperedgeIndex]) -> Vec<u64> {
    indices.iter().copied().map(he_u64).collect()
}

/// Raw `u64` → `VertexIndex`, failing only on a 32-bit target when the stored
/// value exceeds `usize::MAX`.
fn vertex_from_u64(value: u64) -> Result<VertexIndex, WireConversionError> {
    usize::try_from(value)
        .map(VertexIndex::from)
        .map_err(|_| WireConversionError {
            message: format!("vertex index {value} exceeds usize on this target"),
        })
}

/// Raw `u64` → `HyperedgeIndex` (see [`vertex_from_u64`]).
fn hyperedge_from_u64(value: u64) -> Result<HyperedgeIndex, WireConversionError> {
    usize::try_from(value)
        .map(HyperedgeIndex::from)
        .map_err(|_| WireConversionError {
            message: format!("hyperedge index {value} exceeds usize on this target"),
        })
}

/// Raw `u64` → `usize` for count fields (see [`vertex_from_u64`]).
fn usize_from_u64(value: u64) -> Result<usize, WireConversionError> {
    usize::try_from(value).map_err(|_| WireConversionError {
        message: format!("count {value} exceeds usize on this target"),
    })
}

fn vertices_from_u64(values: Vec<u64>) -> Result<Vec<VertexIndex>, WireConversionError> {
    values.into_iter().map(vertex_from_u64).collect()
}

fn hyperedges_from_u64(values: Vec<u64>) -> Result<Vec<HyperedgeIndex>, WireConversionError> {
    values.into_iter().map(hyperedge_from_u64).collect()
}

impl<VW, HW> WireTopologyEvent<VW, HW> {
    /// Project an in-memory [`TemporalEvent`] onto its wire mirror.
    ///
    /// Indices and timestamps widen to `u64`; the `V`/`HE` weights convert
    /// through `Into<VW>`/`Into<HW>`. The identity case (`VW = V`, `HW = HE`)
    /// uses the blanket `From<T> for T`.
    #[must_use]
    #[allow(clippy::too_many_lines)] // one arm per TemporalEvent variant (13)
    pub fn from_event<V, HE>(event: &TemporalEvent<V, HE>) -> Self
    where
        V: Clone + Debug + Into<VW>,
        HE: Clone + Debug + Into<HW>,
    {
        match event {
            TemporalEvent::VertexAdded {
                timestamp,
                index,
                weight,
            } => WireTopologyEvent::VertexAdded {
                timestamp: timestamp.value(),
                index: v_u64(*index),
                weight: weight.clone().into(),
            },
            TemporalEvent::VertexRemoved {
                timestamp,
                index,
                weight,
            } => WireTopologyEvent::VertexRemoved {
                timestamp: timestamp.value(),
                index: v_u64(*index),
                weight: weight.clone().into(),
            },
            TemporalEvent::VertexWeightUpdated {
                timestamp,
                index,
                old_weight,
                new_weight,
            } => WireTopologyEvent::VertexWeightUpdated {
                timestamp: timestamp.value(),
                index: v_u64(*index),
                old_weight: old_weight.clone().into(),
                new_weight: new_weight.clone().into(),
            },
            TemporalEvent::HyperedgeAdded {
                timestamp,
                index,
                vertices,
                weight,
            } => WireTopologyEvent::HyperedgeAdded {
                timestamp: timestamp.value(),
                index: he_u64(*index),
                vertices: v_vec(vertices),
                weight: weight.clone().into(),
            },
            TemporalEvent::HyperedgeRemoved {
                timestamp,
                index,
                vertices,
                weight,
            } => WireTopologyEvent::HyperedgeRemoved {
                timestamp: timestamp.value(),
                index: he_u64(*index),
                vertices: v_vec(vertices),
                weight: weight.clone().into(),
            },
            TemporalEvent::HyperedgeWeightUpdated {
                timestamp,
                index,
                old_weight,
                new_weight,
            } => WireTopologyEvent::HyperedgeWeightUpdated {
                timestamp: timestamp.value(),
                index: he_u64(*index),
                old_weight: old_weight.clone().into(),
                new_weight: new_weight.clone().into(),
            },
            TemporalEvent::HyperedgeVerticesUpdated {
                timestamp,
                index,
                old_vertices,
                new_vertices,
            } => WireTopologyEvent::HyperedgeVerticesUpdated {
                timestamp: timestamp.value(),
                index: he_u64(*index),
                old_vertices: v_vec(old_vertices),
                new_vertices: v_vec(new_vertices),
            },
            TemporalEvent::HyperedgeReversed {
                timestamp,
                index,
                old_vertices,
                new_vertices,
            } => WireTopologyEvent::HyperedgeReversed {
                timestamp: timestamp.value(),
                index: he_u64(*index),
                old_vertices: v_vec(old_vertices),
                new_vertices: v_vec(new_vertices),
            },
            TemporalEvent::HyperedgesJoined {
                timestamp,
                target_index,
                source_indices,
                new_vertices,
            } => WireTopologyEvent::HyperedgesJoined {
                timestamp: timestamp.value(),
                target_index: he_u64(*target_index),
                source_indices: he_vec(source_indices),
                new_vertices: v_vec(new_vertices),
            },
            TemporalEvent::VerticesContracted {
                timestamp,
                hyperedge_index,
                contracted_vertices,
                target_vertex,
                old_vertices,
                new_vertices,
            } => WireTopologyEvent::VerticesContracted {
                timestamp: timestamp.value(),
                hyperedge_index: he_u64(*hyperedge_index),
                contracted_vertices: v_vec(contracted_vertices),
                target_vertex: v_u64(*target_vertex),
                old_vertices: v_vec(old_vertices),
                new_vertices: v_vec(new_vertices),
            },
            TemporalEvent::HyperedgesCleared { timestamp, count } => {
                WireTopologyEvent::HyperedgesCleared {
                    timestamp: timestamp.value(),
                    count: usize_u64(*count),
                }
            }
            TemporalEvent::GraphCleared {
                timestamp,
                vertex_count,
                hyperedge_count,
            } => WireTopologyEvent::GraphCleared {
                timestamp: timestamp.value(),
                vertex_count: usize_u64(*vertex_count),
                hyperedge_count: usize_u64(*hyperedge_count),
            },
            TemporalEvent::SnapshotMarker {
                timestamp,
                snapshot_id,
            } => WireTopologyEvent::SnapshotMarker {
                timestamp: timestamp.value(),
                // `SnapshotId`'s inner `u64` is `pub(crate)`; this projection is
                // in-crate, so read it directly (no public accessor exists).
                snapshot_id: snapshot_id.0,
            },
        }
    }

    /// Reconstruct an in-memory [`TemporalEvent`] from this wire mirror.
    ///
    /// Timestamps and indices narrow from `u64`; the weight projections convert
    /// back through `TryInto<V>`/`TryInto<HE>`. A conversion failure (a weight
    /// projection or, on a 32-bit target, an out-of-range index) yields a
    /// [`WireConversionError`]; [`replay_into_event_log`](super::replay_into_event_log)
    /// wraps it with the record's sequence number. The identity case
    /// (`VW = V`, `HW = HE`) uses
    /// the blanket `TryFrom<T> for T` (whose `Infallible` error implements
    /// `Display`).
    ///
    /// # Errors
    ///
    /// Returns a [`WireConversionError`] if a weight projection fails to convert
    /// back, or (on a 32-bit target) if a stored `u64` index/count exceeds
    /// `usize`.
    #[allow(clippy::too_many_lines)] // one arm per TemporalEvent variant (13)
    pub fn try_into_event<V, HE>(self) -> Result<TemporalEvent<V, HE>, WireConversionError>
    where
        V: Clone + Debug,
        HE: Clone + Debug,
        VW: TryInto<V>,
        <VW as TryInto<V>>::Error: Display,
        HW: TryInto<HE>,
        <HW as TryInto<HE>>::Error: Display,
    {
        let v_weight = |w: VW| -> Result<V, WireConversionError> {
            w.try_into().map_err(|e| WireConversionError {
                message: format!("vertex weight: {e}"),
            })
        };
        let he_weight = |w: HW| -> Result<HE, WireConversionError> {
            w.try_into().map_err(|e| WireConversionError {
                message: format!("hyperedge weight: {e}"),
            })
        };

        Ok(match self {
            WireTopologyEvent::VertexAdded {
                timestamp,
                index,
                weight,
            } => TemporalEvent::VertexAdded {
                timestamp: Timestamp::new(timestamp),
                index: vertex_from_u64(index)?,
                weight: v_weight(weight)?,
            },
            WireTopologyEvent::VertexRemoved {
                timestamp,
                index,
                weight,
            } => TemporalEvent::VertexRemoved {
                timestamp: Timestamp::new(timestamp),
                index: vertex_from_u64(index)?,
                weight: v_weight(weight)?,
            },
            WireTopologyEvent::VertexWeightUpdated {
                timestamp,
                index,
                old_weight,
                new_weight,
            } => TemporalEvent::VertexWeightUpdated {
                timestamp: Timestamp::new(timestamp),
                index: vertex_from_u64(index)?,
                old_weight: v_weight(old_weight)?,
                new_weight: v_weight(new_weight)?,
            },
            WireTopologyEvent::HyperedgeAdded {
                timestamp,
                index,
                vertices,
                weight,
            } => TemporalEvent::HyperedgeAdded {
                timestamp: Timestamp::new(timestamp),
                index: hyperedge_from_u64(index)?,
                vertices: vertices_from_u64(vertices)?,
                weight: he_weight(weight)?,
            },
            WireTopologyEvent::HyperedgeRemoved {
                timestamp,
                index,
                vertices,
                weight,
            } => TemporalEvent::HyperedgeRemoved {
                timestamp: Timestamp::new(timestamp),
                index: hyperedge_from_u64(index)?,
                vertices: vertices_from_u64(vertices)?,
                weight: he_weight(weight)?,
            },
            WireTopologyEvent::HyperedgeWeightUpdated {
                timestamp,
                index,
                old_weight,
                new_weight,
            } => TemporalEvent::HyperedgeWeightUpdated {
                timestamp: Timestamp::new(timestamp),
                index: hyperedge_from_u64(index)?,
                old_weight: he_weight(old_weight)?,
                new_weight: he_weight(new_weight)?,
            },
            WireTopologyEvent::HyperedgeVerticesUpdated {
                timestamp,
                index,
                old_vertices,
                new_vertices,
            } => TemporalEvent::HyperedgeVerticesUpdated {
                timestamp: Timestamp::new(timestamp),
                index: hyperedge_from_u64(index)?,
                old_vertices: vertices_from_u64(old_vertices)?,
                new_vertices: vertices_from_u64(new_vertices)?,
            },
            WireTopologyEvent::HyperedgeReversed {
                timestamp,
                index,
                old_vertices,
                new_vertices,
            } => TemporalEvent::HyperedgeReversed {
                timestamp: Timestamp::new(timestamp),
                index: hyperedge_from_u64(index)?,
                old_vertices: vertices_from_u64(old_vertices)?,
                new_vertices: vertices_from_u64(new_vertices)?,
            },
            WireTopologyEvent::HyperedgesJoined {
                timestamp,
                target_index,
                source_indices,
                new_vertices,
            } => TemporalEvent::HyperedgesJoined {
                timestamp: Timestamp::new(timestamp),
                target_index: hyperedge_from_u64(target_index)?,
                source_indices: hyperedges_from_u64(source_indices)?,
                new_vertices: vertices_from_u64(new_vertices)?,
            },
            WireTopologyEvent::VerticesContracted {
                timestamp,
                hyperedge_index,
                contracted_vertices,
                target_vertex,
                old_vertices,
                new_vertices,
            } => TemporalEvent::VerticesContracted {
                timestamp: Timestamp::new(timestamp),
                hyperedge_index: hyperedge_from_u64(hyperedge_index)?,
                contracted_vertices: vertices_from_u64(contracted_vertices)?,
                target_vertex: vertex_from_u64(target_vertex)?,
                old_vertices: vertices_from_u64(old_vertices)?,
                new_vertices: vertices_from_u64(new_vertices)?,
            },
            WireTopologyEvent::HyperedgesCleared { timestamp, count } => {
                TemporalEvent::HyperedgesCleared {
                    timestamp: Timestamp::new(timestamp),
                    count: usize_from_u64(count)?,
                }
            }
            WireTopologyEvent::GraphCleared {
                timestamp,
                vertex_count,
                hyperedge_count,
            } => TemporalEvent::GraphCleared {
                timestamp: Timestamp::new(timestamp),
                vertex_count: usize_from_u64(vertex_count)?,
                hyperedge_count: usize_from_u64(hyperedge_count)?,
            },
            WireTopologyEvent::SnapshotMarker {
                timestamp,
                snapshot_id,
            } => TemporalEvent::SnapshotMarker {
                timestamp: Timestamp::new(timestamp),
                snapshot_id: SnapshotId::new(snapshot_id),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny serde weight fixture that is its own identity projection.
    #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
    struct W(u32);

    /// Every wire variant with boundary `u64` values in the index/timestamp/count
    /// fields, for the round-trip sweeps below.
    fn all_variants() -> Vec<WireTopologyEvent<W, W>> {
        vec![
            WireTopologyEvent::VertexAdded {
                timestamp: 0,
                index: u64::MAX,
                weight: W(7),
            },
            WireTopologyEvent::VertexRemoved {
                timestamp: u64::MAX,
                index: 0,
                weight: W(9),
            },
            WireTopologyEvent::VertexWeightUpdated {
                timestamp: 1,
                index: 2,
                old_weight: W(1),
                new_weight: W(2),
            },
            WireTopologyEvent::HyperedgeAdded {
                timestamp: 3,
                index: 4,
                vertices: vec![0, u64::MAX, 5],
                weight: W(3),
            },
            WireTopologyEvent::HyperedgeRemoved {
                timestamp: 6,
                index: 7,
                vertices: vec![8, 9],
                weight: W(4),
            },
            WireTopologyEvent::HyperedgeWeightUpdated {
                timestamp: 10,
                index: 11,
                old_weight: W(5),
                new_weight: W(6),
            },
            WireTopologyEvent::HyperedgeVerticesUpdated {
                timestamp: 12,
                index: 13,
                old_vertices: vec![14],
                new_vertices: vec![14, 15],
            },
            WireTopologyEvent::HyperedgeReversed {
                timestamp: 16,
                index: 17,
                old_vertices: vec![18, 19],
                new_vertices: vec![19, 18],
            },
            WireTopologyEvent::HyperedgesJoined {
                timestamp: 20,
                target_index: 21,
                source_indices: vec![22, 23],
                new_vertices: vec![24, 25, 26],
            },
            WireTopologyEvent::VerticesContracted {
                timestamp: 27,
                hyperedge_index: 28,
                contracted_vertices: vec![29, 30],
                target_vertex: 29,
                old_vertices: vec![29, 30, 31],
                new_vertices: vec![29, 31],
            },
            WireTopologyEvent::HyperedgesCleared {
                timestamp: 32,
                count: u64::MAX,
            },
            WireTopologyEvent::GraphCleared {
                timestamp: 33,
                vertex_count: 34,
                hyperedge_count: u64::MAX,
            },
            WireTopologyEvent::SnapshotMarker {
                timestamp: 35,
                snapshot_id: u64::MAX,
            },
        ]
    }

    /// CBOR round-trip is identity for every variant, including boundary values.
    #[test]
    fn cbor_round_trip_all_variants() {
        for wire in all_variants() {
            let mut buf = Vec::new();
            ciborium::into_writer(&wire, &mut buf).unwrap();
            let back: WireTopologyEvent<W, W> = ciborium::from_reader(buf.as_slice()).unwrap();
            assert_eq!(wire, back);
        }
    }

    /// `try_into_event` then `from_event` is identity for every variant under the
    /// identity weight projection. Boundary indices below `usize::MAX` on the
    /// 64-bit test target survive the `u64`↔`usize` narrowing.
    #[test]
    fn conversion_round_trip_identity_projection() {
        for wire in all_variants() {
            let event: TemporalEvent<W, W> = wire.clone().try_into_event().unwrap();
            let back = WireTopologyEvent::<W, W>::from_event(&event);
            assert_eq!(wire, back);
        }
    }
}
