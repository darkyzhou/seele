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
	Enabled     bool   `mapstructure:"enabled"`
	RootUid     uint32 `mapstructure:"root_uid" validate:"required"`
	UidMapBegin uint32 `mapstructure:"uid_map_begin" validate:"required"`
	UidMapCount uint32 `mapstructure:"uid_map_count" validate:"required"`
	RootGid     uint32 `mapstructure:"root_gid" validate:"required"`
	GidMapBegin uint32 `mapstructure:"gid_map_begin" validate:"required"`
	GidMapCount uint32 `mapstructure:"gid_map_count" validate:"required"`
}

type FdConfig struct {
	StdIn  string `mapstructure:"stdin"`
	StdOut string `mapstructure:"stdout"`
	StdErr string `mapstructure:"stderr"`
}

type MountConfig struct {
	From    string   `mapstructure:"from" validate:"required"`
	To      string   `mapstructure:"to" validate:"required"`
	Options []string `mapstructure:"options"`
}

type LimitsConfig struct {
	TimeMs uint64        `mapstructure:"time_ms" validate:"required"`
	Cgroup *CgroupConfig `mapstructure:"cgroup" validate:"required"`
	Rlimit *RlimitConfig `mapstructure:"rlimit" validate:"required"`
}

type CgroupConfig struct {
	Memory            int64  `mapstructure:"memory" validate:"required"`
	MemoryReservation int64  `mapstructure:"memory_reservation"`
	MemorySwap        int64  `mapstructure:"memory_swap" validate:"required"`
	MemorySwappiness  uint64 `mapstructure:"memory_swappiness"`
	CpuShares         uint64 `mapstructure:"cpu_shares"`
	CpuQuota          int64  `mapstructure:"cpu_quota"`
	CpusetCpus        string `mapstructure:"cpuset_cpus"`
	CpusetMems        string `mapstructure:"cpuset_mems"`
	PidsLimit         int64  `mapstructure:"pids_limit" validate:"required"`
}

type RlimitConfig struct {
	Core   *RlimitItem `mapstructure:"core" validate:"required"`
	Fsize  *RlimitItem `mapstructure:"fsize" validate:"required"`
	NoFile *RlimitItem `mapstructure:"no_file" validate:"required"`
}

type RlimitItem struct {
	Hard uint64 `mapstructure:"hard"`
	Soft uint64 `mapstructure:"soft"`
}
