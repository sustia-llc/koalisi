use tokio_util::{sync::CancellationToken, task::TaskTracker};

pub struct CoalitionRuntime {
    task_tracker: TaskTracker,
    cancellation_token: CancellationToken,
}

impl CoalitionRuntime {
    pub fn new() -> Self {
        Self {
            task_tracker: TaskTracker::new(),
            cancellation_token: CancellationToken::new(),
        }
    }

    pub fn task_tracker(&self) -> &TaskTracker {
        &self.task_tracker
    }

    pub fn cancellation_token(&self) -> &CancellationToken {
        &self.cancellation_token
    }

    /// Three-step shutdown: cancel the root token, close the tracker,
    /// then drain all tracked tasks. Callers are responsible for stopping
    /// their own actors after this returns.
    pub async fn shutdown(self) {
        self.cancellation_token.cancel();
        self.task_tracker.close();
        self.task_tracker.wait().await;
    }
}

impl Default for CoalitionRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    /// `shutdown` cancels the root token so a long-running child task stops,
    /// then drains it — the three-step contract, end to end.
    #[tokio::test]
    async fn shutdown_cancels_and_drains_a_running_task() {
        let runtime = CoalitionRuntime::new();
        let stopped = Arc::new(AtomicUsize::new(0));

        let child = runtime.cancellation_token().child_token();
        let flag = stopped.clone();
        runtime.task_tracker().spawn(async move {
            // Loop until cancelled — only the token can end this task.
            loop {
                tokio::select! {
                    () = child.cancelled() => break,
                    () = tokio::time::sleep(Duration::from_secs(3600)) => {}
                }
            }
            flag.fetch_add(1, Ordering::SeqCst);
        });

        // Task is parked; shutdown must cancel + drain it (this await returns
        // only once the task has run to completion).
        runtime.shutdown().await;
        assert_eq!(
            stopped.load(Ordering::SeqCst),
            1,
            "the running task must observe cancellation and finish before shutdown returns"
        );
    }

    /// A task that finishes on its own is still drained by `shutdown`.
    #[tokio::test]
    async fn shutdown_drains_a_self_completing_task() {
        let runtime = CoalitionRuntime::new();
        let done = Arc::new(AtomicUsize::new(0));

        let flag = done.clone();
        runtime.task_tracker().spawn(async move {
            flag.fetch_add(1, Ordering::SeqCst);
        });

        runtime.shutdown().await;
        assert_eq!(done.load(Ordering::SeqCst), 1);
        // The token is cancelled even when nothing observed it.
        // (shutdown consumed `self`; observing via a fresh runtime is not
        // meaningful, so we only assert the drain side-effect above.)
    }
}
