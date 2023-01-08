use crate::entity::ActionExecutionReport;
use serde::{Deserialize, Serialize};

pub async fn noop(config: &ActionNoopConfig) -> anyhow::Result<ActionExecutionReport> {
    Ok(ActionExecutionReport::Noop(NoopExecutionReport { test: config.test }))
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ActionNoopConfig {
    #[serde(default)]
    pub test: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NoopExecutionReport {
    pub test: u64,
}
