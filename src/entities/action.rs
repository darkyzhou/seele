use serde::{Deserialize, Serialize};

use crate::worker::action;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "action")]
pub enum ActionTaskConfig {
    #[serde(rename = "seele/noop@1")]
    Noop(action::noop::Config),

    #[serde(rename = "seele/add-file@1")]
    AddFile(action::add_file::Config),

    #[serde(rename = "seele/run-container@1")]
    RunContainer(action::run_container::Config),

    #[serde(rename = "seele/run-judge/compile@1")]
    RunJudgeCompile(action::run_container::run_judge::compile::Config),

    #[serde(rename = "seele/run-judge/run@1")]
    RunJudgeRun(action::run_container::run_judge::run::Config),
}

#[derive(Debug, Clone)]
pub enum ActionReportExt {
    Success(ActionSuccessReportExt),
    Failure(ActionFailureReportExt),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ActionSuccessReportExt {
    Noop(action::noop::ExecutionReport),
    AddFile,
    RunCompile(action::run_container::run_judge::compile::ExecutionReport),
    RunContainer(action::run_container::ExecutionReport),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ActionFailureReportExt {
    Noop(action::noop::ExecutionReport),
    AddFile(action::add_file::FailedReport),
    RunContainer(action::run_container::ExecutionReport),
}
