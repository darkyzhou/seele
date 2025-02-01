use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    #[serde(default)]
    pub test: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExecutionReport {
    pub test: u64,
}
