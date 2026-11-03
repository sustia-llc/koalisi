//! Reference daemon for koalisi — a domain-neutral coalition runtime.
//!
//! Assembles a [`CoalitionRuntime`], forms a seed coalition of synthetic agents
//! whose capability masks are distinct bits (so a coalition's value is coverage
//! diversity), then runs a bounded, policy-gated join loop that grows the
//! coalition one candidate at a time through the [`CoalitionService`] seam —
//! the same runtime seam the flagship `synthetic_ingestion` example uses. Waits
//! for ctrl-c or for the service to stop, whichever comes first, then runs the
//! runtime's three-step shutdown (cancel the root token, close the tracker,
//! drain tracked tasks) and reads the join loop's outcome. Exits with an
//! error when ctrl-c could not be awaited, the service panicked, the join
//! loop panicked, or the service stopped before ctrl-c.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, bail};
use tokio::task::JoinHandle;
use tokio::time::interval;
use tokio_util::sync::CancellationToken;

use koalisi::algorithms::{AdditiveCalculator, CapabilityAgent};
use koalisi::core::config::setup_logging;
use koalisi::core::{CoalitionRuntime, Executor};
use koalisi::decision::{DecisionContext, ThresholdPolicy};
use koalisi::subsystems::coalition_actor::{
    CoalitionService, CoalitionServiceHandle, ServiceError,
};
use koalisi::topology::{CoalitionManager, HyperedgeIndex, VertexIndex};

/// A minimal coalition label (satisfies the topology `HyperedgeTrait` bound).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Team(u32);

const AGENT_COUNT: usize = 6;
const SEED_MEMBERS: usize = 2;

#[tokio::main]
async fn main() -> Result<()> {
    setup_logging();
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
    let join_loop = runtime.task_tracker().spawn(join_loop(
        service.clone(),
        coalition,
        candidates,
        loop_token,
    ));

    run_until_stopped(runtime, &service, join_loop, tokio::signal::ctrl_c()).await
}

/// Wait for `signal` or for `service` to stop taking commands, whichever
/// comes first, then run `runtime`'s shutdown and await `join_loop`. Returns
/// `Ok` when `signal` came first and neither the service nor the join loop
/// panicked; otherwise the first of these errors: `signal` failed, the
/// service panicked, the join loop panicked or was cancelled, the service
/// stopped before `signal`.
async fn run_until_stopped<S>(
    runtime: CoalitionRuntime,
    service: &CoalitionServiceHandle,
    join_loop: JoinHandle<()>,
    signal: S,
) -> Result<()>
where
    S: Future<Output = std::io::Result<()>>,
{
    let signalled = tokio::select! {
        r = signal => {
            match &r {
                Ok(()) => tracing::info!("ctrl-c received, shutting down"),
                Err(e) => tracing::error!(error = %e, "awaiting ctrl-c failed, shutting down"),
            }
            Some(r)
        }
        () = service.closed() => {
            tracing::error!(
                panicked = service.panicked(),
                "coalition service stopped, shutting down"
            );
            None
        }
    };

    // `shutdown` cancels the root token, which stops the join loop and the
    // service, and drains both tracked tasks.
    runtime.shutdown().await;
    let join_outcome = join_loop.await;
    if let Some(Err(e)) = &signalled {
        bail!("awaiting ctrl-c failed: {e}");
    }
    if service.panicked() {
        bail!("coalition service panicked");
    }
    if let Err(e) = join_outcome {
        bail!("join loop failed: {e}");
    }
    if signalled.is_none() {
        bail!("coalition service stopped before ctrl-c");
    }
    tracing::info!("shutdown clean");
    Ok(())
}

/// What the join loop does after one offer.
#[derive(Debug, PartialEq, Eq)]
enum Next {
    /// Offer the next candidate.
    Continue,
    /// The service has stopped; end the loop.
    Stop,
}

/// Whether `e` means the service no longer answers commands, as opposed to a
/// topology error on one command.
fn service_stopped(e: &ServiceError) -> bool {
    match e {
        ServiceError::Topology(_) => false,
        ServiceError::Gone
        | ServiceError::ReplyDropped
        | ServiceError::Cancelled
        | ServiceError::Panicked { .. } => true,
    }
}

