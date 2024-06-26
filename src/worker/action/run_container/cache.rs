use std::{sync::Arc, time::Duration};

use moka::sync::Cache;
use once_cell::sync::Lazy;

use crate::conf;

#[allow(clippy::type_complexity)]
static CACHE: Lazy<Cache<Box<[u8]>, Arc<[u8]>>> = Lazy::new(|| {
    let config = &conf::CONFIG.worker.action.run_container;
    Cache::builder()
        .name("seele-run-container")
        .weigher(|_, value: &Arc<[u8]>| -> u32 { value.len().try_into().unwrap_or(u32::MAX) })
        .max_capacity(1024 * 1024 * config.cache_size_mib)
        .time_to_idle(Duration::from_secs(60 * 60 * config.cache_ttl_hour))
        .build()
});

pub async fn init() {
    _ = *CACHE;
}

pub async fn get(key: &[u8]) -> Option<Arc<[u8]>> {
    CACHE.get(key)
}

pub async fn write(key: Box<[u8]>, value: Arc<[u8]>) {
    CACHE.insert(key, value)
}
