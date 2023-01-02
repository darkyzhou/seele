use std::sync::Arc;

use crate::{entity::SubmissionConfig, worker::WorkerQueueItem};
use anyhow::Context;
use tokio::sync::mpsc;
use tokio_graceful_shutdown::{FutureExt, SubsystemHandle};
use tracing::error;

mod execute;
mod predicate;
mod resolve;

pub type ComposerQueueItem = (Arc<SubmissionConfig>, ring_channel::RingSender<()>);

pub async fn composer_main(
    handle: SubsystemHandle,
    mut composer_queue_rx: mpsc::Receiver<ComposerQueueItem>,
    worker_queue_tx: async_channel::Sender<WorkerQueueItem>,
) -> anyhow::Result<()> {
    while let Ok(Some((submission, mut tx))) =
        composer_queue_rx.recv().cancel_on_shutdown(&handle).await
    {
        tokio::spawn({
            let worker_queue_tx = worker_queue_tx.clone();
            async move {
                // TODO: pass the `handle`
                match handle_submission(worker_queue_tx, submission).await {
                    Err(err) => {
                        error!("Error handling the submission: {:#?}", err);
                    }
                    Ok(_) => {
                        if let Err(err) = tx.send(()) {
                            error!("Error sending the result to tx: {:#?}", err);
                        }
                    }
                }
            }
        });
    }

    Ok(())
}

async fn handle_submission(
    worker_queue_tx: async_channel::Sender<WorkerQueueItem>,
    submission: Arc<SubmissionConfig>,
) -> anyhow::Result<()> {
    let submission =
        resolve::resolve_submission(submission).context("Failed to resolve the submission")?;

    execute::execute_submission(worker_queue_tx, submission)
        .await
        .context("Error executing the submission")?;

    Ok(())
}
