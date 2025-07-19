use std::{
    fs,
    io::{BufRead, BufReader},
    path::PathBuf,
    process,
    sync::LazyLock,
};

use anyhow::{Context, Result, bail};
use libcgroups::common::{CgroupSetup, get_cgroup_setup, read_cgroup_file, write_cgroup_file_str};
use seele_config as conf;
use seele_shared as shared;

pub use self::utils::*;

#[rustfmt::skip]
#[allow(clippy::all)]
#[allow(unused_imports)]
#[allow(dead_code)]
mod systemd_api;
mod systemd;
mod utils;

const MANDATORY_CONTROLLERS: &str = "+cpu +cpuset +memory +io +pids";

pub static CGROUP_PATH: LazyLock<PathBuf> = LazyLock::new(|| match &conf::CONFIG.work_mode {
    conf::SeeleWorkMode::Bare => {
        systemd::create_and_enter_cgroup().expect("Error entering cgroup scope cgroup")
    }
    _ => utils::check_and_get_self_cgroup().expect("Error getting process' cgroup path"),
});

pub static CGROUP_MAIN_SCOPE_PATH: LazyLock<PathBuf> =
    LazyLock::new(|| CGROUP_PATH.join("main.scope"));

pub static CGROUP_CONTAINER_SLICE_PATH: LazyLock<PathBuf> =
    LazyLock::new(|| CGROUP_PATH.join("container.slice"));

#[inline]
pub fn check_cgroup_setup() -> Result<()> {
    if !matches!(get_cgroup_setup().unwrap(), CgroupSetup::Unified) {
        bail!("Seele only supports cgroup v2");
    }

    Ok(())
}

pub fn initialize_cgroup_subtrees() -> Result<()> {
    fs::create_dir(CGROUP_MAIN_SCOPE_PATH.as_path())?;

    let container_slice_path = CGROUP_PATH.join(CGROUP_CONTAINER_SLICE_PATH.as_path());
    fs::create_dir(&container_slice_path)?;

    let process_id = process::id();
    write_cgroup_file_str(CGROUP_MAIN_SCOPE_PATH.join("cgroup.procs"), &format!("{process_id}"))?;
    if *shared::TINI_PRESENTS {
        write_cgroup_file_str(CGROUP_MAIN_SCOPE_PATH.join("cgroup.procs"), "1")?;
    }

    write_cgroup_file_str(CGROUP_PATH.join("cgroup.subtree_control"), MANDATORY_CONTROLLERS)?;
    write_cgroup_file_str(CGROUP_MAIN_SCOPE_PATH.join("cgroup.subtree_control"), "+cpuset")?;
    write_cgroup_file_str(
        container_slice_path.join("cgroup.subtree_control"),
        MANDATORY_CONTROLLERS,
    )?;

    Ok(())
}

pub fn bind_application_threads() -> Result<()> {
    let available_cpus = {
        let mut cpus: Vec<u32> = vec![];
        let content = read_cgroup_file(CGROUP_MAIN_SCOPE_PATH.join("cpuset.cpus.effective"))?;

        for item in content.trim().split(',') {
            match item.split('-').collect::<Vec<_>>()[..] {
                [from, to] => {
                    let from = from.parse::<u32>()?;
                    let to = to.parse::<u32>()?;
                    cpus.extend(from..=to);
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
        let content = read_cgroup_file(CGROUP_MAIN_SCOPE_PATH.join("cgroup.threads"))?;
        let pids: Vec<u32> = BufReader::new(content.as_bytes())
            .lines()
            .map_while(Result::ok)
            .filter_map(|line| {
                let line = line.trim();
                if *shared::TINI_PRESENTS && line == "1" {
                    None
                } else {
                    Some(line.parse::<u32>().with_context(|| format!("Error parsing line: {line}")))
                }
            })
            .collect::<Result<Vec<_>>>()?;

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
        let cgroup_path = CGROUP_MAIN_SCOPE_PATH.join(format!("thread-{pid}"));
        fs::create_dir(&cgroup_path)?;

        write_cgroup_file_str(cgroup_path.join("cgroup.type"), "threaded")?;
        write_cgroup_file_str(cgroup_path.join("cgroup.threads"), &format!("{pid}"))?;
        write_cgroup_file_str(cgroup_path.join("cpuset.cpus"), &format!("{cpu}"))?;
    }

    Ok(())
}
