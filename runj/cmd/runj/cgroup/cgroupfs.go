package cgroup

import (
	"fmt"
	"os"
	"path"

	"github.com/darkyzhou/seele/runj/cmd/runj/utils"
	"github.com/opencontainers/runc/libcontainer/cgroups/fs2"
)

// Initialize a new cgroup v2 directory via cgroupfs.
// Mainly used for containerized environments with the help of sysbox.
func GetCgroupPathViaFs() (string, error) {
	cgroupPath := path.Join(fs2.UnifiedMountpoint, "runj-containers", utils.RunjInstanceId)

	if err := os.MkdirAll(cgroupPath, 0775); err != nil {
		return "", fmt.Errorf("Failed to create cgroup directory: %w", err)
	}

	return cgroupPath, nil
}
