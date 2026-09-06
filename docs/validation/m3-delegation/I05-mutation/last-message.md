# I05 — M3-05 `rust.mutation.test` — delivery report

## Task
Implement the containment-critical M3-05 vertical: `cargo-mutants` over hostile project code in a private writable copy, mandatory baseline, `mutants.out`-only oracles, bounded egress, never touching host source.

## Result
**Complete, integrable non-Docker cut.** `rust.mutation.test` is tool 22, end to end: domain grammar → application contract + Stage-0 publication → execution gateway (6 phases, 2 volumes, 3 fixed-argv exporters) → port + bounded parsers → MCP tool with closed schema and snapshot. 44 new tests pass; 10 Docker tests are written and `#[ignore]`d (no Docker in this window). All five gates green.

## Files changed (SHA-256)

New (owned):
```
3bdfbe645a12bdabc89e146381402f9e87ab4e752dcf83ead308b50bf2fe8631  crates/domain/src/mutation_test.rs
6a28f1fec8dd6dd7aed225aa2a05040ae087446426e026a874d5c24c83459ce7  crates/domain/tests/mutation_test.rs
d0ede56e0aa16b97c1165109057d18865d62178730c377ed1a9c7d5e05e894ce  crates/application/src/mutation_test.rs
9208efe201b03cc51cb0730f83e1ce213364e900cd8bfa3b0c7814d83ee3a628  crates/application/tests/mutation_test.rs
e0de87d79e84f40b4f1959ca34dc733b07addba6b13b49491e154f54825a7b50  crates/execution-adapter/src/mutation_test_gateway.rs
8d32efd1b5ba66538fa6b576134bcae0335600e4378f512c90a05350cf8d37d6  crates/execution-adapter/src/mutation_outcomes.rs
49aa3f996b4a2a51d05f9365624429fc7ba42909fd07ecf38097b75f14bd593b  crates/execution-adapter/src/mutation_test_port.rs
e87706a5a7fa5a4edeb29638b5cb3c17be8a13e7843b582c80b9a40c58d70841  crates/execution-adapter/tests/mutation_runtime.rs
3fc86840057e834e3233294771b006be066f19c853302581893a1dc36157ac4e  crates/mcp-server/src/stdio/mutation_test.rs
ed6523a73fb595403f54c8e57c1de654fe43828c27d9ad33109dff386fc237c5  crates/mcp-server/src/stdio/mutation_test/tests.rs
6d96291bfe05824298901fff90853eb08c2d7dc1276c72948b206b730414ec50  crates/mcp-server/tests/snapshots/mutation-test-tool.json
8b65a96442732073500cd7147ff1ad72d3ee8fa3d62efc41dd97c1d793014c8c  docs/validation/M3-05-mutation-calibration.md
```

Minimal shared patches (hashes are of the combined state — other workers edited these concurrently):
```
cefa33f08bb2a300d7001b81f27f3c3aeff810714353481283eadbfe6d250d5c  crates/domain/src/lib.rs                       (one `pub mod` line)
36df7894336694b6e4c36925fb7a82aefb2ecbababaf508dc9f8001d9ad803ff  crates/domain/src/rust_execution.rs            (MutationTest, MutantsVersion)
4aee09bfe3cba4c10cee02e720b238dcb24bc9917203f0a546ebb2f07a2216ac  crates/application/src/lib.rs                  (one `pub mod` line)
27221de06fe3ec55cae29b51279628c7244d00117d2bf77e93920d0c096b51b5  crates/execution-adapter/src/lib.rs            (3 mods + 1 re-export)
dd4dede7a70c17db060a576b8d7d4d414ca61a3b8c332877dfa1ad805cca7d9d  crates/execution-adapter/src/rust_gateway.rs   (phases/argv/env/tmpfs/fingerprint/2 methods)
cdf78b25b93a02ca472aaf91daca083d8ed06a664890a4d1afdd7ad5ccd81983  crates/execution-adapter/src/rust_applied.rs   (verify_mutation_test + extra-tmpfs clause)
3cf352213036f484bf1e4baadc8a80ef8948f3447fd7496941bf9635094f6249  crates/execution-adapter/src/project_inspection.rs (port impl — see Decisions)
835ab1b68bdd8bc49701a461a550021272b4375b78a26f5e4799ebc6be3f9566  crates/mcp-server/src/stdio.rs                 (registration)
bacda4490364b6d500f9cc820f466889d0d40db5b96aa375798221632a414e75  crates/mcp-server/tests/protocol.rs            (snapshot row + 1 test)
e5bc11e06124bed086ee1a959301804d69219cff650a0f1a32f6532d1116948b  docs/tools.md
0af22e4296af7147aceaea05152bec69bff51894dc629cd85aac59c22a162cbb  docs/validation/M3-matrix.md
d9a4b40540b80f0e49327a9662ec004d61f83217bdd5ee16c82a07c45dc6ea6b  docs/implementation-status.md
```
`mutation_gateway.rs` was **not** edited at all (only read from); `fixtures/`, `scripts/gate.py`, `quality_artifact*`, `nextest*`, `coverage*`, `semver*` untouched.

