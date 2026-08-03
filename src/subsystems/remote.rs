//! Domain-neutral remote coalition-event gateway over raw libp2p
//! `request-response` (issue #38, feature `remote`).
//!
//! This is the **publish-to-outside-world boundary**, and only that. The hot
//! path — the [`CoalitionService`](crate::subsystems::coalition_actor::CoalitionService)
//! seam and every other intra-process seam — stays on `tokio::sync` channels,
//! which are sub-µs; a request-response round-trip adds CBOR encode/decode plus
//! libp2p over yamux + noise + network I/O, sized for the network, not for the
//! decision loop. Never put this layer on an intra-process path.
//!
//! ## What it publishes
//!
//! The source is the [`CoalitionService`]'s optional
//! [`DecisionRecord`] tap — the same feature-independent record the `durable`
//! decision log consumes. (The v0.10.0 ancestor of this module published forex
//! arbitrage alerts off a domain bus; that domain is gone, and the coalition
//! decision stream is the domain-neutral replacement.) When more than one
//! consumer wants the tap — say the durable log *and* this gateway — put a
//! [`spawn_decision_tee`](crate::subsystems::coalition_actor::spawn_decision_tee)
//! between them.
//!
//! [`DecisionRecord`] itself stays serde-free (it is core, feature-independent);
//! conversion to the versioned wire type [`RemoteCoalitionEventV1`] happens only
//! here, at the boundary — the same "wire projection, not serde on domain types"
//! discipline as P7.2's
//! [`WireTopologyEvent`](crate::persistence::WireTopologyEvent).
//!
//! ## Wire protocol
//!
//! The service identity is the protocol name [`PROTOCOL_NAME`] — there is no
//! registry / name lookup. Any peer speaking the protocol answers.
//!
//! - [`EventRequest::PollSince`] → [`EventResponse::Events`] — every buffered
//!   event with `seq > last_seq`, as a *clone*. Polling never drains, so many
//!   independent consumers can each hold their own cursor.
//! - [`EventRequest::Head`] → [`EventResponse::Head`] — the newest stamped
//!   sequence number and the current buffer length, without copying payloads.
//!
//! There is deliberately **no `Clear` request** (the v0.10.0 alert gateway had
//! one). A destructive request is unsound with more than one consumer: one
//! peer's `Clear` silently erases another peer's un-polled backlog. Eviction is
//! owned solely by [`RemoteConfig::buffer_cap`].
//!
//! A future *topology*-event gateway would be a second `request_response`
//! behaviour on the same swarm under its own protocol name
//! (`/koalisi/topology-events/1`), not a new variant here — the two streams have
//! independent schemas and independent version lifetimes.
//!
//! ## Sequence numbers and gap detection
//!
//! `seq` starts at 1 and is stamped by the gateway when it buffers a record. It
//! is monotonic and never reused. The buffer is capped
//! ([`RemoteConfig::buffer_cap`]) and evicts the OLDEST event on overflow, so a
//! lagging poller can never OOM the producer — but it *can* miss events. A
//! client detects that: after `poll_since(last_seq)`, if the first returned
//! event's `seq` is greater than `last_seq + 1`, the events in between were
//! evicted. [`EventResponse::Head`]'s `head_seq` gives the same information
//! without transferring payloads.
//!
//! ## Lifecycle
//!
//! [`enable_remote_gateway`] is the producer-side entry point. It builds a
//! libp2p swarm (TCP + noise + yamux transport, mDNS + request-response
//! behaviour), listens, captures the bound address, and spawns the gateway task
//! on the caller's [`TaskTracker`] under a child of the supplied
//! [`CancellationToken`], so it participates in the caller's three-step
//! shutdown. The gateway task owns the event buffer and `tokio::select!`s over
//! (a) cancellation, (b) the decision receiver, and (c) libp2p swarm events.
//!
//! Shutdown discipline (pick ONE; don't mix — the
//! [`outcome`](crate::subsystems::outcome) convention):
//!
//! - **Lossless drain**: drop every tap `Sender` upstream → `rx.recv()` yields
//!   `None` once buffered records drain → the gateway buffers them all, then
//!   exits. Nothing produced before the drop is lost from the buffer.
//! - **Prompt teardown**: cancel the token → the loop breaks immediately; a
//!   buffered-but-unread record is dropped, and an in-flight request may go
//!   unanswered.
//!
//! [`RemoteCoalitionClient`] is the consumer-side counterpart: build a swarm,
//! discover a gateway (via mDNS or an explicit dial), send an [`EventRequest`],
//! await the [`EventResponse`] bounded by a timeout.
//!
//! [`CoalitionService`]: crate::subsystems::coalition_actor::CoalitionService

