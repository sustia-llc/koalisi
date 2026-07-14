//! Durable coalition decision messaging survives restart — through koalisi's
//! seam. (feature `durable`)
//!
//! Modeled on the upstream `scenario_durable_restart` proof, but driven through
//! koalisi's `CoalitionService` decision tap + `spawn_decision_forwarder`:
//!
//! 1. Boot SurrealDB (upstream container harness). Skip-with-diagnostic if
//!    Docker is unavailable (print + pass, so CI without Docker stays green).
//! 2. Round 1: stand up the durable bus (`recorder` + `decision_log`) so
//!    `decision_log` snapshots a "from-now" cursor against the empty log, then
//!    shut it down — `decision_log` is now OFFLINE, cursor persisted.
//! 3. Drive TWO policy-gated joins through the service while `decision_log` is
//!    down; the tap → forwarder publishes both as durable edges with no live
//!    subscriber. Drain the tap (drop the service, await the forwarder) so both
//!    are durably written.
//! 4. Round 2: recreate the bus. `decision_log` resumes from its persisted
//!    cursor and the `SHOW CHANGES` catch-up replays BOTH events — the property
//!    plain at-most-once `LIVE` lacks.
//!
//! The hot path never touches the DB; only the *decisions* are forwarded.

#![cfg(feature = "durable")]

use std::collections::HashMap;

use tokio::time::{Duration, sleep, timeout};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use koalisi::algorithms::{AdditiveCalculator, AgentCapabilities};
use koalisi::core::config::setup_logging;
use koalisi::decision::{DecisionContext, ThresholdPolicy};
use koalisi::subsystems::coalition_actor::CoalitionService;
use koalisi::subsystems::durable::{DurableDecisionBus, spawn_decision_forwarder};
use koalisi::topology::CoalitionManager;

use surrealdb_live_message::subsystems::sdb::{self, SurrealDBWrapper};

const RECORDER: &str = "recorder";
const DECISION_LOG: &str = "decision_log";
const BOOT_TIMEOUT: Duration = Duration::from_secs(180);
const RECV_TIMEOUT: Duration = Duration::from_secs(10);

/// Domain agent — a valid graph vertex weight and an `AgentCapabilities`.
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

/// Boot the DB under a child token; return `true` once ready. Returns `false`
/// (skip signal) if the sdb task exits early (e.g. Docker unavailable → the
/// container create `expect` panics) or startup times out.
async fn boot_db(tracker: &TaskTracker, token: &CancellationToken) -> bool {
    let sdb_token = token.child_token();
    let mut handle = tracker.spawn(async move {
        if let Err(e) = sdb::sdb_task(sdb_token).await {
            tracing::error!("sdb_task failed: {e}");
        }
    });
    tokio::select! {
        r = SurrealDBWrapper::wait_until_ready() => r.is_ok(),
        _ = &mut handle => false,
        _ = sleep(BOOT_TIMEOUT) => false,
    }
}

#[tokio::test]
async fn durable_decisions_survive_restart_through_the_seam() {
    setup_logging();

    let token = CancellationToken::new();
    let tracker = TaskTracker::new();

    if !boot_db(&tracker, &token).await {
        eprintln!(
            "SKIP durable_decisions_survive_restart_through_the_seam: \
             SurrealDB/Docker unavailable"
        );
        token.cancel();
        tracker.close();
        tracker.wait().await;
        return;
    }

    // --- Round 1: pin decision_log's "from-now" cursor against an empty log,
    //     then take it offline. recorder's Agent handle stays usable for sends
    //     (send only needs the global connection). ---
    let bus1 = DurableDecisionBus::new(vec![RECORDER.to_string(), DECISION_LOG.to_string()])
        .await
        .expect("round-1 durable bus");
    let recorder = bus1.agent(RECORDER).await.expect("recorder agent");
    bus1.shutdown().await; // decision_log offline; cursor persisted at start

    // --- koalisi service with a decision tap + forwarder (recorder → decision_log). ---
    let manager = CoalitionManager::<Worker, ()>::empty();
    let seed = manager
        .add_agent(Worker { id: 2, caps: 0b010, trust: 90 })
        .await
        .expect("add seed");
    let coalition = manager.form_coalition(vec![seed], ()).await.expect("form");
    let c1 = manager
        .add_agent(Worker { id: 1, caps: 0b001, trust: 80 })
        .await
        .expect("add c1");
    let c2 = manager
        .add_agent(Worker { id: 3, caps: 0b100, trust: 70 })
        .await
        .expect("add c2");

    let (tap_tx, tap_rx) = tokio::sync::mpsc::channel(64);
    let service = CoalitionService::spawn_with_tap(
        manager,
        Box::new(ThresholdPolicy::new(AdditiveCalculator, 0.0, 0.0)),
        DecisionContext::default(),
        tap_tx,
    );
    let forwarder = spawn_decision_forwarder(
        recorder,
        DECISION_LOG.to_string(),
        tap_rx,
        &tracker,
        token.child_token(),
    );

    // --- Drive two joins while decision_log is DOWN. Each `join().await`
    //     returns only after the tap emit, so both records are queued. ---
    let d1 = service.join(c1, coalition).await.expect("join c1");
    let d2 = service.join(c2, coalition).await.expect("join c2");
    assert!(d1.act && d2.act, "positive marginal ⇒ both join");

    // Drop the service so its tap Sender closes; the forwarder drains the two
    // buffered records, publishes them durably, then exits on the closed rx.
    // Awaiting the forwarder guarantees both durable edges are written before
    // the restart.
    drop(service);
    forwarder.await.expect("forwarder join");

    // --- Round 2: restart. decision_log resumes from its cursor and the
    //     catch-up replays BOTH events. ---
    let bus2 = DurableDecisionBus::new(vec![RECORDER.to_string(), DECISION_LOG.to_string()])
        .await
        .expect("round-2 durable bus (restart)");
    let inbox = bus2.coalition().inbox();

    let mut seen: HashMap<u64, (String, bool)> = HashMap::new();
    for _ in 0..2 {
        let d = timeout(RECV_TIMEOUT, inbox.recv())
            .await
            .expect("durable replay timed out — a decision sent while down was lost")
            .expect("inbox bus closed unexpectedly");
        assert_eq!(d.recipient, DECISION_LOG, "events are addressed to the log sink");
        let e = &d.message.payload;
        seen.insert(e.agent_id, (e.kind.clone(), e.act));
    }

    // Both decisions replayed, keyed by the topology vertex index (usize→u64).
    let c1_id = usize::from(c1) as u64;
    let c2_id = usize::from(c2) as u64;
    assert_eq!(
        seen.get(&c1_id),
        Some(&("join".to_string(), true)),
        "c1's join decision must be durably replayed"
    );
    assert_eq!(
        seen.get(&c2_id),
        Some(&("join".to_string(), true)),
        "c2's join decision must be durably replayed"
    );

    bus2.shutdown().await;
    token.cancel();
    tracker.close();
    tracker.wait().await;
}
