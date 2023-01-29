use std::path::PathBuf;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum SubmissionReporter {
    #[serde(rename = "javascript")]
    JavaScript { source: String },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SubmissionReportConfig {
    pub report: IndexMap<String, serde_yaml::Value>,

    #[serde(default)]
    pub embeds: Vec<SubmissionReportEmbedConfig>,

    #[serde(default)]
    pub uploads: Vec<SubmissionReportUploadConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SubmissionReportEmbedConfig {
    pub path: PathBuf,
    pub field: String,
    pub truncate_kib: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SubmissionReportUploadConfig {
    pub path: PathBuf,
    pub target: String,
    pub method: SubmissionReportUploadMethod,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub enum SubmissionReportUploadMethod {
    #[serde(rename = "GET")]
    Get,
    #[serde(rename = "POST")]
    Post,
    #[serde(rename = "PUT")]
    Put,
}
