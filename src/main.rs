//! Reference daemon for koalisi — a domain-neutral coalition runtime.
//!
//! Assembles a [`CoalitionRuntime`], forms a seed coalition of synthetic agents
//! whose capability masks are distinct bits (so a coalition's value is coverage
//! diversity), then runs a bounded, policy-gated join loop that grows the
//! coalition one candidate at a time through the [`CoalitionService`] seam —
//! the same runtime seam the flagship `synthetic_ingestion` example uses. Waits
//! for ctrl-c, then runs the runtime's three-step shutdown (cancel the root
//! token, close the tracker, drain tracked tasks).

use std::time::Duration;

use anyhow::Result;
use tokio::time::interval;
use tokio_util::sync::CancellationToken;

use koalisi::algorithms::{AdditiveCalculator, AgentCapabilities};
use koalisi::core::CoalitionRuntime;
use koalisi::core::config::setup_logging;
use koalisi::decision::{DecisionContext, ThresholdPolicy};
use koalisi::subsystems::coalition_actor::{CoalitionService, CoalitionServiceHandle};
use koalisi::topology::{CoalitionManager, HyperedgeIndex, VertexIndex};

/// A minimal domain-neutral agent: its capability mask is a single distinct bit,
/// so a coalition's value comes from coverage diversity. `Copy + Eq + Debug`
/// satisfies the topology `VertexTrait` blanket bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Agent {
    id: usize,
    caps: u32,
    trust: u32,
}

impl AgentCapabilities for Agent {
    fn agent_id(&self) -> usize {
        self.id
    }
    fn capabilities(&self) -> u32 {
        self.caps
    }
    fn trust_level(&self) -> u32 {
        self.trust
    }
}

/// A minimal coalition label (satisfies the topology `HyperedgeTrait` bound).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Team(u32);

const AGENT_COUNT: usize = 6;
const SEED_MEMBERS: usize = 2;

#[tokio::main]
async fn main() -> Result<()> {
    setup_logging();

    let runtime = CoalitionRuntime::new();

    // A set of agents, each covering a distinct capability bit.
    let agents: Vec<Agent> = (0..AGENT_COUNT)
        .map(|id| Agent {
            id,
            caps: 1u32 << id,
            trust: 50,
        })
        .collect();

    // Add every agent to the manager (the candidate vertices must exist before
    // the manager is moved into the service), then form a seed coalition over
    // the first `SEED_MEMBERS`.
    let manager: CoalitionManager<Agent, Team> = CoalitionManager::empty();
    let mut vertices = Vec::with_capacity(agents.len());
    for a in &agents {
        vertices.push(manager.add_agent(*a).await?);
    }
    let coalition = manager
        .form_coalition(vertices[..SEED_MEMBERS].to_vec(), Team(1))
        .await?;

    // Spawn the policy-gated membership seam. A candidate that covers a fresh
    // capability bit clears the additive marginal-value threshold and joins.
    let required = (1u32 << AGENT_COUNT) - 1;
    let service = CoalitionService::spawn(
        manager,
        Box::new(ThresholdPolicy::new(AdditiveCalculator, 0.0, 0.0)),
        DecisionContext {
            required_capabilities: required,
        },
    );

    tracing::info!(
        seed_members = SEED_MEMBERS,
        candidates = AGENT_COUNT - SEED_MEMBERS,
        "coalition runtime started"
    );

    // Background join loop on a child of the root token, tracked so shutdown
    // drains it.
    let candidates: Vec<VertexIndex> = vertices[SEED_MEMBERS..].to_vec();
    let loop_token = runtime.cancellation_token().child_token();
    let _join = runtime.task_tracker().spawn(join_loop(
        service.clone(),
        coalition,
        candidates,
        loop_token,
    ));

    tokio::signal::ctrl_c().await?;
    tracing::info!("ctrl-c received, shutting down");

    // Drop the daemon's own handle; the loop's clone releases when `shutdown`
    // cancels the root token (draining the loop task), after which the service
    // task exits on its now-closed command channel.
    drop(service);
    runtime.shutdown().await;
    tracing::info!("shutdown clean");
    Ok(())
}

/// Offer each candidate to the coalition once per interval tick, logging the
/// policy decision. Idles until cancelled after the last candidate.
async fn join_loop(
    service: CoalitionServiceHandle,
    coalition: HyperedgeIndex,
    candidates: Vec<VertexIndex>,
    token: CancellationToken,
) {
    let mut clock = interval(Duration::from_secs(2));
    let mut next = 0usize;
    loop {
        tokio::select! {
            biased;
            () = token.cancelled() => return,
            _ = clock.tick() => {}
        }

        if next >= candidates.len() {
            continue; // all candidates offered; idle until cancelled
        }
        let candidate = candidates[next];
        next += 1;

        match service.join(candidate, coalition).await {
            Ok(decision) => tracing::info!(
                agent = usize::from(candidate),
                act = decision.act,
                score = decision.score,
                "policy-gated join decision"
            ),
            Err(e) => {
                tracing::warn!(error = %e, "join failed; service gone");
                return;
            }
        }
        if let Ok(members) = service.members(coalition).await {
            tracing::info!(size = members.len(), "coalition size");
        }
    }
}
