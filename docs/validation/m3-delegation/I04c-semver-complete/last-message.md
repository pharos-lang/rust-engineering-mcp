## Task

Completed the non-Docker portion of M3-04, integrating `rust.semver.check` end to end. Docker qualification remains intentionally pending for the next exclusive socket window.

## Result

- Added stable baseline → candidate capture with immediate and pre-publication revalidation.
- Enforced one closed, identical Cargo selection for both sides.
- Added dual read-only `/source` and `/baseline` gateway mounts.
- Added quality seccomp selection, fixed Git-discovery environment, bounded output and joined cleanup.
- Missing library targets return `Unavailable` before semver execution.
- Added bounded coarse parser with `Partial`/`Incomplete` semantics; parser uncertainty never becomes “no break”.
- Added Stage 1 durable raw-output publication, with Stage 0 fallback when no qualified state root exists.
- Registered `rust.semver.check` as tool 21, after coverage. The concurrent mutation vertical makes the current total 22 tools.
- Added five-version protocol coverage, a closed schema, 512 KiB response enforcement, fixtures and ignored Docker tests.
- Wired all 18 semver Docker selections into `scripts/test-m3-runtime.py` without executing Docker.

## Files changed

The complete 102-file inventory, with full SHA-256 for every source, test, fixture, snapshot, documentation and script file, is in [M3-04-files.sha256](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-04-files.sha256).

Manifest SHA-256:

```text
904c0ead1c4926dd3f4507f7df0f77e780e2246549e2a5fe167cee69a9d89e20
```

Principal additions:

- [domain semver model](/Users/cburgosro/Projects/rust-mcp/crates/domain/src/semver_check.rs)
- [application use case](/Users/cburgosro/Projects/rust-mcp/crates/application/src/semver_check.rs)
- [application integration tests](/Users/cburgosro/Projects/rust-mcp/crates/application/tests/semver_check.rs)
- [dual-volume gateway](/Users/cburgosro/Projects/rust-mcp/crates/execution-adapter/src/semver_gateway.rs)
- [bounded output parser](/Users/cburgosro/Projects/rust-mcp/crates/execution-adapter/src/semver_output.rs)
- [application-port adapter](/Users/cburgosro/Projects/rust-mcp/crates/execution-adapter/src/semver_port.rs)
- [Docker runtime selections](/Users/cburgosro/Projects/rust-mcp/crates/execution-adapter/tests/semver_runtime.rs)
- [MCP tool](/Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/semver.rs)
- [tool snapshot](/Users/cburgosro/Projects/rust-mcp/crates/mcp-server/tests/snapshots/semver-tool.json)
- [MCP runtime tests](/Users/cburgosro/Projects/rust-mcp/crates/mcp-server/tests/inspection_runtime/semver.rs)
- [calibration record](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-04-semver-calibration.md)
- [runtime gate](/Users/cburgosro/Projects/rust-mcp/scripts/test-m3-runtime.py)
- [fixture corpus](/Users/cburgosro/Projects/rust-mcp/fixtures/semver)

All pre-existing tracked snapshots remain byte-identical.

## Tests executed

| Command/gate | Exit | Result |
|---|---:|---|
| `cargo fmt --check` | 0 | Pass |
| `cargo check --workspace --all-targets --locked --offline` | 0 | Pass |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | 0 | Pass |
| Domain semver tests | 0 | 5 passed |
| Application semver unit tests | 0 | 3 passed |
| `cargo test -p rust-engineering-application --test semver_check --locked --offline` | 0 | 5 passed |
| Execution semver unit tests | 0 | 9 passed |
| MCP semver unit tests | 0 | 2 passed |
| `cargo test -p rust-engineering-mcp --test protocol --locked --offline` | 0 | 42 passed |
| Qualified non-Docker workspace suite | 0 | 1,042 passed, 0 failed, 109 ignored, 26 filtered |
| `python3 -B scripts/check-architecture.py` | 0 | Pass |
| Runtime-script syntax/selection validation | 0 | 18 semver selections |
| Fixture lock validation | 0 | 11 pairs, 22 handwritten locks |
| SHA-256 manifest verification | 0 | 102 entries verified |
| `git diff --check` | 0 | Pass |

