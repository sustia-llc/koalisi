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
/// panicked; otherwise the first of these errors, with each later one that
/// holds logged at error level: `signal` failed, the service panicked, the
/// join loop panicked or was cancelled, the service stopped before `signal`.
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
    let mut failures = Vec::new();
    if let Some(Err(e)) = &signalled {
        failures.push(format!("awaiting ctrl-c failed: {e}"));
    }
    if service.panicked() {
        failures.push("coalition service panicked".to_string());
    }
    if let Err(e) = &join_outcome {
        failures.push(format!("join loop failed: {e}"));
    }
    if signalled.is_none() {
        failures.push("coalition service stopped before ctrl-c".to_string());
    }
    let mut failures = failures.into_iter();
    let Some(first) = failures.next() else {
        tracing::info!("shutdown clean");
        return Ok(());
    };
    for later in failures {
        tracing::error!(error = %later, first = %first, "a further shutdown failure");
    }
    bail!(first)
}

/// What the join loop does after one offer.
#[derive(Debug, PartialEq, Eq)]
enum Next {
    /// Offer the next candidate.
    Continue,
    /// The service has stopped; end the loop.
    Stop,
}

/// Offer `candidate` to `coalition`, then read the coalition size, logging
/// each outcome: a decision and a size at info, a non-terminal error
/// (`ServiceError::is_terminal`) at warn and [`Next::Continue`], a terminal
/// one at warn as a stopped service and [`Next::Stop`]. A failed join skips
/// the size read.
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
        Err(e) if e.is_terminal() => {
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
        Err(e) if e.is_terminal() => {
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
#[path = "test_support.rs"]
mod test_support;

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

    /// A panicked service and a panicked join loop under a pending signal
    /// return the first error, `coalition service panicked`, and log the two
    /// later ones, `join loop failed: …` and `coalition service stopped before
    /// ctrl-c`, once each at error level, in that order.
    #[tokio::test]
    async fn run_until_stopped_logs_each_failure_after_the_first() -> Result<()> {
        let capture = test_support::log_capture::Capture::default();
        let _guard = capture.install();
        let (runtime, service, candidate, _, coalition) =
            daemon(|_| Box::new(PanickingPolicy)).await?;
        let join = service.join(candidate, coalition).await;
        assert!(
            join.is_err(),
            "expected the policy panic to fail the join, observed {join:?}"
        );
        let join_loop = runtime
            .task_tracker()
            .spawn(async { panic!("join loop exploded") });
        let outcome = stop_outcome_with(
            "three failures",
            runtime,
            &service,
            join_loop,
            std::future::pending(),
        )
        .await;
        let further = capture.matching("a further shutdown failure");
        let errors: Vec<(tracing::Level, Option<&str>)> = further
            .iter()
            .map(|e| (e.level, e.fields.get("error").map(String::as_str)))
            .collect();
        assert_eq!(
            outcome.as_deref(),
            Some("coalition service panicked"),
            "the returned error; further failures logged {errors:?}"
        );
        assert!(
            errors.len() == 2
                && errors
                    .iter()
                    .all(|(level, _)| *level == tracing::Level::ERROR)
                && errors[0]
                    .1
                    .is_some_and(|e| e.starts_with("join loop failed:"))
                && errors[1].1 == Some("coalition service stopped before ctrl-c"),
            "expected error-level logs of the join loop failure, then the early stop, \
             observed {errors:?}"
        );
        Ok(())
    }

    /// A join whose graph lock another clone of the graph poisoned fails with
    /// `Topology(LockPoisoned)`, which `offer` logs as `join failed;
    /// coalition service stopped` and answers with `Next::Stop`.
    #[tokio::test]
    async fn offer_stops_on_a_poisoned_graph_lock() -> Result<()> {
        use koalisi::topology::TemporalError;

        let capture = test_support::log_capture::Capture::default();
        let _guard = capture.install();
        let runtime = CoalitionRuntime::new(Arc::new(Executor::with_num_threads(1)?));
        let manager: CoalitionManager<CapabilityAgent, Team> =
            CoalitionManager::empty(Arc::clone(runtime.executor()));
        let seed = manager.add_agent(CapabilityAgent::new(0, 0b01, 50)).await?;
        let candidate = manager.add_agent(CapabilityAgent::new(1, 0b10, 50)).await?;
        let coalition = manager.form_coalition(vec![seed], Team(1)).await?;
        let graph = manager.graph().clone();
        let poisoned = tokio::spawn(async move {
            graph
                .update_hyperedge_vertices_try(
                    coalition,
                    |_: Vec<VertexIndex>| -> Result<Vec<VertexIndex>, TemporalError> {
                        panic!("mutator exploded")
                    },
                )
                .await
        })
        .await;
        assert!(
            poisoned.as_ref().is_err_and(|e| e.is_panic()),
            "expected the mutator panic to poison the lock, observed {poisoned:?}"
        );
        let service = CoalitionService::spawn(
            manager,
            Box::new(ThresholdPolicy::new(AdditiveCalculator, 0.0, 0.0)),
            DecisionContext::default(),
            runtime.task_tracker(),
            runtime.cancellation_token().child_token(),
        );

        let next = offer(&service, coalition, candidate).await;
        let seen = warns_after(&capture, 0);
        let messages: Vec<&str> = seen.iter().map(|w| w.message.as_str()).collect();
        assert_eq!(
            (next, messages),
            (Next::Stop, vec!["join failed; coalition service stopped"]),
            "(next, warn messages); warns {seen:?}"
        );
        runtime.shutdown().await;
        Ok(())
    }

    /// One captured warn event: its message and its `error` field.
    #[derive(Debug, Clone, PartialEq)]
    struct Warn {
        message: String,
        error: Option<String>,
    }

    /// The warn-level events `capture` holds after its first `skip` warns.
    fn warns_after(capture: &test_support::log_capture::Capture, skip: usize) -> Vec<Warn> {
        capture
            .matching("")
            .into_iter()
            .filter(|e| e.level == tracing::Level::WARN)
            .skip(skip)
            .map(|e| Warn {
                message: e.fields.get("message").cloned().unwrap_or_default(),
                error: e.fields.get("error").cloned(),
            })
            .collect()
    }

    /// `offer` logs a join topology error as `join failed` and continues; a
    /// members error after a stop as `members failed; coalition service
    /// stopped` and stops; a join on the stopped service as `join failed;
    /// coalition service stopped` and stops. Each warn carries the error.
    #[tokio::test]
    async fn offer_logs_each_error_and_tells_a_stopped_service_apart() -> Result<()> {
        let capture = test_support::log_capture::Capture::default();
        let _guard = capture.install();
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
        let mut warns_before = 0;
        for (label, agent, want_next, want_message) in steps {
            let next = offer(&service, coalition, agent).await;
            let seen = warns_after(&capture, warns_before);
            warns_before += seen.len();
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
