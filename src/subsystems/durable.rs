//! Durable coalition decision-event bus (feature `durable`).
//!
//! This is the SurrealDB-backed *membership/decision-messaging tier* for issue
//! #6. It sits entirely off the hot path: the latency-sensitive coalition seams
//! stay on `tokio::sync`, and only policy-consulted join/leave *decisions* — tapped
//! from [`CoalitionService`](crate::subsystems::coalition_actor::CoalitionService)
//! via its optional [`DecisionRecord`] tap — are forwarded into a durable,
//! restart-replayable log.
//!
//! ## Two-tier durable bus (upstream)
//!
//! Built on `surrealdb-live-message`'s [`Coalition<T>`]: a `message` edge table
//! with a `CHANGEFEED` is the durable log (tier 1); a `LIVE SELECT` is a
//! low-latency wake-up (tier 2) that triggers a `SHOW CHANGES … SINCE <cursor>`
//! catch-up — the sole delivery path. Each consumer persists a per-agent
//! versionstamp cursor, giving at-least-once delivery that survives a
//! disconnect/restart (a decision published while a consumer is down is replayed
//! on restart — the property plain at-most-once `LIVE` lacks).
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

use surrealdb_live_message::subsystems::agents::{Agent, Coalition};

use crate::subsystems::coalition_actor::DecisionRecord;

/// Durable, serializable mirror of a core
/// [`DecisionRecord`](crate::subsystems::coalition_actor::DecisionRecord).
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

    /// Three-step shutdown (cancel → close → drain) of the bus's agents.
    ///
    /// Durable state (messages, cursors, agent records) persists — a later
    /// [`DurableDecisionBus::new`] over the same names resumes and replays.
    pub async fn shutdown(&self) {
        self.coalition.shutdown().await;
    }
}

/// Spawn a task that drains the core decision tap and publishes each record to
/// the durable log.
///
/// `producer` is one of the bus's agents; `log_sink` is the name of the bus
/// agent whose inbox is the durable decision stream. Each [`DecisionRecord`]
/// pulled from `rx` is converted to a [`DecisionEvent`] and sent via
/// [`Agent::send`]. A send failure is logged and skipped — the forwarder never
/// dies on a transient DB error.
///
/// The task ends when either `token` cancels or `rx` closes (the core service —
/// and thus its tap `Sender` — has been dropped; buffered records are drained
/// first). Spawned on `tracker` so the caller's cancel→close→drain shutdown
/// covers it.
pub fn spawn_decision_forwarder(
    producer: Agent,
    log_sink: String,
    mut rx: mpsc::Receiver<DecisionRecord>,
    tracker: &TaskTracker,
    token: CancellationToken,
) -> JoinHandle<()> {
    tracker.spawn(async move {
        loop {
            tokio::select! {
                biased;
                _ = token.cancelled() => break,
                maybe = rx.recv() => match maybe {
                    Some(record) => {
                        let event = DecisionEvent::from(record);
                        if let Err(e) = producer.send(&log_sink, event).await {
                            tracing::warn!("durable decision forward failed: {e}");
                        }
                    }
                    None => break, // core tap dropped; nothing more to forward
                },
            }
        }
        tracing::debug!("decision forwarder stopped");
    })
}
