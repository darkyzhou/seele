use crate::{
    conf,
    entity::{ActionTaskConfig, TaskFailedReport, TaskReport, TaskSuccessReport},
};
use chrono::Utc;
use std::{path::PathBuf, sync::Arc};
use tokio::{sync::oneshot, time::Instant};
use tokio_graceful_shutdown::{FutureExt, SubsystemHandle};
use tracing::error;

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
                message: format!("Error handling the action: {:#?}", err),
            }),
            Ok(report) => report,
        };

        if let Err(err) = ctx.report_tx.send(report) {
            error!("Error sending the report: {:#?}", err);
        }
    }

    Ok(())
}

async fn handle_action(
    submission_id: String,
    task: &ActionTaskConfig,
) -> anyhow::Result<TaskReport> {
    let context = action::ActionContext {
        submission_root: &[conf::CONFIG.root_path.clone(), "submissions".into(), submission_id]
            .iter()
            .collect::<PathBuf>(),
    };

    let now = Instant::now();
    let run_at = Utc::now();
    let result = match task {
        ActionTaskConfig::Noop(config) => action::run_noop_action(config).await,
        ActionTaskConfig::AddFile(config) => action::run_add_file_action(&context, config).await,
    };
    let time_elapsed_ms = {
        let new_now = Instant::now();
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
