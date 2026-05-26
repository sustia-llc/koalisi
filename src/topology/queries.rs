//! Temporal query methods for the TemporalHypergraph.

use super::event_log::EventLog;
use super::events::TemporalEvent;
use super::timestamp::{TimeRange, Timestamp};
use hypergraph::{HyperedgeIndex, VertexIndex};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Helper function to check if a vertex existed at a given timestamp.
fn vertex_exists_at_impl<V, HE>(
    events: &EventLog<V, HE>,
    vertex: VertexIndex,
    timestamp: Timestamp,
) -> bool
where
    V: Clone + std::fmt::Debug,
    HE: Clone + std::fmt::Debug,
{
    let vertex_events = events.vertex_events(vertex);

    let mut exists = false;
    for event in vertex_events {
        if event.timestamp() > timestamp {
            break;
        }
        match event {
            TemporalEvent::VertexAdded { index, .. } if *index == vertex => {
                exists = true;
            }
            TemporalEvent::VertexRemoved { index, .. } if *index == vertex => {
                exists = false;
            }
            _ => {}
        }
    }
    exists
}

/// Helper function to check if a hyperedge existed at a given timestamp.
fn hyperedge_exists_at_impl<V, HE>(
    events: &EventLog<V, HE>,
    hyperedge: HyperedgeIndex,
    timestamp: Timestamp,
) -> bool
where
    V: Clone + std::fmt::Debug,
    HE: Clone + std::fmt::Debug,
{
    let hyperedge_events = events.hyperedge_events(hyperedge);

    let mut exists = false;
    for event in hyperedge_events {
        if event.timestamp() > timestamp {
            break;
        }
        match event {
            TemporalEvent::HyperedgeAdded { index, .. } if *index == hyperedge => {
                exists = true;
            }
            TemporalEvent::HyperedgeRemoved { index, .. } if *index == hyperedge => {
                exists = false;
            }
            TemporalEvent::HyperedgesJoined { source_indices, .. }
                if source_indices.contains(&hyperedge) =>
            {
                // Source hyperedges are removed during join
                exists = false;
            }
            _ => {}
        }
    }
    exists
}

/// Query methods that work with a shared event log.
///
/// All methods are async and use `tokio::sync::RwLock` for non-blocking access.
pub struct TemporalQueries;

impl TemporalQueries {
    /// Check if a vertex existed at a given timestamp.
    pub async fn vertex_exists_at<V, HE>(
        events: &Arc<RwLock<EventLog<V, HE>>>,
        vertex: VertexIndex,
        timestamp: Timestamp,
    ) -> bool
    where
        V: Clone + std::fmt::Debug,
        HE: Clone + std::fmt::Debug,
    {
        let events = events.read().await;
        vertex_exists_at_impl(&events, vertex, timestamp)
    }

    /// Check if a hyperedge existed at a given timestamp.
    pub async fn hyperedge_exists_at<V, HE>(
        events: &Arc<RwLock<EventLog<V, HE>>>,
        hyperedge: HyperedgeIndex,
        timestamp: Timestamp,
    ) -> bool
    where
        V: Clone + std::fmt::Debug,
        HE: Clone + std::fmt::Debug,
    {
        let events = events.read().await;
        hyperedge_exists_at_impl(&events, hyperedge, timestamp)
    }

    /// Get the weight of a vertex at a given timestamp.
    pub async fn vertex_weight_at<V, HE>(
        events: &Arc<RwLock<EventLog<V, HE>>>,
        vertex: VertexIndex,
        timestamp: Timestamp,
    ) -> Option<V>
    where
        V: Clone + std::fmt::Debug,
        HE: Clone + std::fmt::Debug,
    {
        let events = events.read().await;
        let vertex_events = events.vertex_events(vertex);

        let mut weight: Option<V> = None;
        for event in vertex_events {
            if event.timestamp() > timestamp {
                break;
            }
            match event {
                TemporalEvent::VertexAdded {
                    index,
                    weight: w,
                    ..
                } if *index == vertex => {
                    weight = Some(w.clone());
                }
                TemporalEvent::VertexRemoved { index, .. } if *index == vertex => {
                    weight = None;
                }
                TemporalEvent::VertexWeightUpdated {
                    index,
                    new_weight,
                    ..
                } if *index == vertex => {
                    weight = Some(new_weight.clone());
                }
                _ => {}
            }
        }
        weight
    }

