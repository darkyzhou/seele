use std::sync::atomic::{AtomicU64, Ordering};

use once_cell::sync::Lazy;
use tokio::{
    sync::Semaphore,
    task::{self, JoinError},
};

use crate::conf;

pub static PENDING_TASKS: Lazy<AtomicU64> = Lazy::new(|| AtomicU64::new(0));

static RUNNERS: Lazy<Semaphore> = Lazy::new(|| Semaphore::new(conf::CONFIG.thread_counts.runner));

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
