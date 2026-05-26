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