The first unfiltered workspace run stopped with five local-socket `EPERM` failures after 219 passes; a sixth socket-dependent case was subsequently identified. These are sandbox restrictions, not SemVer failures. The qualified rerun excluded those six cases plus one independently clock-sensitive artifact-expiry test. That artifact test passed on its third exact retry.

No Docker command was executed.

## Evidence

- Capture ordering and revalidation: `capture_order_and_final_revalidation_are_stable`.
- Dual-root durable publication: `durable_publication_receives_both_captures_and_revalidates_both_roots`.
- Missing library short-circuit: `missing_candidate_library_is_unavailable_before_execution`.
- Stage 0 quota behavior: `stage0_quota_omits_raw_output_without_changing_the_coarse_outcome`.
- Cancellation rollback: `cancellation_after_raw_capture_rolls_back_the_new_artifact`.
- Mount containment:
  - `semver_baseline_ingest_is_writable_only_at_the_dedicated_mount`
  - `semver_requires_exactly_two_distinct_read_only_source_volumes`
- Closed argv/environment/seccomp mapping: `semver_argv_environment_and_quality_profile_are_closed`.
- Parser uncertainty and bounds: six `semver_output` tests.
- Five-version schema and synchronous gate: `semver_contract_and_synchronous_gate_are_stable_in_all_wire_versions`.
- 512 KiB response bound: `maximally_escaped_itemized_findings_keep_the_mirrored_result_below_512_kib`.

## Decisions

- Raw evidence uses the gateway’s fixed, supervised attached stdout/stderr streams. The pinned plugin offers no report path; introducing a shell or guest-selected path would weaken the boundary.
- Durable evidence is candidate-owned, while both roots are revalidated at every publication checkpoint. Its source identity is a domain-separated digest of the ordered baseline/candidate pair.
- Per-finding output remains best effort and never exceeds `Partial`; unrecognized or truncated parser input becomes `Incomplete`.
- Exit mappings remain explicitly uncalibrated until Docker observation.
- Two bounded coverage integration repairs and its current generated snapshot were needed after the concurrent coverage draft stopped; no coverage semantics were redesigned.
- The sprint workflow guidance shaped the phased implementation, adversarial tests and evidence-first closure.

## Risks

- Plugin exit codes, exact text shapes and the rustdoc target directory remain hypotheses until Docker calibration.
- The current durable-store schema has one live owner, so Resource-read authorization is candidate-bound. Both roots are checked through publication, but dual-root liveness at later read time would require a D17 schema/authority extension.
- If real rustdoc behavior writes into `/source` or `/baseline`, the gateway must remain fail-closed and the ADR decision must be revisited; mount containment must not be weakened.

## Open issues

The following exact Docker selections await execution.

Execution adapter:

1. `identical_libraries_exit_zero`
2. `removed_public_function_is_a_deny_level_break`
3. `trait_method_with_default_is_compatible`
4. `trait_method_without_default_is_a_break`
5. `non_exhaustive_enum_variant_addition_is_compatible`
6. `exhaustive_enum_variant_addition_is_a_break`
7. `feature_gated_removal_uses_the_identical_selection_on_both_sides`
8. `feature_gated_removal_without_feature_has_no_signal`
9. `no_lib_gateway_exit_is_calibrated_before_application_maps_unavailable`
10. `broken_baseline_is_incomplete_not_a_compatibility_pass`
11. `warn_level_only_findings_are_surfaced_under_exit_zero`
12. `exit_100_with_zero_parsed_findings_is_blocked`
13. `planted_git_directory_is_never_discovered`
14. `registry_dependent_baseline_fails_recognizably_without_hanging`
15. `cancellation_or_eof_with_active_child_is_joined_before_return`
16. `baseline_and_candidate_roots_are_immutable_after_run`

MCP:

17. `semver_runtime::mcp_semver_projects_findings_and_reads_bounded_raw_resource`
18. `semver_runtime::mcp_semver_without_state_root_uses_stage0_raw_resource`

These selections, fixture hashes and the exactly-one-test rule are now encoded in [test-m3-runtime.py](/Users/cburgosro/Projects/rust-mcp/scripts/test-m3-runtime.py). M3-04 correctly remains **In progress — pending Docker calibration**.