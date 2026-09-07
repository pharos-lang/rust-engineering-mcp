#!/usr/bin/env python3
"""Measure every qualified synchronous M3 operation and task-control latency.

Preparation (the default) validates inputs only. ``--run`` is the explicit
Docker entry point. The evidence file is created only after each nominated
operation emits exactly N cold and N warm successful samples and both live task
lifecycle probes pass.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import pathlib
import platform
import re
import subprocess
import sys
import time

ROOT = pathlib.Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "docs/validation/M3-02-budgets.json"
LOG_ROOT = ROOT / "target/m3-02-budgets"
IMAGE_CONFIG = ROOT / "docs/validation/M3-image-config.json"
OPERATIONS = [
    {
        "name": "rust.test.nextest",
        "fixture": "fixtures/nextest/passing",
        "package": "rust-engineering-execution",
        "target": "nextest_runtime",
        "test": "passing_fixture_records_five_cold_and_five_warm_sync_samples",
        "marker": "M3_NEXTEST_SYNC_TIMINGS",
    },
    {
        "name": "rust.coverage",
        "fixture": "fixtures/coverage/known-counts",
        "package": "rust-engineering-execution",
        "target": "coverage_runtime",
        "test": "known_counts_records_cold_and_warm_sync_samples",
        "marker": "M3_COVERAGE_SYNC_TIMINGS",
    },
    {
        "name": "rust.semver.check",
        "fixture": "fixtures/semver/identical",
        "package": "rust-engineering-execution",
        "target": "semver_runtime",
        "test": "identical_records_cold_and_warm_sync_samples",
        "marker": "M3_SEMVER_SYNC_TIMINGS",
    },
]
TASK_PROBES = [
    {
        "name": "cancel",
        "test": "tasks_runtime::tasks_cancel_before_start_during_execution_publication_and_cleanup_waits_for_join",
        "marker": "M3_TASK_CANCEL_RECEIPT",
    },
    {
        "name": "eof",
        "test": "tasks_runtime::tasks_eof_joins_hostile_child_and_uncertain_cleanup_fails_session",
        "marker": "M3_TASK_EOF_RECEIPT",
    },
]


def sha256(path: pathlib.Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def percentile(samples: list[int], percent: int) -> int:
    """Nearest-rank percentile, fixed for reproducible comparisons."""
    ordered = sorted(samples)
    return ordered[max(0, math.ceil(percent * len(ordered) / 100) - 1)]


def summary(samples: list[int]) -> dict[str, int]:
    return {
        "min_ms": min(samples),
        "p50_ms": percentile(samples, 50),
        "p95_ms": percentile(samples, 95),
        "p99_ms": percentile(samples, 99),
        "max_ms": max(samples),
    }


def cargo_path(environment: dict[str, str]) -> pathlib.Path:
    return pathlib.Path(
        subprocess.check_output(
            ["rustup", "which", "--toolchain", "1.98.1", "cargo"],
            cwd=ROOT,
            env=environment,
            text=True,
        ).strip()
    )


def operation_command(cargo: pathlib.Path, operation: dict[str, str]) -> list[str]:
    return [
        str(cargo), "test", "-p", operation["package"], "--test", operation["target"],
        "--locked", "--offline", operation["test"], "--", "--exact", "--ignored",
        "--test-threads=1", "--nocapture",
    ]


def task_command(cargo: pathlib.Path, probe: dict[str, str]) -> list[str]:
    return [
        str(cargo), "test", "-p", "rust-engineering-mcp", "--features", "test-hooks",
        "--test", "inspection_runtime", "--locked", "--offline", probe["test"], "--",
        "--exact", "--ignored", "--test-threads=1", "--nocapture",
    ]


def run_one(command: list[str], environment: dict[str, str], log: pathlib.Path,
            timeout: int) -> tuple[int, float, str]:
    started = time.monotonic()
    completed = subprocess.run(
        command,
        cwd=ROOT,
        env=environment,
        capture_output=True,
        text=True,
        timeout=timeout,
        check=False,
    )
    duration = round(time.monotonic() - started, 3)
    combined = completed.stdout + "\n" + completed.stderr
    log.write_text(combined, encoding="utf-8")
    return completed.returncode, duration, combined


def marker(output: str, name: str) -> dict:
    matches = re.findall(rf"{re.escape(name)} (\{{[^\n]*\}})", output)
    if len(matches) != 1:
        raise RuntimeError(f"{name} count was {len(matches)}, expected exactly one")
    return json.loads(matches[0])


def validate_samples(raw: dict, samples_each: int) -> dict:
    keys = [
        "cold_ms", "warm_ms", "cold_command_ms", "warm_command_ms",
        "cold_gateway_ms", "warm_gateway_ms",
    ]
    for key in keys:
        values = raw.get(key)
        if (
            not isinstance(values, list)
            or len(values) != samples_each
            or any(isinstance(value, bool) or not isinstance(value, int) or value < 0
                   for value in values)
        ):
            raise RuntimeError(f"invalid or incomplete {key} samples")
    if raw.get("samples_each") != samples_each:
        raise RuntimeError("measurement emitted the wrong sample count")
    measured = {}
    for temperature in ["cold", "warm"]:
        host = raw[f"{temperature}_ms"]
        gateway = raw[f"{temperature}_gateway_ms"]
        command = raw[f"{temperature}_command_ms"]
        overhead = [max(0, total - inner) for total, inner in zip(host, gateway)]
        measured[temperature] = {
            "raw_ms": host,
            "summary": summary(host),
            "phase_wall_ms": {
                "reported_terminal_command": {"raw": command, "summary": summary(command)},
                "gateway_total": {"raw": gateway, "summary": summary(gateway)},
                "host_wrapper_overhead": {"raw": overhead, "summary": summary(overhead)},
            },
        }
    return measured


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--run", action="store_true")
    parser.add_argument("--resume-current-logs", action="store_true")
    parser.add_argument("--samples", type=int, default=30)
    args = parser.parse_args()
    if not 30 <= args.samples <= 100:
        raise RuntimeError("--samples must be between 30 and 100 for ADR-060 evidence")
    if not args.run:
        print(json.dumps({
            "ready": True,
            "samples_each": args.samples,
            "operations": [operation["name"] for operation in OPERATIONS],
            "output": str(OUTPUT),
        }))
        return 0
    if sys.platform != "darwin" or platform.machine() != "arm64":
        raise RuntimeError("M3 budget measurement requires the qualified macOS ARM64 host")
    socket = os.environ.get("RUST_MCP_TEST_SOCKET")
    if not socket:
        raise RuntimeError("RUST_MCP_TEST_SOCKET is required")
    if OUTPUT.exists():
        raise RuntimeError("M3-02-budgets.json already exists; preserve prior evidence")

    image = json.loads(IMAGE_CONFIG.read_text(encoding="utf-8"))["image"]
    allowed = {
        "HOME", "PATH", "TMPDIR", "CARGO_HOME", "RUSTUP_HOME", "SDKROOT",
        "DEVELOPER_DIR", "CARGO_TARGET_DIR", "RUST_MCP_TEST_SOCKET",
    }
    environment = {key: value for key, value in os.environ.items() if key in allowed}
    cargo = cargo_path(environment)
    environment.update(
        CARGO_INCREMENTAL="0",
        CARGO_TERM_COLOR="never",
        RUSTC=str(cargo.with_name("rustc")),
        RUST_MCP_TEST_IMAGE=image,
        RUST_MCP_M3_BUDGET_SAMPLES=str(args.samples),
    )
    environment["PATH"] = str(cargo.parent) + os.pathsep + environment.get("PATH", "")
    LOG_ROOT.mkdir(parents=True, exist_ok=True)
    started = time.monotonic()
    operation_receipts = []
    commands = []
    for operation in OPERATIONS:
        command = operation_command(cargo, operation)
        commands.append(command)
        log = LOG_ROOT / f"{operation['name']}.log"
        resumed = False
        if args.resume_current_logs and log.exists():
            output = log.read_text(encoding="utf-8")
            exact_pass = "test result: ok. 1 passed; 0 failed; 0 ignored;" in output
            reported = re.findall(r"finished in ([0-9.]+)s", output)
            if exact_pass and len(reported) == 1:
                code = 0
                duration = float(reported[0])
                resumed = True
            else:
                code, duration, output = run_one(
                    command, environment, log, max(900, args.samples * 240)
                )
        else:
            code, duration, output = run_one(
                command, environment, log, max(900, args.samples * 240)
            )
        if code != 0:
            raise RuntimeError(f"{operation['name']} measurement failed: exit={code}; log={log}")
        operation_receipts.append({
            "operation": operation["name"],
            "fixture": operation["fixture"],
            "timeout_seconds": 60,
            "samples_each": args.samples,
            "measurements": validate_samples(marker(output, operation["marker"]), args.samples),
            "command": command,
            "exit_code": code,
            "duration_seconds": duration,
            "resumed_from_current_attempt_log": resumed,
            "log_sha256": sha256(log),
        })

    lifecycle = {}
    for probe in TASK_PROBES:
        command = task_command(cargo, probe)
        commands.append(command)
        log = LOG_ROOT / f"task-{probe['name']}.log"
        code, duration, output = run_one(command, environment, log, 600)
        if code != 0:
            raise RuntimeError(f"task {probe['name']} measurement failed: exit={code}; log={log}")
        lifecycle[probe["name"]] = {
            "raw": marker(output, probe["marker"]),
            "command": command,
            "exit_code": code,
            "duration_seconds": duration,
            "log_sha256": sha256(log),
        }

    receipt = {
        "schema": "rust-mcp-m3-02-budgets-v2",
        "image_id": image,
        "host": {"os": sys.platform, "architecture": platform.machine()},
        "samples_each": args.samples,
        "percentile_method": "nearest-rank",
        "operations": operation_receipts,
        "task_lifecycle": lifecycle,
        "duration_seconds": round(time.monotonic() - started, 3),
        "inputs": {
            "Cargo.lock": sha256(ROOT / "Cargo.lock"),
            "M3-image-config.json": sha256(IMAGE_CONFIG),
            "nextest_runtime.rs": sha256(ROOT / "crates/execution-adapter/tests/nextest_runtime.rs"),
            "coverage_runtime.rs": sha256(ROOT / "crates/execution-adapter/tests/coverage_runtime.rs"),
            "semver_runtime.rs": sha256(ROOT / "crates/execution-adapter/tests/semver_runtime.rs"),
            "tasks_runtime.rs": sha256(ROOT / "crates/mcp-server/tests/inspection_runtime/tasks.rs"),
            "test-m3-budgets.py": sha256(pathlib.Path(__file__)),
        },
        "commands": commands,
    }
    OUTPUT.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(json.dumps({
        "status": "passed",
        "receipt": str(OUTPUT),
        "samples_each": args.samples,
        "operations": len(operation_receipts),
    }))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
