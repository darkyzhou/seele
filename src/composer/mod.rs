use crate::{entity::SubmissionConfig, worker::WorkerQueueTx};
use anyhow::Context;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_graceful_shutdown::{FutureExt, SubsystemHandle};
use tracing::{debug, debug_span, error, Instrument};

mod execute;
mod predicate;
mod resolve;

pub type ComposerQueueItem = (Arc<SubmissionConfig>, ring_channel::RingSender<()>);
pub type ComposerQueueTx = mpsc::Sender<ComposerQueueItem>;
pub type ComposerQueueRx = mpsc::Receiver<ComposerQueueItem>;

pub async fn composer_main(
    handle: SubsystemHandle,
    mut composer_queue_rx: ComposerQueueRx,
    worker_queue_tx: WorkerQueueTx,
) -> anyhow::Result<()> {
    debug!("Composer ready to accept submissions");
    while let Ok(Some((submission, status_tx))) =
        composer_queue_rx.recv().cancel_on_shutdown(&handle).await
    {
        tokio::spawn({
            let span = debug_span!("composer_main_handle_submission", submission = ?submission);
            let worker_queue_tx = worker_queue_tx.clone();
            async move {
                // TODO: pass the `handle`
                debug!("Receives the submission, start handling");
                if let Err(err) = handle_submission(submission, worker_queue_tx, status_tx).await {
                    error!("Error handling the submission: {:#?}", err);
                }
            }
            .instrument(span)
        });
    }

    Ok(())
}

async fn handle_submission(
    submission: Arc<SubmissionConfig>,
    worker_queue_tx: WorkerQueueTx,
    status_tx: ring_channel::RingSender<()>,
) -> anyhow::Result<()> {
    debug!("Resolving the submission");
    let submission =
        resolve::resolve_submission(submission).context("Failed to resolve the submission")?;

    debug!("Executing the submission");
    execute::execute_submission(submission, worker_queue_tx, status_tx)
        .await
        .context("Error executing the submission")?;

    Ok(())
}
