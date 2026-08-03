//! Domain-neutral remote coalition-event gateway, end to end in one process
//! (issue #38, feature `remote`).
//!
//! The story:
//! 1. A `CoalitionManager` + policy-gated `CoalitionService` with a decision
//!    *tap* — the same seam every other koalisi demo drives.
//! 2. `spawn_decision_tee` fans the tap out; one leg feeds the gateway (a
//!    second leg would be e.g. the `durable` decision log).
//! 3. `enable_remote_gateway` publishes those decisions over raw libp2p
//!    `request-response` on loopback (mDNS off, for determinism).
//! 4. A `RemoteCoalitionClient` dials the bound address, asks `head`, then
//!    polls the events in TWO batches with a cursor — showing the non-draining,
//!    resumable read model.
//!
//! Deterministic, no network beyond loopback, finishes in a couple of seconds.
//!
//! # How to run
//! ```text
//! cargo run --features remote --example remote_coalition_consumer
//! ```

use std::fmt::{Display, Formatter};
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use koalisi::algorithms::{AdditiveCalculator, AgentCapabilities};
use koalisi::core::config::setup_logging;
use koalisi::decision::{DecisionContext, ThresholdPolicy};
use koalisi::subsystems::coalition_actor::{CoalitionService, spawn_decision_tee};
use koalisi::subsystems::remote::{RemoteCoalitionClient, RemoteConfig, enable_remote_gateway};
use koalisi::topology::CoalitionManager;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

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

    let tracker = TaskTracker::new();
    let root = CancellationToken::new();

    // =====================================================================
    // 1. Coalition + policy-gated service with a decision tap.
    // =====================================================================
    let manager = CoalitionManager::<Worker, ()>::empty();
    let seed = manager
        .add_agent(Worker { id: 0, caps: 0b0001, trust: 90 })
        .await
        .context("add seed")?;
    let coalition = manager
        .form_coalition(vec![seed], ())
        .await
        .context("form coalition")?;

    // Three candidates: two cover fresh capability bits, one is a redundant
    // clone of the seed. All three are *consulted*, so all three are published
    // — a declined decision is as interesting to a remote observer as an
    // accepted one.
    let mut candidates = Vec::new();
    for (id, caps) in [(1usize, 0b0010u32), (2, 0b0100), (3, 0b0001)] {
        candidates.push(
            manager
                .add_agent(Worker { id, caps, trust: 80 })
                .await
                .context("add candidate")?,
        );
    }

    let (tap_tx, tap_rx) = mpsc::channel(64);
    let service = CoalitionService::spawn_with_tap(
        manager,
        Box::new(ThresholdPolicy::new(AdditiveCalculator, 0.0, 0.0)),
        DecisionContext {
            required_capabilities: 0b0111,
        },
        tap_tx,
    );

    // =====================================================================
    // 2. Tee the tap; one leg feeds the gateway.
    // =====================================================================
    let (gw_tx, gw_rx) = mpsc::channel(64);
    let _tee = spawn_decision_tee(tap_rx, vec![gw_tx], &tracker, root.child_token());

    // =====================================================================
    // 3. Gateway on loopback (mDNS off — deterministic, no LAN discovery).
    // =====================================================================
    let handle = enable_remote_gateway(
        &tracker,
        root.clone(),
        gw_rx,
        RemoteConfig {
            listen_addr: "/ip4/127.0.0.1/tcp/0".into(),
            enable_mdns: false,
            buffer_cap: 64,
        },
    )
    .await
    .context("enable remote gateway")?;
    let gateway_addr = handle
        .listen_addrs
        .first()
        .context("gateway bound no listen address")?
        .clone();
    println!("gateway peer={} addr={gateway_addr}", handle.local_peer_id);

    // =====================================================================
    // 4. Drive policy-gated decisions (each one is tapped → teed → published).
    // =====================================================================
    for (i, candidate) in candidates.iter().enumerate() {
        let d = service
            .join(*candidate, coalition)
            .await
            .context("join candidate")?;
        println!("  join candidate {i}: act={} score={:.3}", d.act, d.score);
    }
    let d = service
        .leave(seed, coalition)
        .await
        .context("leave seed")?;
    println!("  leave seed: act={} score={:.3}", d.act, d.score);

    // =====================================================================
    // 5. Remote consumer: dial, head, then poll in two cursor batches.
    // =====================================================================
    let mut client = RemoteCoalitionClient::connect(RemoteConfig {
        listen_addr: "/ip4/127.0.0.1/tcp/0".into(),
        enable_mdns: false,
        buffer_cap: 0, // unused on the client side
    })
    .await
    .context("client connect")?;
    let peer = client
        .dial(gateway_addr, REQUEST_TIMEOUT)
        .await
        .context("dial gateway")?;

    // The tap → tee → gateway path is asynchronous; wait (bounded) for the four
    // decisions to land rather than sleeping a magic number.
    let mut head_seq = 0;
    let mut buffered = 0;
    for _ in 0..100 {
        let (h, len) = client.head(peer, REQUEST_TIMEOUT).await.context("head")?;
        head_seq = h;
        buffered = len;
        if h >= 4 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    println!("\nremote head: head_seq={head_seq} buffered={buffered}");

    // Batch 1: everything up to seq 2. Batch 2: the rest, resuming from the
    // cursor — the gateway never drains, so the cursor lives with the consumer.
    let (_, first) = client
        .poll_since(peer, 0, REQUEST_TIMEOUT)
        .await
        .context("poll batch 1")?;
    let batch1: Vec<_> = first.iter().take(2).collect();
    println!("--- batch 1 (from cursor 0, first 2) ---");
    for e in &batch1 {
        println!(
            "  seq={} {} agent_id={} act={} score={:.3} coalition={}",
            e.seq, e.kind, e.agent_id, e.act, e.score, e.coalition
        );
    }
    let cursor = batch1.last().map_or(0, |e| e.seq);

    let (head_after, second) = client
        .poll_since(peer, cursor, REQUEST_TIMEOUT)
        .await
        .context("poll batch 2")?;
    println!("--- batch 2 (from cursor {cursor}) ---");
    for e in &second {
        println!(
            "  seq={} {} agent_id={} act={} score={:.3} coalition={}",
            e.seq, e.kind, e.agent_id, e.act, e.score, e.coalition
        );
    }
    println!("gateway head_seq={head_after} (cursor {cursor} + {} events)", second.len());

    // =====================================================================
    // 6. Shutdown — prompt teardown: cancel → close → drain.
    //
    // (The lossless alternative is: drop the service, let the tee and gateway
    // drain to `None`, then wait. Pick ONE discipline; don't mix.)
    // =====================================================================
    drop(service);
    drop(client);
    root.cancel();
    tracker.close();
    tracker.wait().await;
    println!("\nstopped cleanly");
    Ok(())
}
