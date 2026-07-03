//! `ArbitrageCoordinator` — the swarm's collective decision-maker.
//!
//! Subscribes to the tick broadcast; on every `TickUpdate`, refreshes its
//! per-pair quote map, walks all configured triangles, and publishes an
//! `ArbitrageOpportunity` to the alert broadcast whenever the synthetic-vs-
//! actual edge crosses the configured threshold.
//!
//! Per-triangle hysteresis prevents the same opportunity from re-firing on
//! every subsequent tick: once a triangle has reported, it must cross back
//! below the threshold before it can fire again.
//!
//! ## Runtime shape (post-K3, issue #6)
//!
//! The coordinator is a plain [`ArbitrageCoordinator`] state struct driven by a
//! spawned task. The task selects over a `tokio::sync::broadcast::Receiver`
//! (ticks from the monitors) and a `tokio::sync::mpsc` command channel
//! (`Ping`, `GetQuotes`). Because ticks and commands arrive on separate
//! channels, a `Ping`/`GetQuotes` handler first *drains* all currently-buffered
//! ticks before replying — that makes `ping()` a true flush barrier (every tick
//! sent before the ping is processed before the reply), reproducing the old
//! actor FIFO-mailbox guarantee across the two channels.

use std::collections::HashMap;

use anyhow::{Result, anyhow};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::market::{ArbitrageOpportunity, Direction, Pair, Quote, TickUpdate, Triangle};

const COMMAND_CAPACITY: usize = 256;

// ---------------------------------------------------------------------------
// State core
// ---------------------------------------------------------------------------

/// The coordinator's mutable state. Kept separate from the task loop for
/// testability.
pub struct ArbitrageCoordinator {
    triangles: Vec<Triangle>,
    threshold_bps: f64,
    quotes: HashMap<Pair, Quote>,
    /// For each triangle (by index), whether it's currently in the "fired"
    /// state. We require the edge to drop back below the threshold before the
    /// triangle is allowed to fire again — simple hysteresis.
    fired: Vec<bool>,
}

impl ArbitrageCoordinator {
    /// Build a coordinator over `triangles` firing above `threshold_bps`.
    pub fn new(triangles: Vec<Triangle>, threshold_bps: f64) -> Self {
        let fired = vec![false; triangles.len()];
        Self {
            triangles,
            threshold_bps,
            quotes: HashMap::new(),
            fired,
        }
    }

    /// Ingest a `TickUpdate`, returning any arbitrage opportunities that fire
    /// as a result (usually zero or one). Preserves the exact firing +
    /// hysteresis semantics of the original actor.
    pub fn ingest(&mut self, update: &TickUpdate) -> Vec<ArbitrageOpportunity> {
        self.quotes.insert(update.key.clone(), update.view);

        let mut opps = Vec::new();
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
                let detected_at_ms = qa.timestamp_ms.max(qb.timestamp_ms).max(qc.timestamp_ms);
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
                opps.push(opp);
                self.fired[idx] = true;
            } else if self.fired[idx] && abs_edge <= self.threshold_bps {
                self.fired[idx] = false;
                tracing::debug!(triangle_idx = idx, "arbitrage edge re-entered band; rearming");
            }
        }
        opps
    }

    /// Snapshot the current per-pair quote map.
    pub fn quotes(&self) -> HashMap<Pair, Quote> {
        self.quotes.clone()
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

enum CoordinatorCommand {
    Ping(oneshot::Sender<()>),
    GetQuotes(oneshot::Sender<HashMap<Pair, Quote>>),
}

// ---------------------------------------------------------------------------
// Handle
// ---------------------------------------------------------------------------

/// Clone-able handle to a spawned coordinator task.
#[derive(Clone)]
pub struct CoordinatorHandle {
    tx: mpsc::Sender<CoordinatorCommand>,
}

impl CoordinatorHandle {
    /// Flush barrier: returns after the coordinator has drained every tick
    /// buffered so far and processed all prior commands.
    pub async fn ping(&self) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(CoordinatorCommand::Ping(tx))
            .await
            .map_err(|_| anyhow!("coordinator task is gone"))?;
        rx.await.map_err(|_| anyhow!("coordinator task dropped ping"))
    }

    /// Read the current per-pair quote map (implicitly drains pending ticks).
    pub async fn quotes(&self) -> Result<HashMap<Pair, Quote>> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(CoordinatorCommand::GetQuotes(tx))
            .await
            .map_err(|_| anyhow!("coordinator task is gone"))?;
        rx.await.map_err(|_| anyhow!("coordinator task dropped reply"))
    }
}

