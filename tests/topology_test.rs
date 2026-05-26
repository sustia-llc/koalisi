//! Topology layer integration tests — temporal hypergraph, coalition management,
//! time-travel queries, and analytics.
//!
//! Ported from dynamo's temporal_basic, coalition_formation, and
//! temporal_analytics test suites.

use std::fmt::{Display, Formatter};

use koalisi::topology::{
    CoalitionManager, TemporalAnalytics, TemporalEvent, TemporalHypergraph, TemporalQueries,
    TimeRange, Timestamp,
};

// =========================================================================
// Test types — satisfy VertexTrait (Copy+Debug+Display+Eq+Hash+Send+Sync)
//                  and HyperedgeTrait (VertexTrait + Into<usize>)
// =========================================================================

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
struct Agent {
    id: &'static str,
    capabilities: u32,
}

impl Agent {
    const fn new(id: &'static str, capabilities: u32) -> Self {
        Self { id, capabilities }
    }
}

impl Display for Agent {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Agent({})", self.id)
    }
}

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
struct Coalition {
    name: &'static str,
    formation_cost: usize,
    value: usize,
}

impl Coalition {
    const fn new(name: &'static str, formation_cost: usize, value: usize) -> Self {
        Self {
            name,
            formation_cost,
            value,
        }
    }
}

impl Display for Coalition {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Coalition({})", self.name)
    }
}

impl From<Coalition> for usize {
    fn from(c: Coalition) -> Self {
        c.formation_cost
    }
}

// =========================================================================
// Temporal basic tests
// =========================================================================

#[tokio::test]
async fn event_recording() {
    let graph = TemporalHypergraph::<Agent, Coalition>::new();

    let a1 = graph.add_vertex(Agent::new("a1", 5)).await.unwrap();
    let a2 = graph.add_vertex(Agent::new("a2", 3)).await.unwrap();
    let _c1 = graph
        .add_hyperedge(vec![a1, a2], Coalition::new("alpha", 10, 100))
        .await
        .unwrap();

    let events = graph.events().await;
    assert_eq!(events.len(), 3);
    assert!(matches!(&events[0], TemporalEvent::VertexAdded { .. }));
    assert!(matches!(&events[1], TemporalEvent::VertexAdded { .. }));
    assert!(matches!(&events[2], TemporalEvent::HyperedgeAdded { .. }));

    graph
        .update_vertex_weight(a1, Agent::new("a1-updated", 10))
        .await
        .unwrap();

    let events = graph.events().await;
    assert_eq!(events.len(), 4);
    assert!(matches!(
        &events[3],
        TemporalEvent::VertexWeightUpdated { .. }
    ));
}

#[tokio::test]
async fn point_in_time_queries() {
    let graph = TemporalHypergraph::<Agent, Coalition>::new();

    let a1 = graph.add_vertex(Agent::new("a1", 5)).await.unwrap();
    let _a2 = graph.add_vertex(Agent::new("a2", 3)).await.unwrap();
    let _c1 = graph
        .add_hyperedge(vec![a1], Coalition::new("alpha", 10, 100))
        .await
        .unwrap();

    let events_ref = graph.events_ref();

    let count_at_0 = TemporalQueries::count_vertices_at(events_ref, Timestamp(0)).await;
    assert_eq!(count_at_0, 1);

    let count_at_1 = TemporalQueries::count_vertices_at(events_ref, Timestamp(1)).await;
    assert_eq!(count_at_1, 2);

    let he_count_at_2 =
        TemporalQueries::count_hyperedges_at(events_ref, Timestamp(2)).await;
    assert_eq!(he_count_at_2, 1);

    let he_count_at_1 =
        TemporalQueries::count_hyperedges_at(events_ref, Timestamp(1)).await;
    assert_eq!(he_count_at_1, 0);
}

#[tokio::test]
async fn vertex_history() {
    let graph = TemporalHypergraph::<Agent, Coalition>::new();

    let a1 = graph.add_vertex(Agent::new("a1", 5)).await.unwrap();
    graph
        .update_vertex_weight(a1, Agent::new("a1", 10))
        .await
        .unwrap();
    graph
        .update_vertex_weight(a1, Agent::new("a1", 15))
        .await
        .unwrap();

    let history = graph.vertex_history(a1).await;
    assert_eq!(history.len(), 3);

    let timestamps: Vec<_> = history.iter().map(|e| e.timestamp()).collect();
    assert!(timestamps.windows(2).all(|w| w[0] <= w[1]));
}

#[tokio::test]
async fn snapshot_creation() {
    let graph = TemporalHypergraph::<Agent, Coalition>::new();

    let _a1 = graph.add_vertex(Agent::new("a1", 5)).await.unwrap();
    let _a2 = graph.add_vertex(Agent::new("a2", 3)).await.unwrap();

    let snap1 = graph.create_snapshot().await;

    let _a3 = graph.add_vertex(Agent::new("a3", 4)).await.unwrap();

    let snap2 = graph.create_snapshot().await;

    let snapshots = graph.list_snapshots().await;
    assert_eq!(snapshots.len(), 2);

    let retrieved1 = graph.get_snapshot(snap1).await;
    assert!(retrieved1.is_some());
    assert_eq!(retrieved1.unwrap().id, snap1);

    let retrieved2 = graph.get_snapshot(snap2).await;
    assert!(retrieved2.is_some());
    assert_eq!(retrieved2.unwrap().id, snap2);
}

