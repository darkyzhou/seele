use std::sync::Arc;

use anyhow::{bail, Context, Result};
use opentelemetry::{Context as OpenTelemetryCtx, KeyValue};
use ring_channel::RingSender;
use serde_json::Value;
use tokio::{fs, sync::mpsc, time::Instant};
use tokio_graceful_shutdown::{FutureExt, SubsystemHandle};
use tracing::{debug, error, field, instrument, Span};

pub use self::signal::*;
use crate::{conf, entities::SubmissionConfig, shared::metrics, worker::WorkerQueueTx};

mod execute;
mod predicate;
mod report;
mod resolve;
mod signal;

pub type ComposerQueueTx = mpsc::Sender<ComposerQueueItem>;
pub type ComposerQueueRx = mpsc::Receiver<ComposerQueueItem>;

const SUBMISSION_ID: &str = "submission.id";
const SUBMISSION_STATUS: &str = "submission.status";
const SUBMISSION_ATTRIBUTE: &str = "submission.attribute";

#[derive(Debug)]
pub struct ComposerQueueItem {
    pub config_yaml: String,
    pub status_tx: ring_channel::RingSender<SubmissionSignal>,
}

pub async fn composer_main(
    handle: SubsystemHandle,
    mut composer_queue_rx: ComposerQueueRx,
    worker_queue_tx: WorkerQueueTx,
) -> Result<()> {
    while let Ok(Some(mut item)) = composer_queue_rx.recv().cancel_on_shutdown(&handle).await {
        tokio::spawn({
            let worker_queue_tx = worker_queue_tx.clone();
            async move {
                let begin = Instant::now();
                let completed_signal =
                    handle_submission(worker_queue_tx, item.config_yaml, item.status_tx.clone())
                        .await;
                let duration = {
                    let end = Instant::now();
                    end.duration_since(begin).as_secs_f64()
                };

                // Always true
                if let SubmissionSignalExt::Completed(ext) = &completed_signal.ext {
                    metrics::SUBMISSION_HANDLING_HISTOGRAM.record(
                        &OpenTelemetryCtx::current(),
                        duration,
                        &vec![KeyValue::new(SUBMISSION_STATUS, ext.get_type())],
                    );
                }

                _ = item.status_tx.send(completed_signal);
            }
        });
    }

    Ok(())
}

#[instrument(skip_all, fields(submission.id = field::Empty, submission.attribute = field::Empty, submission.status = field::Empty))]
async fn handle_submission(
    worker_queue_tx: WorkerQueueTx,
    config_yaml: String,
    progress_tx: RingSender<SubmissionSignal>,
) -> SubmissionSignal {
    let submission: serde_yaml::Result<Arc<SubmissionConfig>> = serde_yaml::from_str(&config_yaml);
    if let Err(err) = submission {
        let message = format!("Error parsing the submission: {:#}", err);
        error!(message);

        let signal = SubmissionCompletedSignal::ParseError { error: message };
        Span::current().record(SUBMISSION_STATUS, signal.get_type());

        return SubmissionSignal { id: None, ext: SubmissionSignalExt::Completed(signal) };
    }

    let submission = submission.unwrap();
    Span::current().record(SUBMISSION_ID, &submission.id);
    Span::current().record(SUBMISSION_ATTRIBUTE, &submission.tracing_attribute);

    let submission_id = submission.id.clone();
    let signal = match do_handle_submission(submission, worker_queue_tx, progress_tx).await {
        Err(err) => SubmissionCompletedSignal::InternalError { error: format!("{err:#}") },
        Ok((success, status, report)) => {
            let (report, report_error) = match report {
                None => (None, None),
                Some(Ok(report)) => (Some(report), None),
                Some(Err(err)) => (None, Some(format!("{err:#}"))),
            };
            if success {
                SubmissionCompletedSignal::Success { status, report, report_error }
            } else {
                SubmissionCompletedSignal::ExecutionError { status, report, report_error }
            }
        }
    };

    Span::current().record(SUBMISSION_STATUS, signal.get_type());
    SubmissionSignal { id: Some(submission_id), ext: SubmissionSignalExt::Completed(signal) }
}

async fn do_handle_submission(
    submission: Arc<SubmissionConfig>,
    worker_queue_tx: WorkerQueueTx,
    status_tx: ring_channel::RingSender<SubmissionSignal>,
) -> Result<(bool, Value, Option<Result<Value>>)> {
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
    let result = execute::execute_submission(submission, worker_queue_tx, status_tx)
        .await
        .context("Error executing the submission");

    match &result {
        Ok((false, _, Some(Err(err)))) => {
            error!("The execution returned a failed report and the reporter failed: {err:#}");
        }
        Ok((false, _, None)) => {
            error!("The execution returned a failed report");
        }
        Ok((true, _, Some(Err(err)))) => {
            error!("The execution succeeded but the reporter failed: {err:#}");
        }
        Err(err) => {
            error!("{err:#}");
        }
        _ => {}
    }

    result
}
