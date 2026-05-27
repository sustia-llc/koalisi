//! Rayon ↔ tokio bridge for the topology layer.
//!
//! The underlying `hypergraph::Hypergraph` is `Send + Sync` but operates
//! synchronously on a `std::sync::RwLock`. We don't want to hold the std
//! lock across `.await`, so all graph mutations run on a dedicated rayon
//! pool via `tokio_rayon::AsyncThreadPool`, then the future returned to
//! the async caller resolves once rayon finishes.
//!
//! The `EXEC` singleton uses rayon's default pool size (one thread per
//! logical CPU). For deployments that want to share a pool with other
//! subsystems, construct an explicit `HypergraphExecutor::with_num_threads`.

use rayon::ThreadPoolBuilder;
use std::sync::LazyLock;
use tokio_rayon::AsyncThreadPool;

pub(crate) static EXEC: LazyLock<HypergraphExecutor> =
    LazyLock::new(HypergraphExecutor::new);

/// Bridges synchronous graph operations onto a rayon thread pool so
/// async callers can `.await` them without blocking the tokio runtime.
pub struct HypergraphExecutor {
    pool: rayon::ThreadPool,
}

impl HypergraphExecutor {
    fn new() -> Self {
        let pool = ThreadPoolBuilder::new().build().unwrap();
        Self { pool }
    }

    /// Create an executor with an explicit thread count. Use this when
    /// embedding into a runtime that already partitions CPUs across
    /// multiple rayon pools.
    pub fn with_num_threads(num_threads: usize) -> Self {
        let pool = ThreadPoolBuilder::new()
            .num_threads(num_threads)
            .build()
            .unwrap();
        Self { pool }
    }

    pub async fn run_job<F, R>(&self, f: F) -> R
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        self.pool.spawn_async(f).await
    }
}
