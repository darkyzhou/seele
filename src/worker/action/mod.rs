use std::{path::PathBuf, sync::Arc};

use super::eviction::EvictionManager;

pub mod add_file;
pub mod noop;
pub mod run_container;

#[derive(Debug)]
pub struct ActionContext {
    pub submission_root: PathBuf,
    pub submission_eviction_manager: Arc<Option<EvictionManager>>,
    pub image_eviction_manager: Arc<Option<EvictionManager>>,
}
