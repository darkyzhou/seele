use std::{path::PathBuf, sync::Arc};

use anyhow::{bail, Result};
use chrono::Utc;
use futures_util::{future, TryFutureExt};
use tokio::{
    sync::{mpsc, oneshot},
    time::Instant,
};
use tokio_graceful_shutdown::SubsystemHandle;
use tracing::*;
use triggered::Listener;

pub use self::action::*;
use crate::{conf, entities::*};

pub mod action;

#[derive(Debug)]
pub struct WorkerQueueItem {
    pub parent_span: Span,
    pub submission_id: String,
    pub submission_root: PathBuf,
    pub config: Arc<ActionTaskConfig>,
    pub report_tx: oneshot::Sender<Result<ActionReport>>,
}

pub type WorkerQueueTx = mpsc::Sender<WorkerQueueItem>;
pub type WorkerQueueRx = mpsc::Receiver<WorkerQueueItem>;

pub async fn worker_bootstrap(handle: SubsystemHandle, tx: oneshot::Sender<bool>) -> Result<()> {
    action::run_container::cache::init().await;

    let preload_images = &conf::CONFIG.worker.action.run_container.preload_images;
    if preload_images.is_empty() {
        _ = tx.send(true);
        return Ok(());
    }

    let (trigger, abort) = triggered::trigger();
    tokio::spawn(async move {
        handle.on_shutdown_requested().await;
        trigger.trigger();
    });

    info!(
        "Preloading container images: {}",
        preload_images.iter().map(|image| format!("{image}")).collect::<Vec<_>>().join(", ")
    );
    let results = future::join_all(preload_images.iter().map(|image| {
        action::run_container::prepare_image(abort.clone(), image.clone())
            .map_err(move |err| format!("{image}: {err:#}"))
    }))
    .await;
    let messages = results.into_iter().filter_map(|item| item.err()).collect::<Vec<_>>().join("\n");
    if !messages.is_empty() {
        _ = tx.send(false);
        bail!("Error preloading following container images:\n{messages}");
    }

    _ = tx.send(true);
    Ok(())
}

pub async fn worker_main(handle: SubsystemHandle, mut queue_rx: WorkerQueueRx) -> Result<()> {
    let (trigger, abort_handle) = triggered::trigger();

    tokio::spawn(async move {
        handle.on_shutdown_requested().await;
        trigger.trigger();
    });

    loop {
        let outer_handle = abort_handle.clone();
        tokio::select! {
            _ = outer_handle => break,
            item = queue_rx.recv() => match item {
                None => break,
                Some(item) => {
                    tokio::spawn({
                        let abort_handle = abort_handle.clone();
                        let span = info_span!(parent: item.parent_span, "worker_handle_submission");
                        async move {
                            let report = execute_action(abort_handle.clone(), item.submission_root, &item.config).await;

                            if item.report_tx.send(report).is_err() {
                                error!(submission_id = item.submission_id, "Error sending the report");
                            }
                        }
                        .instrument(span)
                    });
                }
            }
        }
    }

    Ok(())
}

async fn execute_action(
    handle: Listener,
    submission_root: PathBuf,
    task: &ActionTaskConfig,
) -> Result<ActionReport> {
    let ctx = Arc::new(ActionContext { submission_root });

    let begin = Instant::now();
    let run_at = Utc::now();
    let ext = match task {
        ActionTaskConfig::Noop(config) => action::noop::execute(config).await?,
        ActionTaskConfig::AddFile(config) => {
            action::add_file::execute(handle, &ctx, config).await?
        }
        ActionTaskConfig::RunContainer(config) => {
            action::run_container::execute(handle, &ctx, config).await?
        }
        ActionTaskConfig::RunJudgeCompile(config) => {
            action::run_container::run_judge::compile::execute(handle, &ctx, config).await?
        }
        ActionTaskConfig::RunJudgeRun(config) => {
            action::run_container::run_judge::run::execute(handle, &ctx, config).await?
        }
    };
    let time_elapsed_ms = {
        let end = Instant::now();
        end.duration_since(begin).as_millis().try_into().unwrap()
    };

    Ok(match ext {
        ActionReportExt::Success(ext) => {
            ActionReport::Success(ActionSuccessReport { run_at, time_elapsed_ms, ext })
        }
        ActionReportExt::Failure(ext) => {
            ActionReport::Failed(ActionFailedReport { run_at, time_elapsed_ms, ext })
        }
    })
}
