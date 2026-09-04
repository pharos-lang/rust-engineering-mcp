# M1-01 prerequisite — explicit Rust runtime provisioning

The owner authorized installation explicitly ("instalalo"). This is an offline-use
runtime prerequisite, not completion of project.inspect or sandbox calibration.
The MCP runtime never calls these provisioning scripts.

## Actual runtime

Docker29.7.2/API1.55, Linux/aarch64, runc1.3.6 and cgroupsv2 were observed via the
explicit local Docker socket. Official Docker Rust1.98.1 tags were unavailable.
ADR-031 selects the exact official Rust distribution on pinned official Debian.
Five component archives were independently rehashed against the recorded official
manifest values. No version, model or runtime substitution was made.

Immutable local image:
`sha256:8fac70723a8d04b6ec9633ab721806b8a55f4f083a1b3f988c61bf6a00fa1909`.
Rust/Cargo1.98.1, rustfmt1.9.0, Clippy0.1.98, GCC12.2.0. See the version, binary-hash
and installed-package [receipt](artifacts/M1-01-runtime-verification.json),
[image inspection](artifacts/M1-01-runtime-image-inspect.json) and
[base inspection](artifacts/M1-01-runtime-base-inspect.json).

The configuration has only Docker's standard PATH, UID65534, cwd/work, and no
entrypoint, command, on-build trigger or volume. Provisioning used official
network sources explicitly; execution verification used no project source and
network=none. The five downloaded archives remain under the ignored provisioning
output. TLS/checksums are integrity evidence, not a detached-signature claim.
Mutable Debian package indexes mean image rebuild identity is not reproducible;
execution must pin the observed immutable ID, never the convenience tag.

## Verification and feasibility

`python3 fixtures/rust-runtime/verify.py --docker
/Applications/Docker.app/Contents/Resources/bin/docker --host
unix://<LOCAL_HOME>/.docker/run/docker.sock --output
target/m1-runtime-provisioning` passed. Eight fixed version/hash/package queries
ran as nonroot, read-only, caps0, with bounded CPU/memory/PIDs. Each owned container
was checked absent afterward. Python syntax and diff checks passed. A provisioning
replay refuses unrelated or linked build-context entries rather than extracting
unchecked archives through the Dockerfile wildcard.

A separate benign feasibility experiment established that Docker rejects cp into
read-only rootfs, and that a managed local volume supports bounded generated tar
input through a root/caps0 ingester, removal of that writer, and successful frozen
Cargo compilation as nonroot with source mounted read-only. The
[raw experiment receipt](artifacts/M1-01-runtime-volume-feasibility.json) records
actual commands and verified cleanup. It is exploratory evidence, not a callable
production adapter and not a hostile build.rs/proc-macro test.

The experiment required a distinct Rust seccomp profile: flock plus anonymous
AF_UNIX/SOCK_SEQPACKET socketpair and send/receive syscalls used by Rust fork/exec.
General socket/bind/connect/listen stay denied. The M0 probe profile is unchanged.
Production profile fingerprinting, applied-volume verification, hostile Cargo
calibration, cancellation/overflow cleanup and MCP integration remain pending.

[Sonnet5 Medium review](../reviews/M1-01-runtime-claude-sonnet-5.md) completed;
principal disposition used actual Docker inspection and replay evidence.
No full milestone gate or M1 completion is claimed. E5 and ORT assets from M0-12
remain present; the full gate still requires explicit paths and real semantic
execution under macOS network deny. M0's native-platform, licensing, utility and
paste1.0.15 caveats remain unchanged.
