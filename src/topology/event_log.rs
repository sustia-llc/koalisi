//! Event log storage and indexing for temporal hypergraph.

use super::events::{EventStats, SnapshotId, TemporalEvent};
use super::timestamp::{TimeRange, Timestamp};
use catgraph_applied::{HyperedgeIndex, VertexIndex};
use std::collections::{BTreeMap, HashMap};
use std::fmt::Debug;

/// The event log stores all events and provides efficient indexing.
pub struct EventLog<V, HE>
where
    V: Clone + Debug,
    HE: Clone + Debug,
{
    /// All events in chronological order.
    events: Vec<TemporalEvent<V, HE>>,
    /// Index from timestamp to event indices at that timestamp.
    time_index: BTreeMap<Timestamp, Vec<usize>>,
    /// Index from vertex to event indices affecting that vertex.
    vertex_index: HashMap<VertexIndex, Vec<usize>>,
    /// Index from hyperedge to event indices affecting that hyperedge.
    hyperedge_index: HashMap<HyperedgeIndex, Vec<usize>>,
    /// Snapshots ordered by timestamp for `snapshot_before` to use range queries.
    snapshot_index: BTreeMap<Timestamp, (SnapshotId, usize)>,
    /// Running event-type counts, maintained on every `append` so `stats()` is O(1).
    stats: EventStats,
}

impl<V, HE> EventLog<V, HE>
where
    V: Clone + Debug,
    HE: Clone + Debug,
{
    /// Create a new empty event log.
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            time_index: BTreeMap::new(),
            vertex_index: HashMap::new(),
            hyperedge_index: HashMap::new(),
            snapshot_index: BTreeMap::new(),
            stats: EventStats::default(),
        }
    }

    /// Append an event to the log and update indices.
    pub fn append(&mut self, event: TemporalEvent<V, HE>) {
        let index = self.events.len();
        let timestamp = event.timestamp();

        self.time_index.entry(timestamp).or_default().push(index);

        if let Some(vertex) = event.vertex_index() {
            self.vertex_index.entry(vertex).or_default().push(index);
        }

        // Contracted vertices each get the same event in their per-vertex history.
        if let TemporalEvent::VerticesContracted {
            contracted_vertices,
            ..
        } = &event
        {
            for v in contracted_vertices {
                self.vertex_index.entry(*v).or_default().push(index);
            }
        }

        if let Some(hyperedge) = event.hyperedge_index() {
            self.hyperedge_index
                .entry(hyperedge)
                .or_default()
                .push(index);
        }

        // Source hyperedges each get the join event in their per-hyperedge history.
        if let TemporalEvent::HyperedgesJoined { source_indices, .. } = &event {
            for h in source_indices {
                self.hyperedge_index.entry(*h).or_default().push(index);
            }
        }

        if let TemporalEvent::SnapshotMarker { snapshot_id, .. } = &event {
            self.snapshot_index
                .insert(timestamp, (*snapshot_id, index));
        }

        self.stats.count(&event);
        self.events.push(event);
    }

    /// Get the total number of events.
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Check if the log is empty.
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Get an event by index.
    pub fn get(&self, index: usize) -> Option<&TemporalEvent<V, HE>> {
        self.events.get(index)
    }

    /// Get all events.
    pub fn events(&self) -> &[TemporalEvent<V, HE>] {
        &self.events
    }

    /// Get events at a specific timestamp.
    pub fn events_at(&self, timestamp: Timestamp) -> Vec<&TemporalEvent<V, HE>> {
        self.time_index
            .get(&timestamp)
            .map(|indices| indices.iter().filter_map(|&i| self.events.get(i)).collect())
            .unwrap_or_default()
    }

    /// Get events in a time range.
    pub fn events_in_range(&self, range: &TimeRange) -> Vec<&TemporalEvent<V, HE>> {
        let start = range.start.unwrap_or(Timestamp::EPOCH);
        let end = range.end.unwrap_or(Timestamp::MAX);

        self.time_index
            .range(start..end)
            .flat_map(|(_, indices)| indices.iter())
            .filter_map(|&i| self.events.get(i))
            .collect()
    }

    /// Get events up to (and including) a timestamp.
    pub fn events_until(&self, timestamp: Timestamp) -> Vec<&TemporalEvent<V, HE>> {
        self.time_index
            .range(..=timestamp)
            .flat_map(|(_, indices)| indices.iter())
            .filter_map(|&i| self.events.get(i))
            .collect()
    }

    /// Get events affecting a specific vertex.
    pub fn vertex_events(&self, vertex: VertexIndex) -> Vec<&TemporalEvent<V, HE>> {
        self.vertex_index
            .get(&vertex)
            .map(|indices| indices.iter().filter_map(|&i| self.events.get(i)).collect())
            .unwrap_or_default()
    }

    /// Get events affecting a specific hyperedge.
    pub fn hyperedge_events(&self, hyperedge: HyperedgeIndex) -> Vec<&TemporalEvent<V, HE>> {
        self.hyperedge_index
            .get(&hyperedge)
            .map(|indices| indices.iter().filter_map(|&i| self.events.get(i)).collect())
            .unwrap_or_default()
    }

    /// Get the timestamp of the first event.
    pub fn first_timestamp(&self) -> Option<Timestamp> {
        self.time_index.keys().next().copied()
    }

    /// Get the timestamp of the last event.
    pub fn last_timestamp(&self) -> Option<Timestamp> {
        self.time_index.keys().next_back().copied()
    }

    /// Get the most recent snapshot at or before `timestamp`. O(log n) via the BTreeMap range.
    pub fn snapshot_before(&self, timestamp: Timestamp) -> Option<(SnapshotId, usize)> {
        self.snapshot_index
            .range(..=timestamp)
            .next_back()
            .map(|(_, payload)| *payload)
    }

    /// Get statistics about events. O(1) — maintained incrementally on append.
    pub fn stats(&self) -> EventStats {
        self.stats.clone()
    }

    /// Get statistics for events in a time range.
    pub fn stats_in_range(&self, range: &TimeRange) -> EventStats {
        let mut stats = EventStats::default();
        for event in self.events_in_range(range) {
            stats.count(event);
        }
        stats
    }

    /// Count mutation events (excluding snapshot markers).
    pub fn mutation_count(&self) -> usize {
        self.events.iter().filter(|e| e.is_mutation()).count()
    }

    /// Get distinct timestamps.
    pub fn distinct_timestamps(&self) -> Vec<Timestamp> {
        self.time_index.keys().copied().collect()
    }

    /// Iterate vertex indices ever touched (without allocating a Vec).
    pub fn vertex_indices(&self) -> impl Iterator<Item = VertexIndex> + '_ {
        self.vertex_index.keys().copied()
    }

    /// Iterate hyperedge indices ever touched (without allocating a Vec).
    pub fn hyperedge_indices(&self) -> impl Iterator<Item = HyperedgeIndex> + '_ {
        self.hyperedge_index.keys().copied()
    }

    /// Get all vertices that have been added (even if later removed).
    pub fn all_vertex_indices(&self) -> Vec<VertexIndex> {
        self.vertex_indices().collect()
    }

    /// Get all hyperedges that have been added (even if later removed).
    pub fn all_hyperedge_indices(&self) -> Vec<HyperedgeIndex> {
        self.hyperedge_indices().collect()
    }

    /// Clear the event log.
    pub fn clear(&mut self) {
        self.events.clear();
        self.time_index.clear();
        self.vertex_index.clear();
        self.hyperedge_index.clear();
        self.snapshot_index.clear();
        self.stats = EventStats::default();
    }
}

