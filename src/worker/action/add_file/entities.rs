use std::{fmt::Display, path::PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub files: Vec<FileItem>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum FileItem {
    Http { path: PathBuf, url: String },
    Inline { path: PathBuf, text: String },
}

impl Display for FileItem {
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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FailedReport {
    pub files: Vec<String>,
}
