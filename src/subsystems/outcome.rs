//! Arm-agnostic task-completion event seam (issue #55).
//!
//! One [`TaskOutcome`] record is emitted per completed task/episode by the
//! **domain embedding koalisi** — koalisi's domain-agnostic core never
//! synthesizes outcomes, exactly as issue #41's
//! [`FeedbackStore::record_outcome`](crate::algorithms::FeedbackStore::record_outcome)
//! is caller-supplied today. This module only provides the typed record, a
//! fan-out consumer surface ([`OutcomeSink`]), and a small non-durable
//! forwarder; the producing side is the domain's responsibility.
//!
//! ## What fidelity this seam carries
//!
//! Per `docs/per-bit-outcome-plumbing-design.md` §3–§4, the runtime floor is
//! the **L2** signal: one whole-task `success` bool over the final member set
//! and the required mask. Issue #54 Step 2 measured that this whole-task
//! success signal, smeared across the required bits, reproduces ≈ the oracle
//! per-bit signal for the E1-persistent AIF arm (0.4381 vs 0.4406 median), so
//! whole-task success is sufficient — no per-member performance telemetry, no
//! per-bit sub-task scoring is required at the runtime level.
//!
//! ## This is not a topology event
//!
//! A [`TaskOutcome`] is **not** a
//! [`TemporalEvent`](crate::topology::TemporalEvent): the topology log
//! is lifecycle-only and its 13-variant replay wire schema (P7.2,
//! `WIRE_TOPOLOGY_SCHEMA_VERSION`) stays frozen. Outcome records are
//! decision-adjacent; their durable home is the P7.4 Decisions/Beliefs streams
//! (issue #32) when that lands. Until then this tap-style, non-durable channel
//! (mirroring the [`DecisionRecord`](crate::subsystems::coalition_actor::DecisionRecord)
//! pattern) suffices for a live consumer.
//!
//! ## The seam FEEDS learned consumers — it does not gate decisions
//!
//! This signal is for *learned* consumers (a [`FeedbackStore`], a decision
//! arm's outcome hook). It must not be turned into a decision veto: the Part 4g
//! result measured that raw ratio-gating on this signal is strictly worse than
//! no gating (absorbing exclusion — an unlucky good agent is permanently
//! excluded). Feed it to consumers that average over many episodes; never gate
//! a join/leave on it directly.
//!
//! ## No `CoalitionDecisionPolicy` trait change
//!
//! A decision arm attaches its outcome hook through the closure
//! [`OutcomeSink`] impl (see below) — a side-channel, exactly like the
//! #46/#48 `Arm { policy, store }` factory — so wiring an arm's
//! `observe_outcome` into the forwarder needs no change to the policy trait.
//!
//! [`FeedbackStore`]: crate::algorithms::FeedbackStore

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

/// One completed-task/episode outcome, emitted by the domain embedding koalisi.
///
/// koalisi never synthesizes these — the producing side is caller-supplied, the
/// #41 `record_outcome` contract now given a type.
///
/// - `required` is the task's required-capability mask (the
///   [`DecisionContext`](crate::decision::DecisionContext) `required_capabilities`
///   the coalition was formed against).
/// - `members` are the final member ids at task end, keyed like
///   [`FeedbackStore`](crate::algorithms::FeedbackStore) ids /
///   [`AgentCapabilities::agent_id`](crate::algorithms::AgentCapabilities::agent_id).
///   Duplicates count **per occurrence** downstream (see the
///   [`FeedbackStore::record_outcome`](crate::algorithms::FeedbackStore::record_outcome)
///   contract, gotcha 19): dedup before emitting if you want
///   at-most-once-per-agent semantics.
/// - `success` is whole-task success (the L2 signal — sufficient per #54
///   Step 2; see the [module docs](self)).
#[derive(Debug, Clone, PartialEq)]
pub struct TaskOutcome {
    /// The task's required-capability mask.
    pub required: u32,
    /// Final member ids at task end (keyed like `FeedbackStore` / `agent_id`).
    pub members: Vec<usize>,
    /// Whole-task success.
    pub success: bool,
}

