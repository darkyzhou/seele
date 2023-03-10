use std::path::Path;

use once_cell::sync::Lazy;

pub mod compile;
pub mod run;

static DEFAULT_MOUNT_DIRECTORY: Lazy<&'static Path> = Lazy::new(|| Path::new("/seele"));
