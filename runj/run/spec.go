package run

import (
	"fmt"
	"os"
	"path/filepath"

	"github.com/darkyzhou/seele/runj/spec"
	"github.com/darkyzhou/seele/runj/utils"
	"github.com/opencontainers/runtime-spec/specs-go"
	"github.com/samber/lo"
	"golang.org/x/sys/unix"
)

var defaultMountPoints = []specs.Mount{
	{
		Source:      "proc",
		Destination: "/proc",
		Type:        "proc",
		Options:     []string{"noexec", "nosuid", "nodev"},
	},
	{
		Source:      "tmpfs",
		Destination: "/dev",
		Type:        "tmpfs",
		Options:     []string{"nosuid", "strictatime", "mode=755", "size=65536k"},
	},
	// TODO: May be not needed
	{
		Source:      "devpts",
		Destination: "/dev/pts",
		Type:        "devpts",
		// Normally a devpts mount point will have a `gid=5` option but in rootless containers it will cause problems
		Options: []string{"nosuid", "noexec", "newinstance", "ptmxmode=0666", "mode=0620"},
	},
	{
		Source:      "shm",
		Destination: "/dev/shm",
		Type:        "tmpfs",
		Options:     []string{"nosuid", "noexec", "nodev", "mode=1777", "size=65536k"},
	},
	{
		Source:      "mqueue",
		Destination: "/dev/mqueue",
		Type:        "mqueue",
		Options:     []string{"nosuid", "noexec", "nodev"},
	},
	{
		Source:      "sysfs",
		Destination: "/sys",
		Type:        "sysfs",
		Options:     []string{"nosuid", "noexec", "nodev", "ro"},
	},
}

var rlimitTypeMap = map[string]int{
	"RLIMIT_AS":         unix.RLIMIT_AS,
	"RLIMIT_CORE":       unix.RLIMIT_CORE,
	"RLIMIT_CPU":        unix.RLIMIT_CPU,
	"RLIMIT_DATA":       unix.RLIMIT_DATA,
	"RLIMIT_FSIZE":      unix.RLIMIT_FSIZE,
	"RLIMIT_LOCKS":      unix.RLIMIT_LOCKS,
	"RLIMIT_MEMLOCK":    unix.RLIMIT_MEMLOCK,
	"RLIMIT_MSGQUEUE":   unix.RLIMIT_MSGQUEUE,
	"RLIMIT_NICE":       unix.RLIMIT_NICE,
	"RLIMIT_NOFILE":     unix.RLIMIT_NOFILE,
	"RLIMIT_NPROC":      unix.RLIMIT_NPROC,
	"RLIMIT_RSS":        unix.RLIMIT_RSS,
	"RLIMIT_RTPRIO":     unix.RLIMIT_RTPRIO,
	"RLIMIT_RTTIME":     unix.RLIMIT_RTTIME,
	"RLIMIT_SIGPENDING": unix.RLIMIT_SIGPENDING,
	"RLIMIT_STACK":      unix.RLIMIT_STACK,
}

