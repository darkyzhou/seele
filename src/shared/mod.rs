#[inline]
pub fn random_task_id() -> String {
    nano_id::base62::<8>()
}