use std::collections::{HashSet, VecDeque};
use std::time::Duration;

use anyhow::{Result, anyhow};
use futures::StreamExt;
use libp2p::{
    StreamProtocol, mdns, noise,
    request_response::{self, ProtocolSupport},
    swarm::{NetworkBehaviour, SwarmEvent, behaviour::toggle::Toggle},
    tcp, yamux,
};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::subsystems::coalition_actor::DecisionRecord;

/// Re-exported so consumers can name the gateway's peer/address types without
/// taking their own `libp2p` dependency (both appear in [`RemoteHandle`] and in
/// [`RemoteCoalitionClient`]'s signatures).
pub use libp2p::{Multiaddr, PeerId};

/// The request-response protocol name. Doubles as the service identity: any
/// peer speaking this protocol answers coalition-event requests.
///
/// A second stream (e.g. topology events) gets its OWN protocol name and its
/// own `request_response` behaviour on the same swarm — see the
/// [module docs](self).
pub const PROTOCOL_NAME: &str = "/koalisi/coalition-events/1";

/// Wire-schema version of [`RemoteCoalitionEventV1`]. Bump on any change to the
/// struct's shape; a consumer that sees a `schema_version` it does not
/// understand should refuse the record rather than guess.
pub const REMOTE_WIRE_SCHEMA_VERSION: u16 = 1;

/// Default request-response timeout, sized for the network (not the hot path).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Idle connection timeout — generous so a lagging poller doesn't drop the link.
const IDLE_TIMEOUT: Duration = Duration::from_secs(300);

// ---------------------------------------------------------------------------
// Wire types
// ---------------------------------------------------------------------------

/// Versioned wire projection of a
/// [`DecisionRecord`](crate::subsystems::coalition_actor::DecisionRecord).
///
/// Fields are raw scalars/strings only — the
/// [`WireTopologyEvent`](crate::persistence::WireTopologyEvent) precedent: the
/// in-memory record keeps its koalisi types and stays serde-free, and this
/// mirror is what crosses the network. `kind` is the stable lowercase label
/// from [`DecisionKind::as_str`](crate::subsystems::coalition_actor::DecisionKind::as_str)
/// (`"join"` / `"leave"`); `agent_id` is the topology `VertexIndex`'s inner
/// value widened to `u64` (as in the `durable` tier's `DecisionEvent`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct RemoteCoalitionEventV1 {
    /// Always [`REMOTE_WIRE_SCHEMA_VERSION`] when produced by this crate.
    pub schema_version: u16,
    /// Gateway-stamped sequence number: starts at 1, monotonic, never reused.
    pub seq: u64,
    /// Stable coalition label (`"coalition-<n>"`).
    pub coalition: String,
    /// Topology vertex index of the agent the decision was about.
    pub agent_id: u64,
    /// `"join"` or `"leave"`.
    pub kind: String,
    /// Whether the policy recommended acting.
    pub act: bool,
    /// The scalar the decision was made from.
    pub score: f64,
}

impl RemoteCoalitionEventV1 {
    /// Project a core [`DecisionRecord`] onto the wire at sequence `seq`.
    ///
    /// Inherent (not a `From` impl) on purpose: the sequence number is supplied
    /// by the gateway, not carried by the record — the same reason P7.2's wire
    /// conversions are inherent rather than `From`/`TryFrom`.
    #[must_use]
    pub fn from_record(seq: u64, record: &DecisionRecord) -> Self {
        Self {
            schema_version: REMOTE_WIRE_SCHEMA_VERSION,
            seq,
            coalition: record.coalition.clone(),
            // usize → u64: a topology vertex index always fits.
            agent_id: record.agent_id as u64,
            kind: record.kind.as_str().to_string(),
            act: record.act,
            score: record.score,
        }
    }
}

/// A request from a remote consumer to the coalition-event gateway.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum EventRequest {
    /// Return every buffered event with `seq > last_seq` (a clone; never
    /// drains). Pass `0` for "everything still buffered".
    PollSince {
        /// The consumer's cursor: the highest `seq` it has already seen.
        last_seq: u64,
    },
    /// Return the newest stamped sequence number and the buffer length, without
    /// copying payloads.
    Head,
}

