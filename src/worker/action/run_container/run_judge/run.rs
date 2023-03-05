use std::{fmt::Display, fs::Permissions, os::unix::prelude::PermissionsExt, path::PathBuf};

use anyhow::{bail, Context, Result};
use serde::{de, Deserialize, Serialize};
use tokio::fs;
use tracing::instrument;
use triggered::Listener;

use super::MOUNT_DIRECTORY;
use crate::{
    entities::ActionSuccessReportExt,
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
    pub name: String,
    pub rename: Option<String>,
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
        Ok(match value.split(':').collect::<Vec<_>>()[..] {
            [name] => Self { name: name.to_owned(), rename: None, exec: false },
            [name, "exec"] => Self { name: name.to_owned(), rename: None, exec: true },
            [name, rename] => {
                Self { name: name.to_owned(), rename: Some(rename.to_owned()), exec: false }
            }
            [name, rename, "exec"] => {
                Self { name: name.to_owned(), rename: Some(rename.to_owned()), exec: true }
            }
            _ => bail!("Unexpected file item: {value}"),
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

                let to_path = [
                    MOUNT_DIRECTORY,
                    match &file.rename {
                        Some(name) => name,
                        None => &file.name,
                    },
                ]
                .iter()
                .collect::<PathBuf>();

                let options = if file.exec { Some(vec!["exec".to_owned()]) } else { None };

                runj::MountConfig { from: from_path, to: to_path, options }
            }));
        }

        run_container_config
    };

    run_container::execute(handle, ctx, &run_container_config).await
}
