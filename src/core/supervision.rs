//! Thin task-restart layer (issue #6).
//!
//! Replaces kameo's `OneForOne` supervision for the tokio::sync runtime. A
//! *supervisor* task owns a `factory` closure that builds a fresh task instance
//! on every (re)start. It spawns the inner future, awaits its `JoinHandle`, and:
//!
//! - if the inner task **panicked** and the restart budget allows, rebuilds it
//!   from the factory (up to `restart_limit` restarts within a sliding
//!   `window`; exceeding the budget logs an error and gives up);
//! - if the inner task **completed cleanly** (`Ok(())`) or the parent
//!   [`CancellationToken`] was cancelled, stops (cancellation is *not* a
//!   failure — no restart).
//!
//! Each inner instance is built with a fresh `child_token()` of the parent
//! token, so cancelling the parent stops both the running instance and the
//! supervisor. The supervisor runs on the provided [`TaskTracker`], so it
//! participates in the swarm's three-step shutdown.

use std::collections::VecDeque;
use std::future::Future;
use std::time::{Duration, Instant};

use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

/// Spawn a supervised task on `tracker` under `token`.
///
/// `factory` is called once per (re)start with a fresh child cancellation
/// token and must return the task future. On panic the task is restarted up to
/// `restart_limit` times within a sliding `window`; a cancelled or
/// cleanly-completed task is not restarted.
///
/// Returns the supervisor's `JoinHandle`.
pub fn spawn_supervised<F, Fut>(
    tracker: &TaskTracker,
    token: CancellationToken,
    restart_limit: usize,
    window: Duration,
    factory: F,
) -> tokio::task::JoinHandle<()>
where
    F: Fn(CancellationToken) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    tracker.spawn(supervise(token, restart_limit, window, factory))
}

async fn supervise<F, Fut>(
    token: CancellationToken,
    restart_limit: usize,
    window: Duration,
    factory: F,
) where
    F: Fn(CancellationToken) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    // Sliding window of recent restart instants.
    let mut restarts: VecDeque<Instant> = VecDeque::new();

    loop {
        if token.is_cancelled() {
            return;
        }

        // Build + spawn a fresh instance. We spawn via `tokio::spawn` (not the
        // tracker) so its `JoinHandle` surfaces a panic; the supervisor itself
        // is tracked, and it awaits this handle, so drain still transitively
        // waits for the instance.
        let child = token.child_token();
        let handle = tokio::spawn(factory(child));

        match handle.await {
            Ok(()) => {
                // Clean completion — nothing to restart.
                return;
            }
            Err(join_err) if join_err.is_cancelled() => {
                // Aborted (we never abort deliberately) — treat as done.
                return;
            }
            Err(join_err) => {
                // Panic. If the parent was cancelled meanwhile, don't restart.
                debug_assert!(join_err.is_panic());
                if token.is_cancelled() {
                    return;
                }

                // Prune restarts older than the window, then check the budget.
                let now = Instant::now();
                while let Some(&front) = restarts.front() {
                    if now.duration_since(front) > window {
                        restarts.pop_front();
                    } else {
                        break;
                    }
                }

                if restarts.len() >= restart_limit {
                    tracing::error!(
                        restart_limit,
                        window_secs = window.as_secs_f64(),
                        "supervised task exceeded its restart budget; giving up"
                    );
                    token.cancel();
                    return;
                }

                restarts.push_back(now);
                tracing::warn!(
                    restart = restarts.len(),
                    restart_limit,
                    "supervised task panicked; restarting"
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// A panicking task restarts up to N times then gives up (N+1 total runs).
    #[tokio::test]
    async fn panicking_task_restarts_then_gives_up() {
        let tracker = TaskTracker::new();
        let token = CancellationToken::new();
        // Pass a child we retain so we can observe the give-up cancellation.
        let child = token.child_token();
        let runs = Arc::new(AtomicUsize::new(0));

        let runs2 = runs.clone();
        spawn_supervised(&tracker, child.clone(), 3, Duration::from_secs(60), move |_grandchild| {
            let runs = runs2.clone();
            async move {
                runs.fetch_add(1, Ordering::SeqCst);
                panic!("boom");
            }
        });

        tracker.close();
        tracker.wait().await;

        // 1 initial run + 3 restarts = 4 runs; the 4th panic exceeds the budget.
        assert_eq!(runs.load(Ordering::SeqCst), 4);
        // Giving up cancels the token the supervisor was given.
        assert!(child.is_cancelled());
    }

    /// A cancelled task is not restarted.
    #[tokio::test]
    async fn cancelled_task_does_not_restart() {
        let tracker = TaskTracker::new();
        let token = CancellationToken::new();
        let runs = Arc::new(AtomicUsize::new(0));

        let runs2 = runs.clone();
        spawn_supervised(&tracker, token.child_token(), 5, Duration::from_secs(60), move |child| {
            let runs = runs2.clone();
            async move {
                runs.fetch_add(1, Ordering::SeqCst);
                child.cancelled().await; // wait to be cancelled, then return
            }
        });

        // Let it start, then cancel.
        tokio::time::sleep(Duration::from_millis(20)).await;
        token.cancel();
        tracker.close();
        tracker.wait().await;

        assert_eq!(runs.load(Ordering::SeqCst), 1, "cancellation is not a failure");
    }

    /// A healthy long-running task is left untouched.
    #[tokio::test]
    async fn healthy_task_is_not_restarted() {
        let tracker = TaskTracker::new();
        let token = CancellationToken::new();
        let runs = Arc::new(AtomicUsize::new(0));

        let runs2 = runs.clone();
        spawn_supervised(&tracker, token.child_token(), 3, Duration::from_secs(60), move |child| {
            let runs = runs2.clone();
            async move {
                runs.fetch_add(1, Ordering::SeqCst);
                child.cancelled().await;
            }
        });

        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(runs.load(Ordering::SeqCst), 1, "healthy task runs exactly once");

        token.cancel();
        tracker.close();
        tracker.wait().await;
    }
}
