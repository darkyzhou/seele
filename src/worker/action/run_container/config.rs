use super::runj;
use crate::shared::oci_image::OciImage;
use anyhow::bail;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ActionRunContainerConfig {
    #[serde(with = "crate::shared::oci_image::serde_format")]
    pub image: OciImage,

    #[serde(default = "default_cwd")]
    pub cwd: PathBuf,

    pub command: CommandConfig,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub fd: Option<runj::FdConfig>,

    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub mounts: Vec<MountConfig>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub limits: Option<runj::LimitsConfig>,
}

#[inline]
fn default_cwd() -> PathBuf {
    "/".into()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum CommandConfig {
    Simple(String),
    Full(Vec<String>),
}

impl TryInto<Vec<String>> for CommandConfig {
    type Error = shell_words::ParseError;

    fn try_into(self) -> Result<Vec<String>, Self::Error> {
        Ok(match self {
            Self::Simple(line) => shell_words::split(&line)?,
            Self::Full(commands) => commands,
        })
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum MountConfig {
    Simple(String),
    Full(runj::MountConfig),
}

impl MountConfig {
    pub fn into_runj_mount(self, parent_path_absolute: &Path) -> anyhow::Result<runj::MountConfig> {
        Ok(match self {
            Self::Simple(config) => {
                let parts: Vec<_> = config.split(':').collect();
                match parts[..] {
                    [item] => runj::MountConfig {
                        from: parent_path_absolute.join(item),
                        to: ["/", item].iter().collect(),
                        options: None,
                    },
                    [from, to] => runj::MountConfig {
                        from: parent_path_absolute.join(from),
                        to: ["/", to].iter().collect(),
                        options: None,
                    },
                    [from, to, options] => runj::MountConfig {
                        from: parent_path_absolute.join(from),
                        to: ["/", to].iter().collect(),
                        options: Some(options.split(',').map(|s| s.to_string()).collect()),
                    },
                    _ => {
                        bail!("Unknown mount value: {}", config)
                    }
                }
            }
            Self::Full(config) => config,
        })
    }
}
