package cgroup

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path"
	"path/filepath"
	"strconv"
	"time"

	systemdDbus "github.com/coreos/go-systemd/v22/dbus"
	securejoin "github.com/cyphar/filepath-securejoin"
	"github.com/darkyzhou/seele/runj/cmd/runj/utils"
	dbus "github.com/godbus/dbus/v5"
	"github.com/opencontainers/runc/libcontainer/cgroups"
	"github.com/opencontainers/runc/libcontainer/cgroups/fs2"
	"github.com/sirupsen/logrus"
)

// Initialize a new cgroup v2 directory using systemd.
// Mainly used for bare environments.
func GetCgroupPathViaSystemd() (string, string, error) {
	dbus := newDbusConnManager()

	if err := checkSupportedControllers(); err != nil {
		return "", "", fmt.Errorf("Failed to check supported controllers: %w", err)
	}

	scopeName := fmt.Sprintf("runj-%s.scope", utils.RunjInstanceId)
	if err := initScope(scopeName, dbus); err != nil {
		return "", "", fmt.Errorf("Failed to create the systemd scope: %w", err)
	}

	scopeCgroupPath, err := getPath(scopeName, dbus)
	if err != nil {
		return "", "", fmt.Errorf("Failed to find sub-cgroup path: %w", err)
	}

	mainScopePath := path.Join(scopeCgroupPath, "main.scope")
	if err := os.Mkdir(mainScopePath, 0775); err != nil {
		return "", "", fmt.Errorf("Failed to create main.scope: %w", err)
	}
	if err := cgroups.WriteCgroupProc(mainScopePath, os.Getpid()); err != nil {
		return "", "", fmt.Errorf("Failed to write proc into main.scope: %w", err)
	}
	if err := enableMandatoryControllers(scopeCgroupPath); err != nil {
		return "", "", fmt.Errorf("Failed to init mandatory controllers: %w", err)
	}

	containerScopePath := path.Join(scopeCgroupPath, "container.scope")
	if err := os.Mkdir(containerScopePath, 0775); err != nil {
		return "", "", fmt.Errorf("Failed to create container.slice: %w", err)
	}

	// TODO: Maybe we should toggle memory.oom.group for cgroups inside the slice?
	return scopeCgroupPath, containerScopePath, nil
}

func initScope(unitName string, dbus *dbusConnManager) error {
	properties := []systemdDbus.Property{
		newProp("Delegate", true),
		newProp("DefaultDependencies", false),
		systemdDbus.PropSlice("user.slice"),
		systemdDbus.PropPids(uint32(os.Getpid())),
		systemdDbus.PropDescription("Runj, a powerful container runtime for online judge. Seele is my best girl!"),
	}

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
	return filepath.Join(managerCG, "user.slice"), nil
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
