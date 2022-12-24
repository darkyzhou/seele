use super::predicate;
use crate::{
    entity::{
        ActionTaskConfig, Submission, TaskConfig, TaskExtraConfig, TaskFailedReport, TaskNode,
        TaskNodeExtra, TaskReport, TaskStatus, TaskSuccessReport,
    },
    worker::WorkerQueueItem,
};
use async_recursion::async_recursion;
use std::{sync::Arc, time::SystemTime};
use tokio::sync::oneshot;

pub async fn execute_submission(
    worker_queue_tx: async_channel::Sender<WorkerQueueItem>,
    submission: Submission,
) -> anyhow::Result<Submission> {
    futures_util::future::join_all(
        submission
            .root
            .tasks
            .iter()
            .cloned()
            .map(|task| track_task_execution(worker_queue_tx.clone(), task)),
    )
    .await;

    Ok(submission)
}

#[async_recursion]
async fn track_task_execution(
    worker_queue_tx: async_channel::Sender<WorkerQueueItem>,
    node: Arc<TaskNode>,
) {
    match &node.extra {
        TaskNodeExtra::Action(config) => {
            track_action_execution(worker_queue_tx.clone(), node.clone(), config.clone()).await
        }
        TaskNodeExtra::Schedule(tasks) => {
            track_schedule_execution(worker_queue_tx.clone(), node.clone(), tasks).await
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
        continue_nodes
            .into_iter()
            .map(|node| track_task_execution(worker_queue_tx.clone(), node.clone())),
    )
    .await;
}

#[async_recursion]
async fn track_action_execution(
    worker_queue_tx: async_channel::Sender<WorkerQueueItem>,
    node: Arc<TaskNode>,
    config: Arc<ActionTaskConfig>,
) {
    let enqueued_at = SystemTime::now();
    let result = submit_task(worker_queue_tx.clone(), config.clone()).await;

    *node.config.status.write().unwrap() = match result {
        Err(err) => TaskStatus::Failed(TaskFailedReport::Action {
            enqueued_at,
            run_at: None,
            time_elapsed_ms: None,
            message: format!("Error submitting the task: {:#?}", err),
        }),
        Ok(report) => match report {
            TaskReport::Success(report) => TaskStatus::Success(report),
            TaskReport::Failed(report) => TaskStatus::Failed(report),
        },
    };
}

async fn track_schedule_execution(
    worker_queue_tx: async_channel::Sender<WorkerQueueItem>,
    node: Arc<TaskNode>,
    tasks: &[Arc<TaskNode>],
) {
    futures_util::future::join_all(
        tasks.iter().cloned().map(|task| track_task_execution(worker_queue_tx.clone(), task)),
    )
    .await;

    *node.config.status.write().unwrap() = match &node.config.extra {
        TaskExtraConfig::Action(_) => panic!("Unexpected schedule task"),
        TaskExtraConfig::Parallel(config) => resolve_parent_status(config.tasks.iter().cloned()),
        TaskExtraConfig::Sequence(config) => resolve_parent_status(config.tasks.values().cloned()),
    };
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
    worker_queue_tx: async_channel::Sender<WorkerQueueItem>,
    task: Arc<ActionTaskConfig>,
) -> anyhow::Result<TaskReport> {
    let (tx, rx) = oneshot::channel();
    worker_queue_tx.send((task, tx)).await?;

    // TODO: timeout
    Ok(rx.await?)
}

#[cfg(test)]
mod tests {
    use crate::{
        composer::resolve::resolve_submission,
        entity::{TaskReport, TaskSuccessReport, TaskSuccessReportExtra},
    };
    use insta::glob;
    use std::{fs, time::SystemTime};

    #[test]
    fn test_execute_submission() {
        glob!("stubs/*.yaml", |path| {
            tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap().block_on(
                async {
                    let submission = resolve_submission(
                        serde_yaml::from_str(&fs::read_to_string(path).unwrap()).unwrap(),
                    )
                    .expect("Error resolving the submission");

                    let (tx, rx) = async_channel::unbounded();
                    let handle = tokio::spawn(async move {
                        super::execute_submission(tx, submission).await.unwrap();
                    });

                    let mut results = vec![];
                    while let Ok((config, tx)) = rx.recv().await {
                        results.push((config,));

                        tx.send(TaskReport::Success(TaskSuccessReport::Action {
                            enqueued_at: SystemTime::now(),
                            run_at: SystemTime::now(),
                            time_elapsed_ms: 0,
                            extra: TaskSuccessReportExtra::Noop,
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
