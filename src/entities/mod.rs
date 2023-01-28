use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

pub use action::*;

pub type SequenceTasks = IndexMap<String, Arc<TaskConfig>>;
pub type ParallelTasks = Vec<Arc<TaskConfig>>;
pub type UtcTimestamp = DateTime<Utc>;

mod action;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SubmissionConfig {
    #[cfg_attr(test, serde(skip_serializing))]
    #[serde(skip_deserializing, default = "make_submitted_at")]
    pub submitted_at: UtcTimestamp,

    #[serde(default = "random_submission_id")]
    pub id: String,

    #[serde(rename = "steps")]
    pub tasks: SequenceTasks,
}

#[inline]
fn make_submitted_at() -> UtcTimestamp {
    Utc::now()
}

#[inline]
fn random_submission_id() -> String {
    nano_id::base62::<16>()
}

#[derive(Debug, Clone)]
#[cfg_attr(test, derive(Serialize))]
pub struct Submission {
    pub id: String,
    pub config: Arc<SubmissionConfig>,
    pub root: Arc<RootTaskNode>,
    #[cfg_attr(test, serde(skip_serializing))]
    pub id_to_node_map: HashMap<String, Arc<TaskNode>>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TaskConfig {
    #[serde(skip_deserializing, default, flatten)]
    pub status: RwLock<TaskStatus>,

    #[serde(skip_serializing, default)]
    pub when: Option<String>,
    #[serde(skip_serializing, default)]
    pub needs: Option<String>,
    #[serde(skip_serializing_if = "TaskConfigExt::is_action_task", flatten)]
    pub ext: TaskConfigExt,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum TaskConfigExt {
    Sequence(SequenceTaskConfig),
    Parallel(ParallelTaskConfig),
    Action(ActionTaskConfig),
}

impl TaskConfigExt {
    #[inline]
    fn is_action_task(config: &TaskConfigExt) -> bool {
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

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status")]
pub enum TaskStatus {
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "skipped")]
    Skipped,
    #[serde(rename = "failed")]
    Failed(TaskFailedReport),
    #[serde(rename = "success")]
    Success(TaskSuccessReport),
}

impl Default for TaskStatus {
    fn default() -> Self {
        Self::Pending
    }
}

#[derive(Debug)]
pub enum TaskReport {
    Success(TaskSuccessReport),
    Failed(TaskFailedReport),
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum TaskSuccessReport {
    Schedule,
    Action { run_at: UtcTimestamp, time_elapsed_ms: u64, report: ActionExecutionReport },
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum TaskFailedReport {
    Schedule,
    Action { run_at: Option<UtcTimestamp>, time_elapsed_ms: Option<u64>, message: String },
}

#[derive(Debug, Clone)]
#[cfg_attr(test, derive(Serialize))]
pub struct RootTaskNode {
    pub id: String,
    pub tasks: Vec<Arc<TaskNode>>,
}

#[derive(Debug, Clone)]
#[cfg_attr(test, derive(Serialize))]
pub struct TaskNode {
    #[cfg_attr(test, serde(skip_serializing))]
    pub config: Arc<TaskConfig>,
    pub id: String,
    pub children: Vec<Arc<TaskNode>>,
    pub ext: TaskNodeExt,
}

#[derive(Debug, Clone)]
#[cfg_attr(test, derive(Serialize))]
#[cfg_attr(test, serde(untagged))]
pub enum TaskNodeExt {
    Schedule(Vec<Arc<TaskNode>>),
    Action(Arc<ActionTaskConfig>),
}
