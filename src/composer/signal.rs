use serde::Serialize;
use serde_yaml::Value;

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum SubmissionSignal {
    #[serde(rename = "progress")]
    Progress(SubmissionProgressSignal),

    #[serde(rename = "error")]
    Error(SubmissionErrorSignal),

    #[serde(rename = "completed")]
    Completed(SubmissionCompletedSignal),
}

#[derive(Debug, Serialize)]
pub struct SubmissionProgressSignal {
    pub name: String,
    pub status: Value,
}

#[derive(Debug, Serialize)]
pub struct SubmissionErrorSignal {
    pub error: String,
}

#[derive(Debug, Serialize)]
pub struct SubmissionCompletedSignal {
    pub status: Value,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub report: Option<Value>,
}
