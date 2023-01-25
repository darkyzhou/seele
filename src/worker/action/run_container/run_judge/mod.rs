pub use self::config::*;
pub use compile::run_judge_compile;
pub use run::run_judge_run;

mod compile;
mod config;
mod run;

const MOUNT_DIRECTORY: &str = "/seele";
