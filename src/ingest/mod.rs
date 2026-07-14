//! Domain-neutral ingestion layer (issue #8, K5).
//!
//! koalisi's architecture is domain-agnostic, but its ingestion used to be
//! finance-specific. This module decouples ingestion from the forex domain:
//!
//! - [`Sample`] — the time-ordered, keyed unit of ingestion.
//! - [`SampleMonitor`] — a per-key ring-buffer worker publishing
//!   [`SampleUpdate`]s to a `broadcast` bus.
//! - [`DataSource`] + [`pump_source`] — a producer of time-ordered samples and
//!   the generic pump that routes them into monitors by key. [`Pacing`] and its
//!   replay semantics live here (domain-neutral).
//! - [`synthetic`] — seeded, credential-free fixtures matching the two real
//!   downstream driver shapes: NEST-style multi-resolution numeric series
//!   ([`MultiResolutionSource`]) and tauhokohoko-style ecological sensor streams
//!   with a changepoint ([`SensorEventSource`]).

pub mod monitor;
pub mod sample;
pub mod source;
pub mod synthetic;

pub use monitor::{
    SampleMonitor, SampleMonitorHandle, SampleSnapshot, SampleUpdate, spawn_sample_monitor,
};
pub use sample::Sample;
pub use source::{DataSource, Pacing, PumpStats, pump_source, spawn_source_pump};
pub use synthetic::{
    MultiResolutionSource, NumericSample, NumericView, SensorEvent, SensorEventSource,
    SensorReading, SensorSpec, SeriesSpec,
};
