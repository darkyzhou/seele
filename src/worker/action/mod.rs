use super::eviction::EvictionManager;
use std::{path::PathBuf, sync::Arc};

pub use add_file::*;
pub use noop::*;
pub use run_container::*;

mod add_file;
mod noop;
mod run_container;

#[derive(Debug)]
pub struct ActionContext {
    pub submission_root: PathBuf,
    pub submission_eviction_manager: Arc<Option<EvictionManager>>,
    pub image_eviction_manager: Arc<Option<EvictionManager>>,
}
