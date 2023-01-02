use serde::Deserialize;
use std::net::{IpAddr, Ipv4Addr};

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum ExchangeConfig {
    #[serde(rename = "http")]
    Http {
        #[serde(default = "default_http_address")]
        address: IpAddr,
        port: u16,
    },
}

#[inline]
fn default_http_address() -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))
}
