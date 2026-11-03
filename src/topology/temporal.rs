//! The core TemporalHypergraph wrapper providing event sourcing.

use super::errors::{TemporalError, TemporalResult};
use super::event_log::EventLog;
use super::events::{SnapshotId, TemporalEvent};
use super::timestamp::{Clock, TimeRange, Timestamp};
use super::{HyperedgeTrait, VertexTrait};
use crate::core::Executor;
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

/// A temporal hypergraph with full event sourcing: it wraps a SharedGraph and
/// records every mutation as an event, so the graph state at any point in
/// time can be reconstructed. Each mutation method runs the graph change and
/// its event append as one spawned tokio task, so it must be called inside a
/// tokio runtime, and a mutation future dropped after its first poll still
/// applies the change and records its event. Every graph operation, mutation
/// or read, returns [`TemporalError::LockPoisoned`] once the graph lock is
/// poisoned.
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
    /// The pool every graph operation runs on. `Clone` shares it.
    executor: Arc<Executor>,
}

/// Mirror `event` to `tap`, if any, with a non-blocking `try_send`; a full or
/// closed tap logs the event type at warn and drops the event.
fn tap_event<V, HE>(tap: Option<&mpsc::Sender<TemporalEvent<V, HE>>>, event: &TemporalEvent<V, HE>)
where
    V: VertexTrait + Clone + 'static,
    HE: HyperedgeTrait + Clone + 'static,
{
    let Some(tap) = tap else {
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

/// Append `event` to `events` and mirror it to `tap`, if any, while holding
/// the events write guard, so tap order equals log order for every set of
/// concurrent mutators sharing the log and the tap.
async fn record_event<V, HE>(
    events: &RwLock<EventLog<V, HE>>,
    tap: Option<&mpsc::Sender<TemporalEvent<V, HE>>>,
    event: TemporalEvent<V, HE>,
) where
    V: VertexTrait + Clone + 'static,
    HE: HyperedgeTrait + Clone + 'static,
{
    let mut events = events.write().await;
    tap_event(tap, &event);
    events.append(event);
}

impl<V, HE> TemporalHypergraph<V, HE>
where
    V: VertexTrait + Clone + 'static,
    HE: HyperedgeTrait + Clone + 'static,
{
    /// Create a new temporal hypergraph whose graph operations run on
    /// `executor`.
    pub fn new(executor: Arc<Executor>) -> Self {
        Self::wrap(Arc::new(StdRwLock::new(Hypergraph::new())), executor)
    }

    /// Create a temporal hypergraph wrapping an existing graph, whose graph
    /// operations run on `executor`.
    pub fn wrap(graph: SharedGraph<V, HE>, executor: Arc<Executor>) -> Self {
        Self {
            graph,
            events: Arc::new(RwLock::new(EventLog::new())),
            snapshots: Arc::new(RwLock::new(BTreeMap::new())),
            clock: Clock::new(),
            snapshot_counter: AtomicU64::new(0),
            event_tap: None,
            executor,
        }
    }

    /// The executor this hypergraph's graph operations run on.
    pub fn executor(&self) -> &Arc<Executor> {
        &self.executor
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

    /// Run `job` on the executor under the graph's write lock, then append the
    /// event `finish` builds from the job's output, both inside one spawned
    /// tokio task, and resolve to the value `finish` returns. The task runs to
    /// completion when the returned future is dropped after its first poll. A
    /// poisoned lock yields [`TemporalError::LockPoisoned`] and records
    /// nothing, a `job` error records nothing, and a panic in `job` resumes on
    /// the awaiting task.
    async fn mutate<T, R>(
        &self,
        job: impl FnOnce(&mut Hypergraph<V, HE>) -> TemporalResult<T> + Send + 'static,
        finish: impl FnOnce(T) -> (TemporalEvent<V, HE>, R) + Send + 'static,
    ) -> TemporalResult<R>
    where
        T: Send + 'static,
        R: Send + 'static,
    {
        let graph = Arc::clone(&self.graph);
        let events = Arc::clone(&self.events);
        let tap = self.event_tap.clone();
        let executor = Arc::clone(&self.executor);
        let task = tokio::spawn(async move {
            let output = executor
                .run(move || {
                    let mut g = graph.write().map_err(|_| TemporalError::LockPoisoned)?;
                    job(&mut g)
                })
                .await?;
            let (event, value) = finish(output);
            record_event(&events, tap.as_ref(), event).await;
            Ok(value)
        });
        match task.await {
            Ok(result) => result,
            Err(e) => match e.try_into_panic() {
                Ok(payload) => std::panic::resume_unwind(payload),
                Err(e) => panic!(
                    "invariant: a graph task is never aborted and the runtime outlives every task awaiting one: {e}"
                ),
            },
        }
    }

    /// Run `job` on the executor under the graph's read lock and resolve to
    /// its result; a poisoned lock yields [`TemporalError::LockPoisoned`].
    async fn read<T>(
        &self,
        job: impl FnOnce(&Hypergraph<V, HE>) -> TemporalResult<T> + Send + 'static,
    ) -> TemporalResult<T>
    where
        T: Send + 'static,
    {
        let graph = Arc::clone(&self.graph);
        self.executor
            .run(move || {
                let g = graph.read().map_err(|_| TemporalError::LockPoisoned)?;
                job(&g)
            })
            .await
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

    /// Add a vertex and record the event, under the mutation contract of
    /// [`TemporalHypergraph`].
    pub async fn add_vertex(&self, weight: V) -> TemporalResult<VertexIndex> {
        let timestamp = self.clock.tick();
        // K1: `add_vertex` is infallible on the catgraph backend (returns the
        // fresh `VertexIndex`, not a `Result`).
        self.mutate(
            move |g| Ok(g.add_vertex(weight)),
            move |index| {
                (
                    TemporalEvent::VertexAdded {
                        timestamp,
                        index,
                        weight,
                    },
                    index,
                )
            },
        )
        .await
    }

    /// Remove a vertex and record the event, under the mutation contract of
    /// [`TemporalHypergraph`].
    pub async fn remove_vertex(&self, index: VertexIndex) -> TemporalResult<()> {
        let timestamp = self.clock.tick();
        self.mutate(
            move |g| {
                let weight = g.get_vertex_weight(index)?;
                g.remove_vertex(index)?;
                Ok(weight)
            },
            move |weight| {
                (
                    TemporalEvent::VertexRemoved {
                        timestamp,
                        index,
                        weight,
                    },
                    (),
                )
            },
        )
        .await
    }

    /// Update a vertex weight and record the event, under the mutation
    /// contract of [`TemporalHypergraph`].
    pub async fn update_vertex_weight(
        &self,
        index: VertexIndex,
        new_weight: V,
    ) -> TemporalResult<()> {
        let timestamp = self.clock.tick();
        self.mutate(
            move |g| {
                let old_weight = g.get_vertex_weight(index)?;
                g.update_vertex_weight(index, new_weight)?;
                Ok(old_weight)
            },
            move |old_weight| {
                (
                    TemporalEvent::VertexWeightUpdated {
                        timestamp,
                        index,
                        old_weight,
                        new_weight,
                    },
                    (),
                )
            },
        )
        .await
    }

    /// Add a hyperedge and record the event, under the mutation contract of
    /// [`TemporalHypergraph`].
    pub async fn add_hyperedge(
        &self,
        vertices: Vec<VertexIndex>,
        weight: HE,
    ) -> TemporalResult<HyperedgeIndex> {
        let timestamp = self.clock.tick();
        let vertices_clone = vertices.clone();
        self.mutate(
            move |g| Ok(g.add_hyperedge(vertices_clone, weight)?),
            move |index| {
                (
                    TemporalEvent::HyperedgeAdded {
                        timestamp,
                        index,
                        vertices,
                        weight,
                    },
                    index,
                )
            },
        )
        .await
    }

    /// Remove a hyperedge and record the event, under the mutation contract
    /// of [`TemporalHypergraph`].
    pub async fn remove_hyperedge(&self, index: HyperedgeIndex) -> TemporalResult<()> {
        let timestamp = self.clock.tick();
        self.mutate(
            move |g| {
                let vertices = g.get_hyperedge_vertices(index)?;
                let weight = g.get_hyperedge_weight(index)?;
                g.remove_hyperedge(index)?;
                Ok((vertices, weight))
            },
            move |(vertices, weight)| {
                (
                    TemporalEvent::HyperedgeRemoved {
                        timestamp,
                        index,
                        vertices,
                        weight,
                    },
                    (),
                )
            },
        )
        .await
    }

    /// Update a hyperedge weight and record the event, under the mutation
    /// contract of [`TemporalHypergraph`].
    pub async fn update_hyperedge_weight(
        &self,
        index: HyperedgeIndex,
        new_weight: HE,
    ) -> TemporalResult<()> {
        let timestamp = self.clock.tick();
        self.mutate(
            move |g| {
                let old_weight = g.get_hyperedge_weight(index)?;
                g.update_hyperedge_weight(index, new_weight)?;
                Ok(old_weight)
            },
            move |old_weight| {
                (
                    TemporalEvent::HyperedgeWeightUpdated {
                        timestamp,
                        index,
                        old_weight,
                        new_weight,
                    },
                    (),
                )
            },
        )
        .await
    }

    /// Update a hyperedge's vertices and record the event, under the mutation
    /// contract of [`TemporalHypergraph`].
    pub async fn update_hyperedge_vertices(
        &self,
        index: HyperedgeIndex,
        new_vertices: Vec<VertexIndex>,
    ) -> TemporalResult<()> {
        let timestamp = self.clock.tick();
        let new_vertices_clone = new_vertices.clone();
        self.mutate(
            move |g| {
                let old_vertices = g.get_hyperedge_vertices(index)?;
                g.update_hyperedge_vertices(index, new_vertices_clone)?;
                Ok(old_vertices)
            },
            move |old_vertices| {
                (
                    TemporalEvent::HyperedgeVerticesUpdated {
                        timestamp,
                        index,
                        old_vertices,
                        new_vertices,
                    },
                    (),
                )
            },
        )
        .await
    }

    /// Replace a hyperedge's vertex set with `mutator(old_vertices)` and
    /// record the event, with the read, the `mutator` call and the write under
    /// one graph write lock on an executor worker; a `mutator` `Err` aborts
    /// the update and records nothing, and a `mutator` panic poisons the lock,
    /// after which every graph operation returns
    /// [`TemporalError::LockPoisoned`]. A `mutator` that runs rayon work
    /// (`rayon::join`, `rayon::scope`, a parallel iterator) on that worker can
    /// deadlock: while the worker waits for the rayon work, it can take a
    /// queued graph job from the same pool, and that job blocks on the write
    /// lock the worker already holds. The mutation contract of
    /// [`TemporalHypergraph`] applies.
    pub async fn update_hyperedge_vertices_try<F>(
        &self,
        index: HyperedgeIndex,
        mutator: F,
    ) -> TemporalResult<()>
    where
        F: FnOnce(Vec<VertexIndex>) -> Result<Vec<VertexIndex>, TemporalError> + Send + 'static,
    {
        let timestamp = self.clock.tick();
        self.mutate(
            move |g| {
                let old_vertices = g.get_hyperedge_vertices(index)?;
                let new_vertices = mutator(old_vertices.clone())?;
                g.update_hyperedge_vertices(index, new_vertices.clone())?;
                Ok((old_vertices, new_vertices))
            },
            move |(old_vertices, new_vertices)| {
                (
                    TemporalEvent::HyperedgeVerticesUpdated {
                        timestamp,
                        index,
                        old_vertices,
                        new_vertices,
                    },
                    (),
                )
            },
        )
        .await
    }

    /// Reverse a hyperedge and record the event, under the mutation contract
    /// of [`TemporalHypergraph`].
    pub async fn reverse_hyperedge(
        &self,
        index: HyperedgeIndex,
    ) -> TemporalResult<Vec<VertexIndex>> {
        let timestamp = self.clock.tick();
        self.mutate(
            move |g| {
                let old_vertices = g.get_hyperedge_vertices(index)?;
                g.reverse_hyperedge(index)?;
                let new_vertices = g.get_hyperedge_vertices(index)?;
                Ok((old_vertices, new_vertices))
            },
            move |(old_vertices, new_vertices)| {
                (
                    TemporalEvent::HyperedgeReversed {
                        timestamp,
                        index,
                        old_vertices,
                        new_vertices: new_vertices.clone(),
                    },
                    new_vertices,
                )
            },
        )
        .await
    }

    /// Join hyperedges and record the event; fewer than two indices is
    /// [`HypergraphError::InvalidJoin`]. The mutation contract of
    /// [`TemporalHypergraph`] applies.
    pub async fn join_hyperedges(&self, indices: Vec<HyperedgeIndex>) -> TemporalResult<()> {
        if indices.len() < 2 {
            return Err(TemporalError::Hypergraph(HypergraphError::InvalidJoin));
        }

        let timestamp = self.clock.tick();
        let target_index = indices[0];
        let source_indices: Vec<HyperedgeIndex> = indices[1..].to_vec();
        self.mutate(
            move |g| {
                g.join_hyperedges(&indices)?;
                Ok(g.get_hyperedge_vertices(target_index)?)
            },
            move |new_vertices| {
                (
                    TemporalEvent::HyperedgesJoined {
                        timestamp,
                        target_index,
                        source_indices,
                        new_vertices,
                    },
                    (),
                )
            },
        )
        .await
    }

    /// Contract vertices in a hyperedge and record the event, under the
    /// mutation contract of [`TemporalHypergraph`].
    pub async fn contract_hyperedge_vertices(
        &self,
        hyperedge_index: HyperedgeIndex,
        vertices: Vec<VertexIndex>,
        target: VertexIndex,
    ) -> TemporalResult<Vec<VertexIndex>> {
        let timestamp = self.clock.tick();
        let vertices_clone = vertices.clone();
        self.mutate(
            move |g| {
                let old_vertices = g.get_hyperedge_vertices(hyperedge_index)?;
                let new_vertices =
                    g.contract_hyperedge_vertices(hyperedge_index, vertices_clone, target)?;
                Ok((old_vertices, new_vertices))
            },
            move |(old_vertices, new_vertices)| {
                (
                    TemporalEvent::VerticesContracted {
                        timestamp,
                        hyperedge_index,
                        contracted_vertices: vertices,
                        target_vertex: target,
                        old_vertices,
                        new_vertices: new_vertices.clone(),
                    },
                    new_vertices,
                )
            },
        )
        .await
    }

    /// Clear all hyperedges and record the event, under the mutation contract
    /// of [`TemporalHypergraph`].
    pub async fn clear_hyperedges(&self) -> TemporalResult<()> {
        let timestamp = self.clock.tick();
        // K1: `clear_hyperedges` is infallible on the catgraph backend (returns
        // `()`, not `Result`), so the job yields the count directly.
        self.mutate(
            move |g| {
                let count = g.count_hyperedges();
                g.clear_hyperedges();
                Ok(count)
            },
            move |count| (TemporalEvent::HyperedgesCleared { timestamp, count }, ()),
        )
        .await
    }

    /// Clear the entire graph and record the event, under the mutation
    /// contract of [`TemporalHypergraph`].
    pub async fn clear(&self) -> TemporalResult<()> {
        let timestamp = self.clock.tick();
        self.mutate(
            move |g| {
                let vertex_count = g.count_vertices();
                let hyperedge_count = g.count_hyperedges();
                g.clear();
                Ok((vertex_count, hyperedge_count))
            },
            move |(vertex_count, hyperedge_count)| {
                (
                    TemporalEvent::GraphCleared {
                        timestamp,
                        vertex_count,
                        hyperedge_count,
                    },
                    (),
                )
            },
        )
        .await
    }

    // =========================================================================
    // Read operations (no event recording needed)
    // =========================================================================

    /// Get the vertex weight.
    pub async fn get_vertex_weight(&self, index: VertexIndex) -> TemporalResult<V> {
        self.read(move |g| Ok(g.get_vertex_weight(index)?)).await
    }

    /// Get the hyperedge weight.
    pub async fn get_hyperedge_weight(&self, index: HyperedgeIndex) -> TemporalResult<HE> {
        self.read(move |g| Ok(g.get_hyperedge_weight(index)?)).await
    }

    /// Get the vertices of a hyperedge.
    pub async fn get_hyperedge_vertices(
        &self,
        index: HyperedgeIndex,
    ) -> TemporalResult<Vec<VertexIndex>> {
        self.read(move |g| Ok(g.get_hyperedge_vertices(index)?))
            .await
    }

    /// Count vertices.
    pub async fn count_vertices(&self) -> TemporalResult<usize> {
        self.read(|g| Ok(g.count_vertices())).await
    }

    /// Count hyperedges.
    pub async fn count_hyperedges(&self) -> TemporalResult<usize> {
        self.read(|g| Ok(g.count_hyperedges())).await
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
            tap_event(self.event_tap.as_ref(), &marker);
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
            executor: Arc::clone(&self.executor),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topology::TemporalQueries;
    use std::time::Duration;

    /// After a mutator passed to `update_hyperedge_vertices_try` panics, each
    /// of the 18 graph operations returns `LockPoisoned` and none records an
    /// event.
    #[tokio::test]
    async fn a_mutator_panic_makes_every_graph_operation_return_lock_poisoned() {
        let g = TemporalHypergraph::<u32, u32>::new(crate::core::executor::test_executor());
        let v0 = g.add_vertex(1).await.expect("add v0");
        let v1 = g.add_vertex(2).await.expect("add v1");
        let a = g.add_hyperedge(vec![v0], 1).await.expect("add a");
        let b = g.add_hyperedge(vec![v1], 2).await.expect("add b");
        let events_before = g.event_count().await;

        let panicking = g.clone();
        let outcome = tokio::spawn(async move {
            panicking
                .update_hyperedge_vertices_try(
                    a,
                    |_: Vec<VertexIndex>| -> Result<Vec<VertexIndex>, TemporalError> {
                        panic!("mutator exploded")
                    },
                )
                .await
        })
        .await;
        assert!(
            outcome.as_ref().is_err_and(|e| e.is_panic()),
            "expected the mutator panic to resume on the awaiting task, observed {outcome:?}"
        );

        let outcomes: Vec<(&str, TemporalResult<()>)> = vec![
            ("add_vertex", g.add_vertex(3).await.map(drop)),
            ("remove_vertex", g.remove_vertex(v1).await),
            ("update_vertex_weight", g.update_vertex_weight(v0, 9).await),
            (
                "add_hyperedge",
                g.add_hyperedge(vec![v0], 3).await.map(drop),
            ),
            ("remove_hyperedge", g.remove_hyperedge(b).await),
            (
                "update_hyperedge_weight",
                g.update_hyperedge_weight(a, 9).await,
            ),
            (
                "update_hyperedge_vertices",
                g.update_hyperedge_vertices(a, vec![v0, v1]).await,
            ),
            (
                "update_hyperedge_vertices_try",
                g.update_hyperedge_vertices_try(a, Ok).await,
            ),
            ("reverse_hyperedge", g.reverse_hyperedge(a).await.map(drop)),
            ("join_hyperedges", g.join_hyperedges(vec![a, b]).await),
            (
                "contract_hyperedge_vertices",
                g.contract_hyperedge_vertices(a, vec![v0], v0)
                    .await
                    .map(drop),
            ),
            ("clear_hyperedges", g.clear_hyperedges().await),
            ("clear", g.clear().await),
            ("get_vertex_weight", g.get_vertex_weight(v0).await.map(drop)),
            (
                "get_hyperedge_weight",
                g.get_hyperedge_weight(a).await.map(drop),
            ),
            (
                "get_hyperedge_vertices",
                g.get_hyperedge_vertices(a).await.map(drop),
            ),
            ("count_vertices", g.count_vertices().await.map(drop)),
            ("count_hyperedges", g.count_hyperedges().await.map(drop)),
        ];
        let not_poisoned: Vec<String> = outcomes
            .iter()
            .filter(|(_, r)| !matches!(r, Err(TemporalError::LockPoisoned)))
            .map(|(op, r)| format!("{op}: {r:?}"))
            .collect();
        assert_eq!(outcomes.len(), 18, "operations checked");
        assert!(
            not_poisoned.is_empty(),
            "expected every operation to return LockPoisoned, observed {not_poisoned:?}"
        );
        assert_eq!(
            g.event_count().await,
            events_before,
            "expected no event recorded after the panic"
        );
    }

    /// With a one-thread executor's only worker occupied, an `add_vertex` and
    /// an `update_hyperedge_vertices_try` future polled once and dropped both
    /// land: the log reaches 5 events and replays to the graph's 3 vertices
    /// and hyperedge members `[v0, v1]`.
    #[tokio::test]
    async fn a_dropped_mutation_future_still_records_its_event() {
        let exec = Arc::new(Executor::with_num_threads(1).expect("one-thread pool builds"));
        let g = TemporalHypergraph::<u32, u32>::new(Arc::clone(&exec));
        let v0 = g.add_vertex(10).await.expect("add v0");
        let v1 = g.add_vertex(11).await.expect("add v1");
        let h = g.add_hyperedge(vec![v0], 1).await.expect("add h");

        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let blocker = {
            let exec = Arc::clone(&exec);
            tokio::spawn(async move {
                exec.run(move || {
                    let _ = started_tx.send(());
                    let _ = release_rx.recv();
                })
                .await;
            })
        };
        started_rx.await.expect("the blocker job starts");
        {
            let mut add = Box::pin(g.add_vertex(12));
            assert!(
                futures::poll!(&mut add).is_pending(),
                "add_vertex resolved while the worker was occupied"
            );
            let mut update = Box::pin(g.update_hyperedge_vertices_try(h, move |mut m| {
                m.push(v1);
                Ok(m)
            }));
            assert!(
                futures::poll!(&mut update).is_pending(),
                "update_hyperedge_vertices_try resolved while the worker was occupied"
            );
        }
        release_tx.send(()).expect("the blocker job is waiting");
        blocker.await.expect("the blocker task completes");

        let landed = tokio::time::timeout(Duration::from_secs(5), async {
            while g.event_count().await < 5 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;
        let ts = g.current_timestamp();
        let graph_state = (
            g.count_vertices().await.expect("count"),
            g.get_hyperedge_vertices(h).await.expect("members"),
        );
        let log_state = (
            TemporalQueries::count_vertices_at(g.events_ref(), ts).await,
            TemporalQueries::hyperedge_vertices_at(g.events_ref(), h, ts).await,
        );
        assert!(
            landed.is_ok(),
            "expected 5 events within 5 s, observed {} (graph {graph_state:?}, log {log_state:?})",
            g.event_count().await
        );
        assert_eq!(graph_state, (3, vec![v0, v1]), "graph state");
        assert_eq!(log_state, (3, Some(vec![v0, v1])), "log replay");
    }
}
