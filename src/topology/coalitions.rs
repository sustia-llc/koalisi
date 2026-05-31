//! Coalition management for temporal hypergraphs.
//!
//! This module provides a high-level API for managing agent coalitions,
//! where agents are vertices and coalitions are hyperedges connecting them.

use super::errors::{TemporalError, TemporalResult};
use super::events::TemporalEvent;
use super::queries::TemporalQueries;
use super::temporal::TemporalHypergraph;
use super::timestamp::{TimeRange, Timestamp};
use hypergraph::{HyperedgeIndex, HyperedgeTrait, VertexIndex, VertexTrait};
use std::collections::HashMap;

/// Manages coalitions (hyperedges) of agents (vertices) in a temporal hypergraph.
///
/// This provides domain-specific operations for coalition formation scenarios,
/// building on top of the generic TemporalHypergraph API.
///
/// # Example
///
/// ```ignore
/// use koalisi::topology::{CoalitionManager, TemporalHypergraph};
///
/// let manager = CoalitionManager::new(TemporalHypergraph::new());
///
/// // Add agents
/// let agent1 = manager.add_agent(Agent::new("a1", 5)).await?;
/// let agent2 = manager.add_agent(Agent::new("a2", 3)).await?;
///
/// // Form a coalition
/// let coalition = manager.form_coalition(
///     vec![agent1, agent2],
///     Coalition::new("alpha", 10, 100),
/// ).await?;
///
/// // Agent joins later
/// let agent3 = manager.add_agent(Agent::new("a3", 4)).await?;
/// manager.join_coalition(agent3, coalition).await?;
/// ```
pub struct CoalitionManager<V, HE>
where
    V: VertexTrait + Clone + 'static,
    HE: HyperedgeTrait + Clone + 'static,
{
    graph: TemporalHypergraph<V, HE>,
}

