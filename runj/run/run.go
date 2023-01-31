package run

import (
	"context"
	"fmt"
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

	stdInFilePath := lo.TernaryF(
		config.Fd == nil || config.Fd.StdIn == "",
		func() string {
			return "/dev/null"
		},
		func() string {
			return config.Fd.StdIn
		},
	)
	stdInFile, err = os.Open(stdInFilePath)
	if err != nil {
		return nil, fmt.Errorf("Error opening the stdin file %s: %w", stdInFilePath, err)
	}
	defer stdInFile.Close()

	stdOutFilePath := lo.TernaryF(
		config.Fd == nil || config.Fd.StdOut == "",
		func() string {
			return "/dev/null"
		},
		func() string {
			return config.Fd.StdOut
		},
	)
	stdOutFile, err = prepareOutFile(stdOutFilePath)
	if err != nil {
		return nil, fmt.Errorf("Error preparing the stdout file %s: %w", stdOutFilePath, err)
	}
	defer stdOutFile.Close()

	stdErrFilePath := lo.TernaryF(
		config.Fd == nil || config.Fd.StdErr == "",
		func() string {
			return "/dev/null"
		},
		func() string {
			return config.Fd.StdErr
		},
	)
	stdErrFile, err = prepareOutFile(stdErrFilePath)
	if err != nil {
		return nil, fmt.Errorf("Error preparing the stderr file %s: %w", stdErrFilePath, err)
	}
	defer stdErrFile.Close()

	var timeLimitMs uint64
	if config.Limits != nil && config.Limits.Time != nil {
		if config.Limits.Time.WallLimitMs != 0 {
			timeLimitMs = config.Limits.Time.WallLimitMs
		} else {
			if config.Limits.Time.KernelLimitMs != 0 || config.Limits.Time.UserLimitMs != 0 {
				timeLimitMs = config.Limits.Time.KernelLimitMs + config.Limits.Time.UserLimitMs
			}
		}
	}

	var rlimits []configs.Rlimit
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

	path := "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin" + lo.Ternary(len(config.Paths) <= 0, "", ":"+strings.Join(config.Paths, ":"))
	process := &libcontainer.Process{
		Args:            config.Command,
		Env:             []string{path},
		Cwd:             config.Cwd,
		User:            "65534:65534",
		Stdin:           stdInFile,
		Stdout:          stdOutFile,
		Stderr:          stdErrFile,
		NoNewPrivileges: &noNewPrivileges,
		Init:            true,
		Rlimits:         rlimits,
	}

	processFinished := false

	ctx, cancel := context.WithTimeout(context.Background(), time.Duration(timeLimitMs*2)*time.Millisecond)
	defer cancel()

	if timeLimitMs > 0 {
		go func(duration uint64) {
			<-ctx.Done()
			if !processFinished {
				if err := container.Signal(unix.SIGKILL, true); err != nil {
					// TODO: Should we panic?
					logrus.WithError(err).Warn("Error sending SIGKELL to the container processes")
				}
			}
		}(timeLimitMs * 2)
	}

	wallTimeBegin := time.Now()
	if err := container.Run(process); err != nil {
		return nil, fmt.Errorf("Error initializing the container process: %w", err)
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

	return report, nil
}
