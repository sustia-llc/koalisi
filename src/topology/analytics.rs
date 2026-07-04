//! Temporal analytics for hypergraphs.
//!
//! This module provides analytics and time-series functions for analyzing
//! the evolution of a temporal hypergraph over time.

use super::event_log::EventLog;
use super::events::TemporalEvent;
use super::timestamp::{TimeRange, Timestamp};
use catgraph_applied::{HyperedgeIndex, VertexIndex};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;

#[cfg(feature = "magnitude")]
use crate::algorithms::AgentCapabilities;
#[cfg(feature = "magnitude")]
use crate::decision::{magnitude_or_zero, relevant_masks};

/// Represents changes between two timestamps.
#[derive(Debug, Clone)]
pub struct GraphDelta<V, HE>
where
    V: Clone,
    HE: Clone,
{
    /// Vertices added during this period.
    pub vertices_added: Vec<(VertexIndex, V)>,
    /// Vertices removed during this period.
    pub vertices_removed: Vec<(VertexIndex, V)>,
    /// Vertices whose weight was updated (index, old_weight, new_weight).
    pub vertices_updated: Vec<(VertexIndex, V, V)>,
    /// Hyperedges added during this period.
    pub hyperedges_added: Vec<(HyperedgeIndex, Vec<VertexIndex>, HE)>,
    /// Hyperedges removed during this period.
    pub hyperedges_removed: Vec<(HyperedgeIndex, Vec<VertexIndex>, HE)>,
    /// Hyperedges whose weight was updated (index, old_weight, new_weight).
    pub hyperedges_weight_updated: Vec<(HyperedgeIndex, HE, HE)>,
    /// Hyperedges whose vertices were updated (index, old_vertices, new_vertices).
    pub hyperedges_vertices_updated: Vec<(HyperedgeIndex, Vec<VertexIndex>, Vec<VertexIndex>)>,
    /// Hyperedges that were reversed.
    pub hyperedges_reversed: Vec<HyperedgeIndex>,
    /// Hyperedges that were joined (target, sources).
    pub hyperedges_joined: Vec<(HyperedgeIndex, Vec<HyperedgeIndex>)>,
    /// Vertex contractions that occurred.
    pub contractions: Vec<(HyperedgeIndex, Vec<VertexIndex>, VertexIndex)>,
    /// `HyperedgesCleared` events (timestamp, count cleared).
    pub hyperedges_cleared: Vec<(Timestamp, usize)>,
    /// `GraphCleared` events (timestamp, vertex_count, hyperedge_count).
    pub graph_cleared: Vec<(Timestamp, usize, usize)>,
}

impl<V, HE> Default for GraphDelta<V, HE>
where
    V: Clone,
    HE: Clone,
{
    fn default() -> Self {
        Self {
            vertices_added: Vec::new(),
            vertices_removed: Vec::new(),
            vertices_updated: Vec::new(),
            hyperedges_added: Vec::new(),
            hyperedges_removed: Vec::new(),
            hyperedges_weight_updated: Vec::new(),
            hyperedges_vertices_updated: Vec::new(),
            hyperedges_reversed: Vec::new(),
            hyperedges_joined: Vec::new(),
            contractions: Vec::new(),
            hyperedges_cleared: Vec::new(),
            graph_cleared: Vec::new(),
        }
    }
}

