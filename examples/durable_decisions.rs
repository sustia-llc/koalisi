//! Policy-gated coalition joins → decision tap → durable SurrealDB bus →
//! consumer inbox prints the replayed decision log. (feature `durable`)
//!
//! End-to-end story:
//! 1. Boot a local SurrealDB container (via `surrealdb-live-message`'s sdb task).
//! 2. Stand up a `DurableDecisionBus` with a `producer` + a `decision_log` sink.
//! 3. Run a koalisi `CoalitionService` with a decision *tap*; a forwarder drains
//!    the tap and publishes each decision durably as the `producer`.
//! 4. Drive a few policy-gated joins through the service; read the decision
//!    events back off the `decision_log` inbox and print them.
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

use anyhow::{Context, Result};
use tokio::time::{Duration, sleep, timeout};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use koalisi::algorithms::{AdditiveCalculator, AgentCapabilities};
use koalisi::core::config::setup_logging;
use koalisi::decision::{DecisionContext, ThresholdPolicy};
use koalisi::subsystems::coalition_actor::CoalitionService;
use koalisi::subsystems::durable::{DurableDecisionBus, spawn_decision_forwarder};
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
    setup_logging();

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
    let producer = bus.agent(PRODUCER).await.context("producer agent")?;
    // Grab the consumer inbox BEFORE any decision so nothing is missed.
    let inbox = bus.coalition().inbox();

    // 3) koalisi coalition service with a decision tap + forwarder.
    let manager = CoalitionManager::<Worker, ()>::empty();
    let seed = manager
        .add_agent(Worker { id: 2, caps: 0b010, trust: 90 })
        .await
        .context("add seed")?;
    let coalition = manager.form_coalition(vec![seed], ()).await.context("form")?;
    let c1 = manager
        .add_agent(Worker { id: 1, caps: 0b001, trust: 80 })
        .await
        .context("add c1")?;
    let c2 = manager
        .add_agent(Worker { id: 3, caps: 0b100, trust: 70 })
        .await
        .context("add c2")?;

    let (tap_tx, tap_rx) = tokio::sync::mpsc::channel(64);
    let service = CoalitionService::spawn_with_tap(
        manager,
        Box::new(ThresholdPolicy::new(AdditiveCalculator, 0.0, 0.0)),
        DecisionContext::default(),
        tap_tx,
    );
    let _forwarder = spawn_decision_forwarder(
        producer,
        DECISION_LOG.to_string(),
        tap_rx,
        &tracker,
        token.child_token(),
    );

    // 4) Drive policy-gated joins; each consulted decision is tapped + forwarded.
    let d1 = service.join(c1, coalition).await.context("join c1")?;
    let d2 = service.join(c2, coalition).await.context("join c2")?;
    println!("drove 2 join decisions: c1.act={} c2.act={}", d1.act, d2.act);

    // Read the durable decision events back off the log inbox.
    println!("--- replayed decision log (durable tier) ---");
    for _ in 0..2 {
        match timeout(RECV_TIMEOUT, inbox.recv()).await {
            Ok(Ok(d)) => {
                let e = &d.message.payload;
                println!(
                    "  [{}] {} agent_id={} act={} score={:.4}",
                    d.recipient, e.kind, e.agent_id, e.act, e.score
                );
            }
            Ok(Err(_)) => {
                println!("  bus closed before all events arrived");
                break;
            }
            Err(_) => {
                println!("  timed out waiting for a durable decision event");
                break;
            }
        }
    }

    // Shutdown: drop the service (its tap closes → forwarder drains + exits),
    // then the bus, then the sdb token; drain everything.
    drop(service);
    bus.shutdown().await;
    token.cancel();
    tracker.close();
    tracker.wait().await;
    println!("stopped cleanly");
    Ok(())
}
