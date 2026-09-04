# Rust containment calibration sources

These are container-only fixture sources, not a Cargo package. There is deliberately
no Cargo.toml. Do not compile or execute these files on the host. The harness must
construct a bounded SourceBundle and generate its manifests/lockfile explicitly,
then use the approved Rust 1.98.1 Linux ARM64 image and ADR-031 gateway. The sources
also reject non-Linux/non-ARM64 compilation. No result has been certified merely
by adding these fixtures; real isolated runs and independent observation are required.

## Bundle wiring

- Build-script checks: place `build.rs` and `checks.rs` at the generated package
  root. A minimal generated `src/lib.rs` is sufficient. Run Cargo check; Cargo
  metadata with --no-deps does not prove build-script execution.
- Proc-macro checks: place `proc_macro.rs` as the generated local proc-macro
  package's `src/lib.rs`, and `checks.rs` beside it as `src/checks.rs`. Generate
  `[lib] proc-macro = true` and a consumer whose source invokes
  `containment_macro::verify_containment!();`. Merely compiling the macro package
  does not execute its checks. The macro returns an empty TokenStream and accepts
  no arguments. No external dependencies are needed.
- Timeout/overflow: place the selected `build_timeout.rs` or `build_overflow.rs`
  under the generated name `build.rs`, with `checks.rs` and `descendants.rs` at
  the same package root. Do not combine both build scripts in a single run.
- These modules are plain text suitable for `include_str!` in the trusted harness;
  fixture sources themselves never select programs or parameters from caller input.

## Exact expectations

Both the actual build-script process and the process executing the proc macro must
pass all checks; an assertion failure invalidates that run:

- `/proc/self/status`: all four UIDs and GIDs are 65534; CapInh/CapPrm/CapEff/CapBnd/
  CapAmb are zero; NoNewPrivs is 1 and Seccomp is 2.
- `socket`: AF_INET (2), AF_INET6 (10), and AF_UNIX (1), each with SOCK_STREAM (1),
  SOCK_DGRAM (2), and SOCK_SEQPACKET (5), protocol 0, return -1/EPERM. These calls
  only attempt creation; no bind/connect/listen or network data transfer occurs.
  A descriptor unexpectedly allowed is closed immediately before failure.
- `socketpair(AF_UNIX, SOCK_SEQPACKET, 0)` succeeds and exchanges one four-byte
  packet using nonblocking send/recv on the private endpoints. NONBLOCK/CLOEXEC
  creation flags are allowed; all four combinations are positive controls.
  UNIX STREAM/DGRAM, UNIX SEQPACKET protocol 1, and INET/INET6 SEQPACKET return
  -1/EPERM. The approved profile masks the socketpair base type;
  EINVAL/EAFNOSUPPORT is not accepted as policy enforcement.
- After all fixture socketpairs close, enumerate at most 1024 `/proc/self/fd`
  entries, each readlink target bounded to 4096 bytes, and reject any visible
  `socket:[...]`. Other descriptors, including Cargo jobserver pipes, are allowed.
  ENOENT between enumeration and readlink is tolerated; other errors fail closed.
  This is a point-in-time observation, not proof of descriptor ancestry or absence
  of a socket created and closed between observations.
- Twelve direct Linux ARM64 syscalls require -1/EPERM: bind, connect and listen
  with fd=-1; unshare with CLONE_NEWUSER plus unsupported bit63; setns with fd=-1;
  mount with NULL paths; ptrace with invalid request/PID; mknodat with NULL path;
  keyctl and bpf with invalid commands; io_uring_setup with zero entries/NULL;
  clone with CLONE_NEWUSER|CLONE_THREAD but no CLONE_SIGHAND/CLONE_VM. Arguments
  remain invalid if the filter regresses: no socket operation, namespace change,
  device, child, key or io_uring resource can be created. EINVAL, EBADF, EFAULT or
  ENOSYS fails the fixture. EPERM alone does not identify its origin: capabilities
  or another kernel policy can also reject some calls. Combine these observations
  with the exact inspected seccomp profile; do not claim independent filter-rule
  coverage for every call without a discriminating outer control.
- `/proc/self/mountinfo`: exactly one entry each for `/`, `/source`, `/work`, `/tmp`
  and `/dev/shm`;
  root and source mount options include ro and exclude rw; work includes rw and
  excludes noexec; tmp and shared memory include rw and noexec. There is no
  positive `exec` flag required in mountinfo: absence of noexec represents
  executable mounting. This does not assert that /work is the sole writable
  executable mount; Docker's /dev can also be writable and executable. The outer
  applied-config check owns the current 1MiB shared-memory size assertion.
- Exclusive file creation at fixed synthetic root/source paths fails with EACCES
  or EROFS. Mountinfo independently proves ro, so EACCES alone is not presented
  as read-only enforcement. A short `/work` canary is created, read and removed.
  No existing source, host path or host file is modified.
