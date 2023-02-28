use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct HealthzConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    #[serde(default = "default_port")]
    pub port: u16,
}

impl Default for HealthzConfig {
    fn default() -> Self {
        Self { enabled: default_enabled(), port: default_port() }
    }
}

#[inline]
fn default_enabled() -> bool {
    true
}

#[inline]
fn default_port() -> u16 {
    50000
}
