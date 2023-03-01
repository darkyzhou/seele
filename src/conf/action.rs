use serde::Deserialize;

use crate::shared::image::OciImage;

#[derive(Debug, Default, Deserialize)]
pub struct ActionConfig {
    #[serde(default)]
    pub add_file: ActionAddFileConfig,

    #[serde(default)]
    pub run_container: ActionRunContainerConfig,
}

#[derive(Debug, Deserialize)]
pub struct ActionAddFileConfig {
    #[serde(default = "default_cache_size_mib")]
    pub cache_size_mib: u64,

    #[serde(default = "default_cache_ttl_hour")]
    pub cache_ttl_hour: u64,
}

impl Default for ActionAddFileConfig {
    fn default() -> Self {
        Self { cache_size_mib: default_cache_size_mib(), cache_ttl_hour: default_cache_ttl_hour() }
    }
}

#[inline]
fn default_cache_size_mib() -> u64 {
    512
}

#[inline]
fn default_cache_ttl_hour() -> u64 {
    24 * 3
}

#[derive(Debug, Deserialize)]
pub struct ActionRunContainerConfig {
    #[serde(default = "default_pull_image_timeout_seconds")]
    pub pull_image_timeout_seconds: u64,

    #[serde(default = "default_unpack_image_timeout_seconds")]
    pub unpack_image_timeout_seconds: u64,

    #[serde(default = "default_userns_uid")]
    pub userns_uid: u32,

    #[serde(default = "default_userns_user")]
    pub userns_user: String,

    #[serde(default = "default_userns_gid")]
    pub userns_gid: u32,

    #[serde(default = "default_userns_group")]
    pub userns_group: String,

    #[serde(default)]
    pub preload_images: Vec<OciImage>,

    #[serde(default)]
    pub tmp_noexec: bool,
}

impl Default for ActionRunContainerConfig {
    fn default() -> Self {
        Self {
            pull_image_timeout_seconds: default_pull_image_timeout_seconds(),
            unpack_image_timeout_seconds: default_unpack_image_timeout_seconds(),
            userns_uid: default_userns_uid(),
            userns_user: default_userns_user(),
            userns_gid: default_userns_gid(),
            userns_group: default_userns_group(),
            preload_images: Default::default(),
            tmp_noexec: Default::default(),
        }
    }
}

#[inline]
fn default_pull_image_timeout_seconds() -> u64 {
    600
}

#[inline]
fn default_unpack_image_timeout_seconds() -> u64 {
    600
}

#[inline]
fn default_userns_uid() -> u32 {
    users::get_effective_uid()
}

#[inline]
fn default_userns_user() -> String {
    users::get_current_username()
        .expect("Failed to get current username")
        .into_string()
        .expect("Failed to convert the username")
}

#[inline]
fn default_userns_gid() -> u32 {
    users::get_effective_gid()
}

#[inline]
fn default_userns_group() -> String {
    users::get_current_groupname()
        .expect("Failed to get current group name")
        .into_string()
        .expect("Failed to convert the group name")
}
