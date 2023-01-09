use super::CONFIG;
use once_cell::sync::Lazy;
use std::path::PathBuf;

#[derive(Debug)]
pub struct SeelePaths {
    pub root: PathBuf,
    pub images: PathBuf,
    pub submissions: PathBuf,
}

pub static PATHS: Lazy<SeelePaths> = Lazy::new(|| SeelePaths {
    root: CONFIG.root_path.clone(),
    images: CONFIG.root_path.join("images"),
    submissions: CONFIG.root_path.join("submissions"),
});
