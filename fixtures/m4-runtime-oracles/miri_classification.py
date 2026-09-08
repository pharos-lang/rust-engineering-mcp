#!/usr/bin/env python3
"""Empirical Miri/nextest classification oracle; this is not gateway qualification."""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import pathlib
import subprocess
import threading
import time
import uuid
import xml.etree.ElementTree as ET


HERE = pathlib.Path(__file__).resolve().parent
FIXTURES = HERE / "miri-classification"
RESULTS = FIXTURES / "results"
DOCKER = [
    "/Applications/Docker.app/Contents/Resources/bin/docker",
    "--host",
    "unix:///Users/cburgosro/.docker/run/docker.sock",
]
IMAGE = "sha256:95dddeb5305f10b09b441e3cc4018ebb1a8a296d365c65106327d59f933c64e7"
IMAGE_CONFIG = "sha256:7d4e58b9e29b2045c13d71542f7892ee071a6886a1b939c4cbfc3ff7ce40dc45"
NIGHTLY = "/opt/rust-nightly-2026-09-07/bin"
MIRI_SYSROOT = "/opt/miri-sysroot/2026-09-07/aarch64-unknown-linux-gnu"
TARGET = "aarch64-unknown-linux-gnu"
PROFILE = "rust-mcp-miri"
MAX_STREAM = 512 * 1024
MAX_JUNIT = 512 * 1024
CASE_TIMEOUT_SECONDS = 240
BASE_MIRIFLAGS = "--error-format=json -Zmiri-isolation-error=abort -Zmiri-backtrace=0"
RUN_MIRIFLAGS = BASE_MIRIFLAGS + " -Zmiri-mute-stdout-stderr"
CASES = (
    "benign-forged",
    "clean",
    "uaf",
    "uninit",
    "alias",
    "race",
    "ffi",
    "compile-fail",
    "empty",
    "ignored",
)
EXPECTED = {
    "benign-forged": ["test_failure"],
    "clean": ["clean"],
    "uaf": ["undefined_behavior"],
    "uninit": ["undefined_behavior"],
    "alias": ["undefined_behavior"],
    "race": ["undefined_behavior"],
    "ffi": ["unsupported_operation"],
    "compile-fail": ["compile_failure"],
    "empty": ["incomplete_no_tests"],
    "ignored": ["incomplete_skipped_only"],
}
SYSROOT_TREE_COMMAND = [
    "/bin/sh",
    "-c",
    "find "
    + MIRI_SYSROOT
    + " -type f -print0 | sort -z | xargs -0 sha256sum | sha256sum",
]


