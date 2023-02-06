package execute

import (
	"context"
	"fmt"
	"os"
	"strings"
	"time"

	"github.com/darkyzhou/seele/runj/cmd/runj/entities"
	"github.com/darkyzhou/seele/runj/cmd/runj/utils"
	"github.com/opencontainers/runc/libcontainer"
	"github.com/opencontainers/runc/libcontainer/cgroups"
	"github.com/opencontainers/runc/libcontainer/cgroups/fs2"
	"github.com/opencontainers/runc/libcontainer/configs"
	"github.com/opencontainers/runc/libcontainer/specconv"
	"github.com/opencontainers/runtime-spec/specs-go"
	"github.com/samber/lo"
	"github.com/sirupsen/logrus"
	"golang.org/x/sys/unix"
)

var (
	uidMappings []specs.LinuxIDMapping
	gidMappings []specs.LinuxIDMapping
)

func Execute(ctx context.Context, config *entities.RunjConfig) (*entities.ExecutionReport, error) {
	if config.Rootless {
		if err := prepareIdMaps(); err != nil {
			return nil, fmt.Errorf("Error preparing id maps: %w", err)
		}
	}

	spec, err := makeContainerSpec(config)
	if err != nil {
		return nil, fmt.Errorf("Error making container specification: %w", err)
	}

	factory, err := initContainerFactory()
	if err != nil {
		return nil, fmt.Errorf("Error preparing container factory: %w", err)
	}

	parentCgroupPath, cgroupPath, err := getCgroupPath(config.Rootless)
	if err != nil {
		return nil, fmt.Errorf("Error preparing cgroup path: %w", err)
	}
	defer func() {
		_ = cgroups.RemovePath(cgroupPath)

		if parentCgroupPath != "" {
			_ = cgroups.RemovePath(parentCgroupPath)
		}
	}()

	containerConfig, err := specconv.CreateLibcontainerConfig(&specconv.CreateOpts{
		UseSystemdCgroup: false,
		Spec:             spec,
		RootlessEUID:     config.Rootless,
		RootlessCgroups:  config.Rootless,
	})
	if err != nil {
		return nil, fmt.Errorf("Error creating libcontainer config: %w", err)
	}

	// This is mandatory for libcontainer to correctly handle cgroup path
	containerConfig.Cgroups.Path = strings.Replace(cgroupPath, fs2.UnifiedMountpoint, "", 1)

	containerId := fmt.Sprintf("runj-container-%s", utils.RunjInstanceId)
	container, err := factory.Create(containerId, containerConfig)
	if err != nil {
		return nil, fmt.Errorf("Error creating container instance: %w", err)
	}
	defer func() {
		if err := container.Destroy(); err != nil {
			logrus.WithError(err).Warn("Error destroying container instance")
		}
	}()

	var (
		stdInFile      *os.File
		stdOutFile     *os.File
		stdErrFile     *os.File
		stdOutFilePath string
		stdErrFilePath string
	)
	{
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

		stdOutFilePath = lo.TernaryF(
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

		stdErrFilePath = lo.TernaryF(
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
	}

	var (
		timeLimitMs uint64
	)
	{
		if config.Limits != nil && config.Limits.Time != nil {
			if config.Limits.Time.WallLimitMs != 0 {
				timeLimitMs = config.Limits.Time.WallLimitMs
			} else {
				if config.Limits.Time.KernelLimitMs != 0 || config.Limits.Time.UserLimitMs != 0 {
					timeLimitMs = config.Limits.Time.KernelLimitMs + config.Limits.Time.UserLimitMs
				}
			}
		}
	}

	var (
		rlimits     []configs.Rlimit
		rlimitFsize uint64
	)
	{
		if config.Limits != nil && config.Limits.Rlimit != nil {
			for _, rule := range config.Limits.Rlimit {
				rlimitType, ok := rlimitTypeMap[rule.Type]
				if !ok {
					return nil, fmt.Errorf("Invalid rlimit type: %s", rule.Type)
				}

				if rlimitType == unix.RLIMIT_FSIZE {
					rlimits = append(rlimits, configs.Rlimit{
						Type: rlimitType,
						Soft: rule.Soft + 1,
						Hard: rule.Hard + 1,
					})
					rlimitFsize = rule.Hard
				} else {
					rlimits = append(rlimits, configs.Rlimit{
						Type: rlimitType,
						Soft: rule.Soft,
						Hard: rule.Hard,
					})
				}
			}
		}

		for _, defaultRule := range defaultRlimitRules {
			if lo.NoneBy(rlimits, func(rule configs.Rlimit) bool {
				return defaultRule.Type == rule.Type
			}) {
				rlimits = append(rlimits, defaultRule)
			}
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

	timeLimitCtx, timeLimitCtxCancel := context.WithTimeout(context.Background(), time.Duration(timeLimitMs*2)*time.Millisecond)
	defer timeLimitCtxCancel()

	processFinishedCtx, processFinishedCtxCancel := context.WithCancel(context.Background())
	defer processFinishedCtxCancel()

	if timeLimitMs > 0 {
		go func() {
			select {
			case <-ctx.Done():
			case <-processFinishedCtx.Done():
				return
			case <-timeLimitCtx.Done():
				if err := container.Signal(unix.SIGKILL, true); err != nil {
					logrus.WithError(err).Fatal("Error sending SIGKILL to the container processes")
				}
			}
		}()
	}

	go func() {
		select {
		case <-processFinishedCtx.Done():
			return
		case <-ctx.Done():
			logrus.Warn("Sending SIGKILL to the running container due to runj shutting down")
			_ = container.Signal(unix.SIGKILL, true)
		}
	}()

	wallTimeBegin := time.Now()
	if err := container.Run(process); err != nil {
		return nil, fmt.Errorf("Error initializing the container process: %w", err)
	}
	state, _ := process.Wait()
	wallTimeEnd := time.Now()
	processFinishedCtxCancel()

	if ctx.Err() != nil {
		return nil, fmt.Errorf("Cancelled")
	}

	containerStats, err := container.Stats()
	if err != nil {
		return nil, fmt.Errorf("Error getting container stats: %w", err)
	}

	wallTime := wallTimeEnd.Sub(wallTimeBegin)

	_ = stdInFile.Close()
	_ = stdOutFile.Close()
	_ = stdErrFile.Close()

	report, err := makeExecutionReport(&ExecutionReportProps{
		config:         config,
		state:          state,
		stats:          containerStats,
		wallTime:       wallTime,
		cgroupPath:     cgroupPath,
		stdOutFilePath: stdOutFilePath,
		stdErrFilePath: stdErrFilePath,
		rlimitFsize:    rlimitFsize,
	})
	if err != nil {
		return nil, fmt.Errorf("Error resolving execution report: %w", err)
	}

	return report, nil
}
