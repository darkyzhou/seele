use std::path::PathBuf;

use anyhow::{Context, Result};
use once_cell::sync::Lazy;
use tokio::fs;

use super::CONFIG;

#[derive(Debug)]
pub struct SeelePaths {
    pub root: PathBuf,
    pub images: PathBuf,
    pub temp: PathBuf,
    pub submissions: PathBuf,
}

impl SeelePaths {
    pub async fn new_temp_directory(&self) -> Result<PathBuf> {
        let path = self.temp.join(format!("{}", nano_id::base62::<16>()));
        fs::create_dir(&path)
            .await
            .with_context(|| format!("Error creating temp directory {}", path.display()))?;
        Ok(path)
    }
}

pub static PATHS: Lazy<SeelePaths> = Lazy::new(|| SeelePaths {
    root: CONFIG.paths.root.clone(),
    images: CONFIG.paths.root.join("images"),
    temp: CONFIG.paths.root.join("temp"),
    submissions: CONFIG.paths.tmp.join("seele").join("submissions"),
});
