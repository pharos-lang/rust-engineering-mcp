#!/usr/bin/env python3
"""Explicit M5 performance-runtime qualification; no provisioning, pulls or downloads."""
import datetime
import hashlib
import json
import os
import pathlib
import platform
import re
import signal
import subprocess
import sys
import time

ROOT = pathlib.Path(__file__).resolve().parents[1]
# The native tests own `target/m5-runtime`: `performance_native::publish()`
# writes `cut-<cut>.json` there and rebuilds `receipt.json` from them after every
# cut. This gate therefore writes its own receipt somewhere else. Overwriting the
# product's qualification receipt with the gate's would destroy the very evidence
# the gate exists to collect.
OUTPUT = pathlib.Path(
    os.environ.get("RUST_MCP_M5_RUNTIME_OUTPUT", ROOT / "target/m5-runtime-gate")
)
NATIVE_OUTPUT = ROOT / "target/m5-runtime"
# The three places the admitted M5 digest is written down. `performance_port.rs`
# is the source of truth and the other two are cross-checks, because the product
# admits an image by comparing against the `M5_IMAGE` constant compiled into the
# port: the ADR is the decision that authorised the digest and
# `M5-provisioning.json` is the receipt of the build that produced it, but
# neither is consulted at runtime. Measuring an image the code does not admit is
# precisely the defect this stage exists to prevent, so a disagreement between
# the three is a loud refusal, never a preference for one of them.
PORT_SOURCE = ROOT / "crates/execution-adapter/src/performance_port.rs"
ADMISSION_DECISION = ROOT / "docs/adr/ADR-077-m5-runtime-admission.md"
PROVISIONING_RECEIPT = ROOT / "docs/validation/M5-provisioning.json"
NATIVE_SOURCE = ROOT / "crates/execution-adapter/src/performance_native.rs"
PACKAGE = "rust-engineering-execution"
# `performance_native` is a `#[cfg(test)] mod` of the execution adapter's lib,
# so the harness selection is the module path plus the function name.
MODULE = NATIVE_SOURCE.stem
# The refusal cut must run before the positives: it is the cheapest selection in
# the file (it opens the M4 image, expects `Unavailable` before any container is
# created, and completed in 37 ms in docs/validation/M5-runtime.json), and it is
# the one that proves the admission list itself. A stale digest, a mis-set
# RUST_MCP_TEST_IMAGE or a host without the M4 image therefore fails in seconds
# instead of after the long positive measurements.
ADMISSION_TEST = "m5_tools_refuse_every_runtime_but_the_qualified_one"
DIGEST = r"sha256:[0-9a-f]{64}"
# The receipt the tests publish (ADR-073/075 evidence). This gate records its
# digest per step so the two documents can be tied together afterwards; it never
# writes into the tests' directory.
NATIVE_RECEIPT_SCHEMA = "rust-engineering-mcp.m5-runtime.v1"
# A stalled selection must become a recorded failure, never an unattended gate
# hang. This bound is deliberately far above any legitimate M5 step and is not a
# substitute for the in-gateway budgets.
STEP_TIMEOUT_S = int(os.environ.get("RUST_MCP_M5_STEP_TIMEOUT_S", "900"))


def utc_now():
    return datetime.datetime.now(datetime.UTC).isoformat().replace("+00:00", "Z")


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def admitted_image():
    """The digest the product admits, refusing when the recorded copies differ."""
    match = re.search(
        rf'pub const M5_IMAGE: &str =\s*"({DIGEST})"\s*;', PORT_SOURCE.read_text()
    )
    if not match:
        raise RuntimeError(f"M5_IMAGE constant not found in {PORT_SOURCE}")
    image = match.group(1)
    decision = sorted(set(re.findall(DIGEST, ADMISSION_DECISION.read_text())))
    built = json.loads(PROVISIONING_RECEIPT.read_text()).get("image_id")
    if decision != [image] or built != image:
        raise RuntimeError(
            "admitted M5 image disagrees across its recorded sources; refusing to "
            f"qualify an image the product does not admit: {PORT_SOURCE}={image} "
            f"{ADMISSION_DECISION}={decision} {PROVISIONING_RECEIPT}={built}"
        )
    return image


