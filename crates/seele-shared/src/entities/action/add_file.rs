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
    PlainText { plain: String },
    Base64 { base64: String },
    LocalPath { local: PathBuf },
}

impl Display for FileItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use ellipse::Ellipse;

        write!(
            f,
            "{}({})",
            self.path.display(),
            match &self.ext {
                FileItemExt::Http { url } => url.to_string(),
                FileItemExt::PlainText { plain } =>
                    format!("{}...", plain.as_str().truncate_ellipse(30)),
                FileItemExt::Base64 { base64 } =>
                    format!("{}...", base64.as_str().truncate_ellipse(30)),
                FileItemExt::LocalPath { local } => format!("{}", local.display()),
            }
        )
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FailedReport {
    pub files: Vec<String>,
}
