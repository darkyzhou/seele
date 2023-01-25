use self::utils::{check_and_create_directories, convert_to_runj_config};
use super::ActionContext;
use crate::entities::ActionExecutionReport;
use crate::{conf, shared};
use anyhow::{anyhow, bail, Context};
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tracing::{debug, error, instrument};

pub use self::config::*;
pub use self::runj::ContainerExecutionReport;

mod config;
mod image;
pub mod run_judge;
pub mod runj;
mod utils;

#[instrument]
pub async fn run_container(
    ctx: &ActionContext,
    config: &ActionRunContainerConfig,
) -> anyhow::Result<ActionExecutionReport> {
    debug!("Preparing the image");
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

        let output = {
            debug!(runj_config = config, "Running runj");
            let mut child = Command::new(&conf::CONFIG.runj_path)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .context("Error spawning the runj process")?;
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| anyhow!("Error opening stdin of the runj process"))?;
            tokio::spawn(async move {
                if let Err(err) = stdin.write_all(config.as_bytes()).await {
                    error!("Error passing the config to the runj process: {:#}", err);
                }
            });

            child.wait_with_output().await.context("Error awaiting the runj process")?
        };

        if !output.status.success() {
            let texts = shared::collect_output(&output);
            error!(texts = texts, code = output.status.code(), "Error running runj");
            bail!("Error running runj")
        }

        let report: ContainerExecutionReport =
            serde_json::from_slice(&output.stdout[..]).context("Error deserializing the report")?;
        Ok(ActionExecutionReport::RunContainer(report))
    }
    .await;

    if let Some(manager) = ctx.image_eviction_manager.as_ref() {
        manager.visit_leave(&image_path).await;
    }

    result
}
