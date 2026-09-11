## Task

Implemented the orchestrator-authorized ADR-065 coverage target volume and prepared the complete 55-test qualification gate.

## Result

**Coverage implementation is complete, but Docker qualification is blocked by the current sandbox.**

The configured socket consistently returns:

```text
permission denied while trying to connect to the docker API at unix:///Users/cburgosro/.docker/run/docker.sock
```

Therefore:

- Coverage Docker tests: not qualified.
- Complete M3 gate: 0/1 attempted, 54 not executed.
- Rust security refresh: 0/1 attempted, 19 not executed.
- Coverage calibration: pending; no values were fabricated.
- ADR-065 remains Proposed.
- Historical receipts were preserved; `M3-runtime.json` and `M3-rust-security.json` were not replaced.
- All 23 snapshots remain byte-identical.

## Implementation

ADR-065 introduces a per-job named tmpfs mounted at `/work/coverage-target` with:

```text
size=512m,nr_inodes=65536,uid=65534,gid=65534,mode=0700,nosuid,nodev
```

It is:

- Writable and executable only during `CoverageRun`.
- Read-only during the coverage keeper and report phases.
- Absent during exports and every non-coverage phase.
- Removed with the source and report volumes; cleanup uncertainty quarantines the gateway.

The source/report mounts, seccomp profiles, network mode, user, capabilities, root filesystem, and resource limits remain unchanged.

A cleanup defect found during review was also fixed: target-volume removal now verifies against the ADR-065 option profile instead of incorrectly requiring the shared `noexec` report profile.

## Files changed

The complete 24-file manifest, including full SHA-256 values, is [Q02-files.sha256](/Users/cburgosro/Projects/rust-mcp/docs/validation/Q02-files.sha256).

Manifest SHA-256:

```text
703a001fcb7a3aa7d8c2c3b6a39ca210b31c5912e973fd30b4a55506d316da19
```

Principal files:

- [ADR-065](/Users/cburgosro/Projects/rust-mcp/docs/adr/ADR-065-coverage-target-volume.md)
- [coverage_gateway.rs](/Users/cburgosro/Projects/rust-mcp/crates/execution-adapter/src/coverage_gateway.rs)
- [rust_gateway.rs](/Users/cburgosro/Projects/rust-mcp/crates/execution-adapter/src/rust_gateway.rs)
- [rust_applied.rs](/Users/cburgosro/Projects/rust-mcp/crates/execution-adapter/src/rust_applied.rs)
- [coverage_runtime.rs](/Users/cburgosro/Projects/rust-mcp/crates/execution-adapter/tests/coverage_runtime.rs)
- [M3-03.md](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-03.md)

## Tests executed

| Command/gate | Result |
|---|---|
| `cargo fmt --check` | Exit 0 |
| `cargo check --workspace --all-targets --locked --offline` | Exit 0, 8.3 s |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | Exit 0, 6.6 s |
| Execution-adapter library tests | Exit 0; 175 passed, 10 ignored |
| ADR-065 applied-mount tests | Exit 0; 2 passed |
| Target/report profile parser regression | Exit 0; 1 passed |
| Durable artifact store focused test | Exit 0; 27 passed, 4 ignored |
| `python3 -B scripts/check-architecture.py` | Exit 0 |
| `cargo test --workspace --locked --offline` | Exit 101; MCP binary: 220 passed, 5 loopback-socket tests failed with `Operation not permitted` |
| Supplementary workspace run excluding those five | Advanced until another Unix-socket fixture was denied; 17 passed, 1 environmental failure in that target |
| Focused coverage Docker oracle | Exit 101; 0 passed, 1 failed with `Unavailable` |
| Complete 55-selection M3 gate, attempt 6 | Exit 1; 0/1, then fail-fast; 4.244 s |
| Rust security 20-selection gate | Exit 1; 0/1, then fail-fast; 5.833 s |
| Docker container/volume hygiene queries | Exit 1; socket permission denied |
| Snapshot comparison | 23/23 byte-identical |

Receipts:

- [M3-runtime-attempt6.json](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-runtime-attempt6.json)
- [M3-runtime-attempt5.json](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-runtime-attempt5.json)
- [M3-rust-security-attempt-Q02-2.json](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-rust-security-attempt-Q02-2.json)
- [M3-rust-security-attempt-Q02.json](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-rust-security-attempt-Q02.json)

## Calibration

| Item | Q02 observation | Status |
|---|---|---|
| ADR-065 target shape | Pinned in gateway, verifier, fingerprint and ADR | Unit-verified |
| cargo-llvm-cov identity | Docker not reached | Pending |
| Exact JSON schema and known counts | No report produced | Pending |
| JSON/LCOV/HTML from one profdata | No report produced | Pending |
| Shared-file dedupe | Docker not reached | Pending |
| Zero-denominator behavior | Docker not reached | Pending |
| HTML/LCOV measured sizes | Docker not reached | Pending |

## Evidence

| Oracle | Test |
|---|---|
| Exact target options and RW/RO matrix | `coverage_mounts_enforce_the_adr065_access_matrix_and_exact_options` |
| Target absent from non-coverage phases | `coverage_target_is_absent_from_every_non_coverage_phase` |
| Target cleanup validates its own profile | `mutation_volume_requires_exact_tmpfs_quota_identity_and_ownership` |
| Configuration identity includes generated mount arguments | `configuration_identity_covers_every_generated_argument` |
| Source/report execution and AF_INET negative controls | `hostile_html_is_retained_only_as_opaque_archive_bundle` — written, Docker pending |
| Joined timeout cleanup | `timeout_mid_build_is_blocked_after_joined_cleanup` — written, Docker pending |
| Cancellation followed by immediate reuse | `cancel_or_eof_joins_active_child_before_capacity_reuse` — written, Docker pending |

## Risks and open issues

- The current code and global gateway fingerprint have not received the required 55/55 and 20/20 Docker qualification.
- The 512 MiB bound is justified against the existing executable build ceiling and inode density, but could not be measured against real instrumented output.
- Coverage parser goldens and hand-derived counts remain provisional.
- Docker hygiene could not be queried. No Docker API operation succeeded during Q02, so this session could not create objects, but zero pre-existing owned objects cannot be asserted.
- Six non-Docker tests that create local sockets are also incompatible with this session’s sandbox.
- A new run with actual Unix-socket authority is required; no containment decision is outstanding.

The sprint workflow influenced the work by keeping implementation, focused verification, immutable failed receipts, and final qualification status separated so the environmental block was not reported as a product pass.