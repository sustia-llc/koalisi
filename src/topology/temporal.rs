//! The core TemporalHypergraph wrapper providing event sourcing.

use super::errors::{TemporalError, TemporalResult};
use super::event_log::EventLog;
use super::events::{SnapshotId, TemporalEvent};
use super::timestamp::{Clock, TimeRange, Timestamp};
use super::executor::EXEC;
use hypergraph::errors::HypergraphError;
use hypergraph::{HyperedgeTrait, Hypergraph, HyperedgeIndex, VertexIndex, VertexTrait};

pub type SharedGraph<V, HE> = std::sync::Arc<std::sync::RwLock<Hypergraph<V, HE>>>;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::RwLock as StdRwLock;
use tokio::sync::RwLock;

/// A snapshot marker at a point in time.
///
/// Snapshots mark a point in the event log. To reconstruct the graph state
/// at a snapshot, replay all events up to the snapshot's event index.
#[derive(Clone, Debug)]
pub struct Snapshot {
    /// The snapshot identifier.
    pub id: SnapshotId,
    /// The timestamp when the snapshot was taken.
    pub timestamp: Timestamp,
    /// The event index at which this snapshot was taken.
    pub event_index: usize,
}

/// A temporal hypergraph with full event sourcing.
///
/// This wraps a SharedGraph and records all mutations as events,
/// allowing reconstruction of the graph state at any point in time.
pub struct TemporalHypergraph<V, HE>
where
    V: VertexTrait + Clone + 'static,
    HE: HyperedgeTrait + Clone + 'static,
{
    /// The underlying graph.
    graph: SharedGraph<V, HE>,
    /// The event log.
    events: Arc<RwLock<EventLog<V, HE>>>,
    /// Snapshots indexed by timestamp.
    snapshots: Arc<RwLock<BTreeMap<Timestamp, Snapshot>>>,
    /// Logical clock for timestamps.
    clock: Clock,
    /// Counter for snapshot IDs.
    snapshot_counter: AtomicU64,
}

