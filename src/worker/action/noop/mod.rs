use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::entities::ActionSuccessReportExt;

pub async fn execute(config: &Config) -> Result<ActionSuccessReportExt> {
    Ok(ActionSuccessReportExt::Noop(ExecutionReport { test: config.test }))
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    #[serde(default)]
    pub test: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExecutionReport {
    pub test: u64,
}
