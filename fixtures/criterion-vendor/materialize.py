#!/usr/bin/env python3
"""Materialize the pinned criterion `.crate` archives into a Cargo directory source.

The committed input is 52 crates.io `.crate` archives plus `INVENTORY.json`. This
script verifies each archive's sha256 against the checksum recorded in the
manifest *before* extracting anything, applies the same archive-safety rules as
`fixtures/rust-runtime/m4-scanner/provision.py`, extracts each package into
`<output>/<name>-<version>/`, and writes the `.cargo-checksum.json` that a Cargo
directory source requires.

It never touches the network and never needs to: every archive is committed
beside it.

    python3 -B fixtures/criterion-vendor/materialize.py
    python3 -B fixtures/criterion-vendor/materialize.py --verify-only
    python3 -B fixtures/criterion-vendor/materialize.py --output /tmp/vendor
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import sys
import tarfile
from pathlib import Path, PurePosixPath

HERE = Path(__file__).resolve().parent
DEFAULT_MANIFEST = HERE / "INVENTORY.json"
DEFAULT_OUTPUT = HERE / "vendor"
SCHEMA = "rust-engineering-mcp.criterion-vendor.v1"

# Same allowlist as m4-scanner/provision.py. All 5962 members of the 52 pinned
# archives match it; a name that does not is a reason to stop, not to widen it.
SAFE_ARCHIVE_NAME = re.compile(r"[A-Za-z0-9_./+@(), =:-]+")

FILE_MODE = 0o644
DIRECTORY_MODE = 0o755


def sha256(path: Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def regular(path: Path) -> None:
    if path.is_symlink() or not path.is_file():
        raise ValueError(f"expected unlinked regular file: {path}")


def validate_archive(path: Path, package_name: str, version: str) -> int:
    """Reject anything an archive has no business containing.

    No absolute paths, no `..`, no backslashes, no name outside the allowlist,
    no member outside the expected `<name>-<version>/` root, no duplicate
    members, and no symlinks, hardlinks, devices or FIFOs.
    """
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


def load_manifest(manifest_path: Path) -> dict[str, object]:
    regular(manifest_path)
    manifest = json.loads(manifest_path.read_text())
    if manifest.get("schema") != SCHEMA:
        raise ValueError(f"unexpected manifest schema: {manifest.get('schema')!r}")
    packages = manifest.get("packages")
    if not isinstance(packages, list) or not packages:
        raise ValueError("manifest declares no packages")
    return manifest


def archive_path(root: Path, name: str, version: str) -> Path:
    path = root / f"{name}-{version}.crate"
    if not path.exists():
        raise ValueError(f"missing pinned archive: {path.name}")
    regular(path)
    return path


def verify(manifest: dict[str, object], root: Path) -> list[dict[str, object]]:
    """Verify every archive before anything is written anywhere."""
    verified: list[dict[str, object]] = []
    for package in manifest["packages"]:  # type: ignore[index]
        name = package["name"]
        version = package["version"]
        expected = package["archive_sha256"]
        if package["sha256"] != expected:
            raise ValueError(
                f"{name}-{version}: manifest sha256 and archive_sha256 disagree"
            )
        path = archive_path(root, name, version)
        observed = sha256(path)
        if observed != expected:
            raise ValueError(
                f"{name}-{version}: archive checksum mismatch "
                f"(expected {expected}, got {observed})"
            )
        size = path.stat().st_size
        if size != package["archive_bytes"]:
            raise ValueError(
                f"{name}-{version}: archive size mismatch "
                f"(expected {package['archive_bytes']}, got {size})"
            )
        validate_archive(path, name, version)
        verified.append({"package": package, "path": path})
    return verified


def extract(path: Path, name: str, version: str, destination: Path) -> None:
    """Write every member explicitly. No extractall, no inherited modes."""
    prefix = f"{name}-{version}/"
    with tarfile.open(path, "r:gz") as archive:
        for member in archive.getmembers():
            relative = member.name[len(prefix) :]
            target = destination / relative if relative else destination
            if member.isdir():
                target.mkdir(parents=True, exist_ok=True)
                target.chmod(DIRECTORY_MODE)
                continue
            target.parent.mkdir(parents=True, exist_ok=True)
            stream = archive.extractfile(member)
            if stream is None:
                raise ValueError(f"unreadable archive member: {member.name!r}")
            target.write_bytes(stream.read())
            target.chmod(FILE_MODE)


def write_cargo_checksum(destination: Path, package_checksum: str) -> tuple[int, int]:
    """Write the `.cargo-checksum.json` a Cargo directory source requires."""
    files: dict[str, str] = {}
    total_bytes = 0
    for path in sorted(destination.rglob("*")):
        if not path.is_file() or path.is_symlink():
            continue
        relative = path.relative_to(destination).as_posix()
        if relative == ".cargo-checksum.json":
            continue
        files[relative] = sha256(path)
        total_bytes += path.stat().st_size
    payload = {"files": {key: files[key] for key in sorted(files)}, "package": package_checksum}
    checksum_path = destination / ".cargo-checksum.json"
    checksum_path.write_text(json.dumps(payload, separators=(",", ":"), sort_keys=True) + "\n")
    checksum_path.chmod(FILE_MODE)
    return len(files), total_bytes


def materialize(manifest_path: Path, output: Path) -> dict[str, object]:
    manifest = load_manifest(manifest_path)
    root = manifest_path.resolve().parent
    verified = verify(manifest, root)

    if output.is_symlink():
        raise ValueError(f"output path is a symlink: {output}")
    if output.exists():
        shutil.rmtree(output)
    output.mkdir(parents=True)
    output.chmod(DIRECTORY_MODE)

    packages: list[dict[str, object]] = []
    for entry in verified:
        package = entry["package"]
        name = package["name"]
        version = package["version"]
        destination = output / f"{name}-{version}"
        destination.mkdir()
        destination.chmod(DIRECTORY_MODE)
        extract(entry["path"], name, version, destination)
        count, total_bytes = write_cargo_checksum(destination, package["sha256"])
        if count != package["files"] or total_bytes != package["bytes"]:
            raise ValueError(
                f"{name}-{version}: materialized tree disagrees with the manifest "
                f"(files {count} vs {package['files']}, "
                f"bytes {total_bytes} vs {package['bytes']})"
            )
        packages.append({"name": name, "version": version, "files": count, "bytes": total_bytes})

    return {
        "schema": "rust-engineering-mcp.criterion-vendor-materialize.v1",
        "status": "materialized",
        "output": str(output),
        "packages": len(packages),
        "files": sum(int(p["files"]) for p in packages),
        "bytes": sum(int(p["bytes"]) for p in packages),
        "network_used": False,
    }


def verify_only(manifest_path: Path) -> dict[str, object]:
    manifest = load_manifest(manifest_path)
    root = manifest_path.resolve().parent
    verified = verify(manifest, root)
    return {
        "schema": "rust-engineering-mcp.criterion-vendor-materialize.v1",
        "status": "verified",
        "packages": len(verified),
        "archive_bytes": sum(int(e["path"].stat().st_size) for e in verified),
        "network_used": False,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument(
        "--verify-only",
        action="store_true",
        help="verify every archive's checksum and safety without writing anything",
    )
    arguments = parser.parse_args()
    try:
        if arguments.verify_only:
            receipt = verify_only(arguments.manifest)
        else:
            receipt = materialize(arguments.manifest, arguments.output)
    except ValueError as error:
        print(f"materialize.py: {error}", file=sys.stderr)
        return 1
    print(json.dumps(receipt, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