/// A fan-out consumer of [`TaskOutcome`] records.
///
/// Infallible by contract (`on_outcome` returns `()`): a sink absorbs whatever
/// it can and must not signal failure back up the forwarder — the outcome path
/// can never be stalled or failed by a slow/absent consumer.
pub trait OutcomeSink: Send + Sync {
    /// Consume one outcome. Called once per record by the forwarder, in the
    /// order sinks were supplied.
    fn on_outcome(&self, outcome: &TaskOutcome);
}

/// [`FeedbackStore`](crate::algorithms::FeedbackStore) as an outcome sink: the
/// #41 consumer, scalarized.
///
/// The whole-task `success` bool is scalarized to `1.0` (success) or `0.0`
/// (failure) and handed to
/// [`record_outcome`](crate::algorithms::FeedbackStore::record_outcome). The
/// store's own `failure_threshold` then decides what counts as a failure: with
/// [`FeedbackStore::new(0.5)`](crate::algorithms::FeedbackStore::new) a value of
/// `0.0` is strictly below `0.5` (a failure) and `1.0` is not, so failure ⟺
/// `!success`. Every member's history is bumped either way (see
/// `record_outcome`'s per-occurrence contract).
impl OutcomeSink for crate::algorithms::FeedbackStore {
    fn on_outcome(&self, outcome: &TaskOutcome) {
        self.record_outcome(&outcome.members, if outcome.success { 1.0 } else { 0.0 });
    }
}

/// Any `Fn(&TaskOutcome)` is an outcome sink.
///
/// This is how a decision arm's outcome hook (e.g. a persistent AIF arm's
/// `observe_outcome`) attaches as a side-channel — capture the arm handle in a
/// closure — without any change to the
/// [`CoalitionDecisionPolicy`](crate::decision::CoalitionDecisionPolicy) trait.
impl<F> OutcomeSink for F
where
    F: Fn(&TaskOutcome) + Send + Sync,
{
    fn on_outcome(&self, outcome: &TaskOutcome) {
        self(outcome);
    }
}

/// Emit a [`TaskOutcome`] on the tap without ever stalling or failing the
/// caller.
///
/// Uses non-blocking [`try_send`](mpsc::Sender::try_send): a full or closed tap
/// drops the record with a `warn`, so a slow/absent outcome consumer can never
/// apply backpressure to the task/decision path. Mirrors
/// [`emit_decision`](crate::subsystems::coalition_actor) — the tap is
/// best-effort and the producing path is unaffected by tap health.
pub fn emit_outcome(tap: Option<&mpsc::Sender<TaskOutcome>>, outcome: TaskOutcome) {
    let Some(tx) = tap else { return };
    match tx.try_send(outcome) {
        Ok(()) => {}
        Err(mpsc::error::TrySendError::Full(_)) => {
            tracing::warn!("outcome tap full — dropping task outcome (task unaffected)");
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            tracing::warn!("outcome tap closed — dropping task outcome (task unaffected)");
        }
    }
}

