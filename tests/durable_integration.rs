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
//! 5. A log sink that is not an agent of the bus is refused with
//!    `DurableError::UnknownLogSink`, and a producer that is not an agent of
//!    the bus, or a bus agent renamed to another bus agent's name, with
//!    `DurableError::UnknownProducer`; none spawns a task.
//! 6. On a separate bus, two forwarded sends fail: a join whose `RELATE`
//!    fails a field assertion, then a leave whose sink `agent` record is
//!    deleted after the sink check. Each failure is logged once at error
//!    level with the decision's fields and the error's source chain.
//!
//! The hot path never touches the DB; only the *decisions* are forwarded.

mod common;

use std::collections::HashMap;

use tokio::sync::mpsc;
use tokio::time::{Duration, sleep, timeout};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::Level;

use common::log_capture::Capture;
use koalisi::algorithms::{AdditiveCalculator, AgentCapabilities};
use koalisi::core::config::setup_logging;
use koalisi::decision::{DecisionContext, ThresholdPolicy};
use koalisi::subsystems::coalition_actor::{CoalitionService, DecisionKind, DecisionRecord};
use koalisi::subsystems::durable::{DurableDecisionBus, DurableError, spawn_decision_forwarder};
use koalisi::topology::CoalitionManager;

use surrealdb_live_message::subsystems::agents::Agent;
use surrealdb_live_message::subsystems::sdb::{self, SurrealDBWrapper};

const RECORDER: &str = "recorder";
const DECISION_LOG: &str = "decision_log";
const OUTSIDER: &str = "outsider";
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
    let manager = CoalitionManager::<Worker, ()>::empty(common::executor());
    let seed = manager
        .add_agent(Worker {
            id: 2,
            caps: 0b010,
            trust: 90,
        })
        .await
        .expect("add seed");
    let coalition = manager.form_coalition(vec![seed], ()).await.expect("form");
    let c1 = manager
        .add_agent(Worker {
            id: 1,
            caps: 0b001,
            trust: 80,
        })
        .await
        .expect("add c1");
    let c2 = manager
        .add_agent(Worker {
            id: 3,
            caps: 0b100,
            trust: 70,
        })
        .await
        .expect("add c2");

    let (tap_tx, tap_rx) = tokio::sync::mpsc::channel(64);
    let service = CoalitionService::spawn_with_tap(
        manager,
        Box::new(ThresholdPolicy::new(AdditiveCalculator, 0.0, 0.0)),
        DecisionContext::default(),
        tap_tx,
        &tracker,
    );
    let forwarder = spawn_decision_forwarder(
        &bus1,
        recorder,
        DECISION_LOG.to_string(),
        tap_rx,
        &tracker,
        token.child_token(),
    )
    .await
    .expect("decision_log is an agent of the bus");

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
        assert_eq!(
            d.recipient, DECISION_LOG,
            "events are addressed to the log sink"
        );
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

    // --- A producer or a log sink that is not an agent of the bus is refused,
    //     and nothing is spawned. ---
    let recorder2 = bus2.agent(RECORDER).await.expect("recorder agent");
    let outsider = Agent::new(OUTSIDER).await.expect("outsider agent");
    // The bus's decision_log agent renamed to recorder: a bus agent has the
    // name, but its record id is agent:decision_log, not agent:recorder.
    let mut impostor = bus2.agent(DECISION_LOG).await.expect("decision_log agent");
    impostor.name = RECORDER.to_string();
    for (producer, log_sink, want) in [
        (
            recorder2,
            "no_such_sink",
            DurableError::UnknownLogSink {
                log_sink: "no_such_sink".to_string(),
            },
        ),
        (
            outsider,
            DECISION_LOG,
            DurableError::UnknownProducer {
                producer: OUTSIDER.to_string(),
            },
        ),
        (
            impostor,
            DECISION_LOG,
            DurableError::UnknownProducer {
                producer: RECORDER.to_string(),
            },
        ),
    ] {
        let refuse_tracker = TaskTracker::new();
        let (_unused_tx, unused_rx) = mpsc::channel(1);
        let refused = spawn_decision_forwarder(
            &bus2,
            producer,
            log_sink.to_string(),
            unused_rx,
            &refuse_tracker,
            token.child_token(),
        )
        .await;
        let spawned = refuse_tracker.len();
        assert_eq!(
            refused.as_ref().err(),
            Some(&want),
            "{want}: must be refused; got Ok: {}",
            refused.is_ok()
        );
        assert_eq!(spawned, 0, "{want}: a refused spawn left {spawned} task(s)");
        refuse_tracker.close();
    }

    bus2.shutdown().await;

    failed_send_logs_at_error(&tracker, &token).await;

    token.cancel();
    tracker.close();
    tracker.wait().await;
}

