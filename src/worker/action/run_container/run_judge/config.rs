use crate::worker::ActionRunContainerConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ActionRunConfig {
    #[serde(flatten)]
    pub run_container_config: ActionRunContainerConfig,

    #[serde(default)]
    pub executable: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ActionCompileConfig {
    #[serde(flatten)]
    pub run_container_config: ActionRunContainerConfig,

    #[serde(default)]
    pub source: Vec<String>,

    #[serde(default)]
    pub save: Vec<String>,

    #[serde(default)]
    pub cache: Vec<CacheItem>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ActionJudgeConfig {
    pub run_config: ActionRunConfig,
    pub compare_config: ActionRunConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum CacheItem {
    String(String),
    File { file: String },
}
