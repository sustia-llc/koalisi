//! Integration test for the domain-neutral remote coalition-event gateway
//! (issue #38, feature `remote`).
//!
//! Verifies the whole publish path end-to-end across two independent libp2p
//! swarms in one process:
//!
//! 1. Stand up a real [`CoalitionService`] with a `DecisionRecord` tap.
//! 2. Route the tap through `spawn_decision_tee` into the gateway's receiver —
//!    so the fan-out seam, not just the gateway, is on the tested path.
//! 3. `enable_remote_gateway` listens on loopback (mDNS OFF; the handle hands
//!    back the bound address for an explicit dial).
//! 4. Drive policy-gated join/leave decisions through the service.
//! 5. A `RemoteCoalitionClient` dials the bound address and asks `Head` /
//!    `PollSince` — each goes through the CBOR request-response protocol
//!    (serialise → wire → deserialise → handle → serialise reply → wire →
//!    deserialise).
//!
//! Cap eviction is asserted at the `EventBuffer` level in the module unit tests
//! (`src/subsystems/remote.rs`), deliberately NOT over the wire — this test
//! stays a single round-trip.
//!
//! Every wait is bounded; the test cannot hang.

#![cfg(feature = "remote")]

use std::fmt::{Display, Formatter};
use std::time::Duration;

use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use koalisi::algorithms::{AdditiveCalculator, AgentCapabilities};
use koalisi::decision::{DecisionContext, ThresholdPolicy};
use koalisi::subsystems::coalition_actor::{CoalitionService, spawn_decision_tee};
use koalisi::subsystems::remote::{
    PeerId, REMOTE_WIRE_SCHEMA_VERSION, RemoteCoalitionClient, RemoteConfig, enable_remote_gateway,
};
use koalisi::topology::CoalitionManager;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

/// A vertex weight that is both a valid graph weight and an agent.
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

