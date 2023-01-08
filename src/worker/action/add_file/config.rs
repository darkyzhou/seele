use serde::{Deserialize, Serialize};
use std::{fmt::Display, path::PathBuf};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ActionAddFileConfig {
    pub files: Vec<ActionAddFileFileItem>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ActionAddFileFileItem {
    Http { path: PathBuf, url: String },
    Inline { path: PathBuf, text: String },
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
