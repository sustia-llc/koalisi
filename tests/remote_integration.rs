//! Integration test for the libp2p-based remote alert gateway.
//!
//! Verifies the wire round-trip end-to-end:
//! 1. Spawn the swarm with `enable_remote_alerts`.
//! 2. Trigger a triangular arb signal via `feed_tick`.
//! 3. From the same process, look up the gateway via `RemoteActorRef`
//!    and ask `PollOpportunities` — the ask goes through libp2p
//!    request-response (serialise → wire → deserialise → handle →
//!    serialise reply → wire → deserialise) even though the destination
//!    peer is ourselves.
//! 4. Assert the round-tripped opportunity matches what was produced
//!    locally.
//!
//! Single test file because `enable_remote_alerts` calls
//! `kameo::remote::Behaviour::init_global()`, which is process-wide. If
//! more remote tests are added later, they need to share a single
//! libp2p swarm (or use `serial_test` + tear-down hooks).

#![cfg(feature = "remote")]

use std::time::Duration;

use futures::TryStreamExt;
use kameo::prelude::*;
use kameo_actors::DeliveryStrategy;
use tokio::time::{sleep, timeout};

use forex_arbitrage_swarm::market::{Pair, Tick, Triangle};
use forex_arbitrage_swarm::subsystems::distributed::{
    PeekOpportunityCount, PollOpportunities, RemoteAlertGateway, RemoteConfig, enable_remote_alerts,
};
use forex_arbitrage_swarm::subsystems::swarm::{Swarm, SwarmConfig};

fn p(s: &str) -> Pair {
    s.parse().unwrap()
}

const GATEWAY_NAME: &str = "forex_swarm_alerts_test";

#[tokio::test]
async fn remote_gateway_round_trips_opportunity() {
    // Bound the whole test so a libp2p / mDNS hang can't lock CI.
    timeout(Duration::from_secs(20), async {
        // ---------- Setup ----------
        let triangle = Triangle::new(p("EUR/USD"), p("GBP/USD"), p("EUR/GBP")).unwrap();
        let swarm = Swarm::new(SwarmConfig {
            triangles: vec![triangle],
            threshold_bps: 10.0,
            history_capacity: 32,
            delivery_strategy: DeliveryStrategy::Guaranteed,
        })
        .await
        .unwrap();

        let handle = enable_remote_alerts(
            &swarm,
            RemoteConfig {
                gateway_name: GATEWAY_NAME.into(),
                listen_addr: "/ip4/127.0.0.1/tcp/0".into(),
                enable_mdns: false, // not needed for loopback round-trip
            },
        )
        .await
        .expect("enable_remote_alerts");

        // Give the libp2p swarm a brief moment to finish listen + register.
        sleep(Duration::from_millis(200)).await;

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

        // Direct (local) check: the gateway has the opportunity.
        let local_count = handle
            .gateway
            .ask(forex_arbitrage_swarm::subsystems::distributed::PeekOpportunityCount)
            .await
            .unwrap();
        assert_eq!(local_count, 1, "gateway should have buffered 1 alert");

        // ---------- Remote round-trip ----------
        // Find ourselves in the kameo registry and ask via the wire.
        let mut remote_refs = RemoteActorRef::<RemoteAlertGateway>::lookup_all(GATEWAY_NAME);
        let mut polled: Option<Vec<_>> = None;
        let mut peeked: Option<u64> = None;
        while let Some(remote_ref) = remote_refs.try_next().await.unwrap() {
            // PeekOpportunityCount and PollOpportunities both go through
            // serialise → libp2p request-response → deserialise.
            peeked = Some(
                remote_ref
                    .ask(&PeekOpportunityCount)
                    .send()
                    .await
                    .expect("remote ask: PeekOpportunityCount"),
            );
            polled = Some(
                remote_ref
                    .ask(&PollOpportunities)
                    .send()
                    .await
                    .expect("remote ask: PollOpportunities"),
            );
        }

        let peeked = peeked.expect("at least one remote peer in registry (loopback to self)");
        let polled = polled.unwrap();
        assert_eq!(peeked, 1, "remote PeekOpportunityCount should be 1");
        assert_eq!(
            polled.len(),
            1,
            "remote PollOpportunities should round-trip the 1 buffered alert"
        );
        let opp = &polled[0];
        assert_eq!(opp.triangle.cross, eg, "cross pair should round-trip");
        assert!(
            opp.edge_bps > 40.0 && opp.edge_bps < 50.0,
            "edge_bps {} not in [40, 50]",
            opp.edge_bps
        );

        // ---------- Shutdown ----------
        // `Swarm::shutdown()` cancels the root token, which propagates to
        // the child token feeding the libp2p event loop — that task exits
        // and `task_tracker.wait()` drains.
        swarm.shutdown().await;
    })
    .await
    .expect("remote integration test timed out");
}
