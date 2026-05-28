//! koalisi — a reference implementation of agentic coalitions in Rust.
//!
//! ## Architecture
//!
//! - [`core`] — domain-agnostic coalition infrastructure:
//!   [`CoalitionRuntime`](core::CoalitionRuntime) (TaskTracker +
//!   CancellationToken + three-step shutdown), settings, logging.
//! - [`topology`] — temporal hypergraph, event sourcing,
//!   `CoalitionManager`, time-travel queries, analytics.
//! - [`algorithms`] — `ValueCalculator`, DCVC, AIPA partition search.
//! - [`llm`] — Phase 5/6 LLM provider stub (real backends land later).
//! - [`subsystems`] — forex-specific kameo actors (monitor, coordinator,
//!   sink, swarm) and optional adapters (databento, libp2p remote).
//! - [`market`] — forex value types: `Pair`, `Tick`, `Quote`, `Triangle`,
//!   `TickUpdate`, `ArbitrageOpportunity`.
//!
//! ## Quick start
//!
//! See `examples/*.rs` for end-to-end wiring patterns.

pub mod algorithms;
pub mod core;
pub mod llm;
pub mod market;
pub mod topology;

pub mod subsystems {
    pub mod coordinator;
    #[cfg(feature = "databento")]
    pub mod databento;
    #[cfg(feature = "remote")]
    pub mod distributed;
    pub mod monitor;
    pub mod sink;
    pub mod swarm;
}

pub use market::{ArbitrageOpportunity, Direction, Pair, Quote, Tick, TickUpdate, Triangle};
pub use subsystems::swarm::{Swarm, SwarmConfig, SwarmFeeder};
pub use core::CoalitionRuntime;
