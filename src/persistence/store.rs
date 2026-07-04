//! The [`EventStore`] trait and its default [`FileEventStore`] implementation.
//!
//! `FileEventStore` lays each stream out as a subdirectory of segment files
//! (`{first_seq:020}.seg`); each segment is a sequence of
//! `[u32 LE frame_len][frame_bytes]`. Sequence numbers are dense and per-stream.
//! See the module docs ([`persistence`](super)) for the hash and durability
//! contracts.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use super::chain::{check_back_link, decode_frame, encode_frame, hash_frame};
use super::envelope::{Record, SequenceNo, StoredRecord, StreamHead, StreamId};
use super::errors::PersistenceError;

/// A durable, per-stream hash-chained append-only log.
///
/// The trait is synchronous and lives off the hot path; producers reach it
/// through [`spawn_store_writer`](super::writer::spawn_store_writer). All four
/// methods are cheap to call under a shared reference — implementations use
/// interior mutability for `append`.
pub trait EventStore: Send + Sync {
    /// Append `record` to `stream`: assign the next [`SequenceNo`], chain
    /// `prev_hash`, frame, hash, and write per the store's durability policy.
    ///
    /// # Errors
    ///
    /// Returns a [`PersistenceError`] if encoding or the underlying write fails.
    fn append(&self, stream: StreamId, record: Record) -> Result<StreamHead, PersistenceError>;

    /// Read up to `limit` records with sequence `>= from`, in order. An empty
    /// stream, `from` beyond the head, or `limit == 0` yields an empty vec.
    ///
    /// # Errors
    ///
    /// Returns a [`PersistenceError`] if a stored frame cannot be read or decoded.
    fn read_from(
        &self,
        stream: StreamId,
        from: SequenceNo,
        limit: usize,
    ) -> Result<Vec<StoredRecord>, PersistenceError>;

    /// The current head of `stream`, or `None` if it is empty.
    ///
    /// # Errors
    ///
    /// Returns a [`PersistenceError`] only if the implementation needs I/O to
    /// answer (the file store does not).
    fn head(&self, stream: StreamId) -> Result<Option<StreamHead>, PersistenceError>;

    /// Re-hash stored frames and check chain continuity over `[from, to]`. `to`
    /// is clamped to the head; `from > to` (after clamping) verifies trivially.
    ///
    /// # Errors
    ///
    /// - [`PersistenceError::StreamEmpty`] if the stream has no records.
    /// - [`PersistenceError::ChainBroken`] / [`PersistenceError::Decode`] if the
    ///   chain does not verify.
    fn verify(
        &self,
        stream: StreamId,
        from: SequenceNo,
        to: SequenceNo,
    ) -> Result<(), PersistenceError>;
}

/// Configuration for a [`FileEventStore`].
#[derive(Clone, Copy, Debug)]
pub struct FileStoreConfig {
    /// Records per segment file before rotation. Rotation `sync_data`s the
    /// closed segment.
    pub segment_max_records: usize,
}

impl Default for FileStoreConfig {
    fn default() -> Self {
        Self {
            segment_max_records: 1024,
        }
    }
}

/// In-memory index of one on-disk segment file.
#[derive(Debug)]
struct SegmentIndex {
    /// Sequence number of this segment's first record (also its filename stem).
    first_seq: u64,
    /// Path to the segment file.
    path: PathBuf,
    /// Byte offset of each frame's length prefix, indexed by `seq - first_seq`.
    frame_offsets: Vec<u64>,
}

/// Per-stream mutable state.
#[derive(Debug)]
struct StreamState {
    /// This stream's subdirectory.
    dir: PathBuf,
    /// Segments in ascending `first_seq` order; the last is active.
    segments: Vec<SegmentIndex>,
    /// Next sequence number to assign (also the record count).
    next_seq: u64,
    /// Current head, or `None` if empty.
    head: Option<StreamHead>,
    /// Buffered writer for the active (last) segment, if any.
    writer: Option<BufWriter<File>>,
    /// Byte length of the active segment file.
    active_len: u64,
    /// Set after a failed write leaves the in-memory index out of sync with the
    /// physical file; further appends are refused until the store is reopened.
    wedged: bool,
}

