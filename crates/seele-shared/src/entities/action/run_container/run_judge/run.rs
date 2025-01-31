use serde::{Deserialize, Serialize};

use super::MountFile;
use crate::entities::run_container;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    #[serde(flatten)]
    pub run_container_config: run_container::Config,

    #[serde(default)]
    pub files: Vec<MountFile>,
}