// ---------------------------------------------------------------------------
// Spawn
// ---------------------------------------------------------------------------

/// Spawn a coordinator task on `tracker` under a child of `token`. It consumes
/// `TickUpdate`s from `tick_rx` and publishes alerts to `alert_bus`.
pub fn spawn_coordinator(
    tracker: &TaskTracker,
    token: CancellationToken,
    triangles: Vec<Triangle>,
    threshold_bps: f64,
    tick_rx: broadcast::Receiver<TickUpdate>,
    alert_bus: broadcast::Sender<ArbitrageOpportunity>,
) -> CoordinatorHandle {
    let (tx, cmd_rx) = mpsc::channel(COMMAND_CAPACITY);
    let coordinator = ArbitrageCoordinator::new(triangles, threshold_bps);
    tracker.spawn(coordinator_loop(cmd_rx, tick_rx, token, coordinator, alert_bus));
    CoordinatorHandle { tx }
}

/// Drain every currently-buffered tick from `tick_rx`, ingesting each and
/// publishing any resulting alerts. Returns `false` once the broadcast is
/// closed (all senders dropped) so the caller can stop polling it.
fn drain_ticks(
    tick_rx: &mut broadcast::Receiver<TickUpdate>,
    coordinator: &mut ArbitrageCoordinator,
    alert_bus: &broadcast::Sender<ArbitrageOpportunity>,
) -> bool {
    use broadcast::error::TryRecvError;
    loop {
        match tick_rx.try_recv() {
            Ok(update) => {
                for opp in coordinator.ingest(&update) {
                    let _ = alert_bus.send(opp);
                }
            }
            Err(TryRecvError::Empty) => return true,
            Err(TryRecvError::Lagged(n)) => {
                tracing::warn!(skipped = n, "coordinator lagged on tick broadcast");
            }
            Err(TryRecvError::Closed) => return false,
        }
    }
}