impl<V, HE> TemporalHypergraph<V, HE>
where
    V: VertexTrait + Clone + 'static,
    HE: HyperedgeTrait + Clone + 'static,
{
    /// Create a new temporal hypergraph.
    pub fn new() -> Self {
        Self {
            graph: Arc::new(StdRwLock::new(Hypergraph::new())),
            events: Arc::new(RwLock::new(EventLog::new())),
            snapshots: Arc::new(RwLock::new(BTreeMap::new())),
            clock: Clock::new(),
            snapshot_counter: AtomicU64::new(0),
        }
    }

    /// Create a temporal hypergraph wrapping an existing graph.
    pub fn wrap(graph: SharedGraph<V, HE>) -> Self {
        Self {
            graph,
            events: Arc::new(RwLock::new(EventLog::new())),
            snapshots: Arc::new(RwLock::new(BTreeMap::new())),
            clock: Clock::new(),
            snapshot_counter: AtomicU64::new(0),
        }
    }

    /// Get the current logical timestamp.
    pub fn current_timestamp(&self) -> Timestamp {
        self.clock.current()
    }

    /// Get a reference to the underlying graph.
    pub fn graph(&self) -> &SharedGraph<V, HE> {
        &self.graph
    }

    /// Get a clone of the shared graph.
    pub fn shared_graph(&self) -> SharedGraph<V, HE> {
        self.graph.clone()
    }

    /// Get a reference to the event log (for temporal queries).
    pub fn events_ref(&self) -> &Arc<RwLock<EventLog<V, HE>>> {
        &self.events
    }

    // =========================================================================
    // Mutation methods with event recording
    // =========================================================================

    /// Add a vertex and record the event.
    pub async fn add_vertex(&self, weight: V) -> TemporalResult<VertexIndex, V, HE> {
        let timestamp = self.clock.tick();
        let weight_clone = weight.clone();
        let graph = self.graph.clone();

        let index = EXEC
            .run_job(move || graph.write().unwrap().add_vertex(weight_clone))
            .await
            .map_err(TemporalError::from)?;

        self.events.write().await.append(TemporalEvent::VertexAdded {
            timestamp,
            index,
            weight,
        });

        Ok(index)
    }

    /// Remove a vertex and record the event.
    pub async fn remove_vertex(&self, index: VertexIndex) -> TemporalResult<(), V, HE> {
        let timestamp = self.clock.tick();
        let graph = self.graph.clone();

        // Get weight and remove atomically in single write lock
        let weight = EXEC
            .run_job(move || {
                let mut g = graph.write().unwrap();
                let weight = *g.get_vertex_weight(index)?;
                g.remove_vertex(index)?;
                Ok::<V, HypergraphError<V, HE>>(weight)
            })
            .await
            .map_err(TemporalError::from)?;

        self.events.write().await.append(TemporalEvent::VertexRemoved {
            timestamp,
            index,
            weight,
        });

        Ok(())
    }

    /// Update a vertex weight and record the event.
    pub async fn update_vertex_weight(
        &self,
        index: VertexIndex,
        new_weight: V,
    ) -> TemporalResult<(), V, HE> {
        let timestamp = self.clock.tick();
        let graph = self.graph.clone();
        let new_weight_clone = new_weight.clone();

        // Get old weight and update atomically in single write lock
        let old_weight = EXEC
            .run_job(move || {
                let mut g = graph.write().unwrap();
                let old_weight = *g.get_vertex_weight(index)?;
                g.update_vertex_weight(index, new_weight_clone)?;
                Ok::<V, HypergraphError<V, HE>>(old_weight)
            })
            .await
            .map_err(TemporalError::from)?;

        self.events.write().await.append(TemporalEvent::VertexWeightUpdated {
            timestamp,
            index,
            old_weight,
            new_weight,
        });

        Ok(())
    }

    /// Add a hyperedge and record the event.
    pub async fn add_hyperedge(
        &self,
        vertices: Vec<VertexIndex>,
        weight: HE,
    ) -> TemporalResult<HyperedgeIndex, V, HE> {
        let timestamp = self.clock.tick();
        let weight_clone = weight.clone();
        let vertices_clone = vertices.clone();
        let graph = self.graph.clone();

        let index = EXEC
            .run_job(move || graph.write().unwrap().add_hyperedge(vertices_clone, weight_clone))
            .await
            .map_err(TemporalError::from)?;

        self.events.write().await.append(TemporalEvent::HyperedgeAdded {
            timestamp,
            index,
            vertices,
            weight,
        });

        Ok(index)
    }

    /// Remove a hyperedge and record the event.
    pub async fn remove_hyperedge(&self, index: HyperedgeIndex) -> TemporalResult<(), V, HE> {
        let timestamp = self.clock.tick();
        let graph = self.graph.clone();

        // Get info and remove atomically in single write lock
        let (vertices, weight) = EXEC
            .run_job(move || {
                let mut g = graph.write().unwrap();
                let vertices = g.get_hyperedge_vertices(index)?;
                let weight = *g.get_hyperedge_weight(index)?;
                g.remove_hyperedge(index)?;
                Ok::<(Vec<VertexIndex>, HE), HypergraphError<V, HE>>((vertices, weight))
            })
            .await
            .map_err(TemporalError::from)?;

        self.events.write().await.append(TemporalEvent::HyperedgeRemoved {
            timestamp,
            index,
            vertices,
            weight,
        });

        Ok(())
    }

    /// Update a hyperedge weight and record the event.
    pub async fn update_hyperedge_weight(
        &self,
        index: HyperedgeIndex,
        new_weight: HE,
    ) -> TemporalResult<(), V, HE> {
        let timestamp = self.clock.tick();
        let graph = self.graph.clone();
        let new_weight_clone = new_weight.clone();

        // Get old weight and update atomically in single write lock
        let old_weight = EXEC
            .run_job(move || {
                let mut g = graph.write().unwrap();
                let old_weight = *g.get_hyperedge_weight(index)?;
                g.update_hyperedge_weight(index, new_weight_clone)?;
                Ok::<HE, HypergraphError<V, HE>>(old_weight)
            })
            .await
            .map_err(TemporalError::from)?;

        self.events.write().await.append(TemporalEvent::HyperedgeWeightUpdated {
            timestamp,
            index,
            old_weight,
            new_weight,
        });

        Ok(())
    }

    /// Update a hyperedge's vertices and record the event.
    pub async fn update_hyperedge_vertices(
        &self,
        index: HyperedgeIndex,
        new_vertices: Vec<VertexIndex>,
    ) -> TemporalResult<(), V, HE> {
        let timestamp = self.clock.tick();
        let graph = self.graph.clone();
        let new_vertices_clone = new_vertices.clone();

        // Get old vertices and update atomically in single write lock
        let old_vertices = EXEC
            .run_job(move || {
                let mut g = graph.write().unwrap();
                let old_vertices = g.get_hyperedge_vertices(index)?;
                g.update_hyperedge_vertices(index, new_vertices_clone)?;
                Ok::<Vec<VertexIndex>, HypergraphError<V, HE>>(old_vertices)
            })
            .await
            .map_err(TemporalError::from)?;

        self.events
            .write()
            .await
            .append(TemporalEvent::HyperedgeVerticesUpdated {
                timestamp,
                index,
                old_vertices,
                new_vertices,
            });

        Ok(())
    }

    /// Reverse a hyperedge and record the event.
    pub async fn reverse_hyperedge(
        &self,
        index: HyperedgeIndex,
    ) -> TemporalResult<Vec<VertexIndex>, V, HE> {
        let timestamp = self.clock.tick();
        let graph = self.graph.clone();

        // Get old vertices, reverse, get new vertices atomically in single write lock
        let (old_vertices, new_vertices) = EXEC
            .run_job(move || {
                let mut g = graph.write().unwrap();
                let old_vertices = g.get_hyperedge_vertices(index)?;
                g.reverse_hyperedge(index)?;
                let new_vertices = g.get_hyperedge_vertices(index)?;
                Ok::<(Vec<VertexIndex>, Vec<VertexIndex>), HypergraphError<V, HE>>((
                    old_vertices,
                    new_vertices,
                ))
            })
            .await
            .map_err(TemporalError::from)?;

        self.events.write().await.append(TemporalEvent::HyperedgeReversed {
            timestamp,
            index,
            old_vertices,
            new_vertices: new_vertices.clone(),
        });

        Ok(new_vertices)
    }

    /// Join hyperedges and record the event.
    pub async fn join_hyperedges(&self, indices: Vec<HyperedgeIndex>) -> TemporalResult<(), V, HE> {
        if indices.len() < 2 {
            return Err(TemporalError::Hypergraph(HypergraphError::HyperedgesInvalidJoin));
        }

        let timestamp = self.clock.tick();
        let graph = self.graph.clone();
        let target_index = indices[0];
        let source_indices: Vec<HyperedgeIndex> = indices[1..].to_vec();
        let indices_clone = indices.clone();

        // Join and get new vertices atomically in single write lock
        let new_vertices = EXEC
            .run_job(move || {
                let mut g = graph.write().unwrap();
                g.join_hyperedges(&indices_clone)?;
                let new_vertices = g.get_hyperedge_vertices(target_index)?;
                Ok::<Vec<VertexIndex>, HypergraphError<V, HE>>(new_vertices)
            })
            .await
            .map_err(TemporalError::from)?;

        self.events.write().await.append(TemporalEvent::HyperedgesJoined {
            timestamp,
            target_index,
            source_indices,
            new_vertices,
        });

        Ok(())
    }

    /// Contract vertices in a hyperedge and record the event.
    pub async fn contract_hyperedge_vertices(
        &self,
        hyperedge_index: HyperedgeIndex,
        vertices: Vec<VertexIndex>,
        target: VertexIndex,
    ) -> TemporalResult<Vec<VertexIndex>, V, HE> {
        let timestamp = self.clock.tick();
        let graph = self.graph.clone();
        let vertices_clone = vertices.clone();

        // Get old vertices, contract, all atomically in single write lock
        let (old_vertices, new_vertices) = EXEC
            .run_job(move || {
                let mut g = graph.write().unwrap();
                let old_vertices = g.get_hyperedge_vertices(hyperedge_index)?;
                let new_vertices =
                    g.contract_hyperedge_vertices(hyperedge_index, vertices_clone, target)?;
                Ok::<(Vec<VertexIndex>, Vec<VertexIndex>), HypergraphError<V, HE>>((
                    old_vertices,
                    new_vertices,
                ))
            })
            .await
            .map_err(TemporalError::from)?;

        self.events.write().await.append(TemporalEvent::VerticesContracted {
            timestamp,
            hyperedge_index,
            contracted_vertices: vertices,
            target_vertex: target,
            old_vertices,
            new_vertices: new_vertices.clone(),
        });

        Ok(new_vertices)
    }

    /// Clear all hyperedges and record the event.
    pub async fn clear_hyperedges(&self) -> TemporalResult<(), V, HE> {
        let timestamp = self.clock.tick();
        let graph = self.graph.clone();

        // Get count and clear atomically in single write lock
        let count = EXEC
            .run_job(move || {
                let mut g = graph.write().unwrap();
                let count = g.count_hyperedges();
                g.clear_hyperedges()?;
                Ok::<usize, HypergraphError<V, HE>>(count)
            })
            .await
            .map_err(TemporalError::from)?;

        self.events
            .write()
            .await
            .append(TemporalEvent::HyperedgesCleared { timestamp, count });

        Ok(())
    }

    /// Clear the entire graph and record the event.
    pub async fn clear(&self) -> TemporalResult<(), V, HE> {
        let timestamp = self.clock.tick();
        let graph = self.graph.clone();

        // Get counts and clear atomically in single write lock
        let (vertex_count, hyperedge_count) = EXEC
            .run_job(move || {
                let mut g = graph.write().unwrap();
                let vertex_count = g.count_vertices();
                let hyperedge_count = g.count_hyperedges();
                g.clear();
                (vertex_count, hyperedge_count)
            })
            .await;

        self.events.write().await.append(TemporalEvent::GraphCleared {
            timestamp,
            vertex_count,
            hyperedge_count,
        });

        Ok(())
    }

    // =========================================================================
    // Read operations (no event recording needed)
    // =========================================================================

    /// Get the vertex weight.
    pub async fn get_vertex_weight(
        &self,
        index: VertexIndex,
    ) -> TemporalResult<V, V, HE> {
        let graph = self.graph.clone();
        EXEC.run_job(move || graph.read().unwrap().get_vertex_weight(index).copied())
            .await
            .map_err(TemporalError::from)
    }

    /// Get the hyperedge weight.
    pub async fn get_hyperedge_weight(
        &self,
        index: HyperedgeIndex,
    ) -> TemporalResult<HE, V, HE> {
        let graph = self.graph.clone();
        EXEC.run_job(move || graph.read().unwrap().get_hyperedge_weight(index).copied())
            .await
            .map_err(TemporalError::from)
    }

    /// Get the vertices of a hyperedge.
    pub async fn get_hyperedge_vertices(
        &self,
        index: HyperedgeIndex,
    ) -> TemporalResult<Vec<VertexIndex>, V, HE> {
        let graph = self.graph.clone();
        EXEC.run_job(move || graph.read().unwrap().get_hyperedge_vertices(index))
            .await
            .map_err(TemporalError::from)
    }

    /// Count vertices.
    pub async fn count_vertices(&self) -> usize {
        let graph = self.graph.clone();
        EXEC.run_job(move || graph.read().unwrap().count_vertices())
            .await
    }

    /// Count hyperedges.
    pub async fn count_hyperedges(&self) -> usize {
        let graph = self.graph.clone();
        EXEC.run_job(move || graph.read().unwrap().count_hyperedges())
            .await
    }

    // =========================================================================
    // Snapshot methods
    // =========================================================================

    /// Create a snapshot at the current timestamp.
    pub async fn create_snapshot(&self) -> SnapshotId {
        let timestamp = self.clock.tick();
        let id = SnapshotId(self.snapshot_counter.fetch_add(1, Ordering::SeqCst));

        let event_index = self.events.read().await.len();

        let snapshot = Snapshot {
            id,
            timestamp,
            event_index,
        };

        self.snapshots.write().await.insert(timestamp, snapshot);

        self.events
            .write()
            .await
            .append(TemporalEvent::SnapshotMarker {
                timestamp,
                snapshot_id: id,
            });

        id
    }

    /// List all snapshots.
    pub async fn list_snapshots(&self) -> Vec<(Timestamp, SnapshotId)> {
        self.snapshots
            .read()
            .await
            .iter()
            .map(|(&ts, s)| (ts, s.id))
            .collect()
    }

    /// Get a snapshot by ID.
    pub async fn get_snapshot(&self, id: SnapshotId) -> Option<Snapshot> {
        self.snapshots
            .read()
            .await
            .values()
            .find(|s| s.id == id)
            .cloned()
    }

    // =========================================================================
    // Event query methods
    // =========================================================================

    /// Get all events.
    pub async fn events(&self) -> Vec<TemporalEvent<V, HE>> {
        self.events.read().await.events().to_vec()
    }

    /// Get events in a time range.
    pub async fn events_in_range(&self, range: &TimeRange) -> Vec<TemporalEvent<V, HE>> {
        self.events
            .read()
            .await
            .events_in_range(range)
            .into_iter()
            .cloned()
            .collect()
    }

    /// Get events for a vertex.
    pub async fn vertex_history(&self, vertex: VertexIndex) -> Vec<TemporalEvent<V, HE>> {
        self.events
            .read()
            .await
            .vertex_events(vertex)
            .into_iter()
            .cloned()
            .collect()
    }

    /// Get events for a hyperedge.
    pub async fn hyperedge_history(&self, hyperedge: HyperedgeIndex) -> Vec<TemporalEvent<V, HE>> {
        self.events
            .read()
            .await
            .hyperedge_events(hyperedge)
            .into_iter()
            .cloned()
            .collect()
    }

    /// Get event statistics.
    pub async fn event_stats(&self) -> super::events::EventStats {
        self.events.read().await.stats()
    }

    /// Get the total number of events.
    pub async fn event_count(&self) -> usize {
        self.events.read().await.len()
    }
}

impl<V, HE> Default for TemporalHypergraph<V, HE>
where
    V: VertexTrait + Clone + 'static,
    HE: HyperedgeTrait + Clone + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<V, HE> Clone for TemporalHypergraph<V, HE>
where
    V: VertexTrait + Clone + 'static,
    HE: HyperedgeTrait + Clone + 'static,
{
    fn clone(&self) -> Self {
        Self {
            graph: self.graph.clone(),
            events: self.events.clone(),
            snapshots: self.snapshots.clone(),
            clock: Clock::starting_at(self.clock.current().value()),
            snapshot_counter: AtomicU64::new(
                self.snapshot_counter.load(Ordering::SeqCst),
            ),
        }
    }
}
