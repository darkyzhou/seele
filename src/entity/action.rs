use serde::{Deserialize, Serialize};
use std::{fmt::Display, path::Path};

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
#[serde(untagged)]
pub enum ActionAddFileFileItem {
    Http { path: Box<Path>, url: String },
    Inline { path: Box<Path>, text: String },
}

impl Display for ActionAddFileFileItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use ellipse::Ellipse;

        match self {
            Self::Http { path, url } => write!(f, "{}({})", path.display(), url),
            Self::Inline { path, text } => {
                write!(f, "{}({}...)", path.display(), text.as_str().truncate_ellipse(20))
            }
        }
    }
}
