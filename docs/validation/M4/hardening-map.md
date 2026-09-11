# M4-06 hardening coverage map

Status: all 19 native selections passed on final sources, 2026-09-08.
See [runtime receipt](runtime.json), [core](core-gate.json) and
[full 33/33 resumed on unchanged sources](full-gate.json). Independent final
[evidence confirmation](../../reviews/m4-final-evidence/review.md) accepts local
closure; the [handoff](handoff.md) records the Technical Owner decision.

## Scope and source of requirements

The controlling requirements are the threat model and minimum hardening list in
`docs/roadmap/m4-security.md`: compromised dependencies/plugins and hostile
`build.rs`/proc macros; catalog or model poisoning; source confusion; broad or
expired suppressions; hostile parsers; secrets in artifacts; containment escape
and quota exhaustion; secret canaries in logs, diagnostics, HTML, and diffs;
catalog signature/hash/sequence/source mismatch; altered plugin digest; direct
socket/cloud-metadata/namespace/mount attempts; orphan/fork/disk/output bombs;
documented native allocator, ACL/privileged-host, and secret-scanning limits; and
tool/source revocation that blocks new admission while preserving audit.

The following map names tests precisely so a final receipt can prove that one
case, rather than merely a similarly named module, executed.

## Explicit M4 runtime harness

`scripts/test-m4-runtime.py` currently selects 19 ignored tests with
`--exact --ignored --nocapture --test-threads=1` and requires the libtest line
for exactly one passing test. All 19 exact selections resolve in their selected
package and target; no misspelled or stale filter was found.

The harness reads its advertised image from
`docs/validation/M4/runtime-image.json` and exports it as
`RUST_MCP_TEST_IMAGE`:

- derived M4 runtime: `sha256:25ed3626e710081a571a86a29521eaf2e890e796afd422ba5e409e0ce1891635`;
- base security runtime: `sha256:95dddeb5305f10b09b441e3cc4018ebb1a8a296d365c65106327d59f933c64e7`;
- M3 Rust runtime: `sha256:384a1742ecc53cdd3a9c0bf36c6f8b66db73ddd118aeeae6e55654ea998ae36a`.

The effective image for each selected case is:

| Exact selection | Effective image | Reason |
| --- | --- | --- |
| `host_canary_cannot_enter_html_diffs_logs_or_diagnostics` | derived M4 `25ed…` | Reads `RUST_MCP_TEST_IMAGE` from the harness. |
| `security_runtime::m4_project_output_canaries_are_absent_from_security_results_and_resources` | derived M4 `25ed…` | Passes `APPROVED_M4_IMAGE` explicitly. |
| `security_runtime::m4_miri_tasks_cancel_eof_and_revocation_join_before_releasing_authority` | derived M4 `25ed…` | Passes `APPROVED_M4_IMAGE` explicitly. |
| `rust_calibration::tests::resource_limits_are_actually_enforced` | derived M4 `25ed…` | Reads `RUST_MCP_TEST_IMAGE`. |
| `security_native::m4_scanner_runtime_base_containment_is_requalified` | derived M4 `25ed…` | Hard-codes the derived digest. |
| `security_native::m4_deny_native_text_licenses_and_bans_are_real_and_cleanup_is_joined` | derived M4 `25ed…` | The qualification path now selects the final derived digest. |
| `security_graph_native::captures_workspace_dependency_graphs_through_the_gateway` | derived M4 `25ed…` | The qualification path now selects the final derived digest. |
| `security_native_adversarial::m4_deny_adversarial_oracles_preserve_cleanup_and_inputs` | derived M4 `25ed…` | The qualification path now selects the final derived digest. |
| `unsafe_native::m4_scanner_native_oracles_preserve_partial_results_inputs_and_cleanup` | derived M4 `25ed…` | Module constant hard-codes the derived digest. |
| `miri_native::m4_miri_gateway_classifies_native_oracles` | derived M4 `25ed…` | Hard-codes the derived digest. |
| `miri_native::m4_miri_rejects_native_producers_and_joins_timeout_and_cancel` | derived M4 `25ed…` | Hard-codes the derived digest. |
| `security_runtime::deny_native_mcp_tasks_policy_licenses_and_owner_bound_redacted_resource` | derived M4 `25ed…` | The M4 run starts on the final digest; its rollback phase deliberately returns to M3. |
| `security_runtime::m4_tools_native_mcp_observations_composition_and_private_resources` | derived M4 `25ed…` | Passes the derived digest explicitly. |
| `rust_calibration::tests::observed_descendants_are_cleaned_on_timeout_cancel_and_overflow` | derived M4 `25ed…` | Reads `RUST_MCP_TEST_IMAGE`; `calibrate` executes timeout, cancel, and overflow descendant scenarios. |
| `rust_calibration::tests::actual_clippy_build_script_and_proc_macro_containment` | derived M4 `25ed…` | Reads `RUST_MCP_TEST_IMAGE`. |
| `rust_gateway::test_runtime::actual_test_runtime_containment_and_descendant_cleanup` | derived M4 `25ed…` | Reads `RUST_MCP_TEST_IMAGE`. |
| `quality_profile_allows_only_the_required_anonymous_unix_stream_pair` | derived M4 `25ed…` | The nextest integration gateway reads `RUST_MCP_TEST_IMAGE`. |
| `hostile_html_is_retained_only_as_opaque_archive_bundle` | derived M4 `25ed…` | Coverage accepts M3 or derived M4 and reads the exported value. |
| `host_source_and_canary_are_unchanged_after_every_mutation_run` | derived M4 `25ed…` | Mutation integration gateway reads `RUST_MCP_TEST_IMAGE`. |

