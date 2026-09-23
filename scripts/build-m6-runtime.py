#!/usr/bin/env python3
"""Build and receipt the M6 guest image. Verifies the base, never widens the pin.

The base is named by a local tag because BuildKit resolves a bare `FROM sha256:…`
as a remote reference and Docker 29 removed the legacy builder. The digest
guarantee is preserved here instead: the tag must resolve to the approved image
id before anything is built, and the resolved id is recorded in the receipt.

`provision.py` is the only step of M6 authorized to use the network (decision
recorded in docs/architecture/decisions.md; historical receipt:
docs/roadmap/m6-provisioning-request.md at 51fa602e); the Docker build below
runs with --network=none.
"""
from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import os
import pathlib
import platform
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
BASE_TAG = "rust-engineering-runtime:1.98.1-arm64-m5"
BASE_IMAGE_ID = "sha256:e0a5ca1661b3e49d0a3d68ee3cc0963453078d08eb7fc43c30538c16b7998aac"
TARGET_TAG = "rust-engineering-runtime:1.98.1-arm64-m6"
ANALYZER_BINARIES = ("/opt/analyzer/bin/rust-analyzer",)
CARRIED_BINARIES = (
    "/opt/rust/bin/cargo",
    "/opt/rust/bin/rustc",
    "/opt/security/bin/cargo-deny",
    "/opt/security/bin/rust-mcp-unsafe-helper",
    "/opt/perf/bin/cargo-bloat",
    "/opt/perf/bin/rust-mcp-profile-helper",
)
DOCKER = os.environ.get("RUST_MCP_DOCKER", "docker")
BUILD_TIMEOUT_S = int(os.environ.get("RUST_MCP_M6_BUILD_TIMEOUT_S", "3600"))

OUTPUT_DEFAULT = ROOT / "tests/data/m6-runtime-provisioning.json"
CONTEXT_DEFAULT = ROOT / "target/m6-provisioning"


def beside_default(default: pathlib.Path, value: object) -> pathlib.Path:
    """Only the file name of a CLI path is honoured, and it lands beside the
    default: an argument can never address a location outside that directory.
    Sonar's taint rules treat every CLI value as attacker-controlled (S2083,
    S8707); `os.path.basename` is the sanitizer they recognise."""
    return default.parent / os.path.basename(os.fspath(value))


def utc_now() -> str:
    return datetime.datetime.now(datetime.UTC).isoformat().replace("+00:00", "Z")


def docker(*args: str, timeout: int = 120) -> str:
    return subprocess.check_output(
        [DOCKER, *args], cwd=ROOT, text=True, stderr=subprocess.PIPE, timeout=timeout
    ).strip()


def image_id(reference: str) -> str:
    return docker("image", "inspect", "--format", "{{.Id}}", reference)


def guest_capture(image: str, command: str) -> str:
    """Read-only, network-free, unprivileged inspection of the built image."""
    return docker(
        "run", "--rm", "--pull=never", "--network=none", "--read-only",
        "--cap-drop=ALL", "--security-opt=no-new-privileges=true",
        "--user=65534:65534", "--entrypoint", "/bin/sh", image, "-c", command,
    )


def presence_report(image: str, binaries: tuple[str, ...]) -> list[str]:
    """`test -x` each absolute path in the guest; order matches the input."""
    command = "; ".join(f"[ -x {binary} ] && echo present || echo absent" for binary in binaries)
    return guest_capture(image, command).split()


def evaluate_status(
    new_components_present: list[str],
    rust_analyzer_off_path: str,
    carried_binaries_present: list[str],
    rust_src_present: bool,
    context_residue: str,
) -> bool:
    """Pure pass/fail rule over the guest-observed evidence. Kept separate from
    the subprocess calls that gather it so the decision itself is unit-testable."""
    return (
        new_components_present == ["present"] * len(new_components_present)
        and rust_analyzer_off_path == "off_path"
        and carried_binaries_present == ["present"] * len(carried_binaries_present)
        and rust_src_present
        and context_residue == "clean"
    )