func makeContainerSpec(config *spec.RunjConfig) (*specs.Spec, error) {
	var (
		cgroupCpuRules        = &specs.LinuxCPU{}
		cgroupMemRules        = &specs.LinuxMemory{}
		cgroupPidRules        = &specs.LinuxPids{}
		swappiness     uint64 = 0
	)

	// By default, a container should not use swap
	cgroupMemRules.Swappiness = &swappiness

	// Apply the cgroup configurations
	if config.Limits != nil && config.Limits.Cgroup != nil {
		if config.Limits.Cgroup.CpuQuota != 0 {
			cgroupCpuRules.Quota = &config.Limits.Cgroup.CpuQuota
		}
		if config.Limits.Cgroup.CpuShares != 0 {
			cgroupCpuRules.Shares = &config.Limits.Cgroup.CpuShares
		}
		if config.Limits.Cgroup.CpusetCpus != "" {
			cgroupCpuRules.Cpus = config.Limits.Cgroup.CpusetCpus
		}
		if config.Limits.Cgroup.CpusetMems != "" {
			cgroupCpuRules.Mems = config.Limits.Cgroup.CpusetMems
		}
		if config.Limits.Cgroup.Memory != 0 {
			cgroupMemRules.Limit = &config.Limits.Cgroup.Memory
		}
		if config.Limits.Cgroup.MemoryReservation != 0 {
			cgroupMemRules.Reservation = &config.Limits.Cgroup.MemoryReservation
		}
		if config.Limits.Cgroup.MemorySwap != 0 {
			cgroupMemRules.Swap = &config.Limits.Cgroup.MemorySwap
		}
		if config.Limits.Cgroup.PidsLimit != 0 {
			cgroupPidRules.Limit = config.Limits.Cgroup.PidsLimit
		}
	}

	mounts := defaultMountPoints[:]
	for _, mount := range config.Mounts {
		fromPath, err := filepath.Abs(mount.From)
		if err != nil {
			return nil, fmt.Errorf("Failed to resolve the absolute path for %s: %w", mount.From, err)
		}
		toPath := filepath.Join("/", mount.To)
		if utils.FileExists(fromPath) {
			options := append([]string{"bind", "ro", "private"}, mount.Options...)
			if lo.Contains(options, "exec") {
				mask := unix.Umask(0)
				err := os.Chmod(fromPath, 0777)
				unix.Umask(mask)
				if err != nil {
					return nil, fmt.Errorf("Failed to chmod the file %s: %w", fromPath, err)
				}
			}
			mounts = append(mounts, specs.Mount{
				Destination: toPath,
				Type:        "bind",
				Source:      fromPath,
				Options:     options,
			})
		} else if utils.DirectoryExists(fromPath) {
			options := append([]string{"rbind", "ro", "private"}, mount.Options...)
			mounts = append(mounts, specs.Mount{
				Destination: toPath,
				Type:        "rbind",
				Source:      fromPath,
				Options:     options,
			})
		} else {
			return nil, fmt.Errorf("The file to be mounted does not exist: %s", mount.From)
		}
	}

	namespaces := []specs.LinuxNamespace{
		{
			Type: specs.PIDNamespace,
		},
		{
			Type: specs.NetworkNamespace,
		},
		{
			Type: specs.IPCNamespace,
		},
		{
			Type: specs.UTSNamespace,
		},
		{
			Type: specs.MountNamespace,
		},
		{
			Type: specs.CgroupNamespace,
		},
	}
	if config.Rootless {
		namespaces = append(namespaces, specs.LinuxNamespace{
			Type: specs.UserNamespace,
		})
	}

	return &specs.Spec{
		Version: specs.Version,
		Root: &specs.Root{
			Path:     config.Rootfs,
			Readonly: true,
		},
		Hostname: "seele",
		Mounts:   mounts,

		// The actual process to be run will be created manually with libcontainer.Process.
		Process: &specs.Process{
			NoNewPrivileges: true,
		},

		Linux: &specs.Linux{
			UIDMappings: uidMappings,
			GIDMappings: gidMappings,
			MaskedPaths: []string{
				"/proc/acpi",
				"/proc/asound",
				"/proc/kcore",
				"/proc/keys",
				"/proc/latency_stats",
				"/proc/timer_list",
				"/proc/timer_stats",
				"/proc/sched_debug",
				"/sys/firmware",
				"/proc/scsi",
			},
			ReadonlyPaths: []string{
				"/proc/bus",
				"/proc/fs",
				"/proc/irq",
				"/proc/sys",
				"/proc/sysrq-trigger",
			},
			Resources: &specs.LinuxResources{
				CPU:    cgroupCpuRules,
				Memory: cgroupMemRules,
				Pids:   cgroupPidRules,
				// TODO: Maybe we should have rules for device?
			},
			Namespaces: namespaces,
		},
	}, nil
}
