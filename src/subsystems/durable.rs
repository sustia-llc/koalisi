//! Durable coalition decision-event bus (feature `durable`).
//!
//! This is the SurrealDB-backed *membership/decision-messaging tier* for issue
//! #6. It sits entirely off the hot path: the latency-sensitive coalition seams
//! stay on `tokio::sync`, and only join/leave *decisions* — tapped
//! from [`CoalitionService`](crate::subsystems::coalition_actor::CoalitionService)
//! via its optional [`DecisionRecord`] tap — are forwarded into a durable,
//! restart-replayable log.
//!
//! ## Two-tier durable bus (upstream)
//!
//! Built on `surrealdb-live-message`'s [`Coalition<T>`]: a `message` edge table
//! with a `CHANGEFEED` is the durable log (tier 1); a `LIVE SELECT` is a
//! low-latency wake-up (tier 2) that triggers a `SHOW CHANGES … SINCE <cursor>`
//! catch-up, which is the only delivery path. Each agent persists a cursor (a
//! versionstamp and a change index). A decision published while the consumer
//! is down is delivered after it restarts if the message is still in the log.
//! While a bus runs, upstream's retention sweep deletes the messages older
//! than `sdb.message_retention_secs`, once every `max(retention / 4, 60 s)`
//! (`surrealdb-live-message` v0.7.0, `src/subsystems/agents.rs:1599-1630`), so
//! a consumer that restarts later than that window after a message was sent
//! can miss it.
//!
//! Delivery guarantees, per hop:
//! - From the decision tap to the log, delivery is at-most-once. A full or
//!   closed tap drops the record with a warn-level log. Each record gets one
//!   [`Agent::send`], with
//!   no retry; an error from it is logged at error level, and the decision
//!   may be missing from the log.
//! - From the log to the upstream delivery bus ([`Coalition::inbox`]),
//!   delivery is at-least-once for a message still in the log, i.e. sent
//!   within `sdb.message_retention_secs`. Upstream logs a record whose payload does not
//!   decode as [`DecisionEvent`] at error level, and skips it.
//! - If the process exits before a receiver reads a delivery that is already
//!   on the bus, that delivery is lost.
//!
//! ## Membership is recorded IN the stream, not as dynamic agents
//!
//! Upstream [`Coalition::new`] takes a FIXED set of agent names. koalisi's
//! coalition membership is *dynamic*, so it is NOT modeled as dynamic upstream
//! agents — instead the decision **event log** is the membership record
//! (consistent with koalisi's event-sourcing). This bus is created over a small,
//! fixed set of process-level agents: a `producer` that publishes decision events
//! and a `log` sink that consumes them.
//!
//! ## Addressing shape
//!
//! The simplest faithful shape: a single designated `producer` agent
//! [`Agent::send`]s each [`DecisionEvent`] to a single designated `log` agent.
//! The `log` agent's inbox is the replayable decision stream. (Upstream `send`
//! addresses exactly one recipient; a fan-out to many log agents would be a
//! trivial extension but adds nothing for a single durable record.)
//!
//! ## With feature `persistence`
//!
//! `spawn_decision_log_bus_tee` drains a decision *trace* tap and, per trace,
//! writes the `Decisions` stream record of the `persistence` log first and
//! publishes the [`DecisionEvent`] on this bus second.
//!
//! ## Settings resolution (downstream gotcha)
//!
//! `surrealdb-live-message` owns a process-global `SETTINGS` (`LazyLock`) that
//! loads `config/default.toml` (+ `config/{RUN_MODE}.toml` + `__`-separated env
//! overrides) from the **current working directory** via the `config` crate. In
//! a downstream crate that cwd is koalisi's root, so the upstream `Settings`
//! deserializes koalisi's `config/default.toml`. koalisi therefore ships the
//! `[sdb]` + `[docker]` sections that upstream requires (host/port/image/tag/
//! container_name/namespace/database/username/password/endpoint). koalisi's own
//! `Settings` ignores those unknown sections. Nothing here needs to touch that
//! global — booting the DB via [`sdb::sdb_task`](surrealdb_live_message::subsystems::sdb::sdb_task)
//! and constructing a [`DurableDecisionBus`] reads it transparently.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use surrealdb_types::SurrealValue;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use surrealdb_live_message::error::chain;
use surrealdb_live_message::subsystems::agents::{Agent, Coalition};

#[cfg(feature = "persistence")]
use crate::persistence::{Record, StreamId, forward_decision_trace, next_trace};
use crate::subsystems::coalition_actor::DecisionRecord;
#[cfg(feature = "persistence")]
use crate::subsystems::coalition_actor::DecisionTrace;

