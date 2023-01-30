package run

import (
	"fmt"
	"os"
	"syscall"
	"time"

	"github.com/darkyzhou/seele/runj/spec"
	"github.com/opencontainers/runc/libcontainer"
	"github.com/samber/lo"
	"golang.org/x/sys/unix"
)

const (
	STATUS_NORMAL                = "NORMAL"
	STATUS_RUNTIME_ERROR         = "RUNTIME_ERROR"
	STATUS_SIGNAL_TERMINATE      = "SIGNAL_TERMINATE"
	STATUS_SIGNAL_STOP           = "SIGNAL_STOP"
	STATUS_TIME_LIMIT_EXCEEDED   = "TIME_LIMIT_EXCEEDED"
	STATUS_MEMORY_LIMIT_EXCEEDED = "MEMORY_LIMIT_EXCEEDED"
	STATUS_OUTPUT_LIMIT_EXCEEDED = "OUTPUT_LIMIT_EXCEEDED"
	STATUS_UNKNOWN               = "UNKNOWN"
)

func resolveExecutionReport(config *spec.RunjConfig, isOOM bool, state *os.ProcessState, stats *libcontainer.Stats, wallTime time.Duration) (*spec.ExecutionReport, error) {
	var (
		memoryUsage uint64
		cpuTotalMs  uint64
		cpuKernelMs uint64
		cpuUserMs   uint64
		exitStatus  = STATUS_UNKNOWN
		code        = -1
		isWallTLE   bool
		isSystemTLE bool
		isUserTLE   bool
	)

	// TODO: if no cgroup limits found, skip the checks
	// Since process.Wait() could return an error, both `state` and `stats` may be nil
	if stats != nil && stats.CgroupStats != nil {
		memoryUsage = lo.Max([]uint64{stats.CgroupStats.MemoryStats.SwapUsage.Usage, stats.CgroupStats.MemoryStats.SwapUsage.MaxUsage}) / 1024
		cpuTotalMs = stats.CgroupStats.CpuStats.CpuUsage.TotalUsage / 1e6
		cpuKernelMs = stats.CgroupStats.CpuStats.CpuUsage.UsageInKernelmode / 1e6
		cpuUserMs = stats.CgroupStats.CpuStats.CpuUsage.UsageInUsermode / 1e6
	}

	if state != nil {
		status := state.Sys().(syscall.WaitStatus)

		switch true {
		case status.Exited():
			code = status.ExitStatus()
			if code == 0 {
				exitStatus = STATUS_NORMAL
			} else {
				exitStatus = STATUS_RUNTIME_ERROR
			}
		case status.Signaled():
			s := status.Signal()
			code = int(s) + 128
			if s == unix.SIGXCPU {
				exitStatus = STATUS_TIME_LIMIT_EXCEEDED
			} else if s == unix.SIGXFSZ {
				exitStatus = STATUS_OUTPUT_LIMIT_EXCEEDED
			} else {
				exitStatus = STATUS_SIGNAL_TERMINATE
			}
		case status.Stopped():
			s := status.StopSignal()
			code = int(s) + 128
			exitStatus = STATUS_SIGNAL_STOP
		default:
			return nil, fmt.Errorf("Unknown status: %v", status)
		}
	}

	if config.Limits != nil && config.Limits.Time != nil {
		if config.Limits.Time.KernelLimitMs != 0 && cpuKernelMs > config.Limits.Time.KernelLimitMs {
			exitStatus = STATUS_TIME_LIMIT_EXCEEDED
			isSystemTLE = true
		}
		if config.Limits.Time.UserLimitMs != 0 && cpuUserMs > config.Limits.Time.UserLimitMs {
			exitStatus = STATUS_TIME_LIMIT_EXCEEDED
			isUserTLE = true
		}
		if config.Limits.Time.WallLimitMs != 0 && lo.Max([]uint64{uint64(wallTime.Milliseconds()), cpuTotalMs}) > config.Limits.Time.WallLimitMs {
			exitStatus = STATUS_TIME_LIMIT_EXCEEDED
			isWallTLE = true
		}
	}

	if isOOM {
		// In the oom case, the exitStatus is actually STATUS_SIGNAL_TERMINATE
		// here we specialize it with a new status.
		exitStatus = STATUS_MEMORY_LIMIT_EXCEEDED
	}

	return &spec.ExecutionReport{
		Status:          exitStatus,
		ExitCode:        code,
		WallTimeMs:      uint64(wallTime.Milliseconds()),
		CpuUserTimeMs:   cpuUserMs,
		CpuKernelTimeMs: cpuKernelMs,
		MemoryUsageKiB:  memoryUsage,
		IsWallTLE:       isWallTLE,
		IsSystemTLE:     isSystemTLE,
		IsUserTLE:       isUserTLE,
		IsOOM:           isOOM,
	}, nil
}
