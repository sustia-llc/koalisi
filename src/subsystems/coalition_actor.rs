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
//! [`CoalitionService`] with a [`CoalitionServiceHandle`]. The task runs on the
//! caller's [`TaskTracker`] and exits when the caller's [`CancellationToken`]
//! is cancelled or the last handle is dropped (its command channel closes).
//! Every handle command returns a [`ServiceError`] on failure. On
//! cancellation every queued command is answered with
//! [`ServiceError::Cancelled`]. A panic while handling a command is logged at
//! error level, sets [`CoalitionServiceHandle::panicked`], answers that
//! command with [`ServiceError::Panicked`] and ends the task.

use std::any::Any;
use std::future::Future;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures::FutureExt;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::algorithms::AgentCapabilities;
use crate::decision::{CoalitionDecisionPolicy, Decision, DecisionContext};
use crate::topology::{
    CoalitionManager, HyperedgeIndex, HyperedgeTrait, TemporalError, VertexIndex, VertexTrait,
};

const COMMAND_CAPACITY: usize = 64;

/// The failure of a [`CoalitionServiceHandle`] join, leave or members command.
#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    /// The service task had stopped taking commands when the command was sent.
    #[error("coalition service is gone")]
    Gone,
    /// The service task ended without answering the command.
    #[error("coalition service dropped reply")]
    ReplyDropped,
    /// The command was still queued when the service's token was cancelled.
    #[error("coalition service stopped: cancelled")]
    Cancelled,
    /// Handling this command panicked; `message` is the panic payload's text.
    #[error("coalition service panicked: {message}")]
    Panicked { message: String },
    /// The coalition manager failed the command with a topology error.
    #[error(transparent)]
    Topology(#[from] TemporalError),
}

impl ServiceError {
    /// `true` for [`Gone`](Self::Gone), [`ReplyDropped`](Self::ReplyDropped),
    /// [`Cancelled`](Self::Cancelled), [`Panicked`](Self::Panicked),
    /// `Topology(`[`TemporalError::LockPoisoned`]`)` and
    /// `Topology(`[`TemporalError::TaskCancelled`]`)`; `false` for every other
    /// `Topology` error.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        match self {
            Self::Gone | Self::ReplyDropped | Self::Cancelled | Self::Panicked { .. } => true,
            Self::Topology(error) => match error {
                TemporalError::LockPoisoned | TemporalError::TaskCancelled => true,
                TemporalError::Hypergraph(_)
                | TemporalError::TimestampInFuture { .. }
                | TemporalError::NoSnapshotBefore(_)
                | TemporalError::SnapshotNotFound(_)
                | TemporalError::ReplayError { .. }
                | TemporalError::InvalidTimeRange { .. }
                | TemporalError::EventLogCorrupted(_)
                | TemporalError::ConcurrentModification
                | TemporalError::EmptyCoalition => false,
            },
        }
    }
}

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

/// A record of one join/leave decision, emitted on the
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

/// A [`DecisionRecord`] plus the topology position the decision was taken at,
/// emitted on the tap installed by [`CoalitionService::spawn_with_trace_tap`].
///
/// Both position fields are read by the service task immediately before it
/// calls the manager for this decision:
///
/// - `event_log_len` — the length of the manager's in-memory event log
///   ([`TemporalHypergraph::events_ref`](crate::topology::TemporalHypergraph::events_ref)).
///   A mutation recorded by this decision (an `act == true` join or leave) is
///   not counted. A mutation recorded between the read and the policy consult
///   by another holder of the same graph is not counted either.
/// - `timestamp` — the manager's logical clock
///   ([`CoalitionManager::current_timestamp`]), i.e. the timestamp the next
///   recorded mutation receives.
#[derive(Debug, Clone, PartialEq)]
pub struct DecisionTrace {
    /// The decision, identical to what [`CoalitionService::spawn_with_tap`]'s
    /// tap would carry for it.
    pub record: DecisionRecord,
    /// In-memory event-log length before the manager call.
    pub event_log_len: u64,
    /// Logical-clock value before the manager call.
    pub timestamp: u64,
}

/// Topology position read before a manager call; see [`DecisionTrace`].
#[derive(Clone, Copy)]
struct DecisionPosition {
    event_log_len: u64,
    timestamp: u64,
}

/// Read the [`DecisionPosition`] iff a trace tap is installed; `None` (and no
/// lock taken) otherwise.
async fn decision_position<V, HE>(
    trace_tap: Option<&mpsc::Sender<DecisionTrace>>,
    manager: &CoalitionManager<V, HE>,
) -> Option<DecisionPosition>
where
    V: VertexTrait + Clone + 'static,
    HE: HyperedgeTrait + Clone + 'static,
{
    trace_tap?;
    let len = manager.graph().events_ref().read().await.len();
    let timestamp = manager.current_timestamp().value();
    Some(DecisionPosition {
        // usize → u64: an in-memory log length always fits.
        event_log_len: len as u64,
        timestamp,
    })
}

fn decision_record(
    coalition: HyperedgeIndex,
    agent: VertexIndex,
    kind: DecisionKind,
    decision: &Decision,
) -> DecisionRecord {
    DecisionRecord {
        coalition: format!("coalition-{}", usize::from(coalition)),
        agent_id: usize::from(agent),
        kind,
        act: decision.act,
        score: decision.score,
    }
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
    let record = decision_record(coalition, agent, kind, decision);
    try_emit(tx, record, "decision tap", "decision record");
}

/// An item a decision tap carries: it holds one [`DecisionRecord`].
trait TapItem {
    fn record(&self) -> &DecisionRecord;
}

impl TapItem for DecisionRecord {
    fn record(&self) -> &DecisionRecord {
        self
    }
}

impl TapItem for DecisionTrace {
    fn record(&self) -> &DecisionRecord {
        &self.record
    }
}

/// `try_send` `item` on `tx`; a full or closed channel drops it with a `warn`
/// naming `tap` and `what`, with the record's coalition, agent id and kind as
/// fields. Never blocks.
fn try_emit<T: TapItem>(tx: &mpsc::Sender<T>, item: T, tap: &str, what: &str) {
    let (state, item) = match tx.try_send(item) {
        Ok(()) => return,
        Err(mpsc::error::TrySendError::Full(item)) => ("full", item),
        Err(mpsc::error::TrySendError::Closed(item)) => ("closed", item),
    };
    let r = item.record();
    tracing::warn!(
        coalition = %r.coalition,
        agent_id = r.agent_id,
        kind = r.kind.as_str(),
        "{tap} {state} — dropping {what} (decision unaffected)"
    );
}