impl<V, HE> GraphDelta<V, HE>
where
    V: Clone,
    HE: Clone,
{
    /// Returns true if no changes occurred.
    pub fn is_empty(&self) -> bool {
        self.vertices_added.is_empty()
            && self.vertices_removed.is_empty()
            && self.vertices_updated.is_empty()
            && self.hyperedges_added.is_empty()
            && self.hyperedges_removed.is_empty()
            && self.hyperedges_weight_updated.is_empty()
            && self.hyperedges_vertices_updated.is_empty()
            && self.hyperedges_reversed.is_empty()
            && self.hyperedges_joined.is_empty()
            && self.contractions.is_empty()
            && self.hyperedges_cleared.is_empty()
            && self.graph_cleared.is_empty()
    }

    /// Total number of changes.
    pub fn change_count(&self) -> usize {
        self.vertices_added.len()
            + self.vertices_removed.len()
            + self.vertices_updated.len()
            + self.hyperedges_added.len()
            + self.hyperedges_removed.len()
            + self.hyperedges_weight_updated.len()
            + self.hyperedges_vertices_updated.len()
            + self.hyperedges_reversed.len()
            + self.hyperedges_joined.len()
            + self.contractions.len()
            + self.hyperedges_cleared.len()
            + self.graph_cleared.len()
    }
}

/// One point on a coalition's magnitude trajectory: the coalition's diversity
/// (its pinned `t = 1` [`catgraph_magnitude::coalition_value`]) at a
/// change-relevant timestamp, alongside the number of members whose weight
/// resolved at that instant.
///
/// `member_count` is the count of resolved members **before** relevance
/// filtering / skeletalization, so a redundant clone joining shows the count
/// rise while `magnitude` stays flat (see
/// [`TemporalAnalytics::magnitude_history`]).
#[cfg(feature = "magnitude")]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MagnitudePoint {
    /// The timestamp this diversity value applies from (until the next point).
    pub timestamp: Timestamp,
    /// Coalition magnitude at `timestamp`, pinned `t = 1`. `f64::NEG_INFINITY`
    /// if the upstream magnitude computation errored (logged), `0.0` for a
    /// dissolved or wholly task-irrelevant coalition.
    pub magnitude: f64,
    /// Members whose weight resolved at `timestamp`, counted before relevance
    /// filtering and skeletalization.
    pub member_count: usize,
}

/// Temporal analytics functions for analyzing graph evolution.
///
/// Several `pub(crate)` methods are anchors for Phase 7 (persistence — was
/// Phase 5 before the roadmap reorder; Phase 6, the decision layer, shipped);
/// they have no in-crate callers yet and carry `#[allow(dead_code)]` until
/// Phase 7 lands.
pub struct TemporalAnalytics;

impl TemporalAnalytics {
    /// Compute the delta (changes) between two timestamps.
    ///
    /// Returns all changes that occurred strictly after `from` and up to and including `to`.
    pub async fn delta<V, HE>(
        events: &Arc<RwLock<EventLog<V, HE>>>,
        from: Timestamp,
        to: Timestamp,
    ) -> GraphDelta<V, HE>
    where
        V: Clone + std::fmt::Debug,
        HE: Clone + std::fmt::Debug,
    {
        let events_guard = events.read().await;
        let range = TimeRange::new(Some(from), Some(to));
        let events_in_range = events_guard.events_in_range(&range);

        let mut delta = GraphDelta::default();

        for event in events_in_range {
            if event.timestamp() == from {
                continue;
            }

            match event {
                TemporalEvent::VertexAdded { index, weight, .. } => {
                    delta.vertices_added.push((*index, weight.clone()));
                }
                TemporalEvent::VertexRemoved { index, weight, .. } => {
                    delta.vertices_removed.push((*index, weight.clone()));
                }
                TemporalEvent::VertexWeightUpdated {
                    index,
                    old_weight,
                    new_weight,
                    ..
                } => {
                    delta
                        .vertices_updated
                        .push((*index, old_weight.clone(), new_weight.clone()));
                }
                TemporalEvent::HyperedgeAdded {
                    index,
                    vertices,
                    weight,
                    ..
                } => {
                    delta
                        .hyperedges_added
                        .push((*index, vertices.clone(), weight.clone()));
                }
                TemporalEvent::HyperedgeRemoved {
                    index,
                    vertices,
                    weight,
                    ..
                } => {
                    delta
                        .hyperedges_removed
                        .push((*index, vertices.clone(), weight.clone()));
                }
                TemporalEvent::HyperedgeWeightUpdated {
                    index,
                    old_weight,
                    new_weight,
                    ..
                } => {
                    delta.hyperedges_weight_updated.push((
                        *index,
                        old_weight.clone(),
                        new_weight.clone(),
                    ));
                }
                TemporalEvent::HyperedgeVerticesUpdated {
                    index,
                    old_vertices,
                    new_vertices,
                    ..
                } => {
                    delta.hyperedges_vertices_updated.push((
                        *index,
                        old_vertices.clone(),
                        new_vertices.clone(),
                    ));
                }
                TemporalEvent::HyperedgeReversed { index, .. } => {
                    delta.hyperedges_reversed.push(*index);
                }
                TemporalEvent::HyperedgesJoined {
                    target_index,
                    source_indices,
                    ..
                } => {
                    delta
                        .hyperedges_joined
                        .push((*target_index, source_indices.clone()));
                }
                TemporalEvent::VerticesContracted {
                    hyperedge_index,
                    contracted_vertices,
                    target_vertex,
                    ..
                } => {
                    delta.contractions.push((
                        *hyperedge_index,
                        contracted_vertices.clone(),
                        *target_vertex,
                    ));
                }
                TemporalEvent::HyperedgesCleared { timestamp, count } => {
                    delta.hyperedges_cleared.push((*timestamp, *count));
                }
                TemporalEvent::GraphCleared {
                    timestamp,
                    vertex_count,
                    hyperedge_count,
                } => {
                    delta
                        .graph_cleared
                        .push((*timestamp, *vertex_count, *hyperedge_count));
                }
                TemporalEvent::SnapshotMarker { .. } => {
                    // Markers are bookkeeping, not topology changes.
                }
            }
        }

        delta
    }

