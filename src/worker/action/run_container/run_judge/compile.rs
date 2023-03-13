use std::{fs::Permissions, os::unix::prelude::PermissionsExt, path::PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use tokio::fs;
use tracing::{instrument, warn};
use triggered::Listener;

use super::DEFAULT_MOUNT_DIRECTORY;
use crate::{
    conf,
    entities::ActionReportExt,
    worker::{
        run_container::{self, runj},
        ActionContext,
    },
};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    #[serde(flatten)]
    pub run_container_config: run_container::Config,

    #[serde(default)]
    pub sources: Vec<PathBuf>,

    #[serde(default)]
    pub saves: Vec<PathBuf>,

    #[serde(default)]
    pub cache: Vec<CacheItem>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum CacheItem {
    String(String),
    File { file: String },
}

#[instrument(skip_all, name = "action_run_judge_compile_execute")]
pub async fn execute(
    handle: Listener,
    ctx: &ActionContext,
    config: &Config,
) -> Result<ActionReportExt> {
    let mount_directory = conf::PATHS.new_temp_directory().await?;
    // XXX: 0o777 is mandatory. The group bit is for rootless case and the others
    // bit is for rootful case.
    fs::set_permissions(&mount_directory, Permissions::from_mode(0o777)).await?;

    let result = async {
        let run_container_config = {
            let mut run_container_config = config.run_container_config.clone();
            run_container_config.cwd = DEFAULT_MOUNT_DIRECTORY.to_owned();

            run_container_config.mounts.push(run_container::MountConfig::Full(runj::MountConfig {
                from: mount_directory.clone(),
                to: DEFAULT_MOUNT_DIRECTORY.to_owned(),
                options: None,
            }));

            run_container_config.mounts.extend(
                config
                    .sources
                    .iter()
                    .map(|file| runj::MountConfig {
                        from: ctx.submission_root.join(file),
                        to: DEFAULT_MOUNT_DIRECTORY.join(file),
                        options: None,
                    })
                    .map(run_container::MountConfig::Full),
            );

            run_container_config
        };

        let report = run_container::execute(handle, ctx, &run_container_config).await?;

        for file in &config.saves {
            let source = mount_directory.join(file);
            let target = ctx.submission_root.join(file);
            let metadata = fs::metadata(&source)
                .await
                .with_context(|| format!("The file {} to save does not exist", file.display()))?;

            if metadata.is_file() {
                fs::copy(source, target).await.context("Error copying the file")?;
                continue;
            } else if metadata.is_dir() {
                bail!("Saving a directory is not supported: {}", file.display());
            } else if metadata.is_symlink() {
                bail!("Saving a symlink is not supported: {}", file.display());
            }

            bail!("Unknown file type: {}", file.display());
        }

        Ok(report)
    }
    .await;

    if let Err(err) = fs::remove_dir_all(&mount_directory).await {
        warn!(directory = %mount_directory.display(), "Error removing mount directory: {:#}", err)
    }

    result
}
