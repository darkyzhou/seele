package cgroup

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"

	systemdDbus "github.com/coreos/go-systemd/v22/dbus"
	securejoin "github.com/cyphar/filepath-securejoin"
	dbus "github.com/godbus/dbus/v5"
	gonanoid "github.com/matoous/go-nanoid/v2"
	"github.com/opencontainers/runc/libcontainer/cgroups"
	"github.com/opencontainers/runc/libcontainer/cgroups/fs2"
	"github.com/samber/lo"
	"github.com/sirupsen/logrus"
)

var id = gonanoid.MustGenerate("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ", 8)

func InitSubCgroupV2() (string, error) {
	dbus := newDbusConnManager()

	content, err := supportedControllers()
	if err != nil {
		return "", fmt.Errorf("Failed to get supported controllers: %w", err)
	}
	availableControllers := strings.Fields(content)

	if count := lo.CountBy(availableControllers, func(controller string) bool {
		return controller == "cpu" || controller == "cpuset" || controller == "memory"
	}); count < 3 {
		return "", fmt.Errorf("Missing cgroup controller, available controllers: %s", content)
	}

	if err := initSlice("seele.slice", dbus); err != nil {
		return "", fmt.Errorf("Failed to create systemd slice: %w", err)
	}

	if err := initScope(fmt.Sprintf("seele-%s.scope", id), dbus); err != nil {
		return "", fmt.Errorf("Failed to create systemd scope: %w", err)
	}

	slicePath, err := getPath("seele.slice", dbus)
	if err != nil {
		return "", fmt.Errorf("Failed to find sub-cgroup path: %w", err)
	}

	// Manually initialize subtree_control with all available controllers
	for _, controller := range []string{"cpu", "cpuset", "memory"} {
		if err := cgroups.WriteFile(slicePath, "cgroup.subtree_control", "+"+controller); err != nil {
			return "", fmt.Errorf("Failed to write the cgroup controller to cgroup.subtree_control: %w", err)
		}
	}

	// TODO: Maybe we should toggle memory.oom.group for cgroups inside the slice?

	return slicePath, nil
}

func initSlice(unitName string, dbus *dbusConnManager) error {
	var properties []systemdDbus.Property

	// There is no need to worry about creating duplicate units
	// because `startUnit` already handles that case.
	properties = append(properties, systemdDbus.PropDescription("Seele containers"))
	properties = append(properties, newProp("MemoryAccounting", true), newProp("CPUAccounting", true), newProp("IOAccounting", true), newProp("TasksAccounting", true))
	properties = append(properties, newProp("DefaultDependencies", false))

	if err := startUnit(dbus, unitName, properties); err != nil {
		return fmt.Errorf("Failed to start unit %q (properties %+v): %w", unitName, properties, err)
	}

	return nil
}

func initScope(unitName string, dbus *dbusConnManager) error {
	var properties []systemdDbus.Property

	// There is no need to worry about creating duplicate units
	// because `startUnit` already handles that case.
	properties = append(properties, systemdDbus.PropDescription(fmt.Sprintf("Seele run-container action %s", id)))
	properties = append(properties, systemdDbus.PropSlice("user.slice"))
	properties = append(properties, newProp("Delegate", true))
	properties = append(properties, newProp("PIDs", []uint32{uint32(os.Getpid())}))
	properties = append(properties, newProp("DefaultDependencies", false))

	if err := startUnit(dbus, unitName, properties); err != nil {
		return fmt.Errorf("Failed to start unit %q (properties %+v): %w", unitName, properties, err)
	}

	return nil
}

// Following codes are modified based on github.com/opencontainers/runc under Apache License V2.0.
// Copyright 2014 Docker, Inc.

func newProp(name string, units interface{}) systemdDbus.Property {
	return systemdDbus.Property{
		Name:  name,
		Value: dbus.MakeVariant(units),
	}
}

func supportedControllers() (string, error) {
	return cgroups.ReadFile(fs2.UnifiedMountpoint, "/cgroup.controllers")
}

func getPath(unitName string, cm *dbusConnManager) (string, error) {
	sliceFull, err := getSliceFull(cm)
	if err != nil {
		return "", err
	}
	path := filepath.Join(sliceFull, unitName)
	path, err = securejoin.SecureJoin(fs2.UnifiedMountpoint, path)
	if err != nil {
		return "", err
	}
	return path, err
}

func getSliceFull(cm *dbusConnManager) (string, error) {
	managerCG, err := getManagerProperty(cm, "ControlGroup")
	if err != nil {
		return "", err
	}
	return managerCG, nil
}

func getManagerProperty(cm *dbusConnManager, name string) (string, error) {
	str := ""
	err := cm.retryOnDisconnect(func(c *systemdDbus.Conn) error {
		var err error
		str, err = c.GetManagerProperty(name)
		return err
	})
	if err != nil {
		return "", err
	}
	return strconv.Unquote(str)
}

func startUnit(cm *dbusConnManager, unitName string, properties []systemdDbus.Property) error {
	statusChan := make(chan string, 1)
	err := cm.retryOnDisconnect(func(c *systemdDbus.Conn) error {
		_, err := c.StartTransientUnitContext(context.TODO(), unitName, "replace", properties, statusChan)
		return err
	})
	if err == nil {
		timeout := time.NewTimer(30 * time.Second)
		defer timeout.Stop()

		select {
		case s := <-statusChan:
			close(statusChan)
			// Please refer to https://pkg.go.dev/github.com/coreos/go-systemd/v22/dbus#Conn.StartUnit
			if s != "done" {
				resetFailedUnit(cm, unitName)
				return fmt.Errorf("Error creating systemd unit `%s`: got `%s`", unitName, s)
			}
		case <-timeout.C:
			resetFailedUnit(cm, unitName)
			return errors.New("Timeout waiting for systemd to create " + unitName)
		}
	} else if !isUnitExists(err) {
		return err
	}

	return nil
}

func resetFailedUnit(cm *dbusConnManager, name string) {
	err := cm.retryOnDisconnect(func(c *systemdDbus.Conn) error {
		return c.ResetFailedUnitContext(context.TODO(), name)
	})
	if err != nil {
		logrus.WithError(err).Warn("Failed to reset failed unit")
	}
}

// isUnitExists returns true if the error is that a systemd unit already exists.
func isUnitExists(err error) bool {
	return isDbusError(err, "org.freedesktop.systemd1.UnitExists")
}
