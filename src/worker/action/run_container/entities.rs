use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

use super::runj;
use crate::shared::oci_image::OciImage;

pub type ExecutionReport = runj::ContainerExecutionReport;
pub type ExecutionStatus = runj::ContainerExecutionStatus;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    #[serde(with = "crate::shared::oci_image::serde_format")]
    pub image: OciImage,

    #[serde(default = "default_cwd")]
    pub cwd: PathBuf,

    pub command: CommandConfig,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub fd: Option<runj::FdConfig>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub paths: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub mounts: Vec<MountConfig>,

    #[serde(default)]
    pub limits: LimitsConfig,
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
    pub fn into_runj_mount(self, parent_path_absolute: &Path) -> Result<runj::MountConfig> {
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
                    _ => bail!("Unknown mount value: {}", config),
                }
            }
            Self::Full(config) => config,
        })
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct LimitsConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<runj::TimeLimitsConfig>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_kib: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub pids_count: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub fsize_kib: Option<u64>,
}

impl Into<runj::LimitsConfig> for LimitsConfig {
    fn into(self) -> runj::LimitsConfig {
        runj::LimitsConfig {
            time: self.time,
            cgroup: runj::CgroupConfig {
                memory: self.memory_kib.map(|memory_kib| memory_kib * 1024),
                memory_swap: self.memory_kib.map(|memory_kib| memory_kib * 1024),
                pids_limit: self.pids_count,
                ..Default::default()
            },
            rlimit: self.fsize_kib.map(|fsize_kib| {
                vec![runj::RlimitConfig {
                    r#type: runj::RlimitType::Fsize,
                    hard: fsize_kib,
                    soft: fsize_kib,
                }]
            }),
        }
    }
}
