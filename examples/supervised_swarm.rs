//! Fault-tolerance demo for the swarm using the thin task-restart layer
//! (`koalisi::core::supervision`, issue #6) that replaced kameo's `OneForOne`
//! supervision.
//!
//! A monitor worker is spawned via [`spawn_supervised`] with a
//! `restart_limit(3, 5s)` budget. It processes ticks from a `broadcast` input
//! (the input survives restart: each rebuilt worker re-`subscribe()`s). We then
//! inject a panic; the supervisor observes the panic through the inner task's
//! `JoinHandle`, rebuilds a *fresh* worker from the factory, and the restarted
//! worker keeps processing new ticks.
//!
//! This obsoletes the old kameo gotcha (supervised actors kept the same
//! `ActorId` across restart, so `wait_for_shutdown` on the ref would hang):
//! here a restart is simply the factory building a new task instance, and a
//! shared progress counter shows liveness across the panic.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use anyhow::Result;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use koalisi::core::spawn_supervised;
use koalisi::market::{Pair, Tick};
use koalisi::subsystems::monitor::MarketMonitor;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_target(false).try_init().ok();

    let pair: Pair = "EUR/USD".parse()?;
    let tracker = TaskTracker::new();
    let root = CancellationToken::new();

    // A broadcast input the worker consumes. It survives restart because each
    // rebuilt worker re-subscribes for a fresh receiver.
    let (input, _seed_rx) = broadcast::channel::<Tick>(64);
    let processed = Arc::new(AtomicUsize::new(0));
    let poison = Arc::new(AtomicBool::new(false));

    // Supervised monitor worker — restart_limit(3, 5s), mirroring the old
    // kameo `OneForOne::restart_limit(3, Duration::from_secs(5))`.
    {
        let input = input.clone();
        let pair = pair.clone();
        let processed = processed.clone();
        let poison = poison.clone();
        spawn_supervised(
            &tracker,
            root.child_token(),
            3,
            Duration::from_secs(5),
            move |child| {
                let mut rx = input.subscribe();
                let pair = pair.clone();
                let processed = processed.clone();
                let poison = poison.clone();
                async move {
                    // A fresh MarketMonitor is built on every (re)start — state
                    // resets, which is exactly the "rebuild from the factory"
                    // story.
                    let mut monitor = MarketMonitor::new(pair, 32);
                    loop {
                        tokio::select! {
                            biased;
                            _ = child.cancelled() => return,
                            r = rx.recv() => match r {
                                Ok(tick) => {
                                    if poison.swap(false, Ordering::SeqCst) {
                                        panic!("injected panic — testing supervision restart");
                                    }
                                    let _ = monitor.ingest(tick);
                                    processed.fetch_add(1, Ordering::SeqCst);
                                }
                                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                                Err(broadcast::error::RecvError::Closed) => return,
                            }
                        }
                    }
                }
            },
        );
    }

    // Phase 1: drive some ticks → the worker processes them.
    let before = drive(&input, &pair, &processed, 5).await;
    println!("✓ phase 1: worker processed {before} ticks");

    // Phase 2: inject a panic. The next tick the worker sees crashes it; the
    // supervisor rebuilds a fresh worker from the factory.
    poison.store(true, Ordering::SeqCst);
    println!("→ phase 2: injecting a panic; supervisor will rebuild the worker");

    // Phase 3: keep driving ticks. The panic fires on the first post-poison
    // tick, then the *restarted* worker resumes processing — the shared
    // counter climbing past `before` proves cross-restart liveness.
    let after = drive(&input, &pair, &processed, before + 5).await;
    println!(
        "✓ phase 3: after the panic + restart, worker processed {} more ticks (total {after})",
        after - before
    );
    assert!(
        after > before,
        "the restarted worker must keep making progress"
    );

    // Shutdown: cancel the root token, close + drain the tracker.
    root.cancel();
    tracker.close();
    tracker.wait().await;
    println!("✓ shutdown clean");
    Ok(())
}

/// Send ticks until `processed` reaches at least `target` (or a 3s deadline).
/// Robust against the broadcast subscribe/restart race: it keeps feeding until
/// the worker demonstrably advances. Returns the final processed count.
async fn drive(
    input: &broadcast::Sender<Tick>,
    pair: &Pair,
    processed: &Arc<AtomicUsize>,
    target: usize,
) -> usize {
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut ts = 0i64;
    loop {
        if processed.load(Ordering::SeqCst) >= target || Instant::now() > deadline {
            break;
        }
        let mid = 1.10 + (ts as f64) * 1e-5;
        let _ = input.send(Tick::new(pair.clone(), mid - 1e-4, mid + 1e-4, ts));
        ts += 1;
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    processed.load(Ordering::SeqCst)
}
