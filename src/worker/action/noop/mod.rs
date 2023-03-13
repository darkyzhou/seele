use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::entities::{ActionReportExt, ActionSuccessReportExt};

pub async fn execute(config: &Config) -> Result<ActionReportExt> {
    Ok(ActionReportExt::Success(ActionSuccessReportExt::Noop(ExecutionReport {
        test: config.test,
    })))
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
