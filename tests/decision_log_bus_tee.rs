//! `spawn_decision_log_bus_tee` (features `persistence` + `durable`): every
//! trace from `CoalitionService::spawn_with_trace_tap` reaches both the
//! `Decisions` stream of a `FileEventStore` and the durable bus inbox.
//!
//! Container-backed like `tests/durable_integration.rs`: boots SurrealDB via
//! upstream's `sdb::sdb_task`, and prints a `SKIP` line and passes if Docker is
//! unavailable.
//!
//! Fixture arithmetic (`ThresholdPolicy(AdditiveCalculator, 0, 0)`): an agent
//! with trust `t` and `p` capability bits contributes `50 + 10·p + t`, so the
//! join is taken, the member's leave is declined, and the non-member's leave
//! returns `act == false`, `score == 0.0` without consulting the policy.

#![cfg(all(feature = "persistence", feature = "durable"))]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::mpsc;
use tokio::time::{Duration, sleep, timeout};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use koalisi::algorithms::{AdditiveCalculator, AgentCapabilities};
use koalisi::core::config::setup_logging;
use koalisi::decision::{DecisionContext, ThresholdPolicy};
use koalisi::persistence::{
    EventRef, EventStore, FileEventStore, FileStoreConfig, Payload, Record, SequenceNo, StreamId,
    WireDecision, spawn_store_writer,
};
use koalisi::subsystems::coalition_actor::{
    CoalitionService, DecisionKind, DecisionRecord, DecisionTrace,
};
use koalisi::subsystems::durable::{DecisionEvent, DurableDecisionBus, spawn_decision_log_bus_tee};
use koalisi::topology::CoalitionManager;

use surrealdb_live_message::subsystems::sdb::{self, SurrealDBWrapper};

const PRODUCER: &str = "tee_producer";
const LOG_SINK: &str = "tee_log";
const BOOT_TIMEOUT: Duration = Duration::from_secs(180);
const RECV_TIMEOUT: Duration = Duration::from_secs(10);

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> Self {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "koalisi-decision-log-bus-tee-{}",
            std::process::id()
        ));
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

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