/// Poll the gateway's `head` until it reports `want` events, bounded by
/// `deadline`. The tap → tee → gateway path is asynchronous, so the count
/// settles a few scheduler ticks after the last decision returns.
async fn await_head(
    client: &mut RemoteCoalitionClient,
    peer: PeerId,
    want: u64,
    deadline: Duration,
) -> (u64, u64) {
    let started = std::time::Instant::now();
    loop {
        let (head_seq, len) = client
            .head(peer, REQUEST_TIMEOUT)
            .await
            .expect("remote Head");
        if head_seq >= want || started.elapsed() >= deadline {
            return (head_seq, len);
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn remote_gateway_publishes_coalition_decisions() {
    // Bound the whole test so a libp2p hang can't lock CI.
    timeout(Duration::from_secs(30), async {
        let tracker = TaskTracker::new();
        let root = CancellationToken::new();

        // ---------- Producer: manager + service with a decision tap ----------
        let manager = CoalitionManager::<Worker, ()>::empty();
        let seed = manager
            .add_agent(Worker { id: 2, caps: 0b010, trust: 90 })
            .await
            .expect("add seed");
        let coalition = manager.form_coalition(vec![seed], ()).await.expect("form");
        let c1 = manager
            .add_agent(Worker { id: 1, caps: 0b001, trust: 80 })
            .await
            .expect("add c1");
        let c2 = manager
            .add_agent(Worker { id: 3, caps: 0b100, trust: 70 })
            .await
            .expect("add c2");

        let (tap_tx, tap_rx) = tokio::sync::mpsc::channel(64);
        let service = CoalitionService::spawn_with_tap(
            manager,
            Box::new(ThresholdPolicy::new(AdditiveCalculator, 0.0, 0.0)),
            DecisionContext {
                required_capabilities: 0b111,
            },
            tap_tx,
        );

        // ---------- Tee: tap → (one leg) → gateway ----------
        let (gw_tx, gw_rx) = tokio::sync::mpsc::channel(64);
        let _tee = spawn_decision_tee(tap_rx, vec![gw_tx], &tracker, root.child_token());

        // ---------- Gateway on loopback, mDNS off ----------
        let handle = enable_remote_gateway(
            &tracker,
            root.clone(),
            gw_rx,
            RemoteConfig {
                listen_addr: "/ip4/127.0.0.1/tcp/0".into(),
                enable_mdns: false, // dial explicitly over loopback
                buffer_cap: 64,
            },
        )
        .await
        .expect("enable_remote_gateway");
        let gateway_addr = handle
            .listen_addrs
            .first()
            .expect("gateway should have a bound listen address")
            .clone();

        // ---------- Drive policy-gated decisions ----------
        // join_threshold 0.0 ⇒ positive marginals join; leave_threshold 0.0 ⇒ a
        // contributing member declines to leave. Both are *consulted* decisions
        // and both are tapped, so the decline shows up on the wire too.
        let d1 = service.join(c1, coalition).await.expect("join c1");
        let d2 = service.join(c2, coalition).await.expect("join c2");
        let d3 = service.leave(seed, coalition).await.expect("leave seed");
        assert!(d1.act && d2.act, "positive marginals ⇒ join");
        assert!(!d3.act, "contributing member does not leave at threshold 0");

        // ---------- Consumer: dial + wire round-trips ----------
        let mut client = RemoteCoalitionClient::connect(RemoteConfig {
            listen_addr: "/ip4/127.0.0.1/tcp/0".into(),
            enable_mdns: false,
            buffer_cap: 0, // unused on the client side
        })
        .await
        .expect("client connect");

        let peer = client
            .dial(gateway_addr, REQUEST_TIMEOUT)
            .await
            .expect("dial gateway");
        assert!(client.connected_peers().contains(&peer));
        assert_ne!(peer, client.local_peer_id(), "gateway is a distinct peer");

        let (head_seq, len) = await_head(&mut client, peer, 3, Duration::from_secs(10)).await;
        assert_eq!(head_seq, 3, "three consulted decisions were published");
        assert_eq!(len, 3, "all three still buffered (cap 64)");

        // Full poll from the origin cursor.
        let (poll_head, events) = client
            .poll_since(peer, 0, REQUEST_TIMEOUT)
            .await
            .expect("PollSince 0");
        assert_eq!(poll_head, 3);
        assert_eq!(events.len(), 3);
        assert_eq!(
            events.iter().map(|e| e.seq).collect::<Vec<_>>(),
            vec![1, 2, 3],
            "seq starts at 1 and is strictly increasing"
        );
        assert!(
            events
                .iter()
                .all(|e| e.schema_version == REMOTE_WIRE_SCHEMA_VERSION)
        );
        assert_eq!(
            events.iter().map(|e| e.kind.as_str()).collect::<Vec<_>>(),
            vec!["join", "join", "leave"],
        );
        assert_eq!(
            events.iter().map(|e| e.act).collect::<Vec<_>>(),
            vec![true, true, false],
        );
        assert_eq!(events[0].agent_id as usize, usize::from(c1));
        assert_eq!(events[1].agent_id as usize, usize::from(c2));
        assert_eq!(events[2].agent_id as usize, usize::from(seed));
        let label = format!("coalition-{}", usize::from(coalition));
        assert!(events.iter().all(|e| e.coalition == label));
        assert!((events[0].score - d1.score).abs() < f64::EPSILON);

        // Delta poll from a mid cursor returns only the tail.
        let (_, tail) = client
            .poll_since(peer, 2, REQUEST_TIMEOUT)
            .await
            .expect("PollSince 2");
        assert_eq!(tail.len(), 1, "only seq 3 is newer than cursor 2");
        assert_eq!(tail[0].seq, 3);
        assert_eq!(tail[0].kind, "leave");

        // Polling never drains: a repeated full poll still sees everything.
        let (_, again) = client
            .poll_since(peer, 0, REQUEST_TIMEOUT)
            .await
            .expect("second PollSince 0");
        assert_eq!(again.len(), 3, "PollSince must not drain the buffer");

        // ---------- Shutdown (prompt teardown: cancel → close → drain) ----------
        drop(service);
        drop(client);
        root.cancel();
        tracker.close();
        tracker.wait().await;
    })
    .await
    .expect("remote integration test timed out");
}