/// Emit a [`DecisionTrace`] on the trace tap, with the same non-blocking
/// drop-with-warn contract as [`emit_decision`]. A no-op unless both the tap
/// and the position read before the manager call are present.
fn emit_trace(
    trace_tap: Option<&mpsc::Sender<DecisionTrace>>,
    position: Option<DecisionPosition>,
    coalition: HyperedgeIndex,
    agent: VertexIndex,
    kind: DecisionKind,
    decision: &Decision,
) {
    let (Some(tx), Some(position)) = (trace_tap, position) else {
        return;
    };
    let trace = DecisionTrace {
        record: decision_record(coalition, agent, kind, decision),
        event_log_len: position.event_log_len,
        timestamp: position.timestamp,
    };
    try_emit(tx, trace, "decision trace tap", "decision trace");
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
/// The tap itself is already best-effort (see `emit_decision` — `try_send`,
/// drop-with-warn). This tee adds a second such hop: each sink receives a clone
/// via non-blocking [`try_send`](mpsc::Sender::try_send), and a full or closed
/// sink drops that record with a `warn` carrying the sink's index in `sinks`
/// and the record's coalition, agent id and kind. A slow sink therefore thins its OWN
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
                                        coalition = %record.coalition,
                                        agent_id = record.agent_id,
                                        kind = record.kind.as_str(),
                                        "decision tee sink full — dropping record (other sinks unaffected)"
                                    );
                                }
                                Err(mpsc::error::TrySendError::Closed(_)) => {
                                    tracing::warn!(
                                        sink = i,
                                        coalition = %record.coalition,
                                        agent_id = record.agent_id,
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
        reply: oneshot::Sender<Result<Decision, ServiceError>>,
    },
    Leave {
        agent: VertexIndex,
        coalition: HyperedgeIndex,
        reply: oneshot::Sender<Result<Decision, ServiceError>>,
    },
    Members {
        coalition: HyperedgeIndex,
        reply: oneshot::Sender<Result<Vec<VertexIndex>, ServiceError>>,
    },
}

// ---------------------------------------------------------------------------
// Handle
// ---------------------------------------------------------------------------

/// Clone-able handle to a spawned [`CoalitionService`] task.
#[derive(Clone)]
pub struct CoalitionServiceHandle {
    tx: mpsc::Sender<CoalitionCommand>,
    panicked: Arc<AtomicBool>,
}

impl CoalitionServiceHandle {
    /// Whether the service task has ended in a panic. `false` while the task
    /// runs and after it stops on cancellation. After a panic it reads `true`
    /// to the caller whose command panicked, from the moment its
    /// [`ServiceError::Panicked`] returns.
    #[must_use]
    pub fn panicked(&self) -> bool {
        self.panicked.load(Ordering::SeqCst)
    }

    /// Resolves once the service task has stopped taking commands, on
    /// cancellation or on a panic. After a panic, [`panicked`](Self::panicked)
    /// reads `true` when this resolves.
    pub async fn closed(&self) {
        self.tx.closed().await;
    }

    /// Ask the service to consider `agent` joining `coalition`. Membership is
    /// mutated iff the policy returns `act == true`; the [`Decision`] is
    /// returned either way. A manager error is [`ServiceError::Topology`].
    pub async fn join(
        &self,
        agent: VertexIndex,
        coalition: HyperedgeIndex,
    ) -> Result<Decision, ServiceError> {
        self.request(|reply| CoalitionCommand::Join {
            agent,
            coalition,
            reply,
        })
        .await
    }

    /// Ask the service to consider `agent` leaving `coalition`. Membership is
    /// mutated iff the policy returns `act == true`. Leaving the final member
    /// is [`ServiceError::Topology`] carrying
    /// [`TemporalError::EmptyCoalition`].
    pub async fn leave(
        &self,
        agent: VertexIndex,
        coalition: HyperedgeIndex,
    ) -> Result<Decision, ServiceError> {
        self.request(|reply| CoalitionCommand::Leave {
            agent,
            coalition,
            reply,
        })
        .await
    }

    /// Read the current members of `coalition`; a manager error is
    /// [`ServiceError::Topology`].
    pub async fn members(
        &self,
        coalition: HyperedgeIndex,
    ) -> Result<Vec<VertexIndex>, ServiceError> {
        self.request(|reply| CoalitionCommand::Members { coalition, reply })
            .await
    }

    /// Send the command `make` builds around a fresh reply channel and await
    /// the reply: [`ServiceError::Gone`] when the channel is closed,
    /// [`ServiceError::ReplyDropped`] when the reply sender drops unanswered.
    async fn request<T>(
        &self,
        make: impl FnOnce(oneshot::Sender<Result<T, ServiceError>>) -> CoalitionCommand,
    ) -> Result<T, ServiceError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(make(reply))
            .await
            .map_err(|_| ServiceError::Gone)?;
        rx.await.map_err(|_| ServiceError::ReplyDropped)?
    }
}

// ---------------------------------------------------------------------------
// Service
// ---------------------------------------------------------------------------

/// A task that gates coalition join/leave behind a
/// [`CoalitionDecisionPolicy`]. Construct via [`CoalitionService::spawn`].
pub struct CoalitionService;

impl CoalitionService {
    /// Spawn the service task on `tracker`. It owns `manager`, consults
    /// `policy` (via its async offload) before mutating membership, and uses
    /// `ctx`'s `required_capabilities` for capability-aware policies. Returns
    /// a clone-able [`CoalitionServiceHandle`].
    ///
    /// The task ends when `token` is cancelled or every handle is dropped. A
    /// command already taken from the channel completes first. On
    /// cancellation the task closes the channel and answers every command
    /// still queued with [`ServiceError::Cancelled`]; a command sent after
    /// that fails with [`ServiceError::Gone`]. A panic while handling a
    /// command is logged at error level with its message, sets
    /// [`CoalitionServiceHandle::panicked`], answers that command with
    /// [`ServiceError::Panicked`] and ends the task; every queued command
    /// fails with [`ServiceError::ReplyDropped`].
    pub fn spawn<V, HE>(
        manager: CoalitionManager<V, HE>,
        policy: Box<dyn CoalitionDecisionPolicy>,
        ctx: DecisionContext,
        tracker: &TaskTracker,
        token: CancellationToken,
    ) -> CoalitionServiceHandle
    where
        V: VertexTrait + Clone + AgentCapabilities + 'static,
        HE: HyperedgeTrait + Clone + 'static,
    {
        Self::spawn_inner(manager, policy, ctx, None, None, tracker, token)
    }

