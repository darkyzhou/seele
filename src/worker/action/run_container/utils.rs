use std::{fs::Permissions, os::unix::prelude::PermissionsExt};

use anyhow::{Context, Result};
use nix::mount;
use tokio::fs;

use super::{
    idmap, image,
    runj::{self, OverlayfsConfig},
    Config,
};
use crate::{
    cgroup,
    conf::{self, SeeleWorkMode},
    shared,
    worker::ActionContext,
};

pub async fn make_runj_config(ctx: &ActionContext, config: Config) -> Result<runj::RunjConfig> {
    let user_namespace = {
        match &conf::CONFIG.work_mode {
            SeeleWorkMode::Bare | SeeleWorkMode::BareSystemd | SeeleWorkMode::Containerized => {
                let config = &conf::CONFIG.worker.action.run_container;
                Some(runj::UserNamespaceConfig {
                    enabled: true,
                    root_uid: config.userns_uid,
                    uid_map_begin: idmap::SUBUIDS.begin,
                    uid_map_count: idmap::SUBUIDS.count,
                    root_gid: config.userns_gid,
                    gid_map_begin: idmap::SUBGIDS.begin,
                    gid_map_count: idmap::SUBGIDS.count,
                })
            }
            SeeleWorkMode::RootlessContainerized => None,
        }
    };

    let overlayfs = {
        let id = nano_id::base62::<8>();

        let lower_dir = image::get_unpacked_image_path(&config.image).join("rootfs");
        let upper_dir = conf::PATHS.temp.join(format!("__run_container_upper_{id}"));
        let work_dir = conf::PATHS.temp.join(format!("__run_container_work_{id}"));
        let merged_dir = conf::PATHS.temp.join(format!("__run_container_merged_{id}"));

        fs::create_dir(&upper_dir).await?;
        fs::create_dir(&work_dir).await?;
        fs::create_dir(&merged_dir).await?;

        // TODO: In bare work mode, others bits are not needed
        fs::set_permissions(&upper_dir, Permissions::from_mode(0o777)).await?;
        fs::set_permissions(&work_dir, Permissions::from_mode(0o777)).await?;
        fs::set_permissions(&merged_dir, Permissions::from_mode(0o777)).await?;

        runj::OverlayfsConfig { lower_dir, upper_dir, work_dir, merged_dir }
    };

    let command = config.command.try_into().context("Error parsing command")?;

    let fd = config.fd.map(|fd| runj::FdConfig {
        stdin: fd.stdin.map(|path| ctx.submission_root.join(path)),
        stdout: fd.stdout.map(|path| ctx.submission_root.join(path)),
        stderr: fd.stderr.map(|path| ctx.submission_root.join(path)),
        ..fd
    });

    let mounts = config
        .mounts
        .into_iter()
        .map(|item| item.into_runj_mount(&ctx.submission_root))
        .collect::<Result<Vec<runj::MountConfig>, _>>()
        .context("Error parsing mount")?;

    Ok(runj::RunjConfig {
        user_namespace,
        overlayfs,
        cgroup_path: cgroup::CGROUP_CONTAINER_SLICE_PATH.clone(),
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
            shared::file::create_parent_directories(path).await?;
        }

        if let Some(path) = &config.stdout {
            shared::file::create_parent_directories(path).await?;
        }

        if let Some(path) = &config.stderr {
            shared::file::create_parent_directories(path).await?;
        }
    }

    Ok(())
}

pub fn cleanup_overlayfs(config: &OverlayfsConfig) -> Result<()> {
    _ = mount::umount(&config.merged_dir);

    use std::fs;
    fs::remove_dir_all(&config.merged_dir).context("Error removing merged directory")?;
    fs::remove_dir_all(&config.work_dir).context("Error removing work directory")?;
    fs::remove_dir_all(&config.upper_dir).context("Error removing upper directory")?;
    Ok(())
}
