# Execution sandbox probe

Fixed-scenario Linux fixture for the Execution Gateway tests. Uses Go standard
library only. Do not run its resource or filesystem scenarios on the host.

Build a static Linux arm64 executable using the pinned local Go 1.27.1 toolchain:

```text
GOOS=linux GOARCH=arm64 CGO_ENABLED=0 go build -trimpath -ldflags='-s -w -buildid=' -o /private/tmp/probe fixtures/execution-probe/main.go
```

The container build context must contain this `Dockerfile`, the checked-in
`canary` text file, and the resulting executable named `probe`. The Dockerfile
copies `canary` to `/rootfs-canary` with mode 0666, so a denied write must result
from the read-only mount (EROFS), not ordinary file permissions. `FROM scratch` requires no base-image pull. The image
uses UID/GID 65532 and `/mcp-probe` as its fixed entrypoint. Container policy,
seccomp, limits, writable `/work` tmpfs, and cleanup belong to the gateway harness.
No Docker commands are performed by the fixture.

Pass exactly one allowlisted argument. Output consists of newline-delimited JSON:
`{"scenario":string,"event":string,"pid":number,"details":object|null}`.
An operation result is `{"allowed":bool,"errno":number,"error":string}`;
`errno` is zero on success or on errors without a syscall errno.

| Scenario | Events and assertions |
| --- | --- |
| `success` | `completed`, `details.ok=true`; exit 0. |
| `exit7` | `completed`, `details.exit_code=7`; exit 7. |
| `output` | Concurrent stdout and stderr writers; each emits 20 `output` records with `stream`, `index`, and 65,536-byte `payload`. Each stream exceeds 1 MiB. Final stdout `completed` reports counts. Closed output exits 74. |
| `sleep` | `started` with `duration_seconds=60`, then sleeps and emits `completed`. |
| `environment` | `environment`, `entries` contains the exact sorted environment. The harness must supply controlled variables only. |
| `network` | Ten `socket` records: four distinct IPv4/IPv6 × TCP/UDP tuples, each repeated with loopback/DNS purpose labels (eight calls), plus AF_UNIX/SOCK_STREAM/protocol 0 and AF_NETLINK/SOCK_RAW/NETLINK_ROUTE (protocol 0). The additional records use family `unix`/`netlink`, transport `stream`/`raw`, purpose `local`. Each includes `operation=socket_only`, family, transport, purpose, and result. The two purpose labels do not represent different IP socket tuples. No bind, connect, DNS lookup, or packet transmission occurs. Assert socket creation denied, normally errno EPERM. |
| `filesystem` | `rootfs_canary` reports world-writable mode; `write` for `/mcp-probe` (deny), `/rootfs-canary` (EROFS), and `/work/positive` (allow). `host_canary` must be absent; `symlink_write` to `/rootfs-canary` must return EROFS. `symlink_swap` reports 256 atomic symlink replacements racing 512 writes. `filesystem_assertions` requires zero unexpected root writes and unchanged rootfs canary contents; failed assertions exit 1. |
| `descendants` | Starts a fixed `daemonize` intermediate, which starts `heartbeat` with Setsid and exits. `descendant_started` retains `child_pid` and `setsid=true`, adding `intermediate_pid`, `original_parent_pid`, `parent_process_group`, and `double_fork=true`. Main waits for the intermediate and emits `intermediate_exited`, then remains alive 60 seconds. Require a heartbeat with `parent_pid=1` and `process_group=child_pid`, distinct from the parent group; this proves orphan adoption and session separation before testing gateway termination. |
| `daemonize` | Internal fixed intermediate scenario; spawns only `/mcp-probe heartbeat`, reports the descendant identity, releases its process handle, and exits. |
| `heartbeat` | Internal child scenario; `heartbeat` reports tick, parent PID, process group every 100 ms, for at most 60 seconds. |
| `pids` | Starts at most 80 fixed `sleep` children with GOMAXPROCS=1; `spawn_failed` reports syscall errno and EAGAIN, then `pids` reports started count, EAGAIN, and cgroup readings. All started children are killed and waited on before normal return. |
| `memory` | `memory_started` reports 192 MiB maximum; `memory_allocated` records each 8 MiB allocation after touching every 4 KiB page; retains memory for 60 seconds. A 64 MiB container limit should kill it before completion; assert container OOM evidence separately. |
| `disk` | Writes at most 32 MiB to `/work/disk-probe`; `disk` reports bytes written, result, and ENOSPC. Use a writable 8 MiB tmpfs and assert ENOSPC. |
| `cpu` | Busy loop for approximately 3 wall-clock seconds; `cpu` reports raw `cpu.stat` before/after, `cpu.max`, elapsed milliseconds, iterations, checksum. Assert positive throttling deltas under a restrictive CPU quota. |
| `cgroups` | `cgroups` reports `pids.max`, `pids.current`, `pids.events`, `memory.max`, `memory.events`, `cpu.max`, each with `value` and result. |

Unknown scenarios or an incorrect argument count produce `rejected` and exit 64.
The program does not accept paths, shell commands, executables, or extra arguments.

For independent PID-limit evidence, set memory high enough that child runtimes do
not hit the memory limit first. Go runtime threads count against `pids.max`;
`pids.events` supplies kernel limit-hit evidence if a child fails during startup
before the parent observes EAGAIN. A socket-denial result is not an external-network
reachability test. Mount and child-containment guarantees must be asserted by the
harness using container state, not inferred from fixture exit alone.

The symlink race replaces `/work/probe-link` atomically via
`/work/probe-link-next`, alternating fixed targets `/work/positive` and
`/rootfs-canary`. Successful opens are classified with `fstat` identity on the
actual opened file; a separate path lookup cannot misclassify a raced write.
The before/after rootfs snapshot is bounded to 4096 bytes. Race scheduling may
change the positive-write and EROFS counts; the direct positive write and direct
canary/symlink EROFS checks remain deterministic assertions. The fixture reports
race counters and exact contents equality without claiming every timing was seen.

The orphaned heartbeat is intentionally not cleaned up by the intermediate;
container PID-namespace cleanup is the boundary being tested. A harness running
`descendants` must stop/remove its container even if assertions fail. Capturing
`parent_pid=1` matters: Setsid alone does not prove the intermediate parent exited.
