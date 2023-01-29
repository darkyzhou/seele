use super::{predicate, report::make_submission_report};
use crate::{
    entities::{
        ActionTaskConfig, Submission, TaskConfig, TaskConfigExt, TaskFailedReport, TaskNode,
        TaskNodeExt, TaskReport, TaskStatus, TaskSuccessReport,
    },
    worker::{WorkerQueueItem, WorkerQueueTx},
};
use async_recursion::async_recursion;
use futures_util::{stream, StreamExt};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::oneshot;
use tracing::{debug, instrument};

#[derive(Debug, Clone)]
struct ExecutionContext {
    submission_root: PathBuf,
    submission_id: String,
    worker_queue_tx: WorkerQueueTx,
    status_tx: ring_channel::RingSender<()>,
}

#[instrument(skip_all, fields(id = submission.id))]
pub async fn execute_submission(
    submission_root: PathBuf,
    submission: Submission,
    worker_queue_tx: WorkerQueueTx,
    status_tx: ring_channel::RingSender<()>,
) -> anyhow::Result<()> {
    let ctx = ExecutionContext {
        submission_root,
        submission_id: submission.id,
        worker_queue_tx,
        status_tx,
    };

    futures_util::future::join_all(
        submission.root.tasks.iter().cloned().map(|task| track_task_execution(ctx.clone(), task)),
    )
    .await;

    let report_config =
        make_submission_report(submission.config.clone(), &submission.config.reporter).await?;
    *submission.config.report.write().unwrap() = Some(report_config.report);

    let _ = stream::once(async { Ok(()) }).forward(ctx.status_tx).await;

    Ok(())
}

#[async_recursion]
async fn track_task_execution(ctx: ExecutionContext, node: Arc<TaskNode>) {
    match &node.ext {
        TaskNodeExt::Action(config) => {
            track_action_execution(ctx.clone(), node.clone(), config.clone()).await
        }
        TaskNodeExt::Schedule(tasks) => {
            track_schedule_execution(ctx.clone(), node.clone(), tasks).await
        }
    }

    let (continue_nodes, skipped_nodes): (Vec<_>, Vec<_>) = node
        .children
        .iter()
        .partition(|child_node| predicate::check_node_predicate(&node, child_node));

    for node in skipped_nodes {
        mark_children_as_skipped(node);
    }

    futures_util::future::join_all(
        continue_nodes.into_iter().map(|node| track_task_execution(ctx.clone(), node.clone())),
    )
    .await;
}

#[instrument(skip(ctx, node))]
async fn track_action_execution(
    ctx: ExecutionContext,
    node: Arc<TaskNode>,
    config: Arc<ActionTaskConfig>,
) {
    debug!("Submitting the action");
    let result = submit_task(ctx.clone(), config.clone()).await;

    let status = match result {
        Err(err) => TaskStatus::Failed(TaskFailedReport::Action {
            run_at: None,
            time_elapsed_ms: None,
            message: format!("Error submitting the task: {err:#}"),
        }),
        Ok(report) => match report {
            TaskReport::Success(report) => TaskStatus::Success(report),
            TaskReport::Failed(report) => TaskStatus::Failed(report),
        },
    };
    debug!(status = ?status, "Setting the status");
    *node.config.status.write().unwrap() = status;

    let _ = stream::once(async { Ok(()) }).forward(ctx.status_tx).await;
}

#[instrument(skip(ctx, node))]
async fn track_schedule_execution(
    ctx: ExecutionContext,
    node: Arc<TaskNode>,
    tasks: &[Arc<TaskNode>],
) {
    futures_util::future::join_all(
        tasks.iter().cloned().map(|task| track_task_execution(ctx.clone(), task)),
    )
    .await;

    let status = match &node.config.ext {
        TaskConfigExt::Action(_) => panic!("Unexpected schedule task"),
        TaskConfigExt::Parallel(config) => resolve_parent_status(config.tasks.iter().cloned()),
        TaskConfigExt::Sequence(config) => resolve_parent_status(config.tasks.values().cloned()),
    };
    debug!(status = ?status, "Setting the status");
    *node.config.status.write().unwrap() = status;

    let _ = stream::once(async { Ok(()) }).forward(ctx.status_tx).await;
}

fn resolve_parent_status(tasks: impl Iterator<Item = Arc<TaskConfig>>) -> TaskStatus {
    let mut status = TaskStatus::Success(TaskSuccessReport::Schedule);
    for task in tasks {
        match *task.status.read().unwrap() {
            TaskStatus::Pending => {
                status = TaskStatus::Pending;
            }
            TaskStatus::Failed(_) => {
                status = TaskStatus::Failed(TaskFailedReport::Schedule);
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

fn mark_children_as_skipped(task: &TaskNode) {
    for node in &task.children {
        *node.config.status.write().unwrap() = TaskStatus::Skipped;
        mark_children_as_skipped(node);
    }
}

async fn submit_task(
    ctx: ExecutionContext,
    config: Arc<ActionTaskConfig>,
) -> anyhow::Result<TaskReport> {
    let (tx, rx) = oneshot::channel();
    ctx.worker_queue_tx
        .send(WorkerQueueItem {
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
    use crate::{
        composer::resolve::resolve_submission,
        entities::{ActionExecutionReport, TaskReport, TaskSuccessReport},
        worker::{NoopExecutionReport, WorkerQueueItem},
    };
    use chrono::Utc;
    use insta::glob;
    use std::{fs, num::NonZeroUsize};

    #[test]
    fn test_execute_submission() {
        glob!("stubs/*.yaml", |path| {
            tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap().block_on(
                async {
                    let submission = resolve_submission(
                        serde_yaml::from_str(&fs::read_to_string(path).unwrap()).unwrap(),
                    )
                    .expect("Error resolving the submission");

                    let (worker_tx, worker_rx) = async_channel::unbounded();
                    let (tx, _rx) = ring_channel::ring_channel(NonZeroUsize::try_from(1).unwrap());
                    let handle = tokio::spawn(async move {
                        super::execute_submission("test".into(), submission, worker_tx, tx)
                            .await
                            .unwrap();
                    });

                    let mut results = vec![];
                    while let Ok(WorkerQueueItem { config, report_tx, .. }) = worker_rx.recv().await
                    {
                        results.push(config);

                        report_tx
                            .send(TaskReport::Success(TaskSuccessReport::Action {
                                run_at: Utc::now(),
                                time_elapsed_ms: 0,
                                report: ActionExecutionReport::Noop(NoopExecutionReport {
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