def build_receipt_skeleton(started_at: str) -> dict[str, object]:
    """The constant, decision-independent fields every receipt carries, pass
    or fail. Factored out so the schema can be asserted without a build."""
    return {
        "schema": "rust-engineering-mcp.m6-provisioning.v1",
        "started_at": started_at,
        "authorization": "docs/roadmap/m6-provisioning-request.md",
        "authorized_by_owner": "2026-09-11",
        "decision": "docs/adr/ADR-082-m6-runtime-provisioning.md",
        "network_used_for": ["manifest", "rust-analyzer tarball", "rust-src tarball"],
        "build_network": "none",
        "base_tag": BASE_TAG,
        "base_image_id_expected": BASE_IMAGE_ID,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=pathlib.Path, default=OUTPUT_DEFAULT)
    parser.add_argument("--context", type=pathlib.Path, default=CONTEXT_DEFAULT)
    arguments = parser.parse_args()
    output_path = beside_default(OUTPUT_DEFAULT, arguments.output)
    context_root = beside_default(CONTEXT_DEFAULT, arguments.context)
    output_path.parent.mkdir(parents=True, exist_ok=True)

    if sys.platform != "darwin" or platform.machine() != "arm64":
        raise RuntimeError("the M6 image is provisioned only on macOS ARM64")

    receipt = build_receipt_skeleton(utc_now())

    observed_base = image_id(BASE_TAG)
    receipt["base_image_id_observed"] = observed_base
    if observed_base != BASE_IMAGE_ID:
        receipt["status"] = "failed"
        receipt["error"] = "base tag does not resolve to the approved image id"
        output_path.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
        raise RuntimeError(f"base image mismatch: {observed_base}")

    prepare = subprocess.run(
        [sys.executable, "-B", "fixtures/rust-runtime/m6/provision.py",
         "--output", str(context_root)],
        cwd=ROOT, text=True, capture_output=True, timeout=900, check=False,
    )
    if prepare.returncode != 0:
        receipt["status"] = "failed"
        receipt["error"] = "prepare failed"
        receipt["prepare_stderr"] = prepare.stderr[-4000:]
        output_path.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
        return 1
    receipt["prepare"] = json.loads(prepare.stdout)
    receipt["inputs"] = receipt["prepare"]["inputs"]

    prepared = context_root / "build-context"
    inputs = sorted(p for p in prepared.rglob("*") if p.is_file())
    receipt["context_files"] = len(inputs)
    receipt["context_bytes"] = sum(p.stat().st_size for p in inputs)
    sums_digest = hashlib.sha256((prepared / "SHA256SUMS").read_bytes()).hexdigest()
    receipt["sha256sums_sha256"] = sums_digest

    # Same reproducibility hazard ADR-075/scripts/build-m5-runtime.py document:
    # BuildKit keys its local-context snapshot on path, size and mtime, and
    # `provision.py` pins every mtime to epoch 0. Building from a
    # content-addressed directory keeps the zeroed mtimes and makes a stale
    # snapshot unreachable, because different content is a different path.
    context = context_root / f"build-context-{sums_digest[:16]}"
    if context.exists():
        shutil.rmtree(context)
    shutil.copytree(prepared, context, copy_function=shutil.copy2)
    receipt["build_context_directory"] = context.name

    prune = subprocess.run(
        [DOCKER, "builder", "prune", "--all", "--force"],
        cwd=ROOT, text=True, capture_output=True, timeout=600, check=False,
    )
    receipt["builder_cache_pruned"] = prune.returncode == 0
    if prune.returncode != 0:
        receipt["status"] = "failed"
        receipt["error"] = "could not prune the builder cache before building"
        output_path.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
        return 1

    build = subprocess.run(
        [DOCKER, "build", "--network=none", "--pull=false", "--no-cache",
         "--build-arg", f"BASE_IMAGE={BASE_TAG}", "--tag", TARGET_TAG, str(context)],
        cwd=ROOT, text=True, capture_output=True, timeout=BUILD_TIMEOUT_S, check=False,
    )
    receipt["build_exit_code"] = build.returncode
    receipt["build_log_tail"] = build.stderr[-8000:] or build.stdout[-8000:]
    if build.returncode != 0:
        receipt["status"] = "failed"
        receipt["error"] = "docker build failed"
        output_path.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
        return 1

    built = image_id(TARGET_TAG)
    receipt["image_id"] = built
    receipt["image_tag"] = TARGET_TAG

    installed = json.loads(
        guest_capture(built, "cat /usr/share/doc/rust-runtime/m6/installed.json")
    )
    receipt["installed"] = installed
    receipt["rust_analyzer_version"] = guest_capture(
        built, "cat /usr/share/doc/rust-runtime/m6/rust-analyzer-version.txt"
    ).strip()

    new_components_present = presence_report(built, ANALYZER_BINARIES)
    receipt["new_components_present"] = new_components_present

    rust_analyzer_off_path = guest_capture(
        built,
        "command -v rust-analyzer >/dev/null 2>&1 && echo on_path || echo off_path",
    ).strip()
    receipt["rust_analyzer_on_path"] = rust_analyzer_off_path

    carried_binaries_present = presence_report(built, CARRIED_BINARIES)
    receipt["carried_binaries_present"] = carried_binaries_present

    rust_src_present = guest_capture(
        built,
        "[ -d /opt/rust/lib/rustlib/src/rust/library ] && "
        "[ -f /opt/rust/lib/rustlib/src/rust/library/Cargo.lock ] && "
        "echo present || echo absent",
    ).strip()
    receipt["rust_src_present"] = rust_src_present

    receipt["context_residue"] = guest_capture(
        built, "[ -e /opt/m6-input ] || [ -e /opt/m6-build ] && echo residue || echo clean"
    ).strip()

    ok = evaluate_status(
        new_components_present,
        rust_analyzer_off_path,
        carried_binaries_present,
        rust_src_present == "present",
        receipt["context_residue"],
    )
    receipt["status"] = "passed" if ok else "failed"
    receipt["finished_at"] = utc_now()
    output_path.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    print(json.dumps({"status": receipt["status"], "image_id": built}, sort_keys=True))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
