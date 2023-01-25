use super::{image, runj, ActionRunContainerConfig};
use crate::{shared, worker::ActionContext};
use anyhow::Context;

pub fn convert_to_runj_config(
    ctx: &ActionContext,
    config: ActionRunContainerConfig,
) -> anyhow::Result<runj::RunjConfig> {
    let rootfs = image::get_unpacked_image_path(&config.image).join("rootfs");
    let command = config.command.try_into().context("Error parsing command")?;
    let fd = config.fd.map(|fd| runj::FdConfig {
        stdin: fd.stdin.map(|path| ctx.submission_root.join(path)),
        stdout: fd.stdout.map(|path| ctx.submission_root.join(path)),
        stderr: fd.stderr.map(|path| ctx.submission_root.join(path)),
    });
    let mounts = config
        .mounts
        .into_iter()
        .map(|item| item.into_runj_mount(&ctx.submission_root))
        .collect::<Result<Vec<runj::MountConfig>, _>>()
        .context("Error parsing mount")?;

    Ok(runj::RunjConfig { rootfs, cwd: config.cwd, command, fd, mounts, limits: config.limits })
}

pub async fn check_and_create_directories(config: &runj::RunjConfig) -> anyhow::Result<()> {
    if let Some(config) = &config.fd {
        if let Some(path) = &config.stdin {
            shared::file_utils::create_parent_directories(path).await?;
        }

        if let Some(path) = &config.stdout {
            shared::file_utils::create_parent_directories(path).await?;
        }

        if let Some(path) = &config.stderr {
            shared::file_utils::create_parent_directories(path).await?;
        }
    }

    Ok(())
}