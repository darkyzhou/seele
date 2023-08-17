package execute

import (
	"fmt"
	"path/filepath"

	"github.com/darkyzhou/seele/runj/cmd/runj/entities"
	"github.com/darkyzhou/seele/runj/cmd/runj/utils"
	"github.com/opencontainers/runtime-spec/specs-go"
)

var defaultMountPoints = []specs.Mount{
	{
		Destination: "/proc",
		Type:        "proc",
		Source:      "proc",
		Options:     nil,
	},
	{
		Destination: "/dev",
		Type:        "tmpfs",
		Source:      "tmpfs",
		Options:     []string{"nosuid", "strictatime", "mode=755", "size=65536k"},
	},
	{
		Destination: "/dev/pts",
		Type:        "devpts",
		Source:      "devpts",
		Options:     []string{"nosuid", "noexec", "newinstance", "ptmxmode=0666", "mode=0620"},
	},
	{
		Destination: "/dev/shm",
		Type:        "tmpfs",
		Source:      "shm",
		Options:     []string{"nosuid", "noexec", "nodev", "mode=1777", "size=65536k"},
	},
	{
		Destination: "/dev/mqueue",
		Type:        "mqueue",
		Source:      "mqueue",
		Options:     []string{"nosuid", "noexec", "nodev"},
	},
	{
		Destination: "/sys",
		Type:        "sysfs",
		Source:      "sysfs",
		Options:     []string{"nosuid", "noexec", "nodev", "ro"},
	},
	{
		Destination: "/tmp",
		Type:        "tmpfs",
		Source:      "tmpfs",
		Options:     []string{"nosuid", "nodev"},
	},
}

func makeContainerSpec(config *entities.RunjConfig, uidMappings []specs.LinuxIDMapping, gidMappings []specs.LinuxIDMapping) (*specs.Spec, error) {
	var (
		cgroupCpuRules = &specs.LinuxCPU{}
		cgroupMemRules = &specs.LinuxMemory{}
		cgroupPidRules = &specs.LinuxPids{}
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

		// Limit = Swap means always disable swap
		cgroupMemRules.Limit = &config.Limits.Cgroup.Memory
		cgroupMemRules.Swap = &config.Limits.Cgroup.Memory

		cgroupPidRules.Limit = config.Limits.Cgroup.PidsLimit
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
	if config.UserNamespace != nil && config.UserNamespace.Enabled {
		namespaces = append(namespaces, specs.LinuxNamespace{
			Type: specs.UserNamespace,
		})
	}

	return &specs.Spec{
		Version: specs.Version,
		Root: &specs.Root{
			Path:     config.Overlayfs.MergedDirectory,
			Readonly: false,
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
			},
			Namespaces: namespaces,
		},
	}, nil
}
