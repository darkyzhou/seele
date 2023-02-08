package entities

type RunjConfig struct {
	UserNamespace *UserNamespaceConfig `mapstructure:"user_namespace"`
	CgroupPath    string               `mapstructure:"cgroup_path"`
	Rootfs        string               `mapstructure:"rootfs" validate:"required"`
	Cwd           string               `mapstructure:"cwd" validate:"required"`
	Command       []string             `mapstructure:"command" validate:"required,dive,required"`
	Paths         []string             `mapstructure:"paths" validate:"dive,required"`
	Fd            *FdConfig            `mapstructure:"fd"`
	Mounts        []*MountConfig       `mapstructure:"mounts"`
	Limits        *LimitsConfig        `mapstructure:"limits"`
}

type UserNamespaceConfig struct {
	Enabled    bool   `mapstructure:"enabled"`
	MapToUser  string `mapstructure:"map_to_user" validate:"required"`
	MapToGroup string `mapstructure:"map_to_group" validate:"required"`
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
	TimeMs uint64          `mapstructure:"time_ms"`
	Cgroup *CgroupConfig   `mapstructure:"cgroup"`
	Rlimit []*RlimitConfig `mapstructure:"rlimit"`
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
