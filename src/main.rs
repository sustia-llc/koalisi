//! Reference daemon for koalisi — a domain-neutral coalition runtime.
//!
//! Assembles a [`CoalitionRuntime`], forms a seed coalition of synthetic agents
//! whose capability masks are distinct bits (so a coalition's value is coverage
//! diversity), then runs a bounded, policy-gated join loop that grows the
//! coalition one candidate at a time through the [`CoalitionService`] seam —
//! the same runtime seam the flagship `synthetic_ingestion` example uses. Waits
//! for ctrl-c or for the service to stop, whichever comes first, then runs the
//! runtime's three-step shutdown (cancel the root token, close the tracker,
//! drain tracked tasks). Exits with an error when the service panicked or
//! stopped before ctrl-c.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, bail};
use tokio::time::interval;
use tokio_util::sync::CancellationToken;

use koalisi::algorithms::{AdditiveCalculator, CapabilityAgent};
use koalisi::core::config::setup_logging;
use koalisi::core::{CoalitionRuntime, Executor};
use koalisi::decision::{DecisionContext, ThresholdPolicy};
use koalisi::subsystems::coalition_actor::{CoalitionService, CoalitionServiceHandle};
use koalisi::topology::{CoalitionManager, HyperedgeIndex, VertexIndex};

/// A minimal coalition label (satisfies the topology `HyperedgeTrait` bound).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Team(u32);

const AGENT_COUNT: usize = 6;
const SEED_MEMBERS: usize = 2;

#[tokio::main]
async fn main() -> Result<()> {
    setup_logging()?;
    let runtime = CoalitionRuntime::new(Arc::new(Executor::new()?));

    // A set of agents, each covering a distinct capability bit.
    let agents: Vec<CapabilityAgent> = (0..AGENT_COUNT)
        .map(|id| CapabilityAgent::new(id, 1u32 << id, 50))
        .collect();

    // Add every agent to the manager (the candidate vertices must exist before
    // the manager is moved into the service), then form a seed coalition over
    // the first `SEED_MEMBERS`.
    let manager: CoalitionManager<CapabilityAgent, Team> =
        CoalitionManager::empty(Arc::clone(runtime.executor()));
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
        runtime.task_tracker(),
        runtime.cancellation_token().child_token(),
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

    run_until_stopped(runtime, &service, tokio::signal::ctrl_c()).await
}

/// Wait for `signal` or for `service` to stop taking commands, whichever
/// comes first, then run `runtime`'s shutdown. Returns `Ok` when `signal`
/// came first and the service did not panic; an error when `signal` fails
/// (before the shutdown), when the service panicked, or when it stopped
/// before `signal`.
async fn run_until_stopped<S>(
    runtime: CoalitionRuntime,
    service: &CoalitionServiceHandle,
    signal: S,
) -> Result<()>
where
    S: Future<Output = std::io::Result<()>>,
{
    let signalled = tokio::select! {
        r = signal => {
            r?;
            tracing::info!("ctrl-c received, shutting down");
            true
        }
        () = service.closed() => {
            tracing::error!(
                panicked = service.panicked(),
                "coalition service stopped, shutting down"
            );
            false
        }
    };

    // `shutdown` cancels the root token, which stops the join loop and the
    // service, and drains both tracked tasks.
    runtime.shutdown().await;
    if service.panicked() {
        bail!("coalition service panicked");
    }
    if !signalled {
        bail!("coalition service stopped before ctrl-c");
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use koalisi::algorithms::AgentCapabilities;
    use koalisi::decision::{CoalitionDecisionPolicy, Decision};

    /// A policy whose join decision panics.
    struct PanickingPolicy;

    impl CoalitionDecisionPolicy for PanickingPolicy {
        fn should_join(
            &self,
            _agent: &dyn AgentCapabilities,
            _coalition: &[&dyn AgentCapabilities],
            _ctx: &DecisionContext,
        ) -> Decision {
            panic!("policy exploded");
        }

        fn should_leave(
            &self,
            _agent: &dyn AgentCapabilities,
            _coalition: &[&dyn AgentCapabilities],
            _ctx: &DecisionContext,
        ) -> Decision {
            Decision {
                act: false,
                score: 0.0,
            }
        }
    }

    /// A runtime and a service under `policy` over a one-member coalition and
    /// one candidate; returns the candidate and the coalition.
    async fn daemon(
        policy: Box<dyn CoalitionDecisionPolicy>,
    ) -> Result<(
        CoalitionRuntime,
        CoalitionServiceHandle,
        VertexIndex,
        HyperedgeIndex,
    )> {
        let runtime = CoalitionRuntime::new(Arc::new(Executor::with_num_threads(1)?));
        let manager: CoalitionManager<CapabilityAgent, Team> =
            CoalitionManager::empty(Arc::clone(runtime.executor()));
        let seed = manager.add_agent(CapabilityAgent::new(0, 0b01, 50)).await?;
        let candidate = manager.add_agent(CapabilityAgent::new(1, 0b10, 50)).await?;
        let coalition = manager.form_coalition(vec![seed], Team(1)).await?;
        let service = CoalitionService::spawn(
            manager,
            policy,
            DecisionContext::default(),
            runtime.task_tracker(),
            runtime.cancellation_token().child_token(),
        );
        Ok((runtime, service, candidate, coalition))
    }

    /// Run `run_until_stopped` under a 5 s bound and return its error text,
    /// `None` for `Ok`; a timeout fails the test naming `label`.
    async fn stop_outcome<S>(
        label: &str,
        runtime: CoalitionRuntime,
        service: &CoalitionServiceHandle,
        signal: S,
    ) -> Option<String>
    where
        S: Future<Output = std::io::Result<()>>,
    {
        let bounded = tokio::time::timeout(
            Duration::from_secs(5),
            run_until_stopped(runtime, service, signal),
        )
        .await;
        bounded
            .unwrap_or_else(|_| {
                panic!(
                    "{label}: expected run_until_stopped to return within 5 s, observed a timeout"
                )
            })
            .err()
            .map(|e| e.to_string())
    }

    /// A signal ends the wait cleanly; a service that panicked or was
    /// cancelled ends it with its error while the signal is still pending.
    #[tokio::test]
    async fn run_until_stopped_ends_on_the_signal_or_a_dead_service() -> Result<()> {
        let (runtime, service, _, _) =
            daemon(Box::new(ThresholdPolicy::new(AdditiveCalculator, 0.0, 0.0))).await?;
        let outcome = stop_outcome("signal first", runtime, &service, async {
            std::io::Result::Ok(())
        })
        .await;
        assert_eq!(outcome, None, "signal first: expected Ok");

        let (runtime, service, candidate, coalition) = daemon(Box::new(PanickingPolicy)).await?;
        let join = service.join(candidate, coalition).await;
        assert!(
            join.is_err(),
            "expected the policy panic to fail the join, observed {join:?}"
        );
        let outcome = stop_outcome(
            "panicked service",
            runtime,
            &service,
            std::future::pending(),
        )
        .await;
        assert_eq!(
            outcome.as_deref(),
            Some("coalition service panicked"),
            "panicked service"
        );

        let (runtime, service, _, _) =
            daemon(Box::new(ThresholdPolicy::new(AdditiveCalculator, 0.0, 0.0))).await?;
        runtime.cancellation_token().cancel();
        let outcome = stop_outcome(
            "cancelled service",
            runtime,
            &service,
            std::future::pending(),
        )
        .await;
        assert_eq!(
            outcome.as_deref(),
            Some("coalition service stopped before ctrl-c"),
            "cancelled service"
        );
        Ok(())
    }
}
