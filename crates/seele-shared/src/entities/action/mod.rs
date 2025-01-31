use serde::{Deserialize, Serialize};

pub mod add_file;
pub mod noop;
pub mod run_container;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "action")]
pub enum ActionTaskConfig {
    #[serde(rename = "seele/noop@1")]
    Noop(noop::Config),

    #[serde(rename = "seele/add-file@1")]
    AddFile(add_file::Config),

    #[serde(rename = "seele/run-container@1")]
    RunContainer(run_container::Config),

    #[serde(rename = "seele/run-judge/compile@1")]
    RunJudgeCompile(run_container::run_judge::compile::Config),

    #[serde(rename = "seele/run-judge/run@1")]
    RunJudgeRun(run_container::run_judge::run::Config),
}

#[derive(Debug, Clone)]
pub enum ActionReportExt {
    Success(ActionSuccessReportExt),
    Failure(ActionFailureReportExt),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ActionSuccessReportExt {
    Noop(noop::ExecutionReport),
    AddFile,
    RunCompile(run_container::run_judge::compile::ExecutionReport),
    RunContainer(run_container::ExecutionReport),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ActionFailureReportExt {
    Noop(noop::ExecutionReport),
    AddFile(add_file::FailedReport),
    RunContainer(run_container::ExecutionReport),
}