/// The gateway's reply to an [`EventRequest`].
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum EventResponse {
    /// Reply to [`EventRequest::PollSince`].
    Events {
        /// `seq` of the newest buffered event, or `0` if none was ever buffered.
        head_seq: u64,
        /// The matching events, oldest first.
        events: Vec<RemoteCoalitionEventV1>,
    },
    /// Reply to [`EventRequest::Head`].
    Head {
        /// `seq` of the newest buffered event, or `0` if none was ever buffered.
        head_seq: u64,
        /// How many events the gateway currently holds.
        len: u64,
    },
}

// ---------------------------------------------------------------------------
// EventBuffer — the pure, testable core of the gateway
// ---------------------------------------------------------------------------

/// The gateway's bounded, sequence-stamped ring of published events.
///
/// Split out from the swarm loop so the sequencing / eviction / poll-delta
/// semantics are unit-testable without any libp2p involvement. Push order is
/// the tap's order; poll results are always oldest-first.
///
/// Capacity is clamped to at least 1: a zero `buffer_cap` would make `head_seq`
/// unobservable and every poll empty, which is indistinguishable from a broken
/// gateway. `EventBuffer::new(0)` therefore behaves as `new(1)` (and
/// [`enable_remote_gateway`] warns).
#[derive(Debug)]
pub struct EventBuffer {
    events: VecDeque<RemoteCoalitionEventV1>,
    cap: usize,
    /// The sequence number the NEXT pushed record will receive (starts at 1).
    next_seq: u64,
}

impl EventBuffer {
    /// Create a buffer holding at most `cap` events (clamped to `>= 1`).
    #[must_use]
    pub fn new(cap: usize) -> Self {
        let cap = cap.max(1);
        Self {
            events: VecDeque::with_capacity(cap),
            cap,
            next_seq: 1,
        }
    }

