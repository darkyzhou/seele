use std::{
    path::PathBuf,
    sync::{Arc, RwLock},
};

use chrono::{DateTime, Utc};
use either::Either;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use super::{ActionFailedReportExt, ActionSuccessReportExt, ActionTaskConfig, SubmissionReporter};

pub type UtcTimestamp = DateTime<Utc>;

pub type SequenceTasks = IndexMap<String, Arc<TaskConfig>>;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ParallelTasks {
    Anonymous(Vec<Arc<TaskConfig>>),
    Named(IndexMap<String, Arc<TaskConfig>>),
}

impl<'a> ParallelTasks {
    pub fn iter(
        &'a self,
    ) -> Either<
        impl Iterator<Item = Arc<TaskConfig>> + 'a,
        impl Iterator<Item = Arc<TaskConfig>> + 'a,
    > {
        match self {
            Self::Anonymous(tasks) => Either::Left(tasks.iter().cloned()),
            Self::Named(tasks) => Either::Right(tasks.values().cloned()),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SubmissionConfig {
    #[cfg_attr(test, serde(skip_serializing))]
    #[serde(skip_deserializing, default = "make_submitted_at")]
    pub submitted_at: UtcTimestamp,

    #[serde(default = "random_submission_id")]
    pub id: String,

    #[serde(default)]
    pub tracing_attribute: Option<String>,

    #[serde(rename = "steps")]
    pub tasks: SequenceTasks,

    #[serde(default, skip_serializing)]
    pub reporter: Option<SubmissionReporter>,
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
    pub root_directory: PathBuf,
    pub config: Arc<SubmissionConfig>,
    pub root_node: Arc<RootTaskNode>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TaskConfig {
    #[serde(default)]
    pub tags: Option<IndexMap<String, String>>,

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
#[serde(tag = "status", rename_all = "UPPERCASE")]
pub enum TaskStatus {
    Pending,
    Skipped,
    Failed { report: TaskFailedReport },
    Success { report: TaskSuccessReport },
}

impl Default for TaskStatus {
    fn default() -> Self {
        Self::Pending
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum TaskSuccessReport {
    Sequence(SequenceSuccessReport),
    Parallel(ParallelSuccessReport),
    Action(ActionSuccessReport),
}

#[derive(Debug, Clone, Serialize)]
pub struct SequenceSuccessReport {
    pub time_elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ParallelSuccessReport {
    pub time_elapsed_ms: u64,
}

#[derive(Debug)]
pub enum ActionReport {
    Success(ActionSuccessReport),
    Failed(ActionFailedReport),
}

impl From<ActionSuccessReport> for ActionReport {
    fn from(value: ActionSuccessReport) -> Self {
        Self::Success(value)
    }
}

impl From<ActionFailedReport> for ActionReport {
    fn from(value: ActionFailedReport) -> Self {
        Self::Failed(value)
    }
}

impl From<ActionReport> for TaskStatus {
    fn from(value: ActionReport) -> Self {
        match value {
            ActionReport::Success(report) => {
                TaskStatus::Success { report: TaskSuccessReport::Action(report) }
            }
            ActionReport::Failed(report) => {
                TaskStatus::Failed { report: TaskFailedReport::Action(report) }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ActionSuccessReport {
    pub run_at: UtcTimestamp,
    pub time_elapsed_ms: u64,

    #[serde(flatten)]
    pub ext: ActionSuccessReportExt,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum TaskFailedReport {
    Sequence(SequenceFailedReport),
    Parallel(ParallelFailedReport),
    Action(ActionFailedReport),
}

#[derive(Debug, Clone, Serialize)]
pub struct SequenceFailedReport {
    pub time_elapsed_ms: u64,
    pub failed_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ParallelFailedReport {
    pub time_elapsed_ms: u64,
    pub failed_count: usize,
    pub failed_indexes: Vec<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActionFailedReport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_at: Option<UtcTimestamp>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_elapsed_ms: Option<u64>,

    pub error: String,

    #[serde(skip_serializing_if = "Option::is_none", flatten)]
    pub ext: Option<ActionFailedReportExt>,
}

impl From<String> for ActionFailedReport {
    fn from(value: String) -> Self {
        Self { run_at: None, time_elapsed_ms: None, error: value, ext: None }
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(test, derive(Serialize))]
pub struct RootTaskNode {
    pub tasks: Vec<Arc<TaskNode>>,
}

#[derive(Debug, Clone)]
#[cfg_attr(test, derive(Serialize))]
pub struct TaskNode {
    pub name: String,
    #[cfg_attr(test, serde(skip_serializing))]
    pub config: Arc<TaskConfig>,
    pub children: Vec<Arc<TaskNode>>,
    pub ext: TaskNodeExt,
}

#[derive(Debug, Clone)]
#[cfg_attr(test, derive(Serialize), serde(untagged))]
pub enum TaskNodeExt {
    Schedule(Vec<Arc<TaskNode>>),
    Action(Arc<ActionTaskConfig>),
}
