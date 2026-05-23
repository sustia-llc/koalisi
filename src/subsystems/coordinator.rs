//! `ArbitrageCoordinator` — the swarm's collective decision-maker.
//!
//! Subscribes to the tick pubsub; on every `TickUpdate`, refreshes its
//! per-pair quote map, walks all configured triangles, and publishes an
//! `ArbitrageOpportunity` to the alert pubsub whenever the synthetic-vs-
//! actual edge crosses the configured threshold.
//!
//! Per-triangle hysteresis prevents the same opportunity from re-firing on
//! every subsequent tick: once a triangle has reported, it must cross back
//! below the threshold before it can fire again.

use std::collections::HashMap;

use kameo::error::Infallible;
use kameo::prelude::*;
use kameo_actors::pubsub::{PubSub, Publish};

use crate::market::{
    ArbitrageOpportunity, Direction, Pair, Quote, TickUpdate, Triangle,
};

// ---------------------------------------------------------------------------
// Args
// ---------------------------------------------------------------------------

pub struct ArbitrageCoordinatorArgs {
    pub triangles: Vec<Triangle>,
    pub threshold_bps: f64,
    pub alert_bus: ActorRef<PubSub<ArbitrageOpportunity>>,
}

// ---------------------------------------------------------------------------
// Actor
// ---------------------------------------------------------------------------

pub struct ArbitrageCoordinator {
    triangles: Vec<Triangle>,
    threshold_bps: f64,
    alert_bus: ActorRef<PubSub<ArbitrageOpportunity>>,
    quotes: HashMap<Pair, Quote>,
    /// For each triangle (by index), whether it's currently in the "fired"
    /// state. We require the edge to drop back below the threshold before
    /// the triangle is allowed to fire again — simple hysteresis.
    fired: Vec<bool>,
}

impl Actor for ArbitrageCoordinator {
    type Args = ArbitrageCoordinatorArgs;
    type Error = Infallible;

    async fn on_start(
        args: Self::Args,
        _actor_ref: ActorRef<Self>,
    ) -> Result<Self, Self::Error> {
        tracing::info!(
            triangles = args.triangles.len(),
            threshold_bps = args.threshold_bps,
            "ArbitrageCoordinator started"
        );
        let fired = vec![false; args.triangles.len()];
        Ok(Self {
            triangles: args.triangles,
            threshold_bps: args.threshold_bps,
            alert_bus: args.alert_bus,
            quotes: HashMap::new(),
            fired,
        })
    }
}

// ---------------------------------------------------------------------------
// Message: TickUpdate (subscribed via tick pubsub)
// ---------------------------------------------------------------------------

impl Message<TickUpdate> for ArbitrageCoordinator {
    type Reply = ();

    async fn handle(
        &mut self,
        update: TickUpdate,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.quotes.insert(update.pair.clone(), update.quote);

        for idx in 0..self.triangles.len() {
            let triangle = self.triangles[idx].clone();
            let (Some(qa), Some(qb), Some(qc)) = (
                self.quotes.get(&triangle.leg_a).copied(),
                self.quotes.get(&triangle.leg_b).copied(),
                self.quotes.get(&triangle.cross).copied(),
            ) else {
                continue;
            };

            let synthetic = triangle.synthetic_mid(&qa, &qb);
            let edge_bps = triangle.edge_bps(&qa, &qb, &qc);
            let abs_edge = edge_bps.abs();

            // Hysteresis: must dip below threshold before re-firing.
            if !self.fired[idx] && abs_edge > self.threshold_bps {
                let direction = if edge_bps > 0.0 {
                    Direction::BuySyntheticSellActual
                } else {
                    Direction::SellSyntheticBuyActual
                };
                let detected_at_ms = qa
                    .timestamp_ms
                    .max(qb.timestamp_ms)
                    .max(qc.timestamp_ms);
                let opp = ArbitrageOpportunity {
                    triangle,
                    leg_a_quote: qa,
                    leg_b_quote: qb,
                    cross_quote: qc,
                    synthetic_mid: synthetic,
                    actual_mid: qc.mid,
                    edge_bps,
                    direction,
                    detected_at_ms,
                };
                tracing::info!(opportunity = %opp, "arbitrage opportunity detected");
                if let Err(err) = self.alert_bus.ask(Publish(opp)).await {
                    tracing::error!(error = %err, "failed to publish ArbitrageOpportunity");
                }
                self.fired[idx] = true;
            } else if self.fired[idx] && abs_edge <= self.threshold_bps {
                self.fired[idx] = false;
                tracing::debug!(triangle_idx = idx, "arbitrage edge re-entered band; rearming");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Message: Ping — flush primitive for tests
// ---------------------------------------------------------------------------

pub struct Ping;

impl Message<Ping> for ArbitrageCoordinator {
    type Reply = ();
    async fn handle(&mut self, _: Ping, _ctx: &mut Context<Self, Self::Reply>) -> Self::Reply {}
}

// ---------------------------------------------------------------------------
// Message: GetQuotes — introspection
// ---------------------------------------------------------------------------

pub struct GetQuotes;

impl Message<GetQuotes> for ArbitrageCoordinator {
    type Reply = HashMap<Pair, Quote>;
    async fn handle(
        &mut self,
        _: GetQuotes,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.quotes.clone()
    }
}