/// The lock-protected interior of a [`FileEventStore`].
#[derive(Debug)]
struct StoreInner {
    config: FileStoreConfig,
    streams: [StreamState; 6],
}

/// A segmented, append-only, hash-chained file store.
///
/// `append` takes `&self` and serializes all operations behind a `Mutex`. On a
/// poisoned lock the guard is recovered ([`PoisonError::into_inner`]) rather
/// than propagating a panic: reopening re-scans each segment's frame structure
/// and truncates a torn tail (full-chain integrity is a separate explicit
/// [`EventStore::verify`] call, not part of open), so a partial mutation under a
/// panicking thread cannot leave a misframed segment undetected.
#[derive(Debug)]
pub struct FileEventStore {
    inner: Mutex<StoreInner>,
}

impl FileEventStore {
    /// Open (creating if absent) a store rooted at `dir`.
    ///
    /// Scans every stream's segments, rebuilding the in-memory index and
    /// recovering a torn tail on the last segment (an incomplete final frame is
    /// truncated with a `warn`). The last frame of each non-empty stream is
    /// re-verified against its predecessor.
    ///
    /// # Errors
    ///
    /// - [`PersistenceError::Io`] on any filesystem failure.
    /// - [`PersistenceError::Decode`] on a torn frame anywhere but the tail of
    ///   the last segment, or a segment sequence discontinuity.
    /// - [`PersistenceError::ChainBroken`] if a stream's tail fails re-verification.
    pub fn open(dir: impl AsRef<Path>, config: FileStoreConfig) -> Result<Self, PersistenceError> {
        let root = dir.as_ref();
        fs::create_dir_all(root)?;

        let mut built: Vec<StreamState> = Vec::with_capacity(StreamId::ALL.len());
        for stream in StreamId::ALL {
            let sdir = root.join(stream.as_dir_name());
            fs::create_dir_all(&sdir)?;
            built.push(scan_stream(stream, sdir)?);
        }
        let streams: [StreamState; 6] = built
            .try_into()
            .map_err(|_| PersistenceError::Io(io::Error::other("stream state array build")))?;

        Ok(Self {
            inner: Mutex::new(StoreInner { config, streams }),
        })
    }

    /// Test-only: force `stream` into the wedged state, simulating a failed
    /// write, so the wedge/reopen recovery contract can be exercised
    /// deterministically without fault-injecting the OS write path.
    #[cfg(test)]
    pub(crate) fn wedge_for_test(&self, stream: StreamId) {
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        inner.streams[stream.to_u8() as usize].wedged = true;
    }
}

impl EventStore for FileEventStore {
    fn append(&self, stream: StreamId, record: Record) -> Result<StreamHead, PersistenceError> {
        let mut inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        inner.append(stream, &record)
    }

    fn read_from(
        &self,
        stream: StreamId,
        from: SequenceNo,
        limit: usize,
    ) -> Result<Vec<StoredRecord>, PersistenceError> {
        let inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        inner.read_from(stream, from, limit)
    }

    fn head(&self, stream: StreamId) -> Result<Option<StreamHead>, PersistenceError> {
        let inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(inner.streams[stream.to_u8() as usize].head)
    }

    fn verify(
        &self,
        stream: StreamId,
        from: SequenceNo,
        to: SequenceNo,
    ) -> Result<(), PersistenceError> {
        let inner = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        inner.verify(stream, from, to)
    }
}

impl StoreInner {
    fn append(
        &mut self,
        stream: StreamId,
        record: &Record,
    ) -> Result<StreamHead, PersistenceError> {
        let idx = stream.to_u8() as usize;
        if self.streams[idx].wedged {
            return Err(PersistenceError::StreamWedged(stream));
        }

        // Pre-write work (rotation, encoding). Errors here happen before any
        // frame byte is written, so they propagate without wedging.
        self.rotate_if_needed(stream)?;
        let (seq, prev_hash) = {
            let state = &self.streams[idx];
            (state.next_seq, state.head.map(|h| h.hash))
        };
        let frame = encode_frame(record, SequenceNo(seq), prev_hash)?;
        let hash = hash_frame(&frame);
        let len = u32::try_from(frame.len())
            .map_err(|_| PersistenceError::Encode("frame exceeds u32 length prefix".to_owned()))?;

        let state = &mut self.streams[idx];
        let offset = state.active_len;

        // Write phase: a failure here may have grown the physical file while the
        // in-memory index stays stale — wedge the stream so we never append at a
        // misaligned offset (reopen re-scans and truncates the torn tail).
        if let Err(e) = write_frame(state, len, &frame) {
            state.wedged = true;
            return Err(e);
        }

        if let Some(seg) = state.segments.last_mut() {
            seg.frame_offsets.push(offset);
        }
        state.active_len += 4 + frame.len() as u64;
        state.next_seq = seq + 1;
        let head = StreamHead {
            seq: SequenceNo(seq),
            hash,
        };
        state.head = Some(head);
        Ok(head)
    }

