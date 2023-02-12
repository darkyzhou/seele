use std::sync::Arc;

use anyhow::{bail, Context, Result};
use opentelemetry::{Context as OpenTelemetryCtx, KeyValue};
use tokio::{fs, sync::mpsc, time::Instant};
use tokio_graceful_shutdown::{FutureExt, SubsystemHandle};
use tracing::{debug, error, info_span, Instrument, Span};

use crate::{conf, entities::SubmissionConfig, shared::metrics, worker::WorkerQueueTx};

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
    const SUBMISSION_ERROR_INTERNAL: &str = "submission.error.internal";
    const SUBMISSION_ERROR_EXECUTION: &str = "submission.error.execution";

    while let Ok(Some((submission, status_tx))) =
        composer_queue_rx.recv().cancel_on_shutdown(&handle).await
    {
        tokio::spawn({
            let span = info_span!(
                "submission_entry",
                submission.id = submission.id,
                submission.error.internal = false,
                submission.error.execution = false
            );
            let worker_queue_tx = worker_queue_tx.clone();
            async move {
                let begin = Instant::now();
                let result = handle_submission(submission, worker_queue_tx, status_tx).await;
                let duration = {
                    let end = Instant::now();
                    end.duration_since(begin).as_secs_f64()
                };

                metrics::SUBMISSION_HANDLING_HISTOGRAM.record(
                    &OpenTelemetryCtx::current(),
                    duration,
                    &vec![
                        KeyValue::new(SUBMISSION_ERROR_INTERNAL, result.is_err()),
                        KeyValue::new(SUBMISSION_ERROR_EXECUTION, matches!(result, Ok(false))),
                    ],
                );

                match result {
                    Err(err) => {
                        Span::current().record(SUBMISSION_ERROR_INTERNAL, true);
                        error!("Error executing the submission: {:#}", err);
                    }
                    Ok(false) => {
                        Span::current().record(SUBMISSION_ERROR_EXECUTION, true);
                        error!("The execution of the submission returned a failed report");
                    }
                    _ => {}
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
) -> Result<bool> {
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
    Ok(execute::execute_submission(submission, worker_queue_tx, status_tx)
        .await
        .context("Error executing the submission")?)
}
