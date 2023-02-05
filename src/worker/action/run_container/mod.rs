use std::{io::Read, sync::Arc};

use anyhow::{bail, Context};
use duct::cmd;
use once_cell::sync::Lazy;
use thread_local::ThreadLocal;
use tokio::{sync::oneshot, task::spawn_blocking};
use tracing::{debug, error, instrument, warn};

pub use self::entities::*;
use self::{
    runj::ContainerExecutionStatus,
    utils::{check_and_create_directories, convert_to_runj_config},
};
use super::ActionContext;
use crate::{
    cgroup, conf,
    entities::{ActionFailedReportExt, ActionSuccessReportExt},
    worker::{
        run_container::runj::{ContainerExecutionReport, RunjConfig},
        ActionErrorWithReport,
    },
};

mod entities;
mod image;
pub mod run_judge;
mod runj;
mod utils;

static RUNNER_THREAD_LOCAL: Lazy<Arc<ThreadLocal<i64>>> = Lazy::new(|| Arc::default());

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
        let config =
            convert_to_runj_config(ctx, config.clone()).context("Error converting the config")?;

        check_and_create_directories(&config).await?;

        let (tx, rx) = oneshot::channel();
        {
            let local = RUNNER_THREAD_LOCAL.clone();
            spawn_blocking(move || {
                fn run(
                    local: &ThreadLocal<i64>,
                    mut config: RunjConfig,
                ) -> anyhow::Result<ContainerExecutionReport> {
                    {
                        let cpu = match local.get() {
                            Some(cpu) => *cpu,
                            None => {
                                let cpu = cgroup::get_self_cpuset_cpu()
                                    .context("Error getting self cpuset cpu")?;
                                _ = local.get_or(|| cpu);
                                cpu
                            }
                        };

                        debug!("Bound the runj container to cpu {}", cpu);
                        config.limits.cgroup.cpuset_cpus = Some(format!("{}", cpu));
                    }

                    let mut reader = {
                        let config = serde_json::to_string(&config)
                            .context("Error serializing the converted config")?;

                        cmd!(&conf::CONFIG.runj_path)
                            .stdin_bytes(config.as_bytes())
                            .stderr_to_stdout()
                            .reader()
                            .context("Error running the runj process")?
                    };

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

                if tx.send(run(&local, config)).is_err() {
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
            _ => {
                if matches!(report.status, ContainerExecutionStatus::Unknown) {
                    warn!("Unknown container execution status");
                }

                bail!(ActionErrorWithReport::new(ActionFailedReportExt::RunContainer(report)))
            }
        },
    }
}
