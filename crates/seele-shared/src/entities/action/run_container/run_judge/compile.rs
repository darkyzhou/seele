use serde::{Deserialize, Serialize};

use super::MountFile;
use crate::entities::run_container;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    #[serde(flatten)]
    pub run_container_config: run_container::Config,

    #[serde(default)]
    pub sources: Vec<MountFile>,

    #[serde(default)]
    pub saves: Vec<String>,

    #[serde(default)]
    pub cache: CacheConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CacheConfig {
    pub enabled: bool,

    #[serde(default = "default_max_allowed_size_mib")]
    pub max_allowed_size_mib: u64,

    #[serde(default)]
    pub extra: Vec<String>,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_allowed_size_mib: default_max_allowed_size_mib(),
            extra: Default::default(),
        }
    }
}

#[inline]
fn default_max_allowed_size_mib() -> u64 {
    seele_config::CONFIG.worker.action.run_container.cache_size_mib / 16
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ExecutionReport {
    CacheHit { cache_hit: bool },
    CacheMiss(run_container::ExecutionReport),
}
