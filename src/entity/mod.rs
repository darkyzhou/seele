use std::sync::Arc;

use serde::{Deserialize, Serialize};

type SequenceTasks = Vec<(String, Arc<TaskConfig>)>;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SubmissionConfig {
    #[serde(rename = "steps")]
    tasks: SequenceTasks,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TaskConfig {
    #[serde(skip_serializing, default = "default_when")]
    pub when: String,
    #[serde(skip_serializing, default)]
    pub needs: Option<String>,
    #[serde(skip_serializing_if = "TaskExtraConfig::is_execution_task", flatten)]
    pub extra: TaskExtraConfig,

    #[serde(skip_deserializing, default = "random_id")]
    pub id: String,
    #[serde(skip_deserializing, default)]
    pub status: TaskStatus,
}

#[inline]
fn default_when() -> String {
    "previous.ok".to_string()
}

#[inline]
fn random_id() -> String {
    "TODO".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum TaskExtraConfig {
    Sequence(SequenceTaskConfig),
    Parallel(ParallelTaskConfig),
    Action(ActionTaskConfig),
}

impl TaskExtraConfig {
    fn is_execution_task(config: &TaskExtraConfig) -> bool {
        matches!(config, Self::Action(_))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SequenceTaskConfig {
    #[serde(rename = "steps")]
    pub tasks: SequenceTasks,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ParallelTaskConfig {
    #[serde(rename = "parallel")]
    pub tasks: Vec<Arc<TaskConfig>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "action")]
pub enum ActionTaskConfig {
    #[serde(rename = "seele/add-file@1")]
    AddFile(ActionAddFileConfig),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ActionAddFileConfig {}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum TaskStatus {
    Pending,
    // define how to serialize it
    Skipped,
    // define how to serialize it
    Failed,
}

impl Default for TaskStatus {
    fn default() -> Self {
        Self::Pending
    }
}