The final passed run contains 19 selections on `25ed…` and zero selections on the
base security or M3 images. The deny MCP rollback phase deliberately restarts on
M3 after its M4 assertions; that phase is part of one selection, not a separate
qualification target. The harness records `primary_image_id`,
`base_security_image_id` and the effective image per step. Several native
fixtures use this host's explicit Docker socket and do not establish portability.

The ignored test `security_native::m4_runtime_base_containment_is_requalified`
is not among the 19 selections and targets `95dd…`. Its similar name must not be
confused with the selected scanner calibration on `25ed…`.

## Threat-to-test map

| Required threat/control | Existing discriminating evidence | Image when applicable | Coverage and limitation |
| --- | --- | --- | --- |
| Malicious `build.rs` and proc macro | `rust_calibration::tests::actual_clippy_build_script_and_proc_macro_containment`; `rust_gateway::test_runtime::actual_test_runtime_containment_and_descendant_cleanup` invokes the shared containment checks in a real test flow. | `25ed…` in the harness | Selected. The fixture checks Linux ARM64, uid/gid 65534, zero capabilities, `no_new_privs`, seccomp, mount modes, environment and forbidden syscalls. |
| Socket and namespace/mount escape | `rust_calibration::tests::actual_clippy_build_script_and_proc_macro_containment`; `quality_profile_allows_only_the_required_anonymous_unix_stream_pair`; shared fixture `fixtures/security/rust-containment/checks.rs`. | `25ed…` | Selected. Network and filesystem socket creation are denied; anonymous Unix `SOCK_SEQPACKET` is the positive control. `unshare`, `setns`, `mount`, `ptrace`, `mknodat`, `keyctl`, `bpf`, `io_uring_setup`, and invalid namespace `clone` must return `EPERM`. |
| Orphan/fork and process-tree cleanup | `rust_calibration::tests::observed_descendants_are_cleaned_on_timeout_cancel_and_overflow`; `rust_gateway::test_runtime::actual_test_runtime_containment_and_descendant_cleanup`; `security_runtime::m4_miri_tasks_cancel_eof_and_revocation_join_before_releasing_authority`. | `25ed…` | The focused Miri control passed cancel, EOF and revocation with cleanup before EOF. The final harness passed it alongside calibration and libtest descendant cases. |
| Disk, memory and output bombs | `rust_calibration::tests::resource_limits_are_actually_enforced`; calibration timeout/overflow cases inside `observed_descendants_are_cleaned_on_timeout_cancel_and_overflow`; actual libtest timeout/cancel/overflow in `actual_test_runtime_containment_and_descendant_cleanup`. | `25ed…` | Selected. The resource fixture exercises the bounded tmpfs/rlimits and the two process tests distinguish termination causes. The dedicated nextest `hostile_output_flood_is_bounded_and_reported_as_output_limit` is stronger for nextest but is not selected. |
| Real deny/plugin behavior and source confusion | `security_native::m4_deny_native_text_licenses_and_bans_are_real_and_cleanup_is_joined`; `security_graph_native::captures_workspace_dependency_graphs_through_the_gateway`; `security_native_adversarial::m4_deny_adversarial_oracles_preserve_cleanup_and_inputs`; `security_metadata::tests::declared_license_cannot_replace_captured_text_and_original_source_is_unchanged`; `security_metadata::tests::escapes_missing_files_and_forged_graphs_are_rejected_before_deny`; `security_metadata::tests::duplicate_json_missing_lock_and_changed_license_bytes_have_distinct_oracles`. | Native deny/graph/adversarial tests select `25ed…`; metadata tests are host unit tests. | Native cases cover license text, ban/source rules, a real exact vendor snapshot, corrupt lock checksum, absent offline data, project exceptions, pre/during cancellation, timeout/output bounds, immutable input and cleanup. Final harness execution passed on current sources. |
| Hostile unsafe parser | `unsafe_native::m4_scanner_native_oracles_preserve_partial_results_inputs_and_cleanup` plus unit cases in `unsafe_scan.rs`. | `25ed…` | Selected native test distinguishes comments/strings, cfg/macro limits, workspace/vendor origin, all modeled unsafe kinds, invalid UTF-8, parse failure, 1 MiB boundary, scanner crash, global file budget, partial output, immutable inputs, and cleanup. |
| Miri classification and hostile producers | `miri_native::m4_miri_gateway_classifies_native_oracles`; `miri_native::m4_miri_rejects_native_producers_and_joins_timeout_and_cancel`. | `25ed…` | The exact `opt-level=1` warning fix passed 13 parser tests, and the focused MCP canary classified one ordinary panic with UB 0. The expanded 13 classification and 7 admission native cases passed on current bytes. |
| Catalog signature/hash/sequence/source poisoning | `bundle::tests::authenticates_real_sqlite_and_enforces_sequence`; `bundle::tests::rejects_signature_hash_identity_and_trust_errors`; `bundle::tests::rejects_signed_manifest_publisher_and_channel_mismatches`; `bundle::tests::rejects_noncanonical_signed_manifest_and_unknown_schema`; `bundle::tests::signed_catalog_tampering_and_provenance_mismatch_fail`; `stdio::catalog::supply_tests::tampered_signature_payload_hash_and_sequence_are_all_unavailable`; `project-adapter/tests/catalog_store.rs::floor_is_independent_bounded_durable_and_never_promoted_from_staging`; `floor_record_and_staging_reject_links_and_oversized_bytes`. | Host unit/integration tests; no Docker image. | Direct positive and negative controls exist for signature, payload hash, publisher/channel/source identity, sequence/floor rollback, archive shape and real SQLite authentication. These are not explicit selections in `test-m4-runtime.py`; the normal Rust gate must carry their evidence. |
| Semantic model poisoning | `catalog-adapter/tests/hybrid.rs::snapshot_schema_and_complete_model_identity_are_checked_before_inference`; `malformed_embedding_never_reaches_index`; `duplicate_unknown_excessive_or_invalid_distance_candidates_fall_back_atomically`; `successful_stub_retrieval_deduplicates_and_rehydrates_only_sqlite_facts`. | Host integration tests; no Docker image. | Negative model/schema/vector controls preserve SQLite as authority. They are outside the explicit 19-test runtime harness. |
| Broad/expired suppression and partial/stale facts | `application/tests/security.rs::suppression_requires_exact_engine_rule_package_source_and_version`; `exact_suppression_preserves_original_and_does_not_repair_missing_audit_data`; `policy_expiry_during_any_engine_stage_rejects_publication`; `stale_or_unknown_audit_cannot_pass_a_clean_deny`; `audit_or_deny_omissions_never_false_pass`; `application/tests/quality_v2.rs::partial_required_stage_never_produces_a_passed_gate`; `missing_vendor_or_policy_makes_the_deny_stage_unavailable_and_gate_incomplete`. | Host application tests; no Docker image. | Exact owner/rule/package/source/version matching, expiry at every engine stage, and incomplete/stale non-pass are discriminated. |
| Owner/source revocation and publication | `application/tests/security.rs::durable_deny_rejects_revocation_and_policy_expiry_during_publication`; `durable_supply_uses_one_capture_audit_deny_and_facts_then_revalidates_owner`; `supply_rejects_owner_revocation_and_publication_failure`; `application/tests/quality_v2.rs::publication_error_and_revocation_after_publisher_revalidation_never_return_evidence`; `security_runtime::m4_miri_tasks_cancel_eof_and_revocation_join_before_releasing_authority`; `security_runtime::deny_native_mcp_tasks_policy_licenses_and_owner_bound_redacted_resource`. | Host tests plus Miri/deny lifecycle on `25ed…`; deny then rolls back to M3. | The focused controls passed owner/source revalidation and joined lifecycle. Deny removed plugin/admission by returning to M3, required unavailable and reread the same v1 artifact through a new reference of the same owner. Final source-bound harness repetition passed. |
| Artifact secret redaction and ownership | `artifact-adapter/src/tests.rs::overlapping_nested_and_adjacent_matches_all_chunk_sizes`; `matches_cross_4096_boundary_and_keep_flags`; `binary_patterns_and_stored_hash_metadata`; `streaming_matches_independent_whole_buffer_oracle_at_all_short_cuts`; `upstream_truncation_is_preserved_and_hashes_only_stored_redacted_bytes`; `private_redact_rejects_empty_secret_before_reading_input`; `project-adapter/tests/quality_artifact_store.rs::owner_binding_separates_state_root_uid_and_granted_root`; `a_hardlinked_or_shortened_blob_is_never_served`; `a_planted_symlink_or_non_regular_object_is_quarantined_not_followed`; `owner_and_global_quotas_reject_before_the_gateway_and_evict_nothing`. | Host unit/integration tests; no Docker image. | Strong byte-level, truncation, rollback, ownership, no-follow and quota controls exist. The normal Rust gate must carry these because the runtime script does not select them. |
| M4 wire/resource canary | `security_runtime::m4_project_output_canaries_are_absent_from_security_results_and_resources`; `deny_native_mcp_tasks_policy_licenses_and_owner_bound_redacted_resource`; `m4_tools_native_mcp_observations_composition_and_private_resources`. | `25ed…` | The focused five-tool canary passed: the ordinary Miri panic produced one test failure, zero UB, no canary in responses/Resources and complete cleanup. Final source-bound harness repetition passed. |
| Opaque coverage HTML and source immutability | `host_canary_cannot_enter_html_diffs_logs_or_diagnostics`; `coverage_runtime::hostile_html_is_retained_only_as_opaque_archive_bundle`; `mutation_runtime::host_source_and_canary_are_unchanged_after_every_mutation_run`. | `25ed…` | The focused privacy receipt passed: a source-authorized canary was present in actual HTML and a real diff, while the host canary was absent from HTML/JSON/LCOV/diff/process streams; cleanup joined. Universal source-secret redaction remains explicitly unclaimed. Final source-bound harness repetition passed. |

