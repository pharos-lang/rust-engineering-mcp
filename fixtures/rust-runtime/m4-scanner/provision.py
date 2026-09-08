#!/usr/bin/env python3
"""Prepare the closed, offline build context for the ADR-069 helper image."""
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
HELPER = ROOT / "fixtures/unsafe-scanner-helper"
BASE_IMAGE_ID = "sha256:95dddeb5305f10b09b441e3cc4018ebb1a8a296d365c65106327d59f933c64e7"
TARGET = "aarch64-unknown-linux-gnu"
SOURCE_DATE_EPOCH = 0

HELPER_FILES = (
    "Cargo.toml",
    "Cargo.lock",
    "src/lib.rs",
    "src/main.rs",
)
NOTICE_FILES = ("LICENSE", "LICENSE-APACHE", "LICENSE-MIT", "NOTICE")

# This is the exact registry closure in the helper's private Cargo.lock. The
# archive checksum is Cargo's package checksum, not a checksum invented here.
PACKAGES = {
    ("itoa", "1.0.18"): ("8f42a60cbdf9a97f5d2305f08a87dc4e09308d1276d28c869c684d7777685682", "MIT OR Apache-2.0"),
    ("memchr", "2.8.3"): ("cf8baf1c55e62ffcace7a9f06f4bd9cd3f0c4beb022d3b367256b91b87513d98", "Unlicense OR MIT"),
    ("proc-macro2", "1.0.107"): ("985e7ec9bb745e6ce6535b544d84d6cd6f7ad8bd711c398938ae983b91a766d9", "MIT OR Apache-2.0"),
    ("quote", "1.0.47"): ("1fbf4db142a473a8d80c26bbf18454ed458bf8d26c8219c331daecfdbd079001", "MIT OR Apache-2.0"),
    ("serde", "1.0.229"): ("4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba", "MIT OR Apache-2.0"),
    ("serde_core", "1.0.229"): ("67dca2c9c51e58a4791a4b1ed58308b39c64224d349a935ab5039aa360942a48", "MIT OR Apache-2.0"),
    ("serde_derive", "1.0.229"): ("e7a5d71263a5a7d47b41f6b3f06ba276f10cc18b0931f1799f710578e2309348", "MIT OR Apache-2.0"),
    ("serde_json", "1.0.151"): ("c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14", "MIT OR Apache-2.0"),
    ("syn", "3.0.4"): ("e6275cddf4610d1775e6d1fe9469b2e77d0f39fd98fb7450901b821e0c53649f", "MIT OR Apache-2.0"),
    ("unicode-ident", "1.0.24"): ("e6e4313cd5fcd3dad5cafa179702e2b244f760991f45397d14d4ebf38247da75", "(MIT OR Apache-2.0) AND Unicode-3.0"),
    ("zmij", "1.0.23"): ("29666d0abbfad1e3dc4dcf6144730dd3a3ab225bbbdac83319345b1b44ccfc1b", "MIT"),
}

SAFE_ARCHIVE_NAME = re.compile(r"[A-Za-z0-9_./+@(), =:-]+")


