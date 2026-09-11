## Task

Applied all V-SEC dispositions and re-qualified the complete M3 runtime and Rust containment paths.

## Result

Accepted implementation result:

- M3 runtime: **55/55 passed**
  - nextest 19/19
  - coverage 8/8
  - semver 18/18
  - mutation 10/10
- Rust security: **20/20 passed**
- Non-Docker workspace: **1,066 passed, 0 failed, 109 ignored**
- Docker hygiene: **0 owned containers, 0 owned volumes**
- No containment rules were weakened.
- Tasks advertisement remains gated by M3-02/G4.

## Dispositions

| Finding | Fix location | Evidence/test | Status |
|---|---|---|---|
| V-SEC-01 | `resources.rs`, `quality_artifacts.rs` | `quality_read_and_tasks_get_stay_prompt_while_registry_is_contended` | Fixed |
| V-SEC-02 | Durable publishers and artifact accounting | `durable_batch_keeps_prior_and_later_members_when_one_member_fails`; `a_realistic_html_bundle_exceeds_the_old_cap_but_fits_the_durable_member_cap`; `an_archive_bundle_is_one_member_with_independently_validated_entries` | Fixed |
| V-SEC-03 | Shared `validated_closed_ustar` | `html_archive_accepts_only_the_tar_root_and_safe_regular_descendants`; `report_bundles_accept_only_a_rooted_regular_tree_owned_by_the_guest_user` | Fixed |
| V-SEC-04 | Literal ADR-065 phase table in `rust_applied.rs` | `coverage_mounts_enforce_the_adr065_access_matrix_and_exact_options`; `coverage_target_is_absent_from_every_non_coverage_phase` | Fixed |
| V-SEC-05 | Mutation domain/application/gateway/DTO/snapshot | `schema_is_closed_and_stable`; `host_source_and_canary_are_unchanged_after_every_mutation_run` | Fixed |
| V-SEC-06 | Architecture rule forbidding production hook installation | `scripts/check-architecture.py` | Fixed |
| V-SEC-07 | Shared domain `M2_RECOVERY_HEADROOM_BYTES` | `capacity_floor_reserves_the_m2_recovery_headroom_exactly`; native headroom test | Fixed |
| V-SEC-08 | Both Docker gate scripts | Per-step 900-second bound; pre/post-kill labeled Docker snapshots in failed receipts | Fixed |
| V-SEC-09 | ADR-060, Resource bootstrap and non-blocking policy | `bootstrap_gate_precedes_the_quality_resource_branch` | Fixed |

The mutation snapshot deliberately changed:

- Before: `6d96291bfe05824298901fff90853eb08c2d7dc1276c72948b206b730414ec50`
- After: `08f37709c507b827f22a58fb75a1f80a35b19da510f22c449e5ba0e6a55a080a`

The other 21 `*-tool.json` contracts stayed byte-identical. The snapshot directory’s additional non-tool snapshot is `doctor-report.json`.

## Files changed

All 40 changed files and their full SHA-256 values are recorded in [W3-files.sha256](/Users/cburgosro/Projects/rust-mcp/docs/validation/W3-files.sha256), whose own SHA-256 is:

`d84f0a8d8aa6d638380d7a733be3a117c6cd21330b192d397816fd100d1e5953`

The manifest passed `shasum -a 256 -c` for all 40 entries.

Current receipts:

- [M3-runtime.json](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-runtime.json) — `910c4f82d6421b06b58102ee03c73257e97e4078dab1981e1f01bc6e52678053`
- [M3-rust-security.json](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-rust-security.json) — `0af30efb3bab075fcab31b54f5b05514d32ec862afb42322b5ae8ff0b973f52d`
- Prior passing receipts preserved as `M3-runtime-pre-vsec.json` and `M3-rust-security-pre-vsec.json`.

## Tests executed

| Command | Exit | Result / duration |
|---|---:|---|
| `cargo fmt --check` | 0 | 0.72 s |
| `cargo check --workspace --all-targets --locked --offline` | 0 | 4.12 s |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | 0 | 0.16 s |
| `cargo test --workspace --locked --offline` | 0 | 1,066 passed; 109 ignored; 0 failed |
| `python3 -B scripts/check-architecture.py` | 0 | PASS |
| `RUST_MCP_TEST_SOCKET=… RUST_MCP_M3_RUNTIME_OUTPUT=target/m3-runtime-w3-attempt9 python3 -B scripts/test-m3-runtime.py` | 0 | 55/55; 1,074.118 s |
| `RUST_MCP_TEST_SOCKET=… RUST_MCP_RUST_SECURITY_OUTPUT=target/m3-rust-security-w3 python3 -B scripts/test-rust-execution.py` | 0 | 20/20; 534.663 s |

Per-cut runtime timing:

- nextest: 427.399 s
- semver: 287.560 s
- mutation: 221.369 s
- coverage: 137.790 s

The previously reported `catalog_sync` sandbox failure did not reproduce: the complete workspace suite passed.

## Docker hygiene

After each Docker gate, both commands reported zero:

```text
containers=0
volumes=0
```

Filters used: `org.rust-mcp.execution=true` against the configured Unix socket.

## Decisions

- Quality Resource reads deliberately remain outside the worker permit, behind `ready`, with fail-fast lock acquisition and masked contention.
- Durable member failures degrade that member and preserve later publications; failed finalization invokes reconciliation.
- An `ArchiveBundle` consumes one job member; its internal entries remain independently USTAR-bounded.
- Host-source immutability is enforced by the read-only mount and applied-mount verification and evidenced by the host canary—not asserted in mutation responses.
- Test-only durable-store hook use is guarded by an architecture rule, preserving integration-test usability without allowing production wiring.
- The evidence-first sprint workflow guided the review, focused regression tests, complete gates, and receipt preservation.

## Risks and open issues

- Tasks advertisement remains OFF pending M3-02 Docker/client G4 evidence.
- ADR-064 and ADR-065 remain Proposed until formal milestone acceptance.
- No gate timed out during requalification, so the timeout residue fields were not populated in these passing receipts; the bounded kill and before/after snapshot branches are implemented for the next actual timeout.
