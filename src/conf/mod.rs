use once_cell::sync::Lazy;
use serde::Deserialize;
use std::path::PathBuf;

pub use action::*;
pub use exchange::*;
pub use path::*;
pub use worker::*;

use self::worker::WorkerConfig;

mod action;
mod exchange;
mod path;
mod worker;

#[derive(Debug, Deserialize)]
pub struct SeeleConfig {
    #[serde(default = "default_root_path")]
    pub root_path: PathBuf,

    #[serde(default = "default_concurrency")]
    pub concurrency: usize,

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

#[inline]
fn default_root_path() -> PathBuf {
    "/seele".into()
}

#[inline]
fn default_concurrency() -> usize {
    // TODO: infer from cpu core numbers
    4
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