## Tests executed
| Command | Exit | Result |
| --- | --- | --- |
| `cargo fmt --check` | 0 | clean |
| `cargo check --workspace --all-targets --locked --offline` | 0 | clean |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | 0 | clean |
| `cargo test --workspace --locked --offline` | 0 | 82 binaries, 0 failed; 44 new mutation tests pass, 10 new ignored |
| `python3 -B scripts/check-architecture.py` | 0 | PASS |

Pre-existing tool snapshots 1–20 remain byte-identical (`protocol.rs` bootstrap compares all of them and passes); mine is new at index 21.

## Evidence (oracle → test)
- baseline mandatory / failing baseline is a `failed` outcome → `stdio::…::a_failing_baseline_is_a_failed_outcome_with_baseline_evidence`, `mutation_outcomes::…::a_failing_baseline_is_reported_as_such_and_never_as_a_clean_report`, `Phase` argv contains `--baseline run` (`…::the_run_phase_argv_is_closed_and_carries_the_mandatory_baseline`)
- `missed >= 1` → failed, names the function → `stdio::…::a_missed_mutant_fails_and_names_the_surviving_function`
- timeout/unviable/incomplete never clean → `stdio::…::timeout_unviable_missing_baseline_and_partial_evidence_never_pass`, `mutation_test::tests::counts_expose_denominators_…`, `a_clean_report_requires_every_containment_and_completeness_clause`
- outcomes only from `mutants.out`; forged text rejected → `mutation_outcomes::…::hostile_and_malformed_reports_are_rejected_rather_than_guessed` (truncated, 64-deep, oversized string, unknown/lowercase summary, raw `caught …` lines), `mutation_test_port::…::list_totals_must_match_the_parsed_classes`
- denominators explicit → `Counts{generated,tested,viable,…}` asserted in `only_a_complete_all_caught_report_passes`
- private copy never exported / `/source` read-only → `…::the_private_copy_never_leaves_and_the_source_is_never_writable`
- `lock.json` identity asserted and redacted, never published → `…::lock_identity_accepts_only_guest_values_and_redacts_host_shapes`, `--exclude=./lock.json` in the bundle argv
- bundle profile (no links/devices/`..`/foreign owner) → `…::report_bundles_accept_only_a_rooted_regular_tree_owned_by_the_guest_user`, `…::a_symlinked_member_is_rejected_without_its_target_being_read`
- cap enforced before building → `an_oversized_generated_set_is_refused_before_anything_is_built`
- `TASKS_REQUIRED` gate → `no_mutation_selection_is_synchronously_qualified`, `mutation_test_contract_and_task_gate_are_stable_in_all_wire_versions` (all five wire versions)

