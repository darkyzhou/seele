use serde::Serialize;
use serde_json::Value;

use crate::entities::UtcTimestamp;

#[derive(Debug, Serialize)]
pub struct SubmissionSignal {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    #[serde(flatten)]
    pub ext: SubmissionSignalExt,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SubmissionSignalExt {
    Progress(SubmissionReportSignal),
    Error(SubmissionErrorSignal),
    Completed(SubmissionReportSignal),
}

#[derive(Debug, Serialize)]
pub struct SubmissionErrorSignal {
    pub error: String,
}

#[derive(Debug, Serialize)]
pub struct SubmissionReportSignal {
    pub report_at: UtcTimestamp,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub report: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub report_error: Option<String>,

    pub status: Value,
}

impl SubmissionSignalExt {
    pub fn get_type(&self) -> &'static str {
        match self {
            Self::Progress { .. } => "PROGRESS",
            Self::Error { .. } => "ERROR",
            Self::Completed { .. } => "COMPLETED",
        }
    }
}
