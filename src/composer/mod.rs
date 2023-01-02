use crate::{entity::SubmissionConfig, worker::WorkerQueueTx};
use anyhow::Context;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_graceful_shutdown::{FutureExt, SubsystemHandle};
use tracing::error;

mod execute;
mod predicate;
mod resolve;

pub type SubmissionProgressTx = ring_channel::RingSender<()>;
pub type ComposerQueueItem = (Arc<SubmissionConfig>, SubmissionProgressTx);

pub async fn composer_main(
    handle: SubsystemHandle,
    mut composer_queue_rx: mpsc::Receiver<ComposerQueueItem>,
    worker_queue_tx: WorkerQueueTx,
) -> anyhow::Result<()> {
    while let Ok(Some((submission, mut tx))) =
        composer_queue_rx.recv().cancel_on_shutdown(&handle).await
    {
        tokio::spawn({
            let worker_queue_tx = worker_queue_tx.clone();
            async move {
                // TODO: pass the `handle`
                match handle_submission(submission, worker_queue_tx, tx.clone()).await {
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
    submission: Arc<SubmissionConfig>,
    worker_queue_tx: WorkerQueueTx,
    submission_progress_tx: SubmissionProgressTx,
) -> anyhow::Result<()> {
    let submission =
        resolve::resolve_submission(submission).context("Failed to resolve the submission")?;

    execute::execute_submission(submission, worker_queue_tx, submission_progress_tx)
        .await
        .context("Error executing the submission")?;

    Ok(())
}