/// Durable, serializable mirror of a core
/// [`crate::subsystems::coalition_actor::DecisionRecord`].
///
/// Derives [`SurrealValue`] so it can be the payload `T` of an upstream
/// [`Coalition<T>`]. Field types are deliberately plain scalars/strings — no
/// `Option<RecordId>` or raw-identifier fields — sidestepping the upstream
/// `SurrealValue`-derive wire-key gotchas (raw-ident keys, `in`/`out`
/// projection) that only bite edge-record shapes.
// Pin the derive's crate path: under the `durable` feature graph the surrealdb
// SDK is present, so surrealdb-types-derive's `sdk-path` feature is unified on
// and the derive would otherwise emit `::surrealdb::types` (which koalisi does
// not dep). Point it at our direct `surrealdb-types` dep instead — the same
// (unified) crate `surrealdb::types` re-exports, so the impl still satisfies the
// upstream `Coalition<T>: SurrealValue` bound.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, SurrealValue)]
#[surreal(crate = "::surrealdb_types")]
pub struct DecisionEvent {
    /// Stable coalition label (`"coalition-<n>"`).
    pub coalition: String,
    /// Topology vertex index of the agent the decision was about.
    pub agent_id: u64,
    /// `"join"` or `"leave"`.
    pub kind: String,
    /// Whether the policy recommended acting (joining / leaving).
    pub act: bool,
    /// Underlying scalar the decision was made from.
    pub score: f64,
}

impl From<DecisionRecord> for DecisionEvent {
    fn from(r: DecisionRecord) -> Self {
        Self {
            coalition: r.coalition,
            // usize → u64: a topology vertex index always fits.
            agent_id: r.agent_id as u64,
            kind: r.kind.as_str().to_string(),
            act: r.act,
            score: r.score,
        }
    }
}

/// A thin wrapper owning an upstream [`Coalition<DecisionEvent>`] — the durable
/// tier's process-level agents.
///
/// Construct with a fixed set of agent names (typically a `producer` and a
/// `log` sink). Dynamic coalition membership is recorded in the event stream,
/// not as dynamic agents here (see the [module docs](self)).
pub struct DurableDecisionBus {
    coalition: Coalition<DecisionEvent>,
}

impl DurableDecisionBus {
    /// Create the durable bus over `names`, spawning each agent's listen loop.
    ///
    /// Requires a reachable SurrealDB (boot it via
    /// [`sdb::sdb_task`](surrealdb_live_message::subsystems::sdb::sdb_task)
    /// first). Returns once every agent's LIVE subscription is registered, so a
    /// send issued immediately after cannot race the subscription.
    pub async fn new(names: Vec<String>) -> Result<Self> {
        let coalition = Coalition::<DecisionEvent>::new(names)
            .await
            .context("create durable decision coalition")?;
        Ok(Self { coalition })
    }

    /// The underlying upstream coalition — reach `inbox()` / `agent()` /
    /// `cancellation_token()` through it.
    #[must_use]
    pub fn coalition(&self) -> &Coalition<DecisionEvent> {
        &self.coalition
    }

    /// Look up one of the bus's agents by name (e.g. the `producer`).
    pub async fn agent(&self, name: &str) -> Option<Agent> {
        self.coalition.agent(name).await
    }

    /// Cancel the bus's agents and wait until their listen loops and the
    /// retention sweep end ([`Coalition::shutdown`]). Dropping the bus also
    /// cancels them, but does not wait.
    ///
    /// After the cancel, each listen loop makes at most one cursor save and
    /// waits up to 2 s for it, then waits up to 2 s for its live-query `KILL`.
    /// A subscribe in flight at the cancel is finished by a task that this does
    /// not wait for, so its `KILL` can run after this returns.
    ///
    /// Agent records and cursors persist. A later [`DurableDecisionBus::new`]
    /// over the same names delivers the messages after each persisted cursor
    /// that are still within `sdb.message_retention_secs`. Deliveries on the bus
    /// that no receiver read are lost.
    pub async fn shutdown(&self) {
        self.coalition.shutdown().await;
    }
}

/// Errors from spawning a durable decision task.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DurableError {
    /// The producer's name (`Agent::name`) names no agent of the
    /// [`DurableDecisionBus`].
    #[error("producer '{producer}' is not an agent of the durable decision bus")]
    UnknownProducer {
        /// The name of the agent that was passed as the producer.
        producer: String,
    },
    /// `log_sink` names no agent of the [`DurableDecisionBus`].
    #[error("log sink '{log_sink}' is not an agent of the durable decision bus")]
    UnknownLogSink {
        /// The name that was passed as the log sink.
        log_sink: String,
    },
}

/// `Ok` iff `producer`'s name and `log_sink` each name an agent of `bus`;
/// the producer is checked first.
async fn check_agents(
    bus: &DurableDecisionBus,
    producer: &Agent,
    log_sink: &str,
) -> Result<(), DurableError> {
    if bus.agent(&producer.name).await.is_none() {
        return Err(DurableError::UnknownProducer {
            producer: producer.name.clone(),
        });
    }
    if bus.agent(log_sink).await.is_none() {
        return Err(DurableError::UnknownLogSink {
            log_sink: log_sink.to_string(),
        });
    }
    Ok(())
}

