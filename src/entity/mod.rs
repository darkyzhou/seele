use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, RwLock},
    time::SystemTime,
};

pub type SequenceTasks = IndexMap<String, Arc<TaskConfig>>;
pub type ParallelTasks = Vec<Arc<TaskConfig>>;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SubmissionConfig {
    pub id: String,
    #[serde(rename = "steps")]
    pub tasks: SequenceTasks,
}

#[derive(Debug, Clone)]
pub struct Submission {
    pub id: String,
    pub config: SubmissionConfig,
    pub root: Arc<RootTaskNode>,
    pub id_to_node_map: HashMap<String, Arc<TaskNode>>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TaskConfig {
    #[serde(skip_serializing, default)]
    pub when: Option<String>,
    #[serde(skip_serializing, default)]
    pub needs: Option<String>,
    #[serde(skip_serializing_if = "TaskExtraConfig::is_execution_task", flatten)]
    pub extra: TaskExtraConfig,

    #[serde(skip_deserializing, default)]
    pub status: RwLock<TaskStatus>,
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
    pub tasks: ParallelTasks,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "action")]
pub enum ActionTaskConfig {
    #[serde(rename = "seele/noop@1")]
    Noop,
    #[serde(rename = "seele/add-file@1")]
    AddFile(ActionAddFileConfig),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ActionAddFileConfig {
    pub files: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum TaskStatus {
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "skipped")]
    Skipped,
    #[serde(rename = "failed")]
    Failed(TaskReport<TaskExecutionFailedReport>),
    #[serde(rename = "success")]
    Success(TaskReport<TaskExecutionSuccessReport>),
}

impl Default for TaskStatus {
    fn default() -> Self {
        Self::Pending
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskReport<T> {
    pub enqueued_at: SystemTime,
    #[serde(flatten)]
    pub execution: T,
}

#[derive(Debug, Clone, Serialize)]
pub enum TaskExecutionReport {
    Success(TaskExecutionSuccessReport),
    Failed(TaskExecutionFailedReport),
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskExecutionSuccessReport {
    pub run_at: SystemTime,
    pub time_elapsed_ms: u64,
    #[serde(flatten)]
    pub extra: TaskExecutionSuccessReportExtra,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind")]
pub enum TaskExecutionSuccessReportExtra {
    #[serde(rename = "noop")]
    Noop,
    #[serde(rename = "add-file")]
    AddFile,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskExecutionFailedReport {
    pub run_at: Option<SystemTime>,
    pub time_elapsed_ms: Option<u64>,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct RootTaskNode {
    pub id: String,
    pub tasks: Vec<Arc<TaskNode>>,
}

#[derive(Debug, Clone)]
pub struct TaskNode {
    pub config: Arc<TaskConfig>,
    pub id: String,
    pub children: Vec<Arc<TaskNode>>,
    pub extra: TaskNodeExtra,
}

#[derive(Debug, Clone)]
pub enum TaskNodeExtra {
    Schedule(Vec<Arc<TaskNode>>),
    Action(Arc<ActionTaskConfig>),
}
