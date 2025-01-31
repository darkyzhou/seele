use std::{
    io::Read,
    sync::{Arc, LazyLock},
};

use anyhow::{Context, Result, bail};
use duct::cmd;
use nix::{
    sys::signal::{self, Signal},
    unistd::Pid,
};
use seele_shared::entities::run_container::{
    Config,
    runj::{ContainerExecutionReport, ContainerExecutionStatus, RunjConfig},
};
use thread_local::ThreadLocal;
use tokio::sync::oneshot;
use tracing::{Span, info, info_span, warn};
use triggered::Listener;

use self::utils::{check_and_create_directories, cleanup_overlayfs, make_runj_config};
pub use self::{idmap::*, image::prepare_image};
use super::ActionContext;
use crate::{
    cgroup, conf,
    entities::{ActionFailureReportExt, ActionReportExt, ActionSuccessReportExt},
    shared::runner,
};

pub mod cache;
mod idmap;
mod image;
pub mod run_judge;
mod utils;

static RUNNER_THREAD_LOCAL: LazyLock<Arc<ThreadLocal<i64>>> = LazyLock::new(Arc::default);

pub async fn execute(
    abort: Listener,
    ctx: &ActionContext,
    config: &Config,
) -> Result<ActionReportExt> {
    image::prepare_image(abort.clone(), config.image.clone())
        .await
        .context("Error preparing the container image")?;

    let runj_config =
        make_runj_config(ctx, config.clone()).await.context("Error converting the config")?;
    check_and_create_directories(&runj_config).await?;

    let report = runner::spawn_blocking({
        let local = RUNNER_THREAD_LOCAL.clone();
        let span = info_span!(
            parent: Span::current(),
            "execute_runj",
            seele.image = %config.image,
            seele.command = %config.command,
        );
        move || span.in_scope(move || execute_runj(abort, &local, runj_config))
    })
    .await??;

    Ok(match report.status {
        ContainerExecutionStatus::Normal => {
            ActionReportExt::Success(ActionSuccessReportExt::RunContainer(report))
        }
        _ => {
            if matches!(report.status, ContainerExecutionStatus::Unknown) {
                warn!("Unknown container execution status");
            }

            ActionReportExt::Failure(ActionFailureReportExt::RunContainer(report))
        }
    })
}

fn execute_runj(
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

        info!("Bound the runj container to cpu {cpu}");
        config.limits.cgroup.cpuset_cpus = Some(format!("{cpu}"));
    }

    let config_json =
        serde_json::to_string(&config).context("Error serializing the converted config")?;

    let mut output = vec![];
    let mut reader = cmd!(&conf::CONFIG.paths.runj)
        .stdin_bytes(config_json.as_bytes())
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

    if let Err(err) = cleanup_overlayfs(&config.overlayfs) {
        warn!("Error cleaning up overlayfs directories: {err:#}");
    }

    _ = cancel_tx.send(());
    if abort.is_triggered() {
        bail!(crate::shared::ABORTED_MESSAGE);
    }

    match result {
        Ok(_) => {
            let report: ContainerExecutionReport =
                serde_json::from_slice(&output[..]).context("Error deserializing the report")?;
            info!(
                seele.container.status = %report.status,
                seele.container.code = report.exit_code,
                seele.container.signal = report.signal,
                seele.container.cpu_user_time = report.cpu_user_time_ms,
                seele.container.cpu_kernel_time = report.cpu_kernel_time_ms,
                seele.container.memory_usage = report.memory_usage_kib,
                "Run container completed"
            );
            Ok(report)
        }
        Err(_) => {
            let texts = {
                let output = output.into_iter().take(1024).collect::<Vec<_>>();
                String::from_utf8_lossy(&output[..]).to_string()
            };
            bail!("The runj process failed: {texts}")
        }
    }
}
