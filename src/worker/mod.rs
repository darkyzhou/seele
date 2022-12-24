use crate::{
    conf,
    entity::{ActionTaskConfig, TaskFailedReport, TaskReport, TaskSuccessReport},
};
use std::{sync::Arc, time::SystemTime};
use tokio::sync::oneshot;
use tokio_graceful_shutdown::{FutureExt, SubsystemHandle};
use tracing::error;

mod action;

pub type WorkerQueueItem = (Arc<ActionTaskConfig>, oneshot::Sender<TaskReport>);

pub async fn worker_main(
    handle: SubsystemHandle,
    queue_rx: async_channel::Receiver<WorkerQueueItem>,
) -> anyhow::Result<()> {
    for i in 0..conf::CONFIG.concurrency {
        let queue_rx = queue_rx.clone();
        handle.start(&format!("worker-{}", i), |handle| worker_main_impl(handle, queue_rx));
    }

    handle.on_shutdown_requested().await;
    Ok(())
}

async fn worker_main_impl(
    handle: SubsystemHandle,
    queue_rx: async_channel::Receiver<WorkerQueueItem>,
) -> anyhow::Result<()> {
    while let Ok(Ok((config, tx))) = queue_rx.recv().cancel_on_shutdown(&handle).await {
        let report = match handle_action(&config).await {
            Err(err) => TaskReport::Failed(TaskFailedReport::Action {
                run_at: None,
                time_elapsed_ms: None,
                message: format!("Error handling the action: {:#?}", err),
            }),
            Ok(report) => report,
        };

        if let Err(err) = tx.send(report) {
            error!("Error sending the report");
        }
    }

    Ok(())
}

async fn handle_action(task: &ActionTaskConfig) -> anyhow::Result<TaskReport> {
    let now = std::time::Instant::now();
    let run_at = SystemTime::now();

    let result = match task {
        ActionTaskConfig::Noop(config) => action::run_noop_action(config).await,
        ActionTaskConfig::AddFile(config) => todo!(),
    };

    let time_elapsed_ms = {
        let new_now = std::time::Instant::now();
        new_now.saturating_duration_since(now).as_millis().try_into()?
    };

    Ok(match result {
        Err(err) => TaskReport::Failed(TaskFailedReport::Action {
            run_at: Some(run_at),
            time_elapsed_ms: Some(time_elapsed_ms),
            message: format!("Error running the action: {:#?}", err),
        }),
        Ok(extra) => {
            TaskReport::Success(TaskSuccessReport::Action { run_at, time_elapsed_ms, extra })
        }
    })
}
