package run

import (
	"bufio"
	"fmt"
	"os"
	"os/user"
	"strconv"
	"strings"

	"github.com/darkyzhou/seele/runj/cmd/runj/cgroup"
	gonanoid "github.com/matoous/go-nanoid/v2"
	"github.com/opencontainers/runc/libcontainer"
	"github.com/opencontainers/runc/libcontainer/cgroups"
	"github.com/opencontainers/runtime-spec/specs-go"
	"golang.org/x/sys/unix"
)

func prepareContainerFactory() error {
	if factory != nil {
		return nil
	}

	var err error
	factory, err = libcontainer.New(
		".",
		libcontainer.NewuidmapPath("/usr/bin/newuidmap"),
		libcontainer.NewgidmapPath("/usr/bin/newgidmap"),
		libcontainer.InitArgs(os.Args[0], "init"),
	)
	return err
}

func prepareCgroupPath(useSystemdCgroupDriver bool) error {
	if cgroupPath != "" {
		return nil
	}

	var err error
	if useSystemdCgroupDriver {
		cgroupPath, err = cgroup.InitSystemdCgroup()
	} else {
		cgroupPath, err = cgroup.InitFsCgroup()
	}
	return err
}

func prepareIdMaps() error {
	if uidMappings == nil {
		// Mapping 0 -> Geteuid() is required for libcontainer to work properly
		uidMappings = append(uidMappings, specs.LinuxIDMapping{
			HostID:      uint32(os.Geteuid()),
			ContainerID: 0,
			Size:        1,
		})

		// Map uids starting from 1
		m, err := findIdMap(1, "/etc/subuid")
		if err != nil {
			return fmt.Errorf("Error initializing the uid map: %w", err)
		}
		uidMappings = append(uidMappings, *m)
	}

	if gidMappings == nil {
		// Mapping 0 -> Getegid() is required for libcontainer to work properly
		gidMappings = append(gidMappings, specs.LinuxIDMapping{
			HostID:      uint32(os.Getegid()),
			ContainerID: 0,
			Size:        1,
		})

		// Map gids starting from 1
		m, err := findIdMap(1, "/etc/subgid")
		if err != nil {
			return fmt.Errorf("Error initializing the gid map: %w", err)
		}
		gidMappings = append(gidMappings, *m)
	}

	return nil
}

func findIdMap(containerId uint32, path string) (*specs.LinuxIDMapping, error) {
	file, err := os.Open(path)
	if err != nil {
		return nil, fmt.Errorf("Failed to open %s: %w", path, err)
	}
	defer file.Close()

	u, err := user.Current()
	if err != nil {
		return nil, fmt.Errorf("Failed to get current user: %w", err)
	}
	target := fmt.Sprintf("%s:", u.Username)

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

	return nil, fmt.Errorf("Cannot find current user %s in %s", u.Username, path)
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

func makeContainerId() string {
	id := gonanoid.MustGenerate("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ", 12)
	return fmt.Sprintf("seele-%s", id)
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
