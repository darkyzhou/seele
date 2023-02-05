use std::path::Path;

use anyhow::{Context, Result};
use tokio::fs;

pub async fn create_parent_directories(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await.context("Error creating the directories")?;
    }

    Ok(())
}

pub async fn create_file(path: &Path) -> Result<fs::File> {
    create_parent_directories(path).await?;
    fs::File::create(path).await.context("Error creating the file")
}