#[tokio::test]
async fn vertex_weight_at() {
    let graph = TemporalHypergraph::<Agent, Coalition>::new();

    let a1 = graph.add_vertex(Agent::new("a1", 5)).await.unwrap();
    graph
        .update_vertex_weight(a1, Agent::new("a1", 10))
        .await
        .unwrap();

    let events_ref = graph.events_ref();

    let weight_at_0 =
        TemporalQueries::vertex_weight_at(events_ref, a1, Timestamp(0)).await;
    assert!(weight_at_0.is_some());
    assert_eq!(weight_at_0.unwrap().capabilities, 5);

    let weight_at_1 =
        TemporalQueries::vertex_weight_at(events_ref, a1, Timestamp(1)).await;
    assert!(weight_at_1.is_some());
    assert_eq!(weight_at_1.unwrap().capabilities, 10);
}

#[tokio::test]
async fn event_stats() {
    let graph = TemporalHypergraph::<Agent, Coalition>::new();

    let a1 = graph.add_vertex(Agent::new("a1", 5)).await.unwrap();
    let a2 = graph.add_vertex(Agent::new("a2", 3)).await.unwrap();
    let _c1 = graph
        .add_hyperedge(vec![a1, a2], Coalition::new("alpha", 10, 100))
        .await
        .unwrap();

    let stats = graph.event_stats().await;
    assert_eq!(stats.vertex_added, 2);
    assert_eq!(stats.hyperedge_added, 1);
}

// =========================================================================
// Coalition formation tests
// =========================================================================

#[tokio::test]
async fn form_and_dissolve_coalition() {
    let manager = CoalitionManager::<Agent, Coalition>::empty();

    let a1 = manager.add_agent(Agent::new("a1", 5)).await.unwrap();
    let a2 = manager.add_agent(Agent::new("a2", 3)).await.unwrap();

    let c1 = manager
        .form_coalition(vec![a1, a2], Coalition::new("alpha", 10, 100))
        .await
        .unwrap();

    assert_eq!(manager.count_coalitions().await, 1);
    let members = manager.coalition_members(c1).await.unwrap();
    assert_eq!(members.len(), 2);
    assert!(members.contains(&a1));
    assert!(members.contains(&a2));

    manager.dissolve_coalition(c1).await.unwrap();
    assert_eq!(manager.count_coalitions().await, 0);
}

#[tokio::test]
async fn agent_joins_leaves_coalition() {
    let manager = CoalitionManager::<Agent, Coalition>::empty();

    let a1 = manager.add_agent(Agent::new("a1", 5)).await.unwrap();
    let a2 = manager.add_agent(Agent::new("a2", 3)).await.unwrap();
    let a3 = manager.add_agent(Agent::new("a3", 4)).await.unwrap();

    let c1 = manager
        .form_coalition(vec![a1, a2], Coalition::new("alpha", 10, 100))
        .await
        .unwrap();

    manager.join_coalition(a3, c1).await.unwrap();
    let members = manager.coalition_members(c1).await.unwrap();
    assert_eq!(members.len(), 3);

    manager.leave_coalition(a3, c1).await.unwrap();
    let members = manager.coalition_members(c1).await.unwrap();
    assert_eq!(members.len(), 2);
}

#[tokio::test]
async fn merge_coalitions() {
    let manager = CoalitionManager::<Agent, Coalition>::empty();

    let a1 = manager.add_agent(Agent::new("a1", 5)).await.unwrap();
    let a2 = manager.add_agent(Agent::new("a2", 3)).await.unwrap();
    let a3 = manager.add_agent(Agent::new("a3", 4)).await.unwrap();
    let a4 = manager.add_agent(Agent::new("a4", 6)).await.unwrap();

    let c1 = manager
        .form_coalition(vec![a1, a2], Coalition::new("alpha", 10, 100))
        .await
        .unwrap();
    let c2 = manager
        .form_coalition(vec![a3, a4], Coalition::new("beta", 8, 80))
        .await
        .unwrap();

    assert_eq!(manager.count_coalitions().await, 2);

    manager.merge_coalitions(vec![c1, c2]).await.unwrap();
    assert_eq!(manager.count_coalitions().await, 1);
}

// =========================================================================
// Analytics tests
// =========================================================================

#[tokio::test]
async fn analytics_delta() {
    let graph = TemporalHypergraph::<Agent, Coalition>::new();

    // T=0: a1 added, T=1: a2 added
    let _a1 = graph.add_vertex(Agent::new("a1", 5)).await.unwrap();
    let _a2 = graph.add_vertex(Agent::new("a2", 3)).await.unwrap();

    // delta skips events at exactly `from`, so from=0 excludes a1(T=0)
    let delta =
        TemporalAnalytics::delta(graph.events_ref(), Timestamp(0), Timestamp(2)).await;
    assert_eq!(delta.vertices_added.len(), 1);
    assert!(delta.vertices_removed.is_empty());
}

#[tokio::test]
async fn analytics_event_count() {
    let graph = TemporalHypergraph::<Agent, Coalition>::new();

    let a1 = graph.add_vertex(Agent::new("a1", 5)).await.unwrap();
    let a2 = graph.add_vertex(Agent::new("a2", 3)).await.unwrap();
    let _c1 = graph
        .add_hyperedge(vec![a1, a2], Coalition::new("alpha", 10, 100))
        .await
        .unwrap();

    let range = TimeRange::all();
    let count =
        TemporalAnalytics::event_count_in_range(graph.events_ref(), &range).await;
    assert_eq!(count, 3);
}
