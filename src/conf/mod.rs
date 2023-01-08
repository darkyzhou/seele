use once_cell::sync::Lazy;
use serde::Deserialize;
use std::path::PathBuf;

pub use action::*;
pub use exchange::*;

mod action;
mod exchange;

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

#[derive(Debug)]
pub struct SeelePaths {
    pub root: PathBuf,
    pub images: PathBuf,
    pub http_cache: PathBuf,
    pub downloads: PathBuf,
    pub submissions: PathBuf,
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

pub static PATHS: Lazy<SeelePaths> = Lazy::new(|| SeelePaths {
    root: CONFIG.root_path.clone(),
    images: CONFIG.root_path.join("images"),
    http_cache: CONFIG.root_path.join("http_cache"),
    downloads: CONFIG.root_path.join("downloads"),
    submissions: CONFIG.root_path.join("submissions"),
});
