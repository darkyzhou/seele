use crate::{
    entity::{SubmissionConfig, TaskConfig},
    worker::WorkerQueueItem,
};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tokio_graceful_shutdown::SubsystemHandle;
use tracing::error;

pub type ComposerQueueItem = (SubmissionConfig, oneshot::Sender<SubmissionConfig>);

pub async fn composer_main(
    handle: SubsystemHandle,
    mut composer_queue_rx: mpsc::Receiver<ComposerQueueItem>,
    worker_queue_tx: async_channel::Sender<WorkerQueueItem>,
) -> anyhow::Result<()> {
    loop {
        while let Some((submission, tx)) = composer_queue_rx.recv().await {
            tokio::spawn(async move {
                let result = handle_submission(submission).await;
                if let Err(err) = tx.send(result) {
                    error!("Error sending result to tx: {:#?}", err);
                }
            });
        }
    }
}

async fn handle_submission(submission: SubmissionConfig) -> SubmissionConfig {
    todo!()
}

async fn submit_task(
    worker_queue_tx: async_channel::Sender<WorkerQueueItem>,
    task: Arc<TaskConfig>,
) -> anyhow::Result<()> {
    let (tx, rx) = oneshot::channel();
    worker_queue_tx.send((task, tx)).await?;

    // TODO: timeout
    Ok(rx.await?)
}
