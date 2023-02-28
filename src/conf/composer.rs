use serde::Deserialize;

#[derive(Debug, Deserialize, Default)]
pub struct ComposerConfig {
    #[serde(default)]
    pub report_progress: bool,
}