## Final source-bound receipts

- [Runtime M4](runtime.json): 19/19 on `25ed…`, including actual cloud-metadata denial, descendant/resource limits, cleanup, canaries and withdrawal.
- [Scanner](scanner-native.json): seven cases; [Miri](miri-native.json): 13 classifications plus seven admission/lifecycle cases, with current source/configuration hashes and raw evidence.
- [Privacy](privacy-runtime.json): actual HTML/diff source-canary positive controls and absent host canary; authorized source is not universally secret-redacted.
- [Inventory](runtime-inventory.json): six binary hashes and complete Miri sysroot, no guest execution/network.
- [Tampered image](tampered-plugin.json): derived unapproved identity rejected before guest execution; owned object cleanup verified. Earlier failed attempts and the correction to `image ls --all` are preserved in [attempts](history/hardening-attempts/README.md).
- [Five-tool canary](output-canaries.json): ordinary Miri panic yields test failure, UB 0, no forged findings or canary in wire/Resources.
- [Lifecycle](miri-task-lifecycle.json): cancel/EOF/revocation join cleanup before authority release.
- [Deny MCP](deny-mcp.json): policy withdrawal, M3 restart, unavailable admission and preserved private v1 artifact under its owner's new reference.
- [Core](core-gate.json) and [full](full-gate.json) cover catalog/model poisoning, authorization and application composition. [M3 runtime](m3-runtime.json) retains 62 stronger quality cases on its own M3 image; [M2 runtime](m2-runtime.json) and [Rust security](rust-security.json) retain their recorded image identities.

Full's first 27 passing steps are retained byte-for-byte; six remaining steps ran
through the original gate runner after exact existing E5 assets were recovered.
The [recovery](e5-local-recovery.json) and [driver](full-gate-resume-driver.py)
disclose the earlier failure; no tests were skipped or source inputs changed.

## Interpretation boundary

Passing the selected native tests demonstrates the stated fixtures on their
effective images. It does not establish universal absence of UB, unsafe behavior,
secrets, supply-chain compromise, sandbox escape, or host compromise. M4-06 also
requires the normal unit/contract/integration gate, client evidence, distribution
provenance, and independent review identified by G1–G9; none is established by
this static map.
