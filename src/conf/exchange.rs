use serde::Deserialize;
use std::net::{IpAddr, Ipv4Addr};

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum ExchangeConfig {
    #[serde(rename = "http")]
    Http(HttpExchangeConfig),
}

#[derive(Debug, Deserialize)]
pub struct HttpExchangeConfig {
    #[serde(default = "default_http_address")]
    pub address: IpAddr,

    pub port: u16,

    #[serde(default = "default_max_body_size")]
    pub max_body_size_bytes: u64,
}

#[inline]
fn default_http_address() -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))
}

#[inline]
fn default_max_body_size() -> u64 {
    8 * 1024 * 1024
}
