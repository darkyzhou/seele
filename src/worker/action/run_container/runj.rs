use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RunjConfig {
    pub rootfs: PathBuf,

    pub cwd: PathBuf,

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
    time: Option<TimeLimitsConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cgroup: Option<CgroupConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rlimit: Option<Vec<RlimitConfig>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TimeLimitsConfig {
    wall: u64,
    kernel: u64,
    user: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CgroupConfig {
    memory: i64,
    memory_reservation: i64,
    memory_swap: i64,
    cpu_shares: u64,
    cpu_quota: i64,
    cpuset_cpus: i64,
    cpuset_mems: i64,
    pids_limit: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RlimitConfig {
    r#type: String,
    hard: String,
    soft: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ContainerExecutionReport {
    reason: String,
    exit_code: i64,
    wall_time_ms: u64,
    cpu_user_time_ms: u64,
    cpu_kernel_time_ms: u64,
    memory_usage_kib: u64,
    is_oom: bool,
    is_wall_tle: bool,
    is_system_tle: bool,
    is_user_tle: bool,
}
