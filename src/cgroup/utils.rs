use std::{fs, path::PathBuf, process};

use anyhow::bail;
use libcgroups::common::DEFAULT_CGROUP_ROOT;

pub fn check_and_get_process_cgroup() -> anyhow::Result<PathBuf> {
    let content = {
        let process_id = format!("{}", process::id());
        fs::read_to_string(["proc", &process_id, "cgroup"].into_iter().collect::<PathBuf>())?
    };
    let content = content.trim();

    if content.is_empty() {
        bail!("Unexpected blank /proc/$$/cgroup content");
    }

    // Refer to https://www.kernel.org/doc/html/latest/admin-guide/cgroup-v2.html#processes
    if !content.starts_with("0::") {
        bail!("Unexpected /proc/$$/cgroup content: {}", content);
    }

    if content.ends_with("(deleted)") {
        bail!("Unexpected /proc/$$/cgroup content, the cgroup is deleted: {}", content);
    }

    let cgroup_path = content.trim_start_matches("0::/");
    Ok([DEFAULT_CGROUP_ROOT, cgroup_path].into_iter().collect())
}