const FAIL_PRODUCER: &str = "fail_producer";
const FAIL_SINK: &str = "fail_sink";
const FAILED_SEND_MESSAGE: &str =
    "durable decision forward failed; the decision may be missing from the log";
/// Makes every `RELATE` into `message` fail its field assertion.
const FAIL_CREATED: &str = "DEFINE FIELD OVERWRITE created ON message TYPE datetime ASSERT false;";
/// The `created` field as upstream `define_schema` defines it.
const RESTORE_CREATED: &str = "DEFINE FIELD OVERWRITE created ON message TYPE datetime;";

/// Run one SurrealQL statement on the upstream connection.
async fn run_query(query: &str) {
    SurrealDBWrapper::connection()
        .await
        .query(query)
        .await
        .and_then(|r| r.check())
        .unwrap_or_else(|e| panic!("query {query:?} failed: {e}"));
}

/// Wait until `capture` holds `n` events with message `FAILED_SEND_MESSAGE`.
async fn wait_for_failed_sends(capture: &Capture, n: usize) {
    timeout(RECV_TIMEOUT, async {
        while capture.matching(FAILED_SEND_MESSAGE).len() < n {
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "timed out waiting for {n} failed-send log(s); saw {:?}",
            capture.matching(FAILED_SEND_MESSAGE)
        )
    });
}

/// The forwarder logs each failed `Agent::send` once, at error level, with the
/// decision's fields and the error's source chain. The first send (a join)
/// fails in the `RELATE`, because the `created` field of `message` is
/// redefined with `ASSERT false` (upstream `Agent::send` returns `Error::Send`,
/// whose source is the database error). The second send (a leave) fails
/// because the sink's `agent` record is deleted after the sink check passed
/// (upstream `Agent::send` returns `UnknownRecipient`, which has no source).
async fn failed_send_logs_at_error(tracker: &TaskTracker, token: &CancellationToken) {
    let bus = DurableDecisionBus::new(vec![FAIL_PRODUCER.to_string(), FAIL_SINK.to_string()])
        .await
        .expect("failure bus");
    let producer = bus.agent(FAIL_PRODUCER).await.expect("producer agent");
    let (tx, rx) = mpsc::channel(4);
    let forwarder = spawn_decision_forwarder(
        &bus,
        producer,
        FAIL_SINK.to_string(),
        rx,
        tracker,
        token.child_token(),
    )
    .await
    .expect("fail_sink is an agent of the bus");

    let capture = Capture::default();
    let guard = capture.install();

    run_query(FAIL_CREATED).await;
    tx.send(DecisionRecord {
        coalition: "coalition-7".to_string(),
        agent_id: 10,
        kind: DecisionKind::Join,
        act: true,
        score: 1.0,
    })
    .await
    .expect("queue the join");
    wait_for_failed_sends(&capture, 1).await;
    run_query(RESTORE_CREATED).await;

    run_query(&format!("DELETE agent:{FAIL_SINK}")).await;
    tx.send(DecisionRecord {
        coalition: "coalition-7".to_string(),
        agent_id: 11,
        kind: DecisionKind::Leave,
        act: false,
        score: 0.0,
    })
    .await
    .expect("queue the leave");
    drop(tx);
    forwarder.await.expect("forwarder join");
    drop(guard);

    let events = capture.matching("durable decision forward");
    let messages: Vec<Option<&str>> = events
        .iter()
        .map(|e| e.fields.get("message").map(String::as_str))
        .collect();
    assert_eq!(
        messages,
        vec![Some(FAILED_SEND_MESSAGE); 2],
        "two failed-send logs; events {events:?}"
    );

    let sourced_prefix = format!("failed to send message to '{FAIL_SINK}': ");
    for (e, agent_id, kind) in [(&events[0], "10", "join"), (&events[1], "11", "leave")] {
        assert_eq!(e.level, Level::ERROR, "{kind}: level was {}", e.level);
        for (field, want) in [
            ("coalition", "coalition-7"),
            ("agent_id", agent_id),
            ("kind", kind),
            ("to", FAIL_SINK),
        ] {
            assert_eq!(
                e.fields.get(field).map(String::as_str),
                Some(want),
                "{kind}: field {field}; fields {:?}",
                e.fields
            );
        }
    }

    // The join's error has a source: `err` is the top-level message, ": ",
    // then the database error.
    let join_err = events[0].fields.get("err").map_or("", String::as_str);
    let source = join_err.strip_prefix(&sourced_prefix);
    assert!(
        source.is_some_and(|s| s.starts_with("Found ")
            && s.contains("for field `created`")
            && s.ends_with("but field must conform to: false")),
        "join: err must be {sourced_prefix:?} followed by the assertion error; err {join_err:?}"
    );
    assert_eq!(
        events[1].fields.get("err").map(String::as_str),
        Some("recipient agent 'fail_sink' does not exist"),
        "leave: err; fields {:?}",
        events[1].fields
    );

    bus.shutdown().await;
}
