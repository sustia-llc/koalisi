//! Integration tests for the Phase 7 persistence core (feature `persistence`).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use koalisi::persistence::{
    EventStore, FileEventStore, FileStoreConfig, KeyId, Payload, PersistenceError, Record,
    SequenceNo, StreamId, spawn_store_writer,
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
        path.push(format!("koalisi-persist-{}-{tag}-{n}", std::process::id()));
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

fn plain(tag: u8, i: u64) -> Record {
    Record {
        timestamp: i,
        schema_version: 1,
        parents: Vec::new(),
        // A distinctive, multi-byte payload so a byte-flip is meaningful.
        payload: Payload::Plain(vec![tag, (i & 0xFF) as u8, 0xDE, 0xAD, 0xBE, 0xEF]),
    }
}

fn segment_path(root: &Path, stream: StreamId, first_seq: u64) -> PathBuf {
    let dir = match stream {
        StreamId::Topology => "topology",
        StreamId::Registry => "registry",
        StreamId::Beliefs => "beliefs",
        StreamId::Provenance => "provenance",
        StreamId::Decisions => "decisions",
        StreamId::Lineage => "lineage",
    };
    root.join(dir).join(format!("{first_seq:020}.seg"))
}

/// Parse a segment file into `(len_prefix_start, end_exclusive)` byte ranges.
fn frame_ranges(seg_path: &Path) -> Vec<(usize, usize)> {
    let data = fs::read(seg_path).unwrap();
    let mut ranges = Vec::new();
    let mut pos = 0usize;
    while pos < data.len() {
        let len =
            u32::from_le_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        let end = pos + 4 + len;
        ranges.push((pos, end));
        pos = end;
    }
    ranges
}

#[test]
fn append_read_head_roundtrip() {
    let tmp = TempDir::new("roundtrip");
    let store = FileEventStore::open(tmp.path(), FileStoreConfig::default()).unwrap();

    let mut last_topo = None;
    let mut last_reg = None;
    for i in 0..10u64 {
        last_topo = Some(store.append(StreamId::Topology, plain(0xA0, i)).unwrap());
        last_reg = Some(store.append(StreamId::Registry, plain(0xB0, i)).unwrap());
    }

    let topo = store
        .read_from(StreamId::Topology, SequenceNo(0), usize::MAX)
        .unwrap();
    assert_eq!(topo.len(), 10);
    for (i, rec) in topo.iter().enumerate() {
        assert_eq!(rec.seq, SequenceNo(i as u64));
    }
    let reg = store
        .read_from(StreamId::Registry, SequenceNo(0), usize::MAX)
        .unwrap();
    assert_eq!(reg.len(), 10);

    // Streams are independent: identical seqs, but distinct chains (genesis
    // prev_hash is None on both, later hashes differ by payload tag).
    assert_eq!(topo[0].prev_hash, None);
    assert_eq!(reg[0].prev_hash, None);
    assert_ne!(topo[5].hash, reg[5].hash);
    assert_eq!(topo[1].prev_hash, Some(topo[0].hash));

    let head = store.head(StreamId::Topology).unwrap().unwrap();
    assert_eq!(head.seq, SequenceNo(9));
    assert_eq!(head.hash, last_topo.unwrap().hash);
    let rhead = store.head(StreamId::Registry).unwrap().unwrap();
    assert_eq!(rhead.hash, last_reg.unwrap().hash);

    store
        .verify(StreamId::Topology, SequenceNo(0), SequenceNo(9))
        .unwrap();
    store
        .verify(StreamId::Registry, SequenceNo(0), SequenceNo(9))
        .unwrap();
}

