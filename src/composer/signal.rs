use serde::Serialize;
use serde_json::Value;

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
    Progress {
        status: Value,

        #[serde(skip_serializing_if = "Option::is_none")]
        report: Option<Value>,

        #[serde(skip_serializing_if = "Option::is_none")]
        report_error: Option<String>,
    },
    Error {
        error: String,
    },
    Completed {
        status: Value,

        #[serde(skip_serializing_if = "Option::is_none")]
        report: Option<Value>,

        #[serde(skip_serializing_if = "Option::is_none")]
        report_error: Option<String>,
    },
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
