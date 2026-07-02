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
//! Formerly an actor (`CoalitionActor`); now a plain task owning the
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
// Decision tap (feature-INDEPENDENT — no surrealdb types here)
// ---------------------------------------------------------------------------

/// Which membership operation a [`DecisionRecord`] describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionKind {
    /// A policy-gated join opportunity.
    Join,
    /// A policy-gated leave opportunity.
    Leave,
}

impl DecisionKind {
    /// Stable lowercase wire label (`"join"` / `"leave"`).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            DecisionKind::Join => "join",
            DecisionKind::Leave => "leave",
        }
    }
}

/// A record of one policy-consulted join/leave decision, emitted on the
/// [`CoalitionService`]'s optional decision tap.
///
/// This is a plain koalisi struct — deliberately free of any durable-messaging
/// (surrealdb) types so the core seam stays feature-independent. The `durable`
/// feature converts it into a serializable event at the tier boundary (see
/// [`crate::subsystems`]`::durable::DecisionEvent`).
///
/// `coalition` is a stable label derived from the hyperedge index
/// (`"coalition-<n>"`); `agent_id` is the topology `VertexIndex`'s inner
/// `usize` (the graph vertex, not the domain `AgentCapabilities::agent_id`).
#[derive(Debug, Clone, PartialEq)]
pub struct DecisionRecord {
    pub coalition: String,
    pub agent_id: usize,
    pub kind: DecisionKind,
    pub act: bool,
    pub score: f64,
}

/// Emit a [`DecisionRecord`] on the tap without ever stalling or failing the
/// decision path.
///
/// Uses non-blocking [`try_send`](mpsc::Sender::try_send): a full or closed tap
/// drops the record with a `warn`, so a slow/absent durable consumer can never
/// apply backpressure to (or error) coalition membership decisions.
fn emit_decision(
    tap: Option<&mpsc::Sender<DecisionRecord>>,
    coalition: HyperedgeIndex,
    agent: VertexIndex,
    kind: DecisionKind,
    decision: &Decision,
) {
    let Some(tx) = tap else { return };
    let record = DecisionRecord {
        coalition: format!("coalition-{}", usize::from(coalition)),
        agent_id: usize::from(agent),
        kind,
        act: decision.act,
        score: decision.score,
    };
    match tx.try_send(record) {
        Ok(()) => {}
        Err(mpsc::error::TrySendError::Full(_)) => {
            tracing::warn!(
                kind = kind.as_str(),
                "decision tap full — dropping decision record (decision unaffected)"
            );
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            tracing::warn!(
                kind = kind.as_str(),
                "decision tap closed — dropping decision record (decision unaffected)"
            );
        }
    }
}

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
        Self::spawn_inner(manager, policy, ctx, None)
    }

    /// Like [`spawn`](Self::spawn) but with a decision tap: every
    /// policy-consulted join/leave that yields a [`Decision`] emits a
    /// [`DecisionRecord`] on `tap`.
    ///
    /// The tap is drained by a downstream consumer (e.g. the `durable` feature's
    /// decision-event forwarder). It is best-effort: a full or closed tap drops
    /// records with a `warn` and NEVER stalls or fails a decision — the
    /// coalition membership path is unaffected by tap health.
    pub fn spawn_with_tap<V, HE>(
        manager: CoalitionManager<V, HE>,
        policy: Box<dyn CoalitionDecisionPolicy>,
        ctx: DecisionContext,
        tap: mpsc::Sender<DecisionRecord>,
    ) -> CoalitionServiceHandle
    where
        V: VertexTrait + Clone + AgentCapabilities + 'static,
        HE: HyperedgeTrait + Clone + 'static,
    {
        Self::spawn_inner(manager, policy, ctx, Some(tap))
    }

    fn spawn_inner<V, HE>(
        manager: CoalitionManager<V, HE>,
        policy: Box<dyn CoalitionDecisionPolicy>,
        ctx: DecisionContext,
        tap: Option<mpsc::Sender<DecisionRecord>>,
    ) -> CoalitionServiceHandle
    where
        V: VertexTrait + Clone + AgentCapabilities + 'static,
        HE: HyperedgeTrait + Clone + 'static,
    {
        let (tx, rx) = mpsc::channel(COMMAND_CAPACITY);
        tracing::info!(
            required_capabilities = ctx.required_capabilities,
            decision_tap = tap.is_some(),
            "CoalitionService started"
        );
        tokio::spawn(service_loop(rx, manager, policy, ctx, tap));
        CoalitionServiceHandle { tx }
    }
}

