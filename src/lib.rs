//! Forex arbitrage swarm — a POC kameo actor swarm that establishes historical
//! data, listens to live tick events, and fires collective arbitrage events
//! once the synthetic-vs-actual cross-rate edge crosses a configured
//! threshold.
//!
//! No persistence yet; everything is in-memory ring buffers + mpsc channels
//! under kameo's actor model.
//!
//! ## Public surface
//!
//! - [`market`] — value types: `Pair`, `Tick`, `Quote`, `Triangle`,
//!   `TickUpdate`, `ArbitrageOpportunity`.
//! - [`subsystems::swarm::Swarm`] — top-level container, modeled after
//!   `surrealdb-live-message`'s `Coalition<T>`: owns the internal kameo
//!   actor refs + a `TaskTracker` + a root `CancellationToken`, exposes
//!   `feed_tick`, `replay_history`, `alerts`, and `shutdown`.
//! - [`subsystems::monitor::MarketMonitor`],
//!   [`subsystems::coordinator::ArbitrageCoordinator`],
//!   [`subsystems::sink::AlertSink`] — the three actor types.
//!
//! See `examples/*.rs` for end-to-end wiring patterns.

pub mod logger;
pub mod market;
pub mod settings;

pub mod subsystems {
    pub mod coordinator;
    #[cfg(feature = "databento")]
    pub mod databento;
    pub mod monitor;
    pub mod sink;
    pub mod swarm;
}

pub use market::{ArbitrageOpportunity, Direction, Pair, Quote, Tick, TickUpdate, Triangle};
pub use subsystems::swarm::{Swarm, SwarmConfig, SwarmFeeder};
