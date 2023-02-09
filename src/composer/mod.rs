use std::sync::Arc;

use anyhow::{bail, Context, Result};
use tokio::{fs, sync::mpsc};
use tokio_graceful_shutdown::{FutureExt, SubsystemHandle};
use tracing::{debug, error, info_span, Instrument};

use crate::{conf, entities::SubmissionConfig, worker::WorkerQueueTx};

mod execute;
mod predicate;
mod report;
mod resolve;

pub type ComposerQueueTx = mpsc::Sender<ComposerQueueItem>;
pub type ComposerQueueRx = mpsc::Receiver<ComposerQueueItem>;
pub type ComposerQueueItem =
    (Arc<SubmissionConfig>, ring_channel::RingSender<SubmissionUpdateSignal>);

pub enum SubmissionUpdateSignal {
    Progress,
    Finished,
}

pub async fn composer_main(
    handle: SubsystemHandle,
    mut composer_queue_rx: ComposerQueueRx,
    worker_queue_tx: WorkerQueueTx,
) -> Result<()> {
    debug!("Composer ready to accept submissions");
    while let Ok(Some((submission, status_tx))) =
        composer_queue_rx.recv().cancel_on_shutdown(&handle).await
    {
        tokio::spawn({
            let span =
                info_span!("composer_handle_submission", seele.submission.id = submission.id);
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

async fn handle_submission(
    submission: Arc<SubmissionConfig>,
    worker_queue_tx: WorkerQueueTx,
    status_tx: ring_channel::RingSender<SubmissionUpdateSignal>,
) -> Result<()> {
    let submission_root = conf::PATHS.submissions.join(&submission.id);
    if fs::metadata(&submission_root).await.is_ok() {
        bail!(
            "The submission's directory already exists, it may indicate a duplicate submission \
             id: {}",
            submission_root.display()
        );
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
