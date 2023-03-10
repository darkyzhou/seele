use serde::Deserialize;

use super::ActionConfig;

#[derive(Debug, Default, Deserialize)]
pub struct WorkerConfig {
    #[serde(default)]
    pub action: ActionConfig,
}