    /// Ensure the active segment has room; rotate (sync + new file) if full or
    /// absent.
    fn rotate_if_needed(&mut self, stream: StreamId) -> Result<(), PersistenceError> {
        let idx = stream.to_u8() as usize;
        let need_new = match self.streams[idx].segments.last() {
            None => true,
            Some(seg) => seg.frame_offsets.len() >= self.config.segment_max_records,
        };
        if !need_new {
            return Ok(());
        }

        let state = &mut self.streams[idx];
        if let Some(writer) = state.writer.as_mut() {
            writer.flush()?;
            writer.get_ref().sync_data()?;
        }
        let first_seq = state.next_seq;
        let path = state.dir.join(format!("{first_seq:020}.seg"));
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        state.writer = Some(BufWriter::new(file));
        state.segments.push(SegmentIndex {
            first_seq,
            path,
            frame_offsets: Vec::new(),
        });
        state.active_len = 0;
        Ok(())
    }

    fn read_from(
        &self,
        stream: StreamId,
        from: SequenceNo,
        limit: usize,
    ) -> Result<Vec<StoredRecord>, PersistenceError> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let state = &self.streams[stream.to_u8() as usize];
        if state.next_seq == 0 {
            return Ok(Vec::new());
        }
        let head_seq = state.next_seq - 1;
        if from.0 > head_seq {
            return Ok(Vec::new());
        }
        let span = u64::try_from(limit - 1).unwrap_or(u64::MAX);
        let end = from.0.saturating_add(span).min(head_seq);

        let mut out = Vec::new();
        for s in from.0..=end {
            let bytes = read_frame_bytes_at(&state.segments, stream, s)?;
            out.push(decode_frame(&bytes, stream, SequenceNo(s))?);
        }
        Ok(out)
    }

    fn verify(
        &self,
        stream: StreamId,
        from: SequenceNo,
        to: SequenceNo,
    ) -> Result<(), PersistenceError> {
        let state = &self.streams[stream.to_u8() as usize];
        if state.next_seq == 0 {
            return Err(PersistenceError::StreamEmpty(stream));
        }
        let head_seq = state.next_seq - 1;
        let to = to.0.min(head_seq);
        if from.0 > to {
            return Ok(());
        }

        let mut predecessor_hash = if from.0 == 0 {
            None
        } else {
            let bytes = read_frame_bytes_at(&state.segments, stream, from.0 - 1)?;
            Some(hash_frame(&bytes))
        };
        for s in from.0..=to {
            let bytes = read_frame_bytes_at(&state.segments, stream, s)?;
            let stored = decode_frame(&bytes, stream, SequenceNo(s))?;
            check_back_link(stream, &stored, predecessor_hash)?;
            predecessor_hash = Some(stored.hash);
        }
        Ok(())
    }
}

/// Write one length-prefixed frame to the stream's active segment and flush to
/// the OS (survives process kill; no fsync per append). On any error the caller
/// wedges the stream.
fn write_frame(state: &mut StreamState, len: u32, frame: &[u8]) -> Result<(), PersistenceError> {
    let writer = state
        .writer
        .as_mut()
        .ok_or_else(|| PersistenceError::Io(io::Error::other("no active segment writer")))?;
    writer.write_all(&len.to_le_bytes())?;
    writer.write_all(frame)?;
    writer.flush()?;
    Ok(())
}

/// The segment containing `seq`, searching newest-first.
fn segment_containing(segments: &[SegmentIndex], seq: u64) -> Option<&SegmentIndex> {
    segments
        .iter()
        .rev()
        .find(|seg| seg.first_seq <= seq && seq < seg.first_seq + seg.frame_offsets.len() as u64)
}

