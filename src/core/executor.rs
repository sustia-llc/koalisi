//! Caller-owned rayon thread pool whose jobs are awaited from async code.

use tokio_rayon::AsyncThreadPool;
use tokio_rayon::rayon;

/// Failure to build an [`Executor`]'s thread pool.
#[derive(Debug, thiserror::Error)]
pub enum ExecutorError {
    /// rayon refused to build the pool. `num_threads` is the requested thread
    /// count, `None` for rayon's default.
    #[error("building the koalisi-exec thread pool (num_threads: {num_threads:?}) failed")]
    Build {
        num_threads: Option<usize>,
        #[source]
        source: rayon::ThreadPoolBuildError,
    },
}

/// A rayon thread pool with threads named `koalisi-exec-{i}`, running each
/// submitted closure on the pool and resolving to its value.
#[derive(Debug)]
pub struct Executor {
    pool: rayon::ThreadPool,
}

impl Executor {
    /// Builds an executor with rayon's default thread count, or
    /// [`ExecutorError::Build`] when the pool cannot be built.
    pub fn new() -> Result<Self, ExecutorError> {
        Self::build(rayon::ThreadPoolBuilder::new(), None)
    }

    /// Builds an executor with `num_threads` threads (rayon's default count
    /// when `0`), or [`ExecutorError::Build`] when the pool cannot be built.
    pub fn with_num_threads(num_threads: usize) -> Result<Self, ExecutorError> {
        Self::build(
            rayon::ThreadPoolBuilder::new().num_threads(num_threads),
            Some(num_threads),
        )
    }

    fn build(
        builder: rayon::ThreadPoolBuilder,
        num_threads: Option<usize>,
    ) -> Result<Self, ExecutorError> {
        let pool = builder
            .thread_name(|i| format!("koalisi-exec-{i}"))
            .build()
            .map_err(|source| ExecutorError::Build {
                num_threads,
                source,
            })?;
        Ok(Self { pool })
    }

    /// Runs `f` on the pool and resolves to its return value; a panic in `f`
    /// resumes on the awaiting task.
    ///
    /// Dropping the returned future before it resolves abandons the result,
    /// not the job: `f` still runs to completion on the pool, and a panic in
    /// `f` is then resumed on no task (tokio-rayon 2.1.0
    /// `src/async_thread_pool.rs:58-59` sends the caught result on a oneshot
    /// whose send error is discarded).
    pub async fn run<F, R>(&self, f: F) -> R
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        self.pool.spawn_async(f).await
    }
}

/// A two-thread executor for the crate's unit tests.
#[cfg(test)]
pub(crate) fn test_executor() -> std::sync::Arc<Executor> {
    std::sync::Arc::new(Executor::with_num_threads(2).expect("two-thread pool builds"))
}
