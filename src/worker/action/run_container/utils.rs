use anyhow::{Context, Result};

use super::{image, runj, Config};
use crate::{
    cgroup,
    conf::{self, SeeleWorkMode},
    shared,
    worker::ActionContext,
};

pub fn convert_to_runj_config(ctx: &ActionContext, config: Config) -> Result<runj::RunjConfig> {
    let user_namespace = {
        match &conf::CONFIG.work_mode {
            SeeleWorkMode::Bare | SeeleWorkMode::Systemd | SeeleWorkMode::Containerized => {
                Some(runj::UserNamespaceConfig {
                    enabled: true,
                    map_to_user: todo!(),
                    map_to_group: todo!(),
                })
            }
            SeeleWorkMode::RootlessContainerized => None,
        }
    };

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

    Ok(runj::RunjConfig {
        user_namespace,
        cgroup_path: cgroup::CGROUP_CONTAINER_SLICE_PATH.clone(),
        rootfs,
        cwd: config.cwd,
        command,
        paths: config.paths,
        fd,
        mounts,
        limits: config.limits.into(),
    })
}

pub async fn check_and_create_directories(config: &runj::RunjConfig) -> Result<()> {
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
