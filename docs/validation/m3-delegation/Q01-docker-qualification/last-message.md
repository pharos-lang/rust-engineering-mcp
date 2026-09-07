## Task

Q01 completed as a single serial Docker qualification window against:

`sha256:384a1742ecc53cdd3a9c0bf36c6f8b66db73ddd118aeeae6e55654ea998ae36a`

No commits, installs, downloads, image pulls/builds/deletes, or containment widening occurred.

## Result

| Cut | Result | Evidence |
| --- | --- | --- |
| M3-01 nextest | Qualified, 19/19 reconfirmed | [M3-01.md](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-01.md) |
| M3-03 coverage | **Blocked** | Persistent target must be executable across run/report containers, but the approved report volume is `noexec`; first oracle exits 101 with `Permission denied`. [M3-03.md](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-03.md) |
| M3-04 SemVer | Qualified, 18/18 | [M3-04.md](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-04.md) |
| M3-05 mutation | Qualified, 10/10 | [M3-05.md](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-05.md) |
| Rust security | Qualified, 20/20 | [M3-rust-security.json](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-rust-security.json) |

The consolidated M3 gate correctly remains failed: 47 passed, one coverage failure, seven coverage selections not executed. No skip was credited as a pass.

## Files changed

All 60 Q01-owned path/hash pairs are recorded in [Q01-files.sha256](/Users/cburgosro/Projects/rust-mcp/docs/validation/Q01-files.sha256), itself SHA-256:

`5e1eabb6dafb9b374809a65c87f9280a24a517810026a01379824be5545e676d`

Major changes:

- Expanded `test-m3-runtime.py` to 55 exact selections: nextest 19, coverage 8, SemVer 18, mutation 10.
- Corrected coverage environment scoping, report argv, and cross-container target persistence until reaching the unauthorized `noexec` boundary.
- Calibrated SemVer exits and its bounded parser against real non-colored output.
- Canonicalized all SemVer fixture locks and implemented the real durable-store `Busy` fallback oracle.
- Made only mutation’s private scratch copy writable, preserving read-only source transfer elsewhere.
- Corrected the `caught-all` fixture’s zero-boundary oracle.
- Pinned mutation exit codes and real `mutants.out` schema/layout.
- Updated ADR-062, matrix, implementation status, calibration records, and narratives.
- Preserved all failed receipts byte-for-byte.

All 23 tool snapshot hashes are byte-identical to their pre-Q01 values.

## Tests executed

| Command/gate | Result | Counts / duration | Receipt |
| --- | --- | --- | --- |
| `cargo fmt --check` | Exit 0 | Final workspace check green | — |
| `cargo check --workspace --all-targets --locked --offline` | Exit 0 | 22.8 s | — |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | Exit 0 | 7.2 s final run | — |
| `cargo test --workspace --locked --offline` | Exit 0 | 82 suites; 1,052 passed, 0 failed, 109 ignored; ~60 s final measured run | — |
| `python3 -B scripts/check-architecture.py` | Exit 0 | All four architectural checks passed | — |
| M3 attempt 1 | Failed as expected | 19 pass, 1 fail; 464.599 selected seconds | [attempt1](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-runtime-attempt1.json) |
| M3 attempt 2 | Failed; noncanonical fixture lock | 21 pass, 1 fail; 567.196 s | [attempt2](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-runtime-attempt2.json) |
| M3 attempt 3 | Failed; synthetic slow lock | 33 pass, 1 fail; 661.897 s | [attempt3](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-runtime-attempt3.json) |
| M3 attempt 4 | Coverage blocked | 47 pass, 1 fail; 931.958 s | [current receipt](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-runtime.json) |
| `python3 -B scripts/test-rust-execution.py` | Exit 0 | 20/20; 533.511 selected seconds | [security receipt](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-rust-security.json) |

Receipt hashes:

