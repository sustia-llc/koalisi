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
mod executor;
mod queries;
mod temporal;
mod timestamp;

pub use analytics::{GraphDelta, TemporalAnalytics};
pub use coalitions::CoalitionManager;
pub use errors::{TemporalError, TemporalResult};
pub use event_log::EventLog;
pub use events::{EventStats, SnapshotId, TemporalEvent};
pub use executor::HypergraphExecutor;
pub use queries::TemporalQueries;
pub use temporal::{SharedGraph, Snapshot, TemporalHypergraph};
pub use timestamp::{Clock, TimeRange, Timestamp};

// Re-export the backend's stable index types for convenience.
pub use catgraph_applied::{HyperedgeIndex, VertexIndex};

/// Bounds a vertex payload must satisfy.
///
/// K1 (sustia-llc/koalisi#4) relaxed this from the previous backend's
/// `VertexTrait: Copy + Debug + Display + Eq + Hash + Send + Sync`:
/// `Display` and `Hash` (and `HyperedgeTrait`'s `Into<usize>`) are dropped —
/// no koalisi call site needs them. The [`catgraph_applied::Hypergraph`]
/// container itself only requires `Copy + Eq + Debug`; `Send + Sync` was
/// already part of the old bound (inherited from the dependency) and is now
/// stated explicitly here because the new container does not supply it, while
/// koalisi still needs it — the tokio-rayon offload
/// (`HypergraphExecutor::run_job`) moves weights between threads and the kameo
/// `CoalitionActor` requires `V`/`HE: Send + Sync`. Blanket-implemented for
/// every qualifying type; test/example payloads that derive extra traits keep
/// them harmlessly.
pub trait VertexTrait: Copy + Eq + std::fmt::Debug + Send + Sync {}
impl<T: Copy + Eq + std::fmt::Debug + Send + Sync> VertexTrait for T {}

/// Bounds a hyperedge payload must satisfy.
///
/// See [`VertexTrait`] — same K1 relaxation (previously additionally required
/// `Into<usize>` on hyperedge payloads; dropped as unused). `Send + Sync`
/// carried over from the old bound, now stated explicitly.
pub trait HyperedgeTrait: Copy + Eq + std::fmt::Debug + Send + Sync {}
impl<T: Copy + Eq + std::fmt::Debug + Send + Sync> HyperedgeTrait for T {}
