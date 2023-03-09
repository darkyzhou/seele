use std::path::PathBuf;

use indexmap::IndexMap;
use once_cell::sync::Lazy;
use serde::Deserialize;
use tracing_subscriber::filter::LevelFilter;

pub use self::{action::*, env::*, exchange::*, path::*, worker::*};
use self::{composer::ComposerConfig, healthz::HealthzConfig, worker::WorkerConfig};

mod action;
mod composer;
mod env;
mod exchange;
mod healthz;
mod path;
mod worker;

pub static CONFIG: Lazy<SeeleConfig> = Lazy::new(|| {
    config::Config::builder()
        .add_source(config::File::with_name("config"))
        .add_source(config::Environment::with_prefix("SEELE"))
        .build()
        .expect("Failed to load the config")
        .try_deserialize()
        .expect("Failed to parse the config")
});

#[derive(Debug, Deserialize)]
pub struct SeeleConfig {
    #[serde(default)]
    pub log_level: LogLevel,

    #[serde(default = "default_work_mode")]
    pub work_mode: SeeleWorkMode,

    #[serde(default)]
    pub thread_counts: ThreadCounts,

    #[serde(default)]
    pub paths: PathsConfig,

    #[serde(default)]
    pub telemetry: Option<TelemetryConfig>,

    #[serde(default)]
    pub healthz: HealthzConfig,

    #[serde(default)]
    pub exchange: IndexMap<String, ExchangeConfig>,

    #[serde(default)]
    pub composer: ComposerConfig,

    #[serde(default)]
    pub worker: WorkerConfig,
}

#[derive(Debug, Copy, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
    Off,
}

impl Default for LogLevel {
    fn default() -> Self {
        Self::Warn
    }
}

impl Into<LevelFilter> for LogLevel {
    fn into(self) -> LevelFilter {
        match self {
            Self::Debug => LevelFilter::DEBUG,
            Self::Info => LevelFilter::INFO,
            Self::Warn => LevelFilter::WARN,
            Self::Error => LevelFilter::ERROR,
            Self::Off => LevelFilter::OFF,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeeleWorkMode {
    Bare,
    BareSystemd,
    Containerized,
    RootlessContainerized,
}

#[inline]
fn default_work_mode() -> SeeleWorkMode {
    SeeleWorkMode::Containerized
}

#[derive(Debug, Deserialize)]
pub struct ThreadCounts {
    #[serde(default = "default_worker_thread_count")]
    pub worker: usize,

    #[serde(default = "default_runner_thread_count")]
    pub runner: usize,
}

impl Default for ThreadCounts {
    fn default() -> Self {
        Self { worker: default_worker_thread_count(), runner: default_runner_thread_count() }
    }
}

#[inline]
fn default_worker_thread_count() -> usize {
    1
}

#[inline]
fn default_runner_thread_count() -> usize {
    num_cpus::get() - 2
}

#[derive(Debug, Deserialize)]
pub struct PathsConfig {
    #[serde(default = "default_root_path")]
    pub root: PathBuf,

    #[serde(default = "default_tmp_path")]
    pub tmp: PathBuf,

    #[serde(default = "default_runj_path")]
    pub runj: String,

    #[serde(default = "default_skopeo_path")]
    pub skopeo: String,

    #[serde(default = "default_umoci_path")]
    pub umoci: String,
}

impl Default for PathsConfig {
    fn default() -> Self {
        Self {
            root: default_root_path(),
            tmp: default_tmp_path(),
            runj: default_runj_path(),
            skopeo: default_skopeo_path(),
            umoci: default_umoci_path(),
        }
    }
}

#[inline]
fn default_root_path() -> PathBuf {
    "/etc/seele".into()
}

#[inline]
fn default_tmp_path() -> PathBuf {
    "/tmp".into()
}

#[inline]
fn default_runj_path() -> String {
    "/usr/local/bin/runj".to_string()
}

#[inline]
fn default_skopeo_path() -> String {
    "/usr/bin/skopeo".to_string()
}

#[inline]
fn default_umoci_path() -> String {
    "/usr/bin/umoci".to_string()
}

#[derive(Debug, Deserialize)]
pub struct TelemetryConfig {
    pub collector_url: String,
}
