use crate::worker::{
    run_judge, ActionAddFileConfig, ActionNoopConfig, ActionRunContainerConfig,
    ContainerExecutionReport, NoopExecutionReport,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "action")]
pub enum ActionTaskConfig {
    #[serde(rename = "seele/noop@1")]
    Noop(ActionNoopConfig),

    #[serde(rename = "seele/add-file@1")]
    AddFile(ActionAddFileConfig),

    #[serde(rename = "seele/run-container@1")]
    RunContainer(ActionRunContainerConfig),

    #[serde(rename = "seele/run-judge/compile@1")]
    RunJudgeCompile(run_judge::ActionCompileConfig),

    #[serde(rename = "seele/run-judge/run@1")]
    RunJudgeRun(run_judge::ActionRunConfig),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ActionExecutionReport {
    Noop(NoopExecutionReport),
    AddFile,
    RunContainer(ContainerExecutionReport),
}
