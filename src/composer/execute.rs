use std::{iter, path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use async_recursion::async_recursion;
use either::Either;
use futures_util::future;
use ring_channel::RingSender;
use serde_json::Value;
use tokio::{
    sync::{mpsc, oneshot},
    time::Instant,
};
use tracing::{debug, error, instrument, Span};

use super::{predicate, report::execute_reporter, SubmissionSignal};
use crate::{
    composer::{report::apply_embeds_config, SubmissionSignalExt},
    entities::{
        ActionFailedReport, ActionFailedReportExt, ActionReport, ActionTaskConfig,
        ParallelFailedReport, ParallelSuccessReport, ParallelTaskConfig, SequenceFailedReport,
        SequenceSuccessReport, Submission, TaskConfig, TaskConfigExt, TaskEmbeds, TaskFailedReport,
        TaskNode, TaskNodeExt, TaskReportEmbedWhenConfig, TaskStatus, TaskSuccessReport,
    },
    worker::{WorkerQueueItem, WorkerQueueTx},
};

#[derive(Debug)]
struct ExecutionContext {
    submission_id: String,
    submission_root: PathBuf,
    worker_queue_tx: WorkerQueueTx,
    progress_tx: mpsc::Sender<()>,
}

#[instrument(skip_all)]
pub async fn execute_submission(
    submission: Submission,
    worker_queue_tx: WorkerQueueTx,
    status_tx: RingSender<SubmissionSignal>,
) -> Result<(Value, Option<Result<Value>>)> {
    let submission = Arc::new(submission);

    let (abort_tx, abort_rx) = mpsc::channel(1);
    let (progress_tx, progress_rx) = mpsc::channel(8);
    tokio::spawn({
        let span = Span::current();
        let submission = submission.clone();
        handle_progress_report(span, submission, abort_rx, progress_rx, status_tx)
    });

    let ctx = ExecutionContext {
        submission_id: submission.id.clone(),
        submission_root: submission.root_directory.clone(),
        worker_queue_tx,
        progress_tx,
    };

    future::join_all(
        submission.root_node.tasks.iter().cloned().map(|task| track_task_execution(&ctx, task)),
    )
    .await;

    _ = abort_tx.send(());

    let status = serde_json::to_value(&submission.config)
        .context("Error serializing the submission report")?;
    let report = match &submission.config.reporter {
        None => None,
        Some(reporter) => Some(execute_reporter(&submission, reporter, status.clone()).await),
    };
    Ok((status, report))
}

#[instrument(skip_all, parent = parent_span)]
async fn handle_progress_report(
    parent_span: Span,
    submission: Arc<Submission>,
    mut abort_rx: mpsc::Receiver<()>,
    mut progress_rx: mpsc::Receiver<()>,
    status_tx: RingSender<SubmissionSignal>,
) {
    loop {
        tokio::select! {
            _ = abort_rx.recv() => break,
            item = progress_rx.recv() => match item {
                None => break,
                Some(_) => {
                    let Some(reporter) = &submission.config.reporter else {
                        continue;
                    };

                    let result = async {
                        let status = serde_json::to_value(&submission.config).context("Error serializing the submission report")?;
                        let result = execute_reporter(&submission, reporter, status.clone()).await;
                        _ = status_tx.clone().send(SubmissionSignal {
                            id: Some(submission.id.clone()),
                            ext: match result {
                                Err(err) => SubmissionSignalExt::Progress {
                                    status,
                                    report: None,
                                    report_error: Some(format!("{err:#}")),
                                },
                                Ok(report) => {
                                    SubmissionSignalExt::Progress { status, report: Some(report), report_error: None }
                                }
                            },
                        });
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

#[async_recursion]
async fn track_task_execution(ctx: &ExecutionContext, node: Arc<TaskNode>) {
    let success = match &node.ext {
        TaskNodeExt::Action(config) => {
            track_action_execution(ctx, node.clone(), config.clone()).await
        }
        TaskNodeExt::Schedule(tasks) => track_schedule_execution(ctx, node.clone(), tasks).await,
    };

    if node.config.progress {
        _ = ctx.progress_tx.send(());
    }

    if let Some(report) = &node.config.report {
        let embeds = report
            .embeds
            .iter()
            .filter(|config| match config.when {
                TaskReportEmbedWhenConfig::Success => success,
                TaskReportEmbedWhenConfig::Failure => !success,
                TaskReportEmbedWhenConfig::Always => true,
            })
            .map(|config| config.inner.clone())
            .collect::<Vec<_>>();
        *node.config.embeds.write().unwrap() =
            match apply_embeds_config(&ctx.submission_root, &embeds).await {
                Err(err) => TaskEmbeds::Error(format!("Error applying embeds config: {err:#}")),
                Ok(embeds) => TaskEmbeds::Values(embeds),
            };
    }

    let (continue_nodes, skipped_nodes): (Vec<_>, Vec<_>) = node
        .children
        .iter()
        .partition(|child_node| predicate::check_node_predicate(&node, child_node));

    for node in skipped_nodes {
        skip_task_node(node);
    }

    future::join_all(
        continue_nodes.into_iter().map(|node| track_task_execution(ctx.clone(), node.clone())),
    )
    .await;
}

#[instrument(skip_all, fields(task.name = node.name))]
async fn track_action_execution(
    ctx: &ExecutionContext,
    node: Arc<TaskNode>,
    config: Arc<ActionTaskConfig>,
) -> bool {
    {
        *node.config.status.write().unwrap() = TaskStatus::Running;
    }

    debug!("Submitting the action");
    let result = submit_action(ctx, config.clone()).await;

    let status = match result {
        Err(err) => TaskStatus::Failed {
            report: TaskFailedReport::Action(ActionFailedReport {
                run_at: None,
                time_elapsed_ms: None,
                ext: ActionFailedReportExt::Internal {
                    error: format!("Error submitting the task: {err:#}"),
                },
            }),
        },
        Ok(report) => report.into(),
    };

    let success = matches!(status, TaskStatus::Success { .. });

    {
        *node.config.status.write().unwrap() = status;
    }

    success
}

#[instrument(skip_all, fields(task.name = node.name))]
async fn track_schedule_execution(
    ctx: &ExecutionContext,
    node: Arc<TaskNode>,
    tasks: &[Arc<TaskNode>],
) -> bool {
    {
        *node.config.status.write().unwrap() = TaskStatus::Running;
    }

    let begin = Instant::now();
    future::join_all(tasks.iter().cloned().map(|task| track_task_execution(ctx.clone(), task)))
        .await;
    let time_elapsed_ms = {
        let end = Instant::now();
        end.duration_since(begin).as_millis().try_into().unwrap()
    };

    let status = match &node.config.ext {
        TaskConfigExt::Action(_) => panic!("Unexpected schedule task"),
        TaskConfigExt::Parallel(config) => resolve_parallel_status(time_elapsed_ms, config),
        TaskConfigExt::Sequence(config) => resolve_sequence_status(
            time_elapsed_ms,
            config.tasks.iter().map(|(key, value)| (key.to_string(), value.to_owned())),
        ),
    };
    let success = matches!(status, TaskStatus::Success { .. });

    {
        *node.config.status.write().unwrap() = status;
    }

    success
}

fn resolve_parallel_status(time_elapsed_ms: u64, config: &ParallelTaskConfig) -> TaskStatus {
    let mut status = TaskStatus::Success {
        report: TaskSuccessReport::Parallel(ParallelSuccessReport { time_elapsed_ms }),
    };

    for task in config.tasks.iter() {
        match *task.status.read().unwrap() {
            TaskStatus::Pending => {
                status = TaskStatus::Pending;
            }
            TaskStatus::Failed { .. } => {
                let failed_indexes = config
                    .tasks
                    .iter()
                    .enumerate()
                    .filter_map(|(index, task)| match *task.status.read().unwrap() {
                        TaskStatus::Failed { .. } => Some(index),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                status = TaskStatus::Failed {
                    report: TaskFailedReport::Parallel(ParallelFailedReport {
                        time_elapsed_ms,
                        failed_count: failed_indexes.len(),
                        failed_indexes,
                    }),
                };
                break;
            }
            TaskStatus::Skipped => {
                panic!("Unexpected child status: Skipped")
            }
            _ => {}
        }
    }
    status
}

fn resolve_sequence_status(
    time_elapsed_ms: u64,
    values: impl Iterator<Item = (String, Arc<TaskConfig>)>,
) -> TaskStatus {
    let mut status = TaskStatus::Success {
        report: TaskSuccessReport::Sequence(SequenceSuccessReport { time_elapsed_ms }),
    };
    for (name, task) in values {
        match *task.status.read().unwrap() {
            TaskStatus::Pending => {
                status = TaskStatus::Pending;
            }
            TaskStatus::Failed { .. } => {
                status = TaskStatus::Failed {
                    report: TaskFailedReport::Sequence(SequenceFailedReport {
                        time_elapsed_ms,
                        failed_at: name,
                    }),
                };
                break;
            }
            TaskStatus::Skipped => {
                panic!("Unexpected child status: Skipped")
            }
            _ => {}
        }
    }
    status
}

fn skip_task_node(node: &TaskNode) {
    {
        *node.config.status.write().unwrap() = TaskStatus::Skipped;
    }

    let nodes = (if let TaskNodeExt::Schedule(tasks) = &node.ext {
        Either::Left(tasks.iter())
    } else {
        Either::Right(iter::empty())
    })
    .chain(node.children.iter());
    for node in nodes {
        {
            *node.config.status.write().unwrap() = TaskStatus::Skipped;
        }
        skip_task_node(node);
    }
}

async fn submit_action(
    ctx: &ExecutionContext,
    config: Arc<ActionTaskConfig>,
) -> Result<ActionReport> {
    let (tx, rx) = oneshot::channel();
    ctx.worker_queue_tx
        .send(WorkerQueueItem {
            parent_span: Span::current(),
            submission_root: ctx.submission_root.clone(),
            submission_id: ctx.submission_id.clone(),
            config,
            report_tx: tx,
        })
        .await?;

    // TODO: timeout
    Ok(rx.await?)
}

#[cfg(test)]
mod tests {
    use std::{fs, num::NonZeroUsize};

    use chrono::Utc;
    use insta::glob;
    use tokio::{runtime::Builder, sync::mpsc};

    use crate::{
        composer::resolve::resolve_submission,
        entities::{ActionReport, ActionSuccessReport, ActionSuccessReportExt},
        worker::{action, WorkerQueueItem},
    };

    #[test]
    fn test_execute_submission() {
        glob!("tests/*.yaml", |path| {
            Builder::new_multi_thread().enable_all().build().unwrap().block_on(async {
                let submission = resolve_submission(
                    serde_yaml::from_str(&fs::read_to_string(path).unwrap()).unwrap(),
                    "test".into(),
                )
                .expect("Error resolving the submission");

                let (worker_tx, mut worker_rx) = mpsc::channel(114);
                let (tx, _rx) = ring_channel::ring_channel(NonZeroUsize::try_from(1).unwrap());
                let handle = tokio::spawn(async move {
                    super::execute_submission(submission, worker_tx, tx).await.unwrap();
                });

                let mut results = vec![];
                while let Some(WorkerQueueItem { config, report_tx, .. }) = worker_rx.recv().await {
                    results.push(config);

                    report_tx
                        .send(ActionReport::Success(ActionSuccessReport {
                            run_at: Utc::now(),
                            time_elapsed_ms: 0,
                            ext: ActionSuccessReportExt::Noop(action::noop::ExecutionReport {
                                test: 0,
                            }),
                        }))
                        .unwrap();
                }

                handle.await.unwrap();

                insta::with_settings!({snapshot_path => "tests/snapshots"}, {
                    insta::assert_ron_snapshot!(results);
                });
            });
        })
    }
}
