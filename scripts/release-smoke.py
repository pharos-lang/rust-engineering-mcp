#!/usr/bin/env python3
"""Fail-closed offline installation and MCP smoke for the core 0.1.0 archive."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import platform
import re
import selectors
import shutil
import signal
import stat
import struct
import subprocess
import tarfile
import tempfile
import time


SUPPORTED_TARGET = "aarch64-apple-darwin"
PROTOCOL_VERSION = "2026-07-28"
TAG = re.compile(r"^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$")
HEX_SHA256 = re.compile(r"^[0-9a-f]{64}$")
TOOLS = (
    "rust.project.open",
    "rust.project.inspect",
    "rust.toolchain.inspect",
    "rust.check",
    "rust.fmt.check",
    "rust.clippy",
    "rust.test",
    "rust.dependencies.audit",
    "rust.diagnostics.explain",
    "rust.quality.gate",
    "rust.catalog.status",
    "rust.crate.search",
    "rust.crate.inspect",
)
TOOL_SCHEMA_SHA256 = {
    "rust.project.open": "e7b454f9d9f026bf9cedf3bb999ff0bd931f8b7472e913638dcd960a142bec17",
    "rust.project.inspect": "1d5d9360356ae0274b2bd108b77883cf6d7a8571cc880785a1e3e9885be4eaec",
    "rust.toolchain.inspect": "ce087099b93a2273648f7c55ed11a8a6c80909d6390f77f805c1b1411cd0d63d",
    "rust.check": "8332a86f7973a77ce2bb52bd4d9a10f82f4172366d0c93fa6c8278c5e260522d",
    "rust.fmt.check": "d820af9a35d5cc936363b6fd37813ab1821a98d582b14cd47477b56b6d487f90",
    "rust.clippy": "8b2aa1245ab48dcaeb208a5e3bda1bee2dd28686b8977b3f2e352173d0b96699",
    "rust.test": "8ff987c184896ee85e84636660a1d0cff28dc0cc434859e9a04c49bf0d1fb688",
    "rust.dependencies.audit": "307a7c7d0b6da4a84a5ff8b2bc33981234f9b470204ce6915d45eea5492b67f9",
    "rust.diagnostics.explain": "ea89376e81c525e1c058ef84f2e65f61d80d136694a2061987985a17f05eb5f2",
    "rust.quality.gate": "0aa0689b5a571706afc1655029872db8b56f5af511ae692e69f075b9a3b43da4",
    "rust.catalog.status": "476ef687538f375c1cdec0a905a3b47a1d0aea9389d12a7c1599c6d77cc19754",
    "rust.crate.search": "b2d5b33fbc204d3a78c02044eb967936cf3d5db74d7d5b85e394b6511d7a4cfa",
    "rust.crate.inspect": "7793f5ddc99755fd6b756689c555611ca14bb022653b545b288dde092a7e28c1",
}
PROHIBITED_PACKAGES = {"fastembed", "kanaria", "lance", "lancedb", "ort", "ort-sys"}
PROHIBITED_ASSET_TOKENS = {
    "catalog",
    "docker",
    "dockerfile",
    "fixture",
    "kanaria",
    "lance",
    "model",
    "onnx",
    "ort",
    "rust-toolchain",
    "toolchain",
    "trust",
}
MAX_ARCHIVE_BYTES = 600 * 1024 * 1024
MAX_MEMBER_BYTES = 512 * 1024 * 1024
MAX_TOTAL_BYTES = 700 * 1024 * 1024
MAX_JSON_BYTES = 16 * 1024 * 1024
MAX_PROTOCOL_FRAME = 1024 * 1024
MAX_PROTOCOL_TOTAL = 4 * 1024 * 1024
PROCESS_TIMEOUT = 10.0
EXPECTED_SELF_ROW = {
    "path": "MANIFEST.json",
    "bytes": None,
    "sha256": None,
    "mode": "0644",
    "self_reference": "size-and-hash-not-representable-inside-member",
}
MACHO_ARM64_CPU = 0x0100000C
MACHO_EXECUTE = 2


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def canonical_json(value: object) -> bytes:
    return (json.dumps(value, indent=2, sort_keys=True, ensure_ascii=False) + "\n").encode()


def strict_json(data: bytes | str, label: str) -> object:
    def unique_object(pairs):
        result = {}
        for key, value in pairs:
            if key in result:
                raise ValueError(f"duplicate JSON key in {label}: {key}")
            result[key] = value
        return result

    try:
        return json.loads(data, object_pairs_hook=unique_object)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ValueError(f"invalid JSON: {label}") from error


def validate_macho_arm64(data: bytes) -> None:
    if len(data) < 16:
        raise ValueError("release binary is too small to be Mach-O arm64")
    if data[:4] == b"\xcf\xfa\xed\xfe":
        cpu = struct.unpack_from("<I", data, 4)[0]
        filetype = struct.unpack_from("<I", data, 12)[0]
    elif data[:4] == b"\xfe\xed\xfa\xcf":
        cpu = struct.unpack_from(">I", data, 4)[0]
        filetype = struct.unpack_from(">I", data, 12)[0]
    else:
        raise ValueError("release binary is not a thin 64-bit Mach-O")
    if cpu != MACHO_ARM64_CPU:
        raise ValueError("release binary Mach-O CPU is not arm64")
    if filetype != MACHO_EXECUTE:
        raise ValueError("release binary Mach-O file type is not MH_EXECUTE")


def validate_host() -> None:
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        raise ValueError("core release smoke requires the qualified Darwin arm64 host")


def safe_relative(value: str) -> str:
    path = PurePosixPath(value)
    if not value or path.is_absolute() or "." in path.parts or ".." in path.parts:
        raise ValueError(f"unsafe archive path: {value!r}")
    if path.as_posix() != value or "\\" in value or "\x00" in value:
        raise ValueError(f"non-canonical archive path: {value!r}")
    return value


def open_regular(path: Path, limit: int) -> tuple[int, os.stat_result]:
    flags = os.O_RDONLY
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        descriptor = os.open(path, flags)  # NOSONAR -- no-follow open followed by regular-file and bounded-size validation.
    except OSError as error:
        raise ValueError(f"cannot open regular no-follow file {path}: {error}") from error
    observed = os.fstat(descriptor)
    if not stat.S_ISREG(observed.st_mode):
        os.close(descriptor)
        raise ValueError(f"file must be regular and no-follow: {path}")
    if observed.st_size > limit:
        os.close(descriptor)
        raise ValueError(f"file exceeds {limit} bytes: {path}")
    return descriptor, observed


def fingerprint(observed: os.stat_result) -> tuple[int, int, int, int]:
    return observed.st_dev, observed.st_ino, observed.st_size, observed.st_mtime_ns


def read_regular(path: Path, limit: int) -> tuple[bytes, tuple[int, int, int, int]]:
    descriptor, before = open_regular(path, limit)
    try:
        chunks = []
        remaining = before.st_size
        while remaining:
            chunk = os.read(descriptor, min(1024 * 1024, remaining))
            if not chunk:
                raise ValueError(f"file changed while reading: {path}")
            chunks.append(chunk)
            remaining -= len(chunk)
        if os.read(descriptor, 1):
            raise ValueError(f"file grew while reading: {path}")
        after = os.fstat(descriptor)
        if fingerprint(before) != fingerprint(after):
            raise ValueError(f"file changed while reading: {path}")
        return b"".join(chunks), fingerprint(after)
    finally:
        os.close(descriptor)


def hash_regular(path: Path, limit: int) -> tuple[str, int, tuple[int, int, int, int]]:
    descriptor, before = open_regular(path, limit)
    digest = hashlib.sha256()
    try:
        remaining = before.st_size
        while remaining:
            chunk = os.read(descriptor, min(1024 * 1024, remaining))
            if not chunk:
                raise ValueError(f"file changed while hashing: {path}")
            digest.update(chunk)
            remaining -= len(chunk)
        if os.read(descriptor, 1):
            raise ValueError(f"file grew while hashing: {path}")
        after = os.fstat(descriptor)
        if fingerprint(before) != fingerprint(after):
            raise ValueError(f"file changed while hashing: {path}")
        return digest.hexdigest(), before.st_size, fingerprint(after)
    finally:
        os.close(descriptor)


def parse_sums(data: bytes, archive_name: str) -> str:
    try:
        text = data.decode("ascii")
    except UnicodeDecodeError as error:
        raise ValueError("SHA256SUMS must be ASCII") from error
    match = re.fullmatch(r"([0-9a-f]{64})  ([^/\r\n]+)\n", text)
    if match is None or match.group(2) != archive_name:
        raise ValueError("SHA256SUMS must contain exactly the expected archive name")
    return match.group(1)


def load_archive(
    archive: Path, expected_name: str, expected_fingerprint: tuple[int, int, int, int]
) -> tuple[dict[str, bytes], dict[str, int]]:
    descriptor, observed = open_regular(archive, MAX_ARCHIVE_BYTES)
    if fingerprint(observed) != expected_fingerprint:
        os.close(descriptor)
        raise ValueError("archive changed between checksum and verification")
    prefix = expected_name.removesuffix(".tar.gz")
    contents: dict[str, bytes] = {}
    modes: dict[str, int] = {}
    total = 0
    try:
        with os.fdopen(descriptor, "rb", closefd=True) as source:
            descriptor = -1
            with tarfile.open(fileobj=source, mode="r:gz") as bundle:
                for item in bundle:
                    full = safe_relative(item.name)
                    expected_prefix = prefix + "/"
                    if not full.startswith(expected_prefix):
                        raise ValueError(f"archive member outside expected prefix: {full}")
                    logical = safe_relative(full[len(expected_prefix) :])
                    if len(PurePosixPath(logical).parts) != 1:
                        raise ValueError(f"nested archive member is not installable: {logical}")
                    if logical in contents:
                        raise ValueError(f"duplicate archive member: {logical}")
                    if not item.isfile() or item.issym() or item.islnk():
                        raise ValueError(f"archive contains non-regular member: {logical}")
                    if item.uid != 0 or item.gid != 0 or item.mtime != 0:
                        raise ValueError(f"archive metadata is not deterministic: {logical}")
                    if item.uname or item.gname:
                        raise ValueError(f"archive owner names must be empty: {logical}")
                    if item.size < 0 or item.size > MAX_MEMBER_BYTES:
                        raise ValueError(f"archive member has invalid size: {logical}")
                    total += item.size
                    if total > MAX_TOTAL_BYTES:
                        raise ValueError("archive expanded content exceeds limit")
                    stream = bundle.extractfile(item)
                    if stream is None:
                        raise ValueError(f"cannot read archive member: {logical}")
                    data = stream.read(item.size + 1)
                    if len(data) != item.size:
                        raise ValueError(f"archive member size mismatch: {logical}")
                    contents[logical] = data
                    modes[logical] = stat.S_IMODE(item.mode)
            if fingerprint(os.fstat(source.fileno())) != expected_fingerprint:
                raise ValueError("archive changed while its members were verified")
    finally:
        if descriptor >= 0:
            os.close(descriptor)
    if not contents:
        raise ValueError("archive contains no members")
    return contents, modes


def json_member(contents: dict[str, bytes], name: str) -> dict[str, object]:
    data = contents.get(name)
    if data is None:
        raise ValueError(f"required member is absent: {name}")
    if len(data) > MAX_JSON_BYTES:
        raise ValueError(f"JSON member exceeds limit: {name}")
    value = strict_json(data, name)
    if not isinstance(value, dict):
        raise ValueError(f"JSON member must be an object: {name}")
    return value


def validate_manifest(contents: dict[str, bytes], modes: dict[str, int]) -> dict[str, object]:
    manifest = json_member(contents, "MANIFEST.json")
    if manifest.get("schema") != "rust-engineering-mcp-release-manifest-v1":
        raise ValueError("manifest schema mismatch")
    if manifest.get("hash_algorithm") != "SHA-256":
        raise ValueError("manifest hash algorithm mismatch")
    rows = manifest.get("members")
    if not isinstance(rows, list):
        raise ValueError("manifest members are absent")
    indexed: dict[str, dict[str, object]] = {}
    for row in rows:
        if not isinstance(row, dict) or not isinstance(row.get("path"), str):
            raise ValueError("manifest contains an invalid member row")
        path = safe_relative(row["path"])
        if path in indexed:
            raise ValueError(f"manifest contains duplicate member: {path}")
        indexed[path] = row
    if set(indexed) != set(contents):
        raise ValueError("manifest member set does not match archive")
    if indexed.get("MANIFEST.json") != EXPECTED_SELF_ROW:
        raise ValueError("manifest self-row mismatch")
    for path, data in contents.items():
        if path == "MANIFEST.json":
            continue
        expected = {
            "path": path,
            "bytes": len(data),
            "sha256": sha256(data),
            "mode": format(modes[path], "04o"),
        }
        if indexed[path] != expected:
            raise ValueError(f"manifest row or hash mismatch: {path}")
    if modes.get("MANIFEST.json") != 0o644:
        raise ValueError("manifest archive mode mismatch")
    return manifest


def validate_inventory(
    contents: dict[str, bytes], modes: dict[str, int], tag: str, target: str
) -> tuple[dict[str, object], str]:
    inventory = json_member(contents, "inventory.json")
    if inventory.get("schema") != "rust-engineering-mcp-core-inventory-v1":
        raise ValueError("inventory schema mismatch")
    artifact = inventory.get("artifact")
    if not isinstance(artifact, dict):
        raise ValueError("inventory artifact is absent")
    version = tag[1:]
    expected_artifact = {
        "tag": tag,
        "version": version,
        "target": target,
        "profile": "core-default",
    }
    for key, value in expected_artifact.items():
        if artifact.get(key) != value:
            raise ValueError(f"inventory artifact {key} mismatch")
    binary = artifact.get("binary")
    if not isinstance(binary, dict) or not isinstance(binary.get("name"), str):
        raise ValueError("inventory binary record is absent")
    binary_name = safe_relative(binary["name"])
    if len(PurePosixPath(binary_name).parts) != 1 or binary_name not in contents:
        raise ValueError("inventory binary member mismatch")
    binary_data = contents[binary_name]
    validate_macho_arm64(binary_data)
    if binary != {
        "name": binary_name,
        "bytes": len(binary_data),
        "sha256": sha256(binary_data),
        "mode": "0755",
    }:
        raise ValueError("inventory binary hash or metadata mismatch")
    if modes.get(binary_name) != 0o755:
        raise ValueError("binary archive mode mismatch")
    for name, mode in modes.items():
        expected_mode = 0o755 if name == binary_name else 0o644
        if mode != expected_mode:
            raise ValueError(f"archive member mode is not fixed: {name}")
    resolution = inventory.get("resolution")
    if not isinstance(resolution, dict):
        raise ValueError("inventory resolution is absent")
    if resolution.get("edge_kinds") != ["normal", "build"]:
        raise ValueError("inventory dependency roles mismatch")
    if resolution.get("dev_dependencies_included") is not False:
        raise ValueError("inventory includes development dependencies")
    command = resolution.get("command")
    if not isinstance(command, list) or "--locked" not in command or "--offline" not in command:
        raise ValueError("inventory metadata command is not locked and offline")
    if "--filter-platform" not in command or target not in command:
        raise ValueError("inventory metadata command target mismatch")
    packages = inventory.get("packages")
    if not isinstance(packages, list) or not packages:
        raise ValueError("inventory package closure is empty")
    if resolution.get("package_count") != len(packages):
        raise ValueError("inventory package count mismatch")
    ids = set()
    root_rows = []
    for package in packages:
        if not isinstance(package, dict):
            raise ValueError("invalid inventory package row")
        identity = package.get("id")
        name = package.get("name")
        if not isinstance(identity, str) or identity in ids or not isinstance(name, str):
            raise ValueError("invalid or duplicate inventory package identity")
        ids.add(identity)
        lowered = name.lower()
        if lowered in PROHIBITED_PACKAGES or lowered.startswith(
            ("fastembed-", "kanaria-", "lance-", "lancedb-", "ort-")
        ):
            raise ValueError(f"prohibited package entered core inventory: {name}")
        features = package.get("enabled_features")
        roles = package.get("roles")
        texts = package.get("texts")
        if not isinstance(features, list) or "local" in features:
            raise ValueError(f"local feature entered core inventory: {name}")
        if not isinstance(roles, list) or not roles:
            raise ValueError(f"package roles are absent: {name}")
        if not package.get("declared_license") or not isinstance(texts, list):
            raise ValueError(f"package license evidence is absent: {name}")
        if not any(isinstance(row, dict) and row.get("kind") == "license_or_copying" for row in texts):
            raise ValueError(f"package license text is absent: {name}")
        for row in texts:
            if not isinstance(row, dict) or not HEX_SHA256.fullmatch(str(row.get("sha256", ""))):
                raise ValueError(f"package text hash is invalid: {name}")
        if name == "rust-engineering-mcp" and "root" in roles:
            root_rows.append(package)
    if len(root_rows) != 1 or root_rows[0].get("version") != version:
        raise ValueError("inventory root package mismatch")
    edges = inventory.get("edges")
    if not isinstance(edges, list):
        raise ValueError("inventory edges are absent")
    seen_edges = set()
    for edge in edges:
        if not isinstance(edge, dict):
            raise ValueError("invalid inventory edge")
        key = edge.get("from"), edge.get("to"), edge.get("role")
        if key in seen_edges or key[0] not in ids or key[1] not in ids:
            raise ValueError("invalid or duplicate inventory edge")
        if key[2] not in {"normal", "build"}:
            raise ValueError("invalid inventory dependency role")
        seen_edges.add(key)
    return inventory, binary_name


def validate_spdx(contents: dict[str, bytes], inventory: dict[str, object]) -> dict[str, object]:
    document = json_member(contents, "sbom.spdx.json")
    if document.get("spdxVersion") != "SPDX-2.3" or document.get("dataLicense") != "CC0-1.0":
        raise ValueError("SPDX document header mismatch")
    if document.get("SPDXID") != "SPDXRef-DOCUMENT":
        raise ValueError("SPDX document identifier mismatch")
    expected_namespace = (
        "https://github.com/pharos-lang/rust-engineering-mcp/sbom/"
        + sha256(canonical_json(inventory))
    )
    if document.get("documentNamespace") != expected_namespace:
        raise ValueError("SPDX document namespace mismatch")
    packages = document.get("packages")
    inventory_packages = inventory["packages"]
    if not isinstance(packages, list) or len(packages) != len(inventory_packages):
        raise ValueError("SPDX package count mismatch")
    package_ids: dict[str, str] = {}
    root_spdx = None
    for package, source in zip(packages, inventory_packages, strict=True):
        if not isinstance(package, dict) or not isinstance(source, dict):
            raise ValueError("SPDX package row mismatch")
        spdx_id = package.get("SPDXID")
        if not isinstance(spdx_id, str) or spdx_id in package_ids.values():
            raise ValueError("SPDX package identifier is invalid or duplicate")
        if (
            package.get("name") != source.get("name")
            or package.get("versionInfo") != source.get("version")
            or package.get("licenseDeclared") != source.get("declared_license")
            or package.get("filesAnalyzed") is not False
        ):
            raise ValueError("SPDX package does not match inventory")
        lock_checksum = source.get("lock_checksum")
        expected_checksums = (
            [{"algorithm": "SHA256", "checksumValue": lock_checksum}]
            if lock_checksum
            else None
        )
        if package.get("checksums") != expected_checksums:
            raise ValueError("SPDX package checksum does not match Cargo.lock inventory")
        package_ids[source["id"]] = spdx_id
        if source.get("name") == "rust-engineering-mcp" and "root" in source.get("roles", []):
            root_spdx = spdx_id
    if root_spdx is None:
        raise ValueError("SPDX root package is absent")
    expected = {
        ("SPDXRef-DOCUMENT", "DESCRIBES", root_spdx),
        *{
            (package_ids[edge["from"]], "DEPENDS_ON", package_ids[edge["to"]])
            for edge in inventory["edges"]
        },
    }
    relationships = document.get("relationships")
    if not isinstance(relationships, list):
        raise ValueError("SPDX relationships are absent")
    observed = []
    for row in relationships:
        if not isinstance(row, dict):
            raise ValueError("invalid SPDX relationship")
        observed.append(
            (row.get("spdxElementId"), row.get("relationshipType"), row.get("relatedSpdxElement"))
        )
    if len(observed) != len(set(observed)) or set(observed) != expected:
        raise ValueError("SPDX relationships do not match inventory graph")
    return document


def validate_documents(contents: dict[str, bytes], inventory: dict[str, object]) -> None:
    for required in ("README.md", "SECURITY.md", "NOTICE", "THIRD_PARTY_NOTICES.txt"):
        if not contents.get(required):
            raise ValueError(f"required release document is absent or empty: {required}")
    licenses = {name: data for name, data in contents.items() if name.startswith("LICENSE")}
    if not licenses or any(not data for data in licenses.values()):
        raise ValueError("top-level LICENSE* evidence is absent or empty")
    notice = contents["THIRD_PARTY_NOTICES.txt"]
    if not notice.startswith(b"Rust Engineering MCP third-party notices\n"):
        raise ValueError("third-party notices header mismatch")
    for package in inventory["packages"]:
        if package.get("workspace_member"):
            continue
        for text in package["texts"]:
            if text["sha256"].encode("ascii") not in notice:
                raise ValueError(f"third-party notice omits text hash for {package['name']}")
    root = next(
        package
        for package in inventory["packages"]
        if package.get("name") == "rust-engineering-mcp" and "root" in package.get("roles", [])
    )
    root_hashes = {row["sha256"] for row in root["texts"]}
    if not {sha256(data) for data in licenses.values()} <= root_hashes:
        raise ValueError("product LICENSE* hashes are absent from inventory")
    for name in contents:
        lower = name.lower()
        tokens = set(filter(None, re.split(r"[^a-z0-9]+", lower)))
        if tokens & PROHIBITED_ASSET_TOKENS or "onnxruntime" in tokens:
            raise ValueError(f"prohibited release asset: {name}")


def validate_archive(
    archive: Path, sums: Path, tag: str, target: str
) -> tuple[dict[str, bytes], dict[str, object]]:
    if target != SUPPORTED_TARGET:
        raise ValueError(f"unsupported release target: {target}")
    if TAG.fullmatch(tag) is None:
        raise ValueError("tag must be a stable semantic version of the form vX.Y.Z")
    expected_name = f"rust-engineering-mcp-{tag}-{target}.tar.gz"
    if archive.name != expected_name:
        raise ValueError(f"archive name must be exactly {expected_name}")
    if sums.name != "SHA256SUMS":
        raise ValueError("checksum file must be named SHA256SUMS")
    sum_data, _ = read_regular(sums, MAX_JSON_BYTES)
    expected_hash = parse_sums(sum_data, expected_name)
    actual_hash, archive_bytes, archive_fingerprint = hash_regular(archive, MAX_ARCHIVE_BYTES)
    if actual_hash != expected_hash:
        raise ValueError("archive checksum does not match SHA256SUMS")
    contents, modes = load_archive(archive, expected_name, archive_fingerprint)
    validate_manifest(contents, modes)
    inventory, binary_name = validate_inventory(contents, modes, tag, target)
    spdx = validate_spdx(contents, inventory)
    validate_documents(contents, inventory)
    evidence = {
        "archive_name": expected_name,
        "archive_bytes": archive_bytes,
        "archive_sha256": actual_hash,
        "sha256sums_sha256": sha256(sum_data),
        "members": len(contents),
        "packages": len(inventory["packages"]),
        "relationships": len(spdx["relationships"]),
        "binary_name": binary_name,
        "binary_sha256": sha256(contents[binary_name]),
    }
    return contents, evidence


def install_members(directory: Path, contents: dict[str, bytes], binary_name: str) -> Path:
    directory.chmod(0o700)
    descriptor = os.open(directory, os.O_RDONLY | os.O_DIRECTORY)
    try:
        for name in sorted(contents):
            safe_relative(name)
            if len(PurePosixPath(name).parts) != 1:
                raise ValueError(f"refusing nested installation member: {name}")
            flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
            if hasattr(os, "O_NOFOLLOW"):
                flags |= os.O_NOFOLLOW
            mode = 0o700 if name == binary_name else 0o600
            output = os.open(name, flags, mode, dir_fd=descriptor)
            try:
                data = contents[name]
                written = 0
                while written < len(data):
                    count = os.write(output, data[written:])
                    if count <= 0:
                        raise ValueError(f"short installation write: {name}")
                    written += count
                os.fsync(output)
                observed = os.fstat(output)
                if not stat.S_ISREG(observed.st_mode) or stat.S_IMODE(observed.st_mode) != mode:
                    raise ValueError(f"installed member metadata mismatch: {name}")
            finally:
                os.close(output)
    finally:
        os.close(descriptor)
    binary = directory / binary_name
    installed_binary, _ = read_regular(binary, MAX_MEMBER_BYTES)
    if sha256(installed_binary) != sha256(contents[binary_name]):
        raise ValueError("installed binary hash mismatch")
    return binary


def clean_env() -> dict[str, str]:
    return {"LANG": "C", "LC_ALL": "C", "TZ": "UTC"}


def run_command(binary: Path, arguments: list[str]) -> tuple[bytes, dict[str, object]]:
    started = time.monotonic()
    process = subprocess.Popen(
        [str(binary), *arguments],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=clean_env(),
        start_new_session=True,
    )
    try:
        stdout, stderr = process.communicate(timeout=PROCESS_TIMEOUT)
    except subprocess.TimeoutExpired as error:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.communicate()
        raise ValueError(f"command timed out: {' '.join(arguments)}") from error
    duration = int((time.monotonic() - started) * 1000)
    process_group_clean = False
    try:
        os.killpg(process.pid, 0)
    except ProcessLookupError:
        process_group_clean = True
    except PermissionError:
        process_group_clean = False
    if not process_group_clean:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        raise ValueError(f"command left a process group alive: {' '.join(arguments)}")
    if process.returncode != 0 or stderr:
        raise ValueError(
            f"command failed or wrote stderr: {' '.join(arguments)} rc={process.returncode}"
        )
    return stdout, {
        "arguments": arguments,
        "exit_code": process.returncode,
        "stdout_sha256": sha256(stdout),
        "stderr_bytes": len(stderr),
        "duration_ms": duration,
        "process_group_clean": True,
    }


def validate_cli(binary: Path, version: str) -> tuple[list[dict[str, object]], dict[str, object]]:
    receipts = []
    human, receipt = run_command(binary, ["--version"])
    receipts.append(receipt)
    if human != f"rust-engineering-mcp {version}\n".encode():
        raise ValueError("--version output mismatch")
    raw_version, receipt = run_command(binary, ["version", "--json"])
    receipts.append(receipt)
    version_report = strict_json(raw_version, "version --json")
    expected_version = {
        "format_version": 1,
        "operation": "version",
        "package": "rust-engineering-mcp",
        "version": version,
        "compiled_local": False,
        "target_os": "macos",
        "target_arch": "aarch64",
    }
    if version_report != expected_version:
        raise ValueError("version --json build facts mismatch")
    raw_doctor, receipt = run_command(binary, ["doctor", "--json"])
    receipts.append(receipt)
    doctor = strict_json(raw_doctor, "doctor --json")
    validate_doctor(doctor)
    return receipts, doctor


def validate_doctor(report: object) -> None:
    if not isinstance(report, dict):
        raise ValueError("doctor report must be an object")
    for key, value in {
        "format_version": 1,
        "operation": "doctor",
        "mode": "passive",
        "status": "warning",
        "runtime": None,
    }.items():
        if report.get(key) != value:
            raise ValueError(f"passive doctor {key} mismatch")
    if not isinstance(report.get("duration_ms"), int) or report["duration_ms"] < 0:
        raise ValueError("passive doctor duration is invalid")
    catalog = report.get("catalog")
    if not isinstance(catalog, dict) or catalog.get("reservation") is not None:
        raise ValueError("passive doctor catalog state mismatch")
    for component in ("catalog", "model", "semantic_index", "rustsec"):
        if catalog.get(component) != {"status": "unavailable", "reason": "not_configured"}:
            raise ValueError(f"passive doctor {component} is not explicitly absent")
    checks = report.get("checks")
    if not isinstance(checks, list):
        raise ValueError("passive doctor checks are absent")
    indexed = {row.get("id"): row for row in checks if isinstance(row, dict)}
    if len(indexed) != len(checks):
        raise ValueError("passive doctor contains invalid or duplicate checks")
    for check in (
        "catalog",
        "model",
        "semantic_index",
        "rustsec",
        "filesystem_roots",
        "rustc",
        "cargo",
        "rustfmt",
        "clippy",
        "sandbox",
    ):
        if indexed.get(check, {}).get("status") != "not_configured":
            raise ValueError(f"passive doctor check is not unconfigured: {check}")
    for check in ("host_tools", "optional_tools"):
        if indexed.get(check, {}).get("status") != "not_checked":
            raise ValueError(f"passive doctor performed host check: {check}")
    if indexed.get("cargo_audit", {}).get("status") != "not_used":
        raise ValueError("passive doctor unexpectedly used cargo-audit")
    if indexed.get("audit_engine", {}).get("status") != "available":
        raise ValueError("passive doctor audit engine fact mismatch")


def modern_request(identifier: int, method: str, params: dict[str, object] | None = None) -> dict[str, object]:
    body = dict(params or {})
    body["_meta"] = {
        "io.modelcontextprotocol/protocolVersion": PROTOCOL_VERSION,
        "io.modelcontextprotocol/clientCapabilities": {},
    }
    return {"jsonrpc": "2.0", "id": identifier, "method": method, "params": body}


class Transport:
    def __init__(self, binary: Path):
        self.process = subprocess.Popen(
            [str(binary), "serve", "--stdio"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=clean_env(),
            start_new_session=True,
        )
        if self.process.stdin is None or self.process.stdout is None or self.process.stderr is None:
            self.abort()
            raise ValueError("failed to create MCP pipes")
        self.selector = selectors.DefaultSelector()
        self.selector.register(self.process.stdout, selectors.EVENT_READ, "stdout")
        self.selector.register(self.process.stderr, selectors.EVENT_READ, "stderr")
        self.stdout_buffer = bytearray()
        self.stderr_buffer = bytearray()
        self.total = 0
        self.calls: list[dict[str, object]] = []

    def abort(self) -> None:
        if self.process.poll() is None:
            try:
                os.killpg(self.process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            self.process.wait(timeout=PROCESS_TIMEOUT)

    def _read_until_frame(self) -> bytes:
        deadline = time.monotonic() + PROCESS_TIMEOUT
        while b"\n" not in self.stdout_buffer:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise ValueError("MCP response timed out")
            events = self.selector.select(remaining)
            if not events:
                raise ValueError("MCP response timed out")
            for key, _ in events:
                chunk = os.read(key.fileobj.fileno(), 65536)
                if not chunk:
                    self.selector.unregister(key.fileobj)
                    if key.data == "stdout":
                        raise ValueError("MCP stdout closed before response")
                    continue
                self.total += len(chunk)
                if self.total > MAX_PROTOCOL_TOTAL:
                    raise ValueError("MCP output exceeds session limit")
                if key.data == "stderr":
                    self.stderr_buffer.extend(chunk)
                    raise ValueError("MCP server wrote unexpected stderr")
                self.stdout_buffer.extend(chunk)
                if len(self.stdout_buffer) > MAX_PROTOCOL_FRAME and b"\n" not in self.stdout_buffer:
                    raise ValueError("MCP frame exceeds limit")
        frame, _, remainder = self.stdout_buffer.partition(b"\n")
        self.stdout_buffer = bytearray(remainder)
        if not frame or len(frame) > MAX_PROTOCOL_FRAME:
            raise ValueError("MCP frame is empty or oversized")
        return bytes(frame)

    def request(self, value: dict[str, object]) -> dict[str, object]:
        if self.process.stdin is None:
            raise ValueError("MCP stdin is already closed")
        encoded = json.dumps(value, separators=(",", ":"), ensure_ascii=False).encode() + b"\n"
        if len(encoded) > MAX_PROTOCOL_FRAME:
            raise ValueError("MCP request exceeds limit")
        started = time.monotonic()
        try:
            self.process.stdin.write(encoded)
            self.process.stdin.flush()
        except (BrokenPipeError, OSError) as error:
            raise ValueError("MCP stdin failed") from error
        frame = self._read_until_frame()
        response = strict_json(frame, "MCP response")
        if not isinstance(response, dict):
            raise ValueError("MCP response is not an object")
        if response.get("jsonrpc") != "2.0" or response.get("id") != value.get("id"):
            raise ValueError("MCP response envelope or id mismatch")
        if ("result" in response) == ("error" in response):
            raise ValueError("MCP response must contain exactly one result or error")
        self.calls.append(
            {
                "method": value.get("method"),
                "tool": (
                    value.get("params", {}).get("name")
                    if isinstance(value.get("params"), dict)
                    else None
                ),
                "request_sha256": sha256(encoded[:-1]),
                "response_sha256": sha256(frame),
                "duration_ms": int((time.monotonic() - started) * 1000),
                "jsonrpc_error_code": (
                    response.get("error", {}).get("code")
                    if isinstance(response.get("error"), dict)
                    else None
                ),
            }
        )
        return response

    def finish(self) -> dict[str, object]:
        if self.process.stdin is not None:
            self.process.stdin.close()
            self.process.stdin = None
        deadline = time.monotonic() + PROCESS_TIMEOUT
        while self.selector.get_map():
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise ValueError("MCP server did not reach EOF")
            events = self.selector.select(remaining)
            if not events:
                raise ValueError("MCP server did not reach EOF")
            for key, _ in events:
                chunk = os.read(key.fileobj.fileno(), 65536)
                if chunk:
                    self.total += len(chunk)
                    if key.data == "stderr":
                        self.stderr_buffer.extend(chunk)
                    else:
                        self.stdout_buffer.extend(chunk)
                else:
                    self.selector.unregister(key.fileobj)
        try:
            code = self.process.wait(timeout=max(0.1, deadline - time.monotonic()))
        except subprocess.TimeoutExpired as error:
            raise ValueError("MCP server did not exit after EOF") from error
        if code != 0 or self.stderr_buffer or self.stdout_buffer:
            raise ValueError("MCP server exit, stderr, or trailing stdout mismatch")
        process_group_clean = False
        try:
            os.killpg(self.process.pid, 0)
        except ProcessLookupError:
            process_group_clean = True
        except PermissionError:
            process_group_clean = False
        if not process_group_clean:
            try:
                os.killpg(self.process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            raise ValueError("MCP process group remains alive")
        return {
            "exit_code": code,
            "stderr_bytes": len(self.stderr_buffer),
            "eof_observed": True,
            "process_group_clean": True,
        }


def validate_discovery(response: object, version: str) -> None:
    if not isinstance(response, dict) or not isinstance(response.get("result"), dict):
        raise ValueError("modern discovery result is absent")
    result = response["result"]
    if result.get("resultType") != "complete":
        raise ValueError("modern discovery is incomplete")
    versions = result.get("supportedVersions")
    if not isinstance(versions, list) or PROTOCOL_VERSION not in versions:
        raise ValueError("modern protocol version is not advertised")
    if result.get("capabilities") != {"tools": {}, "resources": {}}:
        raise ValueError("modern discovery capabilities mismatch")
    meta = result.get("_meta")
    info = meta.get("io.modelcontextprotocol/serverInfo") if isinstance(meta, dict) else None
    if info != {"name": "rust-engineering-mcp", "version": version}:
        raise ValueError("modern discovery server identity mismatch")


def validate_tools(response: object) -> list[dict[str, object]]:
    if not isinstance(response, dict) or not isinstance(response.get("result"), dict):
        raise ValueError("tools/list result is absent")
    result = response["result"]
    definitions = result.get("tools")
    if result.get("resultType") != "complete" or result.get("nextCursor") is not None:
        raise ValueError("tools/list must be complete and unpaginated")
    if not isinstance(definitions, list) or tuple(row.get("name") for row in definitions) != TOOLS:
        raise ValueError("tools/list does not contain exactly the approved thirteen tools")
    for row in definitions:
        if not isinstance(row.get("description"), str) or not row["description"]:
            raise ValueError(f"tool description is absent: {row.get('name')}")
        for key in ("inputSchema", "outputSchema"):
            schema = row.get(key)
            if not isinstance(schema, dict):
                raise ValueError(f"tool {key} is absent: {row['name']}")
            if schema.get("$schema") != "https://json-schema.org/draft/2020-12/schema":
                raise ValueError(f"tool {key} draft mismatch: {row['name']}")
            closed = (
                schema.get("additionalProperties") is False
                or schema.get("unevaluatedProperties") is False
            )
            if schema.get("type") != "object" or not closed:
                raise ValueError(f"tool {key} is not a closed object: {row['name']}")
        annotations = row.get("annotations")
        if not isinstance(annotations, dict) or set(annotations) != {
            "readOnlyHint",
            "destructiveHint",
            "idempotentHint",
            "openWorldHint",
        }:
            raise ValueError(f"tool annotations mismatch: {row['name']}")
        if (
            annotations["readOnlyHint"] is not True
            or annotations["destructiveHint"] is not False
            or annotations["openWorldHint"] is not False
            or not isinstance(annotations["idempotentHint"], bool)
        ):
            raise ValueError(f"tool annotation values mismatch: {row['name']}")
        schemas = {
            "inputSchema": row["inputSchema"],
            "outputSchema": row["outputSchema"],
        }
        if sha256(canonical_json(schemas)) != TOOL_SCHEMA_SHA256[row["name"]]:
            raise ValueError(f"tool schemas differ from the frozen M1 contract: {row['name']}")
    return definitions


def validate_tool_result(response: object, expected_error: bool, code: str | None) -> dict[str, object]:
    if not isinstance(response, dict) or not isinstance(response.get("result"), dict):
        raise ValueError("tool response result is absent")
    result = response["result"]
    if result.get("isError") is not expected_error or result.get("resultType") != "complete":
        raise ValueError("tool completion/error classification mismatch")
    structured = result.get("structuredContent")
    content = result.get("content")
    if not isinstance(structured, dict) or not isinstance(content, list) or len(content) != 1:
        raise ValueError("tool structured/text content is absent")
    if content[0].get("type") != "text" or not isinstance(content[0].get("text"), str):
        raise ValueError("tool fallback content is invalid")
    fallback = strict_json(content[0]["text"], "tool fallback text")
    if fallback != structured:
        raise ValueError("tool structuredContent and text fallback differ")
    if structured.get("error_code") != code:
        raise ValueError(f"tool error code mismatch: expected {code}")
    if code is not None and structured.get("data") is not None:
        raise ValueError("failed tool response unexpectedly contains data")
    return structured


def validate_catalog_absence(structured: dict[str, object]) -> None:
    if structured.get("status") != "passed" or structured.get("error_code") is not None:
        raise ValueError("catalog.status absence response classification mismatch")
    data = structured.get("data")
    if not isinstance(data, dict) or data.get("semantics") != "latest_known":
        raise ValueError("catalog.status semantics mismatch")
    if data.get("network") != {
        "acquisition_allowed": False,
        "enforcement": "runtime_api_disabled",
    }:
        raise ValueError("catalog.status network claim mismatch")
    context = data.get("context")
    if not isinstance(context, dict) or context.get("reservation") is not None:
        raise ValueError("catalog.status reservation mismatch")
    for component in ("catalog", "model", "semantic_index", "rustsec"):
        if context.get(component) != {"status": "unavailable", "reason": "not_configured"}:
            raise ValueError(f"catalog.status {component} absence mismatch")


def run_protocol(binary: Path, version: str) -> tuple[list[dict[str, object]], dict[str, object], str]:
    transport = Transport(binary)
    try:
        discovery = transport.request(modern_request(1, "server/discover"))
        validate_discovery(discovery, version)
        transport.calls[-1]["validated"] = "modern_discovery"
        listing = transport.request(modern_request(2, "tools/list"))
        tools = validate_tools(listing)
        schema_hash = sha256(canonical_json(tools))
        transport.calls[-1].update(
            {"validated": "exact_tools_and_schemas", "tool_count": len(tools)}
        )
        calls = [
            (3, "rust.catalog.status", {}, False, None),
            (4, "rust.crate.search", {"query": "release smoke"}, True, "CATALOG_UNAVAILABLE"),
            (5, "rust.project.open", {"path": "/"}, True, "SANDBOX_DENIED"),
            (6, "rust.diagnostics.explain", {"code": "E0502"}, True, "SANDBOX_DENIED"),
        ]
        for identifier, name, arguments, is_error, code in calls:
            response = transport.request(
                modern_request(identifier, "tools/call", {"name": name, "arguments": arguments})
            )
            structured = validate_tool_result(response, is_error, code)
            transport.calls[-1].update(
                {
                    "validated": "structured_and_text_coherent",
                    "status": structured.get("status"),
                    "error_code": structured.get("error_code"),
                }
            )
            if name == "rust.catalog.status":
                validate_catalog_absence(structured)
            elif code == "SANDBOX_DENIED" and structured.get("status") != "blocked":
                raise ValueError(f"{name} did not report blocked status")
            elif code == "CATALOG_UNAVAILABLE" and structured.get("status") != "unavailable":
                raise ValueError(f"{name} did not report unavailable status")
        process = transport.finish()
        return transport.calls, process, schema_hash
    except Exception:
        transport.abort()
        raise


def write_receipt(path: Path, value: dict[str, object]) -> None:
    if path.name in {"", ".", ".."}:
        raise ValueError("receipt path must name a file")
    parent = path.parent.resolve(strict=True)
    if not parent.is_dir():
        raise ValueError("receipt parent must be a directory")
    directory = os.open(parent, os.O_RDONLY | os.O_DIRECTORY)
    descriptor = -1
    created = False
    try:
        flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
        if hasattr(os, "O_NOFOLLOW"):
            flags |= os.O_NOFOLLOW
        descriptor = os.open(path.name, flags, 0o600, dir_fd=directory)  # NOSONAR -- fixed basename under a resolved directory fd, exclusive and no-follow.
        created = True
        data = canonical_json(value)
        written = 0
        while written < len(data):
            count = os.write(descriptor, data[written:])
            if count <= 0:
                raise ValueError("short receipt write")
            written += count
        os.fsync(descriptor)
    except Exception:
        if descriptor >= 0:
            os.close(descriptor)
            descriptor = -1
        if created:
            try:
                os.unlink(path.name, dir_fd=directory)  # NOSONAR -- removes only the receipt this function exclusively created via the same directory fd.
            except FileNotFoundError:
                pass
        raise
    finally:
        if descriptor >= 0:
            os.close(descriptor)
        os.close(directory)


def smoke(archive: Path, sums: Path, tag: str, target: str) -> dict[str, object]:
    validate_host()
    started_at = utc_now()
    started = time.monotonic()
    contents, archive_evidence = validate_archive(archive, sums, tag, target)
    binary_name = archive_evidence["binary_name"]
    temporary = Path(tempfile.mkdtemp(prefix="rust-mcp-release-smoke-"))
    temporary.chmod(0o700)
    cli_calls: list[dict[str, object]] = []
    protocol_calls: list[dict[str, object]] = []
    process: dict[str, object] = {}
    schema_hash = ""
    try:
        binary = install_members(temporary, contents, binary_name)
        cli_calls, doctor = validate_cli(binary, tag[1:])
        protocol_calls, process, schema_hash = run_protocol(binary, tag[1:])
        doctor_hash = sha256(canonical_json(doctor))
    finally:
        shutil.rmtree(temporary)
    if temporary.exists():
        raise ValueError("temporary installation cleanup failed")
    return {
        "schema": "rust-engineering-mcp-release-smoke-receipt-v1",
        "status": "passed",
        "started_at_utc": started_at,
        "finished_at_utc": utc_now(),
        "duration_ms": int((time.monotonic() - started) * 1000),
        "release": {"tag": tag, "version": tag[1:], "target": target, **archive_evidence},
        "cli": {"calls": cli_calls, "doctor_report_sha256": doctor_hash},
        "mcp": {
            "protocol_version": PROTOCOL_VERSION,
            "tool_count": len(TOOLS),
            "tool_schema_set_sha256": schema_hash,
            "calls": protocol_calls,
            "process": process,
        },
        "counts": {
            "archive_members": archive_evidence["members"],
            "packages": archive_evidence["packages"],
            "tools": len(TOOLS),
            "cli_calls": len(cli_calls),
            "mcp_calls": len(protocol_calls),
        },
        "cleanup": {
            "temporary_installation_removed": True,
            "installed_members": archive_evidence["members"],
            "process_group_clean": process["process_group_clean"],
        },
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", required=True, type=Path)
    parser.add_argument("--sha256sums", required=True, type=Path)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--target", required=True, choices=[SUPPORTED_TARGET])
    parser.add_argument("--output-receipt", required=True, type=Path)
    args = parser.parse_args()
    if args.output_receipt.exists() or args.output_receipt.is_symlink():
        raise ValueError("receipt already exists; refusing to overwrite")
    receipt = smoke(args.archive, args.sha256sums, args.tag, args.target)
    write_receipt(args.output_receipt, receipt)
    print(canonical_json({
        "status": "passed",
        "receipt": str(args.output_receipt),
        "archive_sha256": receipt["release"]["archive_sha256"],
        "members": receipt["counts"]["archive_members"],
        "packages": receipt["counts"]["packages"],
        "tools": receipt["counts"]["tools"],
    }).decode(), end="")


if __name__ == "__main__":
    if not __debug__:
        raise RuntimeError("optimized Python mode is rejected")
    main()
