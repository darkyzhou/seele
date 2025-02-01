use std::sync::{
    LazyLock,
    atomic::{AtomicU64, Ordering},
};

use tokio::{
    sync::Semaphore,
    task::{self, JoinError},
};

use crate::conf;

pub static PENDING_TASKS: LazyLock<AtomicU64> = LazyLock::new(|| AtomicU64::new(0));

static RUNNERS: LazyLock<Semaphore> =
    LazyLock::new(|| Semaphore::new(conf::CONFIG.thread_counts.runner));

pub async fn spawn_blocking<F, R>(f: F) -> Result<R, JoinError>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    PENDING_TASKS.fetch_add(1, Ordering::SeqCst);
    let _permit = RUNNERS.acquire().await.unwrap();
    PENDING_TASKS.fetch_sub(1, Ordering::SeqCst);

    task::spawn_blocking(f).await
}
