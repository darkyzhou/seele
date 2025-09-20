use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result};
use libcontainer::container::builder::ContainerBuilder;
use libcontainer::syscall::syscall::SyscallType;
use libcontainer::workload::default::DefaultExecutor;
use nix::mount::{MntFlags, MsFlags, mount, umount2};
use nix::sys::signal::{Signal, kill};
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use oci_spec::runtime as oci;
use seele_shared::entities::run_container::runj as ent;

const CHARS: &[char] = &[
    'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm',
    'n', 'o', 'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z',
    'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M',
    'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z',
];

pub fn execute(config: &ent::RunjConfig) -> Result<ent::ContainerExecutionReport> {
    // 1) Prepare overlayfs mount to merged_dir
    mount_overlayfs(&config.overlayfs)?;

    // Ensure cleanup
    struct Unmount(PathBuf);
    impl Drop for Unmount {
        fn drop(&mut self) {
            let _ = umount2(&self.0, MntFlags::MNT_DETACH);
        }
    }
    let _unmount = Unmount(config.overlayfs.merged_dir.clone());

    // 2) Prepare bundle dir (use merged_dir as rootfs, create a temp config.json next to it)
    let bundle_dir = tempfile::tempdir_in(&config.overlayfs.merged_dir)
        .context("Failed creating bundle dir in merged dir")?;
    let bundle_path = bundle_dir.path();
    let rootfs_path = &config.overlayfs.merged_dir;
    // State root path for container runtime data
    let state_root = tempfile::tempdir().context("Failed creating state root dir")?;

    // 3) Generate container id and build OCI spec (attach per-container cgroup dir)
    let container_id = format!("runj-container-{}", nanoid::nanoid!(12, &CHARS));
    let spec = build_oci_spec(config, rootfs_path, &container_id)?;
    let spec_json = serde_json::to_vec(&spec).context("Serialize OCI spec")?;
    fs::write(bundle_path.join("config.json"), spec_json).context("Write config.json")?;

    // 4) Build container with libcontainer
    let builder = ContainerBuilder::new(container_id.clone(), SyscallType::default())
        .with_executor(DefaultExecutor {})
        .with_root_path(state_root.path())
        .context("set root path")?
        .validate_id()?
        .as_init(bundle_path)
        .with_systemd(false);

    // NOTE: 当前未通过 libcontainer 提供的 API 定向容器 stdio；默认继承父进程的 stdio。
    // 如果需要将容器 stdout/stderr 重定向到文件，可在后续使用 libcontainer 的 ContainerIO 能力补充实现。

    let mut container = builder.build().context("build container")?;
    container.start().context("failed to start container")?;
    let init_pid_raw = container.pid().expect("container pid must exist").as_raw();
    let init_pid = nix::unistd::Pid::from_raw(init_pid_raw);
    let begin = std::time::Instant::now();

    // 5) Wait with timeout
    let timeout = Duration::from_millis(config.limits.time_ms);
    let deadline = begin + timeout;
    let status_code: Option<i32> = loop {
        let now = std::time::Instant::now();
        let remaining = if deadline > now { deadline - now } else { Duration::from_millis(0) };
        match waitpid(init_pid, Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::Exited(pid, code)) if pid == init_pid => break Some(code as i32),
            Ok(WaitStatus::Signaled(pid, sig, _)) if pid == init_pid => break Some(-(sig as i32)),
            Ok(WaitStatus::StillAlive) | Ok(_) => {
                if remaining.is_zero() {
                    break None;
                }
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }
            Err(_e) => {
                // If no child or other transient error, break with unknown
                break None;
            }
        }
    };
    let status_killed_by_timeout = status_code.is_none();
    if status_killed_by_timeout {
        let _ = kill(init_pid, Signal::SIGKILL);
        // Best-effort reap
        let _ = waitpid(init_pid, None::<WaitPidFlag>);
    }
    // 6) Collect stats from per-container cgroup BEFORE deletion (best-effort)
    let cgroup_abs = container_cgroup_abs_path(&config.cgroup_path, &container_id);
    let (cpu_user_time_ms, cpu_kernel_time_ms, memory_usage_kib, oom_kill) =
        collect_cgroup_stats_ext(&cgroup_abs).unwrap_or((0, 0, 0, false));

    // Now cleanup container
    let _ = container.delete(true);

    let (exit_code, status_enum, sig_name) = if status_killed_by_timeout {
        (-1, ent::ContainerExecutionStatus::WallTimeLimitExceeded, Some("SIGKILL".to_string()))
    } else if let Some(code) = status_code {
        if oom_kill {
            (code, ent::ContainerExecutionStatus::MemoryLimitExceeded, None)
        } else if code == 0 {
            (code, ent::ContainerExecutionStatus::Normal, None)
        } else if code < 0 {
            let sig = (-code) as i32;
            (code, ent::ContainerExecutionStatus::SignalTerminate, Some(format_signal(sig)))
        } else {
            (code, ent::ContainerExecutionStatus::RuntimeError, None)
        }
    } else {
        (-1, ent::ContainerExecutionStatus::Unknown, None)
    };

    let wall_time_ms = begin.elapsed().as_millis() as u64;
    Ok(ent::ContainerExecutionReport {
        status: status_enum,
        exit_code: exit_code as i64,
        signal: sig_name,
        wall_time_ms,
        cpu_user_time_ms,
        cpu_kernel_time_ms,
        memory_usage_kib,
    })
}

