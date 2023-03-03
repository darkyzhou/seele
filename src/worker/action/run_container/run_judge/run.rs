use std::{fmt::Display, fs::Permissions, os::unix::prelude::PermissionsExt, path::PathBuf};

use anyhow::{bail, Context, Result};
use serde::{de, Deserialize, Serialize};
use tokio::fs;
use tracing::instrument;
use triggered::Listener;

use super::MOUNT_DIRECTORY;
use crate::{
    conf,
    entities::ActionSuccessReportExt,
    worker::{
        run_container::{
            self,
            runj::{self},
        },
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
    pub name: String,
    pub exec: bool,
}

impl Display for MountFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl TryFrom<&str> for MountFile {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Ok(match value.split_once(':') {
            None => Self { name: value.to_owned(), exec: false },
            Some((name, options)) => {
                if options != "exec" {
                    bail!("Unexpected file option: {options}");
                }
                Self { name: name.to_owned(), exec: true }
            }
        })
    }
}

impl<'de> Deserialize<'de> for MountFile {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let str = String::deserialize(deserializer)?;
        str.as_str().try_into().map_err(|err| de::Error::custom(format!("{err:#}")))
    }
}

impl Serialize for MountFile {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if self.exec {
            serializer.serialize_str(&format!("{}:exec", self.name))
        } else {
            serializer.serialize_str(&self.name)
        }
    }
}

#[instrument(skip_all, name = "action_run_judge_run_execute")]
pub async fn execute(
    handle: Listener,
    ctx: &ActionContext,
    config: &Config,
) -> Result<ActionSuccessReportExt> {
    if config.files.is_empty() {
        bail!("Unexpected empty `files`");
    }

    let run_container_config = {
        let mut run_container_config = config.run_container_config.clone();

        run_container_config.cwd = MOUNT_DIRECTORY.into();

        if let Some(paths) = run_container_config.paths.as_mut() {
            paths.push(MOUNT_DIRECTORY.to_string());
        } else {
            run_container_config.paths = Some(vec![MOUNT_DIRECTORY.to_string()]);
        }

        for file in &config.files {
            let from_path = ctx.submission_root.join(&file.name);
            let to_path = [MOUNT_DIRECTORY, &file.name].iter().collect::<PathBuf>();

            if let Err(err) = fs::metadata(&from_path).await {
                bail!("The file {file} does not exist: {err:#}");
            }

            run_container_config.mounts.push(run_container::MountConfig::Full({
                if !file.exec {
                    runj::MountConfig { from: from_path, to: to_path, options: None }
                } else {
                    fs::set_permissions(&from_path, Permissions::from_mode(0o777))
                        .await
                        .with_context(|| {
                            format!("Error setting the permission of the executable {file}")
                        })?;

                    if !conf::CONFIG.worker.action.run_container.tmp_noexec {
                        runj::MountConfig {
                            from: from_path,
                            to: to_path,
                            options: Some(vec!["exec".to_owned()]),
                        }
                    } else {
                        let new_from_path =
                            conf::PATHS.new_temp_directory().await?.join(&file.name);
                        fs::copy(&from_path, &new_from_path)
                            .await
                            .with_context(|| format!("Error copying the file: {file}"))?;
                        runj::MountConfig {
                            from: new_from_path,
                            to: to_path,
                            options: Some(vec!["exec".to_owned()]),
                        }
                    }
                }
            }));
        }

        run_container_config
    };

    run_container::execute(handle, ctx, &run_container_config).await
}
