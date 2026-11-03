//! Policy-gated coalition joins → decision tap → durable SurrealDB bus →
//! consumer inbox prints the replayed decision log. (feature `durable`)
//!
//! End-to-end story:
//! 1. Boot a local SurrealDB container (via `surrealdb-live-message`'s sdb task).
//! 2. Stand up a `DurableDecisionBus` with a `producer` + a `decision_log` sink.
//! 3. Run a koalisi `CoalitionService` with a decision *tap*; a forwarder drains
//!    the tap and publishes each decision durably as the `producer`.
//! 4. Drive two policy-gated joins through the service; read the decision
//!    events back off the `decision_log` inbox and print them. The example
//!    returns an error unless the inbox delivers exactly those two decisions,
//!    each addressed to `decision_log`.
//!
//! The hot path (the service's `tokio::sync` seam) never touches the DB — only
//! the *decisions* are forwarded to the durable tier.
//!
//! # How to run
//! ```text
//! cargo run --features durable --example durable_decisions
//! ```
//! Requires a running Docker daemon (the library boots a SurrealDB container).

use std::fmt::{Display, Formatter};
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::time::{Duration, sleep, timeout};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use koalisi::algorithms::{AdditiveCalculator, AgentCapabilities};
use koalisi::core::Executor;
use koalisi::core::config::setup_logging;
use koalisi::decision::{DecisionContext, ThresholdPolicy};
use koalisi::subsystems::coalition_actor::CoalitionService;
use koalisi::subsystems::durable::{DecisionEvent, DurableDecisionBus, spawn_decision_forwarder};
use koalisi::topology::CoalitionManager;

use surrealdb_live_message::subsystems::sdb::{self, SurrealDBWrapper};

const PRODUCER: &str = "producer";
const DECISION_LOG: &str = "decision_log";
const STARTUP_TIMEOUT: Duration = Duration::from_secs(180);
const RECV_TIMEOUT: Duration = Duration::from_secs(10);

/// Domain agent — a valid graph vertex weight *and* an `AgentCapabilities`.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
struct Worker {
    id: usize,
    caps: u32,
    trust: u32,
}

impl Display for Worker {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Worker({})", self.id)
    }
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

#[tokio::main]
async fn main() -> Result<()> {
    let executor = Arc::new(Executor::new()?);
    setup_logging()?;

    let token = CancellationToken::new();
    let tracker = TaskTracker::new();

    // 1) Boot SurrealDB under a child token.
    let sdb_token = token.child_token();
    let mut sdb_handle = tracker.spawn(async move { sdb::sdb_task(sdb_token).await });
    tokio::select! {
        r = SurrealDBWrapper::wait_until_ready() => {
            r.context("SurrealDB ready signal failed")?;
        }
        r = &mut sdb_handle => {
            token.cancel();
            anyhow::bail!("sdb_task exited during startup: {r:?}");
        }
        _ = sleep(STARTUP_TIMEOUT) => {
            token.cancel();
            tracker.close();
            tracker.wait().await;
            anyhow::bail!("SurrealDB not ready within {STARTUP_TIMEOUT:?}");
        }
    }
    tracing::info!("SurrealDB ready.");

    // 2) Durable bus: a producer + a decision-log sink.
    let bus = DurableDecisionBus::new(vec![PRODUCER.to_string(), DECISION_LOG.to_string()])
        .await
        .context("durable decision bus")?;
    // Cancels the sdb token on every exit from here, an early return included.
    let _sdb_guard = token.clone().drop_guard();
    let producer = bus.agent(PRODUCER).await.context("producer agent")?;
    // Grab the consumer inbox BEFORE any decision so nothing is missed.
    let inbox = bus.coalition().inbox();

    // 3) koalisi coalition service with a decision tap + forwarder.
    let manager = CoalitionManager::<Worker, ()>::empty(executor);
    let seed = manager
        .add_agent(Worker {
            id: 2,
            caps: 0b010,
            trust: 90,
        })
        .await
        .context("add seed")?;
    let coalition = manager
        .form_coalition(vec![seed], ())
        .await
        .context("form")?;
    let c1 = manager
        .add_agent(Worker {
            id: 1,
            caps: 0b001,
            trust: 80,
        })
        .await
        .context("add c1")?;
    let c2 = manager
        .add_agent(Worker {
            id: 3,
            caps: 0b100,
            trust: 70,
        })
        .await
        .context("add c2")?;

    let (tap_tx, tap_rx) = tokio::sync::mpsc::channel(64);
    let service = CoalitionService::spawn_with_tap(
        manager,
        Box::new(ThresholdPolicy::new(AdditiveCalculator, 0.0, 0.0)),
        DecisionContext::default(),
        tap_tx,
        &tracker,
        token.child_token(),
    );
    let _forwarder = spawn_decision_forwarder(
        &bus,
        producer,
        DECISION_LOG.to_string(),
        tap_rx,
        &tracker,
        token.child_token(),
    )
    .await
    .context("decision forwarder")?;

    // 4) Drive policy-gated joins; each consulted decision is tapped + forwarded.
    let d1 = service.join(c1, coalition).await.context("join c1")?;
    let d2 = service.join(c2, coalition).await.context("join c2")?;
    println!(
        "drove 2 join decisions: c1.act={} c2.act={}",
        d1.act, d2.act
    );

    // Read the durable decision events back off the log inbox.
    let label = format!("coalition-{}", usize::from(coalition));
    let mut expected = vec![
        DecisionEvent {
            coalition: label.clone(),
            agent_id: usize::from(c1) as u64,
            kind: "join".to_string(),
            act: d1.act,
            score: d1.score,
        },
        DecisionEvent {
            coalition: label,
            agent_id: usize::from(c2) as u64,
            kind: "join".to_string(),
            act: d2.act,
            score: d2.score,
        },
    ];
    println!("--- replayed decision log (durable tier) ---");
    let outcome: Result<Vec<DecisionEvent>> = async {
        let mut received = Vec::with_capacity(expected.len());
        for i in 0..expected.len() {
            let d = timeout(RECV_TIMEOUT, inbox.recv())
                .await
                .with_context(|| {
                    format!(
                        "timed out waiting for durable decision event {} of {}",
                        i + 1,
                        expected.len()
                    )
                })?
                .context("inbox bus closed before every decision event arrived")?;
            let e = d.message.payload;
            println!(
                "  [{}] {} agent_id={} act={} score={:.4}",
                d.recipient, e.kind, e.agent_id, e.act, e.score
            );
            if d.recipient != DECISION_LOG {
                anyhow::bail!(
                    "decision event for agent_id={} addressed to '{}', not '{DECISION_LOG}'",
                    e.agent_id,
                    d.recipient
                );
            }
            received.push(e);
        }
        Ok(received)
    }
    .await;

    // Shutdown on every outcome: drop the service (its tap closes → forwarder
    // drains + exits), then the bus, then the sdb token; drain everything.
    drop(service);
    bus.shutdown().await;
    token.cancel();
    tracker.close();
    tracker.wait().await;

    let mut received = outcome?;
    if let Ok(extra) = inbox.try_recv() {
        anyhow::bail!(
            "unexpected extra decision event to '{}': {:?}",
            extra.recipient,
            extra.message.payload
        );
    }
    expected.sort_by_key(|e| e.agent_id);
    received.sort_by_key(|e| e.agent_id);
    if received != expected {
        anyhow::bail!("decision events {received:?}, expected {expected:?}");
    }
    println!("received the 2 expected decisions; stopped cleanly");
    Ok(())
}
