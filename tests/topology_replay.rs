//! P7.2 acceptance: topology events tapped, projected, stored, and replayed
//! back into an in-memory `EventLog` (feature `persistence`,
//! sustia-llc/koalisi#30).
//!
//! ## Flush discipline
//!
//! The pipeline is `graph tap → forwarder → writer_tx → store writer`. To land
//! every event before replay we close it head-to-tail via channel drops rather
//! than cancellation:
//!
//! 1. drop the graph — the only tap sender — so `tap_rx` closes;
//! 2. the forwarder drains its backlog, breaks, and drops the `writer_tx` it
//!    owns, so `writer_rx` closes;
//! 3. the store writer drains and breaks;
//! 4. `tracker.close()` + `tracker.wait().await` joins both tasks, so all
//!    `append`s have completed before we replay.
//!
//! Neither the forwarder nor the store writer holds any other sender, so this
//! ordering is deterministic (no token cancel needed).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, mpsc};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use koalisi::persistence::{
    EventStore, FileEventStore, FileStoreConfig, Payload, PersistenceError, Record, SequenceNo,
    StreamId, replay_into_event_log, spawn_store_writer, spawn_topology_forwarder,
};
use koalisi::topology::{
    EventLog, HyperedgeIndex, TemporalEvent, TemporalHypergraph, TemporalQueries, Timestamp,
    VertexIndex,
};

/// A self-cleaning unique temp directory (no `tempfile` dev-dep in this repo).
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut path = std::env::temp_dir();
        path.push(format!("koalisi-replay-{}-{tag}-{n}", std::process::id()));
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

/// Vertex weight: a graph weight (`Copy + Eq + Debug + Send + Sync`) that is
/// also its own serde identity projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct W(u32);

/// Hyperedge weight, likewise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct C(u32);

/// The outcome of driving the shared 13-variant scenario: the live log (both as
/// a captured event vec and as the shared handle) and the replayed log, plus the
/// key indices the point-in-time test samples.
struct ScenarioOut {
    live_events: Vec<TemporalEvent<W, C>>,
    live_ref: Arc<RwLock<EventLog<W, C>>>,
    replayed: EventLog<W, C>,
    v0: VertexIndex,
    he0: HyperedgeIndex,
}

/// Drive a `TemporalHypergraph` through every one of the 13 `TemporalEvent`
/// variants with an event tap installed before the first mutation, flush the
/// pipeline into a `FileEventStore`, and replay the `Topology` stream back.
async fn run_scenario(tmp: &TempDir) -> ScenarioOut {
    let store: Arc<dyn EventStore> =
        Arc::new(FileEventStore::open(tmp.path(), FileStoreConfig::default()).unwrap());

    let tracker = TaskTracker::new();
    let token = CancellationToken::new();

    let (writer_tx, writer_rx) = mpsc::channel::<(StreamId, Record)>(1024);
    spawn_store_writer(Arc::clone(&store), writer_rx, &tracker, token.child_token());

    let (tap_tx, tap_rx) = mpsc::channel::<TemporalEvent<W, C>>(1024);
    spawn_topology_forwarder::<W, C, W, C>(tap_rx, writer_tx, &tracker, token.child_token());

    let graph = TemporalHypergraph::<W, C>::new().with_event_tap(tap_tx);

    // Drive every mutation kind. Indices are deterministic: vertices 0..=4,
    // hyperedges 0..=1.
    let v0 = graph.add_vertex(W(0b0001)).await.unwrap();
    let v1 = graph.add_vertex(W(0b0010)).await.unwrap();
    let v2 = graph.add_vertex(W(0b0100)).await.unwrap();
    let v3 = graph.add_vertex(W(0b1000)).await.unwrap();
    let v4 = graph.add_vertex(W(0b1_0000)).await.unwrap(); // never joins an edge
    let he0 = graph.add_hyperedge(vec![v0, v1], C(10)).await.unwrap();
    graph.update_vertex_weight(v0, W(0b0011)).await.unwrap();
    graph.update_hyperedge_weight(he0, C(11)).await.unwrap();
    graph
        .update_hyperedge_vertices(he0, vec![v0, v1, v2])
        .await
        .unwrap();
    graph.reverse_hyperedge(he0).await.unwrap(); // [v2, v1, v0]
    graph
        .contract_hyperedge_vertices(he0, vec![v1], v0)
        .await
        .unwrap(); // [v2, v0]
    let he1 = graph.add_hyperedge(vec![v3], C(20)).await.unwrap();
    graph.join_hyperedges(vec![he0, he1]).await.unwrap(); // he0 absorbs he1
    graph.remove_vertex(v4).await.unwrap();
    graph.create_snapshot().await;
    graph.remove_hyperedge(he0).await.unwrap();
    graph.clear_hyperedges().await.unwrap();
    graph.clear().await.unwrap();

    // Capture the live log before tearing down the tap.
    let live_events = graph.events().await;
    let live_ref = graph.events_ref().clone();

    // Flush: drop the tap, then join the pipeline tasks (see module docs).
    drop(graph);
    tracker.close();
    tracker.wait().await;

    let replay_store = Arc::clone(&store);
    let replayed = tokio::task::spawn_blocking(move || {
        replay_into_event_log::<W, C, W, C>(replay_store.as_ref(), SequenceNo(0))
    })
    .await
    .unwrap()
    .unwrap();

    ScenarioOut {
        live_events,
        live_ref,
        replayed,
        v0,
        he0,
    }
}

