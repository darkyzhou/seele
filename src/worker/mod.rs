use crate::{
    conf,
    entity::{ActionTaskConfig, TaskConfig, TaskReport},
};
use std::sync::Arc;
use tokio::sync::oneshot;
use tokio_graceful_shutdown::SubsystemHandle;

pub type WorkerQueueItem = (Arc<ActionTaskConfig>, oneshot::Sender<TaskReport>);

pub async fn worker_main(
    handle: SubsystemHandle,
    queue_rx: async_channel::Receiver<WorkerQueueItem>,
) -> anyhow::Result<()> {
    for i in 0..conf::CONFIG.concurrency {
        let queue_rx = queue_rx.clone();
        handle.start(&format!("worker-{}", i), |handle| worker_main_impl(handle, queue_rx));
    }

    handle.on_shutdown_requested().await;
    Ok(())
}

async fn worker_main_impl(
    handle: SubsystemHandle,
    queue_rx: async_channel::Receiver<WorkerQueueItem>,
) -> anyhow::Result<()> {
    loop {}

    Ok(())
}

async fn handle_task(task: Arc<TaskConfig>) -> anyhow::Result<()> {
    Ok(())
}
