package execute

import (
	"context"
	"fmt"
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

func Execute(ctx context.Context, config *entities.RunjConfig) (*entities.ExecutionReport, error) {
	userNamespaceEnabled := config.UserNamespace != nil && config.UserNamespace.Enabled

	var (
		uidMappings []specs.LinuxIDMapping
		gidMappings []specs.LinuxIDMapping
	)
	if userNamespaceEnabled {
		uids, gids := getIdMappings(config.UserNamespace)
		uidMappings = append(uidMappings, uids...)
		gidMappings = append(gidMappings, gids...)
	}

	spec, err := makeContainerSpec(config, uidMappings, gidMappings)
	if err != nil {
		return nil, fmt.Errorf("Error making container specification: %w", err)
	}

	overlayfsConfig, err := prepareOverlayfs(config.UserNamespace, config.Overlayfs)
	if err != nil {
		return nil, fmt.Errorf("Error checking overlayfs config: %w", err)
	}

	factory, err := initContainerFactory(overlayfsConfig)
	if err != nil {
		return nil, fmt.Errorf("Error preparing container factory: %w", err)
	}

	parentCgroupPath, cgroupPath, err := getCgroupPath(config.CgroupPath, userNamespaceEnabled)
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
		RootlessEUID:     userNamespaceEnabled,
		RootlessCgroups:  userNamespaceEnabled,
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

	stdInFile, stdOutFile, stdErrFile, err := prepareFds(config.Fd)
	if err != nil {
		return nil, fmt.Errorf("Error preparing fds")
	}
	defer func() {
		_ = stdInFile.Close()
		_ = stdOutFile.Close()
		_ = stdErrFile.Close()
	}()

	var (
		rlimits     []configs.Rlimit
		rlimitFsize uint64
	)
	{
		if config.Limits != nil && config.Limits.Rlimit != nil {
			if config.Limits.Rlimit.Core != nil {
				rlimits = append(rlimits, configs.Rlimit{
					Type: unix.RLIMIT_CORE,
					Hard: config.Limits.Rlimit.Core.Hard,
					Soft: config.Limits.Rlimit.Core.Soft,
				})
			}

			if config.Limits.Rlimit.NoFile != nil {
				rlimits = append(rlimits, configs.Rlimit{
					Type: unix.RLIMIT_NOFILE,
					Hard: config.Limits.Rlimit.NoFile.Hard,
					Soft: config.Limits.Rlimit.NoFile.Soft,
				})
			}

			if config.Limits.Rlimit.Fsize != nil {
				rlimits = append(rlimits, configs.Rlimit{
					Type: unix.RLIMIT_FSIZE,
					Hard: config.Limits.Rlimit.Fsize.Hard + 1,
					Soft: config.Limits.Rlimit.Fsize.Soft + 1,
				})
				rlimitFsize = config.Limits.Rlimit.Fsize.Hard
			}
		}
	}

	noNewPrivileges := true
	pathEnv := "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin" + lo.Ternary(len(config.Paths) <= 0, "", ":"+strings.Join(config.Paths, ":"))
	process := &libcontainer.Process{
		Args:            config.Command,
		Env:             []string{pathEnv},
		Cwd:             config.Cwd,
		User:            "65534:65534",
		Stdin:           stdInFile,
		Stdout:          stdOutFile,
		Stderr:          stdErrFile,
		NoNewPrivileges: &noNewPrivileges,
		Init:            true,
		Rlimits:         rlimits,
	}

	processFinishedCtx, processFinishedCtxCancel := context.WithCancel(context.Background())
	defer processFinishedCtxCancel()

	timeLimit := time.Duration(config.Limits.TimeMs*3) * time.Millisecond
	timeLimitCtx, timeLimitCtxCancel := context.WithTimeout(context.Background(), timeLimit)
	defer timeLimitCtxCancel()

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

	wallTimeLimitExceeded := timeLimitCtx.Err() != nil

	processFinishedCtxCancel()
	timeLimitCtxCancel()

	if ctx.Err() != nil {
		return nil, fmt.Errorf("Cancelled")
	}

	stats, err := container.Stats()
	if err != nil {
		return nil, fmt.Errorf("Error getting container stats: %w", err)
	}

	wallTime := wallTimeEnd.Sub(wallTimeBegin)

	_ = stdInFile.Close()
	_ = stdOutFile.Close()
	_ = stdErrFile.Close()

	report, err := makeExecutionReport(&ExecutionReportProps{
		config,
		state,
		stats,
		wallTime,
		wallTimeLimitExceeded,
		cgroupPath,
		rlimitFsize,
	})
	if err != nil {
		return nil, fmt.Errorf("Error resolving execution report: %w", err)
	}

	return report, nil
}
