use rayon::ThreadPoolBuilder;
use std::sync::LazyLock;
use tokio_rayon::AsyncThreadPool;

pub static EXEC: LazyLock<HypergraphExecutor> = LazyLock::new(|| HypergraphExecutor::new());

pub struct HypergraphExecutor {
    pool: rayon::ThreadPool,
}

impl HypergraphExecutor {
    fn new() -> Self {
        let pool = ThreadPoolBuilder::new().build().unwrap();
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
