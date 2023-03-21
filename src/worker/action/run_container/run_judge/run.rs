use std::{fmt::Display, fs::Permissions, os::unix::prelude::PermissionsExt, path::PathBuf};

use anyhow::{bail, Context, Result};
use serde::{de, Deserialize, Serialize};
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
    pub files: Vec<MountFile>,
}

#[derive(Debug, Clone)]
pub struct MountFile {
    pub from_path: PathBuf,
    pub to_path: PathBuf,
    pub exec: bool,
}

impl<'de> Deserialize<'de> for MountFile {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let str = String::deserialize(deserializer)?;
        str.as_str().try_into().map_err(|err| de::Error::custom(format!("{err:#}")))
    }
}

impl Serialize for MountFile {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&format!("{self}"))
    }
}

impl TryFrom<&str> for MountFile {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Ok(match value.split(':').collect::<Vec<_>>()[..] {
            [from_path] => {
                Self { from_path: from_path.into(), to_path: from_path.into(), exec: false }
            }
            [from_path, "exec"] => {
                Self { from_path: from_path.into(), to_path: from_path.into(), exec: true }
            }
            [from_path, to_path] => {
                Self { from_path: from_path.into(), to_path: to_path.into(), exec: false }
            }
            [from_path, to_path, "exec"] => {
                Self { from_path: from_path.into(), to_path: to_path.into(), exec: true }
            }
            _ => bail!("Unexpected file item: {value}"),
        })
    }
}

impl Display for MountFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}{}",
            self.from_path.display(),
            self.to_path.display(),
            if self.exec { ":exec" } else { "" }
        )
    }
}

#[instrument(skip_all, name = "action_run_judge_run_execute")]
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

        run_container::execute(handle, ctx, &run_container_config).await
    }
    .await;

    if let Err(err) = fs::remove_dir_all(&mount_directory).await {
        warn!(directory = %mount_directory.display(), "Error removing mount directory: {err:#}")
    }

    result
}