/// Spawn a task that drains the outcome tap and fans each record out to every
/// sink.
///
/// Each [`TaskOutcome`] pulled from `rx` is handed to every sink in `sinks`, in
/// order. A sink is infallible by trait contract ([`OutcomeSink::on_outcome`]
/// returns `()`), so there is no error path.
///
/// The task ends when either `token` cancels or `rx` closes:
///
/// - **Lossless shutdown**: drop all tap `Sender`s → `rx.recv()` yields `None`
///   after buffered records drain → the forwarder exits. Every buffered outcome
///   reaches the sinks first.
/// - **Prompt teardown**: cancelling `token` breaks the loop immediately; an
///   in-flight/buffered record may be dropped (still at-most-once). Pick one;
///   don't mix.
///
/// Spawned on `tracker` so the caller's cancel→close→drain shutdown covers it.
/// Mirrors the `durable` feature's `spawn_decision_forwarder` conventions.
pub fn spawn_outcome_forwarder(
    mut rx: mpsc::Receiver<TaskOutcome>,
    sinks: Vec<Box<dyn OutcomeSink>>,
    tracker: &TaskTracker,
    token: CancellationToken,
) -> JoinHandle<()> {
    tracker.spawn(async move {
        loop {
            tokio::select! {
                biased;
                _ = token.cancelled() => break,
                maybe = rx.recv() => match maybe {
                    Some(outcome) => {
                        for sink in &sinks {
                            sink.on_outcome(&outcome);
                        }
                    }
                    None => break, // all taps dropped; nothing more to forward
                },
            }
        }
        tracing::debug!("outcome forwarder stopped");
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::algorithms::FeedbackStore;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn outcome(members: Vec<usize>, success: bool) -> TaskOutcome {
        TaskOutcome {
            required: 0b111,
            members,
            success,
        }
    }

    /// The `FeedbackStore` sink scalarizes success→1.0 (history only) and
    /// failure→0.0 (history + failure), counting duplicate ids per occurrence.
    #[test]
    fn feedback_store_sink_scalarizes() {
        let store = FeedbackStore::new(0.5);

        // Success: bumps history, not failures.
        store.on_outcome(&outcome(vec![1], true));
        assert_eq!(store.history(1), 1);
        assert_eq!(store.failures(1), 0);

        // Failure: bumps both history and failures.
        store.on_outcome(&outcome(vec![1], false));
        assert_eq!(store.history(1), 2);
        assert_eq!(store.failures(1), 1);

        // Duplicate id counts per occurrence.
        store.on_outcome(&outcome(vec![2, 2], false));
        assert_eq!(store.history(2), 2);
        assert_eq!(store.failures(2), 2);
    }

    /// The forwarder fans out to two sinks in order and drains every buffered
    /// record after the tap sender is dropped (the lossless path).
    #[tokio::test]
    async fn forwarder_fans_out_and_drains() {
        let store = FeedbackStore::new(0.5);
        let count = Arc::new(AtomicUsize::new(0));

        let store_sink: Box<dyn OutcomeSink> = Box::new(store.clone());
        let counter = Arc::clone(&count);
        let closure_sink: Box<dyn OutcomeSink> =
            Box::new(move |_o: &TaskOutcome| {
                counter.fetch_add(1, Ordering::SeqCst);
            });

        let tracker = TaskTracker::new();
        let token = CancellationToken::new();
        let (tx, rx) = mpsc::channel(16);
        let handle =
            spawn_outcome_forwarder(rx, vec![store_sink, closure_sink], &tracker, token);

        emit_outcome(Some(&tx), outcome(vec![1], true));
        emit_outcome(Some(&tx), outcome(vec![1, 2], false));

        // Drop the only sender ⇒ recv() yields None after draining both records.
        drop(tx);
        handle.await.expect("forwarder joins");

        // Both records reached both sinks, in order.
        assert_eq!(count.load(Ordering::SeqCst), 2);
        assert_eq!(store.history(1), 2);
        assert_eq!(store.failures(1), 1); // only the second record was a failure
        assert_eq!(store.history(2), 1);
        assert_eq!(store.failures(2), 1);

        tracker.close();
        tracker.wait().await;
    }

    /// Cancelling the token exits the forwarder promptly even with a live
    /// sender still held (no hang).
    #[tokio::test]
    async fn forwarder_cancel_exits_promptly() {
        let tracker = TaskTracker::new();
        let token = CancellationToken::new();
        let (tx, rx) = mpsc::channel::<TaskOutcome>(16);
        let handle = spawn_outcome_forwarder(rx, vec![], &tracker, token.clone());

        // Sender is still alive; only the cancel can end the task.
        token.cancel();
        handle.await.expect("forwarder joins on cancel");

        drop(tx);
        tracker.close();
        tracker.wait().await;
    }

    /// `emit_outcome` never blocks or panics on a full or closed channel.
    #[tokio::test]
    async fn emit_outcome_is_drop_safe() {
        // Full: a 1-cap channel with an unread record already buffered.
        let (tx, _rx) = mpsc::channel::<TaskOutcome>(1);
        emit_outcome(Some(&tx), outcome(vec![1], true)); // fills the buffer
        emit_outcome(Some(&tx), outcome(vec![2], true)); // full ⇒ dropped, no panic

        // Closed: receiver gone.
        let (tx2, rx2) = mpsc::channel::<TaskOutcome>(1);
        drop(rx2);
        emit_outcome(Some(&tx2), outcome(vec![3], true)); // closed ⇒ dropped, no panic

        // None tap is a no-op.
        emit_outcome(None, outcome(vec![4], true));
    }
}
