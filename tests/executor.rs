//! `core::Executor`: `run` resolves to the closure value on a thread named
//! `koalisi-exec-*`, runs on a pool of the configured size, and resumes a
//! closure panic on the awaiting task. A `TemporalHypergraph` runs its graph
//! closures on the executor it was built with, and its clones and a
//! `CoalitionManager::empty` graph hold that same `Arc`. With the executor's
//! only worker occupied, `should_join_async` / `should_leave_async` of
//! `AifDecisionPolicy` and `AifMmDecisionPolicy` (feature `decision`) and of
//! `MagnitudePolicy`, untyped and typed (feature `magnitude`), stay pending,
//! then resolve to the sync result once the worker is released; so does
//! `TemporalAnalytics::magnitude_history` (feature `magnitude`), resolving to
//! one point of magnitude 2.0. A `CoalitionService`
//! is one task on the caller's `TaskTracker`, which drains once every handle
//! is dropped.

use std::sync::Arc;
use std::sync::mpsc as std_mpsc;
use std::time::Duration;

use anyhow::Result;
use koalisi::algorithms::{AdditiveCalculator, CapabilityAgent};
use koalisi::core::Executor;
use koalisi::decision::{DecisionContext, ThresholdPolicy};
use koalisi::subsystems::coalition_actor::CoalitionService;
use koalisi::topology::{CoalitionManager, TemporalHypergraph};
use tokio_rayon::rayon;
use tokio_util::task::TaskTracker;

/// How long a future must stay pending while the executor is occupied.
const PENDING_FOR: Duration = Duration::from_millis(300);

fn thread_name() -> Option<String> {
    std::thread::current().name().map(str::to_owned)
}

fn on_exec_thread(name: Option<&str>) -> bool {
    name.is_some_and(|n| n.starts_with("koalisi-exec-"))
}

/// Occupy the only worker of a one-thread `exec` until the returned sender
/// sends or drops.
#[cfg(any(feature = "decision", feature = "magnitude"))]
async fn occupy(exec: &Arc<Executor>) -> std_mpsc::Sender<()> {
    let (release_tx, release_rx) = std_mpsc::channel::<()>();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let exec = Arc::clone(exec);
    tokio::spawn(async move {
        exec.run(move || {
            let _ = started_tx.send(());
            let _ = release_rx.recv();
        })
        .await;
    });
    started_rx.await.expect("the blocker job starts");
    release_tx
}

/// Assert `fut` stays pending for [`PENDING_FOR`] while the executor is
/// occupied, then release the executor and resolve `fut`.
#[cfg(any(feature = "decision", feature = "magnitude"))]
async fn pending_until_released<F>(
    label: &str,
    release: std_mpsc::Sender<()>,
    mut fut: F,
) -> F::Output
where
    F: std::future::Future + Unpin,
    F::Output: std::fmt::Debug,
{
    let early = tokio::time::timeout(PENDING_FOR, &mut fut).await;
    assert!(
        early.is_err(),
        "{label}: expected the future to stay pending while the executor's only worker is occupied, observed it resolve to {early:?}"
    );
    release.send(()).expect("the blocker job is waiting");
    fut.await
}

#[tokio::test]
async fn run_returns_value_on_exec_thread() -> Result<()> {
    let exec = Executor::new()?;
    let (value, name) = exec.run(|| (6 * 7, thread_name())).await;
    assert_eq!(value, 42, "expected the closure's value 42, got {value}");
    assert!(
        on_exec_thread(name.as_deref()),
        "expected a thread named koalisi-exec-*, observed {name:?}"
    );
    Ok(())
}

#[tokio::test]
async fn with_num_threads_sizes_the_pool() -> Result<()> {
    let exec = Executor::with_num_threads(2)?;
    let threads = exec.run(rayon::current_num_threads).await;
    assert_eq!(
        threads, 2,
        "expected the executor's 2-thread pool, observed {threads}"
    );
    Ok(())
}

