use std::{fmt::Display, path::PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct RunjError {
    #[serde(rename = "msg")]
    pub message: String,
    pub error: Option<String>,
}

impl Display for RunjError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.error {
            None => write!(f, "{}", self.message),
            Some(error) => write!(f, "{}: {}", self.message, error),
        }
    }
}

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
    pub map_to_user: String,
    pub map_to_group: String,
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

    #[serde(skip_serializing_if = "Option::is_none")]
    pub rlimit: Option<Vec<RlimitConfig>>,
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
    #[serde(rename = "type")]
    pub r#type: RlimitType,

    pub hard: u64,
    pub soft: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum RlimitType {
    #[serde(rename = "RLIMIT_AS")]
    As,

    #[serde(rename = "RLIMIT_CORE")]
    Core,

    #[serde(rename = "RLIMIT_CPU")]
    Cpu,

    #[serde(rename = "RLIMIT_DATA")]
    Data,

    #[serde(rename = "RLIMIT_FSIZE")]
    Fsize,

    #[serde(rename = "RLIMIT_LOCKS")]
    Locks,

    #[serde(rename = "RLIMIT_MEMLOCK")]
    MemLock,

    #[serde(rename = "RLIMIT_MSGQUEUE")]
    MsgQueue,

    #[serde(rename = "RLIMIT_NICE")]
    Nice,

    #[serde(rename = "RLIMIT_NOFILE")]
    NoFile,

    #[serde(rename = "RLIMIT_NPROC")]
    Nproc,

    #[serde(rename = "RLIMIT_RSS")]
    Rss,

    #[serde(rename = "RLIMIT_RTPRIO")]
    RtPrio,

    #[serde(rename = "RLIMIT_RTTIME")]
    RtTime,

    #[serde(rename = "RLIMIT_SIGPENDING")]
    SigPending,

    #[serde(rename = "RLIMIT_STACK")]
    Stack,
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
    TimeLimitExceeded,

    #[serde(rename = "WALL_TIME_LIMIT_EXCEEDED")]
    WallLimitExceeded,

    #[serde(rename = "MEMORY_LIMIT_EXCEEDED")]
    MemoryLimitExceeded,

    #[serde(rename = "OUTPUT_LIMIT_EXCEEDED")]
    OutputLimitExceeded,

    #[serde(rename = "UNKNOWN")]
    Unknown,
}
