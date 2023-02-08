package execute

import (
	"bufio"
	"fmt"
	"os"
	"os/user"
	"strconv"
	"strings"

	"github.com/darkyzhou/seele/runj/cmd/runj/cgroup"
	"github.com/darkyzhou/seele/runj/cmd/runj/entities"
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

func getIdMappings(config *entities.UserNamespaceConfig) ([]specs.LinuxIDMapping, []specs.LinuxIDMapping, error) {
	var (
		uidMappings []specs.LinuxIDMapping
		gidMappings []specs.LinuxIDMapping
	)

	// Mapping 0 -> Geteuid() is required for libcontainer to work properly
	uidMappings = append(uidMappings, specs.LinuxIDMapping{
		HostID:      uint32(os.Geteuid()),
		ContainerID: 0,
		Size:        1,
	})

	// Map uids starting from 1
	subUids, err := findIdMap(config.MapToUser, 1, "/etc/subuid")
	if err != nil {
		return nil, nil, fmt.Errorf("Error initializing the uid map: %w", err)
	}
	uidMappings = append(uidMappings, *subUids)

	// Mapping 0 -> Getegid() is required for libcontainer to work properly
	gidMappings = append(gidMappings, specs.LinuxIDMapping{
		HostID:      uint32(os.Getegid()),
		ContainerID: 0,
		Size:        1,
	})

	// Map gids starting from 1
	subGids, err := findIdMap(config.MapToGroup, 1, "/etc/subgid")
	if err != nil {
		return nil, nil, fmt.Errorf("Error initializing the gid map: %w", err)
	}
	gidMappings = append(gidMappings, *subGids)

	return uidMappings, gidMappings, nil
}

func findIdMap(mapTo string, containerId uint32, path string) (*specs.LinuxIDMapping, error) {
	file, err := os.Open(path)
	if err != nil {
		return nil, fmt.Errorf("Failed to open %s: %w", path, err)
	}
	defer file.Close()

	var target string

	if mapTo == "" {
		u, err := user.Current()
		if err != nil {
			return nil, fmt.Errorf("Failed to get current user: %w", err)
		}
		target = fmt.Sprintf("%s:", u.Username)
	} else {
		target = mapTo
	}

	scanner := bufio.NewScanner(file)
	for scanner.Scan() {
		text := scanner.Text()
		if strings.Contains(text, target) {
			data := strings.SplitN(text, ":", 3)
			if len(data) < 3 {
				return nil, fmt.Errorf("Invalid %s content: %s", path, text)
			}

			id, err := strconv.Atoi(data[1])
			if err != nil {
				return nil, fmt.Errorf("Invalid %s content: %s", path, text)
			}

			size, err := strconv.Atoi(data[2])
			if err != nil {
				return nil, fmt.Errorf("Invalid %s content: %s", path, text)
			}

			return &specs.LinuxIDMapping{
				ContainerID: containerId,
				HostID:      uint32(id),
				Size:        uint32(size),
			}, nil
		}
	}

	if err := scanner.Err(); err != nil {
		return nil, fmt.Errorf("Failed to read %s: %w", path, err)
	}

	return nil, fmt.Errorf("Cannot find user or group %s in %s", target, path)
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
