use super::predicate;
use crate::{
    entity::{
        ActionTaskConfig, Submission, TaskFailedReport, TaskNode, TaskNodeExtra, TaskReport,
        TaskStatus, TaskSuccessReport,
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
            let node = submission.id_to_node_map.get(&queue[i].0.id).unwrap();

            *node.config.status.write().unwrap() = match report {
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

            if let Some(parent) = node
                .schedule_parent_id
                .as_ref()
                .and_then(|parent| submission.id_to_node_map.get(parent))
            {
                if let TaskNodeExtra::Schedule(nodes) = &parent.extra {
                    *parent.config.status.write().unwrap() = {
                        let mut status = TaskStatus::Success(TaskSuccessReport::Schedule);
                        for child_status in
                            nodes.iter().map(|node| node.config.status.read().unwrap())
                        {
                            match *child_status {
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
                    };
                }
            }

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

                    insta::assert_debug_snapshot!(results);
                },
            );
        })
    }
}
