use std::io::Read;

use anyhow::{bail, Context};
use duct::cmd;
use once_cell::sync::Lazy;
use threadpool::ThreadPool;
use tokio::sync::{oneshot, Mutex};
use tracing::{error, instrument};

pub use self::entities::*;
use self::{
    runj::ContainerExecutionStatus,
    utils::{check_and_create_directories, convert_to_runj_config},
};
use super::ActionContext;
use crate::{
    conf,
    entities::{ActionFailedReportExt, ActionSuccessReportExt},
    worker::{run_container::runj::ContainerExecutionReport, ActionErrorWithReport},
};

mod entities;
mod image;
pub mod run_judge;
mod runj;
mod utils;

static RUNNER_POOL: Lazy<Mutex<ThreadPool>> = Lazy::new(|| {
    Mutex::new(ThreadPool::new(conf::CONFIG.worker.action.run_container.container_concurrency))
});

#[instrument]
pub async fn execute(
    ctx: &ActionContext,
    config: &Config,
) -> anyhow::Result<ActionSuccessReportExt> {
    let image_path = image::get_image_path(&config.image);
    if let Some(manager) = ctx.image_eviction_manager.as_ref() {
        manager.visit_enter(&image_path).await;
    }
    image::prepare_image(&config.image).await.context("Error preparing the container image")?;

    let result = async move {
        let config = {
            let config = convert_to_runj_config(ctx, config.clone())
                .context("Error converting the config")?;

            check_and_create_directories(&config).await?;

            serde_json::to_string(&config).context("Error serializing the converted config")?
        };

        let (tx, rx) = oneshot::channel();
        {
            RUNNER_POOL.lock().await.execute(move || {
                fn run(config: &str) -> anyhow::Result<ContainerExecutionReport> {
                    let mut reader = cmd!(&conf::CONFIG.runj_path)
                        .stdin_bytes(config.as_bytes())
                        .stderr_to_stdout()
                        .reader()
                        .context("Error running the runj process")?;

                    let mut output = vec![];
                    match reader.read_to_end(&mut output) {
                        Err(_) => {
                            let texts = {
                                let output = output.into_iter().take(400).collect::<Vec<_>>();
                                String::from_utf8_lossy(&output[..]).to_string()
                            };
                            error!(texts = %texts, "The runj process failed");
                            bail!("The runj process failed: {}", texts)
                        }
                        Ok(_) => serde_json::from_slice(&output[..])
                            .context("Error deserializing the report"),
                    }
                }

                if tx.send(run(&config)).is_err() {
                    error!("Error sending the report to parent");
                }
            });
        }

        rx.await?
    }
    .await;

    if let Some(manager) = ctx.image_eviction_manager.as_ref() {
        manager.visit_leave(&image_path).await;
    }

    match result {
        Err(err) => Err(err),
        Ok(report) => match report.status {
            ContainerExecutionStatus::Normal => Ok(ActionSuccessReportExt::RunContainer(report)),
            _ => bail!(ActionErrorWithReport::new(ActionFailedReportExt::RunContainer(report))),
        },
    }
}