fn format_signal(sig: i32) -> String {
    match sig {
        1 => "SIGHUP",
        2 => "SIGINT",
        3 => "SIGQUIT",
        9 => "SIGKILL",
        14 => "SIGALRM",
        15 => "SIGTERM",
        _ => return format!("SIG({sig})"),
    }
    .to_string()
}

fn mount_overlayfs(cfg: &ent::OverlayfsConfig) -> Result<()> {
    fs::create_dir_all(&cfg.merged_dir).context("create merged_dir")?;
    // options: userxattr,xino=off,index=off,lowerdir=...,upperdir=...,workdir=...
    let data = format!(
        "userxattr,xino=off,index=off,lowerdir={},upperdir={},workdir={}",
        cfg.lower_dir.display(),
        cfg.upper_dir.display(),
        cfg.work_dir.display()
    );
    mount(
        Some("overlay"),
        &cfg.merged_dir,
        Some("overlay"),
        MsFlags::empty(),
        Some(data.as_bytes()),
    )
    .with_context(|| format!("mount overlayfs to {}", cfg.merged_dir.display()))?;
    Ok(())
}

fn build_oci_spec(
    config: &ent::RunjConfig,
    rootfs: &Path,
    container_id: &str,
) -> Result<oci::Spec> {
    let mut builder = oci::SpecBuilder::default();

    // Root
    let root =
        oci::RootBuilder::default().path(rootfs).readonly(false).build().context("build root")?;
    builder = builder.root(root);

    // Process
    let mut proc_builder = oci::ProcessBuilder::default();
    if let Some(paths) = &config.paths {
        let env_path = format!(
            "PATH={}",
            paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(":")
        );
        proc_builder = proc_builder.env(vec![env_path]);
    }
    proc_builder = proc_builder.cwd(&config.cwd).no_new_privileges(true);
    if !config.command.is_empty() {
        proc_builder = proc_builder.args(config.command.clone());
    }
    // rlimits: TODO - oci-spec mapping for core/fsize/nofile
    let process = proc_builder.build().context("build process")?;
    builder = builder.process(process);

    // Linux + cgroup
    let mut linux = oci::LinuxBuilder::default();
    // user namespace
    // Always set required namespaces
    let mut ns_list = vec![
        oci::LinuxNamespaceBuilder::default().typ(oci::LinuxNamespaceType::Pid).build()?,
        oci::LinuxNamespaceBuilder::default().typ(oci::LinuxNamespaceType::Network).build()?,
        oci::LinuxNamespaceBuilder::default().typ(oci::LinuxNamespaceType::Ipc).build()?,
        oci::LinuxNamespaceBuilder::default().typ(oci::LinuxNamespaceType::Uts).build()?,
        oci::LinuxNamespaceBuilder::default().typ(oci::LinuxNamespaceType::Mount).build()?,
        oci::LinuxNamespaceBuilder::default().typ(oci::LinuxNamespaceType::Cgroup).build()?,
    ];
    if let Some(ns) = &config.user_namespace {
        if ns.enabled {
            let uid_map = vec![
                oci::LinuxIdMappingBuilder::default()
                    .host_id(ns.uid_map_begin as u32)
                    .container_id(0u32)
                    .size(ns.uid_map_count as u32)
                    .build()?,
            ];
            let gid_map = vec![
                oci::LinuxIdMappingBuilder::default()
                    .host_id(ns.gid_map_begin as u32)
                    .container_id(0u32)
                    .size(ns.gid_map_count as u32)
                    .build()?,
            ];
            linux = linux.uid_mappings(uid_map).gid_mappings(gid_map);
            ns_list.insert(
                0,
                oci::LinuxNamespaceBuilder::default().typ(oci::LinuxNamespaceType::User).build()?,
            );
        }
    }
    linux = linux.namespaces(ns_list);

    // resources
    let mut resources = oci::LinuxResourcesBuilder::default();
    // cpu
    if config.limits.cgroup.cpu_shares.is_some()
        || config.limits.cgroup.cpu_quota.is_some()
        || config.limits.cgroup.cpuset_cpus.is_some()
        || config.limits.cgroup.cpuset_mems.is_some()
    {
        let mut cpu = oci::LinuxCpuBuilder::default();
        if let Some(v) = config.limits.cgroup.cpu_shares {
            cpu = cpu.shares(v as u64);
        }
        if let Some(v) = config.limits.cgroup.cpu_quota {
            cpu = cpu.quota(v);
        }
        if let Some(v) = &config.limits.cgroup.cpuset_cpus {
            cpu = cpu.cpus(v);
        }
        if let Some(v) = &config.limits.cgroup.cpuset_mems {
            cpu = cpu.mems(v);
        }
        resources = resources.cpu(cpu.build()?);
    }
    // memory
    resources = resources
        .memory(oci::LinuxMemoryBuilder::default().limit(config.limits.cgroup.memory).build()?);
    // pids
    resources = resources
        .pids(oci::LinuxPidsBuilder::default().limit(config.limits.cgroup.pids_limit).build()?);
    linux = linux.resources(resources.build()?);

    // cgroup path (v2 path relative to /sys/fs/cgroup); attach container_id subdir
    let mut path = strip_cgroup_mount_prefix(&config.cgroup_path).unwrap_or_default();
    if !path.is_empty() {
        path.push('/');
    }

    path.push_str(container_id);
    linux = linux.cgroups_path(path);

    // mounts from config
    let mut mounts = vec![
        oci::MountBuilder::default()
            .destination(Path::new("/proc"))
            .typ("proc")
            .source("proc")
            .build()?,
        oci::MountBuilder::default()
            .destination(Path::new("/sys"))
            .typ("sysfs")
            .source("sysfs")
            .options(vec!["nosuid".into(), "noexec".into(), "nodev".into(), "ro".into()])
            .build()?,
        oci::MountBuilder::default()
            .destination(Path::new("/dev"))
            .typ("tmpfs")
            .source("tmpfs")
            .options(vec![
                "nosuid".into(),
                "strictatime".into(),
                "mode=755".into(),
                "size=65536k".into(),
            ])
            .build()?,
        oci::MountBuilder::default()
            .destination(Path::new("/dev/pts"))
            .typ("devpts")
            .source("devpts")
            .options(vec![
                "nosuid".into(),
                "noexec".into(),
                "newinstance".into(),
                "ptmxmode=0666".into(),
                "mode=0620".into(),
            ])
            .build()?,
        oci::MountBuilder::default()
            .destination(Path::new("/dev/shm"))
            .typ("tmpfs")
            .source("shm")
            .options(vec![
                "nosuid".into(),
                "noexec".into(),
                "nodev".into(),
                "mode=1777".into(),
                "size=65536k".into(),
            ])
            .build()?,
        oci::MountBuilder::default()
            .destination(Path::new("/dev/mqueue"))
            .typ("mqueue")
            .source("mqueue")
            .options(vec!["nosuid".into(), "noexec".into(), "nodev".into()])
            .build()?,
        oci::MountBuilder::default()
            .destination(Path::new("/tmp"))
            .typ("tmpfs")
            .source("tmpfs")
            .options(vec![
                "nosuid".into(),
                "noexec".into(),
                "nodev".into(),
                "size=128m".into(),
                "nr_inodes=4k".into(),
            ])
            .build()?,
    ];
    for m in &config.mounts {
        let from = &m.from;
        let to = Path::new("/").join(&m.to);
        let meta =
            fs::metadata(from).with_context(|| format!("stat mount from {}", from.display()))?;
        let mut options = if meta.is_dir() {
            vec!["rbind".to_string(), "private".to_string()]
        } else {
            vec!["bind".to_string(), "private".to_string()]
        };
        if let Some(extra) = &m.options {
            options.extend(extra.clone());
        }
        mounts.push(
            oci::MountBuilder::default()
                .destination(to)
                .typ("bind")
                .source(from.display().to_string())
                .options(options)
                .build()?,
        );
    }

    let spec = builder
        .linux(linux.build()?)
        .mounts(mounts)
        .hostname("runj")
        .build()
        .context("build spec")?;

    Ok(spec)
}

