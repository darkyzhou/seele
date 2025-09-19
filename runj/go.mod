module github.com/darkyzhou/seele/runj

go 1.24.4

require (
	github.com/coreos/go-systemd/v22 v22.6.0
	github.com/cyphar/filepath-securejoin v0.4.1
	github.com/go-playground/validator/v10 v10.27.0
	github.com/godbus/dbus/v5 v5.1.0
	github.com/matoous/go-nanoid/v2 v2.1.0
	github.com/mitchellh/mapstructure v1.5.0

	// can not upgrade to runc v1.2+ as LinuxFactory is removed
	// https://github.com/opencontainers/runc/commit/6a3fe1618f5166e5c44f21714736049bac9c02cb
	github.com/opencontainers/runc v1.1.15
	github.com/opencontainers/runtime-spec v1.2.1
	github.com/samber/lo v1.51.0
	github.com/sirupsen/logrus v1.9.3
	golang.org/x/sys v0.36.0
)

require (
	github.com/checkpoint-restore/go-criu/v5 v5.3.0 // indirect
	github.com/cilium/ebpf v0.12.3 // indirect
	github.com/containerd/console v1.0.5 // indirect
	github.com/gabriel-vasile/mimetype v1.4.9 // indirect
	github.com/go-playground/locales v0.14.1 // indirect
	github.com/go-playground/universal-translator v0.18.1 // indirect
	github.com/leodido/go-urn v1.4.0 // indirect
	github.com/moby/sys/mountinfo v0.7.2 // indirect
	github.com/mrunalp/fileutils v0.5.1 // indirect
	github.com/opencontainers/selinux v1.12.0 // indirect
	github.com/seccomp/libseccomp-golang v0.11.0 // indirect
	github.com/syndtr/gocapability v0.0.0-20200815063812-42c35b437635 // indirect
	github.com/vishvananda/netlink v1.3.1 // indirect
	github.com/vishvananda/netns v0.0.5 // indirect
	golang.org/x/crypto v0.40.0 // indirect
	golang.org/x/exp v0.0.0-20230224173230-c95f2b4c22f2 // indirect
	golang.org/x/net v0.42.0 // indirect
	golang.org/x/text v0.27.0 // indirect
	google.golang.org/protobuf v1.36.6 // indirect
)
