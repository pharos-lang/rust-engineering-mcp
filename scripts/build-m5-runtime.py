#!/usr/bin/env python3
"""Build and receipt the M5 guest image. Verifies the base, never acquires inputs.

The base is named by a local tag because BuildKit resolves a bare `FROM sha256:…`
as a remote reference and Docker 29 removed the legacy builder. The digest
guarantee is preserved here instead: the tag must resolve to the approved image
id before anything is built, and the resolved id is recorded in the receipt.
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
BASE_TAG = "rust-engineering-runtime:1.98.1-arm64-m4-scanner"
BASE_IMAGE_ID = "sha256:25ed3626e710081a571a86a29521eaf2e890e796afd422ba5e409e0ce1891635"
TARGET_TAG = "rust-engineering-runtime:1.98.1-arm64-m5"
BINARIES = ("/opt/perf/bin/cargo-bloat", "/opt/perf/bin/rust-mcp-profile-helper")
DOCKER = os.environ.get("RUST_MCP_DOCKER", "docker")
BUILD_TIMEOUT_S = int(os.environ.get("RUST_MCP_M5_BUILD_TIMEOUT_S", "3600"))


OUTPUT_DEFAULT = ROOT / "docs/validation/M5-provisioning.json"
CONTEXT_DEFAULT = ROOT / "target/m5-provisioning"


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


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=pathlib.Path,
                        default=ROOT / "docs/validation/M5-provisioning.json")
    parser.add_argument("--context", type=pathlib.Path,
                        default=ROOT / "target/m5-provisioning")
    arguments = parser.parse_args()
    output_path = beside_default(OUTPUT_DEFAULT, output_path)
    context_root = beside_default(CONTEXT_DEFAULT, context_root)

    if sys.platform != "darwin" or platform.machine() != "arm64":
        raise RuntimeError("the M5 image is provisioned only on macOS ARM64")

    receipt: dict[str, object] = {
        "schema": "rust-engineering-mcp.m5-provisioning.v1",
        "started_at": utc_now(),
        "authorization": "docs/roadmap/m5-provisioning-request.md",
        "decision": "docs/adr/ADR-075-m5-runtime-provisioning.md",
        "network_used": False,
        "base_tag": BASE_TAG,
        "base_image_id_expected": BASE_IMAGE_ID,
    }

    observed_base = image_id(BASE_TAG)
    receipt["base_image_id_observed"] = observed_base
    if observed_base != BASE_IMAGE_ID:
        receipt["status"] = "failed"
        receipt["error"] = "base tag does not resolve to the approved image id"
        output_path.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
        raise RuntimeError(f"base image mismatch: {observed_base}")

    prepare = subprocess.run(
        [sys.executable, "-B", "fixtures/rust-runtime/m5/provision.py",
         "--cargo-cache", str(pathlib.Path.home() / ".cargo/registry/cache"),
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

    prepared = context_root / "build-context"
    inputs = sorted(p for p in prepared.rglob("*") if p.is_file())
    receipt["context_files"] = len(inputs)
    receipt["context_bytes"] = sum(p.stat().st_size for p in inputs)
    sums_digest = hashlib.sha256((prepared / "SHA256SUMS").read_bytes()).hexdigest()
    receipt["sha256sums_sha256"] = sums_digest

    # BuildKit keys its local-context snapshot on path, size and mtime, and
    # `provision.py` pins every mtime to epoch 0 so the image is reproducible.
    # Two contexts at the same path with the same file sizes therefore look
    # identical to BuildKit even when the bytes differ, and it serves the stale
    # content -- `--no-cache` does not invalidate that snapshot. Building from a
    # content-addressed directory keeps the zeroed mtimes and makes a stale
    # snapshot unreachable, because different content is a different path.
    context = context_root / f"build-context-{sums_digest[:16]}"
    if context.exists():
        shutil.rmtree(context)
    shutil.copytree(prepared, context, copy_function=shutil.copy2)
    receipt["build_context_directory"] = context.name

    # BuildKit's local-context snapshot survives both `--no-cache` and a new
    # context path. With `provision.py` pinning every mtime to epoch 0 for
    # reproducibility, two different contexts can look identical to it, and it
    # served stale helper sources until this prune. `build.sh` caught it only
    # because it verifies SHA256SUMS inside the image. The prune is therefore
    # part of the procedure, not an optimization; the build cache is
    # regenerable and nothing else depends on it.
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
        # `--no-cache` is mandatory, not caution. `provision.py` zeroes every
        # mtime in the context so the build is reproducible, and BuildKit keys
        # its local-context snapshot on path/size/mtime; with the timestamps
        # pinned it reused a stale snapshot and built the PREVIOUS helper
        # sources, which only surfaced because `build.sh` verifies SHA256SUMS
        # inside the image and refused (2 computed checksums did NOT match).
        [DOCKER, "build", "--network=none", "--pull=false", "--no-cache",
         "--build-arg", f"BASE_IMAGE={BASE_TAG}", "--tag", TARGET_TAG, str(context)],
        cwd=ROOT, text=True, capture_output=True, timeout=BUILD_TIMEOUT_S, check=False,
    )
    receipt["build_exit_code"] = build.returncode
    receipt["build_log_tail"] = build.stderr[-8000:] or build.stdout[-8000:]
    if build.returncode != 0:
        receipt["status"] = "failed"
        output_path.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
        return 1

    built = image_id(TARGET_TAG)
    receipt["image_id"] = built
    receipt["image_tag"] = TARGET_TAG

    # The image must contain exactly the two new binaries and nothing else new
    # on PATH; the recorded hashes are the ones the guest itself computed.
    installed = json.loads(
        guest_capture(built, "cat /usr/share/doc/rust-runtime/m5/installed.json")
    )
    receipt["installed"] = installed
    receipt["binaries_present"] = guest_capture(
        built, "for b in " + " ".join(BINARIES) + "; do [ -x $b ] && echo present || echo absent; done"
    ).split()
    receipt["not_on_path"] = guest_capture(
        built,
        "command -v cargo-bloat >/dev/null 2>&1 && echo on_path || echo off_path; "
        "command -v rust-mcp-profile-helper >/dev/null 2>&1 && echo on_path || echo off_path",
    ).split()
    receipt["m4_binaries_intact"] = guest_capture(
        built,
        "for b in /opt/rust/bin/cargo /opt/rust/bin/rustc /opt/security/bin/cargo-deny "
        "/opt/security/bin/rust-mcp-unsafe-helper; do [ -x $b ] && echo present || echo absent; done",
    ).split()
    receipt["toolchain"] = guest_capture(
        built, "/opt/rust/bin/rustc --version && /opt/perf/bin/cargo-bloat --version 2>&1 | head -1"
    ).splitlines()
    receipt["build_context_removed"] = guest_capture(
        built, "[ -e /opt/m5-input ] || [ -e /opt/m5-build ] && echo residue || echo clean"
    )

    ok = (
        receipt["binaries_present"] == ["present", "present"]
        and receipt["not_on_path"] == ["off_path", "off_path"]
        and receipt["m4_binaries_intact"] == ["present"] * 4
        and receipt["build_context_removed"] == "clean"
    )
    receipt["status"] = "passed" if ok else "failed"
    receipt["finished_at"] = utc_now()
    output_path.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    print(json.dumps({"status": receipt["status"], "image_id": built}, sort_keys=True))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
