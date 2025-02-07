use std::{fs, path::PathBuf};

use anyhow::{Result, bail};
use libcgroups::common::{DEFAULT_CGROUP_ROOT, read_cgroup_file};

pub fn check_and_get_self_cgroup() -> Result<PathBuf> {
    let content = fs::read_to_string("/proc/thread-self/cgroup")?;
    let content = content.trim();

    if content.is_empty() {
        bail!("Unexpected blank /proc/thread-self/cgroup content");
    }

    // Refer to https://www.kernel.org/doc/html/latest/admin-guide/cgroup-v2.html#processes
    if !content.starts_with("0::") {
        bail!("Unexpected /proc/thread-self/cgroup content: {}", content);
    }

    if content.ends_with("(deleted)") {
        bail!("Unexpected /proc/thread-self/cgroup content, the cgroup is deleted: {}", content);
    }

    if content == "0::/" {
        return Ok(DEFAULT_CGROUP_ROOT.into());
    }

    let cgroup_path = content.trim_start_matches("0::/");
    Ok([DEFAULT_CGROUP_ROOT, cgroup_path].into_iter().collect())
}

pub fn get_self_cpuset_cpu() -> Result<i64> {
    let path = check_and_get_self_cgroup()?;
    let content = read_cgroup_file(path.join("cpuset.cpus"))?;
    tracing::debug!("cpuset.cpus path: {:?}", path.join("cpuset.cpus"));
    match content.trim().parse() {
        Ok(cpu) => Ok(cpu),
        Err(_err) => bail!("Error parsing cpuset.cpus content: {:?}", content),
    }
}