#[tokio::test]
#[should_panic(expected = "koalisi-exec job panicked")]
async fn run_resumes_panic_on_awaiting_task() {
    let exec = Executor::new().expect("pool builds");
    exec.run(|| panic!("koalisi-exec job panicked")).await;
}

#[tokio::test]
async fn graph_mutation_runs_on_the_graphs_executor() -> Result<()> {
    let exec = Arc::new(Executor::with_num_threads(2)?);
    let graph = TemporalHypergraph::<u32, u32>::new(Arc::clone(&exec));
    let v = graph.add_vertex(1).await?;
    let he = graph.add_hyperedge(vec![v], 7).await?;

    let (tx, rx) = std_mpsc::channel();
    graph
        .update_hyperedge_vertices_try(he, move |members| {
            let _ = tx.send((thread_name(), rayon::current_num_threads()));
            Ok(members)
        })
        .await?;
    let (name, threads) = rx.recv()?;
    assert!(
        on_exec_thread(name.as_deref()) && threads == 2,
        "expected the mutation on koalisi-exec-* in the graph's 2-thread pool, observed (thread name, pool size) = ({name:?}, {threads})"
    );
    Ok(())
}

#[tokio::test]
async fn graph_clone_and_manager_share_the_executor() -> Result<()> {
    let exec = Arc::new(Executor::with_num_threads(1)?);
    let graph = TemporalHypergraph::<u32, u32>::new(Arc::clone(&exec));
    let clone = graph.clone();
    let manager = CoalitionManager::<u32, u32>::empty(Arc::clone(&exec));
    let shared = [
        Arc::ptr_eq(graph.executor(), &exec),
        Arc::ptr_eq(clone.executor(), &exec),
        Arc::ptr_eq(manager.graph().executor(), &exec),
    ];
    assert_eq!(
        shared, [true; 3],
        "expected (graph, clone, manager graph) to hold the caller's executor, observed {shared:?}"
    );
    Ok(())
}

#[tokio::test]
async fn service_runs_on_the_callers_tracker() -> Result<()> {
    let tracker = TaskTracker::new();
    let manager =
        CoalitionManager::<CapabilityAgent, u32>::empty(Arc::new(Executor::with_num_threads(1)?));
    let handle = CoalitionService::spawn(
        manager,
        Box::new(ThresholdPolicy::new(AdditiveCalculator, 0.0, 0.0)),
        DecisionContext::default(),
        &tracker,
    );
    let second = handle.clone();
    let tracked = tracker.len();
    assert_eq!(
        tracked, 1,
        "expected the service as the tracker's one task, observed {tracked}"
    );

    tracker.close();
    let early = tokio::time::timeout(PENDING_FOR, tracker.wait()).await;
    assert!(
        early.is_err(),
        "expected tracker.wait() to stay pending while handles are alive, observed it return"
    );

    drop(handle);
    drop(second);
    let drained = tokio::time::timeout(Duration::from_secs(5), tracker.wait()).await;
    assert!(
        drained.is_ok(),
        "expected tracker.wait() to return within 5 s of dropping every handle, observed a timeout"
    );
    Ok(())
}

#[cfg(any(feature = "decision", feature = "magnitude"))]
mod policies {
    use super::*;
    use koalisi::algorithms::AgentCapabilities;
    use koalisi::decision::CoalitionDecisionPolicy;