    /// Like [`spawn`](Self::spawn) but with a decision tap: every join or
    /// leave the manager answers with a [`Decision`] emits a
    /// [`DecisionRecord`] on `tap` — including a leave of an agent that is not
    /// a member, which the manager answers with `act == false` without
    /// consulting the policy. A manager error emits nothing.
    ///
    /// The tap is drained by a downstream consumer (e.g. the `durable` feature's
    /// decision-event forwarder). It is best-effort: a full or closed tap drops
    /// records with a `warn` and NEVER stalls or fails a decision — the
    /// coalition membership path is unaffected by tap health.
    ///
    /// The task ends, answers queued commands on cancellation and reports a
    /// panic as [`spawn`](Self::spawn) describes.
    pub fn spawn_with_tap<V, HE>(
        manager: CoalitionManager<V, HE>,
        policy: Box<dyn CoalitionDecisionPolicy>,
        ctx: DecisionContext,
        tap: mpsc::Sender<DecisionRecord>,
        tracker: &TaskTracker,
        token: CancellationToken,
    ) -> CoalitionServiceHandle
    where
        V: VertexTrait + Clone + AgentCapabilities + 'static,
        HE: HyperedgeTrait + Clone + 'static,
    {
        Self::spawn_inner(manager, policy, ctx, Some(tap), None, tracker, token)
    }

    /// Like [`spawn`](Self::spawn) but with a decision trace tap: every join or
    /// leave the manager answers with a [`Decision`] emits a [`DecisionTrace`]
    /// on `tap` — the [`DecisionRecord`] plus the event-log length and logical
    /// clock read before the manager call. That includes a leave of an agent
    /// that is not a member, which the manager answers with `act == false`
    /// without consulting the policy. A manager error emits nothing.
    ///
    /// The tap has the same contract as [`spawn_with_tap`](Self::spawn_with_tap)'s:
    /// a full or closed tap drops the trace with a `warn` and never stalls or
    /// fails a decision. The position read (one event-log read lock and one
    /// clock load per join/leave) happens only on a service spawned through
    /// this constructor.
    ///
    /// The task ends, answers queued commands on cancellation and reports a
    /// panic as [`spawn`](Self::spawn) describes.
    pub fn spawn_with_trace_tap<V, HE>(
        manager: CoalitionManager<V, HE>,
        policy: Box<dyn CoalitionDecisionPolicy>,
        ctx: DecisionContext,
        tap: mpsc::Sender<DecisionTrace>,
        tracker: &TaskTracker,
        token: CancellationToken,
    ) -> CoalitionServiceHandle
    where
        V: VertexTrait + Clone + AgentCapabilities + 'static,
        HE: HyperedgeTrait + Clone + 'static,
    {
        Self::spawn_inner(manager, policy, ctx, None, Some(tap), tracker, token)
    }

    fn spawn_inner<V, HE>(
        manager: CoalitionManager<V, HE>,
        policy: Box<dyn CoalitionDecisionPolicy>,
        ctx: DecisionContext,
        tap: Option<mpsc::Sender<DecisionRecord>>,
        trace_tap: Option<mpsc::Sender<DecisionTrace>>,
        tracker: &TaskTracker,
        token: CancellationToken,
    ) -> CoalitionServiceHandle
    where
        V: VertexTrait + Clone + AgentCapabilities + 'static,
        HE: HyperedgeTrait + Clone + 'static,
    {
        let (tx, rx) = mpsc::channel(COMMAND_CAPACITY);
        tracing::info!(
            required_capabilities = ctx.required_capabilities,
            decision_tap = tap.is_some(),
            decision_trace_tap = trace_tap.is_some(),
            "CoalitionService started"
        );
        let panicked = Arc::new(AtomicBool::new(false));
        let task = service_loop(
            rx,
            manager,
            policy,
            ctx,
            tap,
            trace_tap,
            token,
            Arc::clone(&panicked),
        );
        #[cfg(test)]
        let task = faults::inherit(task);
        tracker.spawn(task);
        CoalitionServiceHandle { tx, panicked }
    }
}

/// Await `work` and send its result on `reply`, returning `true`. On a panic,
/// set `panicked`, log the panic message at error level, then send
/// [`ServiceError::Panicked`] with that message on `reply` and return `false`.
async fn serve<T, F>(
    panicked: &AtomicBool,
    reply: oneshot::Sender<Result<T, ServiceError>>,
    work: F,
) -> bool
where
    F: Future<Output = Result<T, ServiceError>>,
{
    match AssertUnwindSafe(work).catch_unwind().await {
        Ok(output) => {
            let _ = reply.send(output);
            true
        }
        Err(payload) => {
            let message = panic_message(payload.as_ref()).to_owned();
            panicked.store(true, Ordering::SeqCst);
            tracing::error!(panic = message.as_str(), "CoalitionService panicked");
            #[cfg(test)]
            faults::between_flag_and_reply().await;
            let _ = reply.send(Err(ServiceError::Panicked { message }));
            false
        }
    }
}

/// Test-only fault injection for the service task. A test sets [`Faults`]
/// with [`FAULTS`]`.scope(..)` around [`CoalitionService::spawn`]; the
/// spawned task inherits them. A task with no faults in scope runs unchanged.
#[cfg(test)]
mod faults {
    use std::future::Future;
    use std::sync::Arc;

    use tokio::sync::Notify;

    /// The faults one service task runs with.
    #[derive(Clone, Default)]
    pub(super) struct Faults {
        /// Pause the panic path of `serve` after `panicked` is stored and
        /// before the reply is sent.
        pub(super) pause_before_reply: Option<Arc<Pause>>,
        /// Panic with `"members exploded"` at the start of every members
        /// command's work.
        pub(super) members_panics: bool,
    }