    /// Get the weight of a hyperedge at a given timestamp.
    pub async fn hyperedge_weight_at<V, HE>(
        events: &Arc<RwLock<EventLog<V, HE>>>,
        hyperedge: HyperedgeIndex,
        timestamp: Timestamp,
    ) -> Option<HE>
    where
        V: Clone + std::fmt::Debug,
        HE: Clone + std::fmt::Debug,
    {
        let events = events.read().await;
        let hyperedge_events = events.hyperedge_events(hyperedge);

        let mut weight: Option<HE> = None;
        for event in hyperedge_events {
            if event.timestamp() > timestamp {
                break;
            }
            match event {
                TemporalEvent::HyperedgeAdded {
                    index,
                    weight: w,
                    ..
                } if *index == hyperedge => {
                    weight = Some(w.clone());
                }
                TemporalEvent::HyperedgeRemoved { index, .. } if *index == hyperedge => {
                    weight = None;
                }
                TemporalEvent::HyperedgeWeightUpdated {
                    index,
                    new_weight,
                    ..
                } if *index == hyperedge => {
                    weight = Some(new_weight.clone());
                }
                TemporalEvent::HyperedgesJoined { source_indices, .. }
                    if source_indices.contains(&hyperedge) =>
                {
                    weight = None;
                }
                _ => {}
            }
        }
        weight
    }

    /// Get the vertices of a hyperedge at a given timestamp.
    pub async fn hyperedge_vertices_at<V, HE>(
        events: &Arc<RwLock<EventLog<V, HE>>>,
        hyperedge: HyperedgeIndex,
        timestamp: Timestamp,
    ) -> Option<Vec<VertexIndex>>
    where
        V: Clone + std::fmt::Debug,
        HE: Clone + std::fmt::Debug,
    {
        let events = events.read().await;
        let hyperedge_events = events.hyperedge_events(hyperedge);

        let mut vertices: Option<Vec<VertexIndex>> = None;
        for event in hyperedge_events {
            if event.timestamp() > timestamp {
                break;
            }
            match event {
                TemporalEvent::HyperedgeAdded {
                    index,
                    vertices: v,
                    ..
                } if *index == hyperedge => {
                    vertices = Some(v.clone());
                }
                TemporalEvent::HyperedgeRemoved { index, .. } if *index == hyperedge => {
                    vertices = None;
                }
                TemporalEvent::HyperedgeVerticesUpdated {
                    index,
                    new_vertices,
                    ..
                } if *index == hyperedge => {
                    vertices = Some(new_vertices.clone());
                }
                TemporalEvent::HyperedgeReversed {
                    index,
                    new_vertices,
                    ..
                } if *index == hyperedge => {
                    vertices = Some(new_vertices.clone());
                }
                TemporalEvent::HyperedgesJoined {
                    target_index,
                    new_vertices,
                    source_indices,
                    ..
                } => {
                    if *target_index == hyperedge {
                        vertices = Some(new_vertices.clone());
                    } else if source_indices.contains(&hyperedge) {
                        vertices = None;
                    }
                }
                TemporalEvent::VerticesContracted {
                    hyperedge_index,
                    new_vertices,
                    ..
                } if *hyperedge_index == hyperedge => {
                    vertices = Some(new_vertices.clone());
                }
                _ => {}
            }
        }
        vertices
    }

    /// Count vertices at a given timestamp.
    ///
    /// This method holds the lock for the entire operation to avoid TOCTOU issues.
    pub async fn count_vertices_at<V, HE>(
        events: &Arc<RwLock<EventLog<V, HE>>>,
        timestamp: Timestamp,
    ) -> usize
    where
        V: Clone + std::fmt::Debug,
        HE: Clone + std::fmt::Debug,
    {
        let events_guard = events.read().await;
        let all_vertices = events_guard.all_vertex_indices();

        // Use the impl function directly to avoid lock contention
        all_vertices
            .iter()
            .filter(|v| vertex_exists_at_impl(&events_guard, **v, timestamp))
            .count()
    }

    /// Count hyperedges at a given timestamp.
    ///
    /// This method holds the lock for the entire operation to avoid TOCTOU issues.
    pub async fn count_hyperedges_at<V, HE>(
        events: &Arc<RwLock<EventLog<V, HE>>>,
        timestamp: Timestamp,
    ) -> usize
    where
        V: Clone + std::fmt::Debug,
        HE: Clone + std::fmt::Debug,
    {
        let events_guard = events.read().await;
        let all_hyperedges = events_guard.all_hyperedge_indices();

        // Use the impl function directly to avoid lock contention
        all_hyperedges
            .iter()
            .filter(|h| hyperedge_exists_at_impl(&events_guard, **h, timestamp))
            .count()
    }