    /// Replay a coalition (hyperedge) along the event-sourced history and emit
    /// its magnitude trajectory: one [`MagnitudePoint`] per change-relevant
    /// timestamp, evaluating the pinned `t = 1`
    /// [`catgraph_magnitude::coalition_value`] fresh at each point.
    ///
    /// At every sample the coalition's current members are resolved to their
    /// vertex weights, mapped through the same bystander-excluding
    /// `relevant_masks` contract the [`crate::decision`] magnitude arm uses,
    /// and scored. `member_count` reports the resolved-member count *before*
    /// relevance filtering / skeletalization, so a redundant clone joining
    /// raises the count while `magnitude` stays flat.
    ///
    /// # Sampling (change-driven step function)
    ///
    /// Sampling is change-driven, not fixed-resolution: a point is emitted only
    /// where the task-relevant mask multiset *may* change — a membership event
    /// for `hyperedge` (a [`TemporalEvent::HyperedgeReversed`] is skipped: it
    /// only reorders members, leaving the multiset unchanged), a
    /// [`TemporalEvent::VertexWeightUpdated`] / [`TemporalEvent::VertexRemoved`]
    /// on a current member, or a clear while the coalition is live. The result
    /// is a step function; a consumer wanting fixed resolution resamples it. All
    /// events at a shared timestamp are folded before the point is taken (the
    /// same peek-ahead flush the sibling series functions use), so a
    /// multi-event instant yields one settled point.
    ///
    /// # Window semantics
    ///
    /// `range` is resolved like the sibling series functions (unbounded start →
    /// EPOCH, unbounded end → last event timestamp). A baseline point is emitted
    /// at the resolved start if the coalition is live there (so a window opened
    /// after formation still reports the standing diversity); change points are
    /// taken for `start < ts <= end` (inclusive end). A dissolution inside the
    /// window (removal, source-join, or clear) emits a terminal point that
    /// evaluates to `0.0` with `member_count == 0`.
    ///
    /// # Notes
    ///
    /// - `t = 1` is pinned upstream; there is no `t` parameter (a `t`-sweep is a
    ///   separate, sweep-shaped concern).
    /// - The incremental [`catgraph_magnitude::CoalitionEvaluator`] is
    ///   deliberately **not** used: consecutive samples differ in member set by
    ///   construction, so the evaluator's base key would miss every sample and a
    ///   rebuild costs ≈ 10–15× one fresh evaluation — the trajectory access
    ///   pattern does not fit the incremental path. Revisit only for a
    ///   sweep-shaped variant.
    /// - Members whose weight is missing from the reconstructed vertex-weight
    ///   map (e.g. the vertex was removed) are skipped and not counted.
    /// - An upstream magnitude error at a point is logged and recorded as
    ///   `f64::NEG_INFINITY` (never a panic), mirroring the decision arm.
    /// - Sampling runs under one read guard; the guard is released before the
    ///   `O(m³)` magnitude evaluations, which run in a single
    ///   [`tokio_rayon::spawn`] offload.
    #[cfg(feature = "magnitude")]
    pub async fn magnitude_history<V, HE>(
        events: &Arc<RwLock<EventLog<V, HE>>>,
        hyperedge: HyperedgeIndex,
        required_capabilities: u32,
        range: &TimeRange,
    ) -> Vec<MagnitudePoint>
    where
        V: Clone + std::fmt::Debug + AgentCapabilities + 'static,
        HE: Clone + std::fmt::Debug + 'static,
    {
        // Sample under one read guard (released when this returns), then evaluate
        // every point's O(m³) magnitude fresh at t = 1 in a single rayon offload
        // (CoalitionEvaluator deliberately not used — see fn docs).
        let samples =
            collect_magnitude_samples(events, hyperedge, required_capabilities, range).await;
        tokio_rayon::spawn(move || {
            samples
                .into_iter()
                .map(|(timestamp, masks, member_count)| {
                    let magnitude = match magnitude_or_zero(&masks, required_capabilities) {
                        Ok(m) => m,
                        Err(e) => {
                            tracing::warn!(error = %e, "magnitude trajectory point computation failed");
                            f64::NEG_INFINITY
                        }
                    };
                    MagnitudePoint {
                        timestamp,
                        magnitude,
                        member_count,
                    }
                })
                .collect()
        })
        .await
    }

