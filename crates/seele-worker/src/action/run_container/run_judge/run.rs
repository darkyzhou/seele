use std::{fs::Permissions, os::unix::prelude::PermissionsExt};

use anyhow::{Context, Result, bail};
use seele_shared::entities::{
    ActionReportExt,
    run_container::{self, run_judge::run::Config, runj},
};
use tokio::fs;
use tracing::{instrument, warn};
use triggered::Listener;

use super::DEFAULT_MOUNT_DIRECTORY;
use crate::ActionContext;

#[instrument(skip_all, name = "action_run_judge_run_execute")]
pub async fn execute(
    handle: Listener,
    ctx: &ActionContext,
    config: &Config,
) -> Result<ActionReportExt> {
    let mount_directory = crate::conf::PATHS.new_temp_directory().await?;
    // XXX: 0o777 is mandatory. The group bit is for rootless case and the others
    // bit is for rootful case.
    fs::set_permissions(&mount_directory, Permissions::from_mode(0o777)).await?;

    let result = async {
        let mut run_container_config = config.run_container_config.clone();

        run_container_config.cwd = DEFAULT_MOUNT_DIRECTORY.to_owned();

        run_container_config.mounts.push(run_container::MountConfig::Full(runj::MountConfig {
            from: mount_directory.clone(),
            to: DEFAULT_MOUNT_DIRECTORY.to_owned(),
            options: None,
        }));

        if let Some(paths) = run_container_config.paths.as_mut() {
            paths.push(DEFAULT_MOUNT_DIRECTORY.to_owned());
        } else {
            run_container_config.paths = Some(vec![DEFAULT_MOUNT_DIRECTORY.to_owned()]);
        }

        for file in &config.files {
            let from_path = ctx.submission_root.join(&file.from_path);

            if let Err(err) = fs::metadata(&from_path).await {
                bail!("The file {file} does not exist: {err:#}");
            }

            run_container_config.mounts.push(run_container::MountConfig::Full({
                if file.exec {
                    fs::set_permissions(&from_path, Permissions::from_mode(0o777))
                        .await
                        .with_context(|| {
                            format!("Error setting the permission of the executable {file}")
                        })?;
                }

                let to_path = DEFAULT_MOUNT_DIRECTORY.join(&file.to_path);

                let options = if file.exec { Some(vec!["exec".to_owned()]) } else { None };

                runj::MountConfig { from: from_path, to: to_path, options }
            }));
        }

        crate::run_container::execute(handle, ctx, &run_container_config).await
    }
    .await;

    if let Err(err) = fs::remove_dir_all(&mount_directory).await {
        warn!(directory = %mount_directory.display(), "Error removing mount directory: {err:#}")
    }

    result
}
