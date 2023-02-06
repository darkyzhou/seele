package cgroup

import (
	"fmt"
	"os"
	"path"

	"github.com/darkyzhou/seele/runj/cmd/runj/utils"
)

// Initialize a new cgroup v2 directory via cgroupfs.
// Mainly used for containerized environments with the help of sysbox.
func GetCgroupPathViaFs(parentCgroupPath string) (string, error) {
	cgroupPath := path.Join(parentCgroupPath, fmt.Sprintf("runj-container-%s", utils.RunjInstanceId))

	if err := os.Mkdir(cgroupPath, 0775); err != nil {
		return "", fmt.Errorf("Failed to create cgroup directory: %w", err)
	}

	return cgroupPath, nil
}
