use std::path::PathBuf;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

pub type SubmissionReport = IndexMap<String, serde_yaml::Value>;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum SubmissionReporter {
    #[serde(rename = "javascript")]
    JavaScript { source: String },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SubmissionReportConfig {
    pub report: SubmissionReport,

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
#[serde(rename_all = "UPPERCASE")]
pub enum SubmissionReportUploadMethod {
    Get,
    Post,
    Put,
}