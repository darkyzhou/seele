use super::ActionConfig;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct WorkerConfig {
    #[serde(default)]
    pub action: ActionConfig,

    #[serde(default = "default_image_eviction_config")]
    pub image_eviction: Option<EvictionConfig>,

    #[serde(default = "default_submission_eviction_config")]
    pub submission_eviction: Option<EvictionConfig>,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            action: Default::default(),
            image_eviction: default_image_eviction_config(),
            submission_eviction: default_submission_eviction_config(),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct EvictionConfig {
    pub ttl_minute: u64,
    pub interval_minute: u64,
    pub capacity: usize,
}

#[inline]
fn default_image_eviction_config() -> Option<EvictionConfig> {
    Some(EvictionConfig { ttl_minute: 60 * 24 * 7, interval_minute: 60 * 3, capacity: 50 })
}

#[inline]
fn default_submission_eviction_config() -> Option<EvictionConfig> {
    Some(EvictionConfig { ttl_minute: 60 * 24, interval_minute: 30, capacity: 200 })
}
