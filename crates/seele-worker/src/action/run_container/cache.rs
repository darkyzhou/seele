use std::{
    sync::{Arc, LazyLock},
    time::Duration,
};

use moka::sync::Cache;

use crate::conf;

#[allow(clippy::type_complexity)]
static CACHE: LazyLock<Cache<Box<[u8]>, Arc<[u8]>>> = LazyLock::new(|| {
    let config = &conf::CONFIG.worker.action.run_container;
    Cache::builder()
        .name("seele-run-container")
        .weigher(|_, value: &Arc<[u8]>| -> u32 { value.len().try_into().unwrap_or(u32::MAX) })
        .max_capacity(1024 * 1024 * config.cache_size_mib)
        .time_to_idle(Duration::from_secs(60 * 60 * config.cache_ttl_hour))
        .build()
});

pub fn init() {
    LazyLock::force(&CACHE);
}

pub fn get(key: &[u8]) -> Option<Arc<[u8]>> {
    CACHE.get(key)
}

pub fn write(key: Box<[u8]>, value: Arc<[u8]>) {
    CACHE.insert(key, value)
}
