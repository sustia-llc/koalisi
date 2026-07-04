//! The core TemporalHypergraph wrapper providing event sourcing.

use super::errors::{TemporalError, TemporalResult};
use super::event_log::EventLog;
use super::events::{SnapshotId, TemporalEvent};
use super::executor::EXEC;
use super::timestamp::{Clock, TimeRange, Timestamp};
use super::{HyperedgeTrait, VertexTrait};
use catgraph_applied::{HyperedgeIndex, Hypergraph, HypergraphError, VertexIndex};

pub type SharedGraph<V, HE> = std::sync::Arc<std::sync::RwLock<Hypergraph<V, HE>>>;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::RwLock as StdRwLock;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::RwLock;
use tokio::sync::mpsc;

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
    /// Optional event tap (Phase 7 / P7.2, sustia-llc/koalisi#30): when set,
    /// every mutation recorded *after* installation mirrors its
    /// [`TemporalEvent`] here for a downstream durability forwarder. `None` by
    /// default; installed via [`with_event_tap`](Self::with_event_tap). `Clone`
    /// SHARES the tap (the `Option<Sender>` is cloned), consistent with sharing
    /// the `events` Arc.
    event_tap: Option<mpsc::Sender<TemporalEvent<V, HE>>>,
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
            event_tap: None,
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
            event_tap: None,
        }
    }

    /// Install an event tap (Phase 7 / P7.2, sustia-llc/koalisi#30).
    ///
    /// After this call, every mutation this hypergraph records mirrors its
    /// [`TemporalEvent`] to `tap` in addition to appending it to the in-memory
    /// event log. Delivery is best-effort and non-blocking (a full or closed
    /// tap logs and drops the event — the at-most-once tap contract; a slow or
    /// absent forwarder never stalls or fails a mutation).
    ///
    /// Only mutations recorded *after* the tap is installed are tapped, so set
    /// it **before** the first mutation if a downstream consumer needs the full
    /// history (the P7.2 replay-parity tests do). `Clone` shares the tap.
    #[must_use]
    pub fn with_event_tap(mut self, tap: mpsc::Sender<TemporalEvent<V, HE>>) -> Self {
        self.event_tap = Some(tap);
        self
    }

    /// Mirror `event` to the installed tap, if any.
    ///
    /// Best-effort [`try_send`](mpsc::Sender::try_send): a full or closed tap
    /// logs and drops (the at-most-once tap contract, CLAUDE.md gotchas 14/17) —
    /// it never blocks or fails the mutation. The `event.clone()` happens ONLY
    /// when a tap is installed; with no tap this is a single `Option` check.
    fn tap_event(&self, event: &TemporalEvent<V, HE>) {
        let Some(tap) = &self.event_tap else {
            return;
        };
        match tap.try_send(event.clone()) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                tracing::warn!(
                    event_type = event.event_type(),
                    "topology event tap full — dropping event (mutation unaffected)"
                );
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                tracing::warn!(
                    event_type = event.event_type(),
                    "topology event tap closed — dropping event (mutation unaffected)"
                );
            }
        }
    }

    /// Append `event` to the log and mirror it to the tap (if installed).
    ///
    /// The single append site every mutation method routes through. When no tap
    /// is installed this is byte-for-byte the previous inline
    /// `self.events.write().await.append(event)` — the tap clone happens only
    /// under [`tap_event`](Self::tap_event) when a sender is present, so the
    /// default (untapped) behaviour, ordering, and lock scope are unchanged.
    ///
    /// The tap fires while the events write guard is held: with concurrent
    /// mutators (`Clone` shares the log and the tap), tap order is therefore
    /// always identical to log order — the property `replay_into_event_log`'s
    /// "same events, same order" guarantee rests on. `try_send` never blocks,
    /// so the extra time under the guard is negligible.
    async fn record_event(&self, event: TemporalEvent<V, HE>) {
        let mut events = self.events.write().await;
        self.tap_event(&event);
        events.append(event);
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
    pub async fn add_vertex(&self, weight: V) -> TemporalResult<VertexIndex> {
        let timestamp = self.clock.tick();
        let weight_clone = weight;
        let graph = self.graph.clone();

        // K1: `add_vertex` is infallible on the catgraph backend (returns the
        // fresh `VertexIndex`, not a `Result`).
        let index = EXEC
            .run_job(move || graph.write().unwrap().add_vertex(weight_clone))
            .await;

        self.record_event(TemporalEvent::VertexAdded {
            timestamp,
            index,
            weight,
        })
        .await;

        Ok(index)
    }

    /// Remove a vertex and record the event.
    pub async fn remove_vertex(&self, index: VertexIndex) -> TemporalResult<()> {
        let timestamp = self.clock.tick();
        let graph = self.graph.clone();

        // Get weight and remove atomically in single write lock
        let weight = EXEC
            .run_job(move || {
                let mut g = graph.write().unwrap();
                let weight = g.get_vertex_weight(index)?;
                g.remove_vertex(index)?;
                Ok::<V, HypergraphError>(weight)
            })
            .await
            .map_err(TemporalError::from)?;

        self.record_event(TemporalEvent::VertexRemoved {
            timestamp,
            index,
            weight,
        })
        .await;

        Ok(())
    }

    /// Update a vertex weight and record the event.
    pub async fn update_vertex_weight(
        &self,
        index: VertexIndex,
        new_weight: V,
    ) -> TemporalResult<()> {
        let timestamp = self.clock.tick();
        let graph = self.graph.clone();
        let new_weight_clone = new_weight;

        // Get old weight and update atomically in single write lock
        let old_weight = EXEC
            .run_job(move || {
                let mut g = graph.write().unwrap();
                let old_weight = g.get_vertex_weight(index)?;
                g.update_vertex_weight(index, new_weight_clone)?;
                Ok::<V, HypergraphError>(old_weight)
            })
            .await
            .map_err(TemporalError::from)?;

        self.record_event(TemporalEvent::VertexWeightUpdated {
            timestamp,
            index,
            old_weight,
            new_weight,
        })
        .await;

        Ok(())
    }

    /// Add a hyperedge and record the event.
    pub async fn add_hyperedge(
        &self,
        vertices: Vec<VertexIndex>,
        weight: HE,
    ) -> TemporalResult<HyperedgeIndex> {
        let timestamp = self.clock.tick();
        let weight_clone = weight;
        let vertices_clone = vertices.clone();
        let graph = self.graph.clone();

        let index = EXEC
            .run_job(move || {
                graph
                    .write()
                    .unwrap()
                    .add_hyperedge(vertices_clone, weight_clone)
            })
            .await
            .map_err(TemporalError::from)?;

        self.record_event(TemporalEvent::HyperedgeAdded {
            timestamp,
            index,
            vertices,
            weight,
        })
        .await;

        Ok(index)
    }

    /// Remove a hyperedge and record the event.
    pub async fn remove_hyperedge(&self, index: HyperedgeIndex) -> TemporalResult<()> {
        let timestamp = self.clock.tick();
        let graph = self.graph.clone();

        // Get info and remove atomically in single write lock
        let (vertices, weight) = EXEC
            .run_job(move || {
                let mut g = graph.write().unwrap();
                let vertices = g.get_hyperedge_vertices(index)?;
                let weight = g.get_hyperedge_weight(index)?;
                g.remove_hyperedge(index)?;
                Ok::<(Vec<VertexIndex>, HE), HypergraphError>((vertices, weight))
            })
            .await
            .map_err(TemporalError::from)?;

        self.record_event(TemporalEvent::HyperedgeRemoved {
            timestamp,
            index,
            vertices,
            weight,
        })
        .await;

        Ok(())
    }

    /// Update a hyperedge weight and record the event.
    pub async fn update_hyperedge_weight(
        &self,
        index: HyperedgeIndex,
        new_weight: HE,
    ) -> TemporalResult<()> {
        let timestamp = self.clock.tick();
        let graph = self.graph.clone();
        let new_weight_clone = new_weight;

        // Get old weight and update atomically in single write lock
        let old_weight = EXEC
            .run_job(move || {
                let mut g = graph.write().unwrap();
                let old_weight = g.get_hyperedge_weight(index)?;
                g.update_hyperedge_weight(index, new_weight_clone)?;
                Ok::<HE, HypergraphError>(old_weight)
            })
            .await
            .map_err(TemporalError::from)?;

        self.record_event(TemporalEvent::HyperedgeWeightUpdated {
            timestamp,
            index,
            old_weight,
            new_weight,
        })
        .await;

        Ok(())
    }

    /// Update a hyperedge's vertices and record the event.
    pub async fn update_hyperedge_vertices(
        &self,
        index: HyperedgeIndex,
        new_vertices: Vec<VertexIndex>,
    ) -> TemporalResult<()> {
        let timestamp = self.clock.tick();
        let graph = self.graph.clone();
        let new_vertices_clone = new_vertices.clone();

        // Get old vertices and update atomically in single write lock
        let old_vertices = EXEC
            .run_job(move || {
                let mut g = graph.write().unwrap();
                let old_vertices = g.get_hyperedge_vertices(index)?;
                g.update_hyperedge_vertices(index, new_vertices_clone)?;
                Ok::<Vec<VertexIndex>, HypergraphError>(old_vertices)
            })
            .await
            .map_err(TemporalError::from)?;

        self.record_event(TemporalEvent::HyperedgeVerticesUpdated {
            timestamp,
            index,
            old_vertices,
            new_vertices,
        })
        .await;

        Ok(())
    }

    /// Read-modify-write a hyperedge's vertex set atomically.
    ///
    /// The mutator runs inside the same write-lock as the read of `old_vertices`
    /// and the write of the new set, so two concurrent join/leave calls cannot
    /// race. The mutator may return `Err` to abort the update (e.g. a coalition
    /// leave that would leave the membership empty).
    pub async fn update_hyperedge_vertices_try<F>(
        &self,
        index: HyperedgeIndex,
        mutator: F,
    ) -> TemporalResult<()>
    where
        F: FnOnce(Vec<VertexIndex>) -> Result<Vec<VertexIndex>, TemporalError> + Send + 'static,
    {
        let timestamp = self.clock.tick();
        let graph = self.graph.clone();

        let (old_vertices, new_vertices) = EXEC
            .run_job(move || {
                let mut g = graph.write().unwrap();
                let old_vertices = g
                    .get_hyperedge_vertices(index)
                    .map_err(TemporalError::from)?;
                let new_vertices = mutator(old_vertices.clone())?;
                g.update_hyperedge_vertices(index, new_vertices.clone())
                    .map_err(TemporalError::from)?;
                Ok::<_, TemporalError>((old_vertices, new_vertices))
            })
            .await?;

        self.record_event(TemporalEvent::HyperedgeVerticesUpdated {
            timestamp,
            index,
            old_vertices,
            new_vertices,
        })
        .await;

        Ok(())
    }

    /// Reverse a hyperedge and record the event.
    pub async fn reverse_hyperedge(
        &self,
        index: HyperedgeIndex,
    ) -> TemporalResult<Vec<VertexIndex>> {
        let timestamp = self.clock.tick();
        let graph = self.graph.clone();

        // Get old vertices, reverse, get new vertices atomically in single write lock
        let (old_vertices, new_vertices) = EXEC
            .run_job(move || {
                let mut g = graph.write().unwrap();
                let old_vertices = g.get_hyperedge_vertices(index)?;
                g.reverse_hyperedge(index)?;
                let new_vertices = g.get_hyperedge_vertices(index)?;
                Ok::<(Vec<VertexIndex>, Vec<VertexIndex>), HypergraphError>((
                    old_vertices,
                    new_vertices,
                ))
            })
            .await
            .map_err(TemporalError::from)?;

        self.record_event(TemporalEvent::HyperedgeReversed {
            timestamp,
            index,
            old_vertices,
            new_vertices: new_vertices.clone(),
        })
        .await;

        Ok(new_vertices)
    }

    /// Join hyperedges and record the event.
    pub async fn join_hyperedges(&self, indices: Vec<HyperedgeIndex>) -> TemporalResult<()> {
        if indices.len() < 2 {
            return Err(TemporalError::Hypergraph(HypergraphError::InvalidJoin));
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
                Ok::<Vec<VertexIndex>, HypergraphError>(new_vertices)
            })
            .await
            .map_err(TemporalError::from)?;

        self.record_event(TemporalEvent::HyperedgesJoined {
            timestamp,
            target_index,
            source_indices,
            new_vertices,
        })
        .await;

        Ok(())
    }

    /// Contract vertices in a hyperedge and record the event.
    pub async fn contract_hyperedge_vertices(
        &self,
        hyperedge_index: HyperedgeIndex,
        vertices: Vec<VertexIndex>,
        target: VertexIndex,
    ) -> TemporalResult<Vec<VertexIndex>> {
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
                Ok::<(Vec<VertexIndex>, Vec<VertexIndex>), HypergraphError>((
                    old_vertices,
                    new_vertices,
                ))
            })
            .await
            .map_err(TemporalError::from)?;

        self.record_event(TemporalEvent::VerticesContracted {
            timestamp,
            hyperedge_index,
            contracted_vertices: vertices,
            target_vertex: target,
            old_vertices,
            new_vertices: new_vertices.clone(),
        })
        .await;

        Ok(new_vertices)
    }

    /// Clear all hyperedges and record the event.
    pub async fn clear_hyperedges(&self) -> TemporalResult<()> {
        let timestamp = self.clock.tick();
        let graph = self.graph.clone();

        // Get count and clear atomically in single write lock.
        // K1: `clear_hyperedges` is infallible on the catgraph backend (returns
        // `()`, not `Result`), so the closure yields the count directly.
        let count = EXEC
            .run_job(move || {
                let mut g = graph.write().unwrap();
                let count = g.count_hyperedges();
                g.clear_hyperedges();
                count
            })
            .await;

        self.record_event(TemporalEvent::HyperedgesCleared { timestamp, count })
            .await;

        Ok(())
    }

    /// Clear the entire graph and record the event.
    pub async fn clear(&self) -> TemporalResult<()> {
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

        self.record_event(TemporalEvent::GraphCleared {
            timestamp,
            vertex_count,
            hyperedge_count,
        })
        .await;

        Ok(())
    }

    // =========================================================================
    // Read operations (no event recording needed)
    // =========================================================================

    /// Get the vertex weight.
    pub async fn get_vertex_weight(&self, index: VertexIndex) -> TemporalResult<V> {
        let graph = self.graph.clone();
        EXEC.run_job(move || graph.read().unwrap().get_vertex_weight(index))
            .await
            .map_err(TemporalError::from)
    }

    /// Get the hyperedge weight.
    pub async fn get_hyperedge_weight(&self, index: HyperedgeIndex) -> TemporalResult<HE> {
        let graph = self.graph.clone();
        EXEC.run_job(move || graph.read().unwrap().get_hyperedge_weight(index))
            .await
            .map_err(TemporalError::from)
    }

    /// Get the vertices of a hyperedge.
    pub async fn get_hyperedge_vertices(
        &self,
        index: HyperedgeIndex,
    ) -> TemporalResult<Vec<VertexIndex>> {
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
    ///
    /// `event_index` points to the SnapshotMarker itself, so replay-to-snapshot
    /// includes the marker as its terminator. The marker append and the snapshots
    /// table insert happen under a single events-log write guard to keep the
    /// stored index consistent under concurrent mutation.
    pub async fn create_snapshot(&self) -> SnapshotId {
        let timestamp = self.clock.tick();
        let id = SnapshotId(self.snapshot_counter.fetch_add(1, Ordering::Relaxed));
        let marker = TemporalEvent::SnapshotMarker {
            timestamp,
            snapshot_id: id,
        };

        // The marker is a tapped record too (replay uses it as an accelerator).
        // Tap under the events guard — same discipline as `record_event`, so
        // tap order always matches log order — and compute the stored index
        // under the same guard to keep it consistent under concurrent mutation.
        let event_index = {
            let mut events = self.events.write().await;
            let index = events.len();
            self.tap_event(&marker);
            events.append(marker);
            index
        };

        let snapshot = Snapshot {
            id,
            timestamp,
            event_index,
        };
        self.snapshots.write().await.insert(timestamp, snapshot);

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
            snapshot_counter: AtomicU64::new(self.snapshot_counter.load(Ordering::Acquire)),
            // Share the tap (clone the Option<Sender>), consistent with sharing
            // the events Arc above.
            event_tap: self.event_tap.clone(),
        }
    }
}