    /// Resolve a time range against the actual event log, defaulting unbounded
    /// ends to EPOCH (start) and the last event's timestamp (end).
    ///
    /// Live caller under `magnitude` (`collect_magnitude_samples`); otherwise
    /// reachable only from the `#[allow(dead_code)]` phase-anchor series fns.
    #[cfg_attr(not(feature = "magnitude"), allow(dead_code))]
    fn resolve_window<V, HE>(
        events_guard: &EventLog<V, HE>,
        range: &TimeRange,
    ) -> (Timestamp, Timestamp)
    where
        V: Clone + std::fmt::Debug,
        HE: Clone + std::fmt::Debug,
    {
        let start = range.start.unwrap_or(Timestamp::EPOCH);
        let end = range.end.unwrap_or_else(|| {
            events_guard
                .events()
                .last()
                .map(|e| e.timestamp())
                .unwrap_or(start)
        });
        (start, end)
    }

    /// Generate a time series of vertex counts via a single chronological pass.
    #[allow(dead_code)]
    pub(crate) async fn vertex_count_series<V, HE>(
        events: &Arc<RwLock<EventLog<V, HE>>>,
        range: &TimeRange,
        resolution: u64,
    ) -> Vec<(Timestamp, usize)>
    where
        V: Clone + std::fmt::Debug,
        HE: Clone + std::fmt::Debug,
    {
        let events_guard = events.read().await;
        let (start, end) = Self::resolve_window(&events_guard, range);

        let mut live: HashSet<VertexIndex> = HashSet::new();
        let mut series = Vec::new();
        let mut iter = events_guard.events().iter().peekable();
        let mut current = start;

        loop {
            while let Some(event) = iter.peek() {
                if event.timestamp() > current {
                    break;
                }
                match event {
                    TemporalEvent::VertexAdded { index, .. } => {
                        live.insert(*index);
                    }
                    TemporalEvent::VertexRemoved { index, .. } => {
                        live.remove(index);
                    }
                    TemporalEvent::GraphCleared { .. } => {
                        live.clear();
                    }
                    _ => {}
                }
                iter.next();
            }
            series.push((current, live.len()));
            if current >= end {
                break;
            }
            if current.value() > u64::MAX - resolution {
                break;
            }
            current = Timestamp(current.value() + resolution);
            if current > end {
                current = end;
            }
        }

        series
    }

