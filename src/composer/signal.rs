use serde::Serialize;
use serde_yaml::Value;

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SubmissionSignal {
    Progress(SubmissionProgressSignal),
    Completed(SubmissionCompletedSignal),
}

#[derive(Debug, Serialize)]
pub struct SubmissionProgressSignal {
    pub name: String,
    pub status: Value,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SubmissionCompletedSignal {
    InternalError { error: String },
    ExecutionError { error: String, status: Value },
    ReporterError { error: String, status: Value },
    Success { status: Value, report: Option<Value> },
}
