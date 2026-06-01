//! `CoalitionActor` — the live kameo seam for policy-driven coalition
//! membership.
//!
//! This is the call site issue #1 asked for: a real actor that, on a join or
//! leave opportunity, consults a [`CoalitionDecisionPolicy`] before mutating a
//! [`CoalitionManager`]'s membership. The policy is held as a
//! `Box<dyn CoalitionDecisionPolicy>`, so the AIF expected-free-energy strategy
//! (feature `decision`) and the always-available
//! [`ThresholdPolicy`](crate::decision::ThresholdPolicy) are interchangeable and
//! `AifDecisionPolicy` is never named here. The CPU-bound part of the decision
//! runs through the policy's async offload, so the tokio worker is not blocked
//! even for the AIF policy.
//!
//! The [`DecisionContext`] (the task's `required_capabilities`) is owned by the
//! actor and built from its configured task, not hardcoded at each call.
//!
//! ```ignore
//! let actor = CoalitionActor::spawn(CoalitionActorArgs {
//!     manager,                                  // CoalitionManager<Agent, Team>
//!     policy: Box::new(ThresholdPolicy::new(AdditiveCalculator, 0.0, 0.0)),
//!     ctx: DecisionContext { required_capabilities: 0b111 },
//! });
//! let decision = actor.ask(JoinRequest { agent, coalition }).await??;
//! ```

use kameo::error::Infallible;
use kameo::prelude::*;

use crate::algorithms::AgentCapabilities;
use crate::decision::{CoalitionDecisionPolicy, Decision, DecisionContext};
use crate::topology::{CoalitionManager, HyperedgeIndex, HyperedgeTrait, VertexIndex, VertexTrait};

// ---------------------------------------------------------------------------
// Args
// ---------------------------------------------------------------------------

/// Construction arguments for [`CoalitionActor`].
pub struct CoalitionActorArgs<V, HE>
where
    V: VertexTrait + Clone + AgentCapabilities + 'static,
    HE: HyperedgeTrait + Clone + 'static,
{
    /// The graph this actor governs. The actor takes ownership; route all
    /// membership changes through the actor so policy consultation cannot be
    /// bypassed.
    pub manager: CoalitionManager<V, HE>,
    /// The selectable decision strategy. `ThresholdPolicy` or, behind feature
    /// `decision`, `AifDecisionPolicy` — both reach the non-blocking async path.
    pub policy: Box<dyn CoalitionDecisionPolicy>,
    /// The task's capability requirements, consulted by capability-aware
    /// policies. Built from the task/coalition, not hardcoded per call.
    pub ctx: DecisionContext,
}

// ---------------------------------------------------------------------------
// Actor
// ---------------------------------------------------------------------------

/// A kameo actor that gates coalition join/leave behind a
/// [`CoalitionDecisionPolicy`].
pub struct CoalitionActor<V, HE>
where
    V: VertexTrait + Clone + AgentCapabilities + 'static,
    HE: HyperedgeTrait + Clone + 'static,
{
    manager: CoalitionManager<V, HE>,
    policy: Box<dyn CoalitionDecisionPolicy>,
    ctx: DecisionContext,
}

impl<V, HE> Actor for CoalitionActor<V, HE>
where
    V: VertexTrait + Clone + AgentCapabilities + 'static,
    HE: HyperedgeTrait + Clone + 'static,
{
    type Args = CoalitionActorArgs<V, HE>;
    type Error = Infallible;

    async fn on_start(args: Self::Args, _actor_ref: ActorRef<Self>) -> Result<Self, Self::Error> {
        tracing::info!(
            required_capabilities = args.ctx.required_capabilities,
            "CoalitionActor started"
        );
        Ok(Self {
            manager: args.manager,
            policy: args.policy,
            ctx: args.ctx,
        })
    }
}

// ---------------------------------------------------------------------------
// Message: JoinRequest
// ---------------------------------------------------------------------------

/// Ask the actor to consider `agent` joining `coalition`. The membership is
/// mutated iff the policy returns `act == true`; the [`Decision`] is returned
/// either way.
#[derive(Debug, Clone, Copy)]
pub struct JoinRequest {
    pub agent: VertexIndex,
    pub coalition: HyperedgeIndex,
}

impl<V, HE> Message<JoinRequest> for CoalitionActor<V, HE>
where
    V: VertexTrait + Clone + AgentCapabilities + 'static,
    HE: HyperedgeTrait + Clone + 'static,
{
    type Reply = Result<Decision, String>;

    async fn handle(
        &mut self,
        req: JoinRequest,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.manager
            .try_join_coalition(req.agent, req.coalition, self.policy.as_ref(), &self.ctx)
            .await
            .map_err(|e| e.to_string())
    }
}

// ---------------------------------------------------------------------------
// Message: LeaveRequest
// ---------------------------------------------------------------------------

/// Ask the actor to consider `agent` leaving `coalition`. The membership is
/// mutated iff the policy returns `act == true`. Leaving the final member
/// surfaces the manager's `EmptyCoalition` error as `Err`.
#[derive(Debug, Clone, Copy)]
pub struct LeaveRequest {
    pub agent: VertexIndex,
    pub coalition: HyperedgeIndex,
}

impl<V, HE> Message<LeaveRequest> for CoalitionActor<V, HE>
where
    V: VertexTrait + Clone + AgentCapabilities + 'static,
    HE: HyperedgeTrait + Clone + 'static,
{
    type Reply = Result<Decision, String>;

    async fn handle(
        &mut self,
        req: LeaveRequest,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.manager
            .try_leave_coalition(req.agent, req.coalition, self.policy.as_ref(), &self.ctx)
            .await
            .map_err(|e| e.to_string())
    }
}

// ---------------------------------------------------------------------------
// Message: Members — introspection (used by tests)
// ---------------------------------------------------------------------------

/// Read the current members of `coalition`.
#[derive(Debug, Clone, Copy)]
pub struct Members {
    pub coalition: HyperedgeIndex,
}

impl<V, HE> Message<Members> for CoalitionActor<V, HE>
where
    V: VertexTrait + Clone + AgentCapabilities + 'static,
    HE: HyperedgeTrait + Clone + 'static,
{
    type Reply = Result<Vec<VertexIndex>, String>;

    async fn handle(
        &mut self,
        req: Members,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.manager
            .coalition_members(req.coalition)
            .await
            .map_err(|e| e.to_string())
    }
}
