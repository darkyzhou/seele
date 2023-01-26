package spec

type RunjConfig struct {
	Rootless bool           `mapstructure:"rootless"`
	Rootfs   string         `mapstructure:"rootfs" validate:"required"`
	Cwd      string         `mapstructure:"cwd" validate:"required"`
	Command  []string       `mapstructure:"command" validate:"required,dive,required"`
	Paths    []string       `mapstructure:"paths" validate:"dive,required"`
	Fd       *FdConfig      `mapstructure:"fd"`
	Mounts   []*MountConfig `mapstructure:"mounts"`
	Limits   *LimitsConfig  `mapstructure:"limits"`
}

type FdConfig struct {
	StdIn  string `mapstructure:"stdin"`
	StdOut string `mapstructure:"stdout"`
	StdErr string `mapstructure:"stderr"`
}

type MountConfig struct {
	From    string   `mapstructure:"from"  validate:"required"`
	To      string   `mapstructure:"to"  validate:"required"`
	Options []string `mapstructure:"options"`
}

type LimitsConfig struct {
	Time   *TimeLimitsConfig `mapstructure:"time"`
	Cgroup *CgroupConfig     `mapstructure:"cgroup"`
	Rlimit []*RlimitConfig   `mapstructure:"rlimit"`
}

type TimeLimitsConfig struct {
	WallLimitMs   uint64 `mapstructure:"wall"`
	KernelLimitMs uint64 `mapstructure:"kernel"`
	UserLimitMs   uint64 `mapstructure:"user"`
}

type CgroupConfig struct {
	Memory            int64  `mapstructure:"memory"`
	MemoryReservation int64  `mapstructure:"memory_reservation"`
	MemorySwap        int64  `mapstructure:"memory_swap"`
	CpuShares         uint64 `mapstructure:"cpu_shares"`
	CpuQuota          int64  `mapstructure:"cpu_quota"`
	CpusetCpus        string `mapstructure:"cpuset_cpus"`
	CpusetMems        string `mapstructure:"cpuset_mems"`
	PidsLimit         int64  `mapstructure:"pids_limit"`
}

type RlimitConfig struct {
	Type string `mapstructure:"type"  validate:"required"`
	Hard uint64 `mapstructure:"hard"  validate:"required"`
	Soft uint64 `mapstructure:"soft"  validate:"required"`
}
