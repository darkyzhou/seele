pub use exchange::*;

use once_cell::sync::Lazy;
use serde::Deserialize;

mod exchange;

#[derive(Debug, Deserialize)]
pub struct SeeleConfig {
    #[serde(default = "default_root_path")]
    pub root_path: String,

    #[serde(default = "default_concurrency")]
    pub concurrency: usize,

    #[serde(default)]
    pub exchange: Vec<ExchangeConfig>,
}

#[inline]
fn default_root_path() -> String {
    "/seele".into()
}

#[inline]
fn default_concurrency() -> usize {
    // TODO: infer from cpu core numbers
    4
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