    /// Get the first timestamp when a vertex was added.
    pub async fn vertex_created_at<V, HE>(
        events: &Arc<RwLock<EventLog<V, HE>>>,
        vertex: VertexIndex,
    ) -> Option<Timestamp>
    where
        V: Clone + std::fmt::Debug,
        HE: Clone + std::fmt::Debug,
    {
        let events = events.read().await;
        let vertex_events = events.vertex_events(vertex);

        for event in vertex_events {
            if let TemporalEvent::VertexAdded { index, timestamp, .. } = event {
                if *index == vertex {
                    return Some(*timestamp);
                }
            }
        }
        None
    }

    /// Get the timestamp when a vertex was last removed (or None if still exists).
    pub async fn vertex_removed_at<V, HE>(
        events: &Arc<RwLock<EventLog<V, HE>>>,
        vertex: VertexIndex,
    ) -> Option<Timestamp>
    where
        V: Clone + std::fmt::Debug,
        HE: Clone + std::fmt::Debug,
    {
        let events = events.read().await;
        let vertex_events = events.vertex_events(vertex);

        let mut removed_at = None;
        for event in vertex_events {
            match event {
                TemporalEvent::VertexAdded { index, .. } if *index == vertex => {
                    removed_at = None;
                }
                TemporalEvent::VertexRemoved { index, timestamp, .. } if *index == vertex => {
                    removed_at = Some(*timestamp);
                }
                _ => {}
            }
        }
        removed_at
    }

    /// Get the first timestamp when a hyperedge was added.
    pub async fn hyperedge_created_at<V, HE>(
        events: &Arc<RwLock<EventLog<V, HE>>>,
        hyperedge: HyperedgeIndex,
    ) -> Option<Timestamp>
    where
        V: Clone + std::fmt::Debug,
        HE: Clone + std::fmt::Debug,
    {
        let events = events.read().await;
        let hyperedge_events = events.hyperedge_events(hyperedge);

        for event in hyperedge_events {
            if let TemporalEvent::HyperedgeAdded { index, timestamp, .. } = event {
                if *index == hyperedge {
                    return Some(*timestamp);
                }
            }
        }
        None
    }

    /// Get the timestamp when a hyperedge was last removed (or None if still exists).
    pub async fn hyperedge_removed_at<V, HE>(
        events: &Arc<RwLock<EventLog<V, HE>>>,
        hyperedge: HyperedgeIndex,
    ) -> Option<Timestamp>
    where
        V: Clone + std::fmt::Debug,
        HE: Clone + std::fmt::Debug,
    {
        let events = events.read().await;
        let hyperedge_events = events.hyperedge_events(hyperedge);

        let mut removed_at = None;
        for event in hyperedge_events {
            match event {
                TemporalEvent::HyperedgeAdded { index, .. } if *index == hyperedge => {
                    removed_at = None;
                }
                TemporalEvent::HyperedgeRemoved { index, timestamp, .. }
                    if *index == hyperedge =>
                {
                    removed_at = Some(*timestamp);
                }
                TemporalEvent::HyperedgesJoined {
                    source_indices,
                    timestamp,
                    ..
                } if source_indices.contains(&hyperedge) => {
                    removed_at = Some(*timestamp);
                }
                _ => {}
            }
        }
        removed_at
    }

    /// Get the time range during which a vertex existed.
    pub async fn vertex_lifespan<V, HE>(
        events: &Arc<RwLock<EventLog<V, HE>>>,
        vertex: VertexIndex,
    ) -> Option<TimeRange>
    where
        V: Clone + std::fmt::Debug,
        HE: Clone + std::fmt::Debug,
    {
        let created = Self::vertex_created_at(events, vertex).await?;
        let removed = Self::vertex_removed_at(events, vertex).await;
        Some(TimeRange::new(Some(created), removed))
    }

    /// Get the time range during which a hyperedge existed.
    pub async fn hyperedge_lifespan<V, HE>(
        events: &Arc<RwLock<EventLog<V, HE>>>,
        hyperedge: HyperedgeIndex,
    ) -> Option<TimeRange>
    where
        V: Clone + std::fmt::Debug,
        HE: Clone + std::fmt::Debug,
    {
        let created = Self::hyperedge_created_at(events, hyperedge).await?;
        let removed = Self::hyperedge_removed_at(events, hyperedge).await;
        Some(TimeRange::new(Some(created), removed))
    }
}
