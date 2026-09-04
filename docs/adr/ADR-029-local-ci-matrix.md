# ADR-029 — Initial local CI and explicit platform matrix

## Context

M0 requires CI but the owner requires local Git branches/merges, without remote
pushes, PRs or remote Actions. Only the macOS ARM64 Rust1.98.1 toolchain is installed.
Docker supplies Linux ARM64 containment probes, not a Rust/Cargo project runtime.
The earlier approved scope explicitly gates broader native support before M1 RC.

## Decision

Ship one local CI entrypoint `scripts/gate.py core|full` and a machine-readable
step report. Every failure exits nonzero; missing tools/assets and unsupported
full-gate platforms fail, never turn into passed/skipped certification. No automatic
install, refresh, image pull, model download or toolchain change in the gate.

Core runs locked/offline fmt/check/Clippy/tests/doctests, architecture/vendor checks,
reviewed Cargo fixtures and dependency audit/deny. Full additionally requires the
explicit Docker socket and local semantic assets, runs real containment tests and
capabilities, then feature-local inference/index tests under calibrated network deny.
Pin CARGO_INCREMENTAL=0 after observed stale incremental metadata during development.

Initial verified matrix is macOS26+/APFS ARM64 native plus Docker Linux ARM64 probe
containment. Native Linux, Windows and x86_64 remain unverified and unadvertised;
CI commands are ready for later runners, not evidence that those jobs ran. Windows
fixture harness needs a platform implementation; unsupported operation fails closed.
This completes initial M0 CI for the actually supported development matrix, not
cross-platform release qualification. M1 RC must provide real runners/evidence for
every advertised target and compile native ORT/LanceDB distributions there.

Deny checks known vulnerabilities, yanked crates, banned runtime network/download
features, profiler/test-only packages and unknown sources. No advisory ignore.
Direct unmaintained dependencies fail; transitive paste remains a documented visible
cargo-audit warning under ADR-027. Duplicate versions remain warnings. Licensing
is a separate mandatory M1 distribution gate because product license/redistribution
approval are still pending; M0 does not pretend `deny licenses` has passed.

## Alternatives considered

- GitHub Actions now: contradicts the explicit local-only workflow.
- Mark unavailable platforms green or silently skip full-gate assets: false evidence.
- Install toolchains/images/models automatically: violates explicit provisioning.
- Treat the Go probe container as a Rust Linux runner: does not test Cargo/native libs.
- Postpone all CI until a remote exists: local commands can enforce real gates today.

## Consequences

The report is evidence of a development run, not an authorization token accepted by
the runtime. macOS/Docker/ORT/model environment must be provisioned explicitly.
Core alone cannot close M0 or qualify an M1 release. Published native artifacts,
license review, third-party clients and the full M1 matrix remain release gates.

## Status

Accepted and implemented in M0-11; core entrypoint validated. Full integrated gate
required and recorded at M0-12. ADR-047 supersedes only the prohibition on remote GitHub
workflows and resolves the original-code license; the local gate and evidence limits
remain in force.

Sources: cargo-deny0.19.7 generated template/help and official
https://embarkstudios.github.io/cargo-deny/checks/advisories/cfg.html,
https://embarkstudios.github.io/cargo-deny/checks/sources/cfg.html.
