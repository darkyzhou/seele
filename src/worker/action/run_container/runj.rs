use std::{fmt::Display, path::PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RunjConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_namespace: Option<UserNamespaceConfig>,

    pub cgroup_path: PathBuf,

    pub rootfs: PathBuf,

    pub cwd: PathBuf,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub paths: Option<Vec<String>>,

    pub command: Vec<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub fd: Option<FdConfig>,

    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub mounts: Vec<MountConfig>,

    pub limits: LimitsConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UserNamespaceConfig {
    pub enabled: bool,
    pub root_uid: u32,
    pub uid_map_begin: u32,
    pub uid_map_count: u32,
    pub root_gid: u32,
    pub gid_map_begin: u32,
    pub gid_map_count: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FdConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdin: Option<PathBuf>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout: Option<PathBuf>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub stderr: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MountConfig {
    pub from: PathBuf,
    pub to: PathBuf,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LimitsConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<TimeLimitsConfig>,

    #[serde(default)]
    pub cgroup: CgroupConfig,

    #[serde(default)]
    pub rlimit: RlimitConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TimeLimitsConfig {
    pub wall: u64,
    pub kernel: u64,
    pub user: u64,
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct CgroupConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_reservation: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_swap: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_shares: Option<u64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_quota: Option<i64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpuset_cpus: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpuset_mems: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub pids_limit: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RlimitConfig {
    #[serde(default = "default_rlimit_core")]
    pub core: RlimitItem,

    #[serde(default = "default_rlimit_fsize")]
    pub fsize: RlimitItem,

    #[serde(default = "default_rlimit_no_file")]
    pub no_file: RlimitItem,
}

impl Default for RlimitConfig {
    fn default() -> Self {
        Self {
            core: default_rlimit_core(),
            fsize: default_rlimit_fsize(),
            no_file: default_rlimit_no_file(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RlimitItem {
    hard: u64,
    soft: u64,
}

impl RlimitItem {
    #[inline]
    pub fn new_single(value: u64) -> Self {
        Self { hard: value, soft: value }
    }
}

#[inline]
fn default_rlimit_core() -> RlimitItem {
    RlimitItem { hard: 0, soft: 0 }
}

#[inline]
fn default_rlimit_fsize() -> RlimitItem {
    RlimitItem { hard: 64 * 1024 * 1024, soft: 64 * 1024 * 1024 }
}

#[inline]
fn default_rlimit_no_file() -> RlimitItem {
    RlimitItem { hard: 128, soft: 128 }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ContainerExecutionReport {
    pub status: ContainerExecutionStatus,
    pub exit_code: i64,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal: Option<String>,

    pub wall_time_ms: u64,
    pub cpu_user_time_ms: u64,
    pub cpu_kernel_time_ms: u64,
    pub memory_usage_kib: u64,
    pub is_oom: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum ContainerExecutionStatus {
    #[serde(rename = "NORMAL")]
    Normal,

    #[serde(rename = "RUNTIME_ERROR")]
    RuntimeError,

    #[serde(rename = "SIGNAL_TERMINATE")]
    SignalTerminate,

    #[serde(rename = "SIGNAL_STOP")]
    SignalStop,

    #[serde(rename = "USER_TIME_LIMIT_EXCEEDED")]
    UserTimeLimitExceeded,

    #[serde(rename = "WALL_TIME_LIMIT_EXCEEDED")]
    WallTimeLimitExceeded,

    #[serde(rename = "MEMORY_LIMIT_EXCEEDED")]
    MemoryLimitExceeded,

    #[serde(rename = "OUTPUT_LIMIT_EXCEEDED")]
    OutputLimitExceeded,

    #[serde(rename = "UNKNOWN")]
    Unknown,
}

impl Display for ContainerExecutionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Normal => "NORMAL",
                Self::RuntimeError => "RUNTIME_ERROR",
                Self::SignalTerminate => "SIGNAL_TERMINATE",
                Self::SignalStop => "SIGNAL_STOP",
                Self::UserTimeLimitExceeded => "USER_TIME_LIMIT_EXCEEDED",
                Self::WallTimeLimitExceeded => "WALL_TIME_LIMIT_EXCEEDED",
                Self::MemoryLimitExceeded => "MEMORY_LIMIT_EXCEEDED",
                Self::OutputLimitExceeded => "OUTPUT_LIMIT_EXCEEDED",
                Self::Unknown => "UNKNOWN",
            }
        )
    }
}
