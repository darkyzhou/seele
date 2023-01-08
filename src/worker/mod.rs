use crate::{
    conf,
    entity::{ActionTaskConfig, TaskFailedReport, TaskReport, TaskSuccessReport},
};
use chrono::Utc;
use std::sync::Arc;
use tokio::{sync::oneshot, time::Instant};
use tokio_graceful_shutdown::{FutureExt, SubsystemHandle};
use tracing::{error, instrument};

pub use action::*;

mod action;

pub struct WorkerQueueItem {
    pub submission_id: String,
    pub config: Arc<ActionTaskConfig>,
    pub report_tx: oneshot::Sender<TaskReport>,
}

pub type WorkerQueueTx = async_channel::Sender<WorkerQueueItem>;
pub type WorkerQueueRx = async_channel::Receiver<WorkerQueueItem>;

pub async fn worker_main(handle: SubsystemHandle, queue_rx: WorkerQueueRx) -> anyhow::Result<()> {
    for i in 0..conf::CONFIG.concurrency {
        let queue_rx = queue_rx.clone();
        handle.start(&format!("worker-{}", i), |handle| worker_main_impl(handle, queue_rx));
    }

    handle.on_shutdown_requested().await;
    Ok(())
}

async fn worker_main_impl(handle: SubsystemHandle, queue_rx: WorkerQueueRx) -> anyhow::Result<()> {
    while let Ok(Ok(ctx)) = queue_rx.recv().cancel_on_shutdown(&handle).await {
        let report = match handle_action(ctx.submission_id, &ctx.config).await {
            Err(err) => TaskReport::Failed(TaskFailedReport::Action {
                run_at: None,
                time_elapsed_ms: None,
                message: format!("Error handling the action: {:#}", err),
            }),
            Ok(report) => report,
        };

        if ctx.report_tx.send(report).is_err() {
            error!("Error sending the report");
        }
    }

    Ok(())
}

#[instrument]
async fn handle_action(
    submission_id: String,
    task: &ActionTaskConfig,
) -> anyhow::Result<TaskReport> {
    let ctx =
        action::ActionContext { submission_root: conf::PATHS.submissions.join(submission_id) };

    let now = Instant::now();
    let run_at = Utc::now();
    let result = match task {
        ActionTaskConfig::Noop(config) => action::noop(config).await,
        ActionTaskConfig::AddFile(config) => action::add_file(&ctx, config).await,
        ActionTaskConfig::RunContainer(config) => action::run_container(&ctx, config).await,
    };
    let time_elapsed_ms = {
        let new_now = Instant::now();
        new_now.saturating_duration_since(now).as_millis().try_into()?
    };

    Ok(match result {
        Err(err) => TaskReport::Failed(TaskFailedReport::Action {
            run_at: Some(run_at),
            time_elapsed_ms: Some(time_elapsed_ms),
            message: format!("Error running the action: {:#}", err),
        }),
        Ok(report) => {
            TaskReport::Success(TaskSuccessReport::Action { run_at, time_elapsed_ms, report })
        }
    })
}