/// Every variant survives the tap → projection → store → replay round-trip:
/// the replayed log matches the live log event-for-event (type + timestamp),
/// and all 13 variants are present.
#[tokio::test]
async fn all_thirteen_variants_roundtrip_through_store() {
    let tmp = TempDir::new("all-variants");
    let out = run_scenario(&tmp).await;

    let replayed = out.replayed.events();
    assert_eq!(
        replayed.len(),
        out.live_events.len(),
        "replayed event count must equal live"
    );

    for (i, (live, back)) in out.live_events.iter().zip(replayed).enumerate() {
        assert_eq!(
            live.event_type(),
            back.event_type(),
            "event {i} type mismatch"
        );
        assert_eq!(
            live.timestamp(),
            back.timestamp(),
            "event {i} timestamp mismatch"
        );
    }

    // All 13 variants actually occurred.
    let mut kinds: Vec<&'static str> = replayed.iter().map(|e| e.event_type()).collect();
    kinds.sort_unstable();
    kinds.dedup();
    assert_eq!(kinds.len(), 13, "expected all 13 variants, got {kinds:?}");
}

/// Point-in-time reconstruction agrees between the live log and the replayed log
/// at several timestamps.
#[tokio::test]
async fn point_in_time_reconstruction_matches_on_replayed_log() {
    let tmp = TempDir::new("point-in-time");
    let out = run_scenario(&tmp).await;

    let live_ref = out.live_ref;
    let replayed_ref = Arc::new(RwLock::new(out.replayed));
    let v0 = out.v0;
    let he0 = out.he0;

    for raw in [0u64, 1, 4, 5, 7, 9, 11, 14, 16] {
        let ts = Timestamp::new(raw);

        let live_vcount = TemporalQueries::count_vertices_at(&live_ref, ts).await;
        let replay_vcount = TemporalQueries::count_vertices_at(&replayed_ref, ts).await;
        assert_eq!(live_vcount, replay_vcount, "vertex count at {ts}");

        let live_members = TemporalQueries::hyperedge_vertices_at(&live_ref, he0, ts).await;
        let replay_members = TemporalQueries::hyperedge_vertices_at(&replayed_ref, he0, ts).await;
        assert_eq!(live_members, replay_members, "he0 members at {ts}");

        let live_w = TemporalQueries::vertex_weight_at(&live_ref, v0, ts).await;
        let replay_w = TemporalQueries::vertex_weight_at(&replayed_ref, v0, ts).await;
        assert_eq!(live_w, replay_w, "v0 weight at {ts}");
    }
}

/// Replay rejects a record whose wire-schema version is newer than this build
/// supports, and a `Topology` record that is unexpectedly sealed.
#[tokio::test]
async fn replay_rejects_future_schema_and_sealed() {
    // Future schema version.
    let tmp = TempDir::new("future-schema");
    {
        let store = FileEventStore::open(tmp.path(), FileStoreConfig::default()).unwrap();
        store
            .append(
                StreamId::Topology,
                Record {
                    timestamp: 0,
                    schema_version: 2, // > WIRE_TOPOLOGY_SCHEMA_VERSION (1)
                    parents: Vec::new(),
                    payload: Payload::Plain(vec![0x00]),
                },
            )
            .unwrap();
        match replay_into_event_log::<W, C, W, C>(&store, SequenceNo(0)) {
            Err(PersistenceError::SchemaVersionUnsupported {
                found,
                max_supported,
            }) => {
                assert_eq!(found, 2);
                assert_eq!(max_supported, 1);
            }
            Err(other) => panic!("expected SchemaVersionUnsupported, got {other:?}"),
            Ok(_) => panic!("expected SchemaVersionUnsupported, got Ok"),
        }
    }

    // Sealed payload on the Topology stream.
    let tmp = TempDir::new("sealed-topology");
    {
        let store = FileEventStore::open(tmp.path(), FileStoreConfig::default()).unwrap();
        store
            .append(
                StreamId::Topology,
                Record {
                    timestamp: 0,
                    schema_version: 1,
                    parents: Vec::new(),
                    payload: Payload::Sealed {
                        key_id: koalisi::persistence::KeyId("k".to_owned()),
                        nonce: [0u8; 24],
                        ciphertext: vec![1, 2, 3],
                    },
                },
            )
            .unwrap();
        match replay_into_event_log::<W, C, W, C>(&store, SequenceNo(0)) {
            Err(PersistenceError::Decode { stream, seq, .. }) => {
                assert_eq!(stream, StreamId::Topology);
                assert_eq!(seq, SequenceNo(0));
            }
            Err(other) => panic!("expected Decode for sealed Topology record, got {other:?}"),
            Ok(_) => panic!("expected Decode for sealed Topology record, got Ok"),
        }
    }
}