def sha256(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def regular(path: Path) -> None:
    if path.is_symlink() or not path.is_file():
        raise ValueError(f"expected unlinked regular file: {path}")


def validate_archive(path: Path, package_name: str, version: str) -> int:
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


def verify_helper() -> list[dict[str, object]]:
    for relative in HELPER_FILES:
        regular(HELPER / relative)

    manifest = tomllib.loads((HELPER / "Cargo.toml").read_text())
    if manifest.get("package", {}).get("name") != "rust-mcp-unsafe-scanner-helper":
        raise ValueError("unexpected helper package")
    binary = manifest.get("bin", [])
    if binary != [{"name": "rust-mcp-unsafe-helper", "path": "src/main.rs"}]:
        raise ValueError("unexpected helper binary contract")
    dependencies = manifest.get("dependencies", {})
    expected_syn = {
        "version": "=3.0.4",
        "default-features": False,
        "features": ["full", "parsing", "visit"],
    }
    expected_proc_macro2 = {
        "version": "=1.0.107",
        "default-features": False,
        "features": ["span-locations"],
    }
    if dependencies.get("syn") != expected_syn or dependencies.get("proc-macro2") != expected_proc_macro2:
        raise ValueError("helper parser dependency contract changed")
    if dependencies.get("serde") != {"version": "=1.0.229", "features": ["derive"]}:
        raise ValueError("helper serde dependency contract changed")
    if dependencies.get("serde_json") != "=1.0.151":
        raise ValueError("helper serde_json dependency contract changed")

    lock = tomllib.loads((HELPER / "Cargo.lock").read_text())
    if lock.get("version") != 4:
        raise ValueError("unexpected private Cargo.lock version")
    registry = {
        (package["name"], package["version"]): package.get("checksum")
        for package in lock.get("package", [])
        if str(package.get("source", "")).startswith("registry+")
    }
    expected = {key: value[0] for key, value in PACKAGES.items()}
    if registry != expected:
        raise ValueError("private Cargo.lock registry closure changed")

    return [
        {
            "path": relative,
            "size": (HELPER / relative).stat().st_size,
            "sha256": sha256(HELPER / relative),
        }
        for relative in HELPER_FILES
    ]


def cached_archive(cache_root: Path, name: str, version: str, expected_hash: str) -> Path:
    if cache_root.is_symlink() or not cache_root.is_dir():
        raise ValueError(f"invalid Cargo cache root: {cache_root}")
    filename = f"{name}-{version}.crate"
    candidates = sorted(cache_root.glob(f"*/{filename}"))
    if not candidates:
        raise ValueError(f"missing cached crate: {filename}")
    for candidate in candidates:
        regular(candidate)
        observed = sha256(candidate)
        if observed != expected_hash:
            raise ValueError(f"cached crate checksum mismatch: {filename}")
    return candidates[0]


def context_paths() -> set[str]:
    files = {
        "Dockerfile",
        "build.sh",
        "build-inputs.json",
        "dependency-map.tsv",
        "SHA256SUMS",
    }
    files.update(f"helper/{relative}" for relative in HELPER_FILES)
    files.update(f"notices/{name}" for name in NOTICE_FILES)
    files.update(f"{name}-{version}.crate" for name, version in PACKAGES)
    return files


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
        missing = sorted(allowed_files - observed)
        raise ValueError(f"incomplete build context: {missing}")


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
    source_inventory = verify_helper()
    for notice in NOTICE_FILES:
        regular(ROOT / notice)

    output.mkdir(parents=True, exist_ok=True)
    context = output / "build-context"
    context.mkdir(parents=True, exist_ok=True)
    allowed = context_paths()
    validate_context(context, allowed, complete=False)
    (context / "helper/src").mkdir(parents=True, exist_ok=True)
    (context / "notices").mkdir(parents=True, exist_ok=True)

    shutil.copyfile(HERE / "Dockerfile", context / "Dockerfile")
    shutil.copyfile(HERE / "build.sh", context / "build.sh")
    for relative in HELPER_FILES:
        destination = context / "helper" / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(HELPER / relative, destination)
    for notice in NOTICE_FILES:
        shutil.copyfile(ROOT / notice, context / "notices" / notice)

    archives = []
    for (name, version), (expected_hash, license_expression) in sorted(PACKAGES.items()):
        source = cached_archive(cache_root, name, version, expected_hash)
        member_count = validate_archive(source, name, version)
        filename = source.name
        shutil.copyfile(source, context / filename)
        archives.append(
            {
                "name": name,
                "version": version,
                "filename": filename,
                "sha256": expected_hash,
                "size": source.stat().st_size,
                "license": license_expression,
                "archive_member_count": member_count,
            }
        )

    dependency_map = "".join(
        f'{entry["filename"]}\t{entry["name"]}-{entry["version"]}\t{entry["sha256"]}\n'
        for entry in archives
    )
    (context / "dependency-map.tsv").write_text(dependency_map)
    build_inputs = {
        "schema": "rust-engineering-mcp.m4-scanner-build-inputs.v1",
        "base_image_id": BASE_IMAGE_ID,
        "target": TARGET,
        "helper": {
            "name": "rust-mcp-unsafe-scanner-helper",
            "version": "0.1.0",
            "license": "MIT OR Apache-2.0",
            "source_files": source_inventory,
        },
        "registry_packages": archives,
        "network_required": False,
    }
    (context / "build-inputs.json").write_text(
        json.dumps(build_inputs, indent=2, sort_keys=True) + "\n"
    )

    hashed_files = sorted(allowed - {"SHA256SUMS"})
    sums = "".join(f"{sha256(context / relative)}  {relative}\n" for relative in hashed_files)
    (context / "SHA256SUMS").write_text(sums)
    validate_context(context, allowed, complete=True)
    normalize_context(context)

    context_digest = hashlib.sha256(sums.encode()).hexdigest()
    receipt = {
        "schema": "rust-engineering-mcp.m4-scanner-prepare.v1",
        "status": "prepared_not_built",
        "base_image_id": BASE_IMAGE_ID,
        "target": TARGET,
        "context_file_count": len(allowed),
        "context_sha256s_digest": context_digest,
        "network_used": False,
    }
    (output / "prepare-receipt.json").write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    return receipt


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--cargo-cache", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    arguments = parser.parse_args()
    receipt = prepare(arguments.cargo_cache, arguments.output)
    print(json.dumps(receipt, sort_keys=True))


if __name__ == "__main__":
    main()
