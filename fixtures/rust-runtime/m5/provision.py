#!/usr/bin/env python3
"""Prepare the closed, offline build context for the ADR-075 M5 runtime image.

Every registry archive is taken from the local Cargo cache and verified against
the checksum Cargo itself recorded in the pinned lockfile of the package that
needs it. This script performs no network access; it fails if an input is
missing rather than acquiring it.
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
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parents[3]
HERE = Path(__file__).resolve().parent
HELPER = ROOT / "fixtures/profile-helper"
BASE_IMAGE_ID = "sha256:25ed3626e710081a571a86a29521eaf2e890e796afd422ba5e409e0ce1891635"
TARGET = "aarch64-unknown-linux-gnu"
SOURCE_DATE_EPOCH = 0

BLOAT_NAME = "cargo-bloat"
BLOAT_VERSION = "0.12.1"
BLOAT_SHA256 = "56e2c483ab55e38021c2c701061e078cc6c28d563932cbdf6bc4efaa28ab117e"
BLOAT_LICENSE = "MIT"
BLOAT_SOURCE_DIR = "cargo-bloat-src"

HELPER_PACKAGE = "rust-mcp-profile-helper"
HELPER_BINARY = "rust-mcp-profile-helper"
NOTICE_FILES = ("LICENSE", "LICENSE-APACHE", "LICENSE-MIT", "NOTICE")

LOCK_PACKAGE = re.compile(
    r'\[\[package\]\]\nname = "([^"]+)"\nversion = "([^"]+)"\n'
    r'source = "[^"]*"\nchecksum = "([0-9a-f]{64})"'
)
SAFE_ARCHIVE_NAME = re.compile(r"[A-Za-z0-9_./+@(), =:-]+")


def sha256(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def regular(path: Path) -> None:
    if path.is_symlink() or not path.is_file():
        raise ValueError(f"expected unlinked regular file: {path}")


def validate_archive(path: Path, package_name: str, version: str) -> int:
    """Reject any crate archive that could escape its extraction root."""
    regular(path)
    expected_root = f"{package_name}-{version}"
    seen: set[str] = set()
    count = 0
    with tarfile.open(path, "r:gz") as archive:
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
                raise ValueError(f"archive member outside package root: {name!r}")
            if normalized in seen:
                raise ValueError(f"duplicate archive member: {name!r}")
            seen.add(normalized)
            if member.issym() or member.islnk() or member.isdev() or member.isfifo():
                raise ValueError(f"linked or special archive member: {name!r}")
            if not (member.isfile() or member.isdir()):
                raise ValueError(f"unsupported archive member: {name!r}")
            count += 1
    if count == 0:
        raise ValueError(f"empty crate archive: {path.name}")
    return count


def lock_closure(text: str) -> dict[tuple[str, str], str]:
    return {(name, version): digest for name, version, digest in LOCK_PACKAGE.findall(text)}


def cached_archive(cache_root: Path, name: str, version: str, expected: str) -> Path:
    if cache_root.is_symlink() or not cache_root.is_dir():
        raise ValueError(f"invalid Cargo cache root: {cache_root}")
    filename = f"{name}-{version}.crate"
    candidates = sorted(cache_root.glob(f"*/{filename}"))
    if not candidates:
        raise ValueError(f"missing cached crate (no acquisition here): {filename}")
    for candidate in candidates:
        regular(candidate)
        if sha256(candidate) != expected:
            raise ValueError(f"cached crate checksum mismatch: {filename}")
    return candidates[0]


def package_license(root: Path) -> str:
    manifest = tomllib.loads((root / "Cargo.toml").read_text())
    package = manifest.get("package", {})
    expression = package.get("license")
    if isinstance(expression, str) and expression:
        return expression
    named = package.get("license-file")
    if isinstance(named, str) and named:
        return f"file:{named}"
    return "unknown"


def helper_source_files() -> list[str]:
    files = ["Cargo.toml", "Cargo.lock"]
    source = HELPER / "src"
    if not source.is_dir() or source.is_symlink():
        raise ValueError("helper src directory missing")
    for entry in sorted(source.rglob("*.rs")):
        regular(entry)
        files.append(entry.relative_to(HELPER).as_posix())
    return files


def verify_helper() -> list[dict[str, object]]:
    """The helper contract the image is allowed to build. No other shape passes."""
    relatives = helper_source_files()
    for relative in relatives:
        regular(HELPER / relative)
    manifest = tomllib.loads((HELPER / "Cargo.toml").read_text())
    package = manifest.get("package", {})
    if package.get("name") != HELPER_PACKAGE:
        raise ValueError("unexpected helper package name")
    if package.get("publish") is not False:
        raise ValueError("helper must not be publishable")
    binaries = manifest.get("bin", [])
    if [entry.get("name") for entry in binaries] != [HELPER_BINARY]:
        raise ValueError("unexpected helper binary contract")
    dependencies = manifest.get("dependencies", {})
    if set(dependencies) != {"libc"}:
        raise ValueError(f"helper dependency set changed: {sorted(dependencies)}")
    return [
        {
            "path": relative,
            "size": (HELPER / relative).stat().st_size,
            "sha256": sha256(HELPER / relative),
        }
        for relative in relatives
    ]


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


def prepare(cache_root: Path, output: Path) -> dict[str, object]:
    helper_inventory = verify_helper()
    for notice in NOTICE_FILES:
        regular(ROOT / notice)

    bloat_archive = cached_archive(cache_root, BLOAT_NAME, BLOAT_VERSION, BLOAT_SHA256)
    validate_archive(bloat_archive, BLOAT_NAME, BLOAT_VERSION)
    with tarfile.open(bloat_archive, "r:gz") as archive:
        member = archive.extractfile(f"{BLOAT_NAME}-{BLOAT_VERSION}/Cargo.lock")
        if member is None:
            raise ValueError("cargo-bloat archive has no pinned Cargo.lock")
        bloat_lock = member.read().decode()

    closure = lock_closure(bloat_lock)
    closure.update(lock_closure((HELPER / "Cargo.lock").read_text()))
    if not closure:
        raise ValueError("empty registry closure")

    output.mkdir(parents=True, exist_ok=True)
    context = output / "build-context"
    if context.exists():
        shutil.rmtree(context)
    context.mkdir(parents=True)

    with tarfile.open(bloat_archive, "r:gz") as archive:
        archive.extractall(context / "unpacked", filter="data")
    (context / "unpacked" / f"{BLOAT_NAME}-{BLOAT_VERSION}").rename(context / BLOAT_SOURCE_DIR)
    (context / "unpacked").rmdir()
    # Cargo refuses to build a package whose archive still declares itself vendored.
    for stray in (context / BLOAT_SOURCE_DIR).rglob(".cargo-checksum.json"):
        stray.unlink()

    shutil.copyfile(HERE / "Dockerfile", context / "Dockerfile")
    shutil.copyfile(HERE / "build.sh", context / "build.sh")
    (context / "helper/src").mkdir(parents=True, exist_ok=True)
    for relative in (entry["path"] for entry in helper_inventory):
        destination = context / "helper" / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(HELPER / relative, destination)
    (context / "notices").mkdir(parents=True, exist_ok=True)
    for notice in NOTICE_FILES:
        shutil.copyfile(ROOT / notice, context / "notices" / notice)

    archives = []
    for (name, version), expected in sorted(closure.items()):
        source = cached_archive(cache_root, name, version, expected)
        members = validate_archive(source, name, version)
        shutil.copyfile(source, context / source.name)
        with tarfile.open(source, "r:gz") as archive:
            extracted = output / "license-scan"
            if extracted.exists():
                shutil.rmtree(extracted)
            archive.extractall(extracted, filter="data")
        license_expression = package_license(extracted / f"{name}-{version}")
        shutil.rmtree(extracted)
        archives.append(
            {
                "name": name,
                "version": version,
                "filename": source.name,
                "sha256": expected,
                "size": source.stat().st_size,
                "license": license_expression,
                "archive_member_count": members,
            }
        )

    (context / "dependency-map.tsv").write_text(
        "".join(
            f'{entry["filename"]}\t{entry["name"]}-{entry["version"]}\t{entry["sha256"]}\n'
            for entry in archives
        )
    )
    build_inputs = {
        "schema": "rust-engineering-mcp.m5-build-inputs.v1",
        "base_image_id": BASE_IMAGE_ID,
        "target": TARGET,
        "binaries": [
            {
                "name": BLOAT_NAME,
                "version": BLOAT_VERSION,
                "license": BLOAT_LICENSE,
                "origin": "crates.io published archive",
                "archive_sha256": BLOAT_SHA256,
            },
            {
                "name": HELPER_BINARY,
                "version": tomllib.loads((HELPER / "Cargo.toml").read_text())["package"]["version"],
                "license": "MIT OR Apache-2.0",
                "origin": "this repository",
                "source_files": helper_inventory,
            },
        ],
        "registry_packages": archives,
        "network_required": False,
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
        "schema": "rust-engineering-mcp.m5-prepare.v1",
        "status": "prepared_not_built",
        "base_image_id": BASE_IMAGE_ID,
        "target": TARGET,
        "context_file_count": len(allowed),
        "context_sha256s_digest": hashlib.sha256(sums.encode()).hexdigest(),
        "registry_package_count": len(archives),
        "network_used": False,
    }
    (output / "prepare-receipt.json").write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    return receipt


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--cargo-cache", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    arguments = parser.parse_args()
    print(json.dumps(prepare(arguments.cargo_cache, arguments.output), sort_keys=True))


if __name__ == "__main__":
    main()
