mod add_file;
mod noop;
mod run_container;
mod run_judge;

use std::path::PathBuf;

pub use add_file::*;
pub use noop::*;
pub use run_container::*;

#[derive(Debug)]
pub struct ActionContext {
    pub submission_root: PathBuf,
}