## Decisions (deviations, all documented in `docs/validation/M3-05-mutation-calibration.md`)
1. **`--baseline run`, not `--baseline auto`.** Pinned 27.1.0 accepts only `run`/`skip`; `auto` would be a usage error. `run` is the non-skippable baseline the requirement means.
2. **The mutated copy uses the M2 staging tmpfs *profile* as a container-scoped mount (`/mutants-scratch`, `TMPDIR`), not a named volume.** A named volume outlives its container and is exporter-reachable; a container tmpfs cannot be exported even in principle. The M2 named-tmpfs-volume mechanism *is* used for the report volume, which must survive the run container.
3. **`max_mutants` enforced by a listing pre-pass** (`mutants --list --json`, builds/runs nothing) — cargo-mutants has no such flag and sharding is excluded. It also supplies a `generated` denominator produced without running project code.
4. **`JobKind::MutationTest` was not added.** `JobResult` cannot be extended without editing `stdio/tasks.rs` (outside my ownership), so an admissible kind whose result can never be published would be fail-open. Tasks are off; the synchronous path is fully wired.
5. **`project_inspection.rs` was patched** (one trait impl) although not on the allowed list: `with_gateway`/`quarantined` are module-private, so the port could not be wired from my own file. 20 additive lines mirroring the nextest/coverage impls.
6. **No new syscall.** The ADR-064 quality profile is reused unchanged for both mutation phases.
7. Stage-0 drops an over-ceiling ArchiveBundle whole rather than truncating it (a truncated tar is not an archive) and records `bundle_unavailable`.

## Risks
- **Uncalibrated**: exit codes, `outcomes.json`/list-file field names, `lock.json` fields, whether cargo-mutants honours `TMPDIR`, tmpfs sizing under `--memory=1g`, and per-fixture mutant counts. All fail closed; §"Uncalibrated hypotheses" in the calibration doc lists each with the exact string the parser expects.
- **ADR-061 member accounting**: a 100-mutant report exceeds `QUALITY_MAX_JOB_MEMBERS` (128) as one bundle. `bundle_entries` is reported so Stage-1 wiring can account for it; needs an ADR-061 decision.
- **Residual trust boundary**: cargo-mutants and the project's tests share one container/uid, so hostile *test code* can write into `/mutants`. Containment (no host effect, no network, no surviving child, bounded output, no mutated-source egress) is guaranteed; "mutation results attest against a hostile author" is not, and is stated explicitly.
- Concurrency: two other workers edited `rust_gateway.rs`/`rust_applied.rs`/`project_inspection.rs`/`stdio.rs`/`protocol.rs` during the window; I re-read before each edit and the final full-workspace run is green.

## Open issues
- **Docker tests to run** (`crates/execution-adapter/tests/mutation_runtime.rs`, all `#[ignore]`): `caught_all_fixture_reports_every_viable_mutant_as_caught`, `missed_one_fixture_names_the_surviving_function`, `timeout_loop_fixture_produces_a_bounded_timeout_class`, `unviable_fixture_produces_an_unviable_class_that_never_credits_clean`, `baseline_failing_fixture_reports_the_baseline_and_no_mutation_verdict`, `hostile_writer_fixture_is_contained_and_its_forged_output_is_not_trusted`, `host_source_and_canary_are_unchanged_after_every_mutation_run`, `max_mutants_cap_is_enforced_before_anything_is_built`, `cancellation_with_an_active_child_joins_cleanup_and_leaves_no_objects`, `the_report_bundle_is_bounded_and_never_extracted_to_a_host_path`.
- **The tool is unreachable over MCP until M3-02.** No mutation selection fits the 60 s synchronous budget (derived budget floors at 300 s), so `auto` always returns `TASKS_REQUIRED` and `task`/`synchronous` are rejected. This is required by the contract but means the full path is qualified only by unit tests plus the Docker gateway tests until Tasks land — the single largest acceptance gap.
- **Durable ADR-061 Stage-1 publication is not wired** (needs `stdio/quality_artifacts.rs`, not mine). Stage 0 is wired and tested; the `MutationTestDurablePublisher` trait is defined and ready.
- `docs/validation/M3-05-fixtures.md` asks whether a seventh fixture workspace is required; the six named ones are sufficient for every test above.