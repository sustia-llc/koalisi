//! The pre-registered #18 / P7.2 parity gate (features `persistence` +
//! `magnitude`, sustia-llc/koalisi#30).
//!
//! A coalition is driven through joins, a leave, and a member weight update
//! with an event tap installed; the topology stream is flushed to a
//! `FileEventStore` and replayed back into a fresh `EventLog`. Running
//! `TemporalAnalytics::magnitude_history` over the live log and the replayed log
//! must produce **exactly** equal `Vec<MagnitudePoint>` — same events in, same
//! fresh `t = 1` evaluations out, so `f64` bitwise equality is expected. A
//! mismatch is a real parity break, not a floating-point tolerance issue.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, mpsc};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use koalisi::algorithms::AgentCapabilities;
use koalisi::persistence::{
    EventStore, FileEventStore, FileStoreConfig, Record, SequenceNo, StreamId,
    replay_into_event_log, spawn_store_writer, spawn_topology_forwarder,
};
use koalisi::topology::{
    EventLog, TemporalAnalytics, TemporalEvent, TemporalHypergraph, TimeRange,
};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut path = std::env::temp_dir();
        path.push(format!("koalisi-parity-{}-{tag}-{n}", std::process::id()));
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

/// The pre-registered parity gate: `magnitude_history` over the replayed log
/// equals the same query over the live in-memory log, point for point and bit
/// for bit.
#[tokio::test]
async fn magnitude_history_parity_live_vs_replayed() {
    let tmp = TempDir::new("mag-parity");
    let store: Arc<dyn EventStore> =
        Arc::new(FileEventStore::open(tmp.path(), FileStoreConfig::default()).unwrap());

    let tracker = TaskTracker::new();
    let token = CancellationToken::new();

    let (writer_tx, writer_rx) = mpsc::channel::<(StreamId, Record)>(1024);
    spawn_store_writer(Arc::clone(&store), writer_rx, &tracker, token.child_token());

    let (tap_tx, tap_rx) = mpsc::channel::<TemporalEvent<Worker, Coalition>>(1024);
    spawn_topology_forwarder::<Worker, Coalition, Worker, Coalition>(
        tap_rx,
        writer_tx,
        &tracker,
        token.child_token(),
    );

    let graph = TemporalHypergraph::<Worker, Coalition>::new().with_event_tap(tap_tx);

    // Seeded scenario: joins raise magnitude, a leave lowers it, a member weight
    // update shifts the relevant-mask multiset.
    let v0 = graph
        .add_vertex(Worker { id: 0, caps: 0b001 })
        .await
        .unwrap();
    let v1 = graph
        .add_vertex(Worker { id: 1, caps: 0b010 })
        .await
        .unwrap();
    let v2 = graph
        .add_vertex(Worker { id: 2, caps: 0b100 })
        .await
        .unwrap();
    let he = graph
        .add_hyperedge(vec![v0, v1], Coalition { value: 1 })
        .await
        .unwrap();
    graph
        .update_hyperedge_vertices(he, vec![v0, v1, v2])
        .await
        .unwrap(); // join v2
    graph
        .update_vertex_weight(v1, Worker { id: 1, caps: 0b001 })
        .await
        .unwrap(); // member weight update
    graph
        .update_hyperedge_vertices(he, vec![v0, v2])
        .await
        .unwrap(); // leave v1

    let live_ref = graph.events_ref().clone();

    // Flush the pipeline (same head-to-tail channel-drop discipline as
    // tests/topology_replay.rs).
    drop(graph);
    tracker.close();
    tracker.wait().await;

    let replay_store = Arc::clone(&store);
    let replayed: EventLog<Worker, Coalition> = tokio::task::spawn_blocking(move || {
        replay_into_event_log::<Worker, Coalition, Worker, Coalition>(
            replay_store.as_ref(),
            SequenceNo(0),
        )
    })
    .await
    .unwrap()
    .unwrap();
    let replayed_ref = Arc::new(RwLock::new(replayed));

    let live_points =
        TemporalAnalytics::magnitude_history(&live_ref, he, 0b111, &TimeRange::all()).await;
    let replay_points =
        TemporalAnalytics::magnitude_history(&replayed_ref, he, 0b111, &TimeRange::all()).await;

    assert!(!live_points.is_empty(), "scenario must yield points");
    assert_eq!(
        live_points, replay_points,
        "magnitude trajectory must be bit-identical over the replayed log"
    );
}