async fn service_loop<V, HE>(
    mut rx: mpsc::Receiver<CoalitionCommand>,
    manager: CoalitionManager<V, HE>,
    policy: Box<dyn CoalitionDecisionPolicy>,
    ctx: DecisionContext,
    tap: Option<mpsc::Sender<DecisionRecord>>,
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
                // Tap every consulted decision (join or decline) before replying;
                // a manager error (no Decision produced) is not tapped.
                if let Ok(decision) = &r {
                    emit_decision(tap.as_ref(), coalition, agent, DecisionKind::Join, decision);
                }
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
                if let Ok(decision) = &r {
                    emit_decision(tap.as_ref(), coalition, agent, DecisionKind::Leave, decision);
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algorithms::AdditiveCalculator;
    use crate::decision::ThresholdPolicy;

    /// A vertex weight that is both a valid graph weight and an agent.
    #[derive(Debug, Copy, Clone, Eq, PartialEq)]
    struct Worker {
        id: usize,
        caps: u32,
        trust: u32,
    }

    impl AgentCapabilities for Worker {
        fn agent_id(&self) -> usize {
            self.id
        }
        fn capabilities(&self) -> u32 {
            self.caps
        }
        fn trust_level(&self) -> u32 {
            self.trust
        }
    }

    /// Build a manager with a one-member seed coalition plus one candidate.
    async fn seed() -> (
        CoalitionManager<Worker, ()>,
        VertexIndex,    // candidate (caps 0b001)
        HyperedgeIndex, // coalition
    ) {
        let manager = CoalitionManager::<Worker, ()>::empty();
        let m = manager
            .add_agent(Worker {
                id: 2,
                caps: 0b010,
                trust: 90,
            })
            .await
            .expect("add seed");
        let coalition = manager.form_coalition(vec![m], ()).await.expect("form");
        let candidate = manager
            .add_agent(Worker {
                id: 1,
                caps: 0b001,
                trust: 80,
            })
            .await
            .expect("add candidate");
        (manager, candidate, coalition)
    }

    /// The tap emits a record for a consulted decision, and the decision is still
    /// applied to membership. (Feature-off — no surrealdb involved.)
    #[tokio::test]
    async fn tap_emits_record_and_applies_decision() {
        let (manager, candidate, coalition) = seed().await;

        let (tap_tx, mut tap_rx) = mpsc::channel(8);
        let service = CoalitionService::spawn_with_tap(
            manager,
            Box::new(ThresholdPolicy::new(AdditiveCalculator, 0.0, 0.0)),
            DecisionContext::default(),
            tap_tx,
        );

        let decision = service.join(candidate, coalition).await.expect("decision");
        assert!(decision.act, "positive marginal ⇒ join");

        // The reply returns only after the tap emit, so the record is buffered.
        let record = tap_rx.recv().await.expect("tap record");
        assert_eq!(record.agent_id, usize::from(candidate));
        assert_eq!(record.coalition, format!("coalition-{}", usize::from(coalition)));
        assert_eq!(record.kind, DecisionKind::Join);
        assert!(record.act);
        assert_eq!(record.score, decision.score);

        let members = service.members(coalition).await.expect("members");
        assert_eq!(members.len(), 2, "candidate actually added");
        assert!(members.contains(&candidate));
    }

    /// A closed tap must NOT stall or fail decisions: the record is dropped with
    /// a warn, and membership is mutated exactly as without a tap.
    #[tokio::test]
    async fn closed_tap_does_not_affect_decision() {
        let (manager, candidate, coalition) = seed().await;

        let (tap_tx, tap_rx) = mpsc::channel(1);
        drop(tap_rx); // close the tap before any decision

        let service = CoalitionService::spawn_with_tap(
            manager,
            Box::new(ThresholdPolicy::new(AdditiveCalculator, 0.0, 0.0)),
            DecisionContext::default(),
            tap_tx,
        );

        let decision = service.join(candidate, coalition).await.expect("decision");
        assert!(decision.act, "decision unaffected by a closed tap");

        let members = service.members(coalition).await.expect("members");
        assert_eq!(members.len(), 2, "join applied despite closed tap");
        assert!(members.contains(&candidate));
    }
}
