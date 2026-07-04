# Phase 7 persistence design — EventStore over the event-sourced hypergraph

**Status: DESIGN (issue [#21](https://github.com/sustia-llc/koalisi/issues/21)) —
implementation is follow-up work (§16), gated on sign-off of this document.**

Replaces the original Phase 7 plan (feature-gated `PersistentHypergraph` from
yamafaktory hypergraph v4.2.0), which is obsolete since K1
([#4](https://github.com/sustia-llc/koalisi/issues/4)): that backend was
dropped, and `catgraph_applied::Hypergraph` deliberately ships no
serde/persistence. What survives from the original plan is the `EventStore`
trait idea; graph-state durability is **event-log-only** — point-in-time
reconstruction via `TemporalQueries` already covers state rebuild.

Co-designed with [#18](https://github.com/sustia-llc/koalisi/issues/18)
(magnitude trajectory over the event log), which consumes the same read
surface (§7).

---

## 1. Binding requirements

The requirements payload is the 2026-07-03 design-input-#2 comment on #21
(driver surveys of tauhokohoko + NEST, recorded verbatim there). Numbering
below is used throughout this doc as [R1]–[R9].

From tauhokohoko (Indigenous Data Sovereignty — hard constraints):

- **[R1] Append-only, hash-chained, composable.** KERI's KEL, an
  `event_store`, and DeepCausality's EPP `EffectLog` are "architecturally the
  same pattern… The three log types should be composable"
  (causal-context-architecture R5). A causal-log entry must be anchorable to
  a KEL interaction event (`ixn`).
- **[R2] Crypto-deletion (the crux).** Per-record encryption; deletion = key
  destruction: "the ciphertext stays on disk but is mathematically
  unrecoverable" (the #21 payload's gloss of the Stroh pattern; m2 design-doc
  §7.4 states it as "deletion destroys the per-record key" with
  cryptographic-deletion finality). Append-only must mean
  **append-ciphertext with per-record keys** for sensitive payloads. Stroh's
  I1 runtime tenant-isolation invariant is explicitly NOT adopted — only the
  data-layer deletion pattern.
- **[R3] Bilateral bounded federation.** Sharing per signed, revocable
  manifest; no central registry, no network-wide protocol. Cross-boundary
  propagation is per-manifest-gated — NOT the broadcast bus, NOT the libp2p
  gateway (those stay local / publish-boundary per the K3 §8 rationale).
- **[R4] Revocation is an appended registry event, never a field mutation**
  (m2 design-doc §3.0 invariant 1). Point-in-time reconstruction must honor
  current revocation state as first-class events.
- **[R5] 50-year portability.** "Expressible against portable identifiers so
  a community can migrate platforms without rewriting the governance layer"
  (m2 design-doc §1). Favors a portable event-log wire format over a
  SurrealDB-locked store.
- **[R6] Decision traces as causal graphs.** "Capable of producing a causal
  trace structurally compatible with EPP's `EffectLog`"
  (causal-context-architecture R1) — a graph, not a flat log.
- **[R7] Privacy adjuncts.** Correlatable identity records carry a nonce
  (m2 §3.0 invariant 2); some data is filtered upstream and never stored —
  the sacred-never-enters rule (a gloss of m2 §3.0 invariant 3, "Some things
  never enter the system").

From NEST:

- **[R8] FAIR/reproducible run provenance.** DOI-minted output packs,
  multi-mirror archival (Zenodo-CERN, AWS Open-Data Registry, Internet
  Archive, Software Heritage), Schmidt <12-month release rule
  (nest-overview). The EventStore doubles as run provenance.
- **[R9] Particle-lineage recording has a concrete consumer**: NEST's
  Stage-2 ensemble strategy (~20 full solves anchoring ~80 surrogate runs)
  is what Phase 5 (#20) lineages replay against. Schema reserved now;
  Phase 5 is held.

Pinned negatives (from the same payload):

- koalisi is NOT the SPRT provider (tauhokohoko sources SPRT in its
  DeepCausality service).
- The credential/KERI/ACDC layer is out of scope — koalisi persists at the
  **evaluation layer**. Where this design touches KERI it emits digests for
  someone else to anchor (§5, §10); it never implements KELs.
- These requirements are design constraints, not a funded delivery.

## 2. Existing machinery — what each piece is and is not

| Piece | Is | Is not |
|---|---|---|
| `TemporalEvent<V,HE>` (`src/topology/events.rs`) | 13-variant event record (12 mutations + `SnapshotMarker`) appended by every `TemporalHypergraph` write and by `create_snapshot`; `Clone + Debug` only | serializable (no serde — deliberate, see §4) |
| `EventLog` (`src/topology/event_log.rs`) | in-memory append-only `Vec` + BTreeMap time index + per-entity indices; never mutated, never evicted | durable (process-lifetime only) |
| `TemporalQueries` / `TemporalAnalytics` (`queries.rs`, `analytics.rs`) | point-in-time folds and series over `events_ref()` — the read surface #18 extends | tied to any storage backend |
| `DecisionRecord` tap (`src/subsystems/coalition_actor.rs`) | non-blocking `try_send` tap on every policy-gated join/leave decision; drop-with-warn | a durability guarantee (tap-side loss is possible; K3 gotcha 14) |
| `durable` feature (`src/subsystems/durable.rs`, surrealdb-live-message v0.2.0) | two-tier restart-durable decision bus: CHANGEFEED append + LIVE wakeup + per-agent versionstamp cursor replay, at-least-once | an event store — CHANGEFEED retention is a bounded window; and it is SurrealDB-shaped, failing [R5] as a source of truth |
| Dead: `PersistentHypergraph`/fjall (original plan) | — | revivable: the backend is gone (K1), and upstream catgraph deliberately has no persistence |

The gap Phase 7 fills: nothing survives process exit except the bounded
durable-bus window, and nothing satisfies [R1]–[R8].

## 3. Architecture: layered, portable log as source of truth

**Decision.** A portable, segmented, append-only, hash-chained frame log is
the sole source of truth. Everything else — the K3 `durable` SurrealDB bus,
future dashboards, search indexes — is a **projection**: rebuildable from the
log, never authoritative.

```
producers                     source of truth                 projections
─────────                     ───────────────                 ───────────
TemporalHypergraph ──┐
  (topology events)  │  mpsc   ┌──────────────────┐   replay  ┌─────────────────┐
DecisionRecord tap ──┼───────▶ │ EventStore        │ ────────▶ │ EventLog (mem)  │──▶ TemporalQueries
  (decision traces)  │ writer  │  chained streams  │           │                 │    TemporalAnalytics
belief snapshots ────┤  task   │  CBOR frames      │           └─────────────────┘    (#18 magnitude_history)
  (feature decision) │         │  Plain | Sealed   │   tee     ┌─────────────────┐
run provenance ──────┘         └──────────────────┘ ────────▶ │ durable bus     │ (optional, feature durable)
                                       │                       │ (live, bounded)│
                                       ▼ export_segment        └─────────────────┘
                               signed Manifest + segment bytes  (bilateral federation, §10)
```

**Rejected: SurrealDB-backed source of truth.** The K3 bus is already
in-tree and gives cursor replay for free, but it fails [R5] (a
SurrealDB-locked store is not a 50-year portable format), its CHANGEFEED
retention is a bounded window by design, and hash-chaining/manifest export
would have to be bolted onto someone else's storage engine. It stays as the
live projection it already is.

**Trade accepted:** two write paths when `durable` is also enabled (log +
projection tee), and file-store plumbing SurrealDB would have provided.
Bought: no vendor lock [R5], a chainable byte stream [R1], and offline
segment exchange for federation [R3] — none of which a CHANGEFEED window can
provide.

### Wire encoding: CBOR

**Decision.** Frames are CBOR (RFC 8949), via a serde-compatible
implementation (`ciborium` as the working candidate; see §17).

- CBOR is an IETF standard with a specified deterministic-encoding profile
  (RFC 8949 §4.2) — the strongest 50-year re-implementability story [R5] and
  reproducible bytes for hashing.
- The KERI-adjacent ecosystem [R1] already digests CBOR, easing KEL
  anchorability.
- In-tree precedent: the `remote` gateway already speaks
  libp2p-request-response CBOR.

**Rejected: MessagePack (`rmp-serde`)** — the original plan's choice.
Compact and mature, but no IETF specification and no canonical-encoding
profile; a weaker archival argument. **Rejected: JSON-lines** as the store
format (verbose, float round-trip hazards); it survives as a human-readable
*export/debug view* only.

**Hash contract (load-bearing):** the chain hashes the **exact stored frame
bytes** (length-prefixed), never a re-encoding. Verification therefore never
depends on encoder determinism across implementations or decades;
deterministic CBOR is a reproducibility bonus, not a correctness
requirement.

## 4. Record model

### Envelope, not naked events

Every stored record is an envelope; the payload is opaque bytes, either
plain or sealed:

- `Record { timestamp: u64, schema_version: u16, parents: Vec<EventRef>, payload: Payload }`
  — what a producer hands to `append`.
- `Payload::Plain(Vec<u8>)` | `Payload::Sealed { key_id, nonce, ciphertext }`
  — sealing happens in the adapter **before** append; the store never sees
  sealed plaintext. "Append-only means append-ciphertext" [R2] falls out of
  the type.
- The store assigns `SequenceNo` and `prev_hash` on append and returns the
  new `StoredRecord` head.

### Streams

Records live in independent, individually hash-chained streams:

| Stream | Contents | Sealing | Producer |
|---|---|---|---|
| `Topology` | `WireTopologyEvent` (13-variant mirror of `TemporalEvent`) | Plain — anonymous indices, structure carries no identity | `TemporalHypergraph` append tee |
| `Decisions` | decision traces with causal parents (§11) | identity-bearing fields sealable | `DecisionRecord` forwarder |
| `Beliefs` | EFE belief snapshots (`TrustBeliefs`/`CompatibilityBeliefs`/`CoalitionHistory`) | Sealed — inter-agent trust is correlatable [R2][R7] | feature `decision` + `persistence` |
| `Lineage` | **RESERVED** — SwarmAgentic particle lineages | Sealed | none until #20 unholds; NEST Stage-2 is the consumer [R9] |
| `Registry` | grants, revocations, key destruction, manifest revocation | Never sealed, never revoked (it IS the revocation record) [R4] | sealing/federation layers |
| `Provenance` | FAIR run manifests, output-pack digests [R8] | Plain | run harnesses (e.g. the K4 battery) |

Per-stream chains (rather than one interleaved chain) because
crypto-deletion classification, federation manifests, and revocation each
operate per-stream; interleaving would couple them and make partial export
impossible without breaking chain verification.

### Wire projection, not serde on domain types

**Decision.** `TemporalEvent<V,HE>` does NOT gain serde derives. A versioned
wire mirror (`WireTopologyEvent`, 13 variants, serde-derived, raw `u64`
index/timestamp fields, generic over serializable weight projections) lives
at the persistence boundary, with `From<&TemporalEvent<V,HE>>` and a
fallible inverse.

- Same deliberate call already made at the K3 boundary
  (`DecisionRecord` → `DecisionEvent` in `durable.rs`).
- No `Serialize` bounds leak into every topology API signature.
- The wire schema is frozen and versioned (`schema_version` per record)
  independently of the in-memory enum — the [R5] argument.
- Persisting raw `VertexIndex`/`HyperedgeIndex` values is legitimate: they
  are stable and never reused (catgraph contract, CLAUDE.md gotcha 12) — the
  in-memory event log already depends on this for replay. They are written
  as named `u64` fields, not opaque newtypes.
- Note `Timestamp`/`SnapshotId` inner fields are `pub(crate)` and
  serde-free; the projection lives in-crate and the wire carries raw `u64`.

Trade: ~200 lines of mirror boilerplate per stream vs. domain types staying
persistence-free. Accepted; precedent already exists.

## 5. Hash chaining and verification

- Per-record `prev_hash` over the exact stored frame bytes of the
  predecessor (§3 hash contract). Genesis record of a stream has none.
- Digests are **algorithm-tagged** (multihash-style): v1 pins **SHA-256** —
  the conservative archival-ubiquity and KERI-interop choice; BLAKE3 is the
  named performance alternative, adoptable later without a format break
  because the tag travels with every digest (see §17).
- Verified at three points: on append (writer computes and chains), on
  open/recovery (tail re-verification detects torn writes), and on manifest
  import (full-range verify, §10).
- **KEL anchorability [R1]:** `StreamHead { seq, hash }` is the digest a
  KERI controller anchors in a KEL `ixn` event. koalisi emits the digest and
  stops there — the ACDC/KERI layer is out of scope (pinned negative). This
  is the "ACDC envelope around an EPP chain" composition point cited in
  causal-context-architecture R5; we cite it, we don't re-derive it.

## 6. The `EventStore` trait and the writer seam

Design-level surface (implementation in P7.1; koalisi hand-rolled error
convention per `topology::errors`, no thiserror; no `.unwrap()`):

```rust
// src/persistence/ (feature = "persistence")

/// Logical stream within the store. Each stream hash-chains independently.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum StreamId { Topology, Decisions, Beliefs, Lineage, Registry, Provenance }

/// Per-stream dense monotonic cursor.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SequenceNo(pub u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HashAlgorithm { Sha256 }

/// Algorithm-tagged digest over the exact stored frame bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordHash { pub algorithm: HashAlgorithm, pub digest: [u8; 32] }

/// Opaque handle into the (destroyable) key registry (§8).
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct KeyId(pub String);

/// Payload as it enters the store: already projected + CBOR-encoded,
/// optionally sealed. The store never sees plaintext of sealed payloads.
#[derive(Clone, Debug)]
pub enum Payload {
    Plain(Vec<u8>),
    Sealed { key_id: KeyId, nonce: [u8; 24], ciphertext: Vec<u8> },
}

/// Causal parent reference — the EPP EffectLog edge shape (§11).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventRef { pub stream: StreamId, pub seq: SequenceNo }

/// What a producer hands to `append`; the store assigns seq and prev_hash.
#[derive(Clone, Debug)]
pub struct Record {
    /// Logical-clock value (Timestamp); wire carries the raw u64.
    pub timestamp: u64,
    /// Wire-schema version of the payload (bump on projection change).
    pub schema_version: u16,
    /// Causal antecedents (empty for topology events).
    pub parents: Vec<EventRef>,
    pub payload: Payload,
}

/// A record as read back, with the envelope fields the store assigned.
#[derive(Clone, Debug)]
pub struct StoredRecord {
    pub seq: SequenceNo,
    /// None only for a stream's genesis record.
    pub prev_hash: Option<RecordHash>,
    pub hash: RecordHash,
    pub record: Record,
}

/// Stream head: the digest a KERI controller anchors in a KEL ixn (§5).
#[derive(Clone, Copy, Debug)]
pub struct StreamHead { pub seq: SequenceNo, pub hash: RecordHash }

pub trait EventStore: Send + Sync {
    /// Append: assign the next SequenceNo, chain prev_hash, frame + hash +
    /// write per the store's durability policy.
    fn append(&self, stream: StreamId, record: Record) -> Result<StreamHead, PersistenceError>;

    /// Read up to `limit` records with seq >= `from`, in sequence order.
    fn read_from(&self, stream: StreamId, from: SequenceNo, limit: usize)
        -> Result<Vec<StoredRecord>, PersistenceError>;

    /// Current head of a stream (None if empty).
    fn head(&self, stream: StreamId) -> Result<Option<StreamHead>, PersistenceError>;

    /// Re-hash stored frames and check chain continuity over [from, to].
    fn verify(&self, stream: StreamId, from: SequenceNo, to: SequenceNo)
        -> Result<(), PersistenceError>;
}

#[derive(Debug)]
pub enum PersistenceError {
    Io(std::io::Error),
    Encode(String),
    Decode { stream: StreamId, seq: SequenceNo, message: String },
    ChainBroken { stream: StreamId, seq: SequenceNo },
    SchemaVersionUnsupported { found: u16, max_supported: u16 },
    /// Sealed record whose key was destroyed — crypto-deleted (§8).
    KeyUnavailable(KeyId),
    ManifestInvalid(String),
    ManifestRevoked(String),
    StreamEmpty(StreamId),
}
// + manual Display / std::error::Error / From<io::Error>, per topology::errors convention.
```

Design points:

- **Cursor = `SequenceNo`, not a hash.** Dense monotonic sequence numbers
  give trivial range replay, gap detection, and the same cursor idiom the
  durable bus already uses (versionstamp cursors). Hashes are for integrity
  and anchoring, not addressing.
- **Sync trait + writer task.** The trait is synchronous; the hot path never
  touches it. A spawned writer task consumes an mpsc channel — exactly the
  `DecisionRecord` tap pattern (non-blocking `try_send`, drop-with-warn,
  K3 gotcha 14 semantics): durability is at-least-once from the tee onward,
  and the graph-mutation/decision hot paths stay as fast as today.
- Typed adapters sit above the trait: `TopologyStore` (wire conversion +
  replay, §7), `DecisionStore` (causal parents, §11), and
  `spawn_store_writer` for the async seam.
- Default impl (P7.1): `FileEventStore` — segmented frame files
  (length-prefix + frame bytes), new segment per N records with tail-hash
  continuity across files, crash-tail recovery on open.

## 7. Replay and query integration (the #18 co-design)

**Decision.** One query path. The durable store does not grow its own query
API; it replays into the existing in-memory machinery:

```rust
/// Decode Topology-stream frames in [range] back into TemporalEvent and
/// append into a fresh in-memory EventLog. All existing consumers —
/// TemporalQueries, TemporalAnalytics, #18 magnitude_history — run unchanged.
fn replay_into_event_log<V, HE>(...) -> Result<EventLog<V, HE>, PersistenceError>;
```

Consequences:

- **#18 ships now, independent of Phase 7.** `magnitude_history` is a
  `TemporalAnalytics` query over `events_ref()` — it works against the
  live in-memory log today, and against a replayed log identically once
  P7.2 lands. The parity gate is pre-registered in §16 (P7.2 acceptance:
  the trajectory over a replayed log equals the in-memory series).
- Point-in-time reconstruction (`TemporalQueries::*_at`) plugs in the same
  way — replay then query. Registry-stream state (revocations, §9) is
  folded by the same pattern.
- `SnapshotMarker` events (already in the log) act as replay accelerators:
  `read_from` a snapshot's sequence position instead of genesis. Snapshots
  are appended records like everything else — no side-band state.

## 8. Sealing and crypto-deletion [R2]

Scope: the **Stroh §7.4 data-layer pattern only** — per-record encryption
with cryptographic-deletion finality. Stroh's I1 runtime tenant-isolation is
explicitly not adopted (pinned in the requirements payload).

- **Classification.** `Topology` stays plain: vertex/hyperedge indices and
  capability masks are anonymous structure. Sealed: `Beliefs` (inter-agent
  trust/compatibility is correlatable), identity-bearing decision fields,
  and any agent-identity binding (agent id ↔ vertex index ↔ real-world
  identity lives in its own sealed record kind, so replay of public
  structure never requires a key).
- **Key hierarchy.** Per-record DEK (data-encryption key), wrapped by a
  per-subject KEK (key-encryption key). **Deletion = destroy the subject's
  KEK** (+ `zeroize`): every record about that subject becomes
  mathematically unrecoverable in one operation; ciphertext untouched;
  **the chain survives** because it hashes ciphertext bytes (§3) — key
  destruction cannot break verification. This is where append-only [R1] and
  the right to disappear [R2] reconcile.
- **The key registry lives OUTSIDE the log** — a small mutable keystore
  (file/dir), precisely because keys must be destroyable and the log must
  not be. The *audit* of destruction (`KeyDestroyed { key_id, at }`) is
  appended to the `Registry` stream [R4].
- **Identity nonce [R7].** Correlatable identity records carry a random
  `correlation_salt` mixed into any derived identifier — distinct from the
  AEAD nonce — so the same agent in two exports does not correlate.
- **Upstream filter [R7].** A `PayloadFilter` trait applied in the writer
  path before anything is encoded: the sacred-never-enters rule [R7] becomes
  a seam with a default-deny hook, not a policy promise.
- **Candidate crates (named, NOT committed — implementing is not this
  issue):** `chacha20poly1305` (XChaCha20-Poly1305; 24-byte nonce, forgiving
  nonce management — recommended) + `zeroize`. Rejected: `aes-gcm`
  (nonce-fragile at this layer), `age` (file-granularity, wrong shape for
  per-record).

Trade: a second stateful artifact (the keystore) to back up and reason
about, and sealed records are opaque to ad-hoc inspection. Bought: the only
known reconciliation of [R1] and [R2].

## 9. Revocation registry stream [R4]

Revocation is an appended `Registry`-stream event, never a field mutation:

- `GrantIssued { grant_id, subject, scope, at }`
- `GrantRevoked { grant_id, at, reason }`
- `KeyDestroyed { key_id, at }` (the crypto-deletion audit, §8)
- `ManifestRevoked { manifest_id, at }` (federation, §10)

Read-path rule: reconstruction at time *t* folds the registry stream up to
*t* exactly the way `TemporalQueries` folds topology events — current
revocation state is first-class replayed state, and verifiers must consult
it before honoring anything it governs (grants, manifests, keys). No
registry record is ever sealed or revoked; it is the thing revocation is
made of.

## 10. Federation: bilateral manifest-gated segment exchange [R3]

Seam only (P7.5 implements):

```rust
/// The framed segment bytes (§3 framing) as an opaque newtype over Vec<u8>.
struct SegmentBytes(Vec<u8>);

fn export_segment(streams: &[(StreamId, RangeInclusive<SequenceNo>)])
    -> Result<(SegmentBytes, Manifest), PersistenceError>;

struct Manifest {
    manifest_id: String,
    issuer: String,                       // portable identifier [R5]
    ranges: Vec<(StreamId, SequenceNo, SequenceNo, RecordHash /* head */)>,
    registry_head: RecordHash,            // revocation state at export
    issued_at: u64,
    expires_at: Option<u64>,
    signature: Vec<u8>,                   // Ed25519, design-level
}
```

Import path: verify manifest signature → `verify` every covered chain range
→ check the manifest is not revoked (local `Registry` stream carries
`ManifestRevoked`) → land records under a **foreign namespace**
(`federated/<issuer>/<stream>`), never merged into local streams.

Properties: bilateral, file-shaped, offline-capable, revocable. Explicitly
NOT: the broadcast buses (local hot path), the libp2p gateway (local publish
boundary — K3 §8 rationale stands), any central registry, any network-wide
protocol. The exported `StreamHead` digest is what a counterparty's KERI
controller may anchor (§5); koalisi's involvement ends at emitting it.

## 11. Causal decision graphs [R6]

`parents: Vec<EventRef>` lives on the **envelope** (every record), so each
stream record is a node in a DAG across streams — structurally the EPP
`EffectLog` shape (a causal trace, not a flat log).

v1 semantics (deliberately coarse): the decision forwarder attaches **one
parent — the `Topology` stream head at decision time** ("the graph state
this decision saw"). `DecisionRecord` itself stays untouched: in-memory,
serde-free, hot-path (the K3 tap contract). Richer antecedent sets (the
specific membership/weight events that fed a decision, belief-snapshot
parents) fit the same schema later without a format break.

Cross-federation parent references are an open call (§17): `(stream, seq)`
is cheap and unambiguous locally; refs that cross a manifest boundary may
want content-addressing.

## 12. FAIR run provenance [R8]

The `Provenance` stream records run manifests: config digest, seed set, dep
tags (e.g. catgraph rev), output-pack digests — the material from which a
DOI-minted, multi-mirror output pack (Zenodo-CERN / AWS Open-Data / Internet
Archive / Software Heritage, per NEST framing) is assembled. The K4/K6 A/B
reports in `docs/` are the informal precursors; the stream gives them a
hash-chained home. koalisi's obligation ends at producing the pack contents
and digests; minting and mirroring are NEST-side processes.

## 13. Reserved: particle lineage [R9]

`StreamId::Lineage` and a reserved wire variant land with P7.1/P7.2 as
schema only. No producer exists until Phase 5
([#20](https://github.com/sustia-llc/koalisi/issues/20)) unholds
(post-2026-07-09 NEST ownership decision). The concrete consumer is NEST's
Stage-2 ensemble (~20 full solves anchoring ~80 surrogate runs), which
replays lineages of discovered coalition designs. Reserving the stream now
is what "the dynamics that generate the events settle before committing to
a durable format" bought — the envelope is settled; the payload waits.

## 14. Module layout, feature gating, dependency policy

New module `src/persistence/`, feature **`persistence`** (matches the
one-word convention: `decision`, `magnitude`, `durable`, `remote`):

```
src/persistence/
  mod.rs        // pub use surface, feature docs
  envelope.rs   // StreamId, SequenceNo, RecordHash, Payload, EventRef, Record, StoredRecord
  store.rs      // EventStore trait, FileEventStore
  wire.rs       // WireTopologyEvent, WireDecision, WireBelief, WireLineage (reserved)
  chain.rs      // framing, hashing, verify
  sealing.rs    // KeyId keystore, AEAD seam, PayloadFilter, correlation_salt
  registry.rs   // Registry-stream events + fold
  manifest.rs   // federation export/import seam
  replay.rs     // replay_into_event_log → topology::EventLog
  writer.rs     // spawn_store_writer (mpsc-fed task, tap-style non-blocking)
```

Feature relations: `persistence` is independent of `durable` (neither
depends on the other); together they enable a tee in the decision forwarder
(log = truth, bus = live projection). Belief capture compiles only under
`persistence` + `decision`. Deps gated behind `persistence` (none land
before P7.1): `ciborium`, `sha2`; later phases add `chacha20poly1305`,
`zeroize` (P7.3), `ed25519-dalek` (P7.5). Default features stay empty.

## 15. Compaction and retention stance

**The log is never rewritten.**

- Snapshots are ordinary appended records that *accelerate* replay (§7);
  they never replace history.
- Crypto-deletion shrinks *recoverable* data without touching bytes (§8).
- Segment rotation (new file per N records, tail-hash continuity) handles
  cold archival.
- A compacted log would break the chain [R1] — so there is no compaction.
  Ciphertext with destroyed keys stays on disk. If disk pressure ever forces
  whole-segment rewrite, that requires a re-anchoring `Registry` event and a
  design of its own — noted as open (§17), not solved here.

## 16. Phasing — follow-up implementation issues

Filed after sign-off of this doc; each is independently shippable and gated
by the standard suites + clippy.

| Phase | Scope | Acceptance |
|---|---|---|
| **P7.1** core chained log | envelope, streams, SHA-256 chain, `FileEventStore` (append/read_from/head/verify, segment rotation, crash-tail recovery), writer task, feature `persistence`. No crypto, no federation. | kill-and-restart: reopen, verify full chain, replay count intact |
| **P7.2** topology projection + replay | `WireTopologyEvent`, tee from the `TemporalHypergraph` append path, `replay_into_event_log` | **parity gate: `magnitude_history` (#18) over a replayed log == the in-memory series, seed-for-seed** |
| **P7.3** sealing + revocation registry | keystore, `Sealed` payloads, KEK destruction, `correlation_salt`, `PayloadFilter`, Registry events, reconstruction honoring revocation | sealed round-trip; key destruction ⇒ `KeyUnavailable`; chain still verifies |
| **P7.4** decision + belief streams | `WireDecision` with causal parent (forwarder tee), belief snapshot capture (`decision`), optional `durable` projection tee | decision DAG walkable; tee loses nothing the tap delivered |
| **P7.5** federation + provenance | signed manifest export/import, revocation check, foreign namespace; `Provenance` stream + run-manifest export | import of tampered/revoked segment rejected; exported head verifiable |

Lineage gets **no issue**: stream id + reserved wire variant ride P7.1/P7.2
as schema; the producer waits for #20.

## 17. Open questions (recorded, deliberately not decided here)

1. **KEK granularity for bilateral records.** Trust beliefs involve two
   subjects: does either party's deletion destroy the pair record, or is
   deletion consent bilateral? Options: per-agent KEK (either party can
   delete) vs per-pair KEK (both must). Needs tauhokohoko input.
2. **Ciphertext space reclamation.** Stance is never-rewrite (§15); a
   future segment-rewrite-plus-re-anchor design is possible but unspecified.
3. **SHA-256 vs BLAKE3.** v1 pins SHA-256 for archival ubiquity and KERI
   interop; the algorithm tag makes revisiting cheap.
4. **Cross-federation `EventRef` addressing.** `(stream, seq)` locally vs
   content-addressed hashes across manifest boundaries.
5. **`ciborium` vs `minicbor`.** serde convenience vs deterministic-encoding
   control without serde. Pick at P7.1 implementation time; the hash
   contract (§3) makes this non-load-bearing.

## 18. References

- Issue [#21](https://github.com/sustia-llc/koalisi/issues/21) — the
  requirements payload (2026-07-03 comment); issue
  [#18](https://github.com/sustia-llc/koalisi/issues/18) — trajectory query;
  [#20](https://github.com/sustia-llc/koalisi/issues/20) — Phase 5 (held);
  [#6](https://github.com/sustia-llc/koalisi/issues/6) — K3 durable bus;
  [#4](https://github.com/sustia-llc/koalisi/issues/4) — K1 backend swap.
- tauhokohoko `requirements/causal-context-architecture.md` R1/R5 (EffectLog
  compatibility; KEL ≅ event_store ≅ EffectLog composability — cited, not
  re-derived) and `deliverables/m2-onchain-governance/design-doc.md` §1
  (50-year portability), §3.0 (invariants 1–3), §4 (layer mapping, off-chain
  decision trace), §7.4 (Stroh crypto-deletion; I1 not adopted).
- NEST `requirements/nest-overview.md` (FAIR outputs, mirrors, Schmidt rule).
- Bui & Vigneaux 2025 (arXiv:2501.06662) §3.5 — coalition magnitude (the #18
  quantity); catgraph #22/#23 (`coalition_value`), #31 (`CoalitionEvaluator`
  — see CLAUDE.md gotcha 15 before touching), #33 (evaluator-cost evidence).
- CLAUDE.md gotchas 12 (catgraph index stability — why raw indices persist),
  13 (K3 seams), 14 (`durable` semantics), 15 (K6 evaluator contracts).
- RFC 8949 (CBOR) §4.2 (deterministic encoding).