/// Spawn a task that drains the core decision tap and publishes each record to
/// the durable log.
///
/// `producer` is one of `bus`'s agents; `log_sink` is the name of the `bus`
/// agent whose inbox is the durable decision stream. Each [`DecisionRecord`]
/// pulled from `rx` is converted to a [`DecisionEvent`] and sent via
/// [`Agent::send`]. A send failure is logged at error level with the decision's
/// coalition, agent id and kind; the decision may be missing from the log. The
/// forwarder keeps running.
///
/// The task ends when either `token` cancels or `rx` closes (the core service —
/// and thus its tap `Sender` — has been dropped; buffered records are drained
/// first). Spawned on `tracker` so the caller's cancel→close→drain shutdown
/// covers it.
///
/// # Errors
///
/// Nothing is spawned on either error:
/// - [`DurableError::UnknownProducer`] when `producer`'s name names no agent
///   of `bus`;
/// - [`DurableError::UnknownLogSink`] when `producer` is an agent of `bus` and
///   `log_sink` names no agent of `bus`.
pub async fn spawn_decision_forwarder(
    bus: &DurableDecisionBus,
    producer: Agent,
    log_sink: String,
    mut rx: mpsc::Receiver<DecisionRecord>,
    tracker: &TaskTracker,
    token: CancellationToken,
) -> Result<JoinHandle<()>, DurableError> {
    check_agents(bus, &producer, &log_sink).await?;
    Ok(tracker.spawn(async move {
        loop {
            tokio::select! {
                biased;
                _ = token.cancelled() => break,
                maybe = rx.recv() => match maybe {
                    Some(record) => send_event(&producer, &log_sink, record).await,
                    None => break, // core tap dropped; nothing more to forward
                },
            }
        }
        tracing::debug!("decision forwarder stopped");
    }))
}

/// Send `record`'s [`DecisionEvent`] from `producer` to `log_sink`. A failure
/// is logged at error level with the record's coalition, agent id and kind.
async fn send_event(producer: &Agent, log_sink: &str, record: DecisionRecord) {
    let coalition = record.coalition.clone();
    let agent_id = record.agent_id;
    let kind = record.kind.as_str();
    if let Err(e) = producer.send(log_sink, DecisionEvent::from(record)).await {
        tracing::error!(
            coalition = %coalition,
            agent_id,
            kind,
            to = %log_sink,
            err = %chain(&e),
            "durable decision forward failed; the decision may be missing from the log"
        );
    }
}

/// Log one trace to the `Decisions` stream, then publish its record to the bus.
#[cfg(feature = "persistence")]
async fn tee_one(
    trace: DecisionTrace,
    writer_tx: &mpsc::Sender<(StreamId, Record)>,
    producer: &Agent,
    log_sink: &str,
) {
    forward_decision_trace(&trace, writer_tx).await;
    send_event(producer, log_sink, trace.record).await;
}

/// Spawn a task that drains a decision trace tap
/// ([`CoalitionService::spawn_with_trace_tap`](crate::subsystems::coalition_actor::CoalitionService::spawn_with_trace_tap))
/// into BOTH the persistence log and the durable bus (features `persistence` +
/// `durable`).
///
/// Per trace, in order: the `Decisions` stream record is built and sent on
/// `writer_tx` exactly as
/// [`spawn_decision_store_forwarder`](crate::persistence::spawn_decision_store_forwarder)
/// does (same record, same `send().await` wait for capacity, same drop with an
/// error-level log on an encode failure and a warn-level log on a closed
/// writer channel); then
/// `producer` (one of `bus`'s agents) [`Agent::send`]s the trace's
/// [`DecisionEvent`] to `log_sink`. A bus send failure is logged at error level
/// with the decision's coalition, agent id and kind; the decision may be
/// missing from the bus log.
///
/// Ends when either `token` is cancelled or `rx` closes. On cancellation it
/// first tees whatever is already buffered in `rx`, then stops. Spawned on
/// `tracker` so the caller's shutdown covers it; the returned [`JoinHandle`]
/// may be dropped.
///
/// # Errors
///
/// Nothing is spawned on either error:
/// - [`DurableError::UnknownProducer`] when `producer`'s name names no agent
///   of `bus`;
/// - [`DurableError::UnknownLogSink`] when `producer` is an agent of `bus` and
///   `log_sink` names no agent of `bus`.
#[cfg(feature = "persistence")]
pub async fn spawn_decision_log_bus_tee(
    bus: &DurableDecisionBus,
    mut rx: mpsc::Receiver<DecisionTrace>,
    writer_tx: mpsc::Sender<(StreamId, Record)>,
    producer: Agent,
    log_sink: String,
    tracker: &TaskTracker,
    token: CancellationToken,
) -> Result<JoinHandle<()>, DurableError> {
    check_agents(bus, &producer, &log_sink).await?;
    Ok(tracker.spawn(async move {
        while let Some(trace) = next_trace(&mut rx, &token).await {
            tee_one(trace, &writer_tx, &producer, &log_sink).await;
        }
        tracing::debug!("decision log/bus tee stopped");
    }))
}
