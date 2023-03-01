package execute

import (
	"encoding/json"
	"fmt"
	"os"
	"strconv"
	"strings"

	"github.com/darkyzhou/seele/runj/cmd/runj/cgroup"
	"github.com/darkyzhou/seele/runj/cmd/runj/entities"
	"github.com/darkyzhou/seele/runj/cmd/runj/utils"
	"github.com/opencontainers/runc/libcontainer"
	"github.com/opencontainers/runc/libcontainer/cgroups"
	"github.com/opencontainers/runc/libcontainer/cgroups/fs2"
	"github.com/opencontainers/runtime-spec/specs-go"
	"golang.org/x/sys/unix"
)

func initContainerFactory() (libcontainer.Factory, error) {
	return libcontainer.New(
		".",
		libcontainer.NewuidmapPath("/usr/bin/newuidmap"),
		libcontainer.NewgidmapPath("/usr/bin/newgidmap"),
		libcontainer.InitArgs(os.Args[0], "init"),
	)
}

func getCgroupPath(parentCgroupPath string, rootless bool) (string, string, error) {
	var (
		parentPath     string
		fullCgroupPath string
		err            error
	)

	if parentCgroupPath != "" {
		fullCgroupPath, err = cgroup.GetCgroupPathViaFs(parentCgroupPath)
	} else {
		if rootless {
			parentPath, fullCgroupPath, err = cgroup.GetCgroupPathViaSystemd()
		} else {
			fullCgroupPath, err = cgroup.GetCgroupPathViaFs(fs2.UnifiedMountpoint)
		}
	}

	if err != nil {
		return "", "", err
	}

	return parentPath, fullCgroupPath, nil
}

func getIdMappings(config *entities.UserNamespaceConfig) ([]specs.LinuxIDMapping, []specs.LinuxIDMapping) {
	return []specs.LinuxIDMapping{
			{
				HostID:      config.RootUid,
				ContainerID: 0,
				Size:        1,
			},
			{
				HostID:      config.UidMapBegin,
				ContainerID: 1,
				Size:        config.UidMapCount,
			},
		}, []specs.LinuxIDMapping{
			{
				HostID:      config.RootGid,
				ContainerID: 0,
				Size:        1,
			},
			{
				HostID:      config.GidMapBegin,
				ContainerID: 1,
				Size:        config.GidMapCount,
			},
		}
}

func prepareOverlayfs(config *entities.OverlayfsConfig) (string, error) {
	// FIXME: In seele bare work mode, 'others' bits are not important
	if err := utils.CheckPermission(config.LowerDirectory, 0b000_101_101); err != nil {
		return "", fmt.Errorf("Error checking lower directory's permissions: %w", err)
	}
	if err := utils.CheckPermission(config.UpperDirectory, 0b000_111_111); err != nil {
		return "", fmt.Errorf("Error checking upper directory's permissions: %w", err)
	}
	if err := utils.CheckPermission(config.MergedDirectory, 0b111_000_000); err != nil {
		return "", fmt.Errorf("Error checking merged directory's permissions: %w", err)
	}

	workdirEmpty, err := utils.DirectoryEmpty(config.WorkDirectory)
	if err != nil {
		return "", fmt.Errorf("Error checking work directory: %w", err)
	}
	if !workdirEmpty {
		return "", fmt.Errorf("The workdir is not empty")
	}

	data, err := json.Marshal(config)
	if err != nil {
		return "", fmt.Errorf("Error serializing the config: %w", err)
	}
	return string(data), nil
}

func prepareOutFile(path string) (*os.File, error) {
	modes := os.O_WRONLY | os.O_TRUNC
	if _, err := os.Stat(path); os.IsNotExist(err) {
		modes = modes | os.O_CREATE | os.O_EXCL
	}

	mask := unix.Umask(0)
	file, err := os.OpenFile(path, modes, 0664)
	unix.Umask(mask)
	if err != nil {
		return nil, fmt.Errorf("Error opening the file: %w", err)
	}
	return file, nil
}

func checkIsOOM(cgroupPath string) (bool, error) {
	memoryEvents, err := cgroups.ReadFile(cgroupPath, "memory.events")
	if err != nil {
		return false, fmt.Errorf("Error reading memory events: %w", err)
	}
	index := strings.LastIndex(memoryEvents, "oom_kill")
	// TODO: should handle the case when index+9 is out of bounds
	return index > 0 && memoryEvents[index+9] != '0', nil
}

func readMemoryPeak(cgroupPath string) (uint64, error) {
	data, err := cgroups.ReadFile(cgroupPath, "memory.peak")
	if err != nil {
		return 0, fmt.Errorf("Error reading memory.peak: %w", err)
	}
	memoryUsage, err := strconv.Atoi(strings.Trim(data, "\n "))
	if err != nil || memoryUsage <= 0 {
		return 0, fmt.Errorf("Unexpected memory.peak value: %s", data)
	}

	return uint64(memoryUsage), nil
}
