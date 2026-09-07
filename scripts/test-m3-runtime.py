#!/usr/bin/env python3
"""Explicit M3 quality-runtime qualification; no provisioning, pulls or downloads."""
import datetime
import hashlib
import json
import os
import pathlib
import platform
import signal
import subprocess
import sys
import time

ROOT = pathlib.Path(__file__).resolve().parents[1]
OUTPUT = pathlib.Path(
    os.environ.get("RUST_MCP_M3_RUNTIME_OUTPUT", ROOT / "target/m3-runtime")
)
IMAGE_CONFIG = ROOT / "docs/validation/M3-image-config.json"
# A stalled selection must become a recorded failure, never an unattended gate
# hang. The longest legitimate step observed so far is 124 s; this bound is
# deliberately far above it and is not a substitute for the in-gateway budgets.
STEP_TIMEOUT_S = int(os.environ.get("RUST_MCP_M3_STEP_TIMEOUT_S", "900"))


def utc_now():
    return datetime.datetime.now(datetime.UTC).isoformat().replace("+00:00", "Z")


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def owned_docker_state(socket_path):
    """Bounded, read-only residue evidence for a timed-out selection."""
    docker = "/Applications/Docker.app/Contents/Resources/bin/docker"
    host = f"unix://{socket_path}"
    commands = {
        "containers": [docker, "--host", host, "ps", "-a", "--filter",
                       "label=org.rust-mcp.execution=true", "--format", "{{json .}}"],
        "volumes": [docker, "--host", host, "volume", "ls", "--filter",
                    "label=org.rust-mcp.execution=true", "--format", "{{json .}}"],
    }
    snapshot = {"captured_at": utc_now()}
    for kind, command in commands.items():
        try:
            output = subprocess.check_output(command, cwd=ROOT, text=True,
                                             stderr=subprocess.STDOUT, timeout=10)
            snapshot[kind] = [line for line in output.splitlines() if line]
        except (OSError, subprocess.SubprocessError) as error:
            snapshot[f"{kind}_error"] = type(error).__name__
    return snapshot