    /// With the executor's only worker occupied, `policy`'s async join and
    /// leave stay pending, then equal the sync decisions once released.
    pub(super) async fn joins_and_leaves_wait_for_the_executor(
        label: &str,
        policy: &dyn CoalitionDecisionPolicy,
    ) -> Result<()> {
        let exec = Arc::new(Executor::with_num_threads(1)?);
        let ctx = DecisionContext {
            required_capabilities: 0b111,
        };
        let a0 = CapabilityAgent::new(0, 0b001, 50);
        let a1 = CapabilityAgent::new(1, 0b010, 50);
        let a2 = CapabilityAgent::new(2, 0b100, 50);
        let coalition: [&dyn AgentCapabilities; 2] = [&a1, &a2];
        let full: [&dyn AgentCapabilities; 3] = [&a0, &a1, &a2];

        let release = occupy(&exec).await;
        let join = pending_until_released(
            &format!("{label} should_join_async"),
            release,
            policy.should_join_async(&a0, &coalition, &ctx, &exec),
        )
        .await;
        let want = policy.should_join(&a0, &coalition, &ctx);
        assert_eq!(join, want, "{label}: async join vs sync join");

        let release = occupy(&exec).await;
        let leave = pending_until_released(
            &format!("{label} should_leave_async"),
            release,
            policy.should_leave_async(&a0, &full, &ctx, &exec),
        )
        .await;
        let want = policy.should_leave(&a0, &full, &ctx);
        assert_eq!(leave, want, "{label}: async leave vs sync leave");
        Ok(())
    }
}

#[cfg(feature = "decision")]
#[tokio::test]
async fn aif_policy_runs_on_the_executor() -> Result<()> {
    use koalisi::decision::AifDecisionPolicy;
    policies::joins_and_leaves_wait_for_the_executor(
        "AifDecisionPolicy",
        &AifDecisionPolicy::default(),
    )
    .await
}

#[cfg(feature = "decision")]
#[tokio::test]
async fn aif_mm_policy_runs_on_the_executor() -> Result<()> {
    use koalisi::decision::AifMmDecisionPolicy;
    policies::joins_and_leaves_wait_for_the_executor(
        "AifMmDecisionPolicy",
        &AifMmDecisionPolicy::default(),
    )
    .await
}

#[cfg(feature = "magnitude")]
#[tokio::test]
async fn magnitude_policy_runs_on_the_executor() -> Result<()> {
    use koalisi::decision::{MagnitudePolicy, RoleModulation};
    use std::collections::HashMap;

    policies::joins_and_leaves_wait_for_the_executor(
        "MagnitudePolicy",
        &MagnitudePolicy::default(),
    )
    .await?;
    let roles: HashMap<usize, usize> = [(0, 0), (1, 1), (2, 2)].into();
    let rho = RoleModulation::new(vec![vec![1.0; 3]; 3]).expect("an all-ones table is valid");
    policies::joins_and_leaves_wait_for_the_executor(
        "typed MagnitudePolicy",
        &MagnitudePolicy::new(0.0).with_role_modulation(roles, rho),
    )
    .await
}

#[cfg(feature = "magnitude")]
#[tokio::test]
async fn magnitude_history_runs_on_the_executor() -> Result<()> {
    use koalisi::topology::{TemporalAnalytics, TimeRange};

    let exec = Arc::new(Executor::with_num_threads(1)?);
    let graph = TemporalHypergraph::<CapabilityAgent, u32>::new(Arc::clone(&exec));
    let v0 = graph.add_vertex(CapabilityAgent::new(0, 0b001, 50)).await?;
    let v1 = graph.add_vertex(CapabilityAgent::new(1, 0b010, 50)).await?;
    let he = graph.add_hyperedge(vec![v0, v1], 1).await?;

    let release = occupy(&exec).await;
    let points = pending_until_released(
        "magnitude_history",
        release,
        Box::pin(TemporalAnalytics::magnitude_history(
            graph.events_ref(),
            he,
            0b011,
            &TimeRange::all(),
            &exec,
        )),
    )
    .await;
    let mags: Vec<f64> = points.iter().map(|p| p.magnitude).collect();
    assert!(
        mags.len() == 1 && (mags[0] - 2.0).abs() < 1e-9,
        "expected one point of magnitude 2.0 for two disjoint specialists, observed {mags:?}"
    );
    Ok(())
}
