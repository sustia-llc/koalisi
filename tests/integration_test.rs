//! Integration test for the full swarm wiring.
//!
//! Mirrors the style of `surrealdb-live-message`'s `integration_test.rs`:
//! a single test driver builds the swarm, exercises every code path the
//! library cares about, and asserts on observed alerts. No persistence is
//! involved (POC); no Docker container either — the in-process tokio::sync
//! mpsc + broadcast buses are the entire transport.

use std::time::Duration;
use tokio::time::timeout;

use koalisi::core::config::setup_logging;
use koalisi::market::{Direction, Pair, Tick, Triangle};
use koalisi::subsystems::swarm::{Swarm, SwarmConfig};

fn p(s: &str) -> Pair {
    s.parse().unwrap()
}

/// Convenience: build a swarm with the canonical EUR/USD-GBP/USD-EUR/GBP
/// triangle and a 10 bps threshold.
async fn build_swarm() -> Swarm {
    let triangle = Triangle::new(p("EUR/USD"), p("GBP/USD"), p("EUR/GBP")).unwrap();
    Swarm::new(SwarmConfig {
        triangles: vec![triangle],
        threshold_bps: 10.0,
        history_capacity: 64,
    })
    .await
    .expect("swarm assembly")
}

#[tokio::test]
async fn end_to_end_triangular_arbitrage() {
    setup_logging();

    // Bound the whole test so a misbehaving flush can't hang CI.
    timeout(Duration::from_secs(10), async {
        let swarm = build_swarm().await;
        let eu = p("EUR/USD");
        let gu = p("GBP/USD");
        let eg = p("EUR/GBP");

        // ------------------------------------------------------------------
        // Phase 1 — historical bootstrap. Cross trades at synthetic so no
        // alerts should fire.
        // ------------------------------------------------------------------
        let synthetic = 1.10 / 1.30; // ≈ 0.84615
        swarm
            .replay_history(vec![
                Tick::new(eu.clone(), 1.0998, 1.1002, 0),
                Tick::new(gu.clone(), 1.2998, 1.3002, 0),
                Tick::new(eg.clone(), synthetic - 0.0001, synthetic + 0.0001, 0),
            ])
            .await
            .unwrap();
        assert!(
            swarm.alerts().await.unwrap().is_empty(),
            "no alert expected on aligned market"
        );

        // ------------------------------------------------------------------
        // Phase 2 — cross dislocates to 0.85; ~45 bps above synthetic →
        // exactly one alert, BuySyntheticSellActual.
        // ------------------------------------------------------------------
        swarm
            .feed_tick(Tick::new(eg.clone(), 0.8499, 0.8501, 1_000))
            .await
            .unwrap();
        let alerts = swarm.alerts().await.unwrap();
        assert_eq!(alerts.len(), 1, "expected exactly one alert");
        let opp = &alerts[0];
        assert_eq!(opp.triangle.cross, eg);
        assert!(
            opp.edge_bps > 40.0 && opp.edge_bps < 50.0,
            "edge {} not in [40, 50] bps",
            opp.edge_bps,
        );
        assert_eq!(opp.direction, Direction::BuySyntheticSellActual);
        assert_eq!(opp.detected_at_ms, 1_000);

        // ------------------------------------------------------------------
        // Phase 3 — hysteresis. Re-feeding the same dislocated cross
        // should *not* fire a second alert because the triangle is still
        // in its "fired" state.
        // ------------------------------------------------------------------
        swarm
            .feed_tick(Tick::new(eg.clone(), 0.8499, 0.8501, 1_100))
            .await
            .unwrap();
        assert_eq!(
            swarm.alerts().await.unwrap().len(),
            1,
            "hysteresis should suppress re-firing on identical input"
        );

        // ------------------------------------------------------------------
        // Phase 4 — re-align (edge drops below threshold) then dislocate
        // the other way. We should now see a second, oppositely-signed
        // alert.
        // ------------------------------------------------------------------
        swarm
            .feed_tick(Tick::new(
                eg.clone(),
                synthetic - 0.0001,
                synthetic + 0.0001,
                2_000,
            ))
            .await
            .unwrap();
        assert_eq!(
            swarm.alerts().await.unwrap().len(),
            1,
            "re-align should rearm without firing"
        );
        swarm
            .feed_tick(Tick::new(eg.clone(), 0.8400, 0.8402, 3_000))
            .await
            .unwrap();
        let alerts = swarm.alerts().await.unwrap();
        assert_eq!(alerts.len(), 2, "second, opposite-sign alert expected");
        let flipped = alerts.last().unwrap();
        assert_eq!(flipped.direction, Direction::SellSyntheticBuyActual);
        assert!(
            flipped.edge_bps < -50.0,
            "flipped edge {} not strongly negative",
            flipped.edge_bps
        );

        // ------------------------------------------------------------------
        // Phase 5 — clean shutdown.
        // ------------------------------------------------------------------
        swarm.shutdown().await;
    })
    .await
    .expect("integration test timed out");
}

