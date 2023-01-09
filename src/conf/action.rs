use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
pub struct ActionConfig {
    #[serde(default)]
    pub add_file: ActionAddFileConfig,
}

#[derive(Debug, Deserialize)]
pub struct ActionAddFileConfig {
    #[serde(default = "default_cache_size_mib")]
    pub cache_size_mib: u64,

    #[serde(default = "default_cache_ttl_hour")]
    pub cache_ttl_hour: u64,
}

#[inline]
fn default_cache_size_mib() -> u64 {
    512
}

#[inline]
fn default_cache_ttl_hour() -> u64 {
    24 * 3
}

impl Default for ActionAddFileConfig {
    fn default() -> Self {
        Self { cache_size_mib: default_cache_size_mib(), cache_ttl_hour: default_cache_ttl_hour() }
    }
}
