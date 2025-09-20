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
use nix::unistd::{close, dup};
use oci_spec::runtime as oci;
use seele_shared::entities::run_container::runj as ent;
use std::os::fd::{AsFd, IntoRawFd, OwnedFd};

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
    let _unmount_overlay = Unmount(config.overlayfs.merged_dir.clone());

    // 2) Prepare bundle dir (use merged_dir as rootfs, keep bundle outside of rootfs)
    let bundle_dir = tempfile::tempdir().context("Failed creating bundle dir")?;
    let bundle_path = bundle_dir.path();
    // Create bundle rootfs dir and bind-mount the overlay merged_dir into it, then make it private
    let bundle_rootfs = bundle_path.join("rootfs");
    fs::create_dir_all(&bundle_rootfs).context("create bundle rootfs dir")?;
    mount(
        Some(&config.overlayfs.merged_dir),
        &bundle_rootfs,
        None::<&str>,
        MsFlags::MS_BIND | MsFlags::MS_REC,
        None::<&str>,
    )
    .context("bind mount overlay merged_dir to bundle rootfs")?;
    // Make the mount private to avoid propagation issues when libcontainer prepares rootfs
    mount(
        None::<&str>,
        &bundle_rootfs,
        None::<&str>,
        MsFlags::MS_PRIVATE | MsFlags::MS_REC,
        None::<&str>,
    )
    .context("make bundle rootfs mount private")?;
    struct UnmountBind(PathBuf);
    impl Drop for UnmountBind {
        fn drop(&mut self) {
            let _ = umount2(&self.0, MntFlags::MNT_DETACH);
        }
    }
    let _unmount_bind = UnmountBind(bundle_rootfs.clone());
    // State root path for container runtime data
    let state_root = tempfile::tempdir().context("Failed creating state root dir")?;

    // 3) Ensure common mount points and user-defined bind mount destinations exist in rootfs
    ensure_rootfs_mountpoints(&config.overlayfs.merged_dir, &config.mounts)
        .context("prepare mount points in rootfs")?;

    // 4) Generate container id and build OCI spec (attach per-container cgroup dir)
    let container_id = format!("runj-container-{}", nanoid::nanoid!(12));
    // Use relative path "rootfs" inside bundle for OCI spec
    let spec = build_oci_spec(config, Path::new("rootfs"), &container_id)?;
    let spec_json = serde_json::to_vec(&spec).context("Serialize OCI spec")?;
    fs::write(bundle_path.join("config.json"), spec_json).context("Write config.json")?;

    // 5) Build container with libcontainer
    let builder = ContainerBuilder::new(container_id.clone(), SyscallType::default())
        .with_executor(DefaultExecutor {})
        .with_root_path(state_root.path())
        .context("set root path")?
        .validate_id()?
        .as_init(bundle_path)
        .with_systemd(false);

    // NOTE: 当前未通过 libcontainer 提供的 API 定向容器 stdio；默认继承父进程的 stdio。
    // 如果需要将容器 stdout/stderr 重定向到文件，可在后续使用 libcontainer 的 ContainerIO 能力补充实现。

    // 5.1) Configure stdio redirection just for the spawn window so the child inherits desired FDs
    let stdio_guard = StdioRedirectionGuard::setup(config.fd.as_ref())
        .context("setup stdio redirection for container child")?;

    let mut container = builder.build().context("build container")?;
    container.start().context("failed to start container")?;

    // 5.2) Restore our own stdio immediately (child already inherited the redirected FDs)
    drop(stdio_guard);
    let init_pid_raw = container.pid().expect("container pid must exist").as_raw();
    let init_pid = nix::unistd::Pid::from_raw(init_pid_raw);
    // Resolve the actual cgroup path by waiting briefly until memory.current is available
    let cgroup_abs_early: Option<PathBuf> =
        resolve_pid_cgroup_path_with_wait(init_pid, Duration::from_millis(2000));
    let begin = std::time::Instant::now();

    // 6) Wait with timeout
    // 为了与 Go 版本一致，墙时限采用 3x 的放大，避免与“用户 CPU 时间”阈值竞争
    let timeout = Duration::from_millis(config.limits.time_ms.saturating_mul(3));
    let deadline = begin + timeout;
    // Track termination info: either an exit code or a terminating signal
    let mut term_exit_code: Option<i32> = None;
    let mut term_signal: Option<i32> = None;
    let mut user_time_limit_triggered = false;
    let mut mem_peak_bytes_seen: u64 = 0;
    loop {
        let now = std::time::Instant::now();
        let remaining = if deadline > now { deadline - now } else { Duration::from_millis(0) };
        // Check user CPU time usage against limit and kill if exceeded
        if term_exit_code.is_none() && term_signal.is_none() {
            if let Some(ref c_abs) = cgroup_abs_early {
                if let Ok((user_ms, _sys_ms, _mem_kib, _oom)) =
                    collect_cgroup_stats_ext(c_abs.as_path())
                {
                    // 当用户 CPU 时间达到或超过阈值时触发
                    if user_ms >= config.limits.time_ms {
                        let _ = kill(init_pid, Signal::SIGKILL);
                        user_time_limit_triggered = true;
                    }
                }
                // Track memory.current peak while process is alive
                if let Ok(s) = fs::read_to_string(c_abs.join("memory.current")) {
                    if let Ok(v) = s.trim().parse::<u64>() {
                        if v > mem_peak_bytes_seen {
                            mem_peak_bytes_seen = v;
                        }
                    }
                }
            }
        }
        match waitpid(init_pid, Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::Exited(pid, code)) if pid == init_pid => {
                term_exit_code = Some(code as i32);
                break;
            }
            Ok(WaitStatus::Signaled(pid, sig, _)) if pid == init_pid => {
                term_signal = Some(sig as i32);
                break;
            }
            Ok(WaitStatus::StillAlive) | Ok(_) => {
                if remaining.is_zero() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(_e) => {
                // If no child or other transient error, break with unknown
                break;
            }
        }
    }
    let status_killed_by_timeout = term_exit_code.is_none() && term_signal.is_none();
    if status_killed_by_timeout {
        let _ = kill(init_pid, Signal::SIGKILL);
        // Best-effort reap
        let _ = waitpid(init_pid, None::<WaitPidFlag>);
    }
    // 7) Collect stats from the actual cgroup BEFORE deletion (best-effort)
    // Use the path captured at start; if missing, skip metrics.
    let (cpu_user_time_ms, cpu_kernel_time_ms, memory_usage_kib, oom_kill) =
        if let Some(ref p) = cgroup_abs_early {
            let (u, s, m, oom) = collect_cgroup_stats_ext(p.as_path()).unwrap_or((0, 0, 0, false));
            let mem_kib = if mem_peak_bytes_seen > 0 { mem_peak_bytes_seen / 1024 } else { m };
            (u, s, mem_kib, oom)
        } else {
            (0, 0, 0, false)
        };

    // Now cleanup container
    let _ = container.delete(true);

    let mem_limit_bytes_opt = if config.limits.cgroup.memory > 0 {
        Some(config.limits.cgroup.memory as u64)
    } else {
        None
    };
    let oom_inferred = term_signal == Some(Signal::SIGKILL as i32)
        && mem_limit_bytes_opt.map(|lim| mem_peak_bytes_seen >= lim).unwrap_or(false);

    let (exit_code, status_enum, sig_name) = if oom_kill || oom_inferred {
        (-1, ent::ContainerExecutionStatus::MemoryLimitExceeded, None)
    } else if user_time_limit_triggered {
        (-1, ent::ContainerExecutionStatus::UserTimeLimitExceeded, Some("SIGKILL".to_string()))
    } else if status_killed_by_timeout {
        (-1, ent::ContainerExecutionStatus::WallTimeLimitExceeded, Some("SIGKILL".to_string()))
    } else if let Some(sig) = term_signal {
        let sig_name = format_signal(sig);
        (128 + sig, ent::ContainerExecutionStatus::SignalTerminate, Some(sig_name))
    } else if let Some(code) = term_exit_code {
        if oom_kill {
            (code, ent::ContainerExecutionStatus::MemoryLimitExceeded, None)
        } else if code == 0 {
            (code, ent::ContainerExecutionStatus::Normal, None)
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
        4 => "SIGILL",
        5 => "SIGTRAP",
        6 => "SIGABRT",
        8 => "SIGFPE",
        9 => "SIGKILL",
        11 => "SIGSEGV",
        13 => "SIGPIPE",
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

/// Temporarily redirect our process's STDIN/STDOUT/STDERR so the soon-to-be-spawned
/// container init inherits the desired FDs. Restores originals on drop.
struct StdioRedirectionGuard {
    orig_stdin: Option<OwnedFd>,
    orig_stdout: Option<OwnedFd>,
    orig_stderr: Option<OwnedFd>,
}

impl StdioRedirectionGuard {
    fn setup(fd: Option<&ent::FdConfig>) -> Result<Self> {
        // Save originals via dup(0/1/2). If any is invalid (EBADF), keep None and fallback to /dev/null on restore.
        let orig_stdin = nix::unistd::dup(std::io::stdin().as_fd()).ok();
        let orig_stdout = nix::unistd::dup(std::io::stdout().as_fd()).ok();
        let orig_stderr = nix::unistd::dup(std::io::stderr().as_fd()).ok();

        // Decide targets
        let dev_null_path = "/dev/null";
        // stdin
        let stdin_file = match fd.and_then(|f| f.stdin.as_ref()) {
            Some(p) => Some(p.clone()),
            None => None,
        };

        // stdout/stderr with cross redirect logic
        // Combinations:
        // - stdout_to_stderr: both go to stderr target if provided, else inherit default routing
        // - stderr_to_stdout: both go to stdout target if provided
        // If neither provided, each uses its own target or /dev/null by default
        let (stdout_target, stderr_target) = if let Some(f) = fd {
            match (f.stdout.as_ref(), f.stderr.as_ref(), f.stdout_to_stderr, f.stderr_to_stdout) {
                (Some(of), None, false, true) => (Some(of.clone()), Some(of.clone())),
                (None, Some(ef), true, false) => (Some(ef.clone()), Some(ef.clone())),
                (Some(_of), Some(ef), true, false) => (Some(ef.clone()), Some(ef.clone())),
                (Some(of), Some(_ef), false, true) => (Some(of.clone()), Some(of.clone())),
                (so, se, _, _) => (so.cloned(), se.cloned()),
            }
        } else {
            (None, None)
        };

        // Open as needed with correct modes
        // stdin: read-only, create /dev/null fallback
        let stdin_fd = {
            let path = stdin_file.unwrap_or_else(|| PathBuf::from(dev_null_path));
            let file = std::fs::OpenOptions::new()
                .read(true)
                .open(&path)
                .with_context(|| format!("open stdin target {}", path.display()))?;
            file
        };
        // stdout/stderr: write-only, create and truncate
        let open_out = |p: Option<PathBuf>| -> Result<std::fs::File> {
            let path = p.unwrap_or_else(|| PathBuf::from(dev_null_path));
            let file = std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&path)
                .with_context(|| format!("open stdio target {}", path.display()))?;
            Ok(file)
        };
        let stdout_fd = open_out(stdout_target)?;
        let stderr_fd = open_out(stderr_target)?;

        // Replace our stdio so the child inherits them (close -> dup -> leak fd)
        // stdin -> fd 0
        close(0).ok();
        let new0 = dup(stdin_fd.as_fd()).context("dup stdin to 0")?;
        let _ = new0.into_raw_fd(); // leak ownership so fd 0 stays open
        // stdout -> fd 1
        close(1).ok();
        let new1 = dup(stdout_fd.as_fd()).context("dup stdout to 1")?;
        let _ = new1.into_raw_fd();
        // stderr -> fd 2
        close(2).ok();
        let new2 = dup(stderr_fd.as_fd()).context("dup stderr to 2")?;
        let _ = new2.into_raw_fd();

        Ok(Self { orig_stdin, orig_stdout, orig_stderr })
    }
}

impl Drop for StdioRedirectionGuard {
    fn drop(&mut self) {
        // Restore original stdio; fallback to /dev/null if absent
        let dev_null = Path::new("/dev/null");
        // stdin
        let _ = close(0);
        if let Some(ref ofd) = self.orig_stdin {
            if let Ok(fd) = dup(ofd.as_fd()) {
                let _ = fd.into_raw_fd();
            } else {
                if let Ok(f) = std::fs::OpenOptions::new().read(true).open(dev_null) {
                    let _ = f.into_raw_fd();
                }
            }
        } else {
            if let Ok(f) = std::fs::OpenOptions::new().read(true).open(dev_null) {
                let _ = f.into_raw_fd();
            }
        }
        // stdout
        let _ = close(1);
        if let Some(ref ofd) = self.orig_stdout {
            if let Ok(fd) = dup(ofd.as_fd()) {
                let _ = fd.into_raw_fd();
            } else {
                if let Ok(f) = std::fs::OpenOptions::new().write(true).open(dev_null) {
                    let _ = f.into_raw_fd();
                }
            }
        } else {
            if let Ok(f) = std::fs::OpenOptions::new().write(true).open(dev_null) {
                let _ = f.into_raw_fd();
            }
        }
        // stderr
        let _ = close(2);
        if let Some(ref ofd) = self.orig_stderr {
            if let Ok(fd) = dup(ofd.as_fd()) {
                let _ = fd.into_raw_fd();
            } else {
                if let Ok(f) = std::fs::OpenOptions::new().write(true).open(dev_null) {
                    let _ = f.into_raw_fd();
                }
            }
        } else {
            if let Ok(f) = std::fs::OpenOptions::new().write(true).open(dev_null) {
                let _ = f.into_raw_fd();
            }
        }
    }
}

fn build_oci_spec(
    config: &ent::RunjConfig,
    rootfs: &Path,
    _container_id: &str,
) -> Result<oci::Spec> {
    let mut builder = oci::SpecBuilder::default();

    // Root
    let root =
        oci::RootBuilder::default().path(rootfs).readonly(false).build().context("build root")?;
    builder = builder.root(root);

    // Process
    let mut proc_builder = oci::ProcessBuilder::default();
    // 设置 PATH：默认值 + 用户追加
    const DEFAULT_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";
    let mut path_env = String::from("PATH=");
    path_env.push_str(DEFAULT_PATH);
    if let Some(paths) = &config.paths {
        if !paths.is_empty() {
            path_env.push(':');
            path_env.push_str(
                &paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(":"),
            );
        }
    }
    proc_builder = proc_builder.env(vec![path_env]);
    proc_builder = proc_builder.cwd(&config.cwd).no_new_privileges(true);
    // nobody 用户 65534:65534
    proc_builder =
        proc_builder.user(oci::UserBuilder::default().uid(65534u32).gid(65534u32).build().unwrap());
    if !config.command.is_empty() {
        proc_builder = proc_builder.args(config.command.clone());
    }
    // rlimits: map core/fsize/nofile
    let mut rlimits: Vec<oci::PosixRlimit> = Vec::new();
    let rl = &config.limits.rlimit;
    rlimits.push(
        oci::PosixRlimitBuilder::default()
            .typ(oci::PosixRlimitType::RlimitCore)
            .hard(rl.core.hard())
            .soft(rl.core.soft())
            .build()
            .context("rlimit core")?,
    );
    // 与 go 版本行为一致：+1 以更精确地触发 SIGXFSZ
    rlimits.push(
        oci::PosixRlimitBuilder::default()
            .typ(oci::PosixRlimitType::RlimitFsize)
            .hard(rl.fsize.hard().saturating_add(1))
            .soft(rl.fsize.soft().saturating_add(1))
            .build()
            .context("rlimit fsize")?,
    );
    rlimits.push(
        oci::PosixRlimitBuilder::default()
            .typ(oci::PosixRlimitType::RlimitNofile)
            .hard(rl.no_file.hard())
            .soft(rl.no_file.soft())
            .build()
            .context("rlimit nofile")?,
    );
    if !rlimits.is_empty() {
        proc_builder = proc_builder.rlimits(rlimits);
    }
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

    // NOTE: Do not set linux.cgroups_path here. Some hosts have systemd slice-based semantics
    // that cause libcontainer to try to parse the path as a systemd unit and fail.
    // We'll discover the actual v2 cgroup path from /proc/<pid>/cgroup for metrics instead.

    // mounts from config
    let mut mounts = vec![
        oci::MountBuilder::default()
            .destination(Path::new("/proc"))
            .typ("proc")
            .source("proc")
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
                "mode=1777".into(),
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

// cgroup path base helpers removed: we now always discover from /proc/<pid>/cgroup

fn ensure_rootfs_mountpoints(rootfs: &Path, binds: &[ent::MountConfig]) -> Result<()> {
    // Common directories
    let dirs = ["/proc", "/dev", "/dev/pts", "/dev/shm", "/dev/mqueue", "/tmp"];
    for d in dirs {
        fs::create_dir_all(rootfs.join(d.trim_start_matches('/')))?;
    }

    // Ensure bind destinations exist
    for m in binds {
        let to = Path::new("/").join(&m.to);
        let target = rootfs.join(to.strip_prefix("/").unwrap_or(&to));
        if let Ok(meta) = fs::metadata(&m.from) {
            if meta.is_dir() {
                fs::create_dir_all(&target)?;
            } else {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                // create empty file if not present
                if !target.exists() {
                    fs::write(&target, &[])?;
                }
            }
        } else {
            // If source doesn't exist yet, create directory target by default
            fs::create_dir_all(&target)?;
        }
    }
    Ok(())
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

fn pid_cgroup_v2_relative_path(pid: nix::unistd::Pid) -> Option<String> {
    let path = PathBuf::from("/proc").join(pid.as_raw().to_string()).join("cgroup");
    let text = fs::read_to_string(path).ok()?;
    for line in text.lines() {
        // cgroup v2 format: 0::<controllers>:/<relative_path>
        // Usually controllers field is empty for unified v2 hierarchy: 0::/user.slice/...
        if let Some((_hier, rest)) = line.split_once(":") {
            if let Some((_controllers, rel)) = rest.split_once(":") {
                // Ensure rel starts with '/'
                let rel = rel.trim();
                return Some(rel.trim_start_matches('/').to_string());
            }
        }
    }
    None
}

fn resolve_pid_cgroup_path_with_wait(pid: nix::unistd::Pid, wait: Duration) -> Option<PathBuf> {
    let start = std::time::Instant::now();
    loop {
        if let Some(rel) = pid_cgroup_v2_relative_path(pid) {
            let abs = PathBuf::from("/sys/fs/cgroup").join(rel);
            if abs.join("memory.current").exists() {
                return Some(abs);
            }
        }
        if start.elapsed() >= wait {
            return None;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

// (removed) container_cgroup_abs_path: no longer needed as we discover paths from /proc