#[tokio::test]
async fn monitor_snapshot_holds_full_history_within_capacity() {
    setup_logging();

    let swarm = build_swarm().await;
    let eu = p("EUR/USD");

    let ticks: Vec<Tick> = (0..30)
        .map(|i| {
            Tick::new(
                eu.clone(),
                1.10 + (i as f64) * 1e-5,
                1.10 + (i as f64) * 1e-5 + 0.0001,
                i as i64,
            )
        })
        .collect();
    swarm.replay_history(ticks.clone()).await.unwrap();

    let monitor = swarm.monitor(&eu).unwrap();
    let snap = monitor.snapshot().await.unwrap();
    assert_eq!(snap.history.len(), 30);
    assert_eq!(snap.history[0].timestamp_ms, 0);
    assert_eq!(snap.history.last().unwrap().timestamp_ms, 29);
    assert!(snap.latest.is_some());

    swarm.shutdown().await;
}

#[tokio::test]
async fn monitor_ring_buffer_evicts_oldest_when_full() {
    setup_logging();

    let triangle = Triangle::new(p("EUR/USD"), p("GBP/USD"), p("EUR/GBP")).unwrap();
    let swarm = Swarm::new(SwarmConfig {
        triangles: vec![triangle],
        threshold_bps: 10.0,
        history_capacity: 5,
    })
    .await
    .unwrap();
    let eu = p("EUR/USD");

    let ticks: Vec<Tick> = (0..12)
        .map(|i| Tick::new(eu.clone(), 1.10, 1.1002, i as i64))
        .collect();
    swarm.replay_history(ticks).await.unwrap();

    let snap = swarm.monitor(&eu).unwrap().snapshot().await.unwrap();
    assert_eq!(snap.history.len(), 5);
    // Should contain timestamps 7..=11 (last 5 of 12).
    let stamps: Vec<i64> = snap.history.iter().map(|t| t.timestamp_ms).collect();
    assert_eq!(stamps, vec![7, 8, 9, 10, 11]);

    swarm.shutdown().await;
}

#[tokio::test]
async fn swarm_rejects_empty_triangles() {
    setup_logging();
    let result = Swarm::new(SwarmConfig {
        triangles: vec![],
        threshold_bps: 5.0,
        history_capacity: 8,
    })
    .await;
    match result {
        Ok(_) => panic!("empty-triangles config should not produce a Swarm"),
        Err(err) => assert!(err.to_string().contains("triangles")),
    }
}

#[tokio::test]
async fn feed_tick_rejects_unknown_pair() {
    setup_logging();
    let swarm = build_swarm().await;
    let err = swarm
        .feed_tick(Tick::new(p("CHF/JPY"), 1.0, 1.0001, 0))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("no monitor"));
    swarm.shutdown().await;
}
