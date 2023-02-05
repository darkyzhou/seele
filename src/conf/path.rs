use std::path::PathBuf;

use once_cell::sync::Lazy;

use super::CONFIG;

#[derive(Debug)]
pub struct SeelePaths {
    pub root: PathBuf,
    pub images: PathBuf,
    pub submissions: PathBuf,
    pub evicted: PathBuf,
    pub states: PathBuf,
    pub temp: PathBuf,
}

pub static PATHS: Lazy<SeelePaths> = Lazy::new(|| SeelePaths {
    root: CONFIG.root_path.clone(),
    images: CONFIG.root_path.join("images"),
    submissions: CONFIG.root_path.join("submissions"),
    evicted: CONFIG.root_path.join("evicted"),
    states: CONFIG.root_path.join("states"),
    temp: CONFIG.root_path.join("temp"),
});