    /// Stamp `record` with the next sequence number and buffer it, evicting the
    /// oldest event if the buffer is at capacity. Returns the stamped `seq`.
    pub fn push_record(&mut self, record: &DecisionRecord) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        if self.events.len() == self.cap {
            self.events.pop_front();
        }
        self.events
            .push_back(RemoteCoalitionEventV1::from_record(seq, record));
        seq
    }

    /// `seq` of the newest buffered event, or `0` if nothing was ever buffered.
    #[must_use]
    pub fn head_seq(&self) -> u64 {
        self.next_seq.saturating_sub(1)
    }

    /// How many events are currently buffered.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether the buffer currently holds no events.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// Compute the reply for `request`. Read-only: no request drains or mutates
    /// the buffer (see the [module docs](self) on the dropped `Clear`).
    #[must_use]
    pub fn respond(&self, request: &EventRequest) -> EventResponse {
        match *request {
            EventRequest::PollSince { last_seq } => EventResponse::Events {
                head_seq: self.head_seq(),
                events: self
                    .events
                    .iter()
                    .filter(|e| e.seq > last_seq)
                    .cloned()
                    .collect(),
            },
            EventRequest::Head => EventResponse::Head {
                head_seq: self.head_seq(),
                len: self.events.len() as u64,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// libp2p NetworkBehaviour
// ---------------------------------------------------------------------------

/// Optional mDNS discovery composed with the CBOR request-response protocol.
/// Shared by both the producer gateway and the [`RemoteCoalitionClient`].
///
/// mDNS sits behind a [`Toggle`] rather than being merely quietened with a very
/// long query interval (what the v0.10.0 alert gateway did): a live
/// `mdns::Behaviour` still *receives* other peers' announcements regardless of
/// its own query cadence, so a "disabled" instance would keep discovering — and
/// adding addresses for — every koalisi peer on the LAN. `Toggle::from(None)`
/// is genuinely off, which is what loopback tests and deterministic examples
/// need.
#[derive(NetworkBehaviour)]
struct GatewayBehaviour {
    mdns: Toggle<mdns::tokio::Behaviour>,
    rr: request_response::cbor::Behaviour<EventRequest, EventResponse>,
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
            // Genuinely absent when disabled — see the `GatewayBehaviour` docs.
            let mdns = if enable_mdns {
                Toggle::from(Some(mdns::tokio::Behaviour::new(
                    mdns::Config::default(),
                    local_peer_id,
                )?))
            } else {
                Toggle::from(None)
            };
            let rr = request_response::cbor::Behaviour::<EventRequest, EventResponse>::new(
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

/// Gateway / client network configuration.
#[derive(Debug, Clone)]
pub struct RemoteConfig {
    /// Multiaddr to listen on. Use `"/ip4/0.0.0.0/tcp/0"` for an ephemeral
    /// OS-assigned port (the default).
    pub listen_addr: String,
    /// Whether to enable mDNS for local-network peer discovery. Disable for
    /// loopback tests/examples that dial an explicit address (avoids surprise
    /// discoveries across concurrent processes).
    pub enable_mdns: bool,
    /// How many published events the gateway retains. On overflow the OLDEST is
    /// evicted, so a lagging poller can never OOM the producer. Clamped to
    /// `>= 1` (see [`EventBuffer`]); ignored by the client side.
    pub buffer_cap: usize,
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            listen_addr: "/ip4/0.0.0.0/tcp/0".to_string(),
            enable_mdns: true,
            buffer_cap: 1024,
        }
    }
}

/// Handle returned by [`enable_remote_gateway`]. Carries the libp2p peer id
/// (useful for skip-self filtering) and the bound listen addresses (so an
/// in-process or same-host consumer can dial the gateway without mDNS).
#[derive(Debug, Clone)]
pub struct RemoteHandle {
    /// The gateway swarm's libp2p peer id.
    pub local_peer_id: PeerId,
    /// The addresses the gateway actually bound.
    pub listen_addrs: Vec<Multiaddr>,
}

// ---------------------------------------------------------------------------
// enable_remote_gateway (producer side)
// ---------------------------------------------------------------------------

/// Set up the remote coalition-event gateway. See the [module docs](self) for
/// the full lifecycle and shutdown disciplines.
///
/// Builds a libp2p swarm, listens on `config.listen_addr`, captures the bound
/// address, and spawns the gateway task on `tracker` under
/// `token.child_token()`. `rx` is the receiving end of a
/// [`DecisionRecord`](crate::subsystems::coalition_actor::DecisionRecord) tap
/// (directly the [`CoalitionService`](crate::subsystems::coalition_actor::CoalitionService)'s,
/// or one leg of a
/// [`spawn_decision_tee`](crate::subsystems::coalition_actor::spawn_decision_tee)).
///
/// There is no process-wide registry to claim, so this may be called more than
/// once per process: multiple gateways and clients coexist in a single test
/// binary.
///
/// # Errors
///
/// Returns an error if the swarm cannot be built, `config.listen_addr` is not a
/// valid multiaddr, the listener cannot bind, or no listen address is bound
/// within five seconds.
pub async fn enable_remote_gateway(
    tracker: &TaskTracker,
    token: CancellationToken,
    rx: mpsc::Receiver<DecisionRecord>,
    config: RemoteConfig,
) -> Result<RemoteHandle> {
    if config.buffer_cap == 0 {
        tracing::warn!("remote gateway buffer_cap = 0 is clamped to 1 (see EventBuffer docs)");
    }
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
        buffer_cap = config.buffer_cap,
        "remote coalition-event gateway listening"
    );

    let buffer = EventBuffer::new(config.buffer_cap);
    tracker.spawn(run_gateway(
        libp2p_swarm,
        rx,
        buffer,
        token.child_token(),
    ));

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

/// The producer-side gateway event loop. Owns the event buffer and serves it to
/// remote peers over request-response.
async fn run_gateway(
    mut swarm: libp2p::Swarm<GatewayBehaviour>,
    mut rx: mpsc::Receiver<DecisionRecord>,
    mut buffer: EventBuffer,
    token: CancellationToken,
) {
    loop {
        tokio::select! {
            biased;
            _ = token.cancelled() => {
                tracing::info!("remote coalition-event gateway cancelled");
                return;
            }
            maybe = rx.recv() => match maybe {
                Some(record) => {
                    let seq = buffer.push_record(&record);
                    tracing::debug!(
                        seq,
                        kind = record.kind.as_str(),
                        buffered = buffer.len(),
                        "remote gateway buffered coalition event"
                    );
                }
                None => {
                    tracing::info!("decision tap closed; remote coalition-event gateway stopping");
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
                    let response = buffer.respond(&request);
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
// RemoteCoalitionClient (consumer side)
// ---------------------------------------------------------------------------

/// A consumer that discovers a coalition-event gateway and pulls events over
/// the wire.
///
/// Owns its own libp2p swarm; every method drives that swarm's event loop
/// inline (the client is single-threaded — no background task / channels).
/// Discover a gateway with [`discover`](Self::discover) (mDNS) or
/// [`dial`](Self::dial) (explicit address), then issue
/// [`poll_since`](Self::poll_since) / [`head`](Self::head).
///
/// The cursor lives with the CALLER: hold the highest `seq` you have processed
/// and pass it to the next [`poll_since`](Self::poll_since). The gateway never
/// drains, so several clients can each keep independent cursors.
pub struct RemoteCoalitionClient {
    swarm: libp2p::Swarm<GatewayBehaviour>,
    local_peer_id: PeerId,
    connected: HashSet<PeerId>,
}

impl RemoteCoalitionClient {
    /// Build the client swarm and start listening (so mDNS can advertise it).
    ///
    /// # Errors
    ///
    /// Returns an error if the swarm cannot be built, or `config.listen_addr`
    /// is not a valid multiaddr / cannot be bound.
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
    #[must_use]
    pub fn local_peer_id(&self) -> PeerId {
        self.local_peer_id
    }

    /// Peers this client currently has an open connection to.
    #[must_use]
    pub fn connected_peers(&self) -> Vec<PeerId> {
        self.connected.iter().copied().collect()
    }

    /// Dial an explicit gateway address; return its peer id once connected.
    ///
    /// # Errors
    ///
    /// Returns an error if the dial cannot be initiated, or no connection is
    /// established within `timeout`.
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
    ///
    /// # Errors
    ///
    /// Returns an error if no peer is discovered and connected within `timeout`.
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
    ///
    /// # Errors
    ///
    /// Returns an error if the outbound request fails at the libp2p layer, or
    /// no response arrives within `timeout`.
    pub async fn request(
        &mut self,
        peer: PeerId,
        request: EventRequest,
        timeout: Duration,
    ) -> Result<EventResponse> {
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

    /// Poll `peer` for every buffered event with `seq > last_seq`.
    ///
    /// Returns `(head_seq, events)`. `head_seq` is the gateway's newest stamped
    /// sequence number — compare it against the last returned event's `seq` (or
    /// against `last_seq` on an empty reply) to see how far behind you are, and
    /// compare the FIRST returned `seq` against `last_seq + 1` to detect an
    /// eviction gap.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails/times out, or the gateway answers
    /// with an unexpected response variant.
    pub async fn poll_since(
        &mut self,
        peer: PeerId,
        last_seq: u64,
        timeout: Duration,
    ) -> Result<(u64, Vec<RemoteCoalitionEventV1>)> {
        match self
            .request(peer, EventRequest::PollSince { last_seq }, timeout)
            .await?
        {
            EventResponse::Events { head_seq, events } => Ok((head_seq, events)),
            other => Err(anyhow!("unexpected response to PollSince: {other:?}")),
        }
    }

    /// Ask `peer` for `(head_seq, buffer_len)` without transferring payloads.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails/times out, or the gateway answers
    /// with an unexpected response variant.
    pub async fn head(&mut self, peer: PeerId, timeout: Duration) -> Result<(u64, u64)> {
        match self.request(peer, EventRequest::Head, timeout).await? {
            EventResponse::Head { head_seq, len } => Ok((head_seq, len)),
            other => Err(anyhow!("unexpected response to Head: {other:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subsystems::coalition_actor::DecisionKind;

    fn record(agent_id: usize, kind: DecisionKind) -> DecisionRecord {
        DecisionRecord {
            coalition: "coalition-7".to_string(),
            agent_id,
            kind,
            act: kind == DecisionKind::Join,
            score: 0.25 * agent_id as f64,
        }
    }

    fn events(response: &EventResponse) -> &[RemoteCoalitionEventV1] {
        match response {
            EventResponse::Events { events, .. } => events,
            other => panic!("expected Events, got {other:?}"),
        }
    }

    /// Sequencing starts at 1, is monotonic, and the projection carries every
    /// `DecisionRecord` field (kind label, act, score, agent id, coalition).
    #[test]
    fn seq_is_monotonic_from_one_and_projection_is_faithful() {
        let mut buf = EventBuffer::new(8);
        assert_eq!(buf.head_seq(), 0, "nothing buffered ⇒ head_seq 0");
        assert!(buf.is_empty());

        assert_eq!(buf.push_record(&record(1, DecisionKind::Join)), 1);
        assert_eq!(buf.push_record(&record(2, DecisionKind::Leave)), 2);
        assert_eq!(buf.push_record(&record(3, DecisionKind::Join)), 3);
        assert_eq!(buf.head_seq(), 3);
        assert_eq!(buf.len(), 3);

        let all = buf.respond(&EventRequest::PollSince { last_seq: 0 });
        let evs = events(&all);
        assert_eq!(evs.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![1, 2, 3]);
        assert!(evs.iter().all(|e| e.schema_version == REMOTE_WIRE_SCHEMA_VERSION));
        assert_eq!(evs[0].coalition, "coalition-7");
        assert_eq!(evs[0].agent_id, 1);
        assert_eq!(evs[0].kind, "join");
        assert!(evs[0].act);
        assert!((evs[0].score - 0.25).abs() < f64::EPSILON);
        assert_eq!(evs[1].kind, "leave");
        assert!(!evs[1].act);
    }

    /// `PollSince` filters strictly on `seq > last_seq`, never drains, and
    /// `Head` reports `(head_seq, len)` without touching the buffer.
    #[test]
    fn poll_since_filters_and_never_drains() {
        let mut buf = EventBuffer::new(8);
        for id in 1..=4 {
            buf.push_record(&record(id, DecisionKind::Join));
        }

        let delta = buf.respond(&EventRequest::PollSince { last_seq: 2 });
        assert_eq!(
            events(&delta).iter().map(|e| e.seq).collect::<Vec<_>>(),
            vec![3, 4]
        );
        match delta {
            EventResponse::Events { head_seq, .. } => assert_eq!(head_seq, 4),
            other => panic!("expected Events, got {other:?}"),
        }

        // Past the head ⇒ empty, still with the head cursor.
        let empty = buf.respond(&EventRequest::PollSince { last_seq: 4 });
        assert!(events(&empty).is_empty());

        // Non-draining: the full poll still returns everything.
        assert_eq!(events(&buf.respond(&EventRequest::PollSince { last_seq: 0 })).len(), 4);
        assert_eq!(buf.len(), 4);

        assert_eq!(
            buf.respond(&EventRequest::Head),
            EventResponse::Head {
                head_seq: 4,
                len: 4
            }
        );
        assert_eq!(buf.len(), 4, "Head is read-only");
    }

    /// The cap evicts the OLDEST event; `head_seq` keeps advancing, so a client
    /// can detect the gap (first returned seq > last_seq + 1).
    #[test]
    fn cap_evicts_oldest_and_gap_is_detectable() {
        let mut buf = EventBuffer::new(2);
        for id in 1..=5 {
            buf.push_record(&record(id, DecisionKind::Join));
        }
        assert_eq!(buf.len(), 2, "cap holds");
        assert_eq!(buf.head_seq(), 5, "sequence numbers are never reused");

        let all = buf.respond(&EventRequest::PollSince { last_seq: 0 });
        let evs = events(&all);
        assert_eq!(evs.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![4, 5]);

        // A client at cursor 1 asks for 2..; it gets 4.. ⇒ 2 and 3 were evicted.
        let gapped = buf.respond(&EventRequest::PollSince { last_seq: 1 });
        let gapped_evs = events(&gapped);
        assert_eq!(gapped_evs[0].seq, 4);
        assert!(
            gapped_evs[0].seq > 1 + 1,
            "gap between the cursor and the oldest retained event is observable"
        );
    }

    /// `buffer_cap = 0` is clamped to 1 (never a panic, never a permanently
    /// empty gateway).
    #[test]
    fn zero_cap_is_clamped_to_one() {
        let mut buf = EventBuffer::new(0);
        buf.push_record(&record(1, DecisionKind::Join));
        buf.push_record(&record(2, DecisionKind::Leave));
        assert_eq!(buf.len(), 1);
        assert_eq!(buf.head_seq(), 2);
        let evs = buf.respond(&EventRequest::PollSince { last_seq: 0 });
        assert_eq!(events(&evs).len(), 1);
        assert_eq!(events(&evs)[0].seq, 2, "the newest event survives");
    }
}
