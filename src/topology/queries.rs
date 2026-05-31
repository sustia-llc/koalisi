//! Temporal query methods for the TemporalHypergraph.

use super::event_log::EventLog;
use super::events::TemporalEvent;
use super::timestamp::{TimeRange, Timestamp};
use hypergraph::{HyperedgeIndex, VertexIndex};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Check if a vertex existed at a given timestamp.
fn vertex_exists_at_impl<V, HE>(
    events: &EventLog<V, HE>,
    vertex: VertexIndex,
    timestamp: Timestamp,
) -> bool
where
    V: Clone + std::fmt::Debug,
    HE: Clone + std::fmt::Debug,
{
    let mut exists = false;
    for event in events.vertex_events(vertex) {
        if event.timestamp() > timestamp {
            break;
        }
        // `vertex_events` already filtered by vertex, so the inner-index
        // equality guards from the original implementation are redundant.
        match event {
            TemporalEvent::VertexAdded { .. } => exists = true,
            TemporalEvent::VertexRemoved { .. } => exists = false,
            _ => {}
        }
    }
    exists
}

/// Check if a hyperedge existed at a given timestamp.
fn hyperedge_exists_at_impl<V, HE>(
    events: &EventLog<V, HE>,
    hyperedge: HyperedgeIndex,
    timestamp: Timestamp,
) -> bool
where
    V: Clone + std::fmt::Debug,
    HE: Clone + std::fmt::Debug,
{
    let mut exists = false;
    for event in events.hyperedge_events(hyperedge) {
        if event.timestamp() > timestamp {
            break;
        }
        // `hyperedge_events` already filtered by hyperedge.
        match event {
            TemporalEvent::HyperedgeAdded { .. } => exists = true,
            TemporalEvent::HyperedgeRemoved { .. } => exists = false,
            TemporalEvent::HyperedgesJoined { source_indices, .. }
                // Inserted into both target and sources' index lists; only the
                // source role removes the hyperedge — the target role doesn't
                // change existence (it just absorbs the new vertices).
                if source_indices.contains(&hyperedge) => {
                    exists = false;
                }
            _ => {}
        }
    }
    exists
}

fn vertex_created_at_impl<V, HE>(
    events: &EventLog<V, HE>,
    vertex: VertexIndex,
) -> Option<Timestamp>
where
    V: Clone + std::fmt::Debug,
    HE: Clone + std::fmt::Debug,
{
    events
        .vertex_events(vertex)
        .iter()
        .find_map(|e| match e {
            TemporalEvent::VertexAdded { timestamp, .. } => Some(*timestamp),
            _ => None,
        })
}

fn vertex_removed_at_impl<V, HE>(
    events: &EventLog<V, HE>,
    vertex: VertexIndex,
) -> Option<Timestamp>
where
    V: Clone + std::fmt::Debug,
    HE: Clone + std::fmt::Debug,
{
    let mut removed_at = None;
    for event in events.vertex_events(vertex) {
        match event {
            TemporalEvent::VertexAdded { .. } => removed_at = None,
            TemporalEvent::VertexRemoved { timestamp, .. } => removed_at = Some(*timestamp),
            _ => {}
        }
    }
    removed_at
}

fn hyperedge_created_at_impl<V, HE>(
    events: &EventLog<V, HE>,
    hyperedge: HyperedgeIndex,
) -> Option<Timestamp>
where
    V: Clone + std::fmt::Debug,
    HE: Clone + std::fmt::Debug,
{
    events
        .hyperedge_events(hyperedge)
        .iter()
        .find_map(|e| match e {
            TemporalEvent::HyperedgeAdded { timestamp, .. } => Some(*timestamp),
            _ => None,
        })
}

