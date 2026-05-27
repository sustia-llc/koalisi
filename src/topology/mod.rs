//! Temporal hypergraph topology layer.
//!
//! Event-sourced hypergraph with time-travel queries, coalition management,
//! and analytics. Built on [hypergraph](https://github.com/yamafaktory/hypergraph) v4.2.0.

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

// Re-export hypergraph index types for convenience.
pub use hypergraph::{HyperedgeIndex, HyperedgeTrait, VertexIndex, VertexTrait};
