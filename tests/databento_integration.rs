//! Integration test for the databento DBN adapter.
//!
//! Verifies that the bundled MBP-1 test file is decoded and routed
//! correctly into the swarm. Skipped automatically (test passes with a
//! diagnostic) if the file can't be located.

#![cfg(feature = "databento")]

use std::path::PathBuf;
use std::time::Duration;

use kameo_actors::DeliveryStrategy;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use forex_arbitrage_swarm::market::{Pair, Triangle};
use forex_arbitrage_swarm::subsystems::databento::{
    Pacing, PumpStats, mapper_from_fn, mbp1_to_tick, pump_dbn_file, spawn_dbn_pump,
};
use forex_arbitrage_swarm::subsystems::monitor::GetSnapshot;
use forex_arbitrage_swarm::subsystems::swarm::{Swarm, SwarmConfig};

fn p(s: &str) -> Pair {
    s.parse().unwrap()
}

fn locate_test_data() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("DBN_TEST_PATH") {
        let p = PathBuf::from(p);
        if p.exists() {
            return Some(p);
        }
    }
    for c in [
        "../trade/databento-rs/tests/data/test_data.mbp-1.dbn.zst",
        "../../trade/databento-rs/tests/data/test_data.mbp-1.dbn.zst",
        "../databento-rs/tests/data/test_data.mbp-1.dbn.zst",
    ] {
        let p = PathBuf::from(c);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

async fn build_swarm() -> Swarm {
    Swarm::new(SwarmConfig {
        triangles: vec![Triangle::new(p("EUR/USD"), p("GBP/USD"), p("EUR/GBP")).unwrap()],
        threshold_bps: 5.0,
        history_capacity: 64,
        delivery_strategy: DeliveryStrategy::Guaranteed,
    })
    .await
    .unwrap()
}

#[test]
fn mbp1_to_tick_converts_fixed_point_and_nanos() {
    use dbn::{BidAskPair, FlagSet, Mbp1Msg, record::RecordHeader, rtype};

    // bid 1.10000_0000, ask 1.10004_0000
    let msg = Mbp1Msg {
        hd: RecordHeader::new::<Mbp1Msg>(rtype::MBP_1, 1, 42, 1_700_000_000_500_000_000),
        price: 0,
        size: 0,
        action: b'T' as i8,
        side: b'B' as i8,
        flags: FlagSet::default(),
        depth: 0,
        ts_recv: 1_700_000_000_500_000_000,
        ts_in_delta: 0,
        sequence: 1,
        levels: [BidAskPair {
            bid_px: 1_100_000_000, // 1.10
            ask_px: 1_100_400_000, // 1.1004
            bid_sz: 0,
            ask_sz: 0,
            bid_ct: 0,
            ask_ct: 0,
        }],
    };
    let tick = mbp1_to_tick(&msg, p("EUR/USD"));
    assert!((tick.bid - 1.10).abs() < 1e-9);
    assert!((tick.ask - 1.1004).abs() < 1e-9);
    assert!((tick.mid() - 1.1002).abs() < 1e-9);
    assert_eq!(tick.timestamp_ms, 1_700_000_000_500);
}

#[tokio::test]
async fn pump_dbn_file_routes_bundled_test_data() {
    let Some(path) = locate_test_data() else {
        eprintln!(
            "skipping pump_dbn_file_routes_bundled_test_data: \
             test_data.mbp-1.dbn.zst not found"
        );
        return;
    };

    let swarm = build_swarm().await;

    let mapper = mapper_from_fn(move |_id, _sym| Some(p("EUR/USD")));
    let stats: PumpStats = timeout(
        Duration::from_secs(5),
        pump_dbn_file(
            swarm.feeder(),
            &path,
            mapper,
            Pacing::Asap,
            CancellationToken::new(),
        ),
    )
    .await
    .expect("pump did not finish within 5s")
    .expect("pump returned an error");

    assert!(
        stats.mbp1_records >= 1,
        "expected at least one MBP-1 record from the bundled file, got {stats:?}",
    );
    assert_eq!(
        stats.ticks_emitted, stats.mbp1_records,
        "every mapped record should produce a tick, stats: {stats:?}",
    );
    assert_eq!(stats.feed_errors, 0);

    let snap = swarm
        .monitor(&p("EUR/USD"))
        .unwrap()
        .ask(GetSnapshot)
        .await
        .unwrap();
    assert_eq!(
        snap.history.len(),
        stats.mbp1_records,
        "monitor history should match pumped record count"
    );

    swarm.shutdown().await;
}

#[tokio::test]
async fn spawn_dbn_pump_joins_swarm_lifecycle() {
    let Some(path) = locate_test_data() else {
        eprintln!(
            "skipping spawn_dbn_pump_joins_swarm_lifecycle: \
             test_data.mbp-1.dbn.zst not found"
        );
        return;
    };

    let swarm = build_swarm().await;
    let mapper = mapper_from_fn(move |_id, _sym| Some(p("EUR/USD")));

    let handle = spawn_dbn_pump(&swarm, &path, mapper, Pacing::Asap);

    // The pump should finish on its own (file is tiny) — we just verify
    // that it's joinable and returns Ok.
    let stats = timeout(Duration::from_secs(5), handle)
        .await
        .expect("spawn_dbn_pump task did not finish within 5s")
        .expect("spawn_dbn_pump task panicked")
        .expect("spawn_dbn_pump task returned Err");
    assert!(stats.mbp1_records >= 1);

    swarm.shutdown().await;
}

#[tokio::test]
async fn pump_obeys_cancellation_token() {
    let Some(path) = locate_test_data() else {
        eprintln!("skipping pump_obeys_cancellation_token: test data not found");
        return;
    };

    let swarm = build_swarm().await;
    let mapper = mapper_from_fn(move |_id, _sym| Some(p("EUR/USD")));

    // Pre-cancel the token so the pump returns immediately on first
    // select-cancellation check.
    let token = CancellationToken::new();
    token.cancel();

    let stats = timeout(
        Duration::from_secs(2),
        pump_dbn_file(swarm.feeder(), &path, mapper, Pacing::Asap, token),
    )
    .await
    .expect("cancelled pump didn't return within 2s")
    .expect("cancelled pump returned an error");
    assert_eq!(stats.records_decoded, 0, "cancelled pump should not decode");
    swarm.shutdown().await;
}
