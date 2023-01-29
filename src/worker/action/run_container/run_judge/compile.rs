use super::{config::ActionCompileConfig, MOUNT_DIRECTORY};
use crate::{
    conf,
    entities::ActionExecutionReport,
    worker::{run_container, runj, ActionContext, MountConfig},
};
use anyhow::{bail, Context};
use std::{fs::Permissions, os::unix::prelude::PermissionsExt, path::PathBuf};
use tokio::fs;
use tracing::{instrument, warn};

#[instrument]
pub async fn compile(
    ctx: &ActionContext,
    config: &ActionCompileConfig,
) -> anyhow::Result<ActionExecutionReport> {
    let mount_directory = conf::PATHS.temp_mounts.join(nano_id::base62::<8>());
    fs::create_dir(&mount_directory).await?;

    let result = async {
        // XXX: 0o777 is mandatory. The group bit is for rootless case and the others bit is for rootful case.
        fs::set_permissions(&mount_directory, Permissions::from_mode(0o777)).await?;

        let run_container_config = {
            let mut run_container_config = config.run_container_config.clone();
            run_container_config.cwd = PathBuf::from(MOUNT_DIRECTORY);

            run_container_config.mounts.push(MountConfig::Full(runj::MountConfig {
                from: mount_directory.clone(),
                to: PathBuf::from(MOUNT_DIRECTORY),
                options: Some(vec!["rw".to_string()]),
            }));

            run_container_config.mounts.extend(
                config
                    .source
                    .iter()
                    .map(|file| runj::MountConfig {
                        from: ctx.submission_root.join(file),
                        to: [MOUNT_DIRECTORY, file].iter().collect(),
                        options: None,
                    })
                    .map(MountConfig::Full),
            );

            run_container_config
        };

        let report = run_container(ctx, &run_container_config).await?;

        for file in &config.save {
            let source = mount_directory.join(file);
            let target = ctx.submission_root.join(file);
            let metadata = fs::metadata(&source)
                .await
                .with_context(|| format!("The file `{file}` to save does not exist"))?;

            if metadata.is_file() {
                fs::copy(source, target).await.context("Error copying the file")?;
                continue;
            } else if metadata.is_dir() {
                bail!("Saving a directory is currently unsupported: {}", file);
            } else if metadata.is_symlink() {
                bail!("Saving a symlink is currently unsupported: {}", file);
            }
            bail!("Unknown file type: {}", file);
        }

        Ok(report)
    }
    .await;

    if let Err(err) = fs::remove_dir_all(&mount_directory).await {
        warn!(directory = %mount_directory.display(), "Error removing mount directory: {:#}", err)
    }

    result
}