/// Offer `candidate` to `coalition`, then read the coalition size, logging
/// each outcome: a decision and a size at info, a topology error at warn and
/// [`Next::Continue`], a stopped service at warn and [`Next::Stop`]. A failed
/// join skips the size read.
async fn offer(
    service: &CoalitionServiceHandle,
    coalition: HyperedgeIndex,
    candidate: VertexIndex,
) -> Next {
    let agent = usize::from(candidate);
    match service.join(candidate, coalition).await {
        Ok(decision) => tracing::info!(
            agent,
            act = decision.act,
            score = decision.score,
            "policy-gated join decision"
        ),
        Err(e) if service_stopped(&e) => {
            tracing::warn!(agent, error = %e, "join failed; coalition service stopped");
            return Next::Stop;
        }
        Err(e) => {
            tracing::warn!(agent, error = %e, "join failed");
            return Next::Continue;
        }
    }
    match service.members(coalition).await {
        Ok(members) => {
            tracing::info!(size = members.len(), "coalition size");
            Next::Continue
        }
        Err(e) if service_stopped(&e) => {
            tracing::warn!(error = %e, "members failed; coalition service stopped");
            Next::Stop
        }
        Err(e) => {
            tracing::warn!(error = %e, "members failed");
            Next::Continue
        }
    }
}

