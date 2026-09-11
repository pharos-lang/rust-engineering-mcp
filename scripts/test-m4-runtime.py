#!/usr/bin/env python3
"""Explicit M4 quality-runtime qualification; no provisioning, pulls or downloads."""
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
    os.environ.get("RUST_MCP_M4_RUNTIME_OUTPUT", ROOT / "target/m4-runtime")
)
IMAGE_CONFIG = ROOT / "docs/validation/M4/runtime-image.json"
# A stalled selection must become a recorded failure, never an unattended gate
# hang. The longest legitimate step observed so far is 124 s; this bound is
# deliberately far above it and is not a substitute for the in-gateway budgets.
STEP_TIMEOUT_S = int(os.environ.get("RUST_MCP_M4_STEP_TIMEOUT_S", "900"))


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
        raise RuntimeError("M4-01 runtime is qualified only on macOS ARM64/Docker Linux ARM64")
    if not os.environ.get("RUST_MCP_TEST_SOCKET"):
        raise RuntimeError("RUST_MCP_TEST_SOCKET required; no socket discovery or substitution")
    if STEP_TIMEOUT_S <= 0:
        raise RuntimeError("RUST_MCP_M4_STEP_TIMEOUT_S must be a positive number of seconds")
    image = json.loads(IMAGE_CONFIG.read_text())["image"]
    allowed = {"HOME", "PATH", "TMPDIR", "CARGO_HOME", "RUSTUP_HOME", "SDKROOT",
               "DEVELOPER_DIR", "CARGO_TARGET_DIR", "RUST_MCP_TEST_SOCKET"}
    env = {key: value for key, value in os.environ.items() if key in allowed}
    env.update(CARGO_INCREMENTAL="0", CARGO_TERM_COLOR="never", RUST_MCP_TEST_IMAGE=image)
    cargo = pathlib.Path(subprocess.check_output(
        ["rustup", "which", "--toolchain", "1.98.1", "cargo"], env=env, text=True).strip())
    env["PATH"] = str(cargo.parent) + os.pathsep + env.get("PATH", "")
    env["RUSTC"] = str(cargo.with_name("rustc"))
    tests = [('rust-engineering-execution', ['--test', 'm4_privacy_runtime'], 'host_canary_cannot_enter_html_diffs_logs_or_diagnostics'), ('rust-engineering-mcp', ['--test', 'inspection_runtime', '--features', 'test-hooks'], 'security_runtime::m4_project_output_canaries_are_absent_from_security_results_and_resources'), ('rust-engineering-mcp', ['--test', 'inspection_runtime', '--features', 'test-hooks'], 'security_runtime::m4_miri_tasks_cancel_eof_and_revocation_join_before_releasing_authority'), ('rust-engineering-execution', ['--lib'], 'rust_calibration::tests::resource_limits_are_actually_enforced'), ('rust-engineering-execution', ['--lib'], 'security_native::m4_scanner_runtime_base_containment_is_requalified'), ('rust-engineering-execution', ['--lib'], 'security_native::m4_deny_native_text_licenses_and_bans_are_real_and_cleanup_is_joined'), ('rust-engineering-execution', ['--lib'], 'security_graph_native::captures_workspace_dependency_graphs_through_the_gateway'), ('rust-engineering-execution', ['--lib'], 'security_native_adversarial::m4_deny_adversarial_oracles_preserve_cleanup_and_inputs'), ('rust-engineering-execution', ['--lib'], 'unsafe_native::m4_scanner_native_oracles_preserve_partial_results_inputs_and_cleanup'), ('rust-engineering-execution', ['--lib'], 'miri_native::m4_miri_gateway_classifies_native_oracles'), ('rust-engineering-execution', ['--lib'], 'miri_native::m4_miri_rejects_native_producers_and_joins_timeout_and_cancel'), ('rust-engineering-mcp', ['--test', 'inspection_runtime', '--features', 'test-hooks'], 'security_runtime::deny_native_mcp_tasks_policy_licenses_and_owner_bound_redacted_resource'), ('rust-engineering-mcp', ['--test', 'inspection_runtime', '--features', 'test-hooks'], 'security_runtime::m4_tools_native_mcp_observations_composition_and_private_resources'), ('rust-engineering-execution', ['--lib'], 'rust_calibration::tests::observed_descendants_are_cleaned_on_timeout_cancel_and_overflow'), ('rust-engineering-execution', ['--lib'], 'rust_calibration::tests::actual_clippy_build_script_and_proc_macro_containment'), ('rust-engineering-execution', ['--lib'], 'rust_gateway::test_runtime::actual_test_runtime_containment_and_descendant_cleanup'), ('rust-engineering-execution', ['--test', 'nextest_runtime'], 'quality_profile_allows_only_the_required_anonymous_unix_stream_pair'), ('rust-engineering-execution', ['--test', 'coverage_runtime'], 'hostile_html_is_retained_only_as_opaque_archive_bundle'), ('rust-engineering-execution', ['--test', 'mutation_runtime'], 'host_source_and_canary_are_unchanged_after_every_mutation_run')]
    base_security_image = "sha256:95dddeb5305f10b09b441e3cc4018ebb1a8a296d365c65106327d59f933c64e7"
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
        "schema": "rust-mcp-m4-runtime-v1",
        "status": "running",
        "started_at": utc_now(),
        "primary_image_id": image,
        "base_security_image_id": base_security_image,
        "step_timeout_s": STEP_TIMEOUT_S,
        "steps": [],
        "sources": [],
        "configuration_inputs": [],
    }
    for path in sorted((ROOT / "crates").rglob("*.rs")):
        receipt["sources"].append({"path": str(path.relative_to(ROOT)), "sha256": sha256(path)})
    configuration_paths = [
        ROOT / "Cargo.toml", ROOT / "Cargo.lock", ROOT / "rust-toolchain.toml",
        ROOT / "scripts/test-m4-runtime.py", IMAGE_CONFIG,
        ROOT / "fixtures/security/rust-containment/checks.rs",
        *sorted((ROOT / "crates/execution-adapter/src").glob("seccomp*.json")),
        *sorted(path for directory in ["m4-deny-native", "m4-deny-adversarial", "m4-scanner-native", "m4-runtime-oracles", "unsafe-scanner-helper"] for path in (ROOT/"fixtures"/directory).rglob("*") if path.is_file() and "target" not in path.parts),
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
            print(f"M4 RUNTIME {selection}", flush=True)
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
                "image_id": image,
                "rollback_image_id": "sha256:384a1742ecc53cdd3a9c0bf36c6f8b66db73ddd118aeeae6e55654ea998ae36a" if selection == "security_runtime::deny_native_mcp_tasks_policy_licenses_and_owner_bound_redacted_resource" else None,
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
                    f"M4 test exceeded the {STEP_TIMEOUT_S}s step bound and was killed: {log}")
            if not passed:
                raise RuntimeError(f"M4 test failed or exactly one case did not execute: {log}")
        receipt["status"] = "passed"
    except BaseException as error:
        receipt.update(status="failed", error=str(error))
        raise
    finally:
        receipt["finished_at"] = utc_now()
        save()
    if any(sha256(ROOT / entry["path"]) != entry["sha256"] for entry in receipt["sources"] + receipt["configuration_inputs"]):
        receipt.update(status="failed", error="source inputs changed during qualification")
        save()
        raise RuntimeError(receipt["error"])
    print(f"PASS M4 runtime: {OUTPUT / 'receipt.json'}", flush=True)


if __name__ == "__main__":
    if not __debug__:
        raise RuntimeError("Optimized Python mode is rejected")
    main()
