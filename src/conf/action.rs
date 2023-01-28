use serde::Deserialize;

#[derive(Debug, Default, Deserialize)]
pub struct ActionConfig {
    #[serde(default)]
    pub add_file: ActionAddFileConfig,

    #[serde(default)]
    pub run_container: ActionRunContainerConfig,
}

#[derive(Debug, Deserialize)]
pub struct ActionAddFileConfig {
    #[serde(default = "default_cache_size_mib")]
    pub cache_size_mib: u64,

    #[serde(default = "default_cache_ttl_hour")]
    pub cache_ttl_hour: u64,
}

impl Default for ActionAddFileConfig {
    fn default() -> Self {
        Self { cache_size_mib: default_cache_size_mib(), cache_ttl_hour: default_cache_ttl_hour() }
    }
}

#[inline]
fn default_cache_size_mib() -> u64 {
    512
}

#[inline]
fn default_cache_ttl_hour() -> u64 {
    24 * 3
}

#[derive(Debug, Deserialize)]
pub struct ActionRunContainerConfig {
    #[serde(default = "default_container_concurrency")]
    pub container_concurrency: usize,
}

impl Default for ActionRunContainerConfig {
    fn default() -> Self {
        Self { container_concurrency: default_container_concurrency() }
    }
}

#[inline]
fn default_container_concurrency() -> usize {
    // TODO: infer from cpu core numbers
    4
}
