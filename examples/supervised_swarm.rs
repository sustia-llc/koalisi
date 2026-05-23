//! Demonstrate fault-tolerance for the swarm using kameo's supervision.
//!
//! Uses the **oneshot readiness pattern** from `surrealdb-live-message`'s
//! `sdb_server::SurrealDBContainer::start_and_wait`: the supervisor signals
//! readiness on a `oneshot::Sender` (after its supervised child has been
//! spawned) carrying the child's `ActorRef` as the payload, and the main
//! task awaits it under a `tokio::time::timeout`. No registry polling, no
//! sleeps — main task gets the ref the moment it's available.
//!
//! Demonstrates:
//! 1. `oneshot` readiness handshake from supervisor `on_start` → main.
//! 2. `MarketMonitor::supervise(..)` + `restart_limit(..)` wired under a
//!    `OneForOne` supervisor.
//! 3. After a `ForcePanic`, the supervisor restarts the child (visible as
//!    a second "MarketMonitor started pair=EUR/USD" log line) and itself
//!    remains responsive to messages.

use std::time::Duration;

use anyhow::{Result, anyhow};
use kameo::error::Infallible;
use kameo::prelude::*;
use kameo::supervision::SupervisionStrategy;
use kameo_actors::{DeliveryStrategy, pubsub::PubSub};
use tokio::sync::oneshot;
use tokio::time::timeout;

use forex_arbitrage_swarm::market::{Pair, Tick, TickUpdate};
use forex_arbitrage_swarm::subsystems::monitor::{GetSnapshot, MarketMonitor, MarketMonitorArgs};

// ---------------------------------------------------------------------------
// Force-panic message
// ---------------------------------------------------------------------------

/// Sent to the monitor to deliberately crash its handler so the supervisor
/// can be observed restarting it.
struct ForcePanic;

impl Message<ForcePanic> for MarketMonitor {
    type Reply = ();
    async fn handle(&mut self, _: ForcePanic, _: &mut Context<Self, Self::Reply>) -> Self::Reply {
        panic!("ForcePanic received — testing supervision restart");
    }
}

// ---------------------------------------------------------------------------
// Supervisor with oneshot-based readiness signaling
// ---------------------------------------------------------------------------

/// Args passed into the supervisor. The `ready_tx` is consumed inside
/// `on_start` and fires with the supervised child's ref the moment it's
/// spawned — mirror of `surrealdb-live-message`'s `(tx, rx) = oneshot::channel()`
/// + `let _ = tx.send(())` pattern, but carrying the child ref as the
/// payload so the main task doesn't need a separate lookup step.
struct SupervisorArgs {
    pair: Pair,
    tick_bus: ActorRef<PubSub<TickUpdate>>,
    ready_tx: oneshot::Sender<ActorRef<MarketMonitor>>,
}

#[derive(Default)]
struct SwarmSupervisor;

impl Actor for SwarmSupervisor {
    type Args = SupervisorArgs;
    type Error = Infallible;

    fn supervision_strategy() -> SupervisionStrategy {
        SupervisionStrategy::OneForOne
    }

    async fn on_start(
        args: SupervisorArgs,
        supervisor_ref: ActorRef<Self>,
    ) -> Result<Self, Self::Error> {
        let monitor = MarketMonitor::supervise(
            &supervisor_ref,
            MarketMonitorArgs {
                pair: args.pair.clone(),
                history_capacity: 32,
                tick_bus: args.tick_bus,
            },
        )
        .restart_limit(3, Duration::from_secs(5))
        .spawn()
        .await;

        // Fire the readiness signal carrying the freshly-spawned child.
        // If the receiver has been dropped we proceed anyway — the
        // supervisor is still useful in that case.
        let _ = args.ready_tx.send(monitor);
        tracing::info!("supervisor on_start complete, ready signal fired");

        Ok(SwarmSupervisor)
    }
}

