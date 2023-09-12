## Runj

### Prerequisites

> Using ArchLinux is recommended :)

* Make sure your distribution provides both `newuidmap` and `newgidmap` binaries.
* Install [skopeo](https://github.com/containers/skopeo) and [umoci](https://github.com/opencontainers/umoci).
* Install Node.js 20 and Go 1.19.
* Refer to [https://github.com/opencontainers/runc/blob/main/docs/cgroup-v2.md](https://github.com/opencontainers/runc/blob/main/docs/cgroup-v2.md).
* Make sure you have a normal user with UID `1000` and GID `1000` as well as `/etc/subuid` like `seele:100000:65536` and `/etc/subgid` like `seele:100000:65536`.
* Reboot your system if you have installed some new packages or changed systemd settings.
* Run `skopeo copy docker://gcc:11-bullseye oci:/tmp/_tmp_gcc:11-bullseye`
* Run `umoci unpack --rootless --image /tmp/_tmp_gcc:bullseye ./tests/image`

### Run Unit Tests

`make test-unit`

### Run Integration Tests

`make test-integration`