def main():
    if sys.platform != "darwin" or platform.machine() != "arm64":
        raise RuntimeError("M3-01 runtime is qualified only on macOS ARM64/Docker Linux ARM64")
    if not os.environ.get("RUST_MCP_TEST_SOCKET"):
        raise RuntimeError("RUST_MCP_TEST_SOCKET required; no socket discovery or substitution")
    if STEP_TIMEOUT_S <= 0:
        raise RuntimeError("RUST_MCP_M3_STEP_TIMEOUT_S must be a positive number of seconds")
    image = json.loads(IMAGE_CONFIG.read_text())["image"]
    allowed = {"HOME", "PATH", "TMPDIR", "CARGO_HOME", "RUSTUP_HOME", "SDKROOT",
               "DEVELOPER_DIR", "CARGO_TARGET_DIR", "RUST_MCP_TEST_SOCKET"}
    env = {key: value for key, value in os.environ.items() if key in allowed}
    env.update(CARGO_INCREMENTAL="0", CARGO_TERM_COLOR="never", RUST_MCP_TEST_IMAGE=image)
    cargo = pathlib.Path(subprocess.check_output(
        ["rustup", "which", "--toolchain", "1.98.1", "cargo"], env=env, text=True).strip())
    env["PATH"] = str(cargo.parent) + os.pathsep + env.get("PATH", "")
    env["RUSTC"] = str(cargo.with_name("rustc"))
    tests = [
        ("rust-engineering-execution", ["--test", "nextest_runtime"],
         "all_passing_tests_report_the_hypothesized_success_exit_code"),
        ("rust-engineering-execution", ["--test", "nextest_runtime"],
         "build_error_reports_the_observed_runner_failure_exit_code"),
        ("rust-engineering-execution", ["--test", "nextest_runtime"],
         "quality_profile_allows_only_the_required_anonymous_unix_stream_pair"),
        ("rust-engineering-execution", ["--test", "nextest_runtime"],
         "slow_test_timeout_is_a_test_failure_with_junit_evidence"),
        ("rust-engineering-execution", ["--test", "nextest_runtime"],
         "passing_fixture_records_five_cold_and_five_warm_sync_samples"),
        ("rust-engineering-execution", ["--test", "nextest_runtime"],
         "passing_and_failing_tests_produce_exact_junit_and_hypothesized_exit_codes"),
        ("rust-engineering-execution", ["--test", "nextest_runtime"],
         "ignored_fixture_reports_skipped_tests_when_report_skipped_is_all"),
        ("rust-engineering-execution", ["--test", "nextest_runtime"],
         "flaky_fixture_is_classified_flaky_when_retries_are_requested"),
        ("rust-engineering-execution", ["--test", "nextest_runtime"],
         "leaky_fixture_is_observed_and_never_hangs_the_gateway"),
        ("rust-engineering-execution", ["--test", "nextest_runtime"],
         "doc_only_fixture_never_runs_doctests"),
        ("rust-engineering-execution", ["--test", "nextest_runtime"],
         "no_tests_fixture_still_exits_with_evidence"),
        ("rust-engineering-execution", ["--test", "nextest_runtime"],
         "hostile_output_flood_is_bounded_and_reported_as_output_limit"),
        ("rust-engineering-execution", ["--test", "nextest_runtime"],
         "hostile_symlink_at_the_fixed_junit_path_is_rejected_and_never_followed"),
        ("rust-engineering-execution", ["--test", "nextest_runtime"],
         "active_cancellation_with_an_observed_child_terminates_and_joins_cleanly"),
        ("rust-engineering-execution", ["--test", "nextest_runtime"],
         "source_is_immutable_across_the_run_and_a_second_job_reuses_the_gateway_cleanly"),
        ("rust-engineering-mcp", ["--test", "inspection_runtime"],
         "nextest_runtime::synchronous_passing_failing_ignored_doc_only_and_no_tests_are_observable"),
        ("rust-engineering-mcp", ["--test", "inspection_runtime"],
         "nextest_runtime::synchronous_flaky_and_leaky_are_derived_from_junit"),
        ("rust-engineering-mcp", ["--test", "inspection_runtime"],
         "nextest_runtime::hostile_output_is_bounded_forged_markers_are_ignored_and_source_is_immutable"),
        ("rust-engineering-mcp", ["--test", "inspection_runtime"],
         "nextest_runtime::slow_timeout_cancellation_and_eof_observe_active_children_and_join_cleanup"),
        ("rust-engineering-mcp", ["--features", "test-hooks", "--test", "inspection_runtime"],
         "tasks_runtime::tasks_revocation_during_active_child_masks_cancels_and_prevents_publication"),
        ("rust-engineering-mcp", ["--features", "test-hooks", "--test", "inspection_runtime"],
         "tasks_runtime::tasks_cancel_before_start_during_execution_publication_and_cleanup_waits_for_join"),
        ("rust-engineering-mcp", ["--features", "test-hooks", "--test", "inspection_runtime"],
         "tasks_runtime::tasks_eof_joins_hostile_child_and_uncertain_cleanup_fails_session"),
        ("rust-engineering-mcp", ["--features", "test-hooks", "--test", "inspection_runtime"],
         "tasks_runtime::tasks_restart_masks_old_ids_reconciles_objects_and_admits_fresh_work"),
        ("rust-engineering-mcp", ["--features", "test-hooks", "--test", "inspection_runtime"],
         "tasks_runtime::tasks_coverage_materializes_a_create_task_result_for_a_declaring_peer"),
        ("rust-engineering-mcp", ["--features", "test-hooks", "--test", "inspection_runtime"],
         "tasks_runtime::tasks_semver_materializes_a_create_task_result_bound_to_the_candidate"),
        ("rust-engineering-mcp", ["--features", "test-hooks", "--test", "inspection_runtime"],
         "tasks_runtime::tasks_mutation_materializes_a_create_task_result_on_its_only_reachable_path"),
        ("rust-engineering-execution", ["--test", "coverage_runtime"],
         "known_counts_fixture_has_exact_line_region_and_function_oracle"),
        ("rust-engineering-execution", ["--test", "coverage_runtime"],
         "shared_file_workspace_deduplicates_aggregate_only"),
        ("rust-engineering-execution", ["--test", "coverage_runtime"],
         "zero_denominator_is_absent_from_percent_metrics"),
        ("rust-engineering-execution", ["--test", "coverage_runtime"],
         "three_report_formats_derive_from_one_capture"),
        ("rust-engineering-execution", ["--test", "coverage_runtime"],
         "no_tests_is_not_promoted_to_pass"),
        ("rust-engineering-execution", ["--test", "coverage_runtime"],
         "timeout_mid_build_is_blocked_after_joined_cleanup"),
        ("rust-engineering-execution", ["--test", "coverage_runtime"],
         "cancel_or_eof_joins_active_child_before_capacity_reuse"),
        ("rust-engineering-execution", ["--test", "coverage_runtime"],
         "hostile_html_is_retained_only_as_opaque_archive_bundle"),
        ("rust-engineering-execution", ["--test", "semver_runtime"],
         "identical_libraries_exit_zero"),
        ("rust-engineering-execution", ["--test", "semver_runtime"],
         "removed_public_function_is_a_deny_level_break"),
        ("rust-engineering-execution", ["--test", "semver_runtime"],
         "trait_method_with_default_is_compatible"),
        ("rust-engineering-execution", ["--test", "semver_runtime"],
         "trait_method_without_default_is_a_break"),
        ("rust-engineering-execution", ["--test", "semver_runtime"],
         "non_exhaustive_enum_variant_addition_is_compatible"),
        ("rust-engineering-execution", ["--test", "semver_runtime"],
         "exhaustive_enum_variant_addition_is_a_break"),
        ("rust-engineering-execution", ["--test", "semver_runtime"],
         "feature_gated_removal_uses_the_identical_selection_on_both_sides"),
        ("rust-engineering-execution", ["--test", "semver_runtime"],
         "feature_gated_removal_without_feature_has_no_signal"),
        ("rust-engineering-execution", ["--test", "semver_runtime"],
         "no_lib_gateway_exit_is_calibrated_before_application_maps_unavailable"),
        ("rust-engineering-execution", ["--test", "semver_runtime"],
         "broken_baseline_is_incomplete_not_a_compatibility_pass"),
        ("rust-engineering-execution", ["--test", "semver_runtime"],
         "warn_level_only_findings_are_surfaced_under_exit_zero"),
        ("rust-engineering-execution", ["--test", "semver_runtime"],
         "exit_100_with_zero_parsed_findings_is_blocked"),
        ("rust-engineering-execution", ["--test", "semver_runtime"],
         "planted_git_directory_is_never_discovered"),
        ("rust-engineering-execution", ["--test", "semver_runtime"],
         "registry_dependent_baseline_fails_recognizably_without_hanging"),
        ("rust-engineering-execution", ["--test", "semver_runtime"],
         "cancellation_or_eof_with_active_child_is_joined_before_return"),
        ("rust-engineering-execution", ["--test", "semver_runtime"],
         "baseline_and_candidate_roots_are_immutable_after_run"),
        ("rust-engineering-mcp", ["--test", "inspection_runtime"],
         "semver_runtime::mcp_semver_projects_findings_and_reads_bounded_raw_resource"),
        ("rust-engineering-mcp", ["--test", "inspection_runtime"],
         "semver_runtime::mcp_semver_busy_quality_store_uses_stage0_raw_resource"),
        ("rust-engineering-execution", ["--test", "mutation_runtime"],
         "caught_all_fixture_reports_every_viable_mutant_as_caught"),
        ("rust-engineering-execution", ["--test", "mutation_runtime"],
         "missed_one_fixture_names_the_surviving_function"),
        ("rust-engineering-execution", ["--test", "mutation_runtime"],
         "timeout_loop_fixture_produces_a_bounded_timeout_class"),
        ("rust-engineering-execution", ["--test", "mutation_runtime"],
         "unviable_fixture_produces_an_unviable_class_that_never_credits_clean"),
        ("rust-engineering-execution", ["--test", "mutation_runtime"],
         "baseline_failing_fixture_reports_the_baseline_and_no_mutation_verdict"),
        ("rust-engineering-execution", ["--test", "mutation_runtime"],
         "hostile_writer_fixture_is_contained_and_its_forged_output_is_not_trusted"),
        ("rust-engineering-execution", ["--test", "mutation_runtime"],
         "host_source_and_canary_are_unchanged_after_every_mutation_run"),
        ("rust-engineering-execution", ["--test", "mutation_runtime"],
         "max_mutants_cap_is_enforced_before_anything_is_built"),
        ("rust-engineering-execution", ["--test", "mutation_runtime"],
         "cancellation_with_an_active_child_joins_cleanup_and_leaves_no_objects"),
        ("rust-engineering-execution", ["--test", "mutation_runtime"],
         "the_report_bundle_is_bounded_and_never_extracted_to_a_host_path"),
    ]
    def cut_order(item):
        target = item[1][-1]
        selection = item[2]
        if (target == "nextest_runtime" or selection.startswith("nextest_runtime::")
                or selection.startswith("tasks_runtime::")):
            return 0
        if target == "semver_runtime" or selection.startswith("semver_runtime::"):
            return 1
        if target == "mutation_runtime" or selection.startswith("mutation_runtime::"):
            return 2
        return 3

    # Preserve each cut's declared order while allowing independent cuts to
    # finish before a later blocked cut stops the fail-fast qualification.
    tests.sort(key=cut_order)
    OUTPUT.mkdir(parents=True, exist_ok=True)
    receipt = {
        "schema": "rust-mcp-m3-runtime-v1",
        "status": "running",
        "started_at": utc_now(),
        "image_id": image,
        "step_timeout_s": STEP_TIMEOUT_S,
        "steps": [],
        "sources": [],
        "configuration_inputs": [],
    }
    for path in sorted((ROOT / "crates").rglob("*.rs")):
        receipt["sources"].append({"path": str(path.relative_to(ROOT)), "sha256": sha256(path)})
    configuration_paths = [
        ROOT / "Cargo.toml",
        ROOT / "Cargo.lock",
        ROOT / "rust-toolchain.toml",
        ROOT / "scripts/test-m3-runtime.py",
        IMAGE_CONFIG,
        *sorted((ROOT / "crates/execution-adapter/src").glob("seccomp*.json")),
        *sorted(path for path in (ROOT / "fixtures/nextest").rglob("*") if path.is_file()),
        *sorted(path for path in (ROOT / "fixtures/coverage").rglob("*") if path.is_file()),
        *sorted(path for path in (ROOT / "fixtures/semver").rglob("*") if path.is_file()),
        *sorted(path for path in (ROOT / "fixtures/mutation").rglob("*") if path.is_file()),
    ]
    for path in configuration_paths:
        receipt["configuration_inputs"].append(
            {"path": str(path.relative_to(ROOT)), "sha256": sha256(path)}
        )

    def save():
        (OUTPUT / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")

    try:
        for number, (package, target, selection) in enumerate(tests):
            command = [str(cargo), "test", "--locked", "--offline", "-p", package,
                       *target, selection, "--", "--exact", "--ignored", "--nocapture",
                       "--test-threads=1"]
            log = OUTPUT / f"{number}.log"
            print(f"M3 RUNTIME {selection}", flush=True)
            started = time.monotonic()
            timed_out = False
            # Its own session, so a stalled cargo/docker client tree is killed
            # whole instead of leaving orphans behind the recorded failure.
            with log.open("wb") as stream:
                process = subprocess.Popen(command, cwd=ROOT, env=env, stdout=stream,
                                           stderr=subprocess.STDOUT, start_new_session=True)
                try:
                    returncode = process.wait(timeout=STEP_TIMEOUT_S)
                except subprocess.TimeoutExpired:
                    timed_out = True
                    docker_before_kill = owned_docker_state(env["RUST_MCP_TEST_SOCKET"])
                    os.killpg(process.pid, signal.SIGKILL)
                    returncode = process.wait()
                    docker_after_kill = owned_docker_state(env["RUST_MCP_TEST_SOCKET"])
            output = log.read_text(errors="replace")
            passed = (not timed_out and returncode == 0
                      and "test result: ok. 1 passed; 0 failed; 0 ignored;" in output)
            step = {
                "selection": selection,
                "command": command,
                "status": "passed" if passed else "failed",
                "exit_code": returncode,
                "timed_out": timed_out,
                "expected_executed": 1,
                "seconds": round(time.monotonic() - started, 3),
                "log_sha256": sha256(log),
            }
            if timed_out:
                step["owned_docker_before_kill"] = docker_before_kill
                step["owned_docker_after_kill"] = docker_after_kill
            receipt["steps"].append(step)
            save()
            if timed_out:
                raise RuntimeError(
                    f"M3 test exceeded the {STEP_TIMEOUT_S}s step bound and was killed: {log}")
            if not passed:
                raise RuntimeError(f"M3 test failed or exactly one case did not execute: {log}")
        receipt["status"] = "passed"
    except BaseException as error:
        receipt.update(status="failed", error=str(error))
        raise
    finally:
        receipt["finished_at"] = utc_now()
        save()
    print(f"PASS M3 runtime: {OUTPUT / 'receipt.json'}", flush=True)


if __name__ == "__main__":
    if not __debug__:
        raise RuntimeError("Optimized Python mode is rejected")
    main()