// ---------------------------------------------------------------------------
// Liveness probe for the supervisor
// ---------------------------------------------------------------------------

struct Ping;

impl Message<Ping> for SwarmSupervisor {
    type Reply = ();
    async fn handle(&mut self, _: Ping, _: &mut Context<Self, Self::Reply>) -> Self::Reply {}
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_target(false).try_init().ok();

    let pair: Pair = "EUR/USD".parse()?;
    let tick_bus = PubSub::spawn(PubSub::<TickUpdate>::new(DeliveryStrategy::Guaranteed));

    // -----------------------------------------------------------------------
    // Spawn the supervisor and await the oneshot ready signal.
    // -----------------------------------------------------------------------
    let (ready_tx, ready_rx) = oneshot::channel::<ActorRef<MarketMonitor>>();
    let supervisor = SwarmSupervisor::spawn(SupervisorArgs {
        pair: pair.clone(),
        tick_bus: tick_bus.clone(),
        ready_tx,
    });

    let monitor = match timeout(Duration::from_secs(5), ready_rx).await {
        Ok(Ok(monitor)) => monitor,
        Ok(Err(_)) => return Err(anyhow!("supervisor dropped ready channel without firing")),
        Err(_) => return Err(anyhow!("timed out waiting for supervisor ready signal")),
    };
    println!("✓ supervisor ready (monitor id = {})", monitor.id());

    // -----------------------------------------------------------------------
    // Phase 1: feed a tick → confirm the initial monitor processes it.
    // -----------------------------------------------------------------------
    monitor
        .tell(Tick::new(pair.clone(), 1.0998, 1.1002, 0))
        .await?;
    let snap = monitor.ask(GetSnapshot).await?;
    println!(
        "✓ phase 1: initial monitor accepted tick (history len = {})",
        snap.history.len()
    );

    // -----------------------------------------------------------------------
    // Phase 2: force a panic. Note that kameo's supervised actors keep
    // the *same* actor id across restarts (you'll see `actor.id=#2` in
    // both the "actor stopped" and the subsequent "MarketMonitor started"
    // log lines). This means we do NOT call `monitor.wait_for_shutdown()`
    // — it would hang forever, because from the ref's perspective the
    // actor only blips down and back up.
    // -----------------------------------------------------------------------
    println!("→ phase 2: forcing monitor panic ...");
    monitor.tell(ForcePanic).await?;

    // Give the supervision tree a moment to run the restart factory.
    // (Empirically completes in ~100us; 200ms is safe.)
    tokio::time::sleep(Duration::from_millis(200)).await;

    // -----------------------------------------------------------------------
    // Phase 3a: supervisor is still alive (a panic in the child should
    // NOT take down the parent).
    // -----------------------------------------------------------------------
    supervisor.ask(Ping).await?;
    println!("✓ phase 3a: supervisor responded to Ping (parent survived the panic)");

    // -----------------------------------------------------------------------
    // Phase 3b: the *same* monitor ref now works again — the supervisor
    // restarted the underlying actor in place. State is reset (history
    // empty again) because restart spawns a fresh actor with the same id.
    // -----------------------------------------------------------------------
    monitor
        .tell(Tick::new(pair.clone(), 1.1100, 1.1104, 1_000))
        .await?;
    let snap = monitor.ask(GetSnapshot).await?;
    println!(
        "✓ phase 3b: restarted monitor accepts ticks (history len = {}, latest mid = {:.5})",
        snap.history.len(),
        snap.latest.unwrap().mid,
    );
    assert_eq!(
        snap.history.len(),
        1,
        "post-restart history should hold only the new tick (state was reset)"
    );

    // -----------------------------------------------------------------------
    // Shutdown
    // -----------------------------------------------------------------------
    let _ = supervisor.stop_gracefully().await;
    supervisor.wait_for_shutdown().await;
    let _ = tick_bus.stop_gracefully().await;
    tick_bus.wait_for_shutdown().await;
    Ok(())
}
