//! `CoalitionService` — the live seam for policy-driven coalition membership.
//!
//! This is the call site issue #1 asked for: a task that, on a join or leave
//! opportunity, consults a [`CoalitionDecisionPolicy`] before mutating a
//! [`CoalitionManager`]'s membership. The policy is held as a
//! `Box<dyn CoalitionDecisionPolicy>`, so the AIF expected-free-energy strategy
//! (feature `decision`) and the always-available
//! [`ThresholdPolicy`](crate::decision::ThresholdPolicy) are interchangeable and
//! `AifDecisionPolicy` is never named here. The CPU-bound part of the decision
//! runs through the policy's async offload, so the tokio worker is not blocked
//! even for the AIF policy.
//!
//! ## Runtime shape (post-K3, issue #6)
//!
//! Formerly a kameo actor (`CoalitionActor`); now a plain task owning the
//! `CoalitionManager` + policy + [`DecisionContext`], driven by a
//! `tokio::sync::mpsc` command channel with `oneshot`-correlated replies. The
//! file keeps its historical name (`coalition_actor.rs`); the type is
//! [`CoalitionService`] with a [`CoalitionServiceHandle`]. The task is
//! self-managed: it exits when the last handle is dropped (its command channel
//! closes), mirroring the old actor's self-managed lifecycle.
//!
//! ```ignore
//! let service = CoalitionService::spawn(
//!     manager,                                  // CoalitionManager<Agent, Team>
//!     Box::new(ThresholdPolicy::new(AdditiveCalculator, 0.0, 0.0)),
//!     DecisionContext { required_capabilities: 0b111 },
//! );
//! let decision = service.join(agent, coalition).await?;
//! ```

use anyhow::{Result, anyhow};
use tokio::sync::{mpsc, oneshot};

use crate::algorithms::AgentCapabilities;
use crate::decision::{CoalitionDecisionPolicy, Decision, DecisionContext};
use crate::topology::{CoalitionManager, HyperedgeIndex, HyperedgeTrait, VertexIndex, VertexTrait};

const COMMAND_CAPACITY: usize = 64;

// ---------------------------------------------------------------------------
// Commands (non-generic: indices + replies are all concrete types)
// ---------------------------------------------------------------------------

enum CoalitionCommand {
    Join {
        agent: VertexIndex,
        coalition: HyperedgeIndex,
        reply: oneshot::Sender<Result<Decision, String>>,
    },
    Leave {
        agent: VertexIndex,
        coalition: HyperedgeIndex,
        reply: oneshot::Sender<Result<Decision, String>>,
    },
    Members {
        coalition: HyperedgeIndex,
        reply: oneshot::Sender<Result<Vec<VertexIndex>, String>>,
    },
}

// ---------------------------------------------------------------------------
// Handle
// ---------------------------------------------------------------------------

/// Clone-able handle to a spawned [`CoalitionService`] task.
#[derive(Clone)]
pub struct CoalitionServiceHandle {
    tx: mpsc::Sender<CoalitionCommand>,
}

impl CoalitionServiceHandle {
    /// Ask the service to consider `agent` joining `coalition`. Membership is
    /// mutated iff the policy returns `act == true`; the [`Decision`] is
    /// returned either way.
    pub async fn join(&self, agent: VertexIndex, coalition: HyperedgeIndex) -> Result<Decision> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(CoalitionCommand::Join {
                agent,
                coalition,
                reply,
            })
            .await
            .map_err(|_| anyhow!("coalition service is gone"))?;
        rx.await
            .map_err(|_| anyhow!("coalition service dropped reply"))?
            .map_err(|e| anyhow!(e))
    }

    /// Ask the service to consider `agent` leaving `coalition`. Membership is
    /// mutated iff the policy returns `act == true`. Leaving the final member
    /// surfaces the manager's `EmptyCoalition` error.
    pub async fn leave(&self, agent: VertexIndex, coalition: HyperedgeIndex) -> Result<Decision> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(CoalitionCommand::Leave {
                agent,
                coalition,
                reply,
            })
            .await
            .map_err(|_| anyhow!("coalition service is gone"))?;
        rx.await
            .map_err(|_| anyhow!("coalition service dropped reply"))?
            .map_err(|e| anyhow!(e))
    }

    /// Read the current members of `coalition`.
    pub async fn members(&self, coalition: HyperedgeIndex) -> Result<Vec<VertexIndex>> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(CoalitionCommand::Members { coalition, reply })
            .await
            .map_err(|_| anyhow!("coalition service is gone"))?;
        rx.await
            .map_err(|_| anyhow!("coalition service dropped reply"))?
            .map_err(|e| anyhow!(e))
    }
}

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

/// A task that gates coalition join/leave behind a
/// [`CoalitionDecisionPolicy`]. Construct via [`CoalitionService::spawn`].
pub struct CoalitionService;

impl CoalitionService {
    /// Spawn the service task. It owns `manager`, consults `policy` (via its
    /// async offload) before mutating membership, and uses `ctx`'s
    /// `required_capabilities` for capability-aware policies. Returns a
    /// clone-able [`CoalitionServiceHandle`].
    pub fn spawn<V, HE>(
        manager: CoalitionManager<V, HE>,
        policy: Box<dyn CoalitionDecisionPolicy>,
        ctx: DecisionContext,
    ) -> CoalitionServiceHandle
    where
        V: VertexTrait + Clone + AgentCapabilities + 'static,
        HE: HyperedgeTrait + Clone + 'static,
    {
        let (tx, rx) = mpsc::channel(COMMAND_CAPACITY);
        tracing::info!(
            required_capabilities = ctx.required_capabilities,
            "CoalitionService started"
        );
        tokio::spawn(service_loop(rx, manager, policy, ctx));
        CoalitionServiceHandle { tx }
    }
}

async fn service_loop<V, HE>(
    mut rx: mpsc::Receiver<CoalitionCommand>,
    manager: CoalitionManager<V, HE>,
    policy: Box<dyn CoalitionDecisionPolicy>,
    ctx: DecisionContext,
) where
    V: VertexTrait + Clone + AgentCapabilities + 'static,
    HE: HyperedgeTrait + Clone + 'static,
{
    while let Some(cmd) = rx.recv().await {
        match cmd {
            CoalitionCommand::Join {
                agent,
                coalition,
                reply,
            } => {
                let r = manager
                    .try_join_coalition(agent, coalition, policy.as_ref(), &ctx)
                    .await
                    .map_err(|e| e.to_string());
                let _ = reply.send(r);
            }
            CoalitionCommand::Leave {
                agent,
                coalition,
                reply,
            } => {
                let r = manager
                    .try_leave_coalition(agent, coalition, policy.as_ref(), &ctx)
                    .await
                    .map_err(|e| e.to_string());
                let _ = reply.send(r);
            }
            CoalitionCommand::Members { coalition, reply } => {
                let r = manager
                    .coalition_members(coalition)
                    .await
                    .map_err(|e| e.to_string());
                let _ = reply.send(r);
            }
        }
    }
    tracing::debug!("CoalitionService stopped");
}