impl<V, HE> CoalitionManager<V, HE>
where
    V: VertexTrait + Clone + 'static,
    HE: HyperedgeTrait + Clone + 'static,
{
    /// Create a new coalition manager with a fresh temporal hypergraph.
    pub fn new(graph: TemporalHypergraph<V, HE>) -> Self {
        Self { graph }
    }

    /// Create a coalition manager with a new empty graph.
    pub fn empty() -> Self {
        Self {
            graph: TemporalHypergraph::new(),
        }
    }

    /// Get a reference to the underlying temporal hypergraph.
    pub fn graph(&self) -> &TemporalHypergraph<V, HE> {
        &self.graph
    }

    /// Get the current timestamp.
    pub fn current_timestamp(&self) -> Timestamp {
        self.graph.current_timestamp()
    }

    // =========================================================================
    // Agent (vertex) operations
    // =========================================================================

    /// Add an agent to the system.
    pub async fn add_agent(&self, agent: V) -> TemporalResult<VertexIndex, V, HE> {
        self.graph.add_vertex(agent).await
    }

    /// Remove an agent from the system.
    ///
    /// Note: This does not automatically remove the agent from coalitions.
    /// The coalitions will retain references to the removed agent index.
    pub async fn remove_agent(&self, agent: VertexIndex) -> TemporalResult<(), V, HE> {
        self.graph.remove_vertex(agent).await
    }

    /// Get an agent's weight/data.
    pub async fn get_agent(&self, agent: VertexIndex) -> TemporalResult<V, V, HE> {
        self.graph.get_vertex_weight(agent).await
    }

    /// Update an agent's weight/data.
    pub async fn update_agent(
        &self,
        agent: VertexIndex,
        new_data: V,
    ) -> TemporalResult<(), V, HE> {
        self.graph.update_vertex_weight(agent, new_data).await
    }

    // =========================================================================
    // Coalition (hyperedge) operations
    // =========================================================================

    /// Form a new coalition from a set of agents.
    ///
    /// Returns the index of the newly created coalition.
    pub async fn form_coalition(
        &self,
        agents: Vec<VertexIndex>,
        coalition: HE,
    ) -> TemporalResult<HyperedgeIndex, V, HE> {
        self.graph.add_hyperedge(agents, coalition).await
    }

    /// Dissolve a coalition (remove the hyperedge).
    ///
    /// The agents remain in the system but are no longer part of this coalition.
    pub async fn dissolve_coalition(
        &self,
        coalition: HyperedgeIndex,
    ) -> TemporalResult<(), V, HE> {
        self.graph.remove_hyperedge(coalition).await
    }

    /// Add an agent to an existing coalition.
    ///
    /// Read-modify-write happens atomically under a single graph write lock so
    /// concurrent join/leave calls cannot race on the membership vector.
    pub async fn join_coalition(
        &self,
        agent: VertexIndex,
        coalition: HyperedgeIndex,
    ) -> TemporalResult<(), V, HE> {
        self.graph
            .update_hyperedge_vertices_try(coalition, move |mut members| {
                if !members.contains(&agent) {
                    members.push(agent);
                }
                Ok(members)
            })
            .await
    }

    /// Remove an agent from a coalition.
    ///
    /// Returns `EmptyCoalition` if the result would have zero members (use
    /// `dissolve_coalition` instead). The check happens atomically with the
    /// update under a single graph write lock.
    pub async fn leave_coalition(
        &self,
        agent: VertexIndex,
        coalition: HyperedgeIndex,
    ) -> TemporalResult<(), V, HE> {
        self.graph
            .update_hyperedge_vertices_try(coalition, move |members| {
                let new_members: Vec<VertexIndex> =
                    members.into_iter().filter(|&v| v != agent).collect();
                if new_members.is_empty() {
                    Err(TemporalError::EmptyCoalition)
                } else {
                    Ok(new_members)
                }
            })
            .await
    }

    /// Get the coalition's weight/data.
    pub async fn get_coalition(&self, coalition: HyperedgeIndex) -> TemporalResult<HE, V, HE> {
        self.graph.get_hyperedge_weight(coalition).await
    }

    /// Update a coalition's weight/data.
    pub async fn update_coalition(
        &self,
        coalition: HyperedgeIndex,
        new_data: HE,
    ) -> TemporalResult<(), V, HE> {
        self.graph
            .update_hyperedge_weight(coalition, new_data)
            .await
    }

    /// Get the current members of a coalition.
    pub async fn coalition_members(
        &self,
        coalition: HyperedgeIndex,
    ) -> TemporalResult<Vec<VertexIndex>, V, HE> {
        self.graph.get_hyperedge_vertices(coalition).await
    }

    /// Merge multiple coalitions into one.
    ///
    /// The first coalition in the list becomes the target; others are dissolved.
    /// All members from all coalitions become members of the target coalition.
    pub async fn merge_coalitions(
        &self,
        coalitions: Vec<HyperedgeIndex>,
    ) -> TemporalResult<(), V, HE> {
        self.graph.join_hyperedges(coalitions).await
    }

    // =========================================================================
    // Temporal query operations
    // =========================================================================

    /// Get coalition members at a specific point in time.
    pub async fn coalition_members_at(
        &self,
        coalition: HyperedgeIndex,
        timestamp: Timestamp,
    ) -> Option<Vec<VertexIndex>> {
        TemporalQueries::hyperedge_vertices_at(self.graph.events_ref(), coalition, timestamp).await
    }

    /// Get all coalitions an agent has belonged to over time.
    ///
    /// Returns a list of (coalition_index, time_range) pairs showing when
    /// the agent was a member of each coalition.
    #[allow(dead_code)]
    pub(crate) async fn agent_coalition_history(
        &self,
        agent: VertexIndex,
    ) -> Vec<(HyperedgeIndex, TimeRange)> {
        // Snapshot the events under the lock, then process outside so the read
        // lock isn't held for the duration of the linear scan.
        let snapshot: Vec<TemporalEvent<V, HE>> = {
            let events = self.graph.events_ref().read().await;
            events.events().to_vec()
        };

        let mut history: Vec<(HyperedgeIndex, TimeRange)> = Vec::new();
        let mut membership_start: HashMap<HyperedgeIndex, Timestamp> = HashMap::new();

        for event in &snapshot {
            match event {
                TemporalEvent::HyperedgeAdded {
                    timestamp,
                    index,
                    vertices,
                    ..
                }
                    if vertices.contains(&agent) => {
                        membership_start.insert(*index, *timestamp);
                    }
                TemporalEvent::HyperedgeRemoved {
                    timestamp, index, ..
                } => {
                    if let Some(start) = membership_start.remove(index) {
                        history.push((*index, TimeRange::new(Some(start), Some(*timestamp))));
                    }
                }
                TemporalEvent::HyperedgeVerticesUpdated {
                    timestamp,
                    index,
                    old_vertices,
                    new_vertices,
                    ..
                } => {
                    let was_member = old_vertices.contains(&agent);
                    let is_member = new_vertices.contains(&agent);
                    match (was_member, is_member) {
                        (false, true) => {
                            membership_start.insert(*index, *timestamp);
                        }
                        (true, false) => {
                            if let Some(start) = membership_start.remove(index) {
                                history.push((
                                    *index,
                                    TimeRange::new(Some(start), Some(*timestamp)),
                                ));
                            }
                        }
                        _ => {}
                    }
                }
                TemporalEvent::HyperedgesJoined {
                    timestamp,
                    target_index,
                    source_indices,
                    new_vertices,
                    ..
                } => {
                    for source in source_indices {
                        if let Some(start) = membership_start.remove(source) {
                            history.push((
                                *source,
                                TimeRange::new(Some(start), Some(*timestamp)),
                            ));
                        }
                    }
                    if new_vertices.contains(&agent) {
                        membership_start.insert(*target_index, *timestamp);
                    }
                }
                TemporalEvent::VerticesContracted {
                    timestamp,
                    hyperedge_index,
                    old_vertices,
                    new_vertices,
                    ..
                } => {
                    let was_member = old_vertices.contains(&agent);
                    let is_member = new_vertices.contains(&agent);
                    match (was_member, is_member) {
                        (true, false) => {
                            if let Some(start) = membership_start.remove(hyperedge_index) {
                                history.push((
                                    *hyperedge_index,
                                    TimeRange::new(Some(start), Some(*timestamp)),
                                ));
                            }
                        }
                        (false, true) => {
                            membership_start.insert(*hyperedge_index, *timestamp);
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        for (index, start) in membership_start {
            history.push((index, TimeRange::new(Some(start), None)));
        }

        history
    }

    /// Check if an agent was a member of a coalition at a specific time.
    #[allow(dead_code)]
    pub(crate) async fn was_member_at(
        &self,
        agent: VertexIndex,
        coalition: HyperedgeIndex,
        timestamp: Timestamp,
    ) -> bool {
        self.coalition_members_at(coalition, timestamp)
            .await
            .is_some_and(|members| members.contains(&agent))
    }

    /// Get when a coalition was formed.
    #[allow(dead_code)]
    pub(crate) async fn coalition_formed_at(&self, coalition: HyperedgeIndex) -> Option<Timestamp> {
        TemporalQueries::hyperedge_created_at(self.graph.events_ref(), coalition).await
    }

    /// Get when a coalition was dissolved (if it was).
    #[allow(dead_code)]
    pub(crate) async fn coalition_dissolved_at(&self, coalition: HyperedgeIndex) -> Option<Timestamp> {
        TemporalQueries::hyperedge_removed_at(self.graph.events_ref(), coalition).await
    }

    /// Get the lifespan of a coalition.
    #[allow(dead_code)]
    pub(crate) async fn coalition_lifespan(&self, coalition: HyperedgeIndex) -> Option<TimeRange> {
        TemporalQueries::hyperedge_lifespan(self.graph.events_ref(), coalition).await
    }

    // =========================================================================
    // Counting operations
    // =========================================================================

    /// Count current agents.
    pub async fn count_agents(&self) -> usize {
        self.graph.count_vertices().await
    }

    /// Count current coalitions.
    pub async fn count_coalitions(&self) -> usize {
        self.graph.count_hyperedges().await
    }

    /// Count agents at a specific timestamp.
    #[allow(dead_code)]
    pub(crate) async fn count_agents_at(&self, timestamp: Timestamp) -> usize {
        TemporalQueries::count_vertices_at(self.graph.events_ref(), timestamp).await
    }

    /// Count coalitions at a specific timestamp.
    #[allow(dead_code)]
    pub(crate) async fn count_coalitions_at(&self, timestamp: Timestamp) -> usize {
        TemporalQueries::count_hyperedges_at(self.graph.events_ref(), timestamp).await
    }

    // =========================================================================
    // Snapshot operations
    // =========================================================================

    /// Create a snapshot of the current state.
    pub async fn create_snapshot(&self) -> super::events::SnapshotId {
        self.graph.create_snapshot().await
    }
}

impl<V, HE> Default for CoalitionManager<V, HE>
where
    V: VertexTrait + Clone + 'static,
    HE: HyperedgeTrait + Clone + 'static,
{
    fn default() -> Self {
        Self::empty()
    }
}
