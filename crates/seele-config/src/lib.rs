use std::{path::PathBuf, sync::LazyLock};

use indexmap::IndexMap;
use serde::Deserialize;
use tracing_subscriber::filter::LevelFilter;

pub use self::{action::*, env::*, exchange::*, image::*, path::*};
use self::{
    composer::ComposerConfig, healthz::HealthzConfig, http::HttpConfig, telemetry::TelemetryConfig,
    worker::WorkerConfig,
};

mod action;
mod composer;
pub mod env;
mod exchange;
mod healthz;
mod http;
mod image;
mod path;
mod telemetry;
mod worker;

pub static CONFIG: LazyLock<SeeleConfig> = LazyLock::new(|| {
    match config::Config::builder()
        .add_source(config::File::with_name("config"))
        .add_source(config::Environment::with_prefix("SEELE"))
        .build()
    {
        Ok(config) => config.try_deserialize().expect("Failed to parse the config"),
        Err(err) => {
            tracing::warn!("Failed to load the config, fallback to default: {}", err);
            SeeleConfig::default()
        }
    }
});

#[derive(Default, Debug, Deserialize)]
pub struct SeeleConfig {
    #[serde(default)]
    pub log_level: LogLevel,

    #[serde(default)]
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
    pub http: HttpConfig,

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
    Trace,
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

impl From<LogLevel> for LevelFilter {
    fn from(val: LogLevel) -> Self {
        match val {
            LogLevel::Trace => LevelFilter::TRACE,
            LogLevel::Debug => LevelFilter::DEBUG,
            LogLevel::Info => LevelFilter::INFO,
            LogLevel::Warn => LevelFilter::WARN,
            LogLevel::Error => LevelFilter::ERROR,
            LogLevel::Off => LevelFilter::OFF,
        }
    }
}

#[derive(Default, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeeleWorkMode {
    Bare,
    BareSystemd,
    #[default]
    Containerized,
    RootlessContainerized,
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
const fn default_worker_thread_count() -> usize {
    1
}

#[inline]
fn default_runner_thread_count() -> usize {
    let count = num_cpus::get();
    if count <= 2 {
        panic!("There are too few cpu cores on your system, seele requires at least 3 cpu cores");
    }

    count - 2
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
