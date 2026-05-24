//! Remote alert gateway using kameo's libp2p-based remote-actor RPC.
//!
//! Layered ON TOP of the existing local-mpsc swarm: the gateway is a
//! normal kameo actor subscribed to the local `PubSub<ArbitrageOpportunity>`,
//! but it's also remote-registered (via `kameo::remote::Behaviour` running
//! on a libp2p swarm) so peers on other processes / machines can discover
//! it and pull alerts via `RemoteActorRef::<RemoteAlertGateway>::lookup(...)`.
//!
//! This is the *publish-to-outside-world* boundary. The hot path
//! (monitor → coordinator → sink) stays on local mpsc — see CLAUDE.md
//! §"Worth flagging" for why the libp2p layer is intentionally NOT used
//! for intra-swarm messaging.
//!
//! ## Lifecycle
//!
//! `enable_remote_alerts(&swarm, config)` is the one-shot entry point.
//! It:
//!
//! 1. Builds a libp2p swarm with `kameo::remote::Behaviour` + mDNS for
//!    peer discovery (TCP+noise+yamux transport).
//! 2. Calls `init_global()` so kameo's `register` / `lookup` macros wire
//!    up to this swarm.
//! 3. `listen_on("/ip4/0.0.0.0/tcp/0")` (port auto-assigned by the OS).
//! 4. Spawns the libp2p event loop on `swarm.task_tracker()` with a
//!    child of `swarm.cancellation_token()`.
//! 5. Spawns the gateway actor, subscribes it to the local alert bus, and
//!    registers it remotely under `config.gateway_name`.
//!
//! The libp2p swarm participates in `Swarm::shutdown()` — cancellation
//! propagates through the child token and the event-loop task exits cleanly.

use std::time::Duration;

use anyhow::Result;
use futures::StreamExt;
use kameo::error::Infallible;
use kameo::prelude::*;
use kameo::remote;
use kameo_actors::pubsub::Subscribe;
use libp2p::{
    PeerId, mdns, noise,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux,
};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::market::ArbitrageOpportunity;
use crate::subsystems::swarm::Swarm;

// ---------------------------------------------------------------------------
// Gateway actor
// ---------------------------------------------------------------------------

/// Remote-discoverable actor that buffers `ArbitrageOpportunity` values
/// from the local alert bus and serves them to remote peers.
///
/// Storage is unbounded — `PollOpportunities` clones (does not drain), so
/// long-running processes should periodically issue a `ClearOpportunities`
/// or implement a size cap. (Out of scope for the POC.)
#[derive(Actor, RemoteActor, Default)]
pub struct RemoteAlertGateway {
    alerts: Vec<ArbitrageOpportunity>,
}

/// LOCAL: subscribed to the swarm's alert bus. NOT exposed via
/// `#[remote_message]` — only used by the in-process `PubSub` fan-out.
impl Message<ArbitrageOpportunity> for RemoteAlertGateway {
    type Reply = ();

    async fn handle(
        &mut self,
        opp: ArbitrageOpportunity,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        tracing::debug!(
            edge_bps = opp.edge_bps,
            "RemoteAlertGateway: opportunity buffered for remote peers"
        );
        self.alerts.push(opp);
    }
}

// ---------------------------------------------------------------------------
// Remote-callable messages
// ---------------------------------------------------------------------------

/// REMOTE: ask returning a clone of all buffered opportunities. Does NOT
/// drain — remote peers can poll repeatedly. Use `ClearOpportunities` to
/// trim the buffer.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PollOpportunities;

#[remote_message]
impl Message<PollOpportunities> for RemoteAlertGateway {
    type Reply = Vec<ArbitrageOpportunity>;

    async fn handle(
        &mut self,
        _: PollOpportunities,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.alerts.clone()
    }
}

/// REMOTE: ask returning the current buffer length without copying the
/// payload. Useful for cheap liveness probes.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PeekOpportunityCount;

#[remote_message]
impl Message<PeekOpportunityCount> for RemoteAlertGateway {
    type Reply = u64;

    async fn handle(
        &mut self,
        _: PeekOpportunityCount,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.alerts.len() as u64
    }
}

/// REMOTE: ask that drops every buffered opportunity. Returns the count
/// that was dropped.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ClearOpportunities;

#[remote_message]
impl Message<ClearOpportunities> for RemoteAlertGateway {
    type Reply = u64;

    async fn handle(
        &mut self,
        _: ClearOpportunities,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let n = self.alerts.len() as u64;
        self.alerts.clear();
        n
    }
}

// ---------------------------------------------------------------------------
// libp2p NetworkBehaviour
// ---------------------------------------------------------------------------

/// kameo's remote-actor behaviour composed with mDNS for local peer
/// discovery. Mirror of the `MyBehaviour` in `kameo/examples/custom_swarm.rs`.
#[derive(NetworkBehaviour)]
struct SwarmBehaviour {
    kameo: remote::Behaviour,
    mdns: mdns::tokio::Behaviour,
}

// ---------------------------------------------------------------------------
// Config + handle
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct RemoteConfig {
    /// Multiaddr to listen on. Use `"/ip4/0.0.0.0/tcp/0"` for an
    /// ephemeral OS-assigned port (the default).
    pub listen_addr: String,
    /// Registry name for the gateway. Remote peers find it via
    /// `RemoteActorRef::<RemoteAlertGateway>::lookup(name)`.
    pub gateway_name: String,
    /// Whether to enable mDNS for local-network peer discovery. Disable
    /// for tests that don't need it (avoids surprise discoveries across
    /// concurrent test processes).
    pub enable_mdns: bool,
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            listen_addr: "/ip4/0.0.0.0/tcp/0".to_string(),
            gateway_name: "forex_swarm_alerts".to_string(),
            enable_mdns: true,
        }
    }
}

