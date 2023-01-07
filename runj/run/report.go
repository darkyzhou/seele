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
	REASON_NORMAL              = "NORMAL"
	REASON_RUNTIME_ERROR       = "RUNTIME_ERROR"
	REASON_SIGNAL_TERMINATE    = "SIGNAL_TERMINATE"
	REASON_SIGNAL_STOP         = "SIGNAL_STOP"
	REASON_TIME_LIMIT_EXCEEDED = "TIME_LIMIT_EXCEEDED"
	REASON_OUT_OF_MEMORY       = "OUT_OF_MEMORY"
	REASON_UNKNOWN             = "UNKNOWN"
)

func resolveExecutionReport(config *spec.RunjConfig, isOOM bool, state *os.ProcessState, stats *libcontainer.Stats, wallTime time.Duration) (*spec.ExecutionReport, error) {
	var (
		memoryUsage uint64
		cpuTotalMs  uint64
		cpuKernelMs uint64
		cpuUserMs   uint64
		reason      = REASON_UNKNOWN
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
				reason = REASON_NORMAL
			} else {
				reason = REASON_RUNTIME_ERROR
			}
		case status.Signaled():
			s := status.Signal()
			code = int(s) + 128
			if s == unix.SIGXCPU {
				reason = REASON_TIME_LIMIT_EXCEEDED
			} else {
				reason = REASON_SIGNAL_TERMINATE
			}
		case status.Stopped():
			s := status.StopSignal()
			code = int(s) + 128
			reason = REASON_SIGNAL_STOP
		default:
			return nil, fmt.Errorf("Unknown status: %v", status)
		}
	}

	if config.Limit != nil && config.Limit.Time != nil {
		if config.Limit.Time.KernelLimitMs != 0 && cpuKernelMs > config.Limit.Time.KernelLimitMs {
			reason = REASON_TIME_LIMIT_EXCEEDED
			isSystemTLE = true
		}
		if config.Limit.Time.UserLimitMs != 0 && cpuUserMs > config.Limit.Time.UserLimitMs {
			reason = REASON_TIME_LIMIT_EXCEEDED
			isUserTLE = true
		}
		if config.Limit.Time.WallLimitMs != 0 && lo.Max([]uint64{uint64(wallTime.Milliseconds()), cpuTotalMs}) > config.Limit.Time.WallLimitMs {
			reason = REASON_TIME_LIMIT_EXCEEDED
			isWallTLE = true
		}
	}

	if isOOM {
		// In the oom case, the reason would actually be REASON_SIGNAL_TERMINATE
		// but we choose to specify it with a new reason
		reason = REASON_OUT_OF_MEMORY
	}

	return &spec.ExecutionReport{
		Reason:          reason,
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
