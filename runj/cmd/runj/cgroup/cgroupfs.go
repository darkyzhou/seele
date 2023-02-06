package cgroup

import (
	"github.com/opencontainers/runc/libcontainer/cgroups/fs2"
)

// Initialize a new cgroup v2 directory via cgroupfs.
// Mainly used for containerized environments with the help of sysbox.
func GetCgroupPathViaFs() (string, error) {
	return fs2.UnifiedMountpoint, nil
}
