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
    RunJudgeCompile(action::run_judge::compile::Config),

    #[serde(rename = "seele/run-judge/run@1")]
    RunJudgeRun(action::run_judge::run::Config),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ActionSuccessReportExt {
    Noop(action::noop::ExecutionReport),
    AddFile,
    RunContainer(action::run_container::ExecutionReport),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ActionFailedReportExt {
    Noop(action::noop::ExecutionReport),
    AddFile(action::add_file::FailedReport),
    RunContainer(action::run_container::ExecutionReport),
}
