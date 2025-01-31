use std::path::PathBuf;

pub mod add_file;
pub mod noop;
pub mod run_container;

#[derive(Debug)]
pub struct ActionContext {
    pub submission_root: PathBuf,
}
