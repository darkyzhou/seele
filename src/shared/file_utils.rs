use anyhow::Context;
use std::path::Path;
use tokio::fs;

pub async fn create_file(path: &Path) -> anyhow::Result<fs::File> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await.context("Error creating the directories")?;
    }
    fs::File::create(path).await.context("Error creating the file")
}
