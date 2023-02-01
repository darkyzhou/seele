use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RunjConfig {
    pub rootless: bool,

    pub rootfs: PathBuf,

    pub cwd: PathBuf,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub paths: Option<Vec<String>>,

    pub command: Vec<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub fd: Option<FdConfig>,

    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub mounts: Vec<MountConfig>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub limits: Option<LimitsConfig>,
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

    #[serde(skip_serializing_if = "Option::is_none")]
    pub cgroup: Option<CgroupConfig>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub rlimit: Option<Vec<RlimitConfig>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TimeLimitsConfig {
    pub wall: u64,
    pub kernel: u64,
    pub user: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CgroupConfig {
    pub memory: i64,
    pub memory_reservation: i64,
    pub memory_swap: i64,
    pub cpu_shares: u64,
    pub cpu_quota: i64,
    pub cpuset_cpus: i64,
    pub cpuset_mems: i64,
    pub pids_limit: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RlimitConfig {
    #[serde(rename = "type")]
    pub r#type: String,

    pub hard: u64,
    pub soft: u64,
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
    pub is_wall_tle: bool,
    pub is_system_tle: bool,
    pub is_user_tle: bool,
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
    #[serde(rename = "TIME_LIMIT_EXCEEDED")]
    TimeLimitExceeded,
    #[serde(rename = "MEMORY_LIMIT_EXCEEDED")]
    MemoryLimitExceeded,
    #[serde(rename = "OUTPUT_LIMIT_EXCEEDED")]
    OutputLimitExceeded,
    #[serde(rename = "UNKNOWN")]
    Unknown,
}
