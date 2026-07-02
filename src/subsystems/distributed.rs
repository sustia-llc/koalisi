//! Remote alert gateway over raw libp2p `request-response`.
//!
//! Layered ON TOP of the existing local-broadcast swarm: a plain tokio task
//! subscribes to the swarm's `broadcast::Receiver<ArbitrageOpportunity>`,
//! buffers the alerts, and serves them to remote peers that speak the
//! `/koalisi/alerts/1` request-response protocol (discovered via mDNS).
//!
//! This is the *publish-to-outside-world* boundary. The hot path
//! (monitor → coordinator → sink) stays on the local `tokio::sync::broadcast`
//! buses — see CLAUDE.md §"Worth flagging" for why the libp2p layer is
//! intentionally NOT used for intra-swarm messaging. Local `ask`/broadcast is
//! sub-µs; a request-response round-trip adds CBOR encode/decode + libp2p over
//! yamux + noise + network I/O, sized for the network, not for the hot loop.
//!
//! ## Wire protocol
//!
//! The service identity is the protocol name [`PROTOCOL_NAME`] — there is no
//! actor registry / name lookup. Any peer speaking the protocol answers.
//!
//! - [`AlertRequest::Poll`]      → [`AlertResponse::Alerts`] (a *clone* of the
//!   buffer; does NOT drain, so peers can poll repeatedly).
//! - [`AlertRequest::PeekCount`] → [`AlertResponse::Count`] (buffer length).
//! - [`AlertRequest::Clear`]     → [`AlertResponse::Cleared`] (drops the buffer).
//!
//! ## Lifecycle
//!
//! [`enable_remote_alerts`] is the producer-side entry point. It builds a
//! libp2p swarm (TCP + noise + yamux transport, mDNS + request-response
//! behaviour), listens, captures the bound address, and spawns the gateway
//! task on `swarm.task_tracker()` under a child of `swarm.cancellation_token()`
//! so it participates in `Swarm::shutdown()`. The gateway task owns the alert
//! buffer and `tokio::select!`s over (a) the alert-bus receiver, (b) libp2p
//! swarm events, and (c) cancellation.
//!
//! [`RemoteAlertClient`] is the consumer-side counterpart: build a swarm,
//! discover a gateway (via mDNS or an explicit dial), send an [`AlertRequest`],
//! await the [`AlertResponse`] bounded by a timeout.

use std::collections::HashSet;
use std::time::Duration;

use anyhow::{Result, anyhow};
use futures::StreamExt;
use libp2p::{
    Multiaddr, PeerId, StreamProtocol, mdns, noise,
    request_response::{self, ProtocolSupport},
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux,
};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::market::ArbitrageOpportunity;
use crate::subsystems::swarm::Swarm;

/// The request-response protocol name. Doubles as the service identity: any
/// peer speaking this protocol answers alert requests.
pub const PROTOCOL_NAME: &str = "/koalisi/alerts/1";

/// Default request-response timeout, sized for the network (not the hot path).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Idle connection timeout — generous so a lagging poller doesn't drop the link.
const IDLE_TIMEOUT: Duration = Duration::from_secs(300);

// ---------------------------------------------------------------------------
// Wire types
// ---------------------------------------------------------------------------

/// A request from a remote consumer to the alert gateway.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum AlertRequest {
    /// Return a clone of every buffered opportunity (does NOT drain).
    Poll,
    /// Return the current buffer length without copying the payload.
    PeekCount,
    /// Drop every buffered opportunity.
    Clear,
}

/// The gateway's reply to an [`AlertRequest`].
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum AlertResponse {
    /// Reply to [`AlertRequest::Poll`].
    Alerts(Vec<ArbitrageOpportunity>),
    /// Reply to [`AlertRequest::PeekCount`].
    Count(usize),
    /// Reply to [`AlertRequest::Clear`].
    Cleared,
}

// ---------------------------------------------------------------------------
// libp2p NetworkBehaviour
// ---------------------------------------------------------------------------

/// mDNS discovery composed with the CBOR request-response protocol. Shared by
/// both the producer gateway and the [`RemoteAlertClient`].
#[derive(NetworkBehaviour)]
struct GatewayBehaviour {
    mdns: mdns::tokio::Behaviour,
    rr: request_response::cbor::Behaviour<AlertRequest, AlertResponse>,
}

