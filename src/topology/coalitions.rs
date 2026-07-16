//! Coalition management for temporal hypergraphs.
//!
//! This module provides a high-level API for managing agent coalitions,
//! where agents are vertices and coalitions are hyperedges connecting them.

use super::errors::{TemporalError, TemporalResult};
use super::events::TemporalEvent;
use super::queries::TemporalQueries;
use super::temporal::TemporalHypergraph;
use super::timestamp::{TimeRange, Timestamp};
use crate::algorithms::{AgentCapabilities, FeedbackStore};
use crate::decision::{CoalitionDecisionPolicy, Decision, DecisionContext};
use super::{HyperedgeTrait, VertexTrait};
use catgraph_applied::{HyperedgeIndex, VertexIndex};
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
    pub async fn add_agent(&self, agent: V) -> TemporalResult<VertexIndex> {
        self.graph.add_vertex(agent).await
    }

    /// Remove an agent from the system.
    ///
    /// Note: This does not automatically remove the agent from coalitions.
    /// The coalitions will retain references to the removed agent index.
    pub async fn remove_agent(&self, agent: VertexIndex) -> TemporalResult<()> {
        self.graph.remove_vertex(agent).await
    }

    /// Get an agent's weight/data.
    pub async fn get_agent(&self, agent: VertexIndex) -> TemporalResult<V> {
        self.graph.get_vertex_weight(agent).await
    }

    /// Update an agent's weight/data.
    pub async fn update_agent(
        &self,
        agent: VertexIndex,
        new_data: V,
    ) -> TemporalResult<()> {
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
    ) -> TemporalResult<HyperedgeIndex> {
        self.graph.add_hyperedge(agents, coalition).await
    }

    /// Dissolve a coalition (remove the hyperedge).
    ///
    /// The agents remain in the system but are no longer part of this coalition.
    pub async fn dissolve_coalition(
        &self,
        coalition: HyperedgeIndex,
    ) -> TemporalResult<()> {
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
    ) -> TemporalResult<()> {
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
    ) -> TemporalResult<()> {
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
    pub async fn get_coalition(&self, coalition: HyperedgeIndex) -> TemporalResult<HE> {
        self.graph.get_hyperedge_weight(coalition).await
    }

    /// Update a coalition's weight/data.
    pub async fn update_coalition(
        &self,
        coalition: HyperedgeIndex,
        new_data: HE,
    ) -> TemporalResult<()> {
        self.graph
            .update_hyperedge_weight(coalition, new_data)
            .await
    }

    /// Get the current members of a coalition.
    pub async fn coalition_members(
        &self,
        coalition: HyperedgeIndex,
    ) -> TemporalResult<Vec<VertexIndex>> {
        self.graph.get_hyperedge_vertices(coalition).await
    }

    /// Merge multiple coalitions into one.
    ///
    /// The first coalition in the list becomes the target; others are dissolved.
    /// All members from all coalitions become members of the target coalition.
    pub async fn merge_coalitions(
        &self,
        coalitions: Vec<HyperedgeIndex>,
    ) -> TemporalResult<()> {
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
    ///
    /// The episode count (list length) feeds [`FeedbackStore`] seeding via
    /// [`seed_feedback_history`](Self::seed_feedback_history).
    ///
    /// [`FeedbackStore`]: crate::algorithms::FeedbackStore
    pub async fn agent_coalition_history(
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

// =============================================================================
// Policy-aware membership operations
// =============================================================================
//
// These bridge the [`CoalitionDecisionPolicy`] family (which reasons over
// `&dyn AgentCapabilities`) to the graph mutations above. They are only
// available when the vertex weight `V` itself exposes
// [`AgentCapabilities`], so the manager can build the capability views a
// policy needs without the caller threading them through by hand. The policy
// is taken as `&dyn CoalitionDecisionPolicy`, so the AIF strategy (feature
// `decision`) and the always-available `ThresholdPolicy` are interchangeable
// here and `AifDecisionPolicy` is never named at this seam.
impl<V, HE> CoalitionManager<V, HE>
where
    V: VertexTrait + Clone + AgentCapabilities + 'static,
    HE: HyperedgeTrait + Clone + 'static,
{
    /// Seed the feedback store's personal-best history signal from the
    /// event-sourced membership log.
    ///
    /// For each agent index, its membership-episode count (from
    /// [`agent_coalition_history`](Self::agent_coalition_history) — ongoing
    /// memberships count as one episode) is added to `store` via
    /// [`FeedbackStore::add_history`]. Failures cannot be seeded from the log
    /// (it records memberships, not outcomes); those accrue only through
    /// [`FeedbackStore::record_outcome`] as coalitions produce values.
    ///
    /// **Seed a given store at most once, before outcome recording begins.**
    /// Seeding is not idempotent: the full episode count is recomputed from the
    /// event log and *added* on every call, so re-seeding the same store from
    /// the same log doubles every agent's history. If a mid-slice agent lookup
    /// fails, the error short-circuits and agents earlier in the slice remain
    /// seeded — reseeding into a fresh store is the recovery path.
    pub async fn seed_feedback_history(
        &self,
        agents: &[VertexIndex],
        store: &FeedbackStore,
    ) -> TemporalResult<()> {
        for &idx in agents {
            let id = self.get_agent(idx).await?.agent_id();
            let episodes = self.agent_coalition_history(idx).await.len() as u64;
            store.add_history(id, episodes);
        }
        Ok(())
    }

    /// Fetch the (Copy/Clone) weights of `members`, preserving order.
    async fn agent_weights(&self, members: &[VertexIndex]) -> TemporalResult<Vec<V>> {
        let mut weights = Vec::with_capacity(members.len());
        for &idx in members {
            weights.push(self.get_agent(idx).await?);
        }
        Ok(weights)
    }

    /// Consult `policy` about whether `agent` should join `coalition`, and apply
    /// the join iff the policy says so.
    ///
    /// The decision is computed over the coalition's *current* members
    /// (excluding `agent`) and the candidate's own capabilities, against the
    /// supplied [`DecisionContext`]. The CPU-bound part of the policy runs via
    /// its async offload (`should_join_async`), so this does not block the
    /// runtime worker even for the AIF expected-free-energy policy.
    ///
    /// Returns the policy [`Decision`]. The membership mutation is applied
    /// exactly when `decision.act` is true (and is idempotent if `agent` is
    /// already a member).
    pub async fn try_join_coalition(
        &self,
        agent: VertexIndex,
        coalition: HyperedgeIndex,
        policy: &dyn CoalitionDecisionPolicy,
        ctx: &DecisionContext,
    ) -> TemporalResult<Decision> {
        let agent_weight = self.get_agent(agent).await?;

        // `should_join` convention: `coalition` excludes the candidate.
        let members: Vec<VertexIndex> = self
            .coalition_members(coalition)
            .await?
            .into_iter()
            .filter(|&m| m != agent)
            .collect();
        let weights = self.agent_weights(&members).await?;
        let views: Vec<&dyn AgentCapabilities> =
            weights.iter().map(|w| w as &dyn AgentCapabilities).collect();

        let decision = policy
            .should_join_async(&agent_weight, &views, ctx)
            .await;

        if decision.act {
            self.join_coalition(agent, coalition).await?;
        }
        Ok(decision)
    }

    /// Consult `policy` about whether `agent` should leave `coalition`, and apply
    /// the leave iff the policy says so.
    ///
    /// The decision is computed over the coalition's *current* members
    /// (including `agent`) against the supplied [`DecisionContext`], using the
    /// policy's async offload (`should_leave_async`).
    ///
    /// Returns the policy [`Decision`]. The membership mutation is applied
    /// exactly when `decision.act` is true; as with [`leave_coalition`], leaving
    /// the final member yields [`TemporalError::EmptyCoalition`] rather than
    /// dissolving the coalition.
    ///
    /// [`leave_coalition`]: Self::leave_coalition
    pub async fn try_leave_coalition(
        &self,
        agent: VertexIndex,
        coalition: HyperedgeIndex,
        policy: &dyn CoalitionDecisionPolicy,
        ctx: &DecisionContext,
    ) -> TemporalResult<Decision> {
        // `should_leave` convention: `coalition` includes the agent.
        let members = self.coalition_members(coalition).await?;
        let weights = self.agent_weights(&members).await?;
        let views: Vec<&dyn AgentCapabilities> =
            weights.iter().map(|w| w as &dyn AgentCapabilities).collect();

        // The candidate's view is whichever member equals `agent`; if it is not
        // currently a member there is nothing to leave.
        let Some(pos) = members.iter().position(|&m| m == agent) else {
            return Ok(Decision { act: false, score: 0.0 });
        };
        let agent_view = views[pos];

        let decision = policy.should_leave_async(agent_view, &views, ctx).await;

        if decision.act {
            self.leave_coalition(agent, coalition).await?;
        }
        Ok(decision)
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
