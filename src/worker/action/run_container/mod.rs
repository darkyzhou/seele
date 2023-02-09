use std::{io::Read, sync::Arc};

use anyhow::{bail, Context, Result};
use duct::cmd;
use once_cell::sync::Lazy;
use thread_local::ThreadLocal;
use tokio::task::spawn_blocking;
use tracing::{debug, error, instrument, warn};

pub use self::{entities::*, idmap::*};
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
mod idmap;
mod image;
pub mod run_judge;
mod runj;
mod utils;

static RUNNER_THREAD_LOCAL: Lazy<Arc<ThreadLocal<i64>>> = Lazy::new(|| Arc::default());

#[instrument]
pub async fn execute(ctx: &ActionContext, config: &Config) -> Result<ActionSuccessReportExt> {
    image::prepare_image(&config.image).await.context("Error preparing the container image")?;

    let config =
        convert_to_runj_config(ctx, config.clone()).context("Error converting the config")?;
    check_and_create_directories(&config).await?;

    let local = RUNNER_THREAD_LOCAL.clone();
    let report = spawn_blocking(move || execute_runj(&local, config)).await??;

    match report.status {
        ContainerExecutionStatus::Normal => Ok(ActionSuccessReportExt::RunContainer(report)),
        _ => {
            if matches!(report.status, ContainerExecutionStatus::Unknown) {
                warn!("Unknown container execution status");
            }

            bail!(ActionErrorWithReport::new(ActionFailedReportExt::RunContainer(report)))
        }
    }
}

fn execute_runj(
    local: &ThreadLocal<i64>,
    mut config: RunjConfig,
) -> Result<ContainerExecutionReport> {
    {
        let cpu = match local.get() {
            Some(cpu) => *cpu,
            None => {
                let cpu = cgroup::get_self_cpuset_cpu().context("Error getting self cpuset cpu")?;
                _ = local.get_or(|| cpu);
                cpu
            }
        };

        // FIXME: parent span
        debug!("Bound the runj container to cpu {}", cpu);
        config.limits.cgroup.cpuset_cpus = Some(format!("{}", cpu));
    }

    let mut reader = {
        let config =
            serde_json::to_string(&config).context("Error serializing the converted config")?;

        cmd!(&conf::CONFIG.runj_path)
            .stdin_bytes(config.as_bytes())
            .stderr_to_stdout()
            .reader()
            .context("Error running the runj process")?
    };

    let mut output = vec![];
    match reader.read_to_end(&mut output) {
        Ok(_) => serde_json::from_slice(&output[..]).context("Error deserializing the report"),
        Err(_) => {
            let texts = {
                let output = output.into_iter().take(1024).collect::<Vec<_>>();
                String::from_utf8_lossy(&output[..]).to_string()
            };
            error!(texts = %texts, "The runj process failed");
            bail!("The runj process failed: {}", texts)
        }
    }
}
