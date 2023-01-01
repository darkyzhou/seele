use crate::entity::{ActionNoopConfig, TaskSuccessReportExtra};

pub async fn run_noop_action(config: &ActionNoopConfig) -> anyhow::Result<TaskSuccessReportExtra> {
    Ok(TaskSuccessReportExtra::Noop(config.test))
}
