## Runj

### Prerequisites

> Using ArchLinux is recommended :)

* Make sure your distribution provides both `newuidmap` and `newgidmap` binaries.
* Install [skopeo](https://github.com/containers/skopeo) and [umoci](https://github.com/opencontainers/umoci).
* Install Node.js 20.
* Refer to [https://github.com/opencontainers/runc/blob/main/docs/cgroup-v2.md](https://github.com/opencontainers/runc/blob/main/docs/cgroup-v2.md).
* Make sure you have a normal user with UID `1000` and GID `1000` as well as `/etc/subuid` like `seele:100000:65536` and `/etc/subgid` like `seele:100000:65536`.
* Reboot your system if you have installed some new packages or changed systemd settings.

### Run Unit Tests

`make test-unit`

### Run Integration Tests

1. `skopeo copy docker://gcc:11-bullseye oci:/tmp/_tmp_gcc:11-bullseye`
2. `umoci unpack --rootless --image /tmp/_tmp_gcc:bullseye ./tests/image`
3. `make test-integration`