impl<V, HE> Default for EventLog<V, HE>
where
    V: Clone + Debug,
    HE: Clone + Debug,
{
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug)]
    #[allow(dead_code)]
    struct TestV(String);

    #[derive(Clone, Debug)]
    #[allow(dead_code)]
    struct TestHE(String);

    #[test]
    fn append_and_retrieve() {
        let mut log = EventLog::<TestV, TestHE>::new();

        log.append(TemporalEvent::VertexAdded {
            timestamp: Timestamp(1),
            index: VertexIndex(0),
            weight: TestV("a".into()),
        });

        log.append(TemporalEvent::VertexAdded {
            timestamp: Timestamp(2),
            index: VertexIndex(1),
            weight: TestV("b".into()),
        });

        assert_eq!(log.len(), 2);
        assert!(!log.is_empty());
    }

    #[test]
    fn time_index() {
        let mut log = EventLog::<TestV, TestHE>::new();

        log.append(TemporalEvent::VertexAdded {
            timestamp: Timestamp(5),
            index: VertexIndex(0),
            weight: TestV("a".into()),
        });

        log.append(TemporalEvent::VertexAdded {
            timestamp: Timestamp(5),
            index: VertexIndex(1),
            weight: TestV("b".into()),
        });

        log.append(TemporalEvent::VertexAdded {
            timestamp: Timestamp(10),
            index: VertexIndex(2),
            weight: TestV("c".into()),
        });

        assert_eq!(log.events_at(Timestamp(5)).len(), 2);
        assert_eq!(log.events_at(Timestamp(10)).len(), 1);
        assert_eq!(log.events_at(Timestamp(1)).len(), 0);
    }

    #[test]
    fn vertex_index() {
        let mut log = EventLog::<TestV, TestHE>::new();

        log.append(TemporalEvent::VertexAdded {
            timestamp: Timestamp(1),
            index: VertexIndex(0),
            weight: TestV("a".into()),
        });

        log.append(TemporalEvent::VertexWeightUpdated {
            timestamp: Timestamp(2),
            index: VertexIndex(0),
            old_weight: TestV("a".into()),
            new_weight: TestV("A".into()),
        });

        assert_eq!(log.vertex_events(VertexIndex(0)).len(), 2);
        assert_eq!(log.vertex_events(VertexIndex(1)).len(), 0);
    }

    #[test]
    fn snapshot_before_uses_btree_range() {
        let mut log = EventLog::<TestV, TestHE>::new();
        log.append(TemporalEvent::SnapshotMarker {
            timestamp: Timestamp(10),
            snapshot_id: SnapshotId(0),
        });
        log.append(TemporalEvent::SnapshotMarker {
            timestamp: Timestamp(20),
            snapshot_id: SnapshotId(1),
        });

        let (sid, idx) = log.snapshot_before(Timestamp(15)).unwrap();
        assert_eq!(sid, SnapshotId(0));
        assert_eq!(idx, 0);

        let (sid, _) = log.snapshot_before(Timestamp(25)).unwrap();
        assert_eq!(sid, SnapshotId(1));
    }

    #[test]
    fn stats_are_incremental() {
        let mut log = EventLog::<TestV, TestHE>::new();
        log.append(TemporalEvent::VertexAdded {
            timestamp: Timestamp(1),
            index: VertexIndex(0),
            weight: TestV("a".into()),
        });
        log.append(TemporalEvent::VertexRemoved {
            timestamp: Timestamp(2),
            index: VertexIndex(0),
            weight: TestV("a".into()),
        });
        let stats = log.stats();
        assert_eq!(stats.vertex_added, 1);
        assert_eq!(stats.vertex_removed, 1);
        assert_eq!(stats.total(), 2);
    }
}
