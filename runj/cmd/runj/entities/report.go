package entities

type ExecutionReport struct {
	Status          string `json:"status"`
	ExitCode        int    `json:"exit_code"`
	Signal          string `json:"signal,omitempty"`
	WallTimeMs      uint64 `json:"wall_time_ms"`
	CpuUserTimeMs   uint64 `json:"cpu_user_time_ms"`
	CpuKernelTimeMs uint64 `json:"cpu_kernel_time_ms"`
	MemoryUsageKiB  uint64 `json:"memory_usage_kib"`
}
