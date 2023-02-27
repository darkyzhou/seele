use once_cell::sync::Lazy;
use tokio::{
    sync::Semaphore,
    task::{self, JoinError},
};

use crate::conf;

static RUNNERS: Lazy<Semaphore> = Lazy::new(|| Semaphore::new(conf::CONFIG.thread_counts.runner));

pub async fn spawn_blocking<F, R>(f: F) -> Result<R, JoinError>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let _permit = RUNNERS.acquire().await.unwrap();
    task::spawn_blocking(f).await
}
