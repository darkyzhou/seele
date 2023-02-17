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
    ParseError {
        error: String,
    },
    InternalError {
        error: String,
    },
    ExecutionError {
        status: Value,

        #[serde(skip_serializing_if = "Option::is_none")]
        report: Option<Value>,

        #[serde(skip_serializing_if = "Option::is_none")]
        report_error: Option<String>,
    },
    Success {
        status: Value,

        #[serde(skip_serializing_if = "Option::is_none")]
        report: Option<Value>,

        #[serde(skip_serializing_if = "Option::is_none")]
        report_error: Option<String>,
    },
}

impl SubmissionCompletedSignal {
    pub fn get_type(&self) -> &'static str {
        match self {
            Self::ParseError { .. } => "PARSE_ERROR",
            Self::InternalError { .. } => "INTERNAL_ERROR",
            Self::ExecutionError { .. } => "EXECUTION_ERROR",
            Self::Success { .. } => "SUCCESS",
        }
    }
}
