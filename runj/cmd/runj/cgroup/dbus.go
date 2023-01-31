package cgroup

import (
	"bufio"
	"bytes"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"sync"

	systemdDbus "github.com/coreos/go-systemd/v22/dbus"
	dbus "github.com/godbus/dbus/v5"
	"github.com/opencontainers/runc/libcontainer/userns"
)

// Following codes are from github.com/opencontainers/runc under Apache License V2.0.
// Copyright 2014 Docker, Inc.

var (
	dbusC  *systemdDbus.Conn
	dbusMu sync.RWMutex
)

type dbusConnManager struct{}

// newDbusConnManager initializes systemd dbus connection manager.
func newDbusConnManager() *dbusConnManager {
	dbusMu.Lock()
	defer dbusMu.Unlock()
	return &dbusConnManager{}
}

// getConnection lazily initializes and returns systemd dbus connection.
func (d *dbusConnManager) getConnection() (*systemdDbus.Conn, error) {
	// In the case where dbusC != nil
	// Use the read lock the first time to ensure
	// that Conn can be acquired at the same time.
	dbusMu.RLock()
	if conn := dbusC; conn != nil {
		dbusMu.RUnlock()
		return conn, nil
	}
	dbusMu.RUnlock()

	// In the case where dbusC == nil
	// Use write lock to ensure that only one
	// will be created
	dbusMu.Lock()
	defer dbusMu.Unlock()
	if conn := dbusC; conn != nil {
		return conn, nil
	}

	conn, err := d.newConnection()
	if err != nil {
		// When dbus-user-session is not installed, we can't detect whether we should try to connect to user dbus or system dbus, so d.dbusRootless is set to false.
		// This may fail with a cryptic error "read unix @->/run/systemd/private: read: connection reset by peer: unknown."
		// https://github.com/moby/moby/issues/42793
		return nil, fmt.Errorf("Failed to connect to dbus (hint: for rootless containers, maybe you need to install dbus-user-session package, see https://github.com/opencontainers/runc/blob/master/docs/cgroup-v2.md): %w", err)
	}
	dbusC = conn
	return conn, nil
}

func (d *dbusConnManager) newConnection() (*systemdDbus.Conn, error) {
	return newUserSystemdDbus()
}

// resetConnection resets the connection to its initial state
// (so it can be reconnected if necessary).
func (d *dbusConnManager) resetConnection(conn *systemdDbus.Conn) {
	dbusMu.Lock()
	defer dbusMu.Unlock()
	if dbusC != nil && dbusC == conn {
		dbusC.Close()
		dbusC = nil
	}
}

var errDbusConnClosed = dbus.ErrClosed.Error()

// retryOnDisconnect calls op, and if the error it returns is about closed dbus
// connection, the connection is re-established and the op is retried. This helps
// with the situation when dbus is restarted and we have a stale connection.
func (d *dbusConnManager) retryOnDisconnect(op func(*systemdDbus.Conn) error) error {
	for {
		conn, err := d.getConnection()
		if err != nil {
			return err
		}
		err = op(conn)
		if !isDbusError(err, errDbusConnClosed) {
			return err
		}
		d.resetConnection(conn)
	}
}

// isDbusError returns true if the error is a specific dbus error.
func isDbusError(err error, name string) bool {
	if err != nil {
		var derr dbus.Error
		if errors.As(err, &derr) {
			return strings.Contains(derr.Name, name)
		}
	}
	return false
}

// newUserSystemdDbus creates a connection for systemd user-instance.
func newUserSystemdDbus() (*systemdDbus.Conn, error) {
	addr, err := detectUserDbusSessionBusAddress()
	if err != nil {
		return nil, err
	}
	uid, err := detectUID()
	if err != nil {
		return nil, err
	}

	return systemdDbus.NewConnection(func() (*dbus.Conn, error) {
		conn, err := dbus.Dial(addr)
		if err != nil {
			return nil, fmt.Errorf("Error while dialing %q: %w", addr, err)
		}
		methods := []dbus.Auth{dbus.AuthExternal(strconv.Itoa(uid))}
		err = conn.Auth(methods)
		if err != nil {
			conn.Close()
			return nil, fmt.Errorf("Error while authenticating connection (address=%q, UID=%d): %w", addr, uid, err)
		}
		if err = conn.Hello(); err != nil {
			conn.Close()
			return nil, fmt.Errorf("Error while sending Hello message (address=%q, UID=%d): %w", addr, uid, err)
		}
		return conn, nil
	})
}

// detectUID detects UID from the OwnerUID field of `busctl --user status`
// if running in userNS. The value corresponds to sd_bus_creds_get_owner_uid(3) .
//
// Otherwise returns os.Getuid() .
func detectUID() (int, error) {
	if !userns.RunningInUserNS() {
		return os.Getuid(), nil
	}
	b, err := exec.Command("busctl", "--user", "--no-pager", "status").CombinedOutput()
	if err != nil {
		return -1, fmt.Errorf("could not execute `busctl --user --no-pager status` (output: %q): %w", string(b), err)
	}
	scanner := bufio.NewScanner(bytes.NewReader(b))
	for scanner.Scan() {
		s := strings.TrimSpace(scanner.Text())
		if strings.HasPrefix(s, "OwnerUID=") {
			uidStr := strings.TrimPrefix(s, "OwnerUID=")
			i, err := strconv.Atoi(uidStr)
			if err != nil {
				return -1, fmt.Errorf("could not detect the OwnerUID: %w", err)
			}
			return i, nil
		}
	}
	if err := scanner.Err(); err != nil {
		return -1, err
	}
	return -1, errors.New("could not detect the OwnerUID")
}

// detectUserDbusSessionBusAddress returns $DBUS_SESSION_BUS_ADDRESS if set.
// Otherwise returns "unix:path=$XDG_RUNTIME_DIR/bus" if $XDG_RUNTIME_DIR/bus exists.
// Otherwise parses the value from `systemctl --user show-environment` .
func detectUserDbusSessionBusAddress() (string, error) {
	if env := os.Getenv("DBUS_SESSION_BUS_ADDRESS"); env != "" {
		return env, nil
	}
	if xdr := os.Getenv("XDG_RUNTIME_DIR"); xdr != "" {
		busPath := filepath.Join(xdr, "bus")
		if _, err := os.Stat(busPath); err == nil {
			busAddress := "unix:path=" + busPath
			return busAddress, nil
		}
	}
	b, err := exec.Command("systemctl", "--user", "--no-pager", "show-environment").CombinedOutput()
	if err != nil {
		return "", fmt.Errorf("could not execute `systemctl --user --no-pager show-environment` (output=%q): %w", string(b), err)
	}
	scanner := bufio.NewScanner(bytes.NewReader(b))
	for scanner.Scan() {
		s := strings.TrimSpace(scanner.Text())
		if strings.HasPrefix(s, "DBUS_SESSION_BUS_ADDRESS=") {
			return strings.TrimPrefix(s, "DBUS_SESSION_BUS_ADDRESS="), nil
		}
	}
	return "", errors.New("could not detect DBUS_SESSION_BUS_ADDRESS from `systemctl --user --no-pager show-environment`. Make sure you have installed the dbus-user-session or dbus-daemon package and then run: `systemctl --user start dbus`")
}
