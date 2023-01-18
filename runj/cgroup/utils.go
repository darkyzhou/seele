package cgroup

import (
	"fmt"
	"strings"

	"github.com/opencontainers/runc/libcontainer/cgroups"
	"github.com/opencontainers/runc/libcontainer/cgroups/fs2"
	"github.com/samber/lo"
)

var mandatoryControllers = []string{"cpu", "cpuset", "memory"}

func checkSupportedControllers() error {
	controllers, err := cgroups.ReadFile(fs2.UnifiedMountpoint, "/cgroup.controllers")
	if err != nil {
		return fmt.Errorf("Error reading cgroup.controllers: %w", err)
	}

	if supported := lo.Intersect(strings.Fields(controllers), mandatoryControllers); len(supported) < len(mandatoryControllers) {
		return fmt.Errorf("Missing some cgroup controllers, available controllers: %s", controllers)
	}

	return nil
}

func initMandatoryControllers(path string) error {
	for _, controller := range mandatoryControllers {
		if err := cgroups.WriteFile(path, "cgroup.subtree_control", "+"+controller); err != nil {
			return fmt.Errorf("Failed to enable %s controller via cgroup.subtree_control: %w", controller, err)
		}
	}

	return nil
}
