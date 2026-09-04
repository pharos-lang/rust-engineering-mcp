# M1-01 prerequisite — Rust gateway calibration

Observed 2026-09-04. M1-01 remains In progress: this unit validates execution and
source transfer; `rust.project.inspect` is not yet an operative MCP tool.

## Configuration and evidence

Host macOS26.6.2/APFS ARM64, Rust/Cargo1.98.1 and CARGO_INCREMENTAL=0.
Docker29.7.2/API1.55, Linux/aarch64, runc1.3.6/cgroupsv2. Approved image:
`sha256:8fac70723a8d04b6ec9633ab721806b8a55f4f083a1b3f988c61bf6a00fa1909`.
Provisioning was explicitly authorized and recorded in M1-01-runtime.md.

[Actual calibration receipt](artifacts/M1-01-rust-calibration.json) contains time,
image, fixture/configuration/execution fingerprints, bounded observations and
independent daemon process tables. It is historical evidence; loading this JSON
cannot authorize execution. A newly constructed RustGateway denies project jobs
until its own fixed fixtures succeed. Recalibration revokes previous admission;
cancellation cannot leave it enabled. Every job rechecks engine and CLI identity,
verifies applied containers/volume, and quarantines uncertain cleanup.

| Scenario | Discriminating evidence |
| --- | --- |
| Source transfer | Generated USTAR with empty and exactly100-byte directory names is extracted by actual GNU tar; build.rs asserts both directories and Cargo succeeds. |
| Build script | Cargo actually runs build.rs; UID/GID65534, caps0, seccomp/NNP, source/root read-only, bounded writable tmpfs, synthetic-secret absence, denied network sockets and positive local IPC/child controls. |
| Proc macro | A consumer invokes the compiled macro; the executing process independently passes the same checks. Compiling an unused macro is insufficient. |
| Resource enforcement | /tmp and /work ENOSPC; PID helpers hit EAGAIN and are reaped; memory helper receives SIGKILL with oom_kill counter increment; CPU threads increase throttling counter. |
| Timeout | Daemon observes build parent and double-fork/setsid descendant in distinct sessions, then result is timed_out and owned containers/volume are verified absent. |
| Cancellation | Cancellation is triggered only after that independent live descendant witness; synchronous cleanup precedes cancelled result. |
| Output overflow | A live detached descendant is observed before Cargo warning output exceeds16KiB; output_limit survives subsequent cleanup, with verified absence. |

The six calibration executions include resources plus three interruption scenarios.
The second ignored test also executes metadata successfully after calibration,
then proves an explicitly cancelled recalibration revokes admission. No hostile
fixture is a host Cargo package; only include_str bytes enter the bounded gateway.
The fixture's /tmp noexec check is mount metadata, not an executable attempt.

## Validation

- `python3 scripts/gate.py core --report target/M1-01-rust-gateway-core.json`:
  [ten stages passed](artifacts/M1-01-rust-gateway-core.json),245 Rust tests including
  doctest. No Cargo.lock/dependency change. audit/deny retain the documented
  paste1.0.15 maintenance warning and duplicate-version warnings.
- `RUST_MCP_TEST_SOCKET=<LOCAL_HOME>/.docker/run/docker.sock python3
  scripts/test-rust-execution.py`: both exact ignored tests passed sequentially;
  logs/JSON are under target/rust-security. The script seeds a synthetic host
  environment canary and never provisions an image. It is now a required full stage.
- First core attempt failed because the new security sources lacked corpus hashes;
  added their exact hashes, preserving old entries and host-execution allowlist.
  The first Rust script wrapper missed libtest's same-line prefix when extracting
  a successful receipt; corrected extraction and reran both tests successfully.
- [Opus5 High security review](../reviews/M1-01-rust-gateway-claude-opus-5.md)
  completed. Fixed error masking during interruption, retained observed OOM,
  expanded applied checks and added syscall/FD evidence plus implementation/admission
  fingerprints. Final core and actual Rust gates passed after fixes. Git integration
  and smoke are recorded by the integrating commit and the following unit.

Earlier prerequisites are integrated locally: runtime d247d37/fafa468 with eight
version/hash/package checks; bounded stdin8faddfd/b9c673b with nine supervisor smoke
tests; reviewed source8614b44/e2d6ae0 with18 source smoke tests. Branches are retained.

## Limits and remaining work

ADR-031 defines the narrow source layout and separate Rust seccomp profile. The
source set is not an atomic filesystem snapshot. Missing offline dependencies,
unsupported source/config/toolchain layouts and missing runtime fail closed.
Managed local volumes have no demonstrated hard quota against a compromised
trusted ingester; the final project-code container has read-only source and bounded
tmpfs. Cleanup relies on the trusted daemon; controls have deadlines, but kernel
or daemon failure cannot be represented as proven absence. An uncertain cleanup
quarantines the instance and startup refuses labelled leftovers.

MCP bootstrap readiness, joined worker cleanup/session shutdown, metadata parsing,
ProjectRef revalidation before publication, public contracts and Resources remain
pending. M0 probe authorization, native platform limits, distribution licenses,
third-party MCP clients and utility benchmarks remain unchanged. The integral full
E5/LanceDB gate will run at the requested M1 closure, with explicit unchanged assets.