    /// A two-way rendezvous: the service task notifies `reached`, then waits
    /// for `release`.
    #[derive(Default)]
    pub(super) struct Pause {
        pub(super) reached: Notify,
        pub(super) release: Notify,
    }

    tokio::task_local! {
        pub(super) static FAULTS: Faults;
    }

    /// Run `task` with the faults in scope on the calling task, or none.
    pub(super) fn inherit<F: Future>(task: F) -> impl Future<Output = F::Output> {
        let faults = FAULTS.try_with(Faults::clone).unwrap_or_default();
        FAULTS.scope(faults, task)
    }

    /// The pause between storing `panicked` and sending the reply.
    pub(super) async fn between_flag_and_reply() {
        let pause = FAULTS
            .try_with(|f| f.pause_before_reply.clone())
            .ok()
            .flatten();
        if let Some(pause) = pause {
            pause.reached.notify_one();
            pause.release.notified().await;
        }
    }

    /// The members fault.
    pub(super) fn members() {
        if FAULTS.try_with(|f| f.members_panics).unwrap_or(false) {
            panic!("members exploded");
        }
    }
}

/// Close `rx` and answer every command still in it with
/// [`ServiceError::Cancelled`].
async fn refuse_queued(rx: &mut mpsc::Receiver<CoalitionCommand>) {
    rx.close();
    while let Some(cmd) = rx.recv().await {
        match cmd {
            CoalitionCommand::Join { reply, .. } | CoalitionCommand::Leave { reply, .. } => {
                let _ = reply.send(Err(ServiceError::Cancelled));
            }
            CoalitionCommand::Members { reply, .. } => {
                let _ = reply.send(Err(ServiceError::Cancelled));
            }
        }
    }
}

/// The text of a panic payload: the `&str` or `String` it carries, or a
/// placeholder for any other payload type.
fn panic_message(payload: &(dyn Any + Send)) -> &str {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        s
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.as_str()
    } else {
        "<non-string panic payload>"
    }
}

