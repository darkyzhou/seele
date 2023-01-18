package run

import (
	"bufio"
	"bytes"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/darkyzhou/seele/runj/spec"
	"github.com/opencontainers/runc/libcontainer"
	"github.com/opencontainers/runc/libcontainer/cgroups/fs2"
	"github.com/opencontainers/runc/libcontainer/configs"
	"github.com/opencontainers/runc/libcontainer/specconv"
	"github.com/opencontainers/runtime-spec/specs-go"
	"github.com/samber/lo"
	"github.com/sirupsen/logrus"
	"golang.org/x/sys/unix"
)

var (
	factory     libcontainer.Factory
	cgroupPath  = ""
	uidMappings []specs.LinuxIDMapping
	gidMappings []specs.LinuxIDMapping
)

func RunContainer(config *spec.RunjConfig) (*spec.ExecutionReport, error) {
	if err := prepareContainerFactory(); err != nil {
		return nil, fmt.Errorf("Error preparing container factory: %w", err)
	}

	if err := prepareCgroupPath(config.Rootless); err != nil {
		return nil, fmt.Errorf("Error preparing cgroup path: %w", err)
	}

	if config.Rootless {
		if err := prepareIdMaps(); err != nil {
			return nil, fmt.Errorf("Error preparing id maps: %w", err)
		}
	}

	spec, err := makeContainerSpec(config)
	if err != nil {
		return nil, fmt.Errorf("Error making container specification: %w", err)
	}

	containerId := makeContainerId()

	fullCgroupPath := filepath.Join(cgroupPath, containerId)
	if err := os.Mkdir(fullCgroupPath, 0770); err != nil {
		return nil, fmt.Errorf("Error creating cgroup directory: %w", err)
	}

	cfg, err := specconv.CreateLibcontainerConfig(&specconv.CreateOpts{
		UseSystemdCgroup: false,
		Spec:             spec,
		RootlessEUID:     config.Rootless,
		RootlessCgroups:  config.Rootless,
	})
	if err != nil {
		return nil, fmt.Errorf("Error creating libcontainer config: %w", err)
	}
	// This is mandatory for libcontainer to correctly handle cgroup path
	cfg.Cgroups.Path = strings.Replace(fullCgroupPath, fs2.UnifiedMountpoint, "", 1)

	container, err := factory.Create(containerId, cfg)
	if err != nil {
		return nil, fmt.Errorf("Error creating container instance: %w", err)
	}
	defer func() {
		if err := container.Destroy(); err != nil {
			logrus.WithError(err).Warn("Error destroying container instance")
		}
	}()

	var (
		stdInFile  *os.File
		stdOutFile *os.File
		stdErrFile *os.File
	)
	if config.Fd != nil {
		if config.Fd.StdIn != "" {
			stdInFile, err = os.Open(config.Fd.StdIn)
			if err != nil {
				return nil, fmt.Errorf("Error opening the stdin file: %w", err)
			}
			defer stdInFile.Close()
		}

		stdOutFilePath := lo.Ternary(config.Fd.StdOut == "", "/dev/null", config.Fd.StdOut)
		stdOutFile, err = os.OpenFile(stdOutFilePath, os.O_CREATE|os.O_WRONLY, 0660)
		if err != nil {
			return nil, fmt.Errorf("Error opening the stdout file %s: %w", stdOutFilePath, err)
		}
		defer stdOutFile.Close()

		stdErrFilePath := lo.Ternary(config.Fd.StdErr == "", "/dev/null", config.Fd.StdErr)
		stdErrFile, err = os.OpenFile(stdErrFilePath, os.O_CREATE|os.O_WRONLY, 0660)
		if err != nil {
			return nil, fmt.Errorf("Error opening the stderr file %s: %w", stdErrFilePath, err)
		}
		defer stdErrFile.Close()
	}

	stdInR, stdInW, err := os.Pipe()
	if err != nil {
		return nil, fmt.Errorf("Error creating the stdin pipe: %w", err)
	}
	stdOutR, stdOutW, err := os.Pipe()
	if err != nil {
		return nil, fmt.Errorf("Error creating the stdout pipe: %w", err)
	}
	stdOutReader := bufio.NewReader(stdOutR)
	var stdErrBuffer bytes.Buffer
	defer func() {
		_ = stdInR.Close()
		_ = stdInW.Close()
		_ = stdOutR.Close()
		_ = stdOutW.Close()
	}()

	var timeLimit uint64
	if config.Limits != nil && config.Limits.Time != nil {
		if config.Limits.Time.WallLimitMs != 0 {
			timeLimit = config.Limits.Time.WallLimitMs
		} else {
			if config.Limits.Time.KernelLimitMs != 0 || config.Limits.Time.UserLimitMs != 0 {
				timeLimit = config.Limits.Time.KernelLimitMs + config.Limits.Time.UserLimitMs
			}
		}
	}

	rlimits := defaultRlimitRules[:]
	if config.Limits != nil && config.Limits.Rlimit != nil {
		for _, rule := range config.Limits.Rlimit {
			rlimitType, ok := rlimitTypeMap[rule.Type]
			if !ok {
				return nil, fmt.Errorf("Invalid rlimit type: %s", rule.Type)
			}
			rlimits = append(rlimits, configs.Rlimit{
				Type: rlimitType,
				Soft: rule.Soft,
				Hard: rule.Hard,
			})
		}
	}

	noNewPrivileges := true

	process := &libcontainer.Process{
		Args: config.Command,
		Env: []string{
			"PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
		},
		Cwd: config.Cwd,
		// TODO: We should use 65534:65534 to force the process to be run as nobody for better security
		User: "65534:65534",
		// TODO: Should use buffered io
		Stdin:           stdInR,
		Stdout:          stdOutW,
		Stderr:          &stdErrBuffer,
		NoNewPrivileges: &noNewPrivileges,
		Init:            true,
		Rlimits:         rlimits,
	}

	wallTimeBegin := time.Now()

	if err := container.Run(process); err != nil {
		return nil, fmt.Errorf("Error initializing the container process: %w", err)
	}

	if stdInFile != nil {
		if _, err := io.Copy(stdInW, stdInFile); err != nil {
			return nil, fmt.Errorf("Error writing to stdin: %w", err)
		}
	}
	_ = stdInW.Close()

	processFinished := false
	if timeLimit > 0 {
		go func(duration uint64) {
			<-time.After(time.Duration(duration) * time.Millisecond)
			if !processFinished {
				if err := container.Signal(unix.SIGKILL, true); err != nil {
					// TODO: Should we panic?
					logrus.WithError(err).Warn("Error sending SIGKELL to the container processes")
				}
			}
		}(timeLimit * 2)
	}

	state, _ := process.Wait()
	wallTimeEnd := time.Now()
	processFinished = true

	isOOM, err := checkIsOOM(fullCgroupPath)
	if err != nil {
		return nil, fmt.Errorf("Error checking if container ran out of memory: %w", err)
	}

	containerStats, err := container.Stats()
	if err != nil {
		return nil, fmt.Errorf("Error getting container stats: %w", err)
	}

	wallTime := wallTimeEnd.Sub(wallTimeBegin)

	report, err := resolveExecutionReport(config, isOOM, state, containerStats, wallTime)
	if err != nil {
		return nil, fmt.Errorf("Error resolving execution report: %w", err)
	}

	_ = stdOutW.Close()
	// TODO: This will produce an additional '\n' character
	if _, err := io.Copy(stdOutFile, stdOutReader); err != nil {
		return nil, fmt.Errorf("Error writing to the stdout file: %w", err)
	}
	if _, err := io.Copy(stdErrFile, &stdErrBuffer); err != nil {
		return nil, fmt.Errorf("Error writing to the stderr file: %w", err)
	}

	return report, nil
}