#[test]
fn segment_rotation_and_reopen() {
    let tmp = TempDir::new("rotation");
    let config = FileStoreConfig {
        segment_max_records: 4,
    };

    let head_before;
    {
        let store = FileEventStore::open(tmp.path(), config).unwrap();
        for i in 0..10u64 {
            store.append(StreamId::Topology, plain(0xC0, i)).unwrap();
        }
        head_before = store.head(StreamId::Topology).unwrap().unwrap();
    }

    // At least three segments: [0..3], [4..7], [8..9].
    assert!(segment_path(tmp.path(), StreamId::Topology, 0).exists());
    assert!(segment_path(tmp.path(), StreamId::Topology, 4).exists());
    assert!(segment_path(tmp.path(), StreamId::Topology, 8).exists());

    let store = FileEventStore::open(tmp.path(), config).unwrap();
    let all = store
        .read_from(StreamId::Topology, SequenceNo(0), usize::MAX)
        .unwrap();
    assert_eq!(all.len(), 10);
    store
        .verify(StreamId::Topology, SequenceNo(0), SequenceNo(9))
        .unwrap();

    let head_after = store.head(StreamId::Topology).unwrap().unwrap();
    assert_eq!(head_after.seq, head_before.seq);
    assert_eq!(head_after.hash, head_before.hash);

    // Append across the reopen boundary: seq 10, chained onto record 9.
    let h11 = store.append(StreamId::Topology, plain(0xC0, 10)).unwrap();
    assert_eq!(h11.seq, SequenceNo(10));
    let read = store
        .read_from(StreamId::Topology, SequenceNo(10), 1)
        .unwrap();
    assert_eq!(read[0].prev_hash, Some(head_before.hash));
    store
        .verify(StreamId::Topology, SequenceNo(0), SequenceNo(10))
        .unwrap();
}

#[test]
fn verify_detects_tampering() {
    let tmp = TempDir::new("tamper");
    {
        let store = FileEventStore::open(tmp.path(), FileStoreConfig::default()).unwrap();
        for i in 0..5u64 {
            store.append(StreamId::Topology, plain(0xD0, i)).unwrap();
        }
    }

    // Flip the last byte of frame 2 (inside its payload, not the length prefix).
    let seg = segment_path(tmp.path(), StreamId::Topology, 0);
    let ranges = frame_ranges(&seg);
    let (_, end2) = ranges[2];
    let mut data = fs::read(&seg).unwrap();
    data[end2 - 1] ^= 0xFF;
    fs::write(&seg, &data).unwrap();

    // Open only tail-verifies (frame 4), so it succeeds.
    let store = FileEventStore::open(tmp.path(), FileStoreConfig::default()).unwrap();
    let err = store
        .verify(StreamId::Topology, SequenceNo(0), SequenceNo(4))
        .unwrap_err();
    match err {
        PersistenceError::ChainBroken { seq, .. } | PersistenceError::Decode { seq, .. } => {
            assert_eq!(seq, SequenceNo(2));
        }
        other => panic!("expected ChainBroken/Decode at seq 2, got {other:?}"),
    }
}

#[test]
fn torn_tail_recovered() {
    let tmp = TempDir::new("torn");
    let head3;
    {
        let store = FileEventStore::open(tmp.path(), FileStoreConfig::default()).unwrap();
        for i in 0..5u64 {
            store.append(StreamId::Topology, plain(0xE0, i)).unwrap();
        }
        head3 = store
            .read_from(StreamId::Topology, SequenceNo(3), 1)
            .unwrap()[0]
            .hash;
    }

    // Cut the final frame (seq 4) in half.
    let seg = segment_path(tmp.path(), StreamId::Topology, 0);
    let ranges = frame_ranges(&seg);
    let (start4, end4) = ranges[4];
    let new_len = start4 + (end4 - start4) / 2;
    let file = fs::OpenOptions::new().write(true).open(&seg).unwrap();
    file.set_len(new_len as u64).unwrap();
    drop(file);

    let store = FileEventStore::open(tmp.path(), FileStoreConfig::default()).unwrap();
    let all = store
        .read_from(StreamId::Topology, SequenceNo(0), usize::MAX)
        .unwrap();
    assert_eq!(all.len(), 4);
    store
        .verify(StreamId::Topology, SequenceNo(0), SequenceNo(3))
        .unwrap();

    // Next append re-chains against record 3.
    let h4 = store.append(StreamId::Topology, plain(0xE0, 4)).unwrap();
    assert_eq!(h4.seq, SequenceNo(4));
    let read4 = store
        .read_from(StreamId::Topology, SequenceNo(4), 1)
        .unwrap();
    assert_eq!(read4[0].prev_hash, Some(head3));
    store
        .verify(StreamId::Topology, SequenceNo(0), SequenceNo(4))
        .unwrap();
}

