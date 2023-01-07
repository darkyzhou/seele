mod add_file;
mod noop;
mod run_container;

use std::path::Path;

pub use add_file::add_file;
pub use noop::noop;
pub use run_container::run_container;

pub struct ActionContext<'a> {
    pub submission_root: &'a Path,
}