- Attempt 1: `c20c11a98a986de2bb6e8825dfc0db27d2b35587355f9d66bafa057fc60edf1e`
- Attempt 2: `980476384bd6b3c4a2c85cffedc9bb7ab08970a21fecffd7cdff66fddea1913e`
- Attempt 3: `add344961112e59b8434d673e5e266936c7085f0cd2c7a6bfa500501116ea33c`
- Attempt 4/current: `7ffd15ffafb051e2c5c70da295c9cc5baa9a3c090e2b4d3e39946ff0a8c2973d`
- Security: `0e50923acc9b631186d44f5dc87834fabd76caa3fffbf08be4d51a8794bbf588`

The durable-store expiry tests passed repeatedly. I did not edit the concurrently owned quality-store test.

Docker hygiene after the gates: zero containers and zero volumes labeled `org.rust-mcp.execution=true`.

## Calibration

| Tool | Observed behavior | Pinned in |
| --- | --- | --- |
| nextest 0.9.143 | pass 0; failure/timeout/leak 100; build 101; no-tests 4; cancellation has no publishable exit; 104 never observed | `NextestExit`, M3-01 |
| semver-checks 0.50.0 | compatible/warn-only 0; breaking 100; no-lib/broken/registry-required 101 | `SemverExit`, real parser goldens, ADR-062 |
| cargo-mutants 27.1.0 | success 0; usage/integration failure 1; missed 2; timeout 3; baseline failure 4 | `MutationExit`, mutation calibration |
| coverage | Run reaches cargo test but exits 101 before report because executable is on a `noexec` persistent volume | Not calibrated; remains blocked |

SemVer observed headers include:

- `--- failure function_missing: ... ---`
- `--- warning function_missing: ... ---`
- `223 checks: 222 pass, 1 fail, 0 warn, 31 skip`

Mutation observations include caught-all 14/14, missed-one 6/19 missed, timeout-loop 2/6 timeout, unviable 1/1, and baseline failure with zero mutants executed.

## Evidence

SemVer oracles map to:

- Compatible/breaking families: `identical_libraries_exit_zero`, `removed_public_function_is_a_deny_level_break`, trait and enum paired tests.
- Feature symmetry: both `feature_gated_removal_*` tests.
- Incomplete cases: `no_lib_gateway_exit_is_calibrated_before_application_maps_unavailable`, `broken_baseline_is_incomplete_not_a_compatibility_pass`.
- Warn-only: `warn_level_only_findings_are_surfaced_under_exit_zero`.
- Zero parsed evidence policy: runtime exit-100 oracle plus `parser_uncertainty_and_breaking_without_a_deny_row_fail_closed`.
- Git/network/cancellation/immutability: their four named runtime selections.
- Resources/fallback: both `mcp_semver_*` selections.

Mutation’s ten exact tests cover caught, missed, timeout, unviable, baseline failure, hostile output, immutability, pre-build cap, cancellation cleanup, and bounded no-host extraction.

Coverage’s exact blocking oracle is `known_counts_fixture_has_exact_line_region_and_function_oracle`; the remaining seven were not run after fail-fast termination.

## Decisions

- Chose one consolidated immutable M3 receipt plus per-cut narratives.
- Did not authorize an executable persistent coverage mount.
- SemVer section headers are primary; legacy detailed lines are a whole-stream fallback, preventing duplicate findings.
- Mutation regular files become 0644 only in the private scratch archive.
- Applied the sprint workflow’s evidence-first, serial qualify/diagnose/record discipline.

## Risks

- M3-03 has no qualified metrics, LCOV, HTML, parser golden, or artifact-size measurement.
- Nextest exit 104 remains deliberately `Uncalibrated` because no real timeout/cancellation path emitted it.
- Mutation discards raw `lock.json` identity values after validating guest hostname/user; the exact username is intentionally absent from receipts.
- MCP Tasks advertisement remains off.

## Open issues

The orchestrator must decide how coverage may persist an executable instrumented target between run and report containers. Current evidence is exact:

`/work/coverage/target/debug/deps/...: Permission denied (os error 13)`

Acceptable implementation work cannot continue without authority for either:

- a narrowly bounded executable persistent target volume, or
- a different phase composition that preserves `.profraw` and build outputs without widening mount execution permissions.

After that decision, run all 55 selections into a new `attempt5` receipt and replace only the current `M3-runtime.json`, preserving every prior attempt.