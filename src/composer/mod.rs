use std::{num::NonZeroUsize, sync::Arc};

use anyhow::{bail, Context, Result};
use chrono::Utc;
use ellipse::Ellipse;
use futures_util::StreamExt;
use opentelemetry::{Context as OpenTelemetryCtx, KeyValue};
use ring_channel::{RingReceiver, RingSender};
use tokio::{
    fs,
    sync::mpsc::{self, error::TryRecvError},
    time::Instant,
};
use tokio_graceful_shutdown::{FutureExt, SubsystemHandle};
use tracing::{debug, error, field, instrument, Span};

pub use self::signal::*;
use crate::{
    composer::{report::apply_uploads_config, reporter::execute_reporter},
    conf,
    entities::{Submission, SubmissionConfig},
    shared::metrics,
    worker::WorkerQueueTx,
};

mod execute;
mod predicate;
mod report;
mod reporter;
mod resolve;
mod signal;

pub type ComposerQueueTx = mpsc::Sender<ComposerQueueItem>;
pub type ComposerQueueRx = mpsc::Receiver<ComposerQueueItem>;

const SUBMISSION_ID: &str = "seele.submission.id";
const SUBMISSION_STATUS: &str = "seele.submission.status";
const SUBMISSION_ATTRIBUTE: &str = "seele.submission.attribute";

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
    while let Ok(Some(item)) = composer_queue_rx.recv().cancel_on_shutdown(&handle).await {
        tokio::spawn(handle_submission(worker_queue_tx.clone(), item.config_yaml, item.status_tx));
    }

    Ok(())
}

#[instrument(skip_all, fields(seele.submission.id = field::Empty, seele.submission.attribute = field::Empty, seele.submission.status = field::Empty))]
async fn handle_submission(
    worker_queue_tx: WorkerQueueTx,
    config_yaml: String,
    mut status_tx: RingSender<SubmissionSignal>,
) {
    let begin = Instant::now();

    let submission = serde_yaml::from_str::<Arc<SubmissionConfig>>(&config_yaml);
    let Ok(submission) = submission else {
        let message = format!("Error parsing the submission: {:#}, partial content: {}", submission.err().unwrap(), config_yaml.as_str().truncate_ellipse(256));
        error!(message);

        let ext = SubmissionSignalExt::Error(SubmissionErrorSignal { error: message });
        Span::current().record(SUBMISSION_STATUS, ext.get_type());

        _ = status_tx.send(SubmissionSignal { id: None, ext });
        return;
    };

    Span::current().record(SUBMISSION_ID, &submission.id);
    Span::current().record(SUBMISSION_ATTRIBUTE, &submission.tracing_attribute);
    let signal_type = do_handle_submission(submission, worker_queue_tx, status_tx).await;
    Span::current().record(SUBMISSION_STATUS, signal_type);

    let duration = {
        let end = Instant::now();
        end.duration_since(begin).as_secs_f64()
    };
    metrics::SUBMISSION_HANDLING_HISTOGRAM.record(
        &OpenTelemetryCtx::current(),
        duration,
        &vec![KeyValue::new(SUBMISSION_STATUS, signal_type)],
    );
}

