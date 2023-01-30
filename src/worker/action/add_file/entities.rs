use std::{fmt::Display, path::PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub files: Vec<FileItem>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FileItem {
    pub path: PathBuf,

    #[serde(flatten)]
    pub ext: FileItemExt,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum FileItemExt {
    Http { url: String },
    Inline { content: String },
}

impl Display for FileItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use either::Either;
        use ellipse::Ellipse;

        write!(
            f,
            "{}({})",
            self.path.display(),
            match &self.ext {
                FileItemExt::Http { url } => Either::Left(url),
                FileItemExt::Inline { content } =>
                    Either::Right(format!("{}...", content.as_str().truncate_ellipse(30))),
            }
        )
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FailedReport {
    pub files: Vec<String>,
}
