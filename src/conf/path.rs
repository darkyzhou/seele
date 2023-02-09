use std::path::PathBuf;

use once_cell::sync::Lazy;

use super::CONFIG;

#[derive(Debug)]
pub struct SeelePaths {
    pub root: PathBuf,
    pub images: PathBuf,
    pub evicted: PathBuf,
    pub temp: PathBuf,
    pub submissions: PathBuf,
}

pub static PATHS: Lazy<SeelePaths> = Lazy::new(|| SeelePaths {
    root: CONFIG.paths.root.clone(),
    images: CONFIG.paths.root.join("images"),
    evicted: CONFIG.paths.root.join("evicted"),
    temp: CONFIG.paths.root.join("temp"),
    submissions: CONFIG.paths.tmp.join("seele").join("submissions"),
});