fn hyperedge_removed_at_impl<V, HE>(
    events: &EventLog<V, HE>,
    hyperedge: HyperedgeIndex,
) -> Option<Timestamp>
where
    V: Clone + std::fmt::Debug,
    HE: Clone + std::fmt::Debug,
{
    let mut removed_at = None;
    for event in events.hyperedge_events(hyperedge) {
        match event {
            TemporalEvent::HyperedgeAdded { .. } => removed_at = None,
            TemporalEvent::HyperedgeRemoved { timestamp, .. } => removed_at = Some(*timestamp),
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
                TemporalEvent::VertexAdded { weight: w, .. } => {
                    weight = Some(w.clone());
                }
                TemporalEvent::VertexRemoved { .. } => {
                    weight = None;
                }
                TemporalEvent::VertexWeightUpdated { new_weight, .. } => {
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
                TemporalEvent::HyperedgeAdded { weight: w, .. } => {
                    weight = Some(w.clone());
                }
                TemporalEvent::HyperedgeRemoved { .. } => {
                    weight = None;
                }
                TemporalEvent::HyperedgeWeightUpdated { new_weight, .. } => {
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
                TemporalEvent::HyperedgeAdded { vertices: v, .. } => {
                    vertices = Some(v.clone());
                }
                TemporalEvent::HyperedgeRemoved { .. } => {
                    vertices = None;
                }
                TemporalEvent::HyperedgeVerticesUpdated { new_vertices, .. } => {
                    vertices = Some(new_vertices.clone());
                }
                TemporalEvent::HyperedgeReversed { new_vertices, .. } => {
                    vertices = Some(new_vertices.clone());
                }
                TemporalEvent::HyperedgesJoined {
                    target_index,
                    new_vertices,
                    source_indices,
                    ..
                } => {
                    // The join event is indexed under both the target and each
                    // source, so we must disambiguate which role our query
                    // hyperedge plays.
                    if *target_index == hyperedge {
                        vertices = Some(new_vertices.clone());
                    } else if source_indices.contains(&hyperedge) {
                        vertices = None;
                    }
                }
                TemporalEvent::VerticesContracted { new_vertices, .. } => {
                    vertices = Some(new_vertices.clone());
                }
                _ => {}
            }
        }
        vertices
    }

    /// Count vertices at a given timestamp.
    pub async fn count_vertices_at<V, HE>(
        events: &Arc<RwLock<EventLog<V, HE>>>,
        timestamp: Timestamp,
    ) -> usize
    where
        V: Clone + std::fmt::Debug,
        HE: Clone + std::fmt::Debug,
    {
        let events_guard = events.read().await;
        events_guard
            .vertex_indices()
            .filter(|v| vertex_exists_at_impl(&events_guard, *v, timestamp))
            .count()
    }

    /// Count hyperedges at a given timestamp.
    pub async fn count_hyperedges_at<V, HE>(
        events: &Arc<RwLock<EventLog<V, HE>>>,
        timestamp: Timestamp,
    ) -> usize
    where
        V: Clone + std::fmt::Debug,
        HE: Clone + std::fmt::Debug,
    {
        let events_guard = events.read().await;
        events_guard
            .hyperedge_indices()
            .filter(|h| hyperedge_exists_at_impl(&events_guard, *h, timestamp))
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
        vertex_created_at_impl(&events, vertex)
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
        vertex_removed_at_impl(&events, vertex)
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
        hyperedge_created_at_impl(&events, hyperedge)
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
        hyperedge_removed_at_impl(&events, hyperedge)
    }

    /// Get the time range during which a vertex existed.
    ///
    /// `created` and `removed` are sampled under a single read lock to avoid
    /// the race where a second writer slips an Add/Remove between the two
    /// reads and yields an inconsistent lifespan.
    pub async fn vertex_lifespan<V, HE>(
        events: &Arc<RwLock<EventLog<V, HE>>>,
        vertex: VertexIndex,
    ) -> Option<TimeRange>
    where
        V: Clone + std::fmt::Debug,
        HE: Clone + std::fmt::Debug,
    {
        let events = events.read().await;
        let created = vertex_created_at_impl(&events, vertex)?;
        let removed = vertex_removed_at_impl(&events, vertex);
        Some(TimeRange::new(Some(created), removed))
    }

    /// Get the time range during which a hyperedge existed (single-lock).
    pub async fn hyperedge_lifespan<V, HE>(
        events: &Arc<RwLock<EventLog<V, HE>>>,
        hyperedge: HyperedgeIndex,
    ) -> Option<TimeRange>
    where
        V: Clone + std::fmt::Debug,
        HE: Clone + std::fmt::Debug,
    {
        let events = events.read().await;
        let created = hyperedge_created_at_impl(&events, hyperedge)?;
        let removed = hyperedge_removed_at_impl(&events, hyperedge);
        Some(TimeRange::new(Some(created), removed))
    }
}