def sha256(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def docker(*args: str, check: bool = True) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(DOCKER + list(args), capture_output=True, check=check)


def inventory() -> dict[str, list[str]]:
    containers = docker("ps", "-a", "--format", "{{.ID}} {{.Names}} {{.Status}}").stdout
    volumes = docker("volume", "ls", "--format", "{{.Name}}").stdout
    return {
        "containers": sorted(containers.decode().splitlines()),
        "volumes": sorted(volumes.decode().splitlines()),
    }


def bounded_process(argv: list[str], timeout: int) -> tuple[int | None, bytes, bytes, bool, bool, bool]:
    process = subprocess.Popen(argv, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    buffers = [bytearray(), bytearray()]
    truncated = [False, False]

    def drain(index: int, pipe) -> None:
        while True:
            chunk = pipe.read(8192)
            if not chunk:
                return
            room = MAX_STREAM - len(buffers[index])
            if room > 0:
                buffers[index].extend(chunk[:room])
            if len(chunk) > room:
                truncated[index] = True

    threads = [
        threading.Thread(target=drain, args=(0, process.stdout), daemon=True),
        threading.Thread(target=drain, args=(1, process.stderr), daemon=True),
    ]
    for thread in threads:
        thread.start()
    timed_out = False
    try:
        code = process.wait(timeout=timeout)
    except subprocess.TimeoutExpired:
        timed_out = True
        process.kill()
        code = None
    for thread in threads:
        thread.join(timeout=10)
    return code, bytes(buffers[0]), bytes(buffers[1]), truncated[0], truncated[1], timed_out


def fixture_digest(root: pathlib.Path) -> str:
    accumulator = hashlib.sha256()
    for path in sorted(
        item
        for item in root.rglob("*")
        if item.is_file() and "results" not in item.relative_to(root).parts
    ):
        accumulator.update(path.relative_to(root).as_posix().encode())
        accumulator.update(b"\0")
        accumulator.update(path.read_bytes())
        accumulator.update(b"\0")
    return "sha256:" + accumulator.hexdigest()


def rustc_diagnostics(text: str) -> list[dict]:
    diagnostics = []
    for line in text.splitlines():
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            value = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(value, dict) and value.get("$message_type") == "diagnostic":
            diagnostics.append(value)
    return diagnostics


def parse_junit(data: bytes) -> dict:
    if len(data) > MAX_JUNIT or b"<!DOCTYPE" in data.upper() or b"<!ENTITY" in data.upper():
        return {"complete": False, "reason": "bounded XML policy rejected report"}
    try:
        root = ET.fromstring(data)
    except ET.ParseError as error:
        return {"complete": False, "reason": f"XML parse error: {error}"}
    if root.tag != "testsuites":
        return {"complete": False, "reason": "unexpected root"}
    tests = []
    for testcase in root.iter("testcase"):
        status = "passed"
        terminal = None
        for child_name in ("failure", "error", "skipped"):
            child = testcase.find(child_name)
            if child is not None:
                status = child_name
                terminal = {
                    "message": child.attrib.get("message"),
                    "type": child.attrib.get("type"),
                    "text": child.text or "",
                }
                break
        system_err = testcase.findtext("system-err") or ""
        tests.append(
            {
                "name": testcase.attrib.get("name"),
                "classname": testcase.attrib.get("classname"),
                "status": status,
                "terminal": terminal,
                "system_err": system_err,
                "diagnostics": rustc_diagnostics(system_err),
            }
        )
    declared = {
        key: root.attrib.get(key) for key in ("tests", "failures", "errors", "disabled")
    }
    return {"complete": True, "declared": declared, "tests": tests}


def categories(exit_code: int | None, timed_out: bool, junit: dict | None, stderr: str) -> list[str]:
    if timed_out:
        return ["timeout"]
    diagnostics = rustc_diagnostics(stderr)
    if junit and junit.get("complete"):
        for test in junit["tests"]:
            diagnostics.extend(test["diagnostics"])
    messages = [str(item.get("message", "")) for item in diagnostics if item.get("level") == "error"]
    found = []
    if any(message.startswith("Undefined Behavior:") for message in messages):
        found.append("undefined_behavior")
    if any(message.startswith("unsupported operation:") for message in messages):
        found.append("unsupported_operation")
    if any(message.startswith("post-monomorphization error:") for message in messages):
        found.append("compile_failure")
    if found:
        return found
    if junit and junit.get("complete"):
        tests = junit["tests"]
        failed = [test for test in tests if test["status"] in ("failure", "error")]
        skipped = [test for test in tests if test["status"] == "skipped"]
        passed = [test for test in tests if test["status"] == "passed"]
        if failed:
            return ["test_failure"]
        if tests and passed and not skipped and exit_code == 0:
            return ["clean"]
        if skipped and not passed:
            return ["incomplete_skipped_only"]
    if exit_code in (101, 104) and any(item.get("level") == "error" for item in diagnostics):
        return ["compile_failure"]
    if exit_code == 4:
        return ["incomplete_no_tests"]
    return ["unclassified_incomplete"]


def run_case(case: str) -> dict:
    source = FIXTURES / case
    fixture_before = fixture_digest(source)
    nonce = uuid.uuid4().hex
    name = "m4-miri-classification-" + case + "-" + nonce
    guardian = "m4-miri-classification-guardian-" + nonce
    volume = "m4-miri-classification-junit-" + nonce
    result_path = RESULTS / (case + ".json")
    junit_path = RESULTS / (case + ".junit.xml")
    docker(
        "volume",
        "create",
        "--driver=local",
        "--opt=type=tmpfs",
        "--opt=device=tmpfs",
        "--opt=o=size=33554432,uid=65534,gid=65534,mode=0700",
        "--label=rust-mcp.qualifier=miri-classification",
        "--label=rust-mcp.nonce=" + nonce,
        volume,
    )
    guardian_command = DOCKER + [
        "run",
        "-d",
        "--name=" + guardian,
        "--pull=never",
        "--network=none",
        "--read-only",
        "--cap-drop=ALL",
        "--security-opt=no-new-privileges=true",
        "--user=65534:65534",
        "--pids-limit=8",
        "--memory=64m",
        "--memory-swap=64m",
        "--cpus=0.1",
        "--mount=type=volume,source=" + volume + ",target=/junit",
        "--entrypoint=/usr/bin/tail",
        IMAGE,
        "-f",
        "/dev/null",
    ]
    guardian_start = subprocess.run(guardian_command, capture_output=True)
    if guardian_start.returncode != 0:
        docker("volume", "rm", "-f", volume, check=False)
        raise RuntimeError(guardian_start.stderr.decode("utf-8", "replace"))
    argv = DOCKER + [
        "run",
        "--name=" + name,
        "--pull=never",
        "--network=none",
        "--read-only",
        "--cap-drop=ALL",
        "--security-opt=no-new-privileges=true",
        "--user=65534:65534",
        "--pids-limit=128",
        "--memory=1g",
        "--memory-swap=1g",
        "--cpus=1",
        "--tmpfs=/work:rw,exec,nosuid,nodev,size=512m,mode=1777",
        "--tmpfs=/tmp:rw,noexec,nosuid,nodev,size=64m,mode=1777",
        "--workdir=/source",
        "--mount=type=bind,source=" + str(source) + ",target=/source,readonly",
        "--mount=type=bind,source=" + str(FIXTURES / "nextest.toml") + ",target=/qualification/nextest.toml,readonly",
        "--mount=type=volume,source=" + volume + ",target=/junit",
        "--entrypoint=/usr/bin/env",
        IMAGE,
        "-i",
        "PATH=" + NIGHTLY + ":/opt/rust/bin:/usr/bin:/bin",
        "HOME=/work",
        "TMPDIR=/tmp",
        "CARGO_HOME=/work/cargo",
        "CARGO_TARGET_DIR=/work/target",
        "CARGO_NET_OFFLINE=true",
        "CARGO_INCREMENTAL=0",
        "MIRI=/opt/rust-nightly-2026-09-07/bin/miri",
        "MIRI_SYSROOT=" + MIRI_SYSROOT,
        "MIRIFLAGS=" + BASE_MIRIFLAGS,
        "RUSTC=" + NIGHTLY + "/rustc",
        "CARGO=" + NIGHTLY + "/cargo",
        NIGHTLY + "/cargo",
        "miri",
        "nextest",
        "run",
        "--manifest-path=/source/Cargo.toml",
        "--config-file=/qualification/nextest.toml",
        "--profile=" + PROFILE,
        "--frozen",
        "--offline",
        "--color=never",
        "--no-fail-fast",
        "--build-jobs=1",
        "--test-threads=1",
        "--target=" + TARGET,
    ]
    started = time.monotonic()
    try:
        code, stdout, stderr, stdout_truncated, stderr_truncated, timed_out = bounded_process(
            argv, CASE_TIMEOUT_SECONDS
        )
        if timed_out:
            docker("rm", "-f", name, check=False)
        junit = b""
        if not timed_out:
            junit_command = DOCKER + [
                "exec",
                guardian,
                "/bin/cat",
                "/junit/" + PROFILE + "/reports/junit.xml",
            ]
            junit_code, junit_stdout, _, junit_stdout_truncated, _, _ = bounded_process(
                junit_command, 30
            )
            if junit_code == 0:
                junit = junit_stdout
            elif junit_stdout_truncated:
                junit = junit_stdout
        junit_truncated = len(junit) > MAX_JUNIT
        if junit_truncated:
            junit = junit[:MAX_JUNIT]
        parsed = parse_junit(junit) if junit else None
        stdout_text = stdout.decode("utf-8", "replace")
        stderr_text = stderr.decode("utf-8", "replace")
        classification = categories(code, timed_out, parsed, stderr_text)
        forged_present = "FORGED_MIRI_UNDEFINED_BEHAVIOR" in (
            stdout_text + stderr_text + junit.decode("utf-8", "replace")
        )
        record = {
            "schema": "rust-engineering-mcp.m4-miri-classification-case.v1",
            "case": case,
            "observed_at": dt.datetime.now(dt.timezone.utc).isoformat(),
            "image_id": IMAGE,
            "rootfs_readonly": True,
            "source_readonly": True,
            "network": "none",
            "user": "65534:65534",
            "limits": {
                "pids": 128,
                "memory_bytes": 1024 * 1024 * 1024,
                "cpus": 1,
                "work_tmpfs_bytes": 512 * 1024 * 1024,
                "tmp_tmpfs_bytes": 64 * 1024 * 1024,
                "stream_bytes_each": MAX_STREAM,
                "junit_bytes": MAX_JUNIT,
                "wall_seconds": CASE_TIMEOUT_SECONDS,
            },
            "fixture_sha256": fixture_before,
            "config_sha256": sha256((FIXTURES / "nextest.toml").read_bytes()),
            "argv": argv[len(DOCKER) :],
            "guest_env": {
                "PATH": NIGHTLY + ":/opt/rust/bin:/usr/bin:/bin",
                "HOME": "/work",
                "TMPDIR": "/tmp",
                "CARGO_HOME": "/work/cargo",
                "CARGO_TARGET_DIR": "/work/target",
                "CARGO_NET_OFFLINE": "true",
                "CARGO_INCREMENTAL": "0",
                "MIRI": NIGHTLY + "/miri",
                "MIRI_SYSROOT": MIRI_SYSROOT,
                "MIRIFLAGS_LIST": BASE_MIRIFLAGS,
                "MIRIFLAGS_RUN": RUN_MIRIFLAGS,
                "RUSTC": NIGHTLY + "/rustc",
                "CARGO": NIGHTLY + "/cargo",
            },
            "exit_code": code,
            "timed_out": timed_out,
            "elapsed_seconds": round(time.monotonic() - started, 6),
            "stdout_truncated": stdout_truncated,
            "stderr_truncated": stderr_truncated,
            "junit_truncated": junit_truncated,
            "stdout_sha256": sha256(stdout),
            "stderr_sha256": sha256(stderr),
            "junit_sha256": sha256(junit) if junit else None,
            "stdout": stdout_text,
            "stderr": stderr_text,
            "junit": parsed,
            "classification": classification,
            "forged_runtime_marker_present": forged_present,
            "cleanup_confirmed": False,
            "claim": "empirical first-party fixture oracle; not gateway qualification",
        }
    finally:
        docker("rm", "-f", name, check=False)
        docker("rm", "-f", guardian, check=False)
        docker("volume", "rm", "-f", volume, check=False)
        remaining_run = docker("ps", "-aq", "--filter=name=^/" + name + "$", check=False).stdout
        remaining_guardian = docker(
            "ps", "-aq", "--filter=name=^/" + guardian + "$", check=False
        ).stdout
        remaining_volume = docker("volume", "ls", "-q", "--filter=name=^" + volume + "$", check=False).stdout
        cleanup = not remaining_run.strip() and not remaining_guardian.strip() and not remaining_volume.strip()
    record["cleanup_confirmed"] = cleanup
    record["source_unchanged"] = fixture_digest(source) == fixture_before
    record["passed"] = (
        record["classification"] == EXPECTED[case]
        and record["cleanup_confirmed"]
        and record["source_unchanged"]
        and not record["stdout_truncated"]
        and not record["stderr_truncated"]
        and not record["junit_truncated"]
        and not record["timed_out"]
        and (case != "benign-forged" or not record["forged_runtime_marker_present"])
    )
    result_path.write_text(json.dumps(record, indent=2, sort_keys=True) + "\n")
    if junit:
        junit_path.write_bytes(junit)
    elif junit_path.exists():
        junit_path.unlink()
    return record


def probe(argv: list[str]) -> dict:
    name = "m4-miri-classification-probe-" + uuid.uuid4().hex
    command = DOCKER + [
        "run",
        "--name=" + name,
        "--pull=never",
        "--network=none",
        "--read-only",
        "--cap-drop=ALL",
        "--security-opt=no-new-privileges=true",
        "--user=65534:65534",
        "--pids-limit=128",
        "--memory=1g",
        "--memory-swap=1g",
        "--cpus=1",
        "--tmpfs=/work:rw,exec,nosuid,nodev,size=64m,mode=1777",
        "--tmpfs=/tmp:rw,noexec,nosuid,nodev,size=16m,mode=1777",
        "--entrypoint=/usr/bin/env",
        IMAGE,
        "-i",
        "PATH=" + NIGHTLY + ":/opt/rust/bin:/usr/bin:/bin",
        "HOME=/work",
    ] + argv
    try:
        result = subprocess.run(command, capture_output=True, timeout=60)
        return {
            "argv": argv,
            "exit_code": result.returncode,
            "stdout": result.stdout.decode("utf-8", "replace"),
            "stderr": result.stderr.decode("utf-8", "replace"),
        }
    finally:
        docker("rm", "-f", name, check=False)
        assert not docker("ps", "-aq", "--filter=name=^/" + name + "$", check=False).stdout.strip()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--case", choices=CASES, action="append")
    args = parser.parse_args()
    selected = tuple(args.case) if args.case else CASES
    RESULTS.mkdir(exist_ok=True)
    before = inventory()
    image = json.loads(docker("image", "inspect", IMAGE).stdout)[0]
    config_digest = sha256(
        json.dumps(image["Config"], sort_keys=True, separators=(",", ":")).encode()
    )
    if image["Id"] != IMAGE or image["Os"] != "linux" or image["Architecture"] != "arm64":
        raise SystemExit("unexpected image identity")
    if config_digest != IMAGE_CONFIG:
        raise SystemExit("unexpected image config")
    probes = {
        "nextest_version": probe(["/opt/rust/bin/cargo-nextest", "nextest", "--version"]),
        "rustc_version": probe([NIGHTLY + "/rustc", "-vV"]),
        "binary_hashes": probe(
            [
                "/usr/bin/sha256sum",
                "/opt/rust/bin/cargo-nextest",
                NIGHTLY + "/cargo",
                NIGHTLY + "/cargo-miri",
                NIGHTLY + "/miri",
                NIGHTLY + "/rustc",
            ]
        ),
    }
    sysroot_before = probe(SYSROOT_TREE_COMMAND)
    records = []
    for case in selected:
        record = run_case(case)
        records.append(record)
        print(case + ": " + ",".join(record["classification"]) + f" exit={record['exit_code']}")
    after = inventory()
    sysroot_after = probe(SYSROOT_TREE_COMMAND)
    summary = {
        "schema": "rust-engineering-mcp.m4-miri-classification-oracle.v1",
        "observed_at": dt.datetime.now(dt.timezone.utc).isoformat(),
        "image_id": IMAGE,
        "image_config_digest": config_digest,
        "source_tree_sha256": fixture_digest(FIXTURES),
        "script_sha256": sha256(pathlib.Path(__file__).read_bytes()),
        "probes": probes,
        "sysroot_before": sysroot_before,
        "sysroot_after": sysroot_after,
        "sysroot_unchanged": sysroot_before["stdout"] == sysroot_after["stdout"],
        "cases": [
            {
                "case": record["case"],
                "exit_code": record["exit_code"],
                "classification": record["classification"],
                "fixture_sha256": record["fixture_sha256"],
                "stdout_sha256": record["stdout_sha256"],
                "stderr_sha256": record["stderr_sha256"],
                "junit_sha256": record["junit_sha256"],
                "cleanup_confirmed": record["cleanup_confirmed"],
                "source_unchanged": record["source_unchanged"],
                "passed": record["passed"],
                "forged_runtime_marker_present": record["forged_runtime_marker_present"],
            }
            for record in records
        ],
        "docker_inventory_before": before,
        "docker_inventory_after": after,
        "docker_inventory_preserved": before == after,
        "passed": all(record["passed"] for record in records)
        and before == after
        and sysroot_before["exit_code"] == 0
        and sysroot_after["exit_code"] == 0
        and sysroot_before["stdout"] == sysroot_after["stdout"],
        "claim": "empirical first-party fixture oracle; not gateway qualification",
    }
    (RESULTS / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n")
    if not summary["passed"]:
        raise SystemExit("Miri classification oracle failed")


if __name__ == "__main__":
    main()
