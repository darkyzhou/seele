use std::{path::PathBuf, sync::Arc};

use anyhow::Result;
use async_recursion::async_recursion;
use tokio::{sync::oneshot, time::Instant};
use tracing::{debug, error, instrument, Span};

use super::{
    predicate,
    report::{apply_report_config, make_submission_report},
    SubmissionUpdateSignal,
};
use crate::{
    entities::{
        ActionReport, ActionTaskConfig, ParallelFailedReport, ParallelSuccessReport,
        SequenceFailedReport, SequenceSuccessReport, Submission, TaskConfig, TaskConfigExt,
        TaskFailedReport, TaskNode, TaskNodeExt, TaskStatus, TaskSuccessReport,
    },
    worker::{WorkerQueueItem, WorkerQueueTx},
};

#[derive(Debug, Clone)]
struct ExecutionContext {
    submission_id: String,
    submission_root: PathBuf,
    worker_queue_tx: WorkerQueueTx,
    status_tx: ring_channel::RingSender<SubmissionUpdateSignal>,
}

#[instrument(skip_all)]
pub async fn execute_submission(
    submission: Submission,
    worker_queue_tx: WorkerQueueTx,
    status_tx: ring_channel::RingSender<SubmissionUpdateSignal>,
) -> Result<bool> {
    let mut ctx = ExecutionContext {
        submission_id: submission.id.clone(),
        submission_root: submission.root_directory.clone(),
        worker_queue_tx,
        status_tx,
    };

    let results = futures_util::future::join_all(
        submission
            .root_node
            .tasks
            .iter()
            .cloned()
            .map(|task| track_task_execution(ctx.clone(), task)),
    )
    .await;
    let success = results.into_iter().all(|success| success);

    if success {
        let report_result = async {
            if let Some(reporter) = &submission.config.reporter {
                let mut report_config =
                    make_submission_report(submission.config.clone(), reporter).await?;

                let result = apply_report_config(&report_config, &submission).await?;

                for (field, content) in result.embeds {
                    report_config.report.insert(field, content.into());
                }

                *submission.config.report.lock().unwrap() = Some(report_config.report);
            }

            anyhow::Ok(())
        }
        .await;

        if let Err(err) = &report_result {
            *submission.config.report_error.lock().unwrap() = Some(format!("{err:#}"));
        }

        _ = ctx.status_tx.send(SubmissionUpdateSignal::Finished);

        report_result.map(move |_| success)
    } else {
        _ = ctx.status_tx.send(SubmissionUpdateSignal::Finished);

        Ok(success)
    }
}

#[async_recursion]
async fn track_task_execution(ctx: ExecutionContext, node: Arc<TaskNode>) -> bool {
    let success = match &node.ext {
        TaskNodeExt::Action(config) => {
            track_action_execution(ctx.clone(), node.clone(), config.clone()).await
        }
        TaskNodeExt::Schedule(tasks) => {
            track_schedule_execution(ctx.clone(), node.clone(), tasks).await
        }
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
    mut ctx: ExecutionContext,
    node: Arc<TaskNode>,
    config: Arc<ActionTaskConfig>,
) -> bool {
    debug!("Submitting the action");
    let result = submit_action(ctx.clone(), config.clone()).await;

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

    _ = ctx.status_tx.send(SubmissionUpdateSignal::Progress);

    success
}

#[instrument(skip_all, fields(task.name = node.name))]
async fn track_schedule_execution(
    mut ctx: ExecutionContext,
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

    debug!(status = ?status, "Setting the status");
    {
        *node.config.status.write().unwrap() = status;
    }

    _ = ctx.status_tx.send(SubmissionUpdateSignal::Progress);

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
    *node.config.status.write().unwrap() = TaskStatus::Skipped;

    for node in &node.children {
        {
            *node.config.status.write().unwrap() = TaskStatus::Skipped;
        }
        skip_task_node(node);
    }
}

async fn submit_action(
    ctx: ExecutionContext,
    config: Arc<ActionTaskConfig>,
) -> Result<ActionReport> {
    let (tx, rx) = oneshot::channel();
    ctx.worker_queue_tx
        .send(WorkerQueueItem {
            parent_span: Span::current(),
            submission_root: ctx.submission_root,
            submission_id: ctx.submission_id,
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