/// Read the exact stored frame bytes (no length prefix) for `seq`.
fn read_frame_bytes_at(
    segments: &[SegmentIndex],
    stream: StreamId,
    seq: u64,
) -> Result<Vec<u8>, PersistenceError> {
    let seg = segment_containing(segments, seq).ok_or_else(|| PersistenceError::Decode {
        stream,
        seq: SequenceNo(seq),
        message: "sequence not found in any segment".to_owned(),
    })?;
    let idx = usize::try_from(seq - seg.first_seq).map_err(|_| PersistenceError::Decode {
        stream,
        seq: SequenceNo(seq),
        message: "segment index overflow".to_owned(),
    })?;
    let offset = seg.frame_offsets[idx];

    let mut file = File::open(&seg.path)?;
    let file_len = file.metadata()?.len();
    file.seek(SeekFrom::Start(offset))?;
    let mut len_buf = [0u8; 4];
    file.read_exact(&mut len_buf)?;
    let len = u64::from(u32::from_le_bytes(len_buf));
    // Bound the claimed length against the bytes actually on disk before
    // allocating — a corrupted prefix must not drive a multi-GiB allocation.
    let available = file_len.saturating_sub(offset + 4);
    if len > available {
        return Err(PersistenceError::Decode {
            stream,
            seq: SequenceNo(seq),
            message: "frame length exceeds segment bounds".to_owned(),
        });
    }
    let len = usize::try_from(len).map_err(|_| PersistenceError::Decode {
        stream,
        seq: SequenceNo(seq),
        message: "frame length overflow".to_owned(),
    })?;
    let mut frame = vec![0u8; len];
    file.read_exact(&mut frame)?;
    Ok(frame)
}

/// Scan one stream directory, building its [`StreamState`] and recovering a
/// torn tail.
///
/// Note: each segment file is read fully into memory during the scan, and the
/// per-record byte offsets (8 B/record) are retained for the lifetime of the
/// store. Fine at P7.1 scale; revisit before logs grow large.
fn scan_stream(stream: StreamId, dir: PathBuf) -> Result<StreamState, PersistenceError> {
    let mut seg_files: Vec<(u64, PathBuf)> = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("seg") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if let Ok(first_seq) = stem.parse::<u64>() {
            seg_files.push((first_seq, path));
        }
    }
    seg_files.sort_by_key(|(first, _)| *first);

    let mut segments: Vec<SegmentIndex> = Vec::new();
    let mut next_seq: u64 = 0;
    let mut active_len: u64 = 0;
    let count = seg_files.len();
    for (i, (first_seq, path)) in seg_files.into_iter().enumerate() {
        if first_seq != next_seq {
            return Err(PersistenceError::Decode {
                stream,
                seq: SequenceNo(first_seq),
                message: format!("segment first_seq {first_seq} does not continue from {next_seq}"),
            });
        }
        let is_last = i + 1 == count;
        let (seg, seg_len) = scan_segment(stream, first_seq, path, is_last)?;
        next_seq = first_seq + seg.frame_offsets.len() as u64;
        active_len = seg_len;
        segments.push(seg);
    }

    let head = compute_head(stream, &segments, next_seq)?;

    let writer = match segments.last() {
        Some(seg) => Some(BufWriter::new(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&seg.path)?,
        )),
        None => None,
    };

    Ok(StreamState {
        dir,
        segments,
        next_seq,
        head,
        writer,
        active_len,
        wedged: false,
    })
}

/// Compute the stream head and re-verify the tail frame's back-link.
fn compute_head(
    stream: StreamId,
    segments: &[SegmentIndex],
    next_seq: u64,
) -> Result<Option<StreamHead>, PersistenceError> {
    if next_seq == 0 {
        return Ok(None);
    }
    let last_seq = next_seq - 1;
    let bytes = read_frame_bytes_at(segments, stream, last_seq)?;
    let stored = decode_frame(&bytes, stream, SequenceNo(last_seq))?;

    let predecessor_hash = if last_seq == 0 {
        None
    } else {
        let prev_bytes = read_frame_bytes_at(segments, stream, last_seq - 1)?;
        Some(hash_frame(&prev_bytes))
    };
    check_back_link(stream, &stored, predecessor_hash)?;

    Ok(Some(StreamHead {
        seq: SequenceNo(last_seq),
        hash: stored.hash,
    }))
}

