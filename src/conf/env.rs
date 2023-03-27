use std::env;

use once_cell::sync::Lazy;

pub static HOSTNAME: Lazy<String> = Lazy::new(|| env::var("HOSTNAME").unwrap_or(get_hostname()));
pub static CONTAINER_NAME: Lazy<Option<String>> = Lazy::new(|| env::var("CONTAINER_NAME").ok());
pub static CONTAINER_IMAGE_NAME: Lazy<Option<String>> =
    Lazy::new(|| env::var("CONTAINER_IMAGE_NAME").ok());

pub static COMMIT_TAG: Lazy<Option<&'static str>> = Lazy::new(|| option_env!("COMMIT_TAG"));
pub static COMMIT_SHA: Lazy<Option<&'static str>> = Lazy::new(|| option_env!("COMMIT_SHA"));

#[inline]
fn get_hostname() -> String {
    nix::unistd::gethostname()
        .expect("Failed to get hostname")
        .into_string()
        .expect("Error converting hostname from OsString")
}
