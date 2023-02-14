use std::{iter, path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use async_recursion::async_recursion;
use either::Either;
use ring_channel::RingSender;
use serde_json::Value;
use tokio::{
    sync::{oneshot, Mutex},
    time::Instant,
};
use tracing::{debug, error, info_span, instrument, Instrument, Span};

use super::{
    predicate,
    report::{apply_report_config, make_submission_report},
    SubmissionProgressSignal, SubmissionSignal, SubmissionSignalExt,
};
use crate::{
    entities::{
        ActionReport, ActionTaskConfig, ParallelFailedReport, ParallelSuccessReport,
        SequenceFailedReport, SequenceSuccessReport, Submission, TaskConfig, TaskConfigExt,
        TaskFailedReport, TaskNode, TaskNodeExt, TaskStatus, TaskSuccessReport,
    },
    worker::{WorkerQueueItem, WorkerQueueTx},
};

#[derive(Debug)]
struct ExecutionContext {
    submission_id: String,
    submission_root: PathBuf,
    status_tx: Mutex<RingSender<SubmissionSignal>>,
    worker_queue_tx: WorkerQueueTx,
}

#[instrument(skip_all)]
pub async fn execute_submission(
    submission: Submission,
    worker_queue_tx: WorkerQueueTx,
    status_tx: RingSender<SubmissionSignal>,
) -> Result<(bool, Value, Option<Result<Value>>)> {
    let ctx = ExecutionContext {
        submission_id: submission.id.clone(),
        submission_root: submission.root_directory.clone(),
        status_tx: Mutex::new(status_tx),
        worker_queue_tx,
    };

    let results = futures_util::future::join_all(
        submission.root_node.tasks.iter().cloned().map(|task| track_task_execution(&ctx, task)),
    )
    .await;

    let success = results.into_iter().all(|success| success);
    let status = serde_json::to_value(&submission.config)
        .context("Error serializing the submission report")?;
    let report = match (&submission.config.reporter, success) {
        (None, _) | (Some(_), false) => None,
        (Some(reporter), true) => Some({
            let span = info_span!(parent: Span::current(), "handle_reporter");
            async {
                let mut report_config =
                    make_submission_report(submission.config.clone(), reporter).await?;

                let result = apply_report_config(&report_config, &submission).await?;

                for (field, content) in result.embeds {
                    report_config.report.insert(field, content.into());
                }

                let report = serde_json::to_value(report_config.report)
                    .context("Error serializing report returned by the reporter")?;
                anyhow::Ok(report)
            }
            .instrument(span)
            .await
            .context("Error executing the reporter")
        }),
    };
    Ok((success, status, report))
}

#[async_recursion]
async fn track_task_execution(ctx: &ExecutionContext, node: Arc<TaskNode>) -> bool {
    let success = match &node.ext {
        TaskNodeExt::Action(config) => {
            track_action_execution(ctx, node.clone(), config.clone()).await
        }
        TaskNodeExt::Schedule(tasks) => track_schedule_execution(ctx, node.clone(), tasks).await,
    };

    let (continue_nodes, skipped_nodes): (Vec<_>, Vec<_>) = node
        .children
        .iter()
        .partition(|child_node| predicate::check_node_predicate(&node, child_node));

    for node in skipped_nodes {
        skip_task_node(node);
    }

    let results = futures_util::future::join_all(
        continue_nodes.into_iter().map(|node| track_task_execution(ctx.clone(), node.clone())),
    )
    .await;

    return success && results.into_iter().all(|success| success);
}

#[instrument(skip_all, fields(task.name = node.name))]
async fn track_action_execution(
    ctx: &ExecutionContext,
    node: Arc<TaskNode>,
    config: Arc<ActionTaskConfig>,
) -> bool {
    debug!("Submitting the action");
    let result = submit_action(ctx, config.clone()).await;

    let status = match result {
        Err(err) => TaskStatus::Failed {
            report: TaskFailedReport::Action(format!("Error submitting the task: {err:#}").into()),
        },
        Ok(report) => report.into(),
    };

    if let TaskStatus::Failed { report } = &status {
        error!("The execution of the task returned a failed report: {:?}", report);
    }

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
    let begin = Instant::now();
    futures_util::future::join_all(
        tasks.iter().cloned().map(|task| track_task_execution(ctx.clone(), task)),
    )
    .await;
    let time_elapsed_ms = {
        let end = Instant::now();
        end.duration_since(begin).as_millis().try_into().unwrap()
    };

    let status = match &node.config.ext {
        TaskConfigExt::Action(_) => panic!("Unexpected schedule task"),
        TaskConfigExt::Parallel(config) => {
            resolve_parallel_status(time_elapsed_ms, config.tasks.iter().cloned().collect())
        }
        TaskConfigExt::Sequence(config) => resolve_sequence_status(
            time_elapsed_ms,
            config.tasks.iter().map(|(key, value)| (key.to_string(), value.to_owned())),
        ),
    };
    let success = matches!(status, TaskStatus::Success { .. });

    {
        *node.config.status.write().unwrap() = status;
    }

    match serde_json::to_value(&node.config) {
        Err(err) => {
            error!("Error serializing the task node config: {err:#}");
        }
        Ok(status) => {
            _ = ctx.status_tx.lock().await.send(SubmissionSignal {
                id: Some(ctx.submission_id.clone()),
                ext: SubmissionSignalExt::Progress(SubmissionProgressSignal {
                    name: node.name.clone(),
                    status,
                }),
            });
        }
    }

    success
}

fn resolve_parallel_status(time_elapsed_ms: u64, tasks: Vec<Arc<TaskConfig>>) -> TaskStatus {
    let mut status = TaskStatus::Success {
        report: TaskSuccessReport::Parallel(ParallelSuccessReport { time_elapsed_ms }),
    };
    for task in tasks.iter() {
        match *task.status.read().unwrap() {
            TaskStatus::Pending => {
                status = TaskStatus::Pending;
            }
            TaskStatus::Failed { .. } => {
                status = TaskStatus::Failed {
                    report: TaskFailedReport::Parallel(ParallelFailedReport {
                        time_elapsed_ms,
                        failed_count: tasks
                            .iter()
                            .filter(|task| {
                                matches!(*task.status.read().unwrap(), TaskStatus::Failed { .. })
                            })
                            .count(),
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

    use crate::{
        composer::resolve::resolve_submission,
        entities::{ActionReport, ActionSuccessReport, ActionSuccessReportExt},
        worker::{action, WorkerQueueItem},
    };

    #[test]
    fn test_execute_submission() {
        glob!("stubs/*.yaml", |path| {
            tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap().block_on(
                async {
                    let submission = resolve_submission(
                        serde_yaml::from_str(&fs::read_to_string(path).unwrap()).unwrap(),
                        "test".into(),
                    )
                    .expect("Error resolving the submission");

                    let (worker_tx, worker_rx) = async_channel::unbounded();
                    let (tx, _rx) = ring_channel::ring_channel(NonZeroUsize::try_from(1).unwrap());
                    let handle = tokio::spawn(async move {
                        super::execute_submission(submission, worker_tx, tx).await.unwrap();
                    });

                    let mut results = vec![];
                    while let Ok(WorkerQueueItem { config, report_tx, .. }) = worker_rx.recv().await
                    {
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

                    insta::assert_ron_snapshot!(results);
                },
            );
        })
    }
}
