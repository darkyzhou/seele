use std::time::Duration;

use reqwest::Client;

pub fn build_http_client() -> Client {
    Client::builder()
        .user_agent(concat!("seele/", env!("CARGO_PKG_VERSION")))
        // TODO: move to conf
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(30))
        .pool_idle_timeout(Duration::from_secs(600))
        .pool_max_idle_per_host(8)
        .build()
        .unwrap()
}
