use super::{predicate, SubmissionProgressTx};
use crate::{
    entity::{
        ActionTaskConfig, Submission, TaskConfig, TaskExtraConfig, TaskFailedReport, TaskNode,
        TaskNodeExtra, TaskReport, TaskStatus, TaskSuccessReport,
    },
    worker::{WorkerQueueItem, WorkerQueueTx},
};
use async_recursion::async_recursion;
use std::sync::Arc;
use tokio::sync::oneshot;

#[derive(Debug, Clone)]
struct ExecutionContext {
    submission_id: String,
    worker_queue_tx: WorkerQueueTx,
    submission_progress_tx: SubmissionProgressTx,
}

pub async fn execute_submission(
    submission: Submission,
    worker_queue_tx: WorkerQueueTx,
    submission_progress_tx: SubmissionProgressTx,
) -> anyhow::Result<()> {
    futures_util::future::join_all(submission.root.tasks.iter().cloned().map(|task| {
        track_task_execution(
            ExecutionContext {
                submission_id: submission.id.clone(),
                worker_queue_tx: worker_queue_tx.clone(),
                submission_progress_tx: submission_progress_tx.clone(),
            },
            task,
        )
    }))
    .await;

    Ok(())
}

#[async_recursion]
async fn track_task_execution(ctx: ExecutionContext, node: Arc<TaskNode>) {
    match &node.extra {
        TaskNodeExtra::Action(config) => {
            track_action_execution(ctx.clone(), node.clone(), config.clone()).await
        }
        TaskNodeExtra::Schedule(tasks) => {
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

#[async_recursion]
async fn track_action_execution(
    mut ctx: ExecutionContext,
    node: Arc<TaskNode>,
    config: Arc<ActionTaskConfig>,
) {
    let result = submit_task(ctx.clone(), config.clone()).await;

    *node.config.status.write().unwrap() = match result {
        Err(err) => TaskStatus::Failed(TaskFailedReport::Action {
            run_at: None,
            time_elapsed_ms: None,
            message: format!("Error submitting the task: {:#?}", err),
        }),
        Ok(report) => match report {
            TaskReport::Success(report) => TaskStatus::Success(report),
            TaskReport::Failed(report) => TaskStatus::Failed(report),
        },
    };

    let _ = ctx.submission_progress_tx.send(());
}

async fn track_schedule_execution(
    mut ctx: ExecutionContext,
    node: Arc<TaskNode>,
    tasks: &[Arc<TaskNode>],
) {
    futures_util::future::join_all(
        tasks.iter().cloned().map(|task| track_task_execution(ctx.clone(), task)),
    )
    .await;

    *node.config.status.write().unwrap() = match &node.config.extra {
        TaskExtraConfig::Action(_) => panic!("Unexpected schedule task"),
        TaskExtraConfig::Parallel(config) => resolve_parent_status(config.tasks.iter().cloned()),
        TaskExtraConfig::Sequence(config) => resolve_parent_status(config.tasks.values().cloned()),
    };

    let _ = ctx.submission_progress_tx.send(());
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
        .send(WorkerQueueItem { submission_id: ctx.submission_id, config, report_tx: tx })
        .await?;

    // TODO: timeout
    Ok(rx.await?)
}

#[cfg(test)]
mod tests {
    use crate::{
        composer::resolve::resolve_submission,
        entity::{TaskReport, TaskSuccessReport, TaskSuccessReportExtra},
        worker::WorkerQueueItem,
    };
    use insta::glob;
    use std::{fs, num::NonZeroUsize, time::SystemTime};

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
                    let (progress_tx, _progress_rx) =
                        ring_channel::ring_channel(NonZeroUsize::new(1).unwrap());
                    let handle = tokio::spawn(async move {
                        super::execute_submission(submission, worker_tx, progress_tx)
                            .await
                            .unwrap();
                    });

                    let mut results = vec![];
                    while let Ok(WorkerQueueItem { config, report_tx, .. }) = worker_rx.recv().await
                    {
                        results.push(config);

                        report_tx
                            .send(TaskReport::Success(TaskSuccessReport::Action {
                                run_at: SystemTime::now(),
                                time_elapsed_ms: 0,
                                extra: TaskSuccessReportExtra::Noop(0),
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