/// Scan a single segment file, returning its index and its valid byte length.
///
/// A torn final frame is truncated (tail recovery) only when `is_last`;
/// elsewhere it is corruption.
fn scan_segment(
    stream: StreamId,
    first_seq: u64,
    path: PathBuf,
    is_last: bool,
) -> Result<(SegmentIndex, u64), PersistenceError> {
    let data = fs::read(&path)?;
    let total = data.len() as u64;
    let mut offsets: Vec<u64> = Vec::new();
    let mut pos: u64 = 0;
    let mut seq = first_seq;
    let mut torn = false;

    loop {
        let remaining = total - pos;
        if remaining == 0 {
            break;
        }
        if remaining < 4 {
            torn = true;
            break;
        }
        let p = usize::try_from(pos).map_err(|_| PersistenceError::Decode {
            stream,
            seq: SequenceNo(seq),
            message: "segment offset overflow".to_owned(),
        })?;
        let len = u64::from(u32::from_le_bytes([
            data[p],
            data[p + 1],
            data[p + 2],
            data[p + 3],
        ]));
        if 4 + len > remaining {
            torn = true;
            break;
        }
        offsets.push(pos);
        pos += 4 + len;
        seq += 1;
    }

    if torn {
        if is_last {
            let file = OpenOptions::new().write(true).open(&path)?;
            file.set_len(pos)?;
            file.sync_data()?;
            tracing::warn!(
                stream = ?stream,
                segment = %path.display(),
                truncated_to = pos,
                "recovered torn tail: truncated incomplete final frame"
            );
        } else {
            return Err(PersistenceError::Decode {
                stream,
                seq: SequenceNo(seq),
                message: "torn frame in non-tail segment".to_owned(),
            });
        }
    }

    Ok((
        SegmentIndex {
            first_seq,
            path,
            frame_offsets: offsets,
        },
        pos,
    ))
}

#[cfg(test)]
mod tests {
    use super::{EventStore, FileEventStore, FileStoreConfig};
    use crate::persistence::envelope::{Payload, Record, SequenceNo, StreamId};
    use crate::persistence::errors::PersistenceError;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn unique_dir(tag: &str) -> std::path::PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut path = std::env::temp_dir();
        path.push(format!(
            "koalisi-persist-unit-{}-{tag}-{n}",
            std::process::id()
        ));
        path
    }

    fn plain(i: u64) -> Record {
        Record {
            timestamp: i,
            schema_version: 1,
            parents: Vec::new(),
            payload: Payload::Plain(vec![0xAB, (i & 0xFF) as u8]),
        }
    }

    /// A wedged stream refuses appends; reopening the store re-scans, recovers,
    /// and resumes appending at the correct sequence.
    #[test]
    fn append_failure_wedges_stream() {
        let dir = unique_dir("wedge");
        {
            let store = FileEventStore::open(&dir, FileStoreConfig::default()).unwrap();
            store.append(StreamId::Topology, plain(0)).unwrap();
            store.append(StreamId::Topology, plain(1)).unwrap();

            // Simulate a failed write leaving the index desynced.
            store.wedge_for_test(StreamId::Topology);
            match store.append(StreamId::Topology, plain(2)) {
                Err(PersistenceError::StreamWedged(StreamId::Topology)) => {}
                other => panic!("expected StreamWedged, got {other:?}"),
            }
            // A different stream is unaffected.
            store.append(StreamId::Registry, plain(0)).unwrap();
        }

        // Recovery = reopen.
        let store = FileEventStore::open(&dir, FileStoreConfig::default()).unwrap();
        store
            .verify(StreamId::Topology, SequenceNo(0), SequenceNo(1))
            .unwrap();
        let all = store
            .read_from(StreamId::Topology, SequenceNo(0), usize::MAX)
            .unwrap();
        assert_eq!(all.len(), 2);
        let head = store.append(StreamId::Topology, plain(2)).unwrap();
        assert_eq!(head.seq, SequenceNo(2));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