/// Build the shared libp2p swarm (TCP + noise + yamux, mDNS + request-response).
fn build_swarm(enable_mdns: bool) -> Result<libp2p::Swarm<GatewayBehaviour>> {
    let swarm = libp2p::SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_behaviour(|key| {
            let local_peer_id = key.public().to_peer_id();
            // mDNS is always wired in (the derive(NetworkBehaviour) shape is
            // fixed); when disabled we feed it a query interval so long that
            // discovery effectively never fires (used by loopback-only tests).
            let mdns_cfg = if enable_mdns {
                mdns::Config::default()
            } else {
                mdns::Config {
                    enable_ipv6: false,
                    query_interval: Duration::from_secs(86_400),
                    ..mdns::Config::default()
                }
            };
            let mdns = mdns::tokio::Behaviour::new(mdns_cfg, local_peer_id)?;
            let rr = request_response::cbor::Behaviour::<AlertRequest, AlertResponse>::new(
                [(StreamProtocol::new(PROTOCOL_NAME), ProtocolSupport::Full)],
                request_response::Config::default().with_request_timeout(REQUEST_TIMEOUT),
            );
            Ok(GatewayBehaviour { mdns, rr })
        })?
        .with_swarm_config(|c| c.with_idle_connection_timeout(IDLE_TIMEOUT))
        .build();
    Ok(swarm)
}

// ---------------------------------------------------------------------------
// Config + handle
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct RemoteConfig {
    /// Multiaddr to listen on. Use `"/ip4/0.0.0.0/tcp/0"` for an ephemeral
    /// OS-assigned port (the default).
    pub listen_addr: String,
    /// Whether to enable mDNS for local-network peer discovery. Disable for
    /// loopback tests that dial an explicit address (avoids surprise
    /// discoveries across concurrent test processes).
    pub enable_mdns: bool,
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            listen_addr: "/ip4/0.0.0.0/tcp/0".to_string(),
            enable_mdns: true,
        }
    }
}

/// Handle returned by [`enable_remote_alerts`]. Carries the libp2p peer id
/// (useful for skip-self filtering) and the bound listen addresses (so an
/// in-process or same-host consumer can dial the gateway without mDNS).
pub struct RemoteHandle {
    pub local_peer_id: PeerId,
    pub listen_addrs: Vec<Multiaddr>,
}

// ---------------------------------------------------------------------------
// enable_remote_alerts (producer side)
// ---------------------------------------------------------------------------

/// Set up the remote alert gateway. See module docs for the full lifecycle.
///
/// Builds a libp2p swarm, listens on `config.listen_addr`, captures the bound
/// address, and spawns the gateway task on the swarm's `TaskTracker` under a
/// child cancellation token. Returns a [`RemoteHandle`] with the peer id and
/// bound addresses.
///
/// Unlike the former actor-registry-backed gateway, this may be called more than once
/// per process: there is no process-wide registry to claim, so multiple
/// gateways / clients can coexist in a single test binary.
pub async fn enable_remote_alerts(swarm: &Swarm, config: RemoteConfig) -> Result<RemoteHandle> {
    let mut libp2p_swarm = build_swarm(config.enable_mdns)?;
    libp2p_swarm.listen_on(config.listen_addr.parse()?)?;

    // Pump the swarm until the first listen address is bound, so the handle can
    // hand a concrete dial target to same-host consumers.
    let listen_addrs = first_listen_addr(&mut libp2p_swarm, Duration::from_secs(5)).await?;
    let local_peer_id = *libp2p_swarm.local_peer_id();

    tracing::info!(
        peer_id = %local_peer_id,
        ?listen_addrs,
        mdns = config.enable_mdns,
        "remote alert gateway listening"
    );

    // Subscribe to the alert bus and hand ownership to the gateway task. The
    // task consumes the receiver directly (no intermediate bridge actor).
    let alert_rx = swarm.alert_bus().subscribe();
    let token = swarm.cancellation_token().child_token();
    swarm
        .task_tracker()
        .spawn(run_gateway(libp2p_swarm, alert_rx, token));

    Ok(RemoteHandle {
        local_peer_id,
        listen_addrs,
    })
}

/// Pump the swarm until the first `NewListenAddr` (bounded by `timeout`).
async fn first_listen_addr(
    swarm: &mut libp2p::Swarm<GatewayBehaviour>,
    timeout: Duration,
) -> Result<Vec<Multiaddr>> {
    let addr = tokio::time::timeout(timeout, async {
        loop {
            if let SwarmEvent::NewListenAddr { address, .. } = swarm.select_next_some().await {
                return address;
            }
        }
    })
    .await
    .map_err(|_| anyhow!("timed out waiting for libp2p listen address"))?;
    Ok(vec![addr])
}

/// Compute the gateway reply for a request, mutating the buffer for `Clear`.
fn respond(buffer: &mut Vec<ArbitrageOpportunity>, request: AlertRequest) -> AlertResponse {
    match request {
        AlertRequest::Poll => AlertResponse::Alerts(buffer.clone()),
        AlertRequest::PeekCount => AlertResponse::Count(buffer.len()),
        AlertRequest::Clear => {
            buffer.clear();
            AlertResponse::Cleared
        }
    }
}

