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

type ExecutionReportProps struct {
	config      *spec.RunjConfig
	state       *os.ProcessState
	stats       *libcontainer.Stats
	wallTime    time.Duration
	cgroupPath  string
	stdOutFile  *os.File
	stdErrFile  *os.File
	rlimitFsize uint64
}

func makeExecutionReport(props *ExecutionReportProps) (*spec.ExecutionReport, error) {
	var (
		memoryUsageKib uint64
		cpuTotalMs     uint64
		cpuKernelMs    uint64
		cpuUserMs      uint64
		exitStatus     = STATUS_UNKNOWN
		isWallTLE      bool
		isSystemTLE    bool
		isUserTLE      bool
	)

	// Since `process.Wait()` could return an error, both `state` and `stats` may be nil
	if props.stats != nil && props.stats.CgroupStats != nil {
		memoryUsageKib = lo.Max([]uint64{props.stats.CgroupStats.MemoryStats.SwapUsage.Usage, props.stats.CgroupStats.MemoryStats.SwapUsage.MaxUsage}) / 1024
		cpuTotalMs = props.stats.CgroupStats.CpuStats.CpuUsage.TotalUsage / 1e6
		cpuKernelMs = props.stats.CgroupStats.CpuStats.CpuUsage.UsageInKernelmode / 1e6
		cpuUserMs = props.stats.CgroupStats.CpuStats.CpuUsage.UsageInUsermode / 1e6
	}

	if props.state != nil {
		status := props.state.Sys().(syscall.WaitStatus)
		switch true {
		case status.Exited():
			if status.ExitStatus() == 0 {
				exitStatus = STATUS_NORMAL
			} else {
				exitStatus = STATUS_RUNTIME_ERROR
			}
		case status.Signaled():
			sig := status.Signal()

			switch sig {
			case unix.SIGXCPU:
				exitStatus = STATUS_TIME_LIMIT_EXCEEDED
			case unix.SIGXFSZ:
				exitStatus = STATUS_OUTPUT_LIMIT_EXCEEDED
			default:
				exitStatus = STATUS_SIGNAL_TERMINATE
			}
		case status.Stopped():
			exitStatus = STATUS_SIGNAL_STOP
		default:
			return nil, fmt.Errorf("Unknown status: %v", status)
		}
	}

	// SIGXCPUs sent by RLIMIT_CPU might not be able to terminate some processes in a dead loop.
	// Plus, currently runj only uses a goroutine for time limiting which will send a SIGKILL
	// if the process ran out of time.
	// In order to determine if it's truly a TLE status, we manually check the config and compare them here.
	if props.config.Limits != nil && props.config.Limits.Time != nil {
		if props.config.Limits.Time.KernelLimitMs != 0 && cpuKernelMs > props.config.Limits.Time.KernelLimitMs {
			exitStatus = STATUS_TIME_LIMIT_EXCEEDED
			isSystemTLE = true
		}
		if props.config.Limits.Time.UserLimitMs != 0 && cpuUserMs > props.config.Limits.Time.UserLimitMs {
			exitStatus = STATUS_TIME_LIMIT_EXCEEDED
			isUserTLE = true
		}
		if props.config.Limits.Time.WallLimitMs != 0 && lo.Max([]uint64{uint64(props.wallTime.Milliseconds()), cpuTotalMs}) > props.config.Limits.Time.WallLimitMs {
			exitStatus = STATUS_TIME_LIMIT_EXCEEDED
			isWallTLE = true
		}
	}

	// SIGXFSZs sent by RLIMIT_FSIZE might not be able to terminate some processes in a dead loop.
	// And they will usually be killed by SIGKILLs sent by TLE checker. Therefore we check
	// the output files' length additionally to determine whether it is actually an OLE status.
	if props.rlimitFsize > 0 {
		if props.config.Fd != nil && props.config.Fd.StdOut != "" {
			info, err := props.stdOutFile.Stat()
			if err != nil {
				return nil, fmt.Errorf("Error checking the stdout file length: %w", err)
			}

			if info.Size() > int64(props.rlimitFsize) {
				exitStatus = STATUS_OUTPUT_LIMIT_EXCEEDED
			}
		}

		if props.config.Fd != nil && props.config.Fd.StdErr != "" {
			info, err := props.stdErrFile.Stat()
			if err != nil {
				return nil, fmt.Errorf("Error checking the stderr file length: %w", err)
			}

			if info.Size() > int64(props.rlimitFsize) {
				exitStatus = STATUS_OUTPUT_LIMIT_EXCEEDED
			}
		}
	}

	// If the process runs into OOM, it will be killed by a signal.
	// We check the cgroup additionally to make sure whether it is actually an OOM status.
	isOOM, err := checkIsOOM(props.cgroupPath)
	if err != nil {
		return nil, fmt.Errorf("Error checking the oom status: %w", err)
	}
	if isOOM {
		exitStatus = STATUS_MEMORY_LIMIT_EXCEEDED
	}

	return &spec.ExecutionReport{
		Status:          exitStatus,
		WallTimeMs:      uint64(props.wallTime.Milliseconds()),
		CpuUserTimeMs:   cpuUserMs,
		CpuKernelTimeMs: cpuKernelMs,
		MemoryUsageKiB:  memoryUsageKib,
		IsWallTLE:       isWallTLE,
		IsSystemTLE:     isSystemTLE,
		IsUserTLE:       isUserTLE,
		IsOOM:           isOOM,
	}, nil
}
