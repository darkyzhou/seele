use std::{env, sync::LazyLock};

pub static HOSTNAME: LazyLock<String> =
    LazyLock::new(|| env::var("HOSTNAME").unwrap_or(get_hostname()));
pub static CONTAINER_NAME: LazyLock<Option<String>> =
    LazyLock::new(|| env::var("CONTAINER_NAME").ok());
pub static CONTAINER_IMAGE_NAME: LazyLock<Option<String>> =
    LazyLock::new(|| env::var("CONTAINER_IMAGE_NAME").ok());

pub static COMMIT_TAG: LazyLock<Option<&'static str>> = LazyLock::new(|| option_env!("COMMIT_TAG"));
pub static COMMIT_SHA: LazyLock<Option<&'static str>> = LazyLock::new(|| option_env!("COMMIT_SHA"));

#[inline]
fn get_hostname() -> String {
    nix::unistd::gethostname()
        .expect("Failed to get hostname")
        .into_string()
        .expect("Error converting hostname from OsString")
}
