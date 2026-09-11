# M2 D04 guest staging candidate qualification

Date: 2026-09-05

Status: candidate qualified by a bounded experiment; D04 is not Accepted and no
production gateway is implemented by this evidence.

## Scope

`scripts/probe-m2-guest-staging.py` exercised disposable, uniquely labelled
containers and local-driver volumes through Docker Desktop's pinned local daemon.
It used the already approved immutable Rust runtime image, fixed executable paths
and fixed arguments. It did not use a host bind mount, network, pull, install,
image build, Cargo, rustfmt, build script, proc macro or project code.

The complete command/result stream, including binary stdout and stderr as base64,
length and SHA-256, is in `M2-D04-native-qualification.json`.

## Qualified host and inputs

- Docker client/server: 29.7.2, API 1.55.
- Server: Docker Desktop 4.87.0, Linux/arm64, kernel 7.0.12-linuxkit.
- Image: `sha256:8fac70723a8d04b6ec9633ab721806b8a55f4f083a1b3f988c61bf6a00fa1909`.
- Seccomp SHA-256: `sha256:f9d31acb22989dc6ac37c02d4c73acfbbb3b74b5e08beff9983f3a811fd4e56d`.
- Probe SHA-256: `sha256:cb2adb4da596e6111fe3199b3be3c20f351a5f971949e9dbbd98643a8399b21a`.
- Receipt SHA-256: `sha256:9d92f5f4cd7df9c6f860ad0f92414d2a6c7806d9c8b16c2111e8bcd7932193da`.
- Run nonce: `dc3f90c54234bf46`.
- Runtime: 2.600153334 seconds, 107 Docker commands, 79/79 matched observations.

Every container used the existing pinned image and seccomp profile with no
network, a read-only root, all capabilities dropped, no-new-privileges, private
IPC and cgroup namespaces, PID/CPU/memory limits, no log driver, and only the
owned named volume plus bounded `/work` and `/tmp` tmpfs mounts. Docker inspect
results were checked before each container was started.

## Results

### Guardian lifecycle and export ordering

A local-driver volume configured as
`type=tmpfs`, `device=tmpfs`, and
`o=size=4m,nr_inodes=128,uid=65534,gid=65534,mode=0700,nosuid,nodev,noexec`
reported a tmpfs filesystem with 4,096-byte blocks, 1,024 blocks and 128 inodes.

A fixed `/usr/bin/sleep 2147483647` guardian held a read-only mount. A generated
USTAR was ingested through `/usr/bin/tar`, and a fixed `/usr/bin/dd` mutator wrote
1,024 bytes. Both writable containers were removed and verified absent before
export. The only running owned container at that boundary was:

```text
rust-mcp-m2-d04-dc3f90c54234bf46-life-guardian
```

A fixed write attempt through the exporter-style read-only mount failed with:

```text
/usr/bin/dd: failed to open '/source/readonly-probe': Read-only file system
```

The separate read-only GNU USTAR exporter returned exit zero, empty stderr and
10,240 bytes. It preserved these exact files:

| Path | Bytes | SHA-256 | Mode |
|---|---:|---|---:|
| `./sentinel.bin` | 21 | `sha256:8ff5734eb707d2b2ac38acd4afb0db2713b142ff4c24d79e1d12241da235e825` | `0600` |
| `./src/main.rs` | 13 | `sha256:536e506bb90914c243a12b397b9a998f85ae2cbd9ba02dfd03a9e155ca5ca0f4` | `0600` |
| `./mutated.bin` | 1,024 | `sha256:5f70bf18a086007016e948b04aed3b82103a36bea41755b6cddfaf10ace3c6ef` | `0644` |

After removing the exporter and guardian, a fresh container remounted the same
named volume. Its complete USTAR contained only the root `.` directory. This
discriminates guardian-backed persistence from the empty tmpfs created after the
last active mount is gone.

### Byte quota

A separate `size=2m,nr_inodes=128` volume reported 512 blocks of 4,096 bytes.
The fixed command below attempted to write 3 MiB:

```text
/usr/bin/dd if=/dev/zero of=/source/fill.bin bs=1M count=3 status=none
```

It exited 1 with `No space left on device`; the resulting file was exactly
2,097,152 bytes. This demonstrates the configured data-block ceiling on the
qualified daemon.

### Inode quota

A separate `size=4m,nr_inodes=32` volume reported exactly 32 inodes. Extracting
a generated USTAR with 64 empty files exited 2 with `No space left on device`.
A fixed read-only `find` counted 31 created files, with the mount root consuming
the remaining inode.

### Failure and cleanup

The probe injected a fixed failure after creating and starting a fourth guardian,
then removed and verified that container and volume. The unconditional final
cleanup repeated removal for any still-tracked objects and queried both Docker
inventories using the unique run label.

Final inventory:

```json
{"containers": [], "volumes": []}
```

Cleanup errors: `[]`.

## Interpretation and limits

The experiment supports a D04 candidate using a named local-driver tmpfs volume
and a trusted running guardian. It also proves on this daemon that keeping only a
stopped container or an unmounted named-volume object is insufficient to retain
the tmpfs bytes.

Production still needs an exact phase verifier, a hostile bounded USTAR decoder,
whole-tree and operation-scope comparison, removal/absence enforcement before
export, output and deadline handling, and quarantine on uncertain cleanup. The
small experimental limits establish enforcement behavior; they do not select the
production byte and inode limits or qualify worst-case rustfmt/Cargo temporary
space.

Official behavior references used to design the discriminator:

- <https://docs.docker.com/reference/cli/docker/volume/create/#driver-specific-options--o---opt>
- <https://github.com/moby/moby/blob/docker-v29.7.2/daemon/volume/local/local.go#L1675-L1772>
- <https://github.com/moby/moby/blob/docker-v29.7.2/daemon/volume/mounts/mounts.go#L101-L139>
