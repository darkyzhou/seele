use serde::Deserialize;

use crate::conf::env;

#[derive(Debug, Deserialize)]
pub struct HttpConfig {
    #[serde(default = "default_user_agent")]
    pub user_agent: String,

    #[serde(default = "default_connect_timeout_seconds")]
    pub connect_timeout_seconds: u64,

    #[serde(default = "default_timeout_seconds")]
    pub timeout_seconds: u64,

    #[serde(default = "default_pool_idle_timeout_seconds")]
    pub pool_idle_timeout_seconds: u64,

    #[serde(default = "default_pool_max_idle_per_host")]
    pub pool_max_idle_per_host: usize,
}

impl Default for HttpConfig {
    #[inline]
    fn default() -> Self {
        Self {
            user_agent: default_user_agent(),
            connect_timeout_seconds: default_connect_timeout_seconds(),
            timeout_seconds: default_timeout_seconds(),
            pool_idle_timeout_seconds: default_pool_idle_timeout_seconds(),
            pool_max_idle_per_host: default_pool_max_idle_per_host(),
        }
    }
}

#[inline]
fn default_user_agent() -> String {
    format!("seele/{}", env::COMMIT_TAG.or(*env::COMMIT_SHA).unwrap_or("unknown"))
}

#[inline]
fn default_connect_timeout_seconds() -> u64 {
    8
}

#[inline]
fn default_timeout_seconds() -> u64 {
    60
}

#[inline]
fn default_pool_idle_timeout_seconds() -> u64 {
    600
}

#[inline]
fn default_pool_max_idle_per_host() -> usize {
    8
}
