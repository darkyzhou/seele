use self::eviction::EvictionManager;
use crate::{
    conf,
    entity::{ActionTaskConfig, TaskFailedReport, TaskReport, TaskSuccessReport},
};
use anyhow::Context;
use chrono::Utc;
use std::{sync::Arc, time::Duration};
use tokio::{
    fs::{self, File},
    io::AsyncWriteExt,
    sync::oneshot,
    time::Instant,
};
use tokio_graceful_shutdown::{FutureExt, SubsystemHandle};
use tracing::{error, info, instrument, warn};

pub use action::*;

mod action;
mod eviction;

pub struct WorkerQueueItem {
    pub submission_id: String,
    pub config: Arc<ActionTaskConfig>,
    pub report_tx: oneshot::Sender<TaskReport>,
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
            let mut submission_file = File::create($file).await?;
            let mut data = vec![];
            manager.save_states(&mut data[..]).await?;
            submission_file.write_all(&data[..]).await?;
        }
    };
}

pub async fn worker_main(handle: SubsystemHandle, queue_rx: WorkerQueueRx) -> anyhow::Result<()> {
    let submission_eviction_file = conf::PATHS.states.join("submission_eviction");
    let image_eviction_file = conf::PATHS.states.join("image_eviction");

    {
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

        for i in 0..conf::CONFIG.concurrency {
            let queue_rx = queue_rx.clone();
            let submission_eviction_manager = submission_eviction_manager.clone();
            let image_eviction_manager = image_eviction_manager.clone();
            handle.start(&format!("worker-{}", i), |handle| {
                worker_main_impl(
                    handle,
                    queue_rx,
                    submission_eviction_manager,
                    image_eviction_manager,
                )
            });
        }
    }

    drop(queue_rx);
    handle.on_shutdown_requested().await;
    Ok(())
}

async fn worker_main_impl(
    handle: SubsystemHandle,
    queue_rx: WorkerQueueRx,
    submission_eviction_manager: Arc<Option<EvictionManager>>,
    image_eviction_manager: Arc<Option<EvictionManager>>,
) -> anyhow::Result<()> {
    while let Ok(Ok(ctx)) = queue_rx.recv().cancel_on_shutdown(&handle).await {
        let report = match handle_action(
            ctx.submission_id,
            &ctx.config,
            submission_eviction_manager.clone(),
            image_eviction_manager.clone(),
        )
        .await
        {
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
    submission_eviction_manager: Arc<Option<EvictionManager>>,
    image_eviction_manager: Arc<Option<EvictionManager>>,
) -> anyhow::Result<TaskReport> {
    let ctx = Arc::new(ActionContext {
        submission_root: conf::PATHS.submissions.join(&submission_id),
        submission_eviction_manager,
        image_eviction_manager,
    });

    if fs::metadata(&ctx.submission_root).await.is_ok() {
        warn!(path = %ctx.submission_root.display(), "The submission directory already exists, it may because of the duplicate submission id, now deleting it");
        fs::remove_dir_all(&ctx.submission_root).await?;
    }

    fs::create_dir_all(&ctx.submission_root)
        .await
        .context("Error creating the submission directory")?;
    if let Some(manager) = ctx.submission_eviction_manager.as_ref() {
        manager.visit_enter(&ctx.submission_root).await;
    }

    let manager = ctx.submission_eviction_manager.clone();
    let root = ctx.submission_root.clone();

    let result = async move {
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
    .await;

    if let Some(manager) = manager.as_ref() {
        manager.visit_leave(&root).await;
    }

    result
}
