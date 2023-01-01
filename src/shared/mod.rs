pub mod cond_group;
pub mod file_utils;
pub mod oci_image;

#[inline]
pub fn random_task_id() -> String {
    nano_id::base62::<8>()
}
