use crate::entity::{ActionNoopConfig, TaskSuccessReportExtra};

pub async fn noop(config: &ActionNoopConfig) -> anyhow::Result<TaskSuccessReportExtra> {
    Ok(TaskSuccessReportExtra::Noop { test: config.test })
}