/// Offer each candidate to the coalition once per interval tick through
/// [`offer`]. Returns when `token` is cancelled or the service has stopped;
/// idles after the last candidate.
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

        if offer(&service, coalition, candidate).await == Next::Stop {
            return;
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

    /// A policy whose join cancels the service's token and declines.
    struct CancellingPolicy {
        token: CancellationToken,
    }

    impl CoalitionDecisionPolicy for CancellingPolicy {
        fn should_join(
            &self,
            _agent: &dyn AgentCapabilities,
            _coalition: &[&dyn AgentCapabilities],
            _ctx: &DecisionContext,
        ) -> Decision {
            self.token.cancel();
            Decision {
                act: false,
                score: 0.0,
            }
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

    /// A runtime and a service under the policy `policy` builds from the
    /// service's token, over a one-member coalition, one candidate and one
    /// removed agent; returns the candidate, the removed agent and the
    /// coalition.
    async fn daemon(
        policy: impl FnOnce(CancellationToken) -> Box<dyn CoalitionDecisionPolicy>,
    ) -> Result<(
        CoalitionRuntime,
        CoalitionServiceHandle,
        VertexIndex,
        VertexIndex,
        HyperedgeIndex,
    )> {
        let runtime = CoalitionRuntime::new(Arc::new(Executor::with_num_threads(1)?));
        let manager: CoalitionManager<CapabilityAgent, Team> =
            CoalitionManager::empty(Arc::clone(runtime.executor()));
        let seed = manager.add_agent(CapabilityAgent::new(0, 0b01, 50)).await?;
        let candidate = manager.add_agent(CapabilityAgent::new(1, 0b10, 50)).await?;
        let removed = manager
            .add_agent(CapabilityAgent::new(2, 0b100, 50))
            .await?;
        manager.remove_agent(removed).await?;
        let coalition = manager.form_coalition(vec![seed], Team(1)).await?;
        let token = runtime.cancellation_token().child_token();
        let service = CoalitionService::spawn(
            manager,
            policy(token.clone()),
            DecisionContext::default(),
            runtime.task_tracker(),
            token,
        );
        Ok((runtime, service, candidate, removed, coalition))
    }

    fn threshold(_: CancellationToken) -> Box<dyn CoalitionDecisionPolicy> {
        Box::new(ThresholdPolicy::new(AdditiveCalculator, 0.0, 0.0))
    }

    /// Run `run_until_stopped` under a 5 s bound with a join loop that
    /// returns at once, and return its error text, `None` for `Ok`; a timeout
    /// fails the test naming `label`.
    async fn stop_outcome<S>(
        label: &str,
        runtime: CoalitionRuntime,
        service: &CoalitionServiceHandle,
        signal: S,
    ) -> Option<String>
    where
        S: Future<Output = std::io::Result<()>>,
    {
        let join_loop = runtime.task_tracker().spawn(async {});
        stop_outcome_with(label, runtime, service, join_loop, signal).await
    }

    /// [`stop_outcome`] with the given `join_loop`.
    async fn stop_outcome_with<S>(
        label: &str,
        runtime: CoalitionRuntime,
        service: &CoalitionServiceHandle,
        join_loop: JoinHandle<()>,
        signal: S,
    ) -> Option<String>
    where
        S: Future<Output = std::io::Result<()>>,
    {
        let bounded = tokio::time::timeout(
            Duration::from_secs(5),
            run_until_stopped(runtime, service, join_loop, signal),
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
        let (runtime, service, _, _, _) = daemon(threshold).await?;
        let outcome = stop_outcome("signal first", runtime, &service, async {
            std::io::Result::Ok(())
        })
        .await;
        assert_eq!(outcome, None, "signal first: expected Ok");

        let (runtime, service, candidate, _, coalition) =
            daemon(|_| Box::new(PanickingPolicy)).await?;
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

        let (runtime, service, _, _, _) = daemon(threshold).await?;
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

    /// A join loop that panicked turns a clean signal-first stop into the
    /// error `join loop failed: …`.
    #[tokio::test]
    async fn run_until_stopped_reads_the_join_loop_outcome() -> Result<()> {
        let (runtime, service, _, _, _) = daemon(threshold).await?;
        let join_loop = runtime
            .task_tracker()
            .spawn(async { panic!("join loop exploded") });
        let outcome =
            stop_outcome_with("panicked join loop", runtime, &service, join_loop, async {
                std::io::Result::Ok(())
            })
            .await;
        assert!(
            outcome
                .as_deref()
                .is_some_and(|e| e.starts_with("join loop failed:")),
            "panicked join loop: expected an error starting `join loop failed:`, observed {outcome:?}"
        );
        Ok(())
    }

    /// One captured warn event: its message and its `error` field.
    #[derive(Debug, Clone, PartialEq)]
    struct Warn {
        message: String,
        error: Option<String>,
    }

    /// A `tracing` layer recording every warn-level event.
    #[derive(Clone, Default)]
    struct Warns(Arc<std::sync::Mutex<Vec<Warn>>>);

    impl Warns {
        fn take(&self) -> Vec<Warn> {
            std::mem::take(&mut *self.0.lock().expect("capture lock"))
        }
    }

    struct WarnFields<'a>(&'a mut Warn);

    impl tracing::field::Visit for WarnFields<'_> {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            match field.name() {
                "message" => self.0.message = format!("{value:?}"),
                "error" => self.0.error = Some(format!("{value:?}")),
                _ => {}
            }
        }
    }

    impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Warns {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _ctx: tracing_subscriber::layer::Context<'_, S>,
        ) {
            if *event.metadata().level() != tracing::Level::WARN {
                return;
            }
            let mut warn = Warn {
                message: String::new(),
                error: None,
            };
            event.record(&mut WarnFields(&mut warn));
            self.0.lock().expect("capture lock").push(warn);
        }
    }

    /// `offer` logs a join topology error as `join failed` and continues; a
    /// members error after a stop as `members failed; coalition service
    /// stopped` and stops; a join on the stopped service as `join failed;
    /// coalition service stopped` and stops. Each warn carries the error.
    #[tokio::test]
    async fn offer_logs_each_error_and_tells_a_stopped_service_apart() -> Result<()> {
        use tracing_subscriber::layer::SubscriberExt;

        let warns = Warns::default();
        let _guard =
            tracing::subscriber::set_default(tracing_subscriber::registry().with(warns.clone()));
        let (runtime, service, candidate, removed, coalition) =
            daemon(|token| Box::new(CancellingPolicy { token })).await?;

        let steps = [
            (
                "join of a removed agent",
                removed,
                Next::Continue,
                "join failed",
            ),
            (
                "members after the policy cancels",
                candidate,
                Next::Stop,
                "members failed; coalition service stopped",
            ),
            (
                "join on the stopped service",
                candidate,
                Next::Stop,
                "join failed; coalition service stopped",
            ),
        ];
        for (label, agent, want_next, want_message) in steps {
            let next = offer(&service, coalition, agent).await;
            let seen = warns.take();
            let messages: Vec<&str> = seen.iter().map(|w| w.message.as_str()).collect();
            assert_eq!(
                (next, messages),
                (want_next, vec![want_message]),
                "{label}: (next, warn messages); warns {seen:?}"
            );
            assert!(
                seen[0].error.as_deref().is_some_and(|e| !e.is_empty()),
                "{label}: expected the warn to carry the error, observed {seen:?}"
            );
        }
        runtime.shutdown().await;
        Ok(())
    }
}