    /// Generate a time series of hyperedge counts via a single chronological pass.
    #[allow(dead_code)]
    pub(crate) async fn hyperedge_count_series<V, HE>(
        events: &Arc<RwLock<EventLog<V, HE>>>,
        range: &TimeRange,
        resolution: u64,
    ) -> Vec<(Timestamp, usize)>
    where
        V: Clone + std::fmt::Debug,
        HE: Clone + std::fmt::Debug,
    {
        let events_guard = events.read().await;
        let (start, end) = Self::resolve_window(&events_guard, range);

        let mut live: HashSet<HyperedgeIndex> = HashSet::new();
        let mut series = Vec::new();
        let mut iter = events_guard.events().iter().peekable();
        let mut current = start;

        loop {
            while let Some(event) = iter.peek() {
                if event.timestamp() > current {
                    break;
                }
                match event {
                    TemporalEvent::HyperedgeAdded { index, .. } => {
                        live.insert(*index);
                    }
                    TemporalEvent::HyperedgeRemoved { index, .. } => {
                        live.remove(index);
                    }
                    TemporalEvent::HyperedgesJoined {
                        target_index,
                        source_indices,
                        ..
                    } => {
                        live.insert(*target_index);
                        for s in source_indices {
                            live.remove(s);
                        }
                    }
                    TemporalEvent::HyperedgesCleared { .. }
                    | TemporalEvent::GraphCleared { .. } => {
                        live.clear();
                    }
                    _ => {}
                }
                iter.next();
            }
            series.push((current, live.len()));
            if current >= end {
                break;
            }
            if current.value() > u64::MAX - resolution {
                break;
            }
            current = Timestamp(current.value() + resolution);
            if current > end {
                current = end;
            }
        }

        series
    }

    /// Calculate the mutation rate (events per time unit) within a range.
    #[allow(dead_code)]
    pub(crate) async fn mutation_rate<V, HE>(
        events: &Arc<RwLock<EventLog<V, HE>>>,
        range: &TimeRange,
    ) -> f64
    where
        V: Clone + std::fmt::Debug,
        HE: Clone + std::fmt::Debug,
    {
        let events_guard = events.read().await;
        let event_count = events_guard.events_in_range(range).len();
        let (start, end) = Self::resolve_window(&events_guard, range);

        let duration = end.value().saturating_sub(start.value());
        if duration == 0 {
            return event_count as f64;
        }

        event_count as f64 / duration as f64
    }

    /// Find the most active vertices (those with the most events).
    #[allow(dead_code)]
    pub(crate) async fn most_active_vertices<V, HE>(
        events: &Arc<RwLock<EventLog<V, HE>>>,
        range: &TimeRange,
        limit: usize,
    ) -> Vec<(VertexIndex, usize)>
    where
        V: Clone + std::fmt::Debug,
        HE: Clone + std::fmt::Debug,
    {
        let events_guard = events.read().await;
        let events_in_range = events_guard.events_in_range(range);

        let mut counts: HashMap<VertexIndex, usize> = HashMap::new();

        for event in events_in_range {
            match event {
                TemporalEvent::VertexAdded { index, .. }
                | TemporalEvent::VertexRemoved { index, .. }
                | TemporalEvent::VertexWeightUpdated { index, .. } => {
                    *counts.entry(*index).or_insert(0) += 1;
                }
                TemporalEvent::HyperedgeAdded { vertices, .. } => {
                    for v in vertices {
                        *counts.entry(*v).or_insert(0) += 1;
                    }
                }
                TemporalEvent::HyperedgeVerticesUpdated {
                    old_vertices,
                    new_vertices,
                    ..
                } => {
                    for v in old_vertices {
                        *counts.entry(*v).or_insert(0) += 1;
                    }
                    for v in new_vertices {
                        *counts.entry(*v).or_insert(0) += 1;
                    }
                }
                TemporalEvent::VerticesContracted {
                    contracted_vertices,
                    target_vertex,
                    ..
                } => {
                    for v in contracted_vertices {
                        *counts.entry(*v).or_insert(0) += 1;
                    }
                    *counts.entry(*target_vertex).or_insert(0) += 1;
                }
                _ => {}
            }
        }

        let mut sorted: Vec<_> = counts.into_iter().collect();
        sorted.sort_by_key(|x| std::cmp::Reverse(x.1));
        sorted.truncate(limit);
        sorted
    }

