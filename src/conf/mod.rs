use std::path::PathBuf;

pub use action::*;
pub use exchange::*;
use once_cell::sync::Lazy;
pub use path::*;
use serde::Deserialize;
pub use worker::*;

use self::worker::WorkerConfig;

mod action;
mod exchange;
mod path;
mod worker;

#[derive(Debug, Deserialize)]
pub struct SeeleConfig {
    #[serde(default = "default_work_mode")]
    pub work_mode: SeeleWorkMode,

    #[serde(default)]
    pub thread_counts: ThreadCounts,

    #[serde(default = "default_root_path")]
    pub root_path: PathBuf,

    #[serde(default = "default_tmp_path")]
    pub tmp_path: PathBuf,

    #[serde(default = "default_runj_path")]
    pub runj_path: String,

    #[serde(default = "default_skopeo_path")]
    pub skopeo_path: String,

    #[serde(default = "default_umoci_path")]
    pub umoci_path: String,

    #[serde(default)]
    pub exchange: Vec<ExchangeConfig>,

    #[serde(default)]
    pub worker: WorkerConfig,
}

// TODO: move to cli argument
#[derive(Debug, Deserialize)]
pub enum SeeleWorkMode {
    #[serde(rename = "bare")]
    Bare,

    #[serde(rename = "systemd")]
    Systemd,

    #[serde(rename = "containerized")]
    Containerized,

    #[serde(rename = "rootless_containerized")]
    RootlessContainerized,
}

#[derive(Debug, Deserialize)]
pub struct ThreadCounts {
    pub runtime: usize,
    pub worker: usize,
}

impl Default for ThreadCounts {
    fn default() -> Self {
        Self { runtime: 2, worker: num_cpus::get() - 2 }
    }
}

#[inline]
fn default_work_mode() -> SeeleWorkMode {
    SeeleWorkMode::Bare
}

#[inline]
fn default_root_path() -> PathBuf {
    "/seele".into()
}

#[inline]
fn default_tmp_path() -> PathBuf {
    "/tmp".into()
}

#[inline]
fn default_runj_path() -> String {
    "runj".to_string()
}

#[inline]
fn default_skopeo_path() -> String {
    "skopeo".to_string()
}

#[inline]
fn default_umoci_path() -> String {
    "umoci".to_string()
}

pub static CONFIG: Lazy<SeeleConfig> = Lazy::new(|| {
    config::Config::builder()
        .add_source(config::File::with_name("config"))
        .add_source(config::Environment::with_prefix("SEELE"))
        .build()
        .expect("Failed to load the config")
        .try_deserialize()
        .expect("Failed to parse the config")
});