/// Boot the DB under a child token; return `true` once ready, `false` (skip
/// signal) if the sdb task exits early or startup times out.
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
#[allow(clippy::too_many_lines)]
async fn every_trace_reaches_the_decisions_stream_and_the_bus() {
    setup_logging();

    let token = CancellationToken::new();
    let db_tracker = TaskTracker::new();

    if !boot_db(&db_tracker, &token).await {
        eprintln!(
            "SKIP every_trace_reaches_the_decisions_stream_and_the_bus: \
             SurrealDB/Docker unavailable"
        );
        token.cancel();
        db_tracker.close();
        db_tracker.wait().await;
        return;
    }

    let bus = DurableDecisionBus::new(vec![PRODUCER.to_string(), LOG_SINK.to_string()])
        .await
        .expect("durable bus");
    let producer = bus.agent(PRODUCER).await.expect("producer agent");
    let inbox = bus.coalition().inbox();

    let tmp = TempDir::new();
    let store: Arc<dyn EventStore> =
        Arc::new(FileEventStore::open(tmp.path(), FileStoreConfig::default()).unwrap());
    let pipe_tracker = TaskTracker::new();
    let (writer_tx, writer_rx) = mpsc::channel::<(StreamId, Record)>(16);
    spawn_store_writer(
        Arc::clone(&store),
        writer_rx,
        &pipe_tracker,
        token.child_token(),
    );
    let (trace_tx, trace_rx) = mpsc::channel::<DecisionTrace>(16);
    spawn_decision_log_bus_tee(
        trace_rx,
        writer_tx,
        producer,
        LOG_SINK.to_string(),
        &pipe_tracker,
        token.child_token(),
    );

    // Setup: 4 events (3 agents + the coalition) ⇒ log length 4, clock 4.
    let manager = CoalitionManager::<Worker, ()>::empty();
    let s = manager
        .add_agent(Worker {
            id: 0,
            caps: 0b001,
            trust: 50,
        })
        .await
        .unwrap();
    let j1 = manager
        .add_agent(Worker {
            id: 1,
            caps: 0b010,
            trust: 50,
        })
        .await
        .unwrap();
    let j2 = manager
        .add_agent(Worker {
            id: 2,
            caps: 0b100,
            trust: 50,
        })
        .await
        .unwrap();
    let c = manager.form_coalition(vec![s], ()).await.unwrap();

    let service = CoalitionService::spawn_with_trace_tap(
        manager,
        Box::new(ThresholdPolicy::new(AdditiveCalculator, 0.0, 0.0)),
        DecisionContext::default(),
        trace_tx,
    );

    // Hand-derived (record, Topology parent seq):
    //   join j1   pos (4, 4): +110 ⇒ join (event 4)  → parent 3
    //   leave j1  pos (5, 5): staying +110 ⇒ stay    → parent 4
    //   leave j2  pos (5, 5): non-member ⇒ act false, score 0 → parent 4
    let label = format!("coalition-{}", usize::from(c));
    let record = |agent, kind, act, score| DecisionRecord {
        coalition: label.clone(),
        agent_id: usize::from(agent),
        kind,
        act,
        score,
    };
    let expected = [
        (record(j1, DecisionKind::Join, true, 110.0), 3),
        (record(j1, DecisionKind::Leave, false, 110.0), 4),
        (record(j2, DecisionKind::Leave, false, 0.0), 4),
    ];

    service.join(j1, c).await.unwrap();
    service.leave(j1, c).await.unwrap();
    service.leave(j2, c).await.unwrap();

    // Lossless drain: the service exits on the dropped handle, dropping the
    // trace tap; the tee drains (log, then bus, per trace), then the writer.
    drop(service);
    pipe_tracker.close();
    pipe_tracker.wait().await;

    // --- the Decisions stream ---
    let read_store = Arc::clone(&store);
    let decisions = tokio::task::spawn_blocking(move || {
        read_store.read_from(StreamId::Decisions, SequenceNo(0), usize::MAX)
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(decisions.len(), expected.len(), "one record per trace");
    for (i, (stored, (want, parent))) in decisions.iter().zip(&expected).enumerate() {
        let Payload::Plain(bytes) = &stored.record.payload else {
            panic!("d{i}: expected a Plain payload");
        };
        let wire: WireDecision = ciborium::from_reader(bytes.as_slice()).unwrap();
        assert_eq!(&wire.try_into_record().unwrap(), want, "d{i}: payload");
        assert_eq!(
            stored.record.parents,
            vec![EventRef {
                stream: StreamId::Topology,
                seq: SequenceNo(*parent),
            }],
            "d{i}: parent"
        );
    }

    // --- the bus inbox ---
    let mut seen: Vec<DecisionEvent> = Vec::new();
    for i in 0..expected.len() {
        let delivery = timeout(RECV_TIMEOUT, inbox.recv())
            .await
            .unwrap_or_else(|_| panic!("bus delivery {i} of {} timed out", expected.len()))
            .expect("inbox bus closed unexpectedly");
        assert_eq!(delivery.recipient, LOG_SINK, "addressed to the log sink");
        seen.push(delivery.message.payload);
    }
    let mut want: Vec<DecisionEvent> = expected
        .iter()
        .map(|(r, _)| DecisionEvent::from(r.clone()))
        .collect();
    want.sort_by_key(|e| (e.agent_id, e.kind.clone()));
    seen.sort_by_key(|e| (e.agent_id, e.kind.clone()));
    assert_eq!(seen, want, "every trace published to the bus");

    bus.shutdown().await;
    token.cancel();
    db_tracker.close();
    db_tracker.wait().await;
}
