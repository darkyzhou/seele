use std::sync::Arc;

use anyhow::{bail, Context, Result};
use opentelemetry::{Context as OpenTelemetryCtx, KeyValue};
use serde_json::Value;
use tokio::{fs, sync::mpsc, time::Instant};
use tokio_graceful_shutdown::{FutureExt, SubsystemHandle};
use tracing::{debug, error, field, info_span, Instrument, Span};

pub use self::signal::*;
use crate::{conf, entities::SubmissionConfig, shared::metrics, worker::WorkerQueueTx};

mod execute;
mod predicate;
mod report;
mod resolve;
mod signal;

pub type ComposerQueueTx = mpsc::Sender<ComposerQueueItem>;
pub type ComposerQueueRx = mpsc::Receiver<ComposerQueueItem>;

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
    const SUBMISSION_ID: &str = "submission.id";
    const SUBMISSION_ERROR_INTERNAL: &str = "submission.error.internal";
    const SUBMISSION_ERROR_EXECUTION: &str = "submission.error.execution";
    const SUBMISSION_ERROR_REPORTER: &str = "submission.error.reporter";
    const SUBMISSION_ATTRIBUTE: &str = "submission.attribute";

    while let Ok(Some(item)) = composer_queue_rx.recv().cancel_on_shutdown(&handle).await {
        tokio::spawn({
            let span = info_span!(
                "submission_entry",
                submission.id = field::Empty,
                submission.attribute = field::Empty,
                submission.error.internal = false,
                submission.error.execution = false,
                submission.error.reporter = false,
            );
            let worker_queue_tx = worker_queue_tx.clone();
            async move {
                let ComposerQueueItem { config_yaml, status_tx } = item;
                let mut outer_status_tx = status_tx.clone();

                let result = async move {
                    let submission: Arc<SubmissionConfig> = serde_yaml::from_str(&config_yaml)
                        .context("Error parsing the submission config")?;
                    Span::current().record(SUBMISSION_ID, &submission.id);
                    Span::current().record(SUBMISSION_ATTRIBUTE, &submission.tracing_attribute);

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
                            KeyValue::new(
                                SUBMISSION_ERROR_EXECUTION,
                                matches!(result, Ok((false, _, _))),
                            ),
                        ],
                    );

                    result
                }
                .await;

                match result {
                    Err(err) => {
                        Span::current().record(SUBMISSION_ERROR_INTERNAL, true);
                        error!("Error executing the submission: {:#}", err);
                        _ = outer_status_tx.send(SubmissionSignal::Completed(
                            SubmissionCompletedSignal::InternalError {
                                error: format!("{:#}", err),
                            },
                        ));
                    }
                    Ok((success, status, report)) => {
                        let signal = match (success, report) {
                            (false, _) => {
                                Span::current().record(SUBMISSION_ERROR_EXECUTION, true);

                                let message = "The execution returned a failed report".to_string();
                                error!(message);
                                SubmissionCompletedSignal::ExecutionError { error: message, status }
                            }
                            (true, Some(Err(err))) => {
                                Span::current().record(SUBMISSION_ERROR_REPORTER, true);

                                let message =
                                    format!("The reporter of the submission failed: {:#}", err);
                                error!(message);
                                SubmissionCompletedSignal::ReporterError { error: message, status }
                            }
                            (true, Some(Ok(report))) => {
                                SubmissionCompletedSignal::Success { status, report: Some(report) }
                            }
                            (true, None) => {
                                SubmissionCompletedSignal::Success { status, report: None }
                            }
                        };

                        _ = outer_status_tx.send(SubmissionSignal::Completed(signal));
                    }
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
    execute::execute_submission(submission, worker_queue_tx, status_tx)
        .await
        .context("Error executing the submission")
}
