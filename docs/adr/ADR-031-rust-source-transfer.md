# ADR-031 — Approved Rust runtime and bounded source transfer

## Status

Accepted for M1-01; implementation, actual calibration and independent Opus5
containment review complete for this bounded prerequisite. Does not enable MCP Cargo tools or reuse probe authorization.

## Context

The owner explicitly authorized installing Rust/Cargo1.98.1 Linux ARM64.
Official Docker Rust1.98.1 tags are not yet published, but the exact official Rust
distribution is available. A pinned Debian base plus the hash-verified official
Rust components provides the host-approved local runtime. It remains separate
from the host macOS toolchain and from the Go probe configuration.
Docker rejects copying into a read-only stopped container rootfs. Passing host
source paths to Docker would also reopen paths outside our handle boundary.

## Decision

Acquire source bytes through the existing macOS/APFS root-relative no-follow
filesystem adapter, after resolving a live ProjectRef. Produce an owned bounded
SourceBundle containing sorted unique relative regular-file paths and exact bytes,
plus explicit directories including empty ones. Reject file/directory collisions;
count implied and explicit directories once against the shared entry limit.
No external process or pathname-based ordinary open reads project content.
The bundle is a captured set of bytes, not a claim of an atomic filesystem snapshot.
Recheck observed identities/metadata and the registered manifest identity before
and after capture; reject observed races, links, hardlinks and nonregular objects.
The M0 privileged-device/mount and trusted-root assumptions remain explicit.
Descendant descriptors are not retained after capture: matching observed metadata
is not proof of atomicity or inode history. Original-root authority still governs
every reopen; the registered project directory itself remains pinned. Existing
host absolute-path limits can reject a layout before relative source limits.

Initial source-transfer subset: only the selected project subtree; reject external
or absolute Cargo filesystem paths (dependencies, targets, build scripts, package
workspace/readme/license-file and workspace equivalents, including [patch] path dependencies) rather than rewriting
manifests. Legacy [replace] is rejected. Relative paths must remain inside the selected subtree. Exclude .git and
target directories case-insensitively at every depth. Reject project Cargo configuration (.cargo/config and
.cargo/config.toml, also case-insensitively), which can introduce wrappers/runners/linkers and credentials.
Only an explicit installed1.98.1 project toolchain with no installation request is
accepted; unsupported toolchain config fails closed. No manifest or lockfile is
rewritten. Limits:4096 files/directories, depth32, path100 ASCII bytes (portable
USTAR name subset),1MiB/file and16MiB total regular bytes. Unsupported names or
layout return a typed rejection. Cargo path globs are unsupported; include/exclude
and workspace members accept literal paths only. Parsed Cargo/toolchain TOML keeps
the M0 limit256KiB even though ordinary source files may reach1MiB. Cancellation checkpoints precede reads and
directory traversal. Domain/application remain free of filesystem and Cargo APIs.

The gateway encodes regular files/directories into bounded USTAR, with no
links, sparse/PAX/GNU extensions, devices or permissions beyond read-only source.
A random per-job Docker-managed local volume is explicitly created with empty
driver options, no subpath and no copy-up. Verify absence before create and exact
name/nonce labels/driver/options afterward. The trusted daemon owns namespace
race prevention; names and labels are not security against a hostile daemon.

The generated archive uses root-owned0755 directories (so caps0 extraction can
populate descendants) and0444 regular files; the final volume mount provides the
read-only enforcement. Fixed tar args use --keep-old-files, not the incompatible
--no-overwrite-dir combination.

A fixed trusted tar ingestion container may run UID0 with all capabilities
removed solely to populate the root-owned volume. It has no project-code entry
point, no host mounts, a read-only rootfs, clean environment and socket-deny profile.
The finite generated archive bounds correct ingestion; ordinary local volumes have
no demonstrated hard quota against a compromised extractor. Ingestion is not
advertised as strict project-code execution. Remove and verify the sole writer
before Cargo starts. Cargo runs nonroot/caps0 with the volume mounted read-only,
private bounded target/temp tmpfs (/work explicitly exec for compiled build scripts
and test binaries, /tmp noexec), closed commands and frozen/offline Cargo mode.
No input from source selects programs, arbitrary flags, network or host paths.

