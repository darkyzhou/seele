use std::{error::Error, fmt::Display, path::PathBuf, sync::Arc, time::Duration};

pub use action::*;
use anyhow::Context;
use chrono::Utc;
use tokio::{
    fs::{self, File},
    sync::oneshot,
    time::Instant,
};
use tokio_graceful_shutdown::{FutureExt, SubsystemHandle};
use tracing::{error, info, instrument};

use self::eviction::EvictionManager;
use crate::{
    conf,
    entities::{
        ActionFailedReport, ActionFailedReportExt, ActionReport, ActionSuccessReport,
        ActionTaskConfig,
    },
};

pub mod action;
mod eviction;

#[derive(Debug)]
pub struct WorkerQueueItem {
    pub submission_id: String,
    pub submission_root: PathBuf,
    pub config: Arc<ActionTaskConfig>,
    pub report_tx: oneshot::Sender<ActionReport>,
}

#[derive(Debug, Clone)]
pub struct ActionErrorWithReport {
    report: ActionFailedReportExt,
}

impl ActionErrorWithReport {
    pub fn new(report: ActionFailedReportExt) -> Self {
        Self { report }
    }
}

impl Error for ActionErrorWithReport {}

impl Display for ActionErrorWithReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Action execution failed with a report")
    }
}

pub type WorkerQueueTx = async_channel::Sender<WorkerQueueItem>;
pub type WorkerQueueRx = async_channel::Receiver<WorkerQueueItem>;

macro_rules! new_eviction_manager {
    ($name:expr, $config:expr, $file:expr) => {
        Arc::new(match $config {
            None => None,
            Some(config) => Some(
                EvictionManager::new(
                    $name.to_string(),
                    Duration::from_secs(60 * config.ttl_minute),
                    Duration::from_secs(60 * config.interval_minute),
                    config.capacity,
                    File::open($file).await.ok(),
                )
                .await
                .with_context(|| format!("Error initializing the {} eviction manager", $name))?,
            ),
        })
    };
}

macro_rules! save_states {
    ($manager:expr, $file:expr) => {
        if let Some(manager) = $manager.as_ref() {
            let mut data = Vec::new();
            manager.save_states(&mut data).await?;
            fs::write($file, &data).await?;
        }
    };
}

pub async fn worker_main(handle: SubsystemHandle, queue_rx: WorkerQueueRx) -> anyhow::Result<()> {
    {
        let submission_eviction_file = conf::PATHS.states.join("submission_eviction");
        let image_eviction_file = conf::PATHS.states.join("image_eviction");
        let submission_eviction_manager = new_eviction_manager!(
            "submission",
            conf::CONFIG.worker.submission_eviction,
            &submission_eviction_file
        );
        let image_eviction_manager = new_eviction_manager!(
            "image",
            conf::CONFIG.worker.image_eviction,
            &image_eviction_file
        );

        {
            let submission_eviction_manager = submission_eviction_manager.clone();
            let image_eviction_manager = image_eviction_manager.clone();
            handle.start("eviction", move |handle| async move {
                {
                    let submission_eviction_manager = submission_eviction_manager.clone();
                    let image_eviction_manager = image_eviction_manager.clone();

                    handle.start("submission", move |handle| async move {
                        if let Some(manager) = submission_eviction_manager.as_ref() {
                            let _ = manager.run_loop().cancel_on_shutdown(&handle).await;
                        }

                        anyhow::Ok(())
                    });

                    handle.start("image", move |handle| async move {
                        if let Some(manager) = image_eviction_manager.as_ref() {
                            let _ = manager.run_loop().cancel_on_shutdown(&handle).await;
                        }

                        anyhow::Ok(())
                    });
                }

                handle.start("save_states", |handle| async move {
                    handle.on_shutdown_requested().await;

                    info!("Saving eviction manager states");
                    save_states!(submission_eviction_manager, submission_eviction_file);
                    save_states!(image_eviction_manager, image_eviction_file);

                    anyhow::Ok(())
                });

                handle.on_shutdown_requested().await;
                anyhow::Ok(())
            });
        }

        handle.start("worker", |handle| {
            worker_main_impl(handle, queue_rx, submission_eviction_manager, image_eviction_manager)
        });
    }

    handle.on_shutdown_requested().await;
    Ok(())
}

async fn worker_main_impl(
    handle: SubsystemHandle,
    queue_rx: WorkerQueueRx,
    submission_eviction_manager: Arc<Option<EvictionManager>>,
    image_eviction_manager: Arc<Option<EvictionManager>>,
) -> anyhow::Result<()> {
    while let Ok(Ok(item)) = queue_rx.recv().cancel_on_shutdown(&handle).await {
        let report = execute_action(
            item.submission_id.clone(),
            item.submission_root,
            &item.config,
            submission_eviction_manager.clone(),
            image_eviction_manager.clone(),
        )
        .await;

        if item.report_tx.send(report).is_err() {
            error!(submission_id = item.submission_id, "Error sending the report");
        }
    }

    Ok(())
}

#[instrument(skip(submission_eviction_manager, image_eviction_manager))]
async fn execute_action(
    _submission_id: String,
    submission_root: PathBuf,
    task: &ActionTaskConfig,
    submission_eviction_manager: Arc<Option<EvictionManager>>,
    image_eviction_manager: Arc<Option<EvictionManager>>,
) -> ActionReport {
    let ctx = Arc::new(ActionContext {
        submission_root,
        submission_eviction_manager,
        image_eviction_manager,
    });

    if let Some(manager) = ctx.submission_eviction_manager.as_ref() {
        manager.visit_enter(&ctx.submission_root).await;
    }

    let now = Instant::now();
    let run_at = Utc::now();
    let result = match task {
        ActionTaskConfig::Noop(config) => action::noop::execute(config).await,
        ActionTaskConfig::AddFile(config) => action::add_file::execute(&ctx, config).await,
        ActionTaskConfig::RunContainer(config) => {
            action::run_container::execute(&ctx, config).await
        }
        ActionTaskConfig::RunJudgeCompile(config) => {
            action::run_judge::compile::execute(&ctx, config).await
        }
        ActionTaskConfig::RunJudgeRun(config) => {
            action::run_judge::run::execute(&ctx, config).await
        }
    };
    let time_elapsed_ms = {
        let new_now = Instant::now();
        new_now.saturating_duration_since(now).as_millis().try_into().unwrap()
    };

    if let Some(manager) = ctx.submission_eviction_manager.as_ref() {
        manager.visit_leave(&ctx.submission_root).await;
    }

    match result {
        Err(err) => ActionFailedReport {
            run_at: Some(run_at),
            time_elapsed_ms: Some(time_elapsed_ms),
            error: format!("Error executing the action: {err:#}"),
            ext: err
                .root_cause()
                .downcast_ref::<ActionErrorWithReport>()
                .map(|err| err.report.clone()),
        }
        .into(),
        Ok(ext) => ActionSuccessReport { run_at, time_elapsed_ms, ext }.into(),
    }
}
