//! `MarketMonitor` — a per-pair kameo actor that maintains a ring buffer of
//! historical ticks, tracks the latest quote, and publishes a `TickUpdate`
//! to the swarm's tick pubsub on every incoming tick.
//!
//! Monitors don't decide on opportunities; that's the coordinator's job.
//! They're the "scout" actors of the swarm — each owns one pair.

use std::collections::VecDeque;

use kameo::error::Infallible;
use kameo::prelude::*;
use kameo_actors::pubsub::{PubSub, Publish};

use crate::market::{Pair, Quote, Tick, TickUpdate};

// ---------------------------------------------------------------------------
// Args
// ---------------------------------------------------------------------------

/// Constructor args for `MarketMonitor`. We pass the tick pubsub in here
/// rather than wiring it up post-spawn — keeps the dependency explicit.
///
/// `Clone + Sync` so the supervised-spawn machinery can re-construct the
/// actor on restart with the same args (see `examples/supervised_swarm.rs`).
#[derive(Clone)]
pub struct MarketMonitorArgs {
    pub pair: Pair,
    pub history_capacity: usize,
    pub tick_bus: ActorRef<PubSub<TickUpdate>>,
}

// ---------------------------------------------------------------------------
// Actor
// ---------------------------------------------------------------------------

pub struct MarketMonitor {
    pair: Pair,
    history: VecDeque<Tick>,
    history_capacity: usize,
    latest: Option<Quote>,
    tick_bus: ActorRef<PubSub<TickUpdate>>,
}

impl Actor for MarketMonitor {
    type Args = MarketMonitorArgs;
    type Error = Infallible;

    async fn on_start(args: Self::Args, _actor_ref: ActorRef<Self>) -> Result<Self, Self::Error> {
        tracing::info!(pair = %args.pair, "MarketMonitor started");
        Ok(Self {
            pair: args.pair,
            history: VecDeque::with_capacity(args.history_capacity),
            history_capacity: args.history_capacity,
            latest: None,
            tick_bus: args.tick_bus,
        })
    }
}

// ---------------------------------------------------------------------------
// Message: Tick — primary feed, both historical and live
// ---------------------------------------------------------------------------

impl Message<Tick> for MarketMonitor {
    type Reply = ();

    async fn handle(&mut self, tick: Tick, _ctx: &mut Context<Self, Self::Reply>) -> Self::Reply {
        if tick.pair != self.pair {
            tracing::warn!(
                expected = %self.pair,
                got = %tick.pair,
                "MarketMonitor received tick for the wrong pair; dropping"
            );
            return;
        }

        // Ring-buffer push.
        if self.history.len() == self.history_capacity {
            self.history.pop_front();
        }
        let quote = tick.quote();
        self.history.push_back(tick.clone());
        self.latest = Some(quote);

        // Publish to the tick pubsub. We use `ask` (not `tell`) so the
        // sequence "monitor accepts tick → coordinator has TickUpdate in
        // its mailbox" is FIFO-deterministic; this is what makes the
        // integration test's flushing strategy work without sleeps.
        let update = TickUpdate {
            pair: self.pair.clone(),
            quote,
        };
        if let Err(err) = self.tick_bus.ask(Publish(update)).await {
            tracing::error!(pair = %self.pair, error = %err, "failed to publish TickUpdate");
        }
    }
}

// ---------------------------------------------------------------------------
// Message: GetSnapshot — testing / introspection
// ---------------------------------------------------------------------------

/// Ask-only message to read the monitor's current state. Returns the latest
/// quote (if any) and a snapshot of the history ring buffer.
pub struct GetSnapshot;

#[derive(Debug, Clone, Reply)]
pub struct MonitorSnapshot {
    pub pair: Pair,
    pub latest: Option<Quote>,
    pub history: Vec<Tick>,
}

impl Message<GetSnapshot> for MarketMonitor {
    type Reply = MonitorSnapshot;

    async fn handle(
        &mut self,
        _: GetSnapshot,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        MonitorSnapshot {
            pair: self.pair.clone(),
            latest: self.latest,
            history: self.history.iter().cloned().collect(),
        }
    }
}

// ---------------------------------------------------------------------------
// Message: Ping — flush primitive for tests
// ---------------------------------------------------------------------------

/// A zero-state ask used by tests to deterministically flush the monitor's
/// mailbox (the reply is sent after all prior messages have been processed).
pub struct Ping;

impl Message<Ping> for MarketMonitor {
    type Reply = ();
    async fn handle(&mut self, _: Ping, _ctx: &mut Context<Self, Self::Reply>) -> Self::Reply {}
}
