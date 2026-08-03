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
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

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

/// Spawn a task that drains ONE decision tap and fans each record out to every
/// sink channel.
///
/// The [`CoalitionService`] carries a single `Option<mpsc::Sender<DecisionRecord>>`
/// tap. When more than one consumer needs the decision stream — e.g. the
/// `durable` decision log *and* the `remote` coalition-event gateway (issue #38)
/// — this tee is the fan-out point: give the service the tee's input `Sender`
/// and hand each consumer one of `sinks`.
///
/// ## Delivery contract: at-most-once, one MORE lossy hop
///
/// The tap itself is already best-effort (see [`emit_decision`] — `try_send`,
/// drop-with-warn). This tee adds a second such hop: each sink receives a clone
/// via non-blocking [`try_send`](mpsc::Sender::try_send), and a full or closed
/// sink drops that record with a `warn`. A slow sink therefore thins its OWN
/// stream and never blocks the loop, the other sinks, or the decision path.
/// Sinks are independent: a closed sink is tolerated for the lifetime of the
/// tee (it simply never receives). Size each sink channel for the producer's
/// peak burst rate — drops correlate in time.
///
/// No `catch_unwind` is needed here (unlike
/// [`spawn_outcome_forwarder`](crate::subsystems::outcome::spawn_outcome_forwarder)'s
/// per-sink isolation): a sink is an `mpsc::Sender`, so this task runs no
/// caller-supplied code.
///
/// ## Shutdown: pick ONE discipline, don't mix
///
/// Mirrors [`spawn_outcome_forwarder`](crate::subsystems::outcome::spawn_outcome_forwarder):
///
/// - **Lossless shutdown**: drop every tap `Sender` (i.e. the
///   [`CoalitionService`]) → `rx.recv()` yields `None` after buffered records
///   drain → the tee exits, having pushed everything it held into the sinks.
/// - **Prompt teardown**: cancelling `token` breaks the loop immediately; a
///   buffered record may be dropped (still at-most-once).
///
/// Spawned on `tracker` so the caller's cancel→close→drain shutdown covers it.
pub fn spawn_decision_tee(
    mut rx: mpsc::Receiver<DecisionRecord>,
    sinks: Vec<mpsc::Sender<DecisionRecord>>,
    tracker: &TaskTracker,
    token: CancellationToken,
) -> JoinHandle<()> {
    tracker.spawn(async move {
        loop {
            tokio::select! {
                biased;
                _ = token.cancelled() => break,
                maybe = rx.recv() => match maybe {
                    Some(record) => {
                        for (i, sink) in sinks.iter().enumerate() {
                            match sink.try_send(record.clone()) {
                                Ok(()) => {}
                                Err(mpsc::error::TrySendError::Full(_)) => {
                                    tracing::warn!(
                                        sink = i,
                                        kind = record.kind.as_str(),
                                        "decision tee sink full — dropping record (other sinks unaffected)"
                                    );
                                }
                                Err(mpsc::error::TrySendError::Closed(_)) => {
                                    tracing::warn!(
                                        sink = i,
                                        kind = record.kind.as_str(),
                                        "decision tee sink closed — dropping record (other sinks unaffected)"
                                    );
                                }
                            }
                        }
                    }
                    None => break, // tap dropped; nothing more to tee
                },
            }
        }
        tracing::debug!("decision tee stopped");
    })
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

    // -----------------------------------------------------------------------
    // spawn_decision_tee
    // -----------------------------------------------------------------------

    fn record(agent_id: usize) -> DecisionRecord {
        DecisionRecord {
            coalition: "coalition-0".to_string(),
            agent_id,
            kind: DecisionKind::Join,
            act: true,
            score: 1.5,
        }
    }

    /// Both sinks see every record, in order, and the tee drains after the tap
    /// sender is dropped (the lossless path).
    #[tokio::test]
    async fn tee_fans_out_to_every_sink_and_drains() {
        let tracker = TaskTracker::new();
        let token = CancellationToken::new();
        let (tap_tx, tap_rx) = mpsc::channel(16);
        let (a_tx, mut a_rx) = mpsc::channel(16);
        let (b_tx, mut b_rx) = mpsc::channel(16);

        let handle = spawn_decision_tee(tap_rx, vec![a_tx, b_tx], &tracker, token);

        tap_tx.send(record(1)).await.expect("send 1");
        tap_tx.send(record(2)).await.expect("send 2");

        // Lossless shutdown: drop the only tap sender ⇒ drain, then exit.
        drop(tap_tx);
        handle.await.expect("tee joins");

        for rx in [&mut a_rx, &mut b_rx] {
            assert_eq!(rx.recv().await.expect("record 1").agent_id, 1);
            assert_eq!(rx.recv().await.expect("record 2").agent_id, 2);
            assert!(rx.recv().await.is_none(), "sink closed after the tee exits");
        }

        tracker.close();
        tracker.wait().await;
    }

    /// A FULL sink drops its own records without stalling the loop or starving
    /// the other sink; a CLOSED sink is tolerated for the tee's lifetime.
    #[tokio::test]
    async fn tee_tolerates_full_and_closed_sinks() {
        let tracker = TaskTracker::new();
        let token = CancellationToken::new();
        let (tap_tx, tap_rx) = mpsc::channel(16);
        // Capacity-1 sink that is never read ⇒ full after the first record.
        let (full_tx, _full_rx) = mpsc::channel(1);
        // Closed sink: receiver dropped before the tee ever runs.
        let (closed_tx, closed_rx) = mpsc::channel(16);
        drop(closed_rx);
        let (ok_tx, mut ok_rx) = mpsc::channel(16);

        let handle = spawn_decision_tee(tap_rx, vec![full_tx, closed_tx, ok_tx], &tracker, token);

        for id in 1..=3 {
            tap_tx.send(record(id)).await.expect("send");
        }
        drop(tap_tx);
        handle.await.expect("tee joins despite full + closed sinks");

        // The healthy sink saw all three — neither the full nor the closed sink
        // cost it anything.
        for id in 1..=3 {
            assert_eq!(ok_rx.recv().await.expect("healthy record").agent_id, id);
        }

        tracker.close();
        tracker.wait().await;
    }

    /// Cancelling the token exits the tee promptly even with a live tap sender.
    #[tokio::test]
    async fn tee_cancel_exits_promptly() {
        let tracker = TaskTracker::new();
        let token = CancellationToken::new();
        let (tap_tx, tap_rx) = mpsc::channel::<DecisionRecord>(16);
        let handle = spawn_decision_tee(tap_rx, vec![], &tracker, token.clone());

        token.cancel();
        handle.await.expect("tee joins on cancel");

        drop(tap_tx);
        tracker.close();
        tracker.wait().await;
    }
}
