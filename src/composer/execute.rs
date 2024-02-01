use std::{iter, path::PathBuf, sync::Arc};

use anyhow::{bail, Context, Result};
use async_recursion::async_recursion;
use either::Either;
use futures_util::future;
use ring_channel::RingSender;
use tokio::{
    sync::{oneshot, Mutex},
    time::Instant,
};
use tracing::{debug, instrument, Span};

use super::predicate;
use crate::{
    composer::report::apply_embeds_config,
    entities::{
        ActionTaskConfig, ParallelFailedReport, ParallelSuccessReport, ParallelTaskConfig,
        SequenceFailedReport, SequenceSuccessReport, Submission, SubmissionReportUploadConfig,
        TaskConfig, TaskConfigExt, TaskEmbeds, TaskFailedReport, TaskNode, TaskNodeExt,
        TaskReportWhenConfig, TaskStatus, TaskSuccessReport,
    },
    worker::{WorkerQueueItem, WorkerQueueTx},
};

macro_rules! join_errors {
    ($errors:expr) => {
        $errors
            .into_iter()
            .filter_map(|item| item.err())
            .map(|err| format!("{err:#}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
}

#[derive(Debug)]
struct ExecutionContext {
    submission_id: String,
    submission_root: PathBuf,
    worker_queue_tx: WorkerQueueTx,
    progress_tx: Mutex<RingSender<()>>,
    upload_configs: Mutex<Vec<SubmissionReportUploadConfig>>,
}

#[instrument(skip_all)]
pub async fn execute_submission(
    submission: Arc<Submission>,
    worker_queue_tx: WorkerQueueTx,
    progress_tx: RingSender<()>,
) -> Result<Vec<SubmissionReportUploadConfig>> {
    let ctx = ExecutionContext {
        submission_id: submission.id.clone(),
        submission_root: submission.root_directory.clone(),
        worker_queue_tx,
        progress_tx: Mutex::new(progress_tx),
        upload_configs: Mutex::default(),
    };

    let results = future::join_all(
        submission.root_node.tasks.iter().cloned().map(|task| track_task_execution(&ctx, task)),
    )
    .await;

    let errors = join_errors!(results);
    if !errors.is_empty() {
        bail!("Execution got following internal error(s):\n{errors}");
    }

    Ok(ctx.upload_configs.into_inner())
}

#[async_recursion]
async fn track_task_execution(ctx: &ExecutionContext, node: Arc<TaskNode>) -> Result<()> {
    {
        *node.config.status.write().unwrap() = TaskStatus::Running;
    }

    let status = match &node.ext {
        TaskNodeExt::Action(config) => {
            track_action_execution(ctx, node.clone(), config.clone()).await?
        }
        TaskNodeExt::Schedule(tasks) => track_schedule_execution(ctx, node.clone(), tasks).await?,
    };

    if let Some(report) = &node.config.report {
        let success = matches!(status, TaskStatus::Success { .. });

        *node.config.embeds.write().unwrap() = {
            let embeds = report
                .embeds
                .iter()
                .filter(|config| match config.when {
                    TaskReportWhenConfig::Success => success,
                    TaskReportWhenConfig::Failure => !success,
                    TaskReportWhenConfig::Always => true,
                })
                .map(|config| config.inner.clone())
                .collect::<Vec<_>>();
            match apply_embeds_config(&ctx.submission_root, &embeds).await {
                Err(err) => TaskEmbeds::Error(format!("Error applying embeds config: {err:#}")),
                Ok(embeds) => TaskEmbeds::Values(embeds),
            }
        };

        ctx.upload_configs.lock().await.extend(
            report
                .uploads
                .iter()
                .filter(|config| match config.when {
                    TaskReportWhenConfig::Success => success,
                    TaskReportWhenConfig::Failure => !success,
                    TaskReportWhenConfig::Always => true,
                })
                .map(|config| config.inner.clone()),
        );
    }

    {
        *node.config.status.write().unwrap() = status;
    }

    if node.config.progress {
        _ = ctx.progress_tx.lock().await.send(());
    }

    let (continue_nodes, skipped_nodes): (Vec<_>, Vec<_>) = node
        .children
        .iter()
        .partition(|child_node| predicate::check_node_predicate(&node, child_node));

    for node in skipped_nodes {
        skip_task_node(node);
    }

    let results = future::join_all(
        continue_nodes.into_iter().map(|node| track_task_execution(ctx, node.clone())),
    )
    .await;
    let errors = join_errors!(results);
    if !errors.is_empty() {
        bail!("{errors}");
    }

    Ok(())
}

#[instrument(skip_all, fields(task.name = node.name))]
async fn track_action_execution(
    ctx: &ExecutionContext,
    node: Arc<TaskNode>,
    config: Arc<ActionTaskConfig>,
) -> Result<TaskStatus> {
    debug!("Submitting the action");
    let (tx, rx) = oneshot::channel();
    ctx.worker_queue_tx
        .send(WorkerQueueItem {
            parent_span: Span::current(),
            submission_root: ctx.submission_root.clone(),
            submission_id: ctx.submission_id.clone(),
            config,
            report_tx: tx,
        })
        .await
        .context("Failed to send the item")?;

    let report = rx.await.context("Failed to receive the report")??;
    Ok(report.into())
}

#[instrument(skip_all, fields(task.name = node.name))]
async fn track_schedule_execution(
    ctx: &ExecutionContext,
    node: Arc<TaskNode>,
    tasks: &[Arc<TaskNode>],
) -> Result<TaskStatus> {
    let begin = Instant::now();
    let results =
        future::join_all(tasks.iter().cloned().map(|task| track_task_execution(ctx, task))).await;
    let time_elapsed_ms = {
        let end = Instant::now();
        end.duration_since(begin).as_millis().try_into().unwrap()
    };

    let errors = join_errors!(results);
    if !errors.is_empty() {
        bail!("{errors}");
    }

    Ok(match &node.config.ext {
        TaskConfigExt::Action(_) => unreachable!(),
        TaskConfigExt::Parallel(config) => resolve_parallel_status(time_elapsed_ms, config),
        TaskConfigExt::Sequence(config) => resolve_sequence_status(
            time_elapsed_ms,
            config.tasks.iter().map(|(key, value)| (key.to_string(), value.to_owned())),
        ),
    })
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

#[cfg(test)]
mod tests {
    use std::{fs, num::NonZeroUsize, sync::Arc};

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
                let submission = Arc::new(
                    resolve_submission(
                        serde_yaml::from_str(&fs::read_to_string(path).unwrap()).unwrap(),
                        "test".into(),
                    )
                    .expect("Error resolving the submission"),
                );

                let (worker_tx, mut worker_rx) = mpsc::channel(114);
                let (progress_tx, _progress_rx) =
                    ring_channel::ring_channel(NonZeroUsize::new(1).unwrap());
                let handle = tokio::spawn(async move {
                    super::execute_submission(submission, worker_tx, progress_tx).await.unwrap();
                });

                let mut results = vec![];
                while let Some(WorkerQueueItem { config, report_tx, .. }) = worker_rx.recv().await {
                    results.push(config);

                    report_tx
                        .send(Ok(ActionReport::Success(ActionSuccessReport {
                            run_at: Utc::now(),
                            time_elapsed_ms: 0,
                            ext: ActionSuccessReportExt::Noop(action::noop::ExecutionReport {
                                test: 0,
                            }),
                        })))
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