fn strip_cgroup_mount_prefix(p: &Path) -> Option<String> {
    let s = p.to_string_lossy();
    let pref = "/sys/fs/cgroup";
    if let Some(rest) = s.strip_prefix(pref) {
        Some(rest.trim_start_matches('/').to_string())
    } else {
        None
    }
}

fn collect_cgroup_stats_ext(cgroup_path: &Path) -> Result<(u64, u64, u64, bool)> {
    // v2 paths on host
    let path = if cgroup_path.is_absolute() {
        cgroup_path.to_path_buf()
    } else {
        PathBuf::from("/sys/fs/cgroup").join(cgroup_path)
    };
    let cpu_stat_path = path.join("cpu.stat");
    let mem_current_path = path.join("memory.current");
    let mem_peak_path = path.join("memory.peak");
    let mem_events_path = path.join("memory.events");

    let mut user_ns = 0u64;
    let mut sys_ns = 0u64;
    let mut oom_kill = false;

    if let Ok(text) = fs::read_to_string(cpu_stat_path) {
        for line in text.lines() {
            if let Some((k, v)) = line.split_once(' ') {
                if k == "user_usec" {
                    if let Ok(v) = v.parse::<u64>() {
                        user_ns = v * 1000;
                    }
                } else if k == "system_usec" {
                    if let Ok(v) = v.parse::<u64>() {
                        sys_ns = v * 1000;
                    }
                }
            }
        }
    }

    // Prefer memory.peak (kernel >= 5.19), fallback to current
    let mem_peak_bytes = fs::read_to_string(mem_peak_path)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0);
    let mem_curr_bytes = fs::read_to_string(mem_current_path)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(0);
    let mem_bytes = mem_peak_bytes.max(mem_curr_bytes);

    if let Ok(text) = fs::read_to_string(mem_events_path) {
        for line in text.lines() {
            if let Some((k, v)) = line.split_once(' ') {
                if (k == "oom_kill" || k == "oom") && v.trim() != "0" {
                    oom_kill = true;
                }
            }
        }
    }

    Ok((user_ns / 1_000_000, sys_ns / 1_000_000, mem_bytes / 1024, oom_kill))
}

// TODO: map rlimit from config.limits.rlimit when oci-spec API decided

fn container_cgroup_abs_path(base: &Path, container_id: &str) -> PathBuf {
    let base_abs = if base.is_absolute() {
        base.to_path_buf()
    } else {
        PathBuf::from("/sys/fs/cgroup").join(base)
    };
    base_abs.join(container_id)
}
