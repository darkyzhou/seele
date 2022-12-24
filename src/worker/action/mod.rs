mod add_file;
mod noop;

use std::path::Path;

pub use add_file::run_add_file_action;
pub use noop::run_noop_action;

pub struct ActionContext<'a> {
    pub submission_root: &'a Path,
}