/// The producer-side gateway event loop. Owns the alert buffer and serves it to
/// remote peers over request-response.
async fn run_gateway(
    mut swarm: libp2p::Swarm<GatewayBehaviour>,
    mut alert_rx: broadcast::Receiver<ArbitrageOpportunity>,
    token: CancellationToken,
) {
    let mut buffer: Vec<ArbitrageOpportunity> = Vec::new();
    loop {
        tokio::select! {
            biased;
            _ = token.cancelled() => {
                tracing::info!("remote alert gateway cancelled");
                return;
            }
            recv = alert_rx.recv() => match recv {
                Ok(opp) => {
                    tracing::debug!(
                        edge_bps = opp.edge_bps,
                        "remote gateway buffered opportunity for remote peers"
                    );
                    buffer.push(opp);
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(skipped = n, "remote gateway lagged on alert broadcast");
                }
                Err(broadcast::error::RecvError::Closed) => {
                    tracing::info!("alert bus closed; remote gateway stopping");
                    return;
                }
            },
            event = swarm.select_next_some() => match event {
                SwarmEvent::Behaviour(GatewayBehaviourEvent::Mdns(mdns::Event::Discovered(list))) => {
                    for (peer_id, multiaddr) in list {
                        tracing::info!(peer = %peer_id, addr = %multiaddr, "mDNS discovered peer");
                        swarm.add_peer_address(peer_id, multiaddr);
                    }
                }
                SwarmEvent::Behaviour(GatewayBehaviourEvent::Mdns(mdns::Event::Expired(list))) => {
                    for (peer_id, _) in list {
                        tracing::debug!(peer = %peer_id, "mDNS peer expired");
                    }
                }
                SwarmEvent::Behaviour(GatewayBehaviourEvent::Rr(request_response::Event::Message {
                    peer,
                    message: request_response::Message::Request { request, channel, .. },
                    ..
                })) => {
                    tracing::debug!(peer = %peer, ?request, "remote gateway serving request");
                    let response = respond(&mut buffer, request);
                    if swarm.behaviour_mut().rr.send_response(channel, response).is_err() {
                        tracing::warn!(peer = %peer, "remote gateway response channel closed");
                    }
                }
                SwarmEvent::NewListenAddr { address, .. } => {
                    tracing::info!(addr = %address, "libp2p listening");
                }
                SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                    tracing::info!(peer = %peer_id, "libp2p connection established");
                }
                SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
                    tracing::info!(peer = %peer_id, cause = ?cause, "libp2p connection closed");
                }
                _ => {}
            }
        }
    }
}

// ---------------------------------------------------------------------------
// RemoteAlertClient (consumer side)
// ---------------------------------------------------------------------------

/// A consumer that discovers an alert gateway and pulls alerts over the wire.
///
/// Owns its own libp2p swarm; every method drives that swarm's event loop
/// inline (the client is single-threaded — no background task / channels).
/// Discover a gateway with [`discover`](Self::discover) (mDNS) or
/// [`dial`](Self::dial) (explicit address), then issue [`poll`](Self::poll) /
/// [`peek_count`](Self::peek_count) / [`clear`](Self::clear).
pub struct RemoteAlertClient {
    swarm: libp2p::Swarm<GatewayBehaviour>,
    local_peer_id: PeerId,
    connected: HashSet<PeerId>,
}

impl RemoteAlertClient {
    /// Build the client swarm and start listening (so mDNS can advertise it).
    pub async fn connect(config: RemoteConfig) -> Result<Self> {
        let mut swarm = build_swarm(config.enable_mdns)?;
        swarm.listen_on(config.listen_addr.parse()?)?;
        let local_peer_id = *swarm.local_peer_id();
        Ok(Self {
            swarm,
            local_peer_id,
            connected: HashSet::new(),
        })
    }

    /// This client's libp2p peer id (skip-self filtering).
    pub fn local_peer_id(&self) -> PeerId {
        self.local_peer_id
    }

    /// Peers this client currently has an open connection to.
    pub fn connected_peers(&self) -> Vec<PeerId> {
        self.connected.iter().copied().collect()
    }

    /// Dial an explicit gateway address; return its peer id once connected.
    pub async fn dial(&mut self, addr: Multiaddr, timeout: Duration) -> Result<PeerId> {
        self.swarm.dial(addr)?;
        let peer = tokio::time::timeout(timeout, async {
            loop {
                if let SwarmEvent::ConnectionEstablished { peer_id, .. } =
                    self.swarm.select_next_some().await
                {
                    return peer_id;
                }
            }
        })
        .await
        .map_err(|_| anyhow!("timed out dialing gateway"))?;
        self.connected.insert(peer);
        Ok(peer)
    }

