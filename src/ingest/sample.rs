//! The [`Sample`] trait — the domain-neutral unit of ingestion.
//!
//! A `Sample` is one time-ordered observation flowing through the ingestion
//! layer. It carries three things the layer needs to route and window it,
//! independent of any domain:
//!
//! - a **routing key** ([`Sample::key`]) — which stream / monitor the sample
//!   belongs to (the generic analogue of a forex `Pair`);
//! - a **timestamp** ([`Sample::timestamp_ms`]) — Unix epoch milliseconds, so a
//!   pump can pace or order a merged stream;
//! - a distilled **view** ([`Sample::view`]) — the "latest" per-key state a
//!   monitor tracks (the generic analogue of a forex `Quote`).
//!
//! The forex [`Tick`](crate::market::Tick) is one implementation
//! (`Key = Pair`, `View = Quote`); the synthetic NEST- and tauhokohoko-shaped
//! fixtures in [`synthetic`](super::synthetic) are others.

/// A time-ordered, keyed observation processed by the ingestion layer.
///
/// The bounds are the minimum the [`SampleMonitor`](super::monitor::SampleMonitor)
/// and the pump need: samples cross task and thread boundaries (`Send + Sync +
/// 'static`) and are cloned into snapshots and broadcast updates (`Clone`).
pub trait Sample: Clone + Send + Sync + 'static {
    /// Routing key — which stream / monitor this sample belongs to.
    ///
    /// `Display` is required so wrong-key drops and unroutable samples can be
    /// logged; `Eq + Hash` so a pump can route by key through a `HashMap`.
    type Key: Clone + Eq + std::hash::Hash + std::fmt::Display + Send + Sync + 'static;

    /// Distilled per-key state a monitor tracks as "latest" (the generic
    /// analogue of the forex [`Quote`](crate::market::Quote)).
    type View: Clone + Send + Sync + 'static;

    /// The routing key for this sample.
    ///
    /// Returned by reference so hot-path consumers (the monitor's wrong-key
    /// check, the pump's `HashMap::get`) never clone the key; callers that need
    /// ownership clone explicitly.
    fn key(&self) -> &Self::Key;

    /// Observation time as Unix epoch milliseconds (time-ordered streams).
    fn timestamp_ms(&self) -> i64;

    /// Distil this sample into the "latest" view a monitor holds.
    fn view(&self) -> Self::View;
}
