use std::{
    fmt::Display,
    path::{Path, PathBuf},
};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

pub mod run_judge;
pub mod runj;

use runj::RlimitItem;
use seele_config::OciImage;

pub type ExecutionReport = runj::ContainerExecutionReport;
pub type ExecutionStatus = runj::ContainerExecutionStatus;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub image: OciImage,

    #[serde(default = "default_cwd")]
    pub cwd: PathBuf,

    pub command: CommandConfig,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub fd: Option<runj::FdConfig>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub paths: Option<Vec<PathBuf>>,

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

impl Display for CommandConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Simple(str) => write!(f, "{str}"),
            Self::Full(commands) => write!(f, "{}", commands.join(" ")),
        }
    }
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
    pub time_ms: Option<u64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_kib: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub pids_count: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub fsize_kib: Option<u64>,
}

impl From<LimitsConfig> for runj::LimitsConfig {
    fn from(val: LimitsConfig) -> Self {
        const DEFAULT_TIME_MS: u64 = 10 * 1000; // 10 seconds
        const DEFAULT_MEMORY_LIMIT_BYTES: i64 = 256 * 1024 * 1024; // 256 MiB
        const DEFAULT_PIDS_LIMIT: i64 = 32;
        const DEFAULT_CORE: u64 = 0; // Disable core dump
        const DEFAULT_NO_FILE: u64 = 64;
        const DEFAULT_FSIZE_BYTES: u64 = 64 * 1024 * 1024; // 64 MiB

        runj::LimitsConfig {
            time_ms: val.time_ms.unwrap_or(DEFAULT_TIME_MS),
            cgroup: runj::CgroupConfig {
                memory: val
                    .memory_kib
                    .map(|memory_kib| memory_kib * 1024)
                    .unwrap_or(DEFAULT_MEMORY_LIMIT_BYTES),
                pids_limit: val.pids_count.unwrap_or(DEFAULT_PIDS_LIMIT),
                ..Default::default()
            },
            rlimit: runj::RlimitConfig {
                core: RlimitItem::new_single(DEFAULT_CORE),
                no_file: RlimitItem::new_single(DEFAULT_NO_FILE),
                fsize: RlimitItem::new_single(
                    val.fsize_kib.map(|fsize_kib| fsize_kib * 1024).unwrap_or(DEFAULT_FSIZE_BYTES),
                ),
            },
        }
    }
}
