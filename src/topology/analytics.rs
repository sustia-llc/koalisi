//! Temporal analytics for hypergraphs.
//!
//! This module provides analytics and time-series functions for analyzing
//! the evolution of a temporal hypergraph over time.

use super::event_log::EventLog;
use super::events::TemporalEvent;
use super::timestamp::{TimeRange, Timestamp};
use hypergraph::{HyperedgeIndex, VertexIndex};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;

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

/// Temporal analytics functions for analyzing graph evolution.
///
/// Several `pub(crate)` methods are anchors for Phase 5 (persistence) and
/// Phase 6 (decision layer); they have no in-crate callers yet and carry
/// `#[allow(dead_code)]` until those phases land.
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
                    delta
                        .hyperedges_weight_updated
                        .push((*index, old_weight.clone(), new_weight.clone()));
                }
                TemporalEvent::HyperedgeVerticesUpdated {
                    index,
                    old_vertices,
                    new_vertices,
                    ..
                } => {
                    delta
                        .hyperedges_vertices_updated
                        .push((*index, old_vertices.clone(), new_vertices.clone()));
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

    /// Resolve a time range against the actual event log, defaulting unbounded
    /// ends to EPOCH (start) and the last event's timestamp (end).
    #[allow(dead_code)]
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
