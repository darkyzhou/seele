use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "action")]
pub enum ActionTaskConfig {
    #[serde(rename = "seele/noop@1")]
    Noop(ActionNoopConfig),
    #[serde(rename = "seele/add-file@1")]
    AddFile(ActionAddFileConfig),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ActionNoopConfig {
    #[serde(default)]
    pub test: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ActionAddFileConfig {
    pub files: Vec<ActionAddFileFileItem>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum ActionAddFileFileItem {
    #[serde(rename = "url")]
    Http { path: Box<Path>, url: String },
    #[serde(rename = "inline")]
    Inline { path: Box<Path>, text: String },
}
