use super::predicate;
use crate::{
    entity::{
        ActionTaskConfig, Submission, TaskExecutionFailedReport, TaskExecutionReport, TaskNode,
        TaskNodeExtra, TaskReport, TaskStatus,
    },
    worker::WorkerQueueItem,
};
use std::{sync::Arc, time::SystemTime};
use tokio::sync::oneshot;

pub async fn execute_submission(
    worker_queue_tx: async_channel::Sender<WorkerQueueItem>,
    submission: Submission,
) -> anyhow::Result<Submission> {
    let mut queue = flatten_tasks(submission.root.tasks.iter().cloned());
    // TODO: queue is initially empty?
    while !queue.is_empty() {
        let mut next_queue = vec![];

        let enqueued_at = SystemTime::now();
        let reports = futures_util::future::join_all(
            queue.iter().map(|(_, config)| submit_task(worker_queue_tx.clone(), config.clone())),
        )
        .await;

        for (i, report) in reports.into_iter().enumerate() {
            let id = &queue[i].0.id;
            let node = submission.id_to_node_map.get(id).unwrap();

            // TODO: parent

            *node.config.status.write().unwrap() = match report {
                Err(err) => TaskStatus::Failed(TaskReport {
                    enqueued_at,
                    execution: TaskExecutionFailedReport {
                        run_at: None,
                        time_elapsed_ms: None,
                        message: format!("Error submitting the task: {:#?}", err),
                    },
                }),
                Ok(report) => match report {
                    TaskExecutionReport::Success(report) => {
                        TaskStatus::Success(TaskReport { enqueued_at, execution: report })
                    }
                    TaskExecutionReport::Failed(report) => {
                        TaskStatus::Failed(TaskReport { enqueued_at, execution: report })
                    }
                },
            };

            let (continue_nodes, skipped_nodes): (Vec<_>, Vec<_>) = node
                .children
                .iter()
                .partition(|child_node| predicate::check_node_predicate(node, child_node));

            for node in skipped_nodes {
                mark_children_as_skipped(node);
            }

            next_queue.extend(flatten_tasks(continue_nodes.into_iter().cloned()));
        }

        queue = next_queue;
    }

    Ok(submission)
}

fn flatten_tasks(
    tasks: impl Iterator<Item = Arc<TaskNode>>,
) -> Vec<(Arc<TaskNode>, Arc<ActionTaskConfig>)> {
    tasks.fold(vec![], |mut acc, task| match &task.extra {
        TaskNodeExtra::Schedule(tasks) => {
            acc.extend(flatten_tasks(tasks.iter().cloned()));
            acc
        }
        TaskNodeExtra::Action(config) => {
            acc.push((task.clone(), config.clone()));
            acc
        }
    })
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
) -> anyhow::Result<TaskExecutionReport> {
    let (tx, rx) = oneshot::channel();
    worker_queue_tx.send((task, tx)).await?;

    // TODO: timeout
    Ok(rx.await?)
}

#[cfg(test)]
mod tests {
    use crate::{
        composer::resolve::resolve_submission,
        entity::{
            ActionTaskConfig, TaskExecutionReport, TaskExecutionSuccessReport,
            TaskExecutionSuccessReportExtra,
        },
    };
    use std::time::SystemTime;

    #[tokio::test]
    async fn test_execute_submission() {
        let submission = resolve_submission(
            serde_yaml::from_str(include_str!("./test/resolve_submission_1.yaml"))
                .expect("Failed to parse the input"),
        )
        .expect("Failed to resolve the submission");

        let (tx, rx) = async_channel::unbounded();
        let handle = tokio::spawn(async move {
            super::execute_submission(tx, submission).await.unwrap();
        });

        let mut results = vec![];
        while let Ok((config, tx)) = rx.recv().await {
            results.push(config);

            tx.send(TaskExecutionReport::Success(TaskExecutionSuccessReport {
                run_at: SystemTime::now(),
                time_elapsed_ms: 0,
                extra: TaskExecutionSuccessReportExtra::Noop,
            }))
            .unwrap();
        }

        handle.await.unwrap();

        assert_eq!(results.len(), 2);
        assert!(matches!(results[0].as_ref(), ActionTaskConfig::Noop));
        assert!(matches!(results[1].as_ref(), ActionTaskConfig::Noop));
    }
}
