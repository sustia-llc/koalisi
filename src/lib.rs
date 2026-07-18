//! koalisi — a reference implementation of agentic coalitions in Rust.
//!
//! ## Architecture
//!
//! - [`core`] — domain-agnostic coalition infrastructure:
//!   [`CoalitionRuntime`](core::CoalitionRuntime) (`TaskTracker` +
//!   `CancellationToken` + three-step shutdown), settings, logging.
//! - [`topology`] — temporal hypergraph, event sourcing,
//!   `CoalitionManager`, time-travel queries, analytics.
//! - [`algorithms`] — `ValueCalculator`, DCVC, AIPA partition search.
//! - [`ingest`] — domain-neutral ingestion: the `Sample` trait, the generic
//!   `SampleMonitor`, a `DataSource` pump, and seeded synthetic fixtures (NEST-
//!   and tauhokohoko-shaped).
//! - [`decision`] — coalition join/leave policies: `ThresholdPolicy` (always
//!   available), behind feature `decision` an Active Inference
//!   expected-free-energy policy, and behind feature `magnitude` its categorical
//!   A/B mirror scoring coalitions by enriched-category magnitude.
//! - [`llm`] — Phase 5/6 LLM provider stub (real backends land later).
//! - [`subsystems`] — the [`CoalitionService`](subsystems::coalition_actor::CoalitionService)
//!   policy-gated membership seam (plus, behind feature `durable`, an optional
//!   durable decision log).
//!
//! ## Quick start
//!
//! See `examples/*.rs` for end-to-end wiring patterns.

pub mod algorithms;
pub mod core;
pub mod decision;
pub mod ingest;
pub mod llm;
#[cfg(feature = "persistence")]
pub mod persistence;
pub mod topology;

pub mod subsystems {
    pub mod coalition_actor;
    #[cfg(feature = "durable")]
    pub mod durable;
    pub mod outcome;
}

pub use decision::{CoalitionDecisionPolicy, Decision, DecisionContext, ThresholdPolicy};
pub use ingest::{DataSource, Sample, SampleMonitor, SampleUpdate};
pub use core::CoalitionRuntime;