/// Handle returned by [`enable_remote_alerts`]. Carries the libp2p peer
/// id (useful for skip-self filtering when iterating `lookup_all`) and
/// the gateway `ActorRef` for in-process local introspection.
pub struct RemoteHandle {
    pub local_peer_id: PeerId,
    pub gateway: ActorRef<RemoteAlertGateway>,
}

// ---------------------------------------------------------------------------
// enable_remote_alerts
// ---------------------------------------------------------------------------

/// Set up the remote alert gateway. See module docs for the full
/// lifecycle. Returns a [`RemoteHandle`] for in-process introspection;
/// remote peers discover the gateway via mDNS + `RemoteActorRef::lookup`.
///
/// May only be called once per process — kameo's `init_global()` claims
/// the process-wide remote registry.
pub async fn enable_remote_alerts(swarm: &Swarm, config: RemoteConfig) -> Result<RemoteHandle> {
    let enable_mdns = config.enable_mdns;
    let mut libp2p_swarm = libp2p::SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_tcp(
            tcp::Config::default(),
            noise::Config::new,
            yamux::Config::default,
        )?
        .with_behaviour(|key| {
            let local_peer_id = key.public().to_peer_id();
            let kameo = remote::Behaviour::new(local_peer_id, remote::messaging::Config::default());
            // mDNS is always wired in (the derive(NetworkBehaviour) shape
            // is fixed), but we conditionally feed it a disabled config.
            let mdns_cfg = if enable_mdns {
                mdns::Config::default()
            } else {
                mdns::Config {
                    enable_ipv6: false,
                    // Effectively disable discovery by setting a huge query interval.
                    query_interval: Duration::from_secs(86_400),
                    ..mdns::Config::default()
                }
            };
            let mdns = mdns::tokio::Behaviour::new(mdns_cfg, local_peer_id)?;
            Ok(SwarmBehaviour { kameo, mdns })
        })?
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(300)))
        .build();

    // Claim the process-wide kameo remote registry. Must happen before
    // `register` / `lookup` are called on any actor.
    libp2p_swarm.behaviour().kameo.init_global();
    libp2p_swarm.listen_on(config.listen_addr.parse()?)?;

    let local_peer_id = *libp2p_swarm.local_peer_id();
    tracing::info!(
        peer_id = %local_peer_id,
        listen_addr = config.listen_addr,
        gateway_name = config.gateway_name,
        mdns = enable_mdns,
        "starting libp2p swarm for remote alerts"
    );

    // Spawn the libp2p event loop on the swarm's TaskTracker. The child
    // cancellation token wires this into `Swarm::shutdown()`.
    let token = swarm.cancellation_token().child_token();
    swarm
        .task_tracker()
        .spawn(run_libp2p_swarm(libp2p_swarm, token));

    // Build the gateway actor and wire it into both buses.
    let gateway = RemoteAlertGateway::spawn(RemoteAlertGateway::default());
    swarm.alert_bus().ask(Subscribe(gateway.clone())).await?;
    gateway.register(config.gateway_name.as_str()).await?;

    Ok(RemoteHandle {
        local_peer_id,
        gateway,
    })
}

// ---------------------------------------------------------------------------
// libp2p event loop
// ---------------------------------------------------------------------------

async fn run_libp2p_swarm(
    mut libp2p_swarm: libp2p::Swarm<SwarmBehaviour>,
    token: CancellationToken,
) {
    loop {
        tokio::select! {
            biased;
            _ = token.cancelled() => {
                tracing::info!("libp2p swarm event loop cancelled");
                return;
            }
            event = libp2p_swarm.select_next_some() => match event {
                SwarmEvent::Behaviour(SwarmBehaviourEvent::Mdns(mdns::Event::Discovered(list))) => {
                    for (peer_id, multiaddr) in list {
                        tracing::info!(peer = %peer_id, addr = %multiaddr, "mDNS discovered peer");
                        libp2p_swarm.add_peer_address(peer_id, multiaddr);
                    }
                }
                SwarmEvent::Behaviour(SwarmBehaviourEvent::Mdns(mdns::Event::Expired(list))) => {
                    for (peer_id, _) in list {
                        tracing::debug!(peer = %peer_id, "mDNS peer expired");
                        let _ = libp2p_swarm.disconnect_peer_id(peer_id);
                    }
                }
                SwarmEvent::Behaviour(SwarmBehaviourEvent::Kameo(remote::Event::Registry(ev))) => {
                    tracing::debug!(?ev, "kameo registry event");
                }
                SwarmEvent::Behaviour(SwarmBehaviourEvent::Kameo(remote::Event::Messaging(ev))) => {
                    tracing::trace!(?ev, "kameo messaging event");
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
// Re-export for convenience
// ---------------------------------------------------------------------------

// Suppress unused-import warning: `Infallible` is referenced via the
// derive macro expansion, not in source.
#[allow(dead_code)]
#[doc(hidden)]
fn _force_infallible_visible() -> Infallible {
    unreachable!()
}