def ignored_tests(path):
    """The `#[ignore]`d top-level `#[test]` functions declared by `path`, in order."""
    names = []
    saw_test = saw_ignore = False
    for line in path.read_text().splitlines():
        if line.startswith("#[test]"):
            saw_test, saw_ignore = True, False
            continue
        if saw_test and line.startswith("#[ignore"):
            saw_ignore = True
            continue
        if saw_test and line.startswith("#["):
            continue
        match = re.match(r"fn ([A-Za-z0-9_]+)\s*\(", line)
        if saw_test and match:
            if saw_ignore:
                names.append(match.group(1))
            saw_test = saw_ignore = False
            continue
        if line.strip():
            saw_test = saw_ignore = False
    return names


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


# The materialized criterion vendor tree: gitignored, generated by
# `fixtures/criterion-vendor/materialize.py` from the 52 committed `.crate`
# archives, and 6 010 files of it. Its *shape* is exactly what the M5-01 cut
# measures against the `SourceBundle` bounds, so it cannot be left out of the
# evidence; listing it file by file would put six thousand rows and a megabyte
# into every receipt for no added discrimination. It is rolled up into one
# order-independent digest instead, beside the archives and the script that
# produce it, which are hashed individually.
GENERATED_VENDOR = ROOT / "fixtures/criterion-vendor/vendor"


def rolled_up(directory):
    """One digest, file count and byte total for a generated tree."""
    rolling = hashlib.sha256()
    files = 0
    total = 0
    for path in sorted(directory.rglob("*")):
        if not path.is_file():
            continue
        data = path.read_bytes()
        rolling.update(str(path.relative_to(ROOT)).encode())
        rolling.update(b"\0")
        rolling.update(hashlib.sha256(data).digest())
        files += 1
        total += len(data)
    return {"path": str(directory.relative_to(ROOT)), "generated": True,
            "files": files, "bytes": total, "rollup_sha256": rolling.hexdigest()}


def native_receipt_digest():
    """Digest of the receipt the tests publish, as it stands after a step.

    Read-only: the tests own `target/m5-runtime` and this gate never writes into
    it. `None` means no native receipt was present, or the document there is not
    the tests' own — either way the gate records the absence instead of guessing.
    """
    published = NATIVE_OUTPUT / "receipt.json"
    try:
        document = json.loads(published.read_text())
    except (OSError, ValueError):
        return None
    if document.get("schema") != NATIVE_RECEIPT_SCHEMA:
        return None
    return sha256(published)


