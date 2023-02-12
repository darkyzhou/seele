use std::{io::Read, sync::Arc};

use anyhow::{bail, Context, Result};
use duct::cmd;
use nix::{
    sys::signal::{self, Signal},
    unistd::Pid,
};
use once_cell::sync::Lazy;
use thread_local::ThreadLocal;
use tokio::{sync::oneshot, task::spawn_blocking};
use tracing::{error, info, info_span, instrument, warn, Span};
use triggered::Listener;

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

#[instrument(skip_all, name = "action_run_container_execute")]
pub async fn execute(
    abort: Listener,
    ctx: &ActionContext,
    config: &Config,
) -> Result<ActionSuccessReportExt> {
    image::prepare_image(abort.clone(), config.image.clone())
        .await
        .context("Error preparing the container image")?;

    let config =
        convert_to_runj_config(ctx, config.clone()).context("Error converting the config")?;
    check_and_create_directories(&config).await?;

    let report = spawn_blocking({
        let local = RUNNER_THREAD_LOCAL.clone();
        let span = info_span!(parent: Span::current(), "prepare_and_execute_runj");
        move || span.in_scope(move || prepare_and_execute_runj(abort, &local, config))
    })
    .await??;

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

fn prepare_and_execute_runj(
    abort: Listener,
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

        info!("Bound the runj container to cpu {}", cpu);
        config.limits.cgroup.cpuset_cpus = Some(format!("{}", cpu));
    }

    let config =
        serde_json::to_string(&config).context("Error serializing the converted config")?;

    let mut output = vec![];
    let mut reader = cmd!(&conf::CONFIG.paths.runj)
        .stdin_bytes(config.as_bytes())
        .stderr_to_stdout()
        .reader()
        .context("Error running the runj process")?;
    let pids = reader.pids();

    let (cancel_tx, cancel_rx) = oneshot::channel();
    tokio::spawn({
        let abort = abort.clone();
        let span = Span::current();
        async move {
            tokio::select! {
                _ = cancel_rx => {},
                _ = abort => {
                    for pid in &pids {
                        _ = signal::kill(Pid::from_raw(*pid as i32), Signal::SIGTERM);
                    }
                    info!(parent: span, "Sent SIGTERM to pids: {pids:?}");
                }
            }
        }
    });

    let result = reader.read_to_end(&mut output);

    if abort.is_triggered() {
        bail!("Aborted");
    }

    _ = cancel_tx.send(());

    match result {
        Ok(_) => {
            let report: ContainerExecutionReport =
                serde_json::from_slice(&output[..]).context("Error deserializing the report")?;
            info!(
                seele.container.status = %report.status,
                seele.container.code = report.exit_code,
                seele.container.signal = report.signal,
                "Run container completed"
            );
            Ok(report)
        }
        Err(_) => {
            let texts = {
                let output = output.into_iter().take(1024).collect::<Vec<_>>();
                String::from_utf8_lossy(&output[..]).to_string()
            };
            error!(seele.error = %texts, "The runj process failed");
            bail!("The runj process failed: {}", texts)
        }
    }
}