async fn coordinator_loop(
    mut cmd_rx: mpsc::Receiver<CoordinatorCommand>,
    mut tick_rx: broadcast::Receiver<TickUpdate>,
    token: CancellationToken,
    mut coordinator: ArbitrageCoordinator,
    alert_bus: broadcast::Sender<ArbitrageOpportunity>,
) {
    tracing::info!(
        triangles = coordinator.triangles.len(),
        threshold_bps = coordinator.threshold_bps,
        "ArbitrageCoordinator started"
    );
    // Once the tick broadcast closes we stop selecting on it to avoid a busy
    // spin on `Closed`.
    let mut tick_open = true;
    loop {
        tokio::select! {
            biased;
            _ = token.cancelled() => break,
            cmd = cmd_rx.recv() => match cmd {
                Some(CoordinatorCommand::Ping(reply)) => {
                    // Drain first so ping is a true flush barrier.
                    if tick_open && !drain_ticks(&mut tick_rx, &mut coordinator, &alert_bus) {
                        tick_open = false;
                    }
                    let _ = reply.send(());
                }
                Some(CoordinatorCommand::GetQuotes(reply)) => {
                    if tick_open && !drain_ticks(&mut tick_rx, &mut coordinator, &alert_bus) {
                        tick_open = false;
                    }
                    let _ = reply.send(coordinator.quotes());
                }
                None => break,
            },
            recv = tick_rx.recv(), if tick_open => match recv {
                Ok(update) => {
                    for opp in coordinator.ingest(&update) {
                        let _ = alert_bus.send(opp);
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(skipped = n, "coordinator lagged on tick broadcast");
                }
                Err(broadcast::error::RecvError::Closed) => {
                    tick_open = false;
                }
            },
        }
    }
    tracing::debug!("ArbitrageCoordinator stopped");
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::market::Tick;

    fn p(s: &str) -> Pair {
        s.parse().unwrap()
    }

    fn update(pair: &str, bid: f64, ask: f64, ts: i64) -> TickUpdate {
        let t = Tick::new(p(pair), bid, ask, ts);
        TickUpdate {
            key: t.pair.clone(),
            view: t.quote(),
        }
    }

    #[test]
    fn ingest_fires_once_then_rearms() {
        let tri = Triangle::new(p("EUR/USD"), p("GBP/USD"), p("EUR/GBP")).unwrap();
        let mut c = ArbitrageCoordinator::new(vec![tri], 10.0);
        let synthetic = 1.10 / 1.30;

        // Aligned bootstrap → no fire.
        assert!(c.ingest(&update("EUR/USD", 1.0998, 1.1002, 0)).is_empty());
        assert!(c.ingest(&update("GBP/USD", 1.2998, 1.3002, 0)).is_empty());
        assert!(
            c.ingest(&update("EUR/GBP", synthetic - 0.0001, synthetic + 0.0001, 0))
                .is_empty()
        );

        // Dislocate → exactly one alert.
        let opps = c.ingest(&update("EUR/GBP", 0.8499, 0.8501, 1_000));
        assert_eq!(opps.len(), 1);
        assert_eq!(opps[0].direction, Direction::BuySyntheticSellActual);
        assert_eq!(opps[0].detected_at_ms, 1_000);

        // Re-feed same dislocation → hysteresis suppresses.
        assert!(c.ingest(&update("EUR/GBP", 0.8499, 0.8501, 1_100)).is_empty());

        // Re-align → rearm (no fire).
        assert!(
            c.ingest(&update("EUR/GBP", synthetic - 0.0001, synthetic + 0.0001, 2_000))
                .is_empty()
        );
        // Flip the other way → second, opposite-sign alert.
        let opps = c.ingest(&update("EUR/GBP", 0.8400, 0.8402, 3_000));
        assert_eq!(opps.len(), 1);
        assert_eq!(opps[0].direction, Direction::SellSyntheticBuyActual);
        assert!(opps[0].edge_bps < -50.0);
    }

    #[tokio::test]
    async fn ping_drains_ticks_before_reply() {
        let tracker = TaskTracker::new();
        let token = CancellationToken::new();
        let (tick_bus, tick_rx) = broadcast::channel::<TickUpdate>(64);
        let (alert_bus, _alert_rx) = broadcast::channel::<ArbitrageOpportunity>(64);

        let tri = Triangle::new(p("EUR/USD"), p("GBP/USD"), p("EUR/GBP")).unwrap();
        let h = spawn_coordinator(
            &tracker,
            token.child_token(),
            vec![tri],
            10.0,
            tick_rx,
            alert_bus,
        );

        // Send updates *then* ping: the ping must observe all of them.
        tick_bus.send(update("EUR/USD", 1.0998, 1.1002, 0)).unwrap();
        tick_bus.send(update("GBP/USD", 1.2998, 1.3002, 0)).unwrap();
        tick_bus.send(update("EUR/GBP", 0.8499, 0.8501, 1)).unwrap();
        h.ping().await.unwrap();

        let quotes = h.quotes().await.unwrap();
        assert_eq!(quotes.len(), 3, "ping drained all three ticks");

        token.cancel();
        tracker.close();
        tracker.wait().await;
    }
}