/// Serve commands from `rx` until `token` is cancelled or every sender is
/// dropped. Each command's work runs under [`serve`]: a panic sets `panicked`
/// before that command's `reply` is answered, then the task ends.
#[allow(clippy::too_many_arguments)]
async fn service_loop<V, HE>(
    mut rx: mpsc::Receiver<CoalitionCommand>,
    manager: CoalitionManager<V, HE>,
    policy: Box<dyn CoalitionDecisionPolicy>,
    ctx: DecisionContext,
    tap: Option<mpsc::Sender<DecisionRecord>>,
    trace_tap: Option<mpsc::Sender<DecisionTrace>>,
    token: CancellationToken,
    panicked: Arc<AtomicBool>,
) where
    V: VertexTrait + Clone + AgentCapabilities + 'static,
    HE: HyperedgeTrait + Clone + 'static,
{
    loop {
        let cmd = tokio::select! {
            biased;
            () = token.cancelled() => {
                refuse_queued(&mut rx).await;
                break;
            }
            cmd = rx.recv() => match cmd {
                Some(cmd) => cmd,
                None => break,
            },
        };
        match cmd {
            CoalitionCommand::Join {
                agent,
                coalition,
                reply,
            } => {
                let work = async {
                    let position = decision_position(trace_tap.as_ref(), &manager).await;
                    let r = manager
                        .try_join_coalition(agent, coalition, policy.as_ref(), &ctx)
                        .await
                        .map_err(ServiceError::from);
                    // Tap every consulted decision (join or decline) before
                    // replying; a manager error (no Decision produced) is not
                    // tapped.
                    if let Ok(decision) = &r {
                        emit_decision(tap.as_ref(), coalition, agent, DecisionKind::Join, decision);
                        emit_trace(
                            trace_tap.as_ref(),
                            position,
                            coalition,
                            agent,
                            DecisionKind::Join,
                            decision,
                        );
                    }
                    r
                };
                if !serve(&panicked, reply, work).await {
                    return;
                }
            }
            CoalitionCommand::Leave {
                agent,
                coalition,
                reply,
            } => {
                let work = async {
                    let position = decision_position(trace_tap.as_ref(), &manager).await;
                    let r = manager
                        .try_leave_coalition(agent, coalition, policy.as_ref(), &ctx)
                        .await
                        .map_err(ServiceError::from);
                    if let Ok(decision) = &r {
                        emit_decision(
                            tap.as_ref(),
                            coalition,
                            agent,
                            DecisionKind::Leave,
                            decision,
                        );
                        emit_trace(
                            trace_tap.as_ref(),
                            position,
                            coalition,
                            agent,
                            DecisionKind::Leave,
                            decision,
                        );
                    }
                    r
                };
                if !serve(&panicked, reply, work).await {
                    return;
                }
            }
            CoalitionCommand::Members { coalition, reply } => {
                let work = async {
                    #[cfg(test)]
                    faults::members();
                    manager
                        .coalition_members(coalition)
                        .await
                        .map_err(ServiceError::from)
                };
                if !serve(&panicked, reply, work).await {
                    return;
                }
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
    use crate::test_support::log_capture::{Capture, CapturedEvent};

    /// Assert `capture` holds exactly one warn-level event whose message
    /// contains `needle`, with the given coalition, agent id and kind fields.
    fn assert_one_warn(capture: &Capture, needle: &str, fields: [(&str, &str); 3]) {
        let events: Vec<CapturedEvent> = capture.matching(needle);
        assert_eq!(events.len(), 1, "{needle}: events {events:?}");
        let e = &events[0];
        assert_eq!(
            e.level,
            tracing::Level::WARN,
            "{needle}: level was {}",
            e.level
        );
        for (field, want) in fields {
            assert_eq!(
                e.fields.get(field).map(String::as_str),
                Some(want),
                "{needle}: field {field}; fields {:?}",
                e.fields
            );
        }
    }

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
        let manager = CoalitionManager::<Worker, ()>::empty(crate::core::executor::test_executor());
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
            &TaskTracker::new(),
            CancellationToken::new(),
        );

        let decision = service.join(candidate, coalition).await.expect("decision");
        assert!(decision.act, "positive marginal ⇒ join");

        // The reply returns only after the tap emit, so the record is buffered.
        let record = tap_rx.recv().await.expect("tap record");
        assert_eq!(record.agent_id, usize::from(candidate));
        assert_eq!(
            record.coalition,
            format!("coalition-{}", usize::from(coalition))
        );
        assert_eq!(record.kind, DecisionKind::Join);
        assert!(record.act);
        assert_eq!(record.score, decision.score);

        let members = service.members(coalition).await.expect("members");
        assert_eq!(members.len(), 2, "candidate actually added");
        assert!(members.contains(&candidate));
    }

    /// The trace tap carries the record plus the event-log length and clock
    /// read before the manager call. `seed()` records 3 events at timestamps
    /// 0..=2 (log length 3, clock 3); the join records event 3 at timestamp 3
    /// (length 4, clock 4) before the declined leave.
    #[tokio::test]
    async fn trace_tap_reads_the_position_before_the_manager_call() {
        let (manager, candidate, coalition) = seed().await;

        let (tap_tx, mut tap_rx) = mpsc::channel(8);
        let service = CoalitionService::spawn_with_trace_tap(
            manager,
            Box::new(ThresholdPolicy::new(AdditiveCalculator, 0.0, 0.0)),
            DecisionContext::default(),
            tap_tx,
            &TaskTracker::new(),
            CancellationToken::new(),
        );

        // Candidate: 50 + 10·1 + 80 = 140 — the join marginal and the leave's
        // marginal of staying.
        let join = service.join(candidate, coalition).await.expect("join");
        let leave = service.leave(candidate, coalition).await.expect("leave");
        assert!(join.act && !leave.act);

        let label = format!("coalition-{}", usize::from(coalition));
        let expected = [
            DecisionTrace {
                record: DecisionRecord {
                    coalition: label.clone(),
                    agent_id: usize::from(candidate),
                    kind: DecisionKind::Join,
                    act: true,
                    score: 140.0,
                },
                event_log_len: 3,
                timestamp: 3,
            },
            DecisionTrace {
                record: DecisionRecord {
                    coalition: label,
                    agent_id: usize::from(candidate),
                    kind: DecisionKind::Leave,
                    act: false,
                    score: 140.0,
                },
                event_log_len: 4,
                timestamp: 4,
            },
        ];
        for want in expected {
            assert_eq!(tap_rx.recv().await.expect("trace"), want);
        }
    }

    /// A closed tap must NOT stall or fail decisions: the record is dropped with
    /// one warn carrying its coalition, agent id and kind, and membership is
    /// mutated exactly as without a tap.
    #[tokio::test]
    async fn closed_tap_does_not_affect_decision() {
        let capture = Capture::default();
        let _guard = capture.install();
        let (manager, candidate, coalition) = seed().await;

        let (tap_tx, tap_rx) = mpsc::channel(1);
        drop(tap_rx); // close the tap before any decision

        let service = CoalitionService::spawn_with_tap(
            manager,
            Box::new(ThresholdPolicy::new(AdditiveCalculator, 0.0, 0.0)),
            DecisionContext::default(),
            tap_tx,
            &TaskTracker::new(),
            CancellationToken::new(),
        );

        let decision = service.join(candidate, coalition).await.expect("decision");
        assert!(decision.act, "decision unaffected by a closed tap");

        let members = service.members(coalition).await.expect("members");
        assert_eq!(members.len(), 2, "join applied despite closed tap");
        assert!(members.contains(&candidate));

        let label = format!("coalition-{}", usize::from(coalition));
        let agent_id = usize::from(candidate).to_string();
        assert_one_warn(
            &capture,
            "decision tap closed",
            [
                ("coalition", label.as_str()),
                ("agent_id", agent_id.as_str()),
                ("kind", "join"),
            ],
        );
    }

    /// Through a service: a full record tap drops the leave that follows a
    /// buffered join, and a closed trace tap drops the join; each drop is one
    /// warn carrying the decision's coalition, agent id and kind.
    #[tokio::test]
    async fn tap_drops_log_at_warn_with_the_record_fields() {
        let capture = Capture::default();
        let _guard = capture.install();

        let (manager, candidate, coalition) = seed().await;
        let (full_tx, _full_rx) = mpsc::channel(1);
        let service = CoalitionService::spawn_with_tap(
            manager,
            Box::new(ThresholdPolicy::new(AdditiveCalculator, 0.0, 0.0)),
            DecisionContext::default(),
            full_tx,
            &TaskTracker::new(),
            CancellationToken::new(),
        );
        service.join(candidate, coalition).await.expect("join");
        service.leave(candidate, coalition).await.expect("leave");
        let label = format!("coalition-{}", usize::from(coalition));
        let agent_id = usize::from(candidate).to_string();
        assert_one_warn(
            &capture,
            "decision tap full",
            [
                ("coalition", label.as_str()),
                ("agent_id", agent_id.as_str()),
                ("kind", "leave"),
            ],
        );

        let (manager, candidate, coalition) = seed().await;
        let (closed_tx, closed_rx) = mpsc::channel(1);
        drop(closed_rx);
        let service = CoalitionService::spawn_with_trace_tap(
            manager,
            Box::new(ThresholdPolicy::new(AdditiveCalculator, 0.0, 0.0)),
            DecisionContext::default(),
            closed_tx,
            &TaskTracker::new(),
            CancellationToken::new(),
        );
        service.join(candidate, coalition).await.expect("join");
        let label = format!("coalition-{}", usize::from(coalition));
        let agent_id = usize::from(candidate).to_string();
        assert_one_warn(
            &capture,
            "decision trace tap closed",
            [
                ("coalition", label.as_str()),
                ("agent_id", agent_id.as_str()),
                ("kind", "join"),
            ],
        );
    }

    /// With a handle still alive, `CoalitionRuntime::shutdown` returns within
    /// 5 s of a service spawned on its tracker with a child of its token, and
    /// the handle then reports the service gone.
    #[tokio::test]
    async fn runtime_shutdown_stops_a_service_whose_handle_lives() {
        let (manager, candidate, coalition) = seed().await;
        let runtime = crate::core::CoalitionRuntime::new(crate::core::executor::test_executor());
        let service = CoalitionService::spawn(
            manager,
            Box::new(ThresholdPolicy::new(AdditiveCalculator, 0.0, 0.0)),
            DecisionContext::default(),
            runtime.task_tracker(),
            runtime.cancellation_token().child_token(),
        );
        service.join(candidate, coalition).await.expect("join");

        let stopped =
            tokio::time::timeout(std::time::Duration::from_secs(5), runtime.shutdown()).await;
        assert!(
            stopped.is_ok(),
            "expected shutdown to return within 5 s with a live handle, observed a timeout"
        );
        let after = service.members(coalition).await;
        assert!(
            after.is_err(),
            "expected the stopped service to refuse commands, observed {after:?}"
        );
        assert!(!service.panicked(), "a cancelled service did not panic");
    }

    /// A policy whose join decision panics and whose drop blocks its thread
    /// for `drop_delay`.
    struct PanickingPolicy {
        drop_delay: std::time::Duration,
    }

    impl Drop for PanickingPolicy {
        fn drop(&mut self) {
            std::thread::sleep(self.drop_delay);
        }
    }

    impl CoalitionDecisionPolicy for PanickingPolicy {
        fn should_join(
            &self,
            _agent: &dyn AgentCapabilities,
            _coalition: &[&dyn AgentCapabilities],
            _ctx: &DecisionContext,
        ) -> Decision {
            panic!("policy exploded");
        }

        fn should_leave(
            &self,
            _agent: &dyn AgentCapabilities,
            _coalition: &[&dyn AgentCapabilities],
            _ctx: &DecisionContext,
        ) -> Decision {
            Decision {
                act: false,
                score: 0.0,
            }
        }
    }

    /// A panic in the service task is one error-level event carrying the panic
    /// message, sets `panicked()`, and still lets the tracker drain.
    #[tokio::test]
    async fn service_panic_is_logged_at_error_and_flagged() {
        let capture = Capture::default();
        let _guard = capture.install();
        let (manager, candidate, coalition) = seed().await;
        let tracker = TaskTracker::new();
        let service = CoalitionService::spawn(
            manager,
            Box::new(PanickingPolicy {
                drop_delay: std::time::Duration::ZERO,
            }),
            DecisionContext::default(),
            &tracker,
            CancellationToken::new(),
        );
        assert!(!service.panicked(), "no panic before the first command");

        let join = service.join(candidate, coalition).await;
        assert!(
            join.is_err(),
            "expected the panicked service to drop the reply, observed {join:?}"
        );
        tracker.close();
        tracker.wait().await;

        assert!(
            service.panicked(),
            "expected panicked() == true after the policy panic"
        );
        let events = capture.matching("CoalitionService panicked");
        assert_eq!(events.len(), 1, "events {events:?}");
        assert_eq!(events[0].level, tracing::Level::ERROR, "level");
        assert_eq!(
            events[0].fields.get("panic").map(String::as_str),
            Some("policy exploded"),
            "panic field; fields {:?}",
            events[0].fields
        );
    }

    /// On a multi-thread runtime, a caller whose join fails on the policy
    /// panic receives `Panicked` with the panic message and reads
    /// `panicked() == true` as soon as that error returns, while the service
    /// task is still blocked for 300 ms in the policy's drop.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn panicked_reads_true_when_a_join_panic_error_returns() {
        let (manager, candidate, coalition) = seed().await;
        let tracker = TaskTracker::new();
        let service = CoalitionService::spawn(
            manager,
            Box::new(PanickingPolicy {
                drop_delay: std::time::Duration::from_millis(300),
            }),
            DecisionContext::default(),
            &tracker,
            CancellationToken::new(),
        );

        let join = service.join(candidate, coalition).await;
        let panicked = service.panicked();
        assert!(
            matches!(&join, Err(ServiceError::Panicked { message }) if message == "policy exploded"),
            "join: expected Panicked {{ message: \"policy exploded\" }}, observed {join:?}"
        );
        assert!(
            panicked,
            "expected panicked() == true when the join's Panicked error returned, observed false"
        );
        tracker.close();
        tracker.wait().await;
    }

    /// A policy that joins and leaves on every opportunity, or panics on
    /// every leave when `leave_panics`.
    struct ActPolicy {
        leave_panics: bool,
    }

    impl CoalitionDecisionPolicy for ActPolicy {
        fn should_join(
            &self,
            _agent: &dyn AgentCapabilities,
            _coalition: &[&dyn AgentCapabilities],
            _ctx: &DecisionContext,
        ) -> Decision {
            Decision {
                act: true,
                score: 1.0,
            }
        }

        fn should_leave(
            &self,
            _agent: &dyn AgentCapabilities,
            _coalition: &[&dyn AgentCapabilities],
            _ctx: &DecisionContext,
        ) -> Decision {
            assert!(!self.leave_panics, "leave exploded");
            Decision {
                act: true,
                score: 1.0,
            }
        }
    }

    /// On a multi-thread runtime, a caller whose leave panics in the policy
    /// receives `Panicked` with the panic message and reads
    /// `panicked() == true` as soon as that error returns.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn panicked_reads_true_when_a_leave_panic_error_returns() {
        let (manager, candidate, coalition) = seed().await;
        let tracker = TaskTracker::new();
        let service = CoalitionService::spawn(
            manager,
            Box::new(ActPolicy { leave_panics: true }),
            DecisionContext::default(),
            &tracker,
            CancellationToken::new(),
        );
        let join = service.join(candidate, coalition).await;
        assert!(
            matches!(&join, Ok(d) if d.act),
            "join: expected act == true, observed {join:?}"
        );

        let leave = service.leave(candidate, coalition).await;
        let panicked = service.panicked();
        assert!(
            matches!(&leave, Err(ServiceError::Panicked { message }) if message == "leave exploded"),
            "leave: expected Panicked {{ message: \"leave exploded\" }}, observed {leave:?}"
        );
        assert!(
            panicked,
            "expected panicked() == true when the leave's Panicked error returned, observed false"
        );
        tracker.close();
        tracker.wait().await;
    }

    /// On the panic path, `serve` stores `panicked` before it sends the
    /// reply: with the service paused between the two steps, `panicked()`
    /// reads `true` and the caller has no reply yet; after the release the
    /// caller receives `Panicked` with the panic message.
    #[tokio::test]
    async fn panic_flag_is_stored_before_the_reply_is_sent() {
        let (manager, candidate, coalition) = seed().await;
        let tracker = TaskTracker::new();
        let pause = Arc::new(faults::Pause::default());
        let service = faults::FAULTS
            .scope(
                faults::Faults {
                    pause_before_reply: Some(Arc::clone(&pause)),
                    members_panics: false,
                },
                async {
                    CoalitionService::spawn(
                        manager,
                        Box::new(PanickingPolicy {
                            drop_delay: std::time::Duration::ZERO,
                        }),
                        DecisionContext::default(),
                        &tracker,
                        CancellationToken::new(),
                    )
                },
            )
            .await;

        let caller = {
            let service = service.clone();
            tokio::spawn(async move { service.join(candidate, coalition).await })
        };
        pause.reached.notified().await;
        let panicked = service.panicked();
        let replied = caller.is_finished();
        assert!(
            panicked,
            "expected panicked() == true between the flag store and the reply, observed false"
        );
        assert!(
            !replied,
            "expected no reply between the flag store and the reply, observed the caller finished"
        );

        pause.release.notify_one();
        let join = caller.await.expect("caller task");
        assert!(
            matches!(&join, Err(ServiceError::Panicked { message }) if message == "policy exploded"),
            "join: expected Panicked {{ message: \"policy exploded\" }}, observed {join:?}"
        );
        tracker.close();
        tracker.wait().await;
    }

    /// A members command whose work panics replies `Panicked` with the panic
    /// message, and its caller reads `panicked() == true` as soon as that
    /// error returns.
    #[tokio::test]
    async fn members_panic_replies_panicked_and_sets_the_flag() {
        let (manager, _candidate, coalition) = seed().await;
        let tracker = TaskTracker::new();
        let service = faults::FAULTS
            .scope(
                faults::Faults {
                    pause_before_reply: None,
                    members_panics: true,
                },
                async {
                    CoalitionService::spawn(
                        manager,
                        Box::new(ActPolicy {
                            leave_panics: false,
                        }),
                        DecisionContext::default(),
                        &tracker,
                        CancellationToken::new(),
                    )
                },
            )
            .await;
        assert!(!service.panicked(), "no panic before the members command");

        let members = service.members(coalition).await;
        let panicked = service.panicked();
        assert!(
            matches!(&members, Err(ServiceError::Panicked { message }) if message == "members exploded"),
            "members: expected Panicked {{ message: \"members exploded\" }}, observed {members:?}"
        );
        assert!(
            panicked,
            "expected panicked() == true when the members' Panicked error returned, observed false"
        );
        tracker.close();
        tracker.wait().await;
    }

    /// `is_terminal` is `true` for the four service-stop variants,
    /// `Topology(LockPoisoned)` and `Topology(TaskCancelled)`, and `false`
    /// for `Topology` of each of the nine other `TemporalError` variants.
    #[test]
    fn is_terminal_marks_the_stop_variants_a_poisoned_lock_and_a_cancelled_task() {
        use crate::topology::{SnapshotId, Timestamp};
        use catgraph_applied::HypergraphError;

        let ts = Timestamp::new;
        let observed: Vec<(String, bool)> = [
            ServiceError::Gone,
            ServiceError::ReplyDropped,
            ServiceError::Cancelled,
            ServiceError::Panicked {
                message: "boom".to_string(),
            },
            ServiceError::Topology(TemporalError::LockPoisoned),
            ServiceError::Topology(TemporalError::TaskCancelled),
            ServiceError::Topology(TemporalError::Hypergraph(HypergraphError::InvalidJoin)),
            ServiceError::Topology(TemporalError::TimestampInFuture {
                requested: ts(2),
                current: ts(1),
            }),
            ServiceError::Topology(TemporalError::NoSnapshotBefore(ts(0))),
            ServiceError::Topology(TemporalError::SnapshotNotFound(SnapshotId::new(0))),
            ServiceError::Topology(TemporalError::ReplayError {
                event_index: 0,
                message: "x".to_string(),
            }),
            ServiceError::Topology(TemporalError::InvalidTimeRange {
                start: ts(1),
                end: ts(0),
            }),
            ServiceError::Topology(TemporalError::EventLogCorrupted("x".to_string())),
            ServiceError::Topology(TemporalError::ConcurrentModification),
            ServiceError::Topology(TemporalError::EmptyCoalition),
        ]
        .into_iter()
        .map(|e| (format!("{e:?}"), e.is_terminal()))
        .collect();
        let terminal: Vec<bool> = observed.iter().map(|(_, t)| *t).collect();
        let mut expected = vec![true; 6];
        expected.extend([false; 9]);
        assert_eq!(
            terminal, expected,
            "is_terminal per variant; observed {observed:?}"
        );
    }

    /// A failed join, leave and members command each return
    /// `ServiceError::Topology` carrying the manager's `TemporalError`
    /// variant: a removed agent's join is `VertexNotFound` for that agent,
    /// the final member's leave is `EmptyCoalition`, and an unknown
    /// coalition's members is `HyperedgeNotFound` for that coalition.
    #[tokio::test]
    async fn topology_errors_reach_the_caller_as_temporal_variants() {
        use catgraph_applied::HypergraphError;

        let (manager, _candidate, coalition) = seed().await;
        let removed = manager
            .add_agent(Worker {
                id: 3,
                caps: 0b100,
                trust: 10,
            })
            .await
            .expect("add agent");
        manager.remove_agent(removed).await.expect("remove agent");
        let only_member = manager.coalition_members(coalition).await.expect("members")[0];
        let unknown = HyperedgeIndex::from(usize::from(coalition) + 1);
        let service = CoalitionService::spawn(
            manager,
            Box::new(ActPolicy {
                leave_panics: false,
            }),
            DecisionContext::default(),
            &TaskTracker::new(),
            CancellationToken::new(),
        );

        let join = service.join(removed, coalition).await;
        assert!(
            matches!(
                &join,
                Err(ServiceError::Topology(TemporalError::Hypergraph(
                    HypergraphError::VertexNotFound(v)
                ))) if *v == removed
            ),
            "join of a removed agent: expected Topology(Hypergraph(VertexNotFound({removed:?}))), observed {join:?}"
        );
        let leave = service.leave(only_member, coalition).await;
        assert!(
            matches!(
                &leave,
                Err(ServiceError::Topology(TemporalError::EmptyCoalition))
            ),
            "leave of the final member: expected Topology(EmptyCoalition), observed {leave:?}"
        );
        let members = service.members(unknown).await;
        assert!(
            matches!(
                &members,
                Err(ServiceError::Topology(TemporalError::Hypergraph(
                    HypergraphError::HyperedgeNotFound(h)
                ))) if *h == unknown
            ),
            "members of an unknown coalition: expected Topology(Hypergraph(HyperedgeNotFound({unknown:?}))), observed {members:?}"
        );
    }

    /// A policy whose async join signals `started`, then waits for `release`
    /// and joins.
    struct GatedPolicy {
        started: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    }

    impl CoalitionDecisionPolicy for GatedPolicy {
        fn should_join(
            &self,
            _agent: &dyn AgentCapabilities,
            _coalition: &[&dyn AgentCapabilities],
            _ctx: &DecisionContext,
        ) -> Decision {
            Decision {
                act: true,
                score: 1.0,
            }
        }

        fn should_leave(
            &self,
            _agent: &dyn AgentCapabilities,
            _coalition: &[&dyn AgentCapabilities],
            _ctx: &DecisionContext,
        ) -> Decision {
            Decision {
                act: false,
                score: 0.0,
            }
        }

        fn should_join_async<'a>(
            &'a self,
            agent: &'a dyn AgentCapabilities,
            coalition: &'a [&'a dyn AgentCapabilities],
            ctx: &'a DecisionContext,
            _executor: &'a crate::core::Executor,
        ) -> std::pin::Pin<Box<dyn Future<Output = Decision> + Send + 'a>> {
            let decision = self.should_join(agent, coalition, ctx);
            Box::pin(async move {
                self.started.notify_one();
                self.release.notified().await;
                decision
            })
        }
    }

    /// With a join in flight and a members command queued, cancelling the
    /// token lets the join complete, answers the queued command with the
    /// cancelled error, and refuses a later command as gone.
    #[tokio::test]
    async fn cancel_answers_queued_commands_with_the_cancelled_error() {
        let (manager, candidate, coalition) = seed().await;
        let started = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let tracker = TaskTracker::new();
        let token = CancellationToken::new();
        let service = CoalitionService::spawn(
            manager,
            Box::new(GatedPolicy {
                started: Arc::clone(&started),
                release: Arc::clone(&release),
            }),
            DecisionContext::default(),
            &tracker,
            token.clone(),
        );

        let in_flight = tokio::spawn({
            let service = service.clone();
            async move { service.join(candidate, coalition).await }
        });
        started.notified().await;
        let queued = service.members(coalition);
        tokio::pin!(queued);
        assert!(
            futures::poll!(&mut queued).is_pending(),
            "expected the members command to wait behind the in-flight join"
        );

        token.cancel();
        release.notify_one();

        let joined = in_flight.await.expect("the join task completes");
        assert!(
            matches!(&joined, Ok(d) if d.act),
            "expected the in-flight join to complete with act == true, observed {joined:?}"
        );
        let queued = queued.await;
        assert!(
            matches!(queued, Err(ServiceError::Cancelled)),
            "queued members: expected Cancelled, observed {queued:?}"
        );
        let after = service.members(coalition).await;
        assert!(
            matches!(after, Err(ServiceError::Gone)),
            "members after the stop: expected Gone, observed {after:?}"
        );
        assert!(!service.panicked(), "a cancelled service did not panic");
        tracker.close();
        tracker.wait().await;
    }

    // -----------------------------------------------------------------------
    // spawn_decision_tee
    // -----------------------------------------------------------------------

    fn record(agent_id: usize) -> DecisionRecord {
        record_of_kind(agent_id, DecisionKind::Join)
    }

    fn record_of_kind(agent_id: usize, kind: DecisionKind) -> DecisionRecord {
        DecisionRecord {
            coalition: "coalition-0".to_string(),
            agent_id,
            kind,
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
    /// the other sink; a CLOSED sink is tolerated for the tee's lifetime. Each
    /// drop is one warn carrying the sink and the record's coalition, agent id
    /// and kind.
    #[tokio::test]
    async fn tee_tolerates_full_and_closed_sinks() {
        let capture = Capture::default();
        let _guard = capture.install();
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

        let kinds = [DecisionKind::Join, DecisionKind::Leave, DecisionKind::Join];
        for (id, kind) in (1..=3).zip(kinds) {
            tap_tx.send(record_of_kind(id, kind)).await.expect("send");
        }
        drop(tap_tx);
        handle.await.expect("tee joins despite full + closed sinks");

        // The healthy sink saw all three — neither the full nor the closed sink
        // cost it anything.
        for id in 1..=3 {
            assert_eq!(ok_rx.recv().await.expect("healthy record").agent_id, id);
        }

        // The full sink took record 1, so records 2 (a leave) and 3 drop
        // there; the closed sink drops all three.
        for (needle, sink, dropped) in [
            (
                "decision tee sink full",
                "0",
                [("2", "leave"), ("3", "join")].as_slice(),
            ),
            (
                "decision tee sink closed",
                "1",
                [("1", "join"), ("2", "leave"), ("3", "join")].as_slice(),
            ),
        ] {
            let events = capture.matching(needle);
            let seen: Vec<(Option<&str>, Option<&str>)> = events
                .iter()
                .map(|e| {
                    (
                        e.fields.get("agent_id").map(String::as_str),
                        e.fields.get("kind").map(String::as_str),
                    )
                })
                .collect();
            let want: Vec<(Option<&str>, Option<&str>)> = dropped
                .iter()
                .map(|&(id, kind)| (Some(id), Some(kind)))
                .collect();
            assert_eq!(seen, want, "{needle}: (agent id, kind); events {events:?}");
            for e in &events {
                assert_eq!(e.level, tracing::Level::WARN, "{needle}: level {}", e.level);
                for (field, want) in [("sink", sink), ("coalition", "coalition-0")] {
                    assert_eq!(
                        e.fields.get(field).map(String::as_str),
                        Some(want),
                        "{needle}: field {field}; fields {:?}",
                        e.fields
                    );
                }
            }
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
