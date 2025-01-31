use anyhow::Result;
use seele_shared::entities::noop::*;

use crate::entities::{ActionReportExt, ActionSuccessReportExt};

pub async fn execute(config: &Config) -> Result<ActionReportExt> {
    Ok(ActionReportExt::Success(ActionSuccessReportExt::Noop(ExecutionReport {
        test: config.test,
    })))
}
