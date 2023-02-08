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
	var uidMappings []specs.LinuxIDMapping
	{
		{
			user, err := user.Lookup(config.MapToUser)
			if err != nil {
				return nil, nil, fmt.Errorf("Failed to find the user specified in the namespace config: %w", err)
			}
			userId, err := strconv.Atoi(user.Uid)
			if err != nil {
				return nil, nil, fmt.Errorf("Failed to parse the uid: %w", err)
			}

			uidMappings = append(uidMappings, specs.LinuxIDMapping{
				HostID:      uint32(userId),
				ContainerID: 0,
				Size:        1,
			})
		}

		subUids, err := findIdMap(config.MapToUser, 1, "/etc/subuid")
		if err != nil {
			return nil, nil, err
		}

		uidMappings = append(uidMappings, *subUids)
	}

	var gidMappings []specs.LinuxIDMapping
	{
		{
			group, err := user.LookupGroup(config.MapToGroup)
			if err != nil {
				return nil, nil, fmt.Errorf("Failed to find the user specified in the namespace config: %w", err)
			}
			groupId, err := strconv.Atoi(group.Gid)
			if err != nil {
				return nil, nil, fmt.Errorf("Failed to parse the uid: %w", err)
			}

			gidMappings = append(gidMappings, specs.LinuxIDMapping{
				HostID:      uint32(groupId),
				ContainerID: 0,
				Size:        1,
			})
		}

		subGids, err := findIdMap(config.MapToGroup, 1, "/etc/subgid")
		if err != nil {
			return nil, nil, fmt.Errorf("Error initializing the gid map: %w", err)
		}
		gidMappings = append(gidMappings, *subGids)
	}

	return uidMappings, gidMappings, nil
}

func findIdMap(username string, containerId uint32, path string) (*specs.LinuxIDMapping, error) {
	file, err := os.Open(path)
	if err != nil {
		return nil, fmt.Errorf("Failed to open %s: %w", path, err)
	}
	defer file.Close()

	scanner := bufio.NewScanner(file)
	for scanner.Scan() {
		text := scanner.Text()
		if strings.Contains(text, username) {
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

	return nil, fmt.Errorf("Cannot find user or group %s in %s", username, path)
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
