//! Integration test for the raw-libp2p remote alert gateway.
//!
//! Verifies the wire round-trip end-to-end across two independent libp2p
//! swarms in one process:
//! 1. Spawn a producer swarm with `enable_remote_alerts` (mDNS off; it listens
//!    on `127.0.0.1` and hands back the bound address).
//! 2. Trigger a triangular arb signal via `feed_tick`.
//! 3. Build a `RemoteAlertClient`, dial the producer's bound address, and ask
//!    `PeekCount` / `Poll` — each goes through the CBOR request-response
//!    protocol (serialise → wire → deserialise → handle → serialise reply →
//!    wire → deserialise).
//! 4. Assert the round-tripped opportunity matches what was produced.
//!
//! Note: the former actor-registry gateway used a process-wide global registry,
//! so only ONE remote swarm could exist per process (see the old CLAUDE.md
//! gotcha #9). That constraint is gone with raw request-response: this test
//! stands up a producer AND a client in the same process, dialing over
//! loopback, with no shared global registry.

#![cfg(feature = "remote")]

use std::time::Duration;

use tokio::time::timeout;

use koalisi::market::{Pair, Tick, Triangle};
use koalisi::subsystems::distributed::{RemoteAlertClient, RemoteConfig, enable_remote_alerts};
use koalisi::subsystems::swarm::{Swarm, SwarmConfig};

fn p(s: &str) -> Pair {
    s.parse().unwrap()
}

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

#[tokio::test]
async fn remote_gateway_round_trips_opportunity() {
    // Bound the whole test so a libp2p hang can't lock CI.
    timeout(Duration::from_secs(20), async {
        // ---------- Producer setup ----------
        let triangle = Triangle::new(p("EUR/USD"), p("GBP/USD"), p("EUR/GBP")).unwrap();
        let swarm = Swarm::new(SwarmConfig {
            triangles: vec![triangle],
            threshold_bps: 10.0,
            history_capacity: 32,
        })
        .await
        .unwrap();

        let handle = enable_remote_alerts(
            &swarm,
            RemoteConfig {
                listen_addr: "/ip4/127.0.0.1/tcp/0".into(),
                enable_mdns: false, // dial explicitly over loopback
            },
        )
        .await
        .expect("enable_remote_alerts");
        let gateway_addr = handle
            .listen_addrs
            .first()
            .expect("gateway should have a bound listen address")
            .clone();

        // ---------- Trigger one arbitrage opportunity ----------
        let eu = p("EUR/USD");
        let gu = p("GBP/USD");
        let eg = p("EUR/GBP");
        let synthetic = 1.10 / 1.30; // ~0.84615

        swarm
            .replay_history(vec![
                Tick::new(eu.clone(), 1.0998, 1.1002, 0),
                Tick::new(gu.clone(), 1.2998, 1.3002, 0),
                Tick::new(eg.clone(), synthetic - 0.0001, synthetic + 0.0001, 0),
            ])
            .await
            .unwrap();
        // Dislocate the cross — ~45 bps above synthetic.
        swarm
            .feed_tick(Tick::new(eg.clone(), 0.8499, 0.8501, 1_000))
            .await
            .unwrap();
        // Flush the swarm so the sink + the gateway both see it.
        swarm.flush().await.unwrap();
        // The gateway task consumes the alert off the broadcast bus on its own
        // scheduler tick; give it a beat to buffer before we poll over the wire.
        tokio::time::sleep(Duration::from_millis(200)).await;

        // ---------- Consumer: dial + wire round-trip ----------
        let mut client = RemoteAlertClient::connect(RemoteConfig {
            listen_addr: "/ip4/127.0.0.1/tcp/0".into(),
            enable_mdns: false,
        })
        .await
        .expect("client connect");

        let peer = client
            .dial(gateway_addr, REQUEST_TIMEOUT)
            .await
            .expect("dial gateway");

        // PeekCount and Poll both go through serialise → request-response → wire.
        let peeked = client
            .peek_count(peer, REQUEST_TIMEOUT)
            .await
            .expect("remote PeekCount");
        let polled = client.poll(peer, REQUEST_TIMEOUT).await.expect("remote Poll");

        assert_eq!(peeked, 1, "remote PeekCount should be 1");
        assert_eq!(
            polled.len(),
            1,
            "remote Poll should round-trip the 1 buffered alert"
        );
        let opp = &polled[0];
        assert_eq!(opp.triangle.cross, eg, "cross pair should round-trip");
        assert!(
            opp.edge_bps > 40.0 && opp.edge_bps < 50.0,
            "edge_bps {} not in [40, 50]",
            opp.edge_bps
        );

        // Poll is non-draining: a second Poll still sees the alert.
        let polled_again = client
            .poll(peer, REQUEST_TIMEOUT)
            .await
            .expect("second remote Poll");
        assert_eq!(polled_again.len(), 1, "Poll must not drain the buffer");

        // Clear drops the buffer; a subsequent Poll is empty.
        client.clear(peer, REQUEST_TIMEOUT).await.expect("remote Clear");
        let after_clear = client
            .poll(peer, REQUEST_TIMEOUT)
            .await
            .expect("Poll after Clear");
        assert!(after_clear.is_empty(), "buffer should be empty after Clear");

        // ---------- Shutdown ----------
        // `Swarm::shutdown()` cancels the root token, which propagates to the
        // child token feeding the gateway task — that task exits and
        // `task_tracker.wait()` drains.
        swarm.shutdown().await;
    })
    .await
    .expect("remote integration test timed out");
}
