//! Temporal hypergraph topology layer.
//!
//! Event-sourced hypergraph with time-travel queries, coalition management,
//! and analytics. Built on [`catgraph_applied::Hypergraph`] (the K1 backend,
//! sustia-llc/koalisi#4), which re-backs this layer's CRUD hypergraph container.

mod analytics;
mod coalitions;
mod errors;
mod event_log;
mod events;
mod queries;
mod temporal;
mod timestamp;

#[cfg(feature = "magnitude")]
pub use analytics::MagnitudePoint;
pub use analytics::{GraphDelta, TemporalAnalytics};
pub use coalitions::CoalitionManager;
pub use errors::{TemporalError, TemporalResult};
pub use event_log::EventLog;
pub use events::{EventStats, SnapshotId, TemporalEvent};
pub use queries::TemporalQueries;
pub use temporal::{SharedGraph, Snapshot, TemporalHypergraph};
pub use timestamp::{Clock, TimeRange, Timestamp};

// Re-export the backend's stable index types for convenience.
pub use catgraph_applied::{HyperedgeIndex, VertexIndex};

/// Bounds a vertex payload must satisfy: `Copy + Eq + Debug + Send + Sync`.
/// Blanket-implemented for every type that meets them.
pub trait VertexTrait: Copy + Eq + std::fmt::Debug + Send + Sync {}
impl<T: Copy + Eq + std::fmt::Debug + Send + Sync> VertexTrait for T {}

/// Bounds a hyperedge payload must satisfy: `Copy + Eq + Debug + Send + Sync`.
/// Blanket-implemented for every type that meets them.
pub trait HyperedgeTrait: Copy + Eq + std::fmt::Debug + Send + Sync {}
impl<T: Copy + Eq + std::fmt::Debug + Send + Sync> HyperedgeTrait for T {}
