use crate::{
    entity::{Submission, SubmissionConfig},
    worker::WorkerQueueItem,
};
use anyhow::Context;
use tokio::sync::{mpsc, oneshot};
use tokio_graceful_shutdown::{FutureExt, SubsystemHandle};
use tracing::error;

mod execute;
mod resolve;

pub type ComposerQueueItem = (SubmissionConfig, oneshot::Sender<SubmissionConfig>);

pub async fn composer_main(
    handle: SubsystemHandle,
    mut composer_queue_rx: mpsc::Receiver<ComposerQueueItem>,
    worker_queue_tx: async_channel::Sender<WorkerQueueItem>,
) -> anyhow::Result<()> {
    loop {
        while let Ok(Some((submission, tx))) =
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
                        Ok(submission) => {
                            if let Err(err) = tx.send(submission.config) {
                                error!("Error sending the result to tx: {:#?}", err);
                            }
                        }
                    }
                }
            });
        }
    }
}

async fn handle_submission(
    worker_queue_tx: async_channel::Sender<WorkerQueueItem>,
    submission: SubmissionConfig,
) -> anyhow::Result<Submission> {
    let submission =
        resolve::resolve_submission(submission).context("Failed to resolve the submission")?;
    let submission = execute::execute_submission(worker_queue_tx, submission)
        .await
        .context("Error executing the submission")?;
    Ok(submission)
}