    /// Find the most active hyperedges (those with the most events).
    #[allow(dead_code)]
    pub(crate) async fn most_active_hyperedges<V, HE>(
        events: &Arc<RwLock<EventLog<V, HE>>>,
        range: &TimeRange,
        limit: usize,
    ) -> Vec<(HyperedgeIndex, usize)>
    where
        V: Clone + std::fmt::Debug,
        HE: Clone + std::fmt::Debug,
    {
        let events_guard = events.read().await;
        let events_in_range = events_guard.events_in_range(range);

        let mut counts: HashMap<HyperedgeIndex, usize> = HashMap::new();

        for event in events_in_range {
            match event {
                TemporalEvent::HyperedgeAdded { index, .. }
                | TemporalEvent::HyperedgeRemoved { index, .. }
                | TemporalEvent::HyperedgeWeightUpdated { index, .. }
                | TemporalEvent::HyperedgeVerticesUpdated { index, .. }
                | TemporalEvent::HyperedgeReversed { index, .. } => {
                    *counts.entry(*index).or_insert(0) += 1;
                }
                TemporalEvent::HyperedgesJoined {
                    target_index,
                    source_indices,
                    ..
                } => {
                    *counts.entry(*target_index).or_insert(0) += 1;
                    for s in source_indices {
                        *counts.entry(*s).or_insert(0) += 1;
                    }
                }
                TemporalEvent::VerticesContracted {
                    hyperedge_index, ..
                } => {
                    *counts.entry(*hyperedge_index).or_insert(0) += 1;
                }
                _ => {}
            }
        }

        let mut sorted: Vec<_> = counts.into_iter().collect();
        sorted.sort_by_key(|x| std::cmp::Reverse(x.1));
        sorted.truncate(limit);
        sorted
    }

    /// Count total events in a time range.
    pub async fn event_count_in_range<V, HE>(
        events: &Arc<RwLock<EventLog<V, HE>>>,
        range: &TimeRange,
    ) -> usize
    where
        V: Clone + std::fmt::Debug,
        HE: Clone + std::fmt::Debug,
    {
        let events_guard = events.read().await;
        events_guard.events_in_range(range).len()
    }

    /// Get event counts by type within a range.
    ///
    /// Returns counts keyed by the `&'static str` from `TemporalEvent::event_type()`
    /// so no per-event String allocation occurs.
    #[allow(dead_code)]
    pub(crate) async fn events_by_type<V, HE>(
        events: &Arc<RwLock<EventLog<V, HE>>>,
        range: &TimeRange,
    ) -> HashMap<&'static str, usize>
    where
        V: Clone + std::fmt::Debug,
        HE: Clone + std::fmt::Debug,
    {
        let events_guard = events.read().await;
        let mut counts: HashMap<&'static str, usize> = HashMap::new();
        for event in events_guard.events_in_range(range) {
            *counts.entry(event.event_type()).or_insert(0) += 1;
        }
        counts
    }
}

