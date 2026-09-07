#!/usr/bin/env python3
"""Explicit M2 runtime qualification; requires the existing approved local runtime."""
import datetime
import hashlib
import json
import os
import pathlib
import platform
import subprocess
import sys
import time

ROOT = pathlib.Path(__file__).resolve().parents[1]

def main():
    if sys.platform != "darwin" or platform.machine() != "arm64":
        raise RuntimeError("M2 writer qualified only on macOS ARM64/APFS")
    if not os.environ.get("RUST_MCP_TEST_SOCKET"):
        raise RuntimeError("RUST_MCP_TEST_SOCKET required; no provisioning or substitution")
    allowed = {"HOME", "PATH", "TMPDIR", "CARGO_HOME", "RUSTUP_HOME", "SDKROOT",
               "DEVELOPER_DIR", "CARGO_TARGET_DIR", "RUST_MCP_TEST_SOCKET"}
    env = {key: value for key, value in os.environ.items() if key in allowed}
    env.update(CARGO_INCREMENTAL="0", CARGO_TERM_COLOR="never")
    cargo = pathlib.Path(subprocess.check_output(
        ["rustup", "which", "--toolchain", "1.98.1", "cargo"], env=env, text=True).strip())
    env["PATH"] = str(cargo.parent) + os.pathsep + env.get("PATH", "")
    env["RUSTC"] = str(cargo.with_name("rustc"))
    tests = [
        ("rust-engineering-execution", ["--lib"], "mutation_gateway::tests::real_rustfmt_exports_full_candidate_and_cleans_owned_objects", 1),
        ("rust-engineering-execution", ["--lib"], "resolution_gateway::tests::real_vendor_resolution_preserves_lock_presence_and_cleans_all_objects", 1),
        ("rust-engineering-execution", ["--lib"], "project_inspection::tests::production_inspector_formats_postchecks_rejects_and_cancels", 1),
        ("rust-engineering-mcp", ["--test", "inspection_runtime"], "mutation_runtime::manifest_preview_commit_conflict_reopen_and_restart_receipt", 1),
        ("rust-engineering-mcp", ["--test", "inspection_runtime"], "format_mutation_runtime::", 2),
        ("rust-engineering-mcp", ["--test", "inspection_runtime"], "fix_mutation_runtime::", 3),
        ("rust-engineering-mcp", ["--test", "inspection_runtime"], "fix_hostile_runtime::", 2),
        ("rust-engineering-mcp", ["--test", "inspection_runtime"], "mutation_concurrency_runtime::", 1),
        ("rust-engineering-mcp", ["--test", "inspection_runtime"], "dependency_mutation_runtime::", 4),
        ("rust-engineering-mcp", ["--test", "inspection_runtime"], "terminal_plan_runtime::terminal_plans_free_quota_and_replay_only_from_exact_durable_identity", 1),
    ]
    output = ROOT / "target/m2-runtime"
    output.mkdir(parents=True, exist_ok=True)
    receipt = {"schema": "rust-mcp-m2-runtime-v1", "status": "running", "steps": [],
               "started_at": datetime.datetime.now(datetime.UTC).isoformat(), "sources": []}
    for path in sorted((ROOT / "crates").rglob("*.rs")):
        receipt["sources"].append({"path": str(path.relative_to(ROOT)),
            "sha256": hashlib.sha256(path.read_bytes()).hexdigest()})
    receipt["configuration_inputs"] = [
        {"path": str(path.relative_to(ROOT)), "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
        for path in [ROOT / "Cargo.toml", ROOT / "Cargo.lock", ROOT / "rust-toolchain.toml",
                     ROOT / "scripts/test-m2-runtime.py",
                     *sorted((ROOT / "crates/execution-adapter/src").glob("seccomp*.json"))]
    ]
    def save():
        (output / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    try:
        for number, (package, target, selection, expected) in enumerate(tests):
            command = [str(cargo), "test", "--locked", "--offline", "-p", package,
                       *target, selection, "--", "--ignored", "--nocapture", "--test-threads=1"]
            log = output / f"{number}.log"
            print(f"M2 RUNTIME {selection}", flush=True)
            start = time.monotonic()
            with log.open("wb") as stream:
                result = subprocess.run(command, cwd=ROOT, env=env, stdout=stream, stderr=subprocess.STDOUT)
            text = log.read_text()
            passed = result.returncode == 0 and f"test result: ok. {expected} passed; 0 failed; 0 ignored;" in text
            receipt["steps"].append({"selection": selection, "command": command,
                "status": "passed" if passed else "failed", "exit_code": result.returncode,
                "expected_executed": expected, "seconds": round(time.monotonic() - start, 3),
                "log_sha256": hashlib.sha256(log.read_bytes()).hexdigest()})
            save()
            if not passed:
                raise RuntimeError(f"M2 test failed or required cases absent: {log}")
        receipt["status"] = "passed"
    except BaseException as error:
        receipt.update(status="failed", error=str(error))
        raise
    finally:
        receipt["finished_at"] = datetime.datetime.now(datetime.UTC).isoformat()
        save()
    print(f"PASS M2 runtime: {output / 'receipt.json'}", flush=True)

if __name__ == "__main__":
    if not __debug__:
        raise RuntimeError("Optimized Python mode is rejected")
    main()
