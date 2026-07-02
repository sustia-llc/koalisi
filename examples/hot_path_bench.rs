//! Hot-path latency + throughput bench for the K3 messaging swap (issue #6).
//!
//! This is the *pre-registered baseline* harness: it measures the three
//! latency-sensitive seams of the swarm and prints a small fixed-format
//! table. It compiles and runs on BOTH the kameo runtime (commit A) and the
//! post-swap tokio::sync runtime (commit B) with byte-comparable output — only
//! the internal plumbing calls (subscription mechanism, coordinator ping)
//! differ between the two; the measurement logic and the table format are
//! identical.
//!
//! Sections:
//!   1. Alert round-trip  — feed a dislocating tick, time until a subscribed
//!      listener receives the resulting `ArbitrageOpportunity`. Re-aligns
//!      between iterations so the coordinator's per-triangle hysteresis
//!      re-arms and each iteration fires exactly one alert.
//!   2. Ask round-trip    — `Ping` the coordinator and time the reply.
//!   3. Throughput        — feed aligned ticks (no alerts) and report ticks/s.
//!
//! Run with (numbers are only meaningful on an optimized build):
//! ```sh
//! cargo run --release --example hot_path_bench
//! ```

use std::time::{Duration, Instant};

use anyhow::Result;

use koalisi::market::{Pair, Tick, Triangle};
use koalisi::subsystems::swarm::{Swarm, SwarmConfig};

/// Runtime label printed in the table header. Commit B: "tokio".
const RUNTIME: &str = "tokio";

const ALERT_ITERS: usize = 500;
const ASK_ITERS: usize = 10_000;
const THROUGHPUT_TICKS: usize = 100_000;
const WARMUP: usize = 50;

// ---------------------------------------------------------------------------
// Fixed market choreography (deterministic — no RNG).
// ---------------------------------------------------------------------------

struct Legs {
    eu: Pair,
    gu: Pair,
    eg: Pair,
    synthetic: f64,
}

impl Legs {
    fn new() -> Result<Self> {
        let eu: Pair = "EUR/USD".parse()?;
        let gu: Pair = "GBP/USD".parse()?;
        let eg: Pair = "EUR/GBP".parse()?;
        Ok(Self {
            eu,
            gu,
            eg,
            synthetic: 1.10 / 1.30, // ~0.846153
        })
    }

    fn aligned_eu(&self) -> Tick {
        Tick::new(self.eu.clone(), 1.0999, 1.1001, 0)
    }
    fn aligned_gu(&self) -> Tick {
        Tick::new(self.gu.clone(), 1.2999, 1.3001, 0)
    }
    fn aligned_eg(&self) -> Tick {
        Tick::new(self.eg.clone(), self.synthetic - 0.0001, self.synthetic + 0.0001, 0)
    }
    /// ~45 bps above synthetic ⇒ fires one alert.
    fn dislocated_eg(&self) -> Tick {
        Tick::new(self.eg.clone(), 0.8499, 0.8501, 1_000)
    }
}

// ---------------------------------------------------------------------------
// Percentile helpers
// ---------------------------------------------------------------------------

fn percentile_us(sorted: &[Duration], pct: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (pct / 100.0 * (sorted.len() as f64 - 1.0)).round() as usize;
    sorted[rank.min(sorted.len() - 1)].as_secs_f64() * 1e6
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<()> {
    // Keep logging quiet so it doesn't perturb latency numbers.
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .with_target(false)
        .try_init()
        .ok();

    let legs = Legs::new()?;
    let triangle = Triangle::new(legs.eu.clone(), legs.gu.clone(), legs.eg.clone())?;
    let swarm = Swarm::new(SwarmConfig {
        triangles: vec![triangle],
        threshold_bps: 10.0,
        history_capacity: 64,
    })
    .await?;

    // ---- subscribe a listener to the alert bus (commit-B plumbing) ----
    let mut sig_rx = swarm.alert_bus().subscribe();

    // Seed all three quotes so the coordinator can resolve the triangle.
    swarm.feed_tick(legs.aligned_eu()).await?;
    swarm.feed_tick(legs.aligned_gu()).await?;
    swarm.feed_tick(legs.aligned_eg()).await?;
    swarm.flush().await?;

    // =====================================================================
    // 1. Alert round-trip
    // =====================================================================
    let mut alert_samples: Vec<Duration> = Vec::with_capacity(ALERT_ITERS);
    for i in 0..(WARMUP + ALERT_ITERS) {
        // Re-align so the triangle re-arms, then flush so the coordinator
        // has processed the re-alignment before we time the dislocation.
        swarm.feed_tick(legs.aligned_eu()).await?;
        swarm.feed_tick(legs.aligned_gu()).await?;
        swarm.feed_tick(legs.aligned_eg()).await?;
        swarm.flush().await?;
        while sig_rx.try_recv().is_ok() {} // clear any stray signal

        let start = Instant::now();
        swarm.feed_tick(legs.dislocated_eg()).await?;
        // Wait for the listener to observe the alert.
        let _ = sig_rx.recv().await;
        let dt = start.elapsed();
        if i >= WARMUP {
            alert_samples.push(dt);
        }
    }
    alert_samples.sort_unstable();
    let alert_median = percentile_us(&alert_samples, 50.0);
    let alert_p95 = percentile_us(&alert_samples, 95.0);
    let alert_p99 = percentile_us(&alert_samples, 99.0);

    // =====================================================================
    // 2. Ask round-trip (coordinator Ping)
    // =====================================================================
    for _ in 0..WARMUP {
        swarm.coordinator().ping().await?;
    }
    let mut ask_samples: Vec<Duration> = Vec::with_capacity(ASK_ITERS);
    for _ in 0..ASK_ITERS {
        let start = Instant::now();
        swarm.coordinator().ping().await?;
        ask_samples.push(start.elapsed());
    }
    ask_samples.sort_unstable();
    let ask_median = percentile_us(&ask_samples, 50.0);
    let ask_p99 = percentile_us(&ask_samples, 99.0);

    // =====================================================================
    // 3. Throughput (aligned ticks, no alerts)
    // =====================================================================
    for _ in 0..WARMUP {
        swarm.feed_tick(legs.aligned_eu()).await?;
    }
    swarm.flush().await?;
    let start = Instant::now();
    for _ in 0..THROUGHPUT_TICKS {
        swarm.feed_tick(legs.aligned_eu()).await?;
    }
    swarm.flush().await?;
    let elapsed = start.elapsed();
    let ticks_per_sec = THROUGHPUT_TICKS as f64 / elapsed.as_secs_f64();

    // =====================================================================
    // Report
    // =====================================================================
    println!("hot_path_bench (--release)  runtime={RUNTIME}");
    println!(
        "  alert round-trip (K={ALERT_ITERS}):   median={alert_median:8.2} us  p95={alert_p95:8.2} us  p99={alert_p99:8.2} us"
    );
    println!(
        "  ask   round-trip (K={ASK_ITERS}): median={ask_median:8.2} us  p99={ask_p99:8.2} us"
    );
    println!(
        "  throughput ({THROUGHPUT_TICKS} ticks):  {ticks_per_sec:10.0} ticks/sec"
    );

    // Tidy up.
    swarm.shutdown().await;
    Ok(())
}
