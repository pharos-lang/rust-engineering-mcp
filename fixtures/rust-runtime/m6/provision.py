#!/usr/bin/env python3
"""Prepare the closed build context for the ADR-082 M6 runtime image.

This is the only step of M6 authorized to use the network
(docs/roadmap/m6-provisioning-request.md), and only to fetch exactly the three
URLs pinned below: the 1.98.1 channel manifest and the two component tarballs
it must describe. Every byte is verified against a checksum fixed in this file
before it enters the build context; nothing is trusted from the network alone.
The Docker build itself (fixtures/rust-runtime/m6/build.sh) runs with
--network=none and never touches the network.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import tarfile
import tomllib
import urllib.request
from pathlib import Path, PurePosixPath
from typing import Callable

ROOT = Path(__file__).resolve().parents[3]
HERE = Path(__file__).resolve().parent
BASE_IMAGE_ID = "sha256:e0a5ca1661b3e49d0a3d68ee3cc0963453078d08eb7fc43c30538c16b7998aac"
TARGET = "aarch64-unknown-linux-gnu"
SOURCE_DATE_EPOCH = 0

OUTPUT_DEFAULT = ROOT / "target/m6-provisioning"
MAX_MEMBER_BYTES = 64 * 1024 * 1024
MAX_ARCHIVE_BYTES = 512 * 1024 * 1024


def beside_default(default: Path, value: object) -> Path:
    """Only the file name of a CLI path is honoured, and it lands beside the
    default: an argument can never address a location outside that directory.
    Sonar's taint rules treat every CLI value as attacker-controlled (S2083,
    S8707); `os.path.basename` is the sanitizer they recognise."""
    return default.parent / os.path.basename(os.fspath(value))

DIST_HOST_PREFIX = "https://static.rust-lang.org/dist/"

MANIFEST_URL = DIST_HOST_PREFIX + "channel-rust-1.98.1.toml"
MANIFEST_SHA256 = "a7c8774a5fd8441c997d94c029776cbc5eb111e9d72ab5d256fa69866644347e"

# (manifest pkg name, manifest target key, url, xz sha256, tarball's own top-level
# directory, license, human label used in the receipt and Dockerfile context)
RUST_ANALYZER = {
    "pkg": "rust-analyzer-preview",
    "manifest_target": TARGET,
    "url": DIST_HOST_PREFIX + "2026-09-03/rust-analyzer-1.98.1-aarch64-unknown-linux-gnu.tar.xz",
    "sha256": "a0fd960a9ab36193ae9ba4310e5f780f6ca38fa86160fae739be4ac541b6d10c",
    "archive_root": "rust-analyzer-1.98.1-aarch64-unknown-linux-gnu",
    "license": "MIT OR Apache-2.0",
    "label": "rust-analyzer",
}
RUST_SRC = {
    "pkg": "rust-src",
    "manifest_target": "*",
    "url": DIST_HOST_PREFIX + "2026-09-03/rust-src-1.98.1.tar.xz",
    "sha256": "5c846ebcebcc7e2e0777a4cdaa12051691593f16a7e94edbae5e6241cc62d98c",
    "archive_root": "rust-src-1.98.1",
    "license": "MIT OR Apache-2.0",
    "label": "rust-src",
}
INPUTS = (RUST_ANALYZER, RUST_SRC)

NOTICE_NAMES = ("LICENSE-APACHE", "LICENSE-MIT", "COPYRIGHT")
SAFE_ARCHIVE_NAME = re.compile(r"[A-Za-z0-9_./+@(), =:-]+")


def sha256(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def regular(path: Path) -> None:
    if path.is_symlink() or not path.is_file():
        raise ValueError(f"expected unlinked regular file: {path}")


def parse_manifest_entries(
    text: str, inputs: tuple[dict[str, object], ...] = INPUTS
) -> dict[tuple[str, str], dict[str, object]]:
    """Read only the entries this provisioning step is authorized to use."""
    parsed = tomllib.loads(text)
    packages = parsed.get("pkg", {})
    entries: dict[tuple[str, str], dict[str, object]] = {}
    for input_spec in inputs:
        package = packages.get(input_spec["pkg"])
        if not isinstance(package, dict):
            raise ValueError(f"manifest missing package: {input_spec['pkg']}")
        targets = package.get("target", {})
        entry = targets.get(input_spec["manifest_target"])
        if not isinstance(entry, dict):
            raise ValueError(
                f"manifest missing target {input_spec['manifest_target']!r} "
                f"for package {input_spec['pkg']}"
            )
        entries[(input_spec["pkg"], input_spec["manifest_target"])] = entry
    return entries


def verify_manifest_entries(
    entries: dict[tuple[str, str], dict[str, object]],
    inputs: tuple[dict[str, object], ...] = INPUTS,
) -> None:
    """Cross-check the pinned constants against the published manifest before
    anything is downloaded. A mismatch fails closed; it never widens the pin."""
    for input_spec in inputs:
        entry = entries[(input_spec["pkg"], input_spec["manifest_target"])]
        if entry.get("xz_url") != input_spec["url"]:
            raise ValueError(
                f"manifest xz_url for {input_spec['pkg']} does not match the pinned URL"
            )
        if entry.get("xz_hash") != input_spec["sha256"]:
            raise ValueError(
                f"manifest xz_hash for {input_spec['pkg']} does not match the pinned sha256"
            )


def fetch_url(url: str) -> bytes:
    if not url.startswith(DIST_HOST_PREFIX):
        raise ValueError(f"refusing to fetch outside the pinned distribution host: {url}")
    with urllib.request.urlopen(url, timeout=120) as response:  # noqa: S310 (pinned https host only)
        return response.read()


def ensure_downloaded(
    url: str,
    expected_sha256: str,
    destination: Path,
    fetch: Callable[[str], bytes] = fetch_url,
) -> Path:
    """Skip the network entirely when a prior run already left the right bytes."""
    if destination.is_file() and not destination.is_symlink() and sha256(destination) == expected_sha256:
        return destination
    destination.parent.mkdir(parents=True, exist_ok=True)
    data = fetch(url)
    observed = sha256_bytes(data)
    if observed != expected_sha256:
        raise ValueError(f"downloaded artifact checksum mismatch for {url}: {observed}")
    part = destination.with_name(destination.name + ".part")
    part.write_bytes(data)
    os.replace(part, destination)
    return destination


def validate_component_archive(path: Path, expected_root: str) -> int:
    """Reject any component tarball whose members could escape its own root."""
    regular(path)
    seen: set[str] = set()
    count = 0
    total_bytes = 0
    with tarfile.open(path, "r:xz") as archive:
        for member in archive.getmembers():
            name = member.name
            parsed = PurePosixPath(name)
            normalized = parsed.as_posix().rstrip("/")
            if (
                not name
                or parsed.is_absolute()
                or ".." in parsed.parts
                or "\\" in name
                or not SAFE_ARCHIVE_NAME.fullmatch(name)
            ):
                raise ValueError(f"unsafe archive path: {name!r}")
            if not parsed.parts or parsed.parts[0] != expected_root:
                raise ValueError(f"archive member outside expected root: {name!r}")
            if normalized in seen:
                raise ValueError(f"duplicate archive member: {name!r}")
            seen.add(normalized)
            if member.issym() or member.islnk() or member.isdev() or member.isfifo():
                raise ValueError(f"linked or special archive member: {name!r}")
            if not (member.isfile() or member.isdir()):
                raise ValueError(f"unsupported archive member: {name!r}")
            if member.size > MAX_MEMBER_BYTES:
                raise ValueError(f"archive member too large: {name!r} ({member.size} bytes)")
            total_bytes += member.size
            if total_bytes > MAX_ARCHIVE_BYTES:
                raise ValueError(f"archive too large: {path.name} exceeds {MAX_ARCHIVE_BYTES} bytes")
            count += 1
    if count == 0:
        raise ValueError(f"empty component archive: {path.name}")
    return count


def extract_root_notices(path: Path, expected_root: str, destination: Path) -> list[str]:
    """Collect the license/notice files the tarball ships at its own top level.

    The destination directory is created only if at least one notice is found,
    so a tarball that ships none never leaves an empty directory behind for
    `validate_context` to reject as an unexpected build-context entry.
    """
    collected: list[str] = []
    with tarfile.open(path, "r:xz") as archive:
        for name in NOTICE_NAMES:
            member_name = f"{expected_root}/{name}"
            try:
                member = archive.getmember(member_name)
            except KeyError:
                continue
            if not member.isfile():
                continue
            extracted = archive.extractfile(member)
            if extracted is None:
                continue
            destination.mkdir(parents=True, exist_ok=True)
            (destination / name).write_bytes(extracted.read())
            collected.append(name)
    return collected


def validate_context(path: Path, allowed_files: set[str], complete: bool) -> None:
    if path.is_symlink() or not path.is_dir():
        raise ValueError(f"invalid build context: {path}")
    observed: set[str] = set()
    allowed_directories = {
        str(parent)
        for relative in allowed_files
        for parent in PurePosixPath(relative).parents
        if str(parent) != "."
    }
    for entry in path.rglob("*"):
        relative = entry.relative_to(path).as_posix()
        if entry.is_symlink():
            raise ValueError(f"linked build-context entry: {relative}")
        if entry.is_dir():
            if relative not in allowed_directories:
                raise ValueError(f"unexpected build-context directory: {relative}")
            continue
        if not entry.is_file() or relative not in allowed_files:
            raise ValueError(f"unexpected build-context entry: {relative}")
        observed.add(relative)
    if complete and observed != allowed_files:
        raise ValueError(f"incomplete build context: {sorted(allowed_files - observed)}")


def normalize_context(path: Path) -> None:
    for entry in sorted(path.rglob("*"), reverse=True):
        if entry.is_file():
            entry.chmod(0o755 if entry.name == "build.sh" else 0o644)
            os.utime(entry, (SOURCE_DATE_EPOCH, SOURCE_DATE_EPOCH))
        elif entry.is_dir():
            entry.chmod(0o755)
            os.utime(entry, (SOURCE_DATE_EPOCH, SOURCE_DATE_EPOCH))
    os.utime(path, (SOURCE_DATE_EPOCH, SOURCE_DATE_EPOCH))


def prepare(
    output: Path,
    fetch: Callable[[str], bytes] = fetch_url,
    manifest_url: str = MANIFEST_URL,
    manifest_sha256: str = MANIFEST_SHA256,
    inputs: tuple[dict[str, object], ...] = INPUTS,
) -> dict[str, object]:
    downloads = output / "downloads"
    downloads.mkdir(parents=True, exist_ok=True)

    manifest_path = downloads / "channel-rust-1.98.1.toml"
    manifest_path = ensure_downloaded(manifest_url, manifest_sha256, manifest_path, fetch)
    entries = parse_manifest_entries(manifest_path.read_text(), inputs)
    verify_manifest_entries(entries, inputs)

    output.mkdir(parents=True, exist_ok=True)
    context = output / "build-context"
    if context.exists():
        shutil.rmtree(context)
    context.mkdir(parents=True)

    shutil.copyfile(HERE / "Dockerfile", context / "Dockerfile")
    shutil.copyfile(HERE / "build.sh", context / "build.sh")

    inputs_manifest: list[dict[str, object]] = []
    for input_spec in inputs:
        filename = input_spec["url"].rsplit("/", 1)[-1]
        downloaded = ensure_downloaded(
            input_spec["url"], input_spec["sha256"], downloads / filename, fetch
        )
        member_count = validate_component_archive(downloaded, input_spec["archive_root"])
        shutil.copyfile(downloaded, context / filename)
        notices_dir = context / "notices" / input_spec["label"]
        notice_files = extract_root_notices(downloaded, input_spec["archive_root"], notices_dir)
        inputs_manifest.append(
            {
                "label": input_spec["label"],
                "package": input_spec["pkg"],
                "manifest_target": input_spec["manifest_target"],
                "url": input_spec["url"],
                "sha256": input_spec["sha256"],
                "filename": filename,
                "size": downloaded.stat().st_size,
                "license": input_spec["license"],
                "archive_member_count": member_count,
                "archive_root": input_spec["archive_root"],
                "notice_files": notice_files,
            }
        )

    build_inputs = {
        "schema": "rust-engineering-mcp.m6-build-inputs.v1",
        "base_image_id": BASE_IMAGE_ID,
        "target": TARGET,
        "manifest": {
            "url": manifest_url,
            "sha256": manifest_sha256,
        },
        "inputs": inputs_manifest,
        "network_required": True,
    }
    (context / "build-inputs.json").write_text(
        json.dumps(build_inputs, indent=2, sort_keys=True) + "\n"
    )

    allowed = {
        entry.relative_to(context).as_posix()
        for entry in context.rglob("*")
        if entry.is_file()
    } | {"SHA256SUMS"}
    hashed = sorted(allowed - {"SHA256SUMS"})
    sums = "".join(f"{sha256(context / relative)}  {relative}\n" for relative in hashed)
    (context / "SHA256SUMS").write_text(sums)
    validate_context(context, allowed, complete=True)
    normalize_context(context)

    receipt = {
        "schema": "rust-engineering-mcp.m6-prepare.v1",
        "status": "prepared_not_built",
        "base_image_id": BASE_IMAGE_ID,
        "target": TARGET,
        "manifest_sha256_verified": True,
        "inputs": inputs_manifest,
        "context_file_count": len(allowed),
        "context_sha256s_digest": sha256_bytes(sums.encode()),
        "network_used_for": ["manifest", "rust-analyzer tarball", "rust-src tarball"],
    }
    (output / "prepare-receipt.json").write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    return receipt


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, default=OUTPUT_DEFAULT)
    arguments = parser.parse_args()
    output = beside_default(OUTPUT_DEFAULT, arguments.output)
    print(json.dumps(prepare(output), sort_keys=True))


if __name__ == "__main__":
    main()
