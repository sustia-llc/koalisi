//! The `Decisions` stream (Phase 7 / P7.4, sustia-llc/koalisi#32; feature
//! `persistence`): `CoalitionService::spawn_with_trace_tap` →
//! `spawn_decision_store_forwarder` → `spawn_store_writer`, next to the
//! `Topology` stream fed by the graph's event tap.
//!
//! Fixture arithmetic (`ThresholdPolicy(AdditiveCalculator, 0, 0)`, trust 50):
//! an agent with `p` capability bits contributes `50 + 10·p + 50` to a
//! coalition's additive value, so every join has a positive marginal
//! (`act == true`) and every leave a positive marginal of staying
//! (`act == false`). A leave of a non-member returns `act == false`,
//! `score == 0.0` without consulting the policy.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use koalisi::algorithms::{AdditiveCalculator, AgentCapabilities};
use koalisi::decision::{DecisionContext, ThresholdPolicy};
use koalisi::persistence::{
    EventRef, EventStore, FileEventStore, FileStoreConfig, Payload, Record, SequenceNo,
    StoredRecord, StreamId, WIRE_DECISION_SCHEMA_VERSION, WireDecision, WireTopologyEvent,
    spawn_decision_store_forwarder, spawn_store_writer, spawn_topology_forwarder,
};
use koalisi::subsystems::coalition_actor::{
    CoalitionService, DecisionKind, DecisionRecord, DecisionTrace,
};
use koalisi::topology::{CoalitionManager, TemporalEvent, TemporalHypergraph, VertexIndex};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut path = std::env::temp_dir();
        path.push(format!(
            "koalisi-decision-stream-{}-{tag}-{n}",
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

/// A vertex weight that is a valid graph weight, an `AgentCapabilities` view,
/// and its own serde identity projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Worker {
    id: usize,
    caps: u32,
}

impl AgentCapabilities for Worker {
    fn agent_id(&self) -> usize {
        self.id
    }
    fn capabilities(&self) -> u32 {
        self.caps
    }
    fn trust_level(&self) -> u32 {
        50
    }
}

/// A hyperedge weight fixture (serde identity projection).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Coalition {
    value: u32,
}

/// One hand-derived `Decisions` record: the decision, its trace timestamp and
/// its `Topology` parent seq.
struct Expected {
    kind: DecisionKind,
    agent_id: usize,
    act: bool,
    score: f64,
    timestamp: u64,
    parent: u64,
}

impl Expected {
    fn new(
        kind: DecisionKind,
        agent: VertexIndex,
        act: bool,
        score: f64,
        timestamp: u64,
        parent: u64,
    ) -> Self {
        Self {
            kind,
            agent_id: usize::from(agent),
            act,
            score,
            timestamp,
            parent,
        }
    }
}

fn policy() -> Box<ThresholdPolicy<AdditiveCalculator>> {
    Box::new(ThresholdPolicy::new(AdditiveCalculator, 0.0, 0.0))
}

fn plain_bytes(stored: &StoredRecord) -> &[u8] {
    match &stored.record.payload {
        Payload::Plain(bytes) => bytes,
        Payload::Sealed { .. } => panic!("seq {}: expected a Plain payload", stored.seq.0),
    }
}

fn decode_decision(stored: &StoredRecord) -> DecisionRecord {
    let wire: WireDecision = ciborium::from_reader(plain_bytes(stored)).expect("CBOR decode");
    wire.try_into_record().expect("known decision kind")
}

fn decode_topology(stored: &StoredRecord) -> WireTopologyEvent<Worker, Coalition> {
    ciborium::from_reader(plain_bytes(stored)).expect("CBOR decode")
}

/// Records in `stream` from seq 0 through the head, read on a blocking thread.
async fn read_all(store: &Arc<dyn EventStore>, stream: StreamId) -> Vec<StoredRecord> {
    let store = Arc::clone(store);
    tokio::task::spawn_blocking(move || store.read_from(stream, SequenceNo(0), usize::MAX))
        .await
        .expect("read task")
        .expect("read_from")
}