Each applied container and volume configuration is verified before use. All
paths are guest constants or validated relative archive names. On completion,
timeout, cancellation or overflow, kill/remove every owned container, verify
absence, then remove/verify the volume. Uncertain cleanup quarantines the gateway.
Startup refuses labelled leftovers; recovery requires explicit host cleanup, not
automatic reclamation of unknown work.
Cargo/project-code capability requires independent calibration of this exact
runtime/profile/transfer path using real build.rs/proc-macro adversaries in Docker.
Probe-only M0 evidence cannot authorize it. No runtime provisioning/downloads.

Real Rust1.98.1 requires flock and, on its fork/exec path, anonymous AF_UNIX
SOCK_SEQPACKET socketpair IPC. The Rust profile separately permits flock, socketpair
only with family AF_UNIX/type SOCK_SEQPACKET/protocol0, and send/receive syscalls
for those inherited local endpoints. General socket, bind, connect, listen and
namespace creation remain denied, with no inherited network descriptors and
network=none. This is private IPC, not a relaxation to network allow. Calibrate
these exact deltas and adversarial network attempts; M0 profile bytes stay unchanged.

## Alternatives considered

- Host bind mounts after canonicalization: reintroduce TOCTOU and broad host access.
- Docker cp into read-only rootfs: rejected by the actual daemon.
- Writable source/rootfs during Cargo: contradicts the M1 source-write policy.
- Root Cargo or CAP_SYS_ADMIN remount helper: unnecessary project privilege.
- Rewrite absolute dependencies/config or create Cargo.lock in an ephemeral copy:
  changes semantics/fingerprints; rejected for this initial subset.
- Treat local volumes as quota-enforced: unsupported by current evidence.

## Consequences

The initial layout is deliberately restricted and can reject projects accepted by
structural project.open. The runtime is a separately approved Linux environment;
results must identify it. Hostile code never runs on the host. Source capture,
archive encoding, ingestion, Cargo containment and cleanup need separate adversarial
tests and review before the vertical is Done. Registry/cache dependency provisioning
is explicit; missing offline dependencies never trigger downloads.

## Sources

- https://static.rust-lang.org/dist/channel-rust-1.98.1.toml
- https://docs.docker.com/reference/cli/docker/container/cp/
- https://docs.docker.com/reference/cli/docker/volume/create/
- https://docs.docker.com/engine/storage/volumes/
- Moby6a43e3d api/types/mount/mount.go and daemon/volume/local/local_unix.go.
- ADR-007, ADR-009, ADR-024 and ADR-025.

Observed Docker29.7.2 managed-volume Mounts.Mode is exactly z; this default label
mode is verified together with volume identity and RW=false during Cargo. It does
not grant host mounts or writable source. Explicit /work exec is required: a real
build-script fixture otherwise fails EACCES despite successful compilation.

Review clarification: 0755/0444 are the generated archive modes; extraction umask
may make them more restrictive. Runtime readability is checked by actual Cargo.
Cgroups and tmpfs enforce the calibrated quotas; rlimit soft/hard defaults are
not pinned. prlimit64 may change a process's own limits within its existing rights.
The profile permits ioctl on the guest's private devices; no host tty/descriptors
are passed and no controlling tty is allocated. /work and /tmp mount policies do
not claim that every other Docker-managed mount is read-only or noexec. /dev/shm
is separately checked rw/noexec. Applied checks cover security-relevant expected
fields; unknown daemon metadata is not asserted to be an exhaustive configuration.

Execution deadlines cover preflight, transfer and Cargo, not solely guest CPU time.
An operation may time out before any guest starts; calibration interruption tests
require an independent live descendant witness. Harness/configuration/cleanup errors
always propagate even when a deadline or cancellation is simultaneously observed.
Completed-container OOMKilled evidence is retained. Execution identity distinguishes
project admission from calibration and includes an implementation digest covering
verifier, supervisor, archive/source bounds, calibration, lock and toolchain inputs.
The JSON receipt remains historical evidence and is never accepted as authority.
