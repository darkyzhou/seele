use std::time::Duration;

use reqwest::Client;

use crate::conf;

pub fn build_http_client() -> Client {
    Client::builder()
        .user_agent(&conf::CONFIG.http.user_agent)
        .connect_timeout(Duration::from_secs(conf::CONFIG.http.connect_timeout_seconds))
        .timeout(Duration::from_secs(conf::CONFIG.http.connect_timeout_seconds))
        .pool_idle_timeout(Duration::from_secs(conf::CONFIG.http.pool_idle_timeout_seconds))
        .pool_max_idle_per_host(conf::CONFIG.http.pool_max_idle_per_host)
        .build()
        .unwrap()
}
