use crate::{
    entity::{
        ActionTaskConfig, Submission, TaskExecutionFailedReport, TaskExecutionReport, TaskNode,
        TaskNodeExtra, TaskReport, TaskStatus,
    },
    worker::WorkerQueueItem,
};
use std::{iter, sync::Arc, time::SystemTime};
use tokio::sync::oneshot;

pub async fn execute_submission(
    worker_queue_tx: async_channel::Sender<WorkerQueueItem>,
    submission: Submission,
) -> anyhow::Result<Submission> {
    let mut queue = flatten_tasks(iter::once(submission.root.clone()));
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
            let config = submission.id_to_config_map.get(id).unwrap();
            let node = submission.id_to_node_map.get(id).unwrap();

            // TODO: parent

            *config.status.lock().unwrap() = match report {
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

            let should_continue = false;
            if !should_continue {
                mark_children_as_skipped(&submission, node);
                continue;
            }

            next_queue.extend(flatten_tasks(node.children.iter().cloned()));
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

fn mark_children_as_skipped(submission: &Submission, task: &TaskNode) {
    for node in &task.children {
        if let Some(node) = submission.id_to_config_map.get(&node.id) {
            *node.status.lock().unwrap() = TaskStatus::Skipped;
        }
        mark_children_as_skipped(submission, node);
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