    /// Wait for the first mDNS-discovered gateway peer, dial it, return its id.
    pub async fn discover(&mut self, timeout: Duration) -> Result<PeerId> {
        let peer = tokio::time::timeout(timeout, async {
            loop {
                match self.swarm.select_next_some().await {
                    SwarmEvent::Behaviour(GatewayBehaviourEvent::Mdns(
                        mdns::Event::Discovered(list),
                    )) => {
                        for (peer_id, addr) in list {
                            if peer_id == self.local_peer_id {
                                continue;
                            }
                            self.swarm.add_peer_address(peer_id, addr);
                            let _ = self.swarm.dial(peer_id);
                        }
                    }
                    SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                        return peer_id;
                    }
                    _ => {}
                }
            }
        })
        .await
        .map_err(|_| anyhow!("timed out discovering gateway via mDNS"))?;
        self.connected.insert(peer);
        Ok(peer)
    }

    /// Drive the swarm for up to `duration`, tracking connections and reacting
    /// to mDNS discoveries (add address + dial). Use between polls to keep the
    /// connection healthy and pick up new gateways.
    pub async fn pump(&mut self, duration: Duration) {
        let _ = tokio::time::timeout(duration, async {
            loop {
                match self.swarm.select_next_some().await {
                    SwarmEvent::Behaviour(GatewayBehaviourEvent::Mdns(
                        mdns::Event::Discovered(list),
                    )) => {
                        for (peer_id, addr) in list {
                            if peer_id == self.local_peer_id {
                                continue;
                            }
                            self.swarm.add_peer_address(peer_id, addr);
                            let _ = self.swarm.dial(peer_id);
                        }
                    }
                    SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                        self.connected.insert(peer_id);
                    }
                    SwarmEvent::ConnectionClosed { peer_id, .. } => {
                        self.connected.remove(&peer_id);
                    }
                    _ => {}
                }
            }
        })
        .await;
    }

    /// Send `request` to `peer` and await the reply, bounded by `timeout`.
    pub async fn request(
        &mut self,
        peer: PeerId,
        request: AlertRequest,
        timeout: Duration,
    ) -> Result<AlertResponse> {
        let req_id = self.swarm.behaviour_mut().rr.send_request(&peer, request);
        tokio::time::timeout(timeout, async {
            loop {
                match self.swarm.select_next_some().await {
                    SwarmEvent::Behaviour(GatewayBehaviourEvent::Rr(
                        request_response::Event::Message {
                            message:
                                request_response::Message::Response {
                                    request_id,
                                    response,
                                },
                            ..
                        },
                    )) if request_id == req_id => return Ok(response),
                    SwarmEvent::Behaviour(GatewayBehaviourEvent::Rr(
                        request_response::Event::OutboundFailure {
                            request_id, error, ..
                        },
                    )) if request_id == req_id => {
                        return Err(anyhow!("outbound request failed: {error}"));
                    }
                    SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                        self.connected.insert(peer_id);
                    }
                    SwarmEvent::ConnectionClosed { peer_id, .. } => {
                        self.connected.remove(&peer_id);
                    }
                    _ => {}
                }
            }
        })
        .await
        .map_err(|_| anyhow!("timed out waiting for gateway response"))?
    }

    /// Poll `peer` for all buffered opportunities (does not drain the gateway).
    pub async fn poll(
        &mut self,
        peer: PeerId,
        timeout: Duration,
    ) -> Result<Vec<ArbitrageOpportunity>> {
        match self.request(peer, AlertRequest::Poll, timeout).await? {
            AlertResponse::Alerts(alerts) => Ok(alerts),
            other => Err(anyhow!("unexpected response to Poll: {other:?}")),
        }
    }

    /// Ask `peer` for its current buffer length.
    pub async fn peek_count(&mut self, peer: PeerId, timeout: Duration) -> Result<usize> {
        match self.request(peer, AlertRequest::PeekCount, timeout).await? {
            AlertResponse::Count(n) => Ok(n),
            other => Err(anyhow!("unexpected response to PeekCount: {other:?}")),
        }
    }

    /// Ask `peer` to drop its buffer.
    pub async fn clear(&mut self, peer: PeerId, timeout: Duration) -> Result<()> {
        match self.request(peer, AlertRequest::Clear, timeout).await? {
            AlertResponse::Cleared => Ok(()),
            other => Err(anyhow!("unexpected response to Clear: {other:?}")),
        }
    }
}