#[test]
fn sealed_payload_opaque_roundtrip() {
    let tmp = TempDir::new("sealed");
    let record = Record {
        timestamp: 100,
        schema_version: 2,
        parents: Vec::new(),
        payload: Payload::Sealed {
            key_id: KeyId("subject-42".to_owned()),
            nonce: [3u8; 24],
            ciphertext: vec![0x01, 0x02, 0x03, 0x04, 0x05],
        },
    };
    {
        let store = FileEventStore::open(tmp.path(), FileStoreConfig::default()).unwrap();
        store.append(StreamId::Beliefs, record).unwrap();
    }

    let store = FileEventStore::open(tmp.path(), FileStoreConfig::default()).unwrap();
    let read = store
        .read_from(StreamId::Beliefs, SequenceNo(0), 10)
        .unwrap();
    assert_eq!(read.len(), 1);
    match &read[0].record.payload {
        Payload::Sealed {
            key_id,
            nonce,
            ciphertext,
        } => {
            assert_eq!(key_id, &KeyId("subject-42".to_owned()));
            assert_eq!(*nonce, [3u8; 24]);
            assert_eq!(ciphertext, &vec![0x01, 0x02, 0x03, 0x04, 0x05]);
        }
        Payload::Plain(_) => panic!("expected sealed payload"),
    }
}

#[tokio::test]
async fn writer_task_drains_on_cancel() {
    use tokio::sync::mpsc;
    use tokio_util::sync::CancellationToken;
    use tokio_util::task::TaskTracker;

    let tmp = TempDir::new("writer");
    let store: Arc<dyn EventStore> =
        Arc::new(FileEventStore::open(tmp.path(), FileStoreConfig::default()).unwrap());

    let tracker = TaskTracker::new();
    let token = CancellationToken::new();
    // Channel holds all 20 so they are buffered-and-unconsumed at cancel time,
    // exercising the drain guarantee.
    let (tx, rx) = mpsc::channel(32);
    let handle = spawn_store_writer(store.clone(), rx, &tracker, token.clone());

    for i in 0..20u64 {
        tx.try_send((StreamId::Provenance, plain(0xF0, i))).unwrap();
    }
    token.cancel();
    handle.await.unwrap();

    let head = store.head(StreamId::Provenance).unwrap().unwrap();
    assert_eq!(head.seq, SequenceNo(19));
    let all = store
        .read_from(StreamId::Provenance, SequenceNo(0), usize::MAX)
        .unwrap();
    assert_eq!(all.len(), 20);
}

#[test]
fn empty_and_bounds_semantics() {
    let tmp = TempDir::new("empty");
    let store = FileEventStore::open(tmp.path(), FileStoreConfig::default()).unwrap();

    assert!(store.head(StreamId::Provenance).unwrap().is_none());
    assert!(
        store
            .read_from(StreamId::Provenance, SequenceNo(0), 10)
            .unwrap()
            .is_empty()
    );
    match store.verify(StreamId::Provenance, SequenceNo(0), SequenceNo(0)) {
        Err(PersistenceError::StreamEmpty(StreamId::Provenance)) => {}
        other => panic!("expected StreamEmpty, got {other:?}"),
    }

    store.append(StreamId::Provenance, plain(0x11, 0)).unwrap();
    // from beyond head reads empty.
    assert!(
        store
            .read_from(StreamId::Provenance, SequenceNo(5), 10)
            .unwrap()
            .is_empty()
    );
}