/// (i) Full pipeline: every `Decisions` record decodes to the expected
/// decision, carries the hand-derived parent, and the parent resolves; each
/// `act == true` join's own membership event is `Topology` record parent + 1.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn decision_records_carry_the_decision_time_topology_parent() {
    let tmp = TempDir::new("pipeline");
    let store: Arc<dyn EventStore> =
        Arc::new(FileEventStore::open(tmp.path(), FileStoreConfig::default()).unwrap());
    let tracker = TaskTracker::new();
    let token = CancellationToken::new();

    let (writer_tx, writer_rx) = mpsc::channel::<(StreamId, Record)>(64);
    spawn_store_writer(Arc::clone(&store), writer_rx, &tracker, token.child_token());

    let (topo_tx, topo_rx) = mpsc::channel::<TemporalEvent<Worker, Coalition>>(64);
    spawn_topology_forwarder::<Worker, Coalition, Worker, Coalition>(
        topo_rx,
        writer_tx.clone(),
        &tracker,
        token.child_token(),
    );
    let (trace_tx, trace_rx) = mpsc::channel::<DecisionTrace>(64);
    spawn_decision_store_forwarder(trace_rx, writer_tx, &tracker, token.child_token());

    // Tap installed on an empty log: event-log index i ↔ Topology seq i.
    let manager = CoalitionManager::new(
        TemporalHypergraph::<Worker, Coalition>::new().with_event_tap(topo_tx),
    );
    // Setup: events 0..=4 at timestamps 0..=4 ⇒ log length 5, clock 5.
    let s = manager
        .add_agent(Worker { id: 0, caps: 0b001 })
        .await
        .unwrap();
    let j1 = manager
        .add_agent(Worker { id: 1, caps: 0b010 })
        .await
        .unwrap();
    let j2 = manager
        .add_agent(Worker { id: 2, caps: 0b100 })
        .await
        .unwrap();
    let j3 = manager
        .add_agent(Worker { id: 3, caps: 0b011 })
        .await
        .unwrap();
    let c = manager
        .form_coalition(vec![s], Coalition { value: 1 })
        .await
        .unwrap();

    let service = CoalitionService::spawn_with_trace_tap(
        manager,
        policy(),
        DecisionContext::default(),
        trace_tx,
    );

    // Hand-derived; `pos` is (event-log length, clock) before the manager call:
    //   d0 join j1   pos (5, 5): +110 ⇒ join, own event 5 at ts 5
    //   d1 leave j1  pos (6, 6): staying +110 ⇒ stay
    //   d2 leave j2  pos (6, 6): non-member ⇒ act false, score 0
    //   d3 join j2   pos (6, 6): +110 ⇒ join, own event 6 at ts 6
    //   d4 leave s   pos (7, 7): staying +110 ⇒ stay
    //   d5 join j3   pos (7, 7): +120 (2 caps) ⇒ join, own event 7 at ts 7
    let label = format!("coalition-{}", usize::from(c));
    let expected = [
        Expected::new(DecisionKind::Join, j1, true, 110.0, 5, 4),
        Expected::new(DecisionKind::Leave, j1, false, 110.0, 6, 5),
        Expected::new(DecisionKind::Leave, j2, false, 0.0, 6, 5),
        Expected::new(DecisionKind::Join, j2, true, 110.0, 6, 5),
        Expected::new(DecisionKind::Leave, s, false, 110.0, 7, 6),
        Expected::new(DecisionKind::Join, j3, true, 120.0, 7, 6),
    ];

    let replies = [
        service.join(j1, c).await.unwrap(),
        service.leave(j1, c).await.unwrap(),
        service.leave(j2, c).await.unwrap(),
        service.join(j2, c).await.unwrap(),
        service.leave(s, c).await.unwrap(),
        service.join(j3, c).await.unwrap(),
    ];
    for (i, (reply, exp)) in replies.iter().zip(&expected).enumerate() {
        assert_eq!(
            (reply.act, reply.score),
            (exp.act, exp.score),
            "d{i}: service reply"
        );
    }

    // Lossless drain: the service exits on the dropped handle, dropping the
    // graph (topology tap) and the trace tap; both forwarders drain, then the
    // writer.
    drop(service);
    tracker.close();
    tracker.wait().await;

    let topology = read_all(&store, StreamId::Topology).await;
    let decisions = read_all(&store, StreamId::Decisions).await;
    assert_eq!(topology.len(), 8, "setup 5 + three act=true joins");
    assert_eq!(decisions.len(), expected.len(), "one record per decision");

    for (i, (stored, exp)) in decisions.iter().zip(&expected).enumerate() {
        let parent = exp.parent;
        assert_eq!(stored.seq, SequenceNo(i as u64), "d{i}: seq");
        assert_eq!(
            decode_decision(stored),
            DecisionRecord {
                coalition: label.clone(),
                agent_id: exp.agent_id,
                kind: exp.kind,
                act: exp.act,
                score: exp.score,
            },
            "d{i}: payload"
        );
        assert_eq!(stored.record.timestamp, exp.timestamp, "d{i}: timestamp");
        assert_eq!(
            stored.record.schema_version, WIRE_DECISION_SCHEMA_VERSION,
            "d{i}: schema version"
        );
        assert_eq!(
            stored.record.parents,
            vec![EventRef {
                stream: StreamId::Topology,
                seq: SequenceNo(parent),
            }],
            "d{i}: parent (expected Topology seq {parent})"
        );

        let resolve_store = Arc::clone(&store);
        let resolved = tokio::task::spawn_blocking(move || {
            resolve_store.read_from(StreamId::Topology, SequenceNo(parent), 1)
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(resolved.len(), 1, "d{i}: parent resolves");
        assert_eq!(resolved[0].seq, SequenceNo(parent), "d{i}: resolved seq");
    }

    // Each act=true join's own membership event is parent + 1.
    let own_events = [
        (0usize, vec![s], vec![s, j1]),
        (3, vec![s, j1], vec![s, j1, j2]),
        (5, vec![s, j1, j2], vec![s, j1, j2, j3]),
    ];
    for (i, old, new) in own_events {
        let own = usize::try_from(expected[i].parent + 1).unwrap();
        assert_eq!(
            decode_topology(&topology[own]),
            WireTopologyEvent::HyperedgeVerticesUpdated {
                timestamp: expected[i].timestamp,
                index: usize::from(c) as u64,
                old_vertices: old.iter().map(|&v| usize::from(v) as u64).collect(),
                new_vertices: new.iter().map(|&v| usize::from(v) as u64).collect(),
            },
            "d{i}: Topology seq {own} is the join's own event"
        );
    }

    let verify_store = Arc::clone(&store);
    tokio::task::spawn_blocking(move || {
        verify_store.verify(StreamId::Topology, SequenceNo(0), SequenceNo(u64::MAX))?;
        verify_store.verify(StreamId::Decisions, SequenceNo(0), SequenceNo(u64::MAX))
    })
    .await
    .unwrap()
    .expect("both chains verify");
}

/// (ii) The forwarder → writer hop is lossless: with a capacity-1 writer
/// channel whose writer starts only after every decision is taken, the
/// `Decisions` stream still holds one record per delivered trace.
#[tokio::test]
async fn full_writer_channel_drops_no_decision() {
    const N: usize = 32;

    let tmp = TempDir::new("capacity-1");
    let store: Arc<dyn EventStore> =
        Arc::new(FileEventStore::open(tmp.path(), FileStoreConfig::default()).unwrap());
    let tracker = TaskTracker::new();
    let token = CancellationToken::new();

    let (writer_tx, writer_rx) = mpsc::channel::<(StreamId, Record)>(1);
    // Capacity N: the trace tap cannot drop any of the N traces.
    let (trace_tx, trace_rx) = mpsc::channel::<DecisionTrace>(N);
    let mut forwarder =
        spawn_decision_store_forwarder(trace_rx, writer_tx, &tracker, token.child_token());

    let manager = CoalitionManager::<Worker, Coalition>::empty();
    let s = manager
        .add_agent(Worker { id: 0, caps: 0b001 })
        .await
        .unwrap();
    let j1 = manager
        .add_agent(Worker { id: 1, caps: 0b010 })
        .await
        .unwrap();
    let c = manager
        .form_coalition(vec![s], Coalition { value: 1 })
        .await
        .unwrap();
    let service = CoalitionService::spawn_with_trace_tap(
        manager,
        policy(),
        DecisionContext::default(),
        trace_tx,
    );
    for _ in 0..N {
        assert!(service.join(j1, c).await.unwrap().act);
    }
    drop(service);

    // Nothing reads the writer channel yet. A forwarder that drops on a full
    // channel finishes here with N − 1 records lost; a lossless one is still
    // waiting for capacity when the timeout fires.
    let _ = tokio::time::timeout(Duration::from_millis(200), &mut forwarder).await;
    spawn_store_writer(Arc::clone(&store), writer_rx, &tracker, token.child_token());
    tracker.close();
    tracker.wait().await;

    let decisions = read_all(&store, StreamId::Decisions).await;
    assert_eq!(
        decisions.len(),
        N,
        "{N} traces delivered, {} Decisions records",
        decisions.len()
    );
}

/// (iii) `WireDecision` CBOR round-trip for both kinds; an unknown kind label
/// is a conversion error.
#[test]
fn wire_decision_round_trips_both_kinds_and_rejects_unknown() {
    let records = [
        DecisionRecord {
            coalition: "coalition-7".to_owned(),
            agent_id: 3,
            kind: DecisionKind::Join,
            act: true,
            score: 1.5,
        },
        DecisionRecord {
            coalition: "coalition-7".to_owned(),
            agent_id: 4,
            kind: DecisionKind::Leave,
            act: false,
            score: -0.25,
        },
    ];
    for (record, label) in records.iter().zip(["join", "leave"]) {
        let wire = WireDecision::from_record(record);
        assert_eq!(wire.kind, label, "kind label");
        let mut buf = Vec::new();
        ciborium::into_writer(&wire, &mut buf).unwrap();
        let back: WireDecision = ciborium::from_reader(buf.as_slice()).unwrap();
        assert_eq!(back, wire, "CBOR round-trip");
        assert_eq!(
            &back.try_into_record().unwrap(),
            record,
            "record round-trip"
        );
    }

    let unknown = WireDecision {
        coalition: "coalition-7".to_owned(),
        agent_id: 3,
        kind: "stay".to_owned(),
        act: true,
        score: 1.5,
    };
    let err = unknown
        .try_into_record()
        .expect_err("unknown kind must not convert");
    assert!(
        err.message.contains("\"stay\""),
        "error names the label: {}",
        err.message
    );
}

/// (iv) The record shape: `event_log_len == 0` ⇒ no parent; otherwise one
/// `Topology` parent at `event_log_len − 1`; timestamp and schema version as
/// given; payload is the CBOR `WireDecision`.
#[tokio::test]
async fn record_parent_is_absent_on_an_empty_log() {
    let tracker = TaskTracker::new();
    let (writer_tx, mut writer_rx) = mpsc::channel::<(StreamId, Record)>(4);
    let (trace_tx, trace_rx) = mpsc::channel::<DecisionTrace>(4);
    spawn_decision_store_forwarder(trace_rx, writer_tx, &tracker, CancellationToken::new());

    let record = DecisionRecord {
        coalition: "coalition-0".to_owned(),
        agent_id: 1,
        kind: DecisionKind::Join,
        act: true,
        score: 2.0,
    };
    let traces = [
        DecisionTrace {
            record: record.clone(),
            event_log_len: 0,
            timestamp: 0,
        },
        DecisionTrace {
            record: record.clone(),
            event_log_len: 3,
            timestamp: 9,
        },
    ];
    for trace in &traces {
        trace_tx.send(trace.clone()).await.unwrap();
    }
    drop(trace_tx);
    tracker.close();
    tracker.wait().await;

    let expected_parents = [
        vec![],
        vec![EventRef {
            stream: StreamId::Topology,
            seq: SequenceNo(2),
        }],
    ];
    for (i, (trace, parents)) in traces.iter().zip(expected_parents).enumerate() {
        let (stream, rec) = writer_rx.recv().await.expect("one record per trace");
        assert_eq!(stream, StreamId::Decisions, "t{i}: stream");
        assert_eq!(rec.parents, parents, "t{i}: parents");
        assert_eq!(rec.timestamp, trace.timestamp, "t{i}: timestamp");
        assert_eq!(rec.schema_version, WIRE_DECISION_SCHEMA_VERSION);
        let Payload::Plain(bytes) = &rec.payload else {
            panic!("t{i}: expected a Plain payload");
        };
        let wire: WireDecision = ciborium::from_reader(bytes.as_slice()).unwrap();
        assert_eq!(
            wire,
            WireDecision::from_record(&trace.record),
            "t{i}: payload"
        );
    }
    assert!(writer_rx.recv().await.is_none(), "no extra record");
}
