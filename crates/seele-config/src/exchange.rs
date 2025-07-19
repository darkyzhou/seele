use std::net::{IpAddr, Ipv4Addr};

use serde::Deserialize;
use url::Url;

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
#[allow(clippy::large_enum_variant)]
pub enum ExchangeConfig {
    #[serde(rename = "http")]
    Http(HttpExchangeConfig),

    #[serde(rename = "amqp")]
    Amqp(AmqpExchangeConfig),
}

#[derive(Debug, Deserialize)]
pub struct HttpExchangeConfig {
    #[serde(default = "default_http_address")]
    pub address: IpAddr,

    pub port: u16,

    #[serde(default = "default_max_body_size")]
    pub max_body_size_bytes: u64,
}

#[derive(Debug, Deserialize)]
pub struct AmqpExchangeConfig {
    pub url: Url,
    pub submission: AmqpExchangeSubmissionConfig,
    pub report: AmqpExchangeReportConfig,
}

#[derive(Debug, Deserialize)]
pub struct AmqpExchangeSubmissionConfig {
    pub exchange: LapinExchangeConfig,
    pub routing_key: String,
    pub queue: String,

    #[serde(default)]
    #[serde(with = "QueueDeclareOptionsProxy")]
    pub queue_options: lapin::options::QueueDeclareOptions,
}

#[derive(Deserialize)]
#[serde(remote = "lapin::options::QueueDeclareOptions")]
pub struct QueueDeclareOptionsProxy {
    pub passive: bool,
    pub durable: bool,
    pub exclusive: bool,
    pub auto_delete: bool,
    pub nowait: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AmqpExchangeReportConfig {
    pub exchange: LapinExchangeConfig,
    pub report_routing_key: String,

    #[serde(default)]
    pub progress_routing_key: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LapinExchangeConfig {
    pub name: String,

    #[serde(default)]
    #[serde(with = "ExchangeKindProxy")]
    pub kind: lapin::ExchangeKind,

    #[serde(default)]
    #[serde(with = "ExchangeDeclareOptionsProxy")]
    pub options: lapin::options::ExchangeDeclareOptions,
}

#[derive(Deserialize)]
#[serde(remote = "lapin::ExchangeKind")]
pub enum ExchangeKindProxy {
    Custom(String),
    Direct,
    Fanout,
    Headers,
    Topic,
}

#[derive(Deserialize)]
#[serde(remote = "lapin::options::ExchangeDeclareOptions")]
pub struct ExchangeDeclareOptionsProxy {
    pub passive: bool,
    pub durable: bool,
    pub auto_delete: bool,
    pub internal: bool,
    pub nowait: bool,
}

#[inline]
const fn default_http_address() -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))
}

#[inline]
const fn default_max_body_size() -> u64 {
    8 * 1024 * 1024
}
