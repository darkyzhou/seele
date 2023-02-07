package execute

import (
	"fmt"
	"os"
	"path/filepath"

	"github.com/darkyzhou/seele/runj/cmd/runj/entities"
	"github.com/darkyzhou/seele/runj/cmd/runj/utils"
	"github.com/opencontainers/runc/libcontainer/configs"
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

var defaultRlimitRules = []configs.Rlimit{
	{
		Type: unix.RLIMIT_FSIZE,
		Hard: 256 * 1024 * 1024, // 256 MiB
		Soft: 256 * 1024 * 1024,
	},
	{
		Type: unix.RLIMIT_NOFILE,
		Hard: 256,
		Soft: 256,
	},
	{
		Type: unix.RLIMIT_CORE,
		Hard: 0,
		Soft: 0,
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

var defaultMemoryLimitBytes int64 = 512 * 1024 * 1024 // 512 MiB
var defaultSwappiness uint64 = 0
var defaultPidsLimit int64 = 64

func makeContainerSpec(config *entities.RunjConfig) (*specs.Spec, error) {
	var (
		cgroupCpuRules = &specs.LinuxCPU{}
		cgroupMemRules = &specs.LinuxMemory{
			Limit:      &defaultMemoryLimitBytes,
			Swap:       &defaultMemoryLimitBytes,
			Swappiness: &defaultSwappiness,
		}
		cgroupPidRules = &specs.LinuxPids{
			Limit: defaultPidsLimit,
		}
	)

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
			options := append([]string{"bind", "private"}, mount.Options...)

			if lo.Contains(options, "exec") {
				// FIXME: Runj should not do this
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
			options := append([]string{"rbind", "private"}, mount.Options...)
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