/// Single chronological pass for [`TemporalAnalytics::magnitude_history`]: under
/// one read guard, replay the event log to produce the settled samples
/// `(timestamp, task-relevant member masks, resolved-member count)`. The guard
/// is released when this returns, before the caller's magnitude evaluations.
#[cfg(feature = "magnitude")]
async fn collect_magnitude_samples<V, HE>(
    events: &Arc<RwLock<EventLog<V, HE>>>,
    hyperedge: HyperedgeIndex,
    required: u32,
    range: &TimeRange,
) -> Vec<(Timestamp, Vec<u32>, usize)>
where
    V: Clone + std::fmt::Debug + AgentCapabilities + 'static,
    HE: Clone + std::fmt::Debug + 'static,
{
    let events_guard = events.read().await;
    let (start, end) = TemporalAnalytics::resolve_window(&events_guard, range);

    let mut weights: HashMap<VertexIndex, V> = HashMap::new();
    let mut members: Option<Vec<VertexIndex>> = None;
    let mut samples: Vec<(Timestamp, Vec<u32>, usize)> = Vec::new();

    let all = events_guard.events();
    let mut idx = 0;

    // Fold every event at or before the window start into the baseline state,
    // then emit a baseline point if the coalition is live at start.
    while idx < all.len() && all[idx].timestamp() <= start {
        apply_event_to_state(&all[idx], hyperedge, &mut weights, &mut members);
        idx += 1;
    }
    if start <= end && members.is_some() {
        let (masks, count) = snapshot_masks(members.as_deref(), &weights, required);
        samples.push((start, masks, count));
    }

    // Change-driven sampling over (start, end]. Process every event at a
    // timestamp before sampling so a multi-event timestamp yields one settled
    // point.
    while idx < all.len() {
        let ts = all[idx].timestamp();
        if ts > end {
            break;
        }
        let mut change_relevant = false;
        while idx < all.len() && all[idx].timestamp() == ts {
            if is_change_relevant(&all[idx], hyperedge, members.as_deref()) {
                change_relevant = true;
            }
            apply_event_to_state(&all[idx], hyperedge, &mut weights, &mut members);
            idx += 1;
        }
        if change_relevant {
            let (masks, count) = snapshot_masks(members.as_deref(), &weights, required);
            samples.push((ts, masks, count));
        }
    }

    samples
}

/// Fold one event into the running `magnitude_history` state: the vertex-weight
/// map and this hyperedge's membership.
///
/// The membership fold mirrors [`crate::topology::queries`]'s
/// `hyperedge_vertices_at` (including the [`TemporalEvent::HyperedgesJoined`]
/// target-vs-source disambiguation) with **one deliberate divergence**: a
/// [`TemporalEvent::HyperedgesCleared`] or [`TemporalEvent::GraphCleared`]
/// dissolves the coalition (`members` → `None`). The point-in-time query has no
/// clear handling because it stops at a single timestamp; a trajectory must
/// reflect the dissolution as a terminal `0.0` point.
#[cfg(feature = "magnitude")]
fn apply_event_to_state<V, HE>(
    event: &TemporalEvent<V, HE>,
    hyperedge: HyperedgeIndex,
    weights: &mut HashMap<VertexIndex, V>,
    members: &mut Option<Vec<VertexIndex>>,
) where
    V: Clone + std::fmt::Debug,
    HE: Clone + std::fmt::Debug,
{
    match event {
        TemporalEvent::VertexAdded { index, weight, .. } => {
            weights.insert(*index, weight.clone());
        }
        TemporalEvent::VertexWeightUpdated {
            index, new_weight, ..
        } => {
            weights.insert(*index, new_weight.clone());
        }
        TemporalEvent::VertexRemoved { index, .. } => {
            weights.remove(index);
        }
        TemporalEvent::GraphCleared { .. } => {
            weights.clear();
            *members = None; // deliberate divergence from hyperedge_vertices_at
        }
        TemporalEvent::HyperedgeAdded {
            index, vertices, ..
        } if *index == hyperedge => {
            *members = Some(vertices.clone());
        }
        TemporalEvent::HyperedgeVerticesUpdated {
            index,
            new_vertices,
            ..
        } if *index == hyperedge => {
            *members = Some(new_vertices.clone());
        }
        TemporalEvent::HyperedgeReversed {
            index,
            new_vertices,
            ..
        } if *index == hyperedge => {
            // Order-only: not a sample point, but the fold keeps membership accurate.
            *members = Some(new_vertices.clone());
        }
        TemporalEvent::VerticesContracted {
            hyperedge_index,
            new_vertices,
            ..
        } if *hyperedge_index == hyperedge => {
            *members = Some(new_vertices.clone());
        }
        TemporalEvent::HyperedgesJoined {
            target_index,
            source_indices,
            new_vertices,
            ..
        } => {
            if *target_index == hyperedge {
                *members = Some(new_vertices.clone());
            } else if source_indices.contains(&hyperedge) {
                *members = None;
            }
        }
        TemporalEvent::HyperedgeRemoved { index, .. } if *index == hyperedge => {
            *members = None;
        }
        TemporalEvent::HyperedgesCleared { .. } => {
            *members = None; // deliberate divergence from hyperedge_vertices_at
        }
        _ => {}
    }
}

