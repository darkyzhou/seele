use std::sync::Arc;

use anyhow::{bail, Context};
use tokio::{fs, sync::mpsc};
use tokio_graceful_shutdown::{FutureExt, SubsystemHandle};
use tracing::{debug, debug_span, error, instrument, Instrument};

use crate::{conf, entities::SubmissionConfig, worker::WorkerQueueTx};

mod execute;
mod predicate;
mod report;
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
                    error!("Error handling the submission: {:#}", err);
                }
            }
            .instrument(span)
        });
    }

    Ok(())
}

#[instrument(level = "debug")]
async fn handle_submission(
    submission: Arc<SubmissionConfig>,
    worker_queue_tx: WorkerQueueTx,
    status_tx: ring_channel::RingSender<()>,
) -> anyhow::Result<()> {
    let submission_root = conf::PATHS.submissions.join(&submission.id);
    if fs::metadata(&submission_root).await.is_ok() {
        bail!(
            "The submission's directory already exists, it may indicate a duplicate submission \
             id: {}",
            submission_root.display()
        )
    }

    fs::create_dir_all(&submission_root)
        .await
        .context("Error creating the submission directory")?;

    debug!("Resolving the submission");
    let submission = resolve::resolve_submission(submission, submission_root)
        .context("Failed to resolve the submission")?;

    debug!("Executing the submission");
    execute::execute_submission(submission, worker_queue_tx, status_tx)
        .await
        .context("Error executing the submission")?;

    Ok(())
}