def main():
    if sys.platform != "darwin" or platform.machine() != "arm64":
        raise RuntimeError("M5 runtime is qualified only on macOS ARM64/Docker Linux ARM64")
    if not os.environ.get("RUST_MCP_TEST_SOCKET"):
        raise RuntimeError("RUST_MCP_TEST_SOCKET required; no socket discovery or substitution")
    if STEP_TIMEOUT_S <= 0:
        raise RuntimeError("RUST_MCP_M5_STEP_TIMEOUT_S must be a positive number of seconds")
    image = admitted_image()
    allowed = {"HOME", "PATH", "TMPDIR", "CARGO_HOME", "RUSTUP_HOME", "SDKROOT",
               "DEVELOPER_DIR", "CARGO_TARGET_DIR", "RUST_MCP_TEST_SOCKET"}
    env = {key: value for key, value in os.environ.items() if key in allowed}
    env.update(CARGO_INCREMENTAL="0", CARGO_TERM_COLOR="never", RUST_MCP_TEST_IMAGE=image)
    cargo = pathlib.Path(subprocess.check_output(
        ["rustup", "which", "--toolchain", "1.98.1", "cargo"], env=env, text=True).strip())
    env["PATH"] = str(cargo.parent) + os.pathsep + env.get("PATH", "")
    env["RUSTC"] = str(cargo.with_name("rustc"))
    declared = ignored_tests(NATIVE_SOURCE)
    if ADMISSION_TEST not in declared:
        raise RuntimeError(
            f"{ADMISSION_TEST} is not an ignored test of {NATIVE_SOURCE}; the ordering "
            "this gate depends on no longer matches the source"
        )
    # Refusal first, then the positives in their declared order.
    tests = [f"{MODULE}::{ADMISSION_TEST}"] + [
        f"{MODULE}::{name}" for name in declared if name != ADMISSION_TEST
    ]
    OUTPUT.mkdir(parents=True, exist_ok=True)
    receipt = {
        "schema": "rust-mcp-m5-runtime-v1",
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
        ROOT / "Cargo.toml", ROOT / "Cargo.lock", ROOT / "rust-toolchain.toml",
        ROOT / "scripts/test-m5-runtime.py",
        ADMISSION_DECISION, PROVISIONING_RECEIPT,
        *sorted((ROOT / "crates/execution-adapter/src").glob("seccomp*.json")),
        *sorted(path for directory in ["profile-helper", "criterion-vendor", "benchmark",
                                       "bloat", "profile-workload", "benchmark-datasets",
                                       "rust-runtime/m5"]
                for path in (ROOT / "fixtures" / directory).rglob("*")
                if path.is_file() and "target" not in path.parts
                and GENERATED_VENDOR not in path.parents),
    ]
    for path in configuration_paths:
        receipt["configuration_inputs"].append(
            {"path": str(path.relative_to(ROOT)), "sha256": sha256(path)}
        )
    receipt["generated_inputs"] = [rolled_up(GENERATED_VENDOR)]

    def save():
        (OUTPUT / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")

    try:
        for number, selection in enumerate(tests):
            command = [str(cargo), "test", "--locked", "--offline", "-p", PACKAGE,
                       "--lib", selection, "--", "--exact", "--ignored", "--nocapture",
                       "--test-threads=1"]
            log = OUTPUT / f"{number}.log"
            print(f"M5 RUNTIME {selection}", flush=True)
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
            # A filtered-out selection exits zero and proves nothing, so exactly
            # one executed case is required rather than merely a zero exit.
            passed = (not timed_out and returncode == 0
                      and "test result: ok. 1 passed; 0 failed; 0 ignored;" in output)
            step = {
                "selection": selection,
                "image_id": image,
                "command": command,
                "status": "passed" if passed else "failed",
                "exit_code": returncode,
                "timed_out": timed_out,
                "expected_executed": 1,
                "seconds": round(time.monotonic() - started, 3),
                "log_sha256": sha256(log),
                "native_receipt_sha256": native_receipt_digest(),
            }
            if timed_out:
                step["owned_docker_before_kill"] = docker_before_kill
                step["owned_docker_after_kill"] = docker_after_kill
            receipt["steps"].append(step)
            save()
            if timed_out:
                raise RuntimeError(
                    f"M5 test exceeded the {STEP_TIMEOUT_S}s step bound and was killed: {log}")
            if not passed:
                raise RuntimeError(f"M5 test failed or exactly one case did not execute: {log}")
        receipt["status"] = "passed"
    except BaseException as error:
        receipt.update(status="failed", error=str(error))
        raise
    finally:
        receipt["finished_at"] = utc_now()
        save()
    # A vanished input is a changed input, not a traceback.
    for entry in receipt["sources"] + receipt["configuration_inputs"]:
        path = ROOT / entry["path"]
        if not path.is_file() or sha256(path) != entry["sha256"]:
            receipt.update(status="failed", error="source inputs changed during qualification")
            save()
            raise RuntimeError(receipt["error"])
    print(f"PASS M5 runtime: {OUTPUT / 'receipt.json'}", flush=True)


if __name__ == "__main__":
    if not __debug__:
        raise RuntimeError("Optimized Python mode is rejected")
    main()
