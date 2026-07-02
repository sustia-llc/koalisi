//! `AlertSink` — terminal subscriber on the alert broadcast.
//!
//! Stores every received opportunity for later inspection (used by the
//! integration test) and logs them at INFO. In a real deployment this worker
//! would be replaced by something that submits orders, alerts an ops channel,
//! etc.
//!
//! ## Runtime shape (post-K3, issue #6)
//!
//! A plain [`AlertSink`] state struct driven by a spawned task that consumes
//! `ArbitrageOpportunity` values from a `tokio::sync::broadcast::Receiver` and
//! answers `Ping`/`GetAlerts`/`DrainAlerts` over a `tokio::sync::mpsc` command
//! channel. As with the coordinator, command handlers first drain all buffered
//! alerts so `ping()` / `get_alerts()` are deterministic flush points.

use anyhow::{Result, anyhow};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::market::ArbitrageOpportunity;

const COMMAND_CAPACITY: usize = 256;

// ---------------------------------------------------------------------------
// State core
// ---------------------------------------------------------------------------

/// The sink's captured alerts.
#[derive(Default)]
pub struct AlertSink {
    alerts: Vec<ArbitrageOpportunity>,
}

impl AlertSink {
    /// Ingest one opportunity.
    pub fn ingest(&mut self, opp: ArbitrageOpportunity) {
        tracing::info!(opportunity = %opp, "AlertSink: opportunity received");
        self.alerts.push(opp);
    }

    /// Clone the captured alerts (does not clear).
    pub fn get(&self) -> Vec<ArbitrageOpportunity> {
        self.alerts.clone()
    }

    /// Take + clear the captured alerts.
    pub fn drain(&mut self) -> Vec<ArbitrageOpportunity> {
        std::mem::take(&mut self.alerts)
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

enum SinkCommand {
    Ping(oneshot::Sender<()>),
    GetAlerts(oneshot::Sender<Vec<ArbitrageOpportunity>>),
    DrainAlerts(oneshot::Sender<Vec<ArbitrageOpportunity>>),
}

// ---------------------------------------------------------------------------
// Handle
// ---------------------------------------------------------------------------

/// Clone-able handle to a spawned sink task.
#[derive(Clone)]
pub struct SinkHandle {
    tx: mpsc::Sender<SinkCommand>,
}

impl SinkHandle {
    /// Flush barrier: returns after all buffered alerts have been ingested.
    pub async fn ping(&self) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(SinkCommand::Ping(tx))
            .await
            .map_err(|_| anyhow!("sink task is gone"))?;
        rx.await.map_err(|_| anyhow!("sink task dropped ping"))
    }

    /// Snapshot the captured alerts (implicitly drains pending ones first).
    pub async fn get_alerts(&self) -> Result<Vec<ArbitrageOpportunity>> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(SinkCommand::GetAlerts(tx))
            .await
            .map_err(|_| anyhow!("sink task is gone"))?;
        rx.await.map_err(|_| anyhow!("sink task dropped reply"))
    }

    /// Take + clear the captured alerts (implicitly drains pending ones first).
    pub async fn drain_alerts(&self) -> Result<Vec<ArbitrageOpportunity>> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(SinkCommand::DrainAlerts(tx))
            .await
            .map_err(|_| anyhow!("sink task is gone"))?;
        rx.await.map_err(|_| anyhow!("sink task dropped reply"))
    }
}

// ---------------------------------------------------------------------------
// Spawn
// ---------------------------------------------------------------------------

/// Spawn a sink task on `tracker` under a child of `token`, consuming alerts
/// from `alert_rx`.
pub fn spawn_sink(
    tracker: &TaskTracker,
    token: CancellationToken,
    alert_rx: broadcast::Receiver<ArbitrageOpportunity>,
) -> SinkHandle {
    let (tx, cmd_rx) = mpsc::channel(COMMAND_CAPACITY);
    tracker.spawn(sink_loop(cmd_rx, alert_rx, token, AlertSink::default()));
    SinkHandle { tx }
}

/// Drain every buffered alert into `sink`. Returns `false` once the broadcast
/// is closed so the caller can stop polling it.
fn drain_alerts(rx: &mut broadcast::Receiver<ArbitrageOpportunity>, sink: &mut AlertSink) -> bool {
    use broadcast::error::TryRecvError;
    loop {
        match rx.try_recv() {
            Ok(opp) => sink.ingest(opp),
            Err(TryRecvError::Empty) => return true,
            Err(TryRecvError::Lagged(n)) => {
                tracing::warn!(skipped = n, "sink lagged on alert broadcast");
            }
            Err(TryRecvError::Closed) => return false,
        }
    }
}

async fn sink_loop(
    mut cmd_rx: mpsc::Receiver<SinkCommand>,
    mut alert_rx: broadcast::Receiver<ArbitrageOpportunity>,
    token: CancellationToken,
    mut sink: AlertSink,
) {
    tracing::info!("AlertSink started");
    let mut alerts_open = true;
    loop {
        tokio::select! {
            biased;
            _ = token.cancelled() => break,
            cmd = cmd_rx.recv() => match cmd {
                Some(SinkCommand::Ping(reply)) => {
                    if alerts_open && !drain_alerts(&mut alert_rx, &mut sink) {
                        alerts_open = false;
                    }
                    let _ = reply.send(());
                }
                Some(SinkCommand::GetAlerts(reply)) => {
                    if alerts_open && !drain_alerts(&mut alert_rx, &mut sink) {
                        alerts_open = false;
                    }
                    let _ = reply.send(sink.get());
                }
                Some(SinkCommand::DrainAlerts(reply)) => {
                    if alerts_open && !drain_alerts(&mut alert_rx, &mut sink) {
                        alerts_open = false;
                    }
                    let _ = reply.send(sink.drain());
                }
                None => break,
            },
            recv = alert_rx.recv(), if alerts_open => match recv {
                Ok(opp) => sink.ingest(opp),
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(skipped = n, "sink lagged on alert broadcast");
                }
                Err(broadcast::error::RecvError::Closed) => {
                    alerts_open = false;
                }
            },
        }
    }
    tracing::debug!("AlertSink stopped");
}