async fn do_handle_submission(
    submission: Arc<SubmissionConfig>,
    worker_queue_tx: WorkerQueueTx,
    mut status_tx: RingSender<SubmissionSignal>,
) -> &'static str {
    let submission_root = conf::PATHS.submissions.join(&submission.id);

    let inner_submission = submission.clone();
    let inner_status_tx = status_tx.clone();
    let result = async {
        if fs::metadata(&submission_root).await.is_ok() {
            bail!(
                "The submission's directory already exists, it may indicate a duplicate \
                 submission id: {}",
                submission_root.display()
            );
        }

        fs::create_dir_all(&submission_root)
            .await
            .context("Error creating the submission directory")?;

        debug!("Resolving the submission");
        let submission = Arc::new(
            resolve::resolve_submission(inner_submission, submission_root.clone())
                .context("Failed to resolve the submission")?,
        );

        let (_abort_tx, abort_rx) = mpsc::channel(1);
        let (progress_tx, progress_rx) = ring_channel::ring_channel(NonZeroUsize::new(1).unwrap());
        tokio::spawn({
            let span = Span::current();
            let submission = submission.clone();
            handle_progress_report(span, submission, abort_rx, progress_rx, inner_status_tx)
        });

        debug!("Executing the submission");
        let uploads = execute::execute_submission(submission.clone(), worker_queue_tx, progress_tx)
            .await
            .context("Error executing the submission")?;

        let status = serde_json::to_value(&submission.config)
            .context("Error serializing the submission report")?;
        Ok((status, uploads))
    }
    .await;

    let (ext, uploads) = match result {
        Err(err) => {
            error!("Error handling the submission: {err:#}");
            (SubmissionSignalExt::Error(SubmissionErrorSignal { error: format!("{err:#}") }), None)
        }
        Ok((status, mut uploads)) => {
            let result = match &submission.reporter {
                None => None,
                Some(reporter) => {
                    Some(execute_reporter(&submission_root, reporter, status.clone()).await)
                }
            };

            let (report, report_error) = match result {
                None => (None, None),
                Some(Ok((report, reporter_uploads))) => {
                    uploads.extend(reporter_uploads);
                    (Some(report), None)
                }
                Some(Err(err)) => {
                    error!("The reporter failed: {err:#}");
                    (None, Some(format!("{err:#}")))
                }
            };

            (
                SubmissionSignalExt::Completed(SubmissionReportSignal {
                    report_at: Utc::now(),
                    status,
                    report,
                    report_error,
                }),
                Some(uploads),
            )
        }
    };
    let signal_type = ext.get_type();

    debug!("Sending the final submission signal");
    _ = status_tx.send(SubmissionSignal { id: Some(submission.id.clone()), ext });

    if let Some(uploads) = uploads {
        debug!("Handling file uploads");
        if let Err(err) = apply_uploads_config(&submission_root, &uploads).await {
            error!("Error uploading files: {err:#}");
        }
    }

    _ = fs::remove_dir_all(submission_root).await;

    signal_type
}

#[instrument(skip_all, parent = parent_span)]
async fn handle_progress_report(
    parent_span: Span,
    submission: Arc<Submission>,
    mut abort_rx: mpsc::Receiver<()>,
    mut progress_rx: RingReceiver<()>,
    mut status_tx: RingSender<SubmissionSignal>,
) {
    loop {
        tokio::select! {
            _ = abort_rx.recv() => break,
            item = progress_rx.next() => match item {
                None => break,
                Some(_) => {
                    let result = async {
                        let signal = {
                            let report_at = Utc::now();
                            let status = serde_json::to_value(&submission.config).context("Error serializing the submission report")?;
                            SubmissionSignal {
                                id: Some(submission.id.clone()),
                                ext: SubmissionSignalExt::Progress(match &submission.config.reporter {
                                    None => SubmissionReportSignal {
                                        report_at,
                                        report: None,
                                        report_error: None,
                                        status
                                    },
                                    Some(reporter) => match execute_reporter(&submission.root_directory, reporter, status.clone()).await {
                                        Ok((report, _)) => SubmissionReportSignal {
                                            report_at,
                                            report: Some(report),
                                            report_error: None,
                                            status
                                        },
                                        Err(err) => SubmissionReportSignal {
                                            report_at,
                                            report: None,
                                            report_error: Some(format!("{err:#}")),
                                            status,
                                        },
                                    }
                                })
                            }
                        };

                        if matches!(abort_rx.try_recv(), Err(TryRecvError::Empty)) {
                            _ = status_tx.send(signal);
                        }

                        anyhow::Ok(())
                    }
                    .await;

                    if let Err(err) = result {
                        error!("Error handling the progress report: {err:#}");
                    }
                }
            }
        }
    }
}
