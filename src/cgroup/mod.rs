use std::{
    fs,
    io::{BufRead, BufReader},
    path::PathBuf,
};

use anyhow::bail;
use libcgroups::common::{get_cgroup_setup, read_cgroup_file, write_cgroup_file_str, CgroupSetup};
use once_cell::sync::Lazy;
use tracing::debug;

use crate::conf::{self, SeeleWorkMode};

mod systemd;
mod systemd_api;
mod utils;

static CGROUP_PATH: Lazy<PathBuf> = Lazy::new(|| match &conf::CONFIG.work_mode {
    SeeleWorkMode::RootlessBare => {
        systemd::create_and_enter_cgroup().expect("Error entering cgroup scope cgroup")
    }
    SeeleWorkMode::RootlessSystemd | SeeleWorkMode::Privileged => {
        utils::check_and_get_process_cgroup().expect("Error getting process' cgroup path")
    }
});

#[inline]
pub fn check_cgroup_setup() -> anyhow::Result<()> {
    if !matches!(get_cgroup_setup().unwrap(), CgroupSetup::Unified) {
        bail!("Seele only supports cgroup v2");
    }

    Ok(())
}

pub fn initialize_cgroup_subtrees() -> anyhow::Result<()> {
    write_cgroup_file_str(CGROUP_PATH.join("cgroup.subtree_control"), "+cpuset")
}

pub fn bind_app_threads(skip_id: u32) -> anyhow::Result<()> {
    let available_cpus = {
        let mut cpus: Vec<u32> = vec![];
        let content = read_cgroup_file(CGROUP_PATH.join("cpuset.cpus.effective"))?;

        for item in content.trim().split(',') {
            match item.split('-').collect::<Vec<_>>()[..] {
                [from, to] => {
                    let from = from.parse::<u32>()?;
                    let to = to.parse::<u32>()?;
                    cpus.extend((from..=to).into_iter());
                }
                [cpu] => {
                    cpus.push(cpu.parse()?);
                }
                _ => bail!("Unexpected cpuset.cpus.effective item: {}", item),
            }
        }

        if cpus.is_empty() {
            bail!("Unexpected empty cpuset.cpu.effective");
        }

        cpus
    };

    let pids = {
        let content = read_cgroup_file(CGROUP_PATH.join("cgroup.threads"))?;
        let mut pids = vec![];

        for line in BufReader::new(content.as_bytes()).lines().flatten() {
            let pid = line.trim().parse::<u32>()?;
            if pid == skip_id {
                continue;
            }

            pids.push(pid)
        }

        if pids.is_empty() {
            bail!("No pids found in the cgroup.threads");
        }

        pids
    };

    if available_cpus.len() < pids.len() {
        // TODO: Option to disable the check
        bail!(
            "Insufficient available cpus, available: {}, want: {}",
            available_cpus.len(),
            pids.len()
        );
    }

    for (cpu, pid) in available_cpus.into_iter().zip(pids) {
        let cgroup_path = CGROUP_PATH.join(format!("thread-{}", pid));
        fs::create_dir(&cgroup_path)?;

        write_cgroup_file_str(cgroup_path.join("cgroup.type"), "threaded")?;
        write_cgroup_file_str(cgroup_path.join("cgroup.threads"), &format!("{}", pid))?;
        write_cgroup_file_str(cgroup_path.join("cpuset.cpus"), &format!("{}", cpu))?;

        debug!("Bound thread {} to core {}", pid, cpu);
    }

    Ok(())
}