/// Whether `event` may change this coalition's task-relevant mask multiset, and
/// so warrants a [`MagnitudePoint`].
///
/// A [`TemporalEvent::HyperedgeReversed`] is excluded: it only reorders members.
/// Vertex weight updates / removals count only when the vertex is a current
/// member; clears count only while the coalition is live (they yield a terminal
/// dissolution point).
#[cfg(feature = "magnitude")]
fn is_change_relevant<V, HE>(
    event: &TemporalEvent<V, HE>,
    hyperedge: HyperedgeIndex,
    members: Option<&[VertexIndex]>,
) -> bool
where
    V: Clone + std::fmt::Debug,
    HE: Clone + std::fmt::Debug,
{
    match event {
        TemporalEvent::HyperedgeAdded { index, .. }
        | TemporalEvent::HyperedgeVerticesUpdated { index, .. }
        | TemporalEvent::HyperedgeRemoved { index, .. } => *index == hyperedge,
        TemporalEvent::VerticesContracted {
            hyperedge_index, ..
        } => *hyperedge_index == hyperedge,
        TemporalEvent::HyperedgesJoined {
            target_index,
            source_indices,
            ..
        } => *target_index == hyperedge || source_indices.contains(&hyperedge),
        TemporalEvent::VertexWeightUpdated { index, .. }
        | TemporalEvent::VertexRemoved { index, .. } => members.is_some_and(|m| m.contains(index)),
        TemporalEvent::HyperedgesCleared { .. } | TemporalEvent::GraphCleared { .. } => {
            members.is_some()
        }
        _ => false,
    }
}

/// Snapshot the current coalition state into `(task-relevant member masks,
/// resolved-member count)` for one [`MagnitudePoint`].
///
/// Members whose weight is not in `weights` are skipped and not counted;
/// `member_count` is the resolved-member count *before* [`relevant_masks`]
/// applies bystander exclusion and dedup.
#[cfg(feature = "magnitude")]
fn snapshot_masks<V>(
    members: Option<&[VertexIndex]>,
    weights: &HashMap<VertexIndex, V>,
    required: u32,
) -> (Vec<u32>, usize)
where
    V: AgentCapabilities,
{
    let Some(member_indices) = members else {
        return (Vec::new(), 0);
    };
    let resolved: Vec<&V> = member_indices
        .iter()
        .filter_map(|v| weights.get(v))
        .collect();
    let member_count = resolved.len();
    let refs: Vec<&dyn AgentCapabilities> = resolved
        .iter()
        .map(|w| *w as &dyn AgentCapabilities)
        .collect();
    (relevant_masks(&refs, required), member_count)
}