- Unified cgroup v2 files: memory.max=1073741824, memory.swap.max=0, pids.max=128,
  and numeric positive cpu.max quota equal to period (one CPU). These observe
  configured controls; they do not independently prove throttling, OOM or PID
  exhaustion. Missing/non-v2 views fail closed rather than silently skipping.
- Environment variables MCP_TEST_SYNTHETIC_SECRET and HOST_SECRET are absent;
  values are never read or printed. The trusted outer harness must seed a known
  synthetic variable in its own environment for this to be discriminating.
- Rust `Command::status` successfully executes the fixed `/usr/bin/true` with
  env_clear, cwd `/work` and stdin/stdout/stderr null. This is a positive control
  for the Rust fork/exec path; no shell or rustup is used.

The markers RUST_CONTAINMENT_BUILD_CHECKS_PASSED and
RUST_CONTAINMENT_PROC_MACRO_CHECKS_PASSED are emitted only after these assertions.
Require the expected run/consumer to complete successfully as well as validating
its observations; a marker by itself is insufficient. Kernel text reads are
bounded to 256 KiB each, and synthetic file contents are short fixed records.
The /tmp and /dev/shm noexec assertions inspect mount metadata, not executable probes.

## Descendants, timeout and overflow

The fixed descendant helper performs fork, setsid and a second fork. The
intermediate child is reaped. The grandchild has all standard handles redirected
to /dev/null, retains the detached session, and sleeps for at most 60 seconds of
sleep time before `_exit`. It does not hold Cargo's output pipes open. The parent
checks the returned PID/session ID and writes a short record under
`/work/rust-containment-descendant-{timeout|overflow}-<parent-pid>.pid`.

`build_timeout.rs` sleeps for 60 seconds after spawning; choose a gateway deadline
well below that, while allowing compilation/setup enough time to reach the
fixture. A run that times out before the descendant exists is not descendant
cleanup evidence. Cancellation testing similarly needs a live descendant witness
before the host cancels the operation.

`build_overflow.rs` emits 1024 cargo:warning lines of about 1 KiB each, fewer than
2 MiB of build-script stdout including its initial marker, then exits. Configure
an outer output budget materially below that amount. Cargo adds its own framing
and may buffer warnings until the build script completes. Output overflow can
therefore occur after the Cargo/build-script process has already exited; do not
infer that overflow killed a live descendant from warnings alone.

Producer stdout.flush does not flush Cargo's private buffers. For both scenarios,
use independent trusted Docker top/inspect or an explicit bounded guest /proc
observation while the run is active to establish that the descendant exists and
is detached. A /work marker is supporting evidence, not proof of current liveness.
The PID is in the container PID namespace, not a host PID; correlate it with the
owned container/daemon evidence. After timeout/cancellation/overflow, verify that
all owned containers are absent and the source volume is removed. Container
absence covers that PID namespace; merely observing the CLI's exit or disappearance
of its immediate child does not. The principal harness owns these observations
and cleanup checks; these source files do not certify them.

## Reference interfaces

- [Linux socketpair(2)](https://man7.org/linux/man-pages/man2/socketpair.2.html)
- [Linux proc_pid_mountinfo(5)](https://man7.org/linux/man-pages/man5/proc_pid_mountinfo.5.html)
- [Kernel cgroup v2 interface](https://docs.kernel.org/admin-guide/cgroup-v2.html)
- Syscall numbers except io_uring_setup were checked against installed Go1.27.1
  `src/syscall/zsysnum_linux_arm64.go`; [Linux v7.0 syscall table](https://github.com/torvalds/linux/blob/v7.0/scripts/syscall.tbl)
  supplies io_uring_setup=425 and confirms the generic ARM64 numbering.
- [Linux v7.0 fork/unshare validation](https://github.com/torvalds/linux/blob/v7.0/kernel/fork.c)
  rejects THREAD without SIGHAND before child creation and unsupported unshare
  flags before namespace changes.
- Repository ADR-031 and the exact applied image/profile remain authoritative for
  the expected runtime configuration.

## Resource enforcement

`build_resources.rs` fills the bounded /tmp and /work tmpfs until ENOSPC, removes
the synthetic files, exhausts pids.max with bounded sleep helpers until EAGAIN,
and kills/reaps every owned helper. A separately spawned helper raises its own
oom_score_adj, allocates/touches memory and must be killed by the memory cgroup;
the parent checks SIGKILL and the oom_kill counter delta. Four bounded CPU threads
must increase the throttling counter. Missing controls, unexpected baselines,
missing enforcement or helper cleanup failures fail the fixture. These stressors
run solely inside the approved container, with memory1GiB, pids128 and CPU1.
