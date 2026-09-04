# ADR-025 — Bounded Docker/Linux execution boundary

## Status

Accepted for M0-05/M0-06, 2026-09-03. Does not enable any additional MCP tool.

## Context

The macOS host provides Docker Desktop; native process groups cannot contain a
setsid descendant and sandbox-exec alone cannot satisfy ADR-009. No approved Rust
Linux image is installed. A foundation gateway can be exercised using a locally
built, trusted static probe image without downloading or executing project code.

## Decision

The first execution adapter uses a local Docker Linux engine, a host-selected
absolute Docker executable and Unix socket, and an immutable local image ID. It
never pulls images, consults ambient Docker context, starts the daemon, or inherits
host environment at runtime. A host-authorized physical state directory holds
private control files opened relative to directory handles with no-follow semantics.
Initial state-file support is macOS/APFS; unsupported adapters fail closed.

M0-05's executable vertical is the closed set of trusted probe scenarios. Every
external process in product code belongs to the gateway. Commands, arguments,
working directory and environment come from enums and bounded types. `/work` is
an isolated tmpfs cwd, not a caller path. Host/project bind mounts, project Cargo
execution, and arbitrary programs are not accepted. M1 must add an approved Linux
Rust image and an independently reviewed source-transfer boundary; validating a
host path and then passing it to Docker would reintroduce TOCTOU. The host macOS
toolchain is never silently replaced by a Linux one.

Each execution owns a separate container, PID namespace and cgroup. The generated
configuration specifies read-only rootfs, no host mounts/ports, non-root user,
no capabilities, no-new-privileges, no healthcheck, no persistent daemon logs,
explicit tmpfs sizes, memory+swap ceiling, PID ceiling and CPU quota. An allowlist
seccomp profile denies socket creation (including loopback/IPv6/DNS), namespace
creation and mount operations; `network=none` alone is insufficient.

Timeout, cancellation and output overflow remove the whole container, not merely
the CLI process. Names are random and known before create, so cleanup also applies
to uncertain create outcomes. A failed/uncertain cleanup is an infrastructure
failure and disables the gateway; it is never reported as successful cancellation.
Daemon/host failure is outside synchronous cleanup guarantees and must be reported
explicitly; no best-effort process-group guarantee is promoted to strong containment.

Capabilities are distinct facts tied to the exact engine/image/profile and proven
by explicit M0-06 probes. `strict` requires all resource guarantees; `restricted`
requires filesystem, network, environment, descendants, wall/output and PID limits.
`none` never permits project code. Defaults deny project code. Missing, failed or
unverified guarantees cause rejection; no profile fallback.

The probe source is a development fixture written in Go solely to cross-compile
an offline Linux static executable using the already installed host toolchain.
It is not a Cargo adapter, Rust toolchain or dependency of the Rust library build.
It accepts only fixed scenarios and uses synthetic canaries. M0-06's explicit gate
builds its image from scratch with no registry dependency. A capability result
applies to this approved execution configuration, not arbitrary Docker images.

## Alternatives considered

- Native Unix process groups: do not prevent setsid escape.
- sandbox-exec fallback: lacks demonstrated aggregate resource/descendant guarantees.
- Automatically install a Linux Rust toolchain/image: violates explicit provisioning.
- Live bind mounts following pre-validation: do not preserve the filesystem boundary.
- Mark an unreachable daemon as having false capabilities: confuses unavailable
  evidence with demonstrated absence; report unavailable instead.

## Consequences

Docker daemon/VM and the selected image become trusted host infrastructure. Engine
availability is optional and runtime provisioning is explicit. Linux execution is
reported honestly. M0-05/06 do not deliver M1 Cargo tools or close M0. No host bind
mount or general-purpose executor is exposed to the MCP peer.

## Sources

- https://docs.docker.com/reference/cli/docker/container/run/
- https://docs.docker.com/engine/security/seccomp/
- https://docs.docker.com/engine/network/drivers/none/
- https://docs.docker.com/engine/storage/tmpfs/
- https://docs.kernel.org/admin-guide/cgroup-v2.html
- https://man7.org/linux/man-pages/man7/pid_namespaces.7.html

## Review refinements

The configuration identity hashes the full normalized generated argument vectors
for every closed scenario, engine, client digest and seccomp content. Per-execution
identity additionally binds scenario and budgets. Admission requires matching
configuration evidence; bare booleans cannot authorize another configuration.
Before start, inspect verifies the daemon-applied config, including empty CapAdd,
no inherited volumes/mounts, default-private PID mode, seccomp JSON and all budgets.
At startup any existing container with the gateway label rejects initialization;
this is conservative even if another healthy instance currently owns it. No
container or directory is silently deleted during reconciliation. Host operators
resolve stale resources explicitly; stale private directories alone grant no
execution authority. The label check makes persisted orphan containers visible
across gateway restarts, but cannot guarantee cleanup while the daemon is down.

State roots must be owned by the effective uid and not group/world writable.
The host also protects ancestor directories, executable and socket. No-follow
protects gateway opens; the trusted Docker CLI consumes validated control paths
by name. This is an explicit TCB assumption, not immunity to privileged or same-user
host namespace races. Applied seccomp content is checked before start. The client
binary digest is rechecked before execution (observed identity, not atomic exec).
Reader pipes are nonblocking, cooperatively stopped and joined on every return.
Run duration excludes create/inspect/remove; total duration is a separate field.
`/work` permits executable scratch artifacts inside the guest; `/tmp` is noexec.
Project programs remain unavailable regardless of probe tier evidence.

Runtime is explicitly runc (also verified in applied config and engine identity).
Init is disabled, proc masks/read-only defaults and absence of daemon-injected
ulimits/sysctls/userns/cgroup parent are checked. Applied gateway label is required.
Completed results require a real started/exited container without runtime error and
matching attach exit status; created containers are infrastructure errors. Output
budgets are per stream. Concurrency admission is per instance, never a global host
quota or cross-process lock. Trusted host deployment owns aggregate concurrency.

## M0-06 calibration contract

An explicit host CLI capabilities command runs the bounded probe suite and emits
a live JSON report scoped to the approved probe image/configuration. It records
observation time, exact configuration/execution identities, engine/platform and
per-capability evidence. Unknown/unavailable is not verified absence: infrastructure
failure emits unavailable and zero advertised guarantees; mismatched/failed probes
produce degraded, never fallback. No saved report is accepted as runtime authority.
Two internal calibration controls only relax seccomp for socket creation (no
connect/send) and read-only rootfs for the synthetic filesystem canary. The controls
are fixed scenario/profile pairs, not public ExecutionPort input, and retain all
other isolation/resources. Their exact generated arguments are fingerprinted too.
Only enforced profile results can advertise a capability. OOM requires Docker
State.OOMKilled, not merely exit137. Future Cargo/source-transfer remains unavailable.

M0-06 review refinement: socket control uses a second explicit profile whose only
delta is allowing socket; both contents are fingerprinted and applied JSON verified.
The fixture also measures UNIX and NETLINK socket creation. Malformed guest records
invalidate that probe, preserve observations and produce degraded with invalid_evidence;
only infrastructure failure yields unavailable. Report observations include numeric
wall/output and memory limits. No prior report is an input to the CLI or gateway.
