//! `AlertSink` — terminal subscriber on the alert pubsub.
//!
//! Stores every received opportunity for later inspection (used by the
//! integration test) and logs them at INFO. In a real deployment this
//! actor would be replaced by something that submits orders, alerts an
//! ops channel, etc.

use kameo::error::Infallible;
use kameo::prelude::*;

use crate::market::ArbitrageOpportunity;

// ---------------------------------------------------------------------------
// Actor
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct AlertSink {
    alerts: Vec<ArbitrageOpportunity>,
}

impl Actor for AlertSink {
    type Args = Self;
    type Error = Infallible;

    async fn on_start(state: Self::Args, _actor_ref: ActorRef<Self>) -> Result<Self, Self::Error> {
        tracing::info!("AlertSink started");
        Ok(state)
    }
}

// ---------------------------------------------------------------------------
// Message: ArbitrageOpportunity (subscribed via alert pubsub)
// ---------------------------------------------------------------------------

impl Message<ArbitrageOpportunity> for AlertSink {
    type Reply = ();

    async fn handle(
        &mut self,
        opp: ArbitrageOpportunity,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        tracing::info!(opportunity = %opp, "AlertSink: opportunity received");
        self.alerts.push(opp);
    }
}

// ---------------------------------------------------------------------------
// Message: GetAlerts — drain the captured alerts
// ---------------------------------------------------------------------------

pub struct GetAlerts;

impl Message<GetAlerts> for AlertSink {
    type Reply = Vec<ArbitrageOpportunity>;

    async fn handle(
        &mut self,
        _: GetAlerts,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.alerts.clone()
    }
}

/// Ask that returns and clears the stored alerts in one shot.
pub struct DrainAlerts;

impl Message<DrainAlerts> for AlertSink {
    type Reply = Vec<ArbitrageOpportunity>;

    async fn handle(
        &mut self,
        _: DrainAlerts,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        std::mem::take(&mut self.alerts)
    }
}

// ---------------------------------------------------------------------------
// Message: Ping — flush primitive for tests
// ---------------------------------------------------------------------------

pub struct Ping;

impl Message<Ping> for AlertSink {
    type Reply = ();
    async fn handle(&mut self, _: Ping, _ctx: &mut Context<Self, Self::Reply>) -> Self::Reply {}
}
