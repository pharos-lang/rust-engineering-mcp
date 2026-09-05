#!/usr/bin/env python3
"""Build and verify one deterministic core release archive using only stdlib.

The command never resolves online, builds code, downloads assets, or guesses missing
license text.  Cargo metadata supplies the target-filtered default-feature graph;
only normal and build dependency edges reachable from rust-engineering-mcp are kept.
Archive bytes are deterministic on the qualified Darwin arm64 Python/zlib toolchain;
cross-zlib byte identity is not claimed.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import gzip
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import platform
import re
import stat
import struct
import subprocess
import tarfile
import tempfile
import tomllib
from typing import Iterable


ROOT = Path(__file__).resolve().parents[1]
SUPPORTED_TARGET = "aarch64-apple-darwin"
ROOT_PACKAGE = "rust-engineering-mcp"
LICENSE_IDENTIFIER = (
    r"(?:MIT|APACHE(?:2|[-_.]?2(?:\.0)?)?|ISC|UNICODE|ZLIB|BOOST|"
    r"BSD(?:[-_.]?3[-_]?CLAUSE)?|BORINGSSL|THIRD[-_]?PARTY|OTHER[-_]?BITS|"
    r"APACHE[-_.]?2(?:\.0)?_WITH_LLVM[-_]?EXCEPTION)"
)
TEXT_NAME = re.compile(
    rf"(?:LICENSE|LICENCE|COPYING|NOTICES?|UNLICENSE)"
    rf"(?:(?:[-_.]){LICENSE_IDENTIFIER})?(?:\.(?:txt|md|html))?"
    rf"|THIRD[-_]?PARTY[-_]?NOTICES?(?:\.(?:txt|md|html))?",
    re.IGNORECASE,
)
TAG = re.compile(r"^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$")
PROHIBITED_PACKAGES = {
    "fastembed",
    "kanaria",
    "lance",
    "lancedb",
    "ort",
    "ort-sys",
}
PROHIBITED_PATH_PARTS = {
    "catalog",
    "fixture-1.tar.zst",
    "fixture-trust.json",
    "model",
    "model.onnx",
    "onnxruntime",
}
MAX_BINARY_BYTES = 512 * 1024 * 1024
MAX_TEXT_BYTES = 16 * 1024 * 1024
MAX_TOTAL_NOTICE_BYTES = 128 * 1024 * 1024
MACHO_ARM64_CPU = 0x0100000C
MACHO_EXECUTE = 2


@dataclass(frozen=True)
class Member:
    path: str
    data: bytes
    mode: int = 0o644

    def row(self) -> dict[str, object]:
        return {
            "path": self.path,
            "bytes": len(self.data),
            "sha256": digest(self.data),
            "mode": format(self.mode, "04o"),
        }


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def canonical_json(value: object) -> bytes:
    return (json.dumps(value, indent=2, sort_keys=True, ensure_ascii=False) + "\n").encode()


def validate_host() -> None:
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        raise ValueError("core release packaging requires the qualified Darwin arm64 host")


def validate_macho_arm64(data: bytes) -> None:
    if len(data) < 16:
        raise ValueError("release binary is too small to be a Mach-O arm64 executable")
    if data[:4] == b"\xcf\xfa\xed\xfe":
        cpu = struct.unpack_from("<I", data, 4)[0]
        filetype = struct.unpack_from("<I", data, 12)[0]
    elif data[:4] == b"\xfe\xed\xfa\xcf":
        cpu = struct.unpack_from(">I", data, 4)[0]
        filetype = struct.unpack_from(">I", data, 12)[0]
    else:
        raise ValueError("release binary must be a thin 64-bit Mach-O executable")
    if cpu != MACHO_ARM64_CPU:
        raise ValueError("release binary Mach-O CPU is not arm64")
    if filetype != MACHO_EXECUTE:
        raise ValueError("release binary Mach-O file type is not MH_EXECUTE")


def require_exact_licenses(missing: list[str]) -> None:
    if missing:
        raise ValueError(
            "packages missing declared license or exact text: " + ", ".join(missing)
        )


def safe_relative(value: str) -> str:
    path = PurePosixPath(value)
    if not value or path.is_absolute() or ".." in path.parts or "." in path.parts:
        raise ValueError(f"unsafe archive path: {value!r}")
    normalized = path.as_posix()
    if normalized != value or "\\" in value or "\x00" in value:
        raise ValueError(f"non-canonical archive path: {value!r}")
    return normalized


def read_regular(path: Path, limit: int) -> bytes:
    """Read stable regular bytes without following the final symlink."""
    flags = os.O_RDONLY
    if hasattr(os, "O_NOFOLLOW"):
        flags |= os.O_NOFOLLOW
    try:
        descriptor = os.open(path, flags)
    except OSError as error:
        raise ValueError(f"cannot open regular no-follow file {path}: {error}") from error
    try:
        before = os.fstat(descriptor)
        if not stat.S_ISREG(before.st_mode) or before.st_nlink != 1:
            raise ValueError(f"file must be regular, single-link and no-follow: {path}")
        if before.st_size > limit:
            raise ValueError(f"file exceeds {limit} bytes: {path}")
        chunks = []
        remaining = before.st_size
        while remaining:
            chunk = os.read(descriptor, min(remaining, 1024 * 1024))
            if not chunk:
                raise ValueError(f"file changed while reading: {path}")
            chunks.append(chunk)
            remaining -= len(chunk)
        if os.read(descriptor, 1):
            raise ValueError(f"file grew while reading: {path}")
        after = os.fstat(descriptor)
        stable = (before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns)
        observed = (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns)
        if stable != observed:
            raise ValueError(f"file changed while reading: {path}")
        return b"".join(chunks)
    finally:
        os.close(descriptor)


def cargo_metadata(root: Path, target: str) -> dict[str, object]:
    command = [
        "cargo",
        "+1.98.1",
        "metadata",
        "--locked",
        "--offline",
        "--filter-platform",
        target,
        "--format-version",
        "1",
    ]
    try:
        output = subprocess.run(
            command,
            cwd=root,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
            timeout=120,
            env={key: value for key, value in os.environ.items() if key in {
                "CARGO_HOME", "HOME", "PATH", "RUSTUP_HOME", "TMPDIR"
            }},
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise ValueError(f"cargo metadata unavailable: {error}") from error
    if output.returncode:
        message = output.stderr.decode("utf-8", "replace")[-4096:]
        raise ValueError(f"cargo metadata failed offline: {message}")
    try:
        return json.loads(output.stdout)
    except json.JSONDecodeError as error:
        raise ValueError("cargo metadata returned invalid JSON") from error


def root_package_id(metadata: dict[str, object], name: str = ROOT_PACKAGE) -> str:
    members = set(metadata.get("workspace_members", []))
    matches = [
        package["id"]
        for package in metadata.get("packages", [])
        if package.get("name") == name and package.get("id") in members
    ]
    if len(matches) != 1:
        raise ValueError(f"expected exactly one workspace package named {name}, got {len(matches)}")
    return matches[0]


def dependency_closure(
    metadata: dict[str, object], root_id: str
) -> tuple[set[str], dict[str, set[str]], list[tuple[str, str, str]]]:
    """Return reachable package ids, incoming roles and selected directed edges."""
    resolve = metadata.get("resolve")
    if not isinstance(resolve, dict) or not isinstance(resolve.get("nodes"), list):
        raise ValueError("metadata resolve graph is absent")
    nodes = {node.get("id"): node for node in resolve["nodes"]}
    if root_id not in nodes:
        raise ValueError("root package is absent from resolve graph")
    seen = {root_id}
    roles: dict[str, set[str]] = {root_id: {"root"}}
    edges: list[tuple[str, str, str]] = []
    pending = [root_id]
    while pending:
        parent = pending.pop()
        node = nodes[parent]
        deps = node.get("deps")
        if not isinstance(deps, list):
            raise ValueError(f"invalid dependency list for {parent}")
        for dependency in deps:
            child = dependency.get("pkg")
            if child not in nodes:
                raise ValueError(f"dependency node absent from graph: {child}")
            selected_roles = set()
            for kind in dependency.get("dep_kinds", []):
                value = kind.get("kind") or "normal"
                if value in {"normal", "build"}:
                    selected_roles.add(value)
            for role in sorted(selected_roles):
                edges.append((parent, child, role))
                roles.setdefault(child, set()).add(role)
            if selected_roles and child not in seen:
                seen.add(child)
                pending.append(child)
    return seen, roles, sorted(set(edges))


def validate_tag(metadata: dict[str, object], tag: str, root_id: str) -> str:
    if not TAG.fullmatch(tag):
        raise ValueError("tag must be a stable semantic version of the form vX.Y.Z")
    package = next((item for item in metadata["packages"] if item.get("id") == root_id), None)
    if package is None:
        raise ValueError("root package metadata is absent")
    version = package.get("version")
    if version != tag[1:]:
        raise ValueError(f"tag {tag} does not match workspace version {version}")
    return version


def text_kind(path: Path) -> str:
    return "notice" if "notice" in path.name.lower() else "license_or_copying"


def package_texts(
    package: dict[str, object], root: Path, workspace_member: bool
) -> list[tuple[str, bytes, str]]:
    manifest = Path(str(package["manifest_path"]))
    package_root = manifest.parent
    if workspace_member:
        candidates = [
            (path, "discovered-name")
            for path in sorted(root.iterdir())
            if TEXT_NAME.fullmatch(path.name)
        ]
    else:
        candidates = []
        for directory, dirs, files in os.walk(package_root, followlinks=False):
            base = Path(directory)
            dirs[:] = sorted(
                name for name in dirs
                if name not in {".git", "target"} and not (base / name).is_symlink()
            )
            candidates.extend(
                (base / name, "discovered-name")
                for name in sorted(files)
                if TEXT_NAME.fullmatch(name)
            )
        explicit = package.get("license_file")
        if explicit:
            explicit_path = Path(str(explicit))
            if not explicit_path.is_absolute():
                explicit_path = package_root / explicit_path
            try:
                explicit_path.resolve(strict=True).relative_to(package_root.resolve(strict=True))
            except (OSError, ValueError) as error:
                raise ValueError(f"license-file escapes package {package['name']}") from error
            # Cargo metadata's license_file is an explicit semantic declaration,
            # not a filename guess. Preserve that provenance even when publishers
            # use conventional names such as COPYRIGHT or legal/terms.txt.
            candidates.append((explicit_path, "manifest-license-file"))
    found = []
    for index, (path, source) in enumerate(sorted(set(candidates))):
        data = read_regular(path, MAX_TEXT_BYTES)
        kind = "license_or_copying" if source == "manifest-license-file" else text_kind(path)
        found.append((f"package/{source}/{index:03d}-{path.name}", data, kind))
    return found


def supplements(root: Path) -> dict[tuple[str, str], list[tuple[str, bytes, str]]]:
    receipt_path = root / "docs/release/upstream-licenses/receipt.json"
    receipt = json.loads(read_regular(receipt_path, MAX_TEXT_BYTES))
    result: dict[tuple[str, str], list[tuple[str, bytes, str]]] = {}
    for group in receipt.get("groups", []):
        texts = []
        for index, row in enumerate(group.get("texts", [])):
            relative = row.get("file")
            expected = row.get("sha256")
            if not isinstance(relative, str) or not isinstance(expected, str):
                raise ValueError("invalid upstream license receipt text")
            path = root / relative
            data = read_regular(path, MAX_TEXT_BYTES)
            if digest(data) != expected:
                raise ValueError(f"upstream license receipt hash mismatch: {relative}")
            texts.append((f"supplement/{index:03d}-{path.name}", data, text_kind(path)))
        for package in group.get("packages", []):
            key = package.get("name"), package.get("version")
            if not all(isinstance(value, str) for value in key):
                raise ValueError("invalid upstream license package identity")
            if texts:
                result.setdefault(key, []).extend(texts)
    return result


def lock_rows(root: Path) -> dict[tuple[str, str, object], dict[str, object]]:
    lock = tomllib.loads(read_regular(root / "Cargo.lock", MAX_TEXT_BYTES).decode())
    return {
        (row["name"], row["version"], row.get("source")): row
        for row in lock.get("package", [])
    }


def ensure_core_only(package_names: Iterable[str]) -> None:
    forbidden = sorted(
        name for name in set(package_names)
        if name in PROHIBITED_PACKAGES
        or name.startswith(("fastembed-", "kanaria-", "lance-", "lancedb-", "ort-"))
    )
    if forbidden:
        raise ValueError(
            "local/model/native packages entered core closure: " + ", ".join(forbidden)
        )


def inventory_and_notices(
    metadata: dict[str, object], root: Path, target: str, tag: str
) -> tuple[dict[str, object], bytes, str, list[tuple[str, str, str]]]:
    root_id = root_package_id(metadata)
    version = validate_tag(metadata, tag, root_id)
    closure, roles, edges = dependency_closure(metadata, root_id)
    packages_by_id = {package["id"]: package for package in metadata["packages"]}
    missing_nodes = sorted(closure - packages_by_id.keys())
    if missing_nodes:
        raise ValueError("resolved packages absent from metadata: " + ", ".join(missing_nodes))
    selected = [packages_by_id[identity] for identity in closure]
    ensure_core_only(str(package["name"]) for package in selected)
    locked = lock_rows(root)
    supplemental = supplements(root)
    workspace_members = set(metadata.get("workspace_members", []))
    package_rows = []
    missing_license = []
    notice = bytearray(
        b"Rust Engineering MCP third-party notices\n"
        b"Exact source license and notice bytes for the target-specific core closure follow.\n"
    )
    for package in sorted(selected, key=lambda item: (item["name"], item["version"], item["id"])):
        key = package["name"], package["version"], package.get("source")
        lock = locked.get(key)
        if lock is None:
            raise ValueError(f"resolved package missing from Cargo.lock: {key}")
        declared = package.get("license")
        workspace_member = package["id"] in workspace_members
        texts = package_texts(package, root, workspace_member)
        has_license_text = any(kind == "license_or_copying" for _, _, kind in texts)
        if not workspace_member and not has_license_text:
            texts.extend(supplemental.get((package["name"], package["version"]), []))
        if not declared or not any(kind == "license_or_copying" for _, _, kind in texts):
            missing_license.append(f"{package['name']} {package['version']}")
        text_rows = []
        for label, data, kind in texts:
            text_rows.append({
                "label": label,
                "kind": kind,
                "bytes": len(data),
                "sha256": digest(data),
            })
            if package["id"] not in workspace_members:
                header = (
                    f"\n===== {package['name']} {package['version']} :: {label} :: "
                    f"sha256:{digest(data)} =====\n"
                ).encode()
                notice.extend(header)
                notice.extend(data)
                notice.extend(b"\n===== END EXACT SOURCE BYTES =====\n")
                if len(notice) > MAX_TOTAL_NOTICE_BYTES:
                    raise ValueError("third-party notices exceed bounded size")
        node = next(item for item in metadata["resolve"]["nodes"] if item["id"] == package["id"])
        if "local" in node.get("features", []):
            raise ValueError(f"local feature entered core closure: {package['name']}")
        package_rows.append({
            "id": package["id"].replace(str(root), "$WORKSPACE"),
            "name": package["name"],
            "version": package["version"],
            "source": package.get("source"),
            "lock_checksum": lock.get("checksum"),
            "declared_license": declared,
            "workspace_member": package["id"] in workspace_members,
            "roles": sorted(roles.get(package["id"], set())),
            "enabled_features": sorted(node.get("features", [])),
            "texts": text_rows,
        })
    require_exact_licenses(missing_license)
    inventory = {
        "schema": "rust-engineering-mcp-core-inventory-v1",
        "artifact": {
            "tag": tag,
            "version": version,
            "target": target,
            "profile": "core-default",
            "determinism_scope": (
                "qualified Darwin arm64 Python/zlib toolchain; "
                "not cross-zlib-byte-universal"
            ),
        },
        "inputs": {
            "cargo_lock": {
                "path": "Cargo.lock",
                "sha256": digest(read_regular(root / "Cargo.lock", MAX_TEXT_BYTES)),
            },
            "packaging_script": {
                "path": "scripts/release-artifact.py",
                "sha256": digest(read_regular(Path(__file__), MAX_TEXT_BYTES)),
            },
        },
        "resolution": {
            "command": ["cargo", "+1.98.1", "metadata", "--locked", "--offline",
                        "--filter-platform", target, "--format-version", "1"],
            "edge_kinds": ["normal", "build"],
            "dev_dependencies_included": False,
            "package_count": len(package_rows),
        },
        "packages": package_rows,
        "edges": [
            {"from": parent.replace(str(root), "$WORKSPACE"),
             "to": child.replace(str(root), "$WORKSPACE"), "role": role}
            for parent, child, role in edges
        ],
    }
    return inventory, bytes(notice), root_id, edges


def spdx_document(
    inventory: dict[str, object], root_id: str, edges: list[tuple[str, str, str]]
) -> dict[str, object]:
    packages = inventory["packages"]
    ids = {row["id"]: f"SPDXRef-Package-{index:04d}" for index, row in enumerate(packages, 1)}
    root_key = root_id.replace(str(ROOT), "$WORKSPACE")
    namespace_seed = canonical_json(inventory)
    relationships = [{
        "spdxElementId": "SPDXRef-DOCUMENT",
        "relationshipType": "DESCRIBES",
        "relatedSpdxElement": ids[root_key],
    }]
    for parent, child in sorted({(parent, child) for parent, child, _role in edges}):
        parent_key = parent.replace(str(ROOT), "$WORKSPACE")
        child_key = child.replace(str(ROOT), "$WORKSPACE")
        relationships.append({
            "spdxElementId": ids[parent_key],
            "relationshipType": "DEPENDS_ON",
            "relatedSpdxElement": ids[child_key],
        })
    spdx_packages = []
    for row in packages:
        package = {
            "SPDXID": ids[row["id"]],
            "name": row["name"],
            "versionInfo": row["version"],
            "downloadLocation": row["source"] or "NOASSERTION",
            "filesAnalyzed": False,
            "licenseConcluded": "NOASSERTION",
            "licenseDeclared": row["declared_license"],
            "copyrightText": "NOASSERTION",
        }
        if row["lock_checksum"]:
            package["checksums"] = [{"algorithm": "SHA256", "checksumValue": row["lock_checksum"]}]
        spdx_packages.append(package)
    return {
        "spdxVersion": "SPDX-2.3",
        "dataLicense": "CC0-1.0",
        "SPDXID": "SPDXRef-DOCUMENT",
        "name": (
            f"rust-engineering-mcp-{inventory['artifact']['tag']}-"
            f"{inventory['artifact']['target']}"
        ),
        "documentNamespace": (
            "https://github.com/pharos-lang/rust-engineering-mcp/sbom/"
            + digest(namespace_seed)
        ),
        "creationInfo": {
            "created": "1970-01-01T00:00:00Z",
            "creators": ["Tool: scripts/release-artifact.py"],
        },
        "packages": spdx_packages,
        "relationships": sorted(
            relationships,
            key=lambda row: (
                row["spdxElementId"],
                row["relationshipType"],
                row["relatedSpdxElement"],
            ),
        ),
    }


def build_members(
    root: Path,
    binary_name: str,
    binary_data: bytes,
    inventory: dict[str, object],
    notices: bytes,
    spdx: dict[str, object],
) -> list[Member]:
    safe_relative(binary_name)
    members = [Member(binary_name, binary_data, 0o755)]
    document_names = ["README.md", "SECURITY.md", "NOTICE"]
    document_names.extend(
        path.name
        for path in sorted(root.iterdir())
        if path.name.startswith("LICENSE") and TEXT_NAME.fullmatch(path.name)
    )
    if not any(name.startswith("LICENSE") for name in document_names):
        raise ValueError("release requires at least one top-level LICENSE* file")
    for filename in document_names:
        members.append(Member(filename, read_regular(root / filename, MAX_TEXT_BYTES)))
    members.extend([
        Member("inventory.json", canonical_json(inventory)),
        Member("sbom.spdx.json", canonical_json(spdx)),
        Member("THIRD_PARTY_NOTICES.txt", notices),
    ])
    paths = [member.path for member in members]
    if len(paths) != len(set(paths)):
        raise ValueError("duplicate release member")
    for path in paths:
        safe_relative(path)
    manifest = {
        "schema": "rust-engineering-mcp-release-manifest-v1",
        "hash_algorithm": "SHA-256",
        "members": sorted([
            *[member.row() for member in members],
            {
                "path": "MANIFEST.json",
                "bytes": None,
                "sha256": None,
                "mode": "0644",
                "self_reference": "size-and-hash-not-representable-inside-member",
            },
        ], key=lambda row: row["path"]),
    }
    members.append(Member("MANIFEST.json", canonical_json(manifest)))
    return sorted(members, key=lambda item: item.path)


def write_deterministic_archive(path: Path, prefix: str, members: list[Member]) -> None:
    safe_relative(prefix)
    with path.open("xb") as raw:
        with gzip.GzipFile(
            filename="", mode="wb", compresslevel=9, fileobj=raw, mtime=0
        ) as compressed:
            with tarfile.open(
                fileobj=compressed, mode="w|", format=tarfile.USTAR_FORMAT
            ) as archive:
                for member in sorted(members, key=lambda item: item.path):
                    info = tarfile.TarInfo(f"{prefix}/{member.path}")
                    info.size = len(member.data)
                    info.mode = member.mode
                    info.uid = 0
                    info.gid = 0
                    info.uname = ""
                    info.gname = ""
                    info.mtime = 0
                    archive.addfile(info, io.BytesIO(member.data))


def verify_archive(path: Path, prefix: str, expected: list[Member]) -> None:
    expected_rows = {member.path: member.row() for member in expected}
    seen = set()
    manifest_data = None
    with tarfile.open(path, mode="r:gz") as archive:
        for item in archive:
            name = safe_relative(item.name)
            expected_prefix = prefix + "/"
            if not name.startswith(expected_prefix):
                raise ValueError(f"archive member outside prefix: {name}")
            logical = name[len(expected_prefix):]
            safe_relative(logical)
            if logical in seen:
                raise ValueError(f"duplicate archive member: {logical}")
            if not item.isfile() or item.issym() or item.islnk():
                raise ValueError(f"archive contains non-regular member: {logical}")
            if item.uid != 0 or item.gid != 0 or item.mtime != 0:
                raise ValueError(f"archive metadata is not deterministic: {logical}")
            if item.uname or item.gname:
                raise ValueError(f"archive owner names are not empty: {logical}")
            row = expected_rows.get(logical)
            if row is None:
                raise ValueError(f"unexpected archive member: {logical}")
            stream = archive.extractfile(item)
            if stream is None:
                raise ValueError(f"cannot read archive member: {logical}")
            data = stream.read(MAX_BINARY_BYTES + 1)
            if len(data) != row["bytes"] or digest(data) != row["sha256"]:
                raise ValueError(f"archive member hash mismatch: {logical}")
            if stat.S_IMODE(item.mode) != int(str(row["mode"]), 8):
                raise ValueError(f"archive member mode mismatch: {logical}")
            if logical == "MANIFEST.json":
                manifest_data = data
            seen.add(logical)
    if seen != set(expected_rows):
        raise ValueError("archive members are incomplete")
    if manifest_data is None:
        raise ValueError("archive manifest is absent")
    manifest = json.loads(manifest_data)
    payload = {row["path"]: row for row in manifest.get("members", [])}
    expected_payload = {name: row for name, row in expected_rows.items() if name != "MANIFEST.json"}
    self_row = payload.pop("MANIFEST.json", None)
    expected_self = {
        "path": "MANIFEST.json",
        "bytes": None,
        "sha256": None,
        "mode": "0644",
        "self_reference": "size-and-hash-not-representable-inside-member",
    }
    if payload != expected_payload or self_row != expected_self:
        raise ValueError("archive manifest does not match payload")
    prohibited = []
    for logical in seen:
        lowered = {part.lower() for part in PurePosixPath(logical).parts}
        if lowered & PROHIBITED_PATH_PARTS or logical.lower().endswith(".onnx"):
            prohibited.append(logical)
    inventory = json.loads(
        next(member.data for member in expected if member.path == "inventory.json")
    )
    ensure_core_only(row["name"] for row in inventory.get("packages", []))
    if prohibited:
        raise ValueError(
            "prohibited local/model/catalog assets in core archive: "
            + ", ".join(prohibited)
        )


def prepare_output(path: Path) -> Path:
    if path.exists():
        if path.is_symlink() or not path.is_dir():
            raise ValueError("output directory must be a real directory")
    else:
        path.mkdir(parents=True, mode=0o755)
    return path.resolve(strict=True)


def publish_pair(candidate: Path, sum_candidate: Path, archive: Path, sums: Path) -> None:
    os.link(candidate, archive, follow_symlinks=False)
    try:
        os.link(sum_candidate, sums, follow_symlinks=False)
    except Exception:
        candidate_stat = candidate.stat(follow_symlinks=False)
        try:
            archive_stat = archive.stat(follow_symlinks=False)
        except FileNotFoundError:
            archive_stat = None
        if archive_stat is not None and (
            archive_stat.st_dev,
            archive_stat.st_ino,
        ) == (candidate_stat.st_dev, candidate_stat.st_ino):
            archive.unlink()
        raise


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--target", required=True, choices=[SUPPORTED_TARGET])
    parser.add_argument("--tag", required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    validate_host()
    binary_data = read_regular(args.binary, MAX_BINARY_BYTES)
    validate_macho_arm64(binary_data)
    metadata = cargo_metadata(ROOT, args.target)
    inventory, notices, root_id, edges = inventory_and_notices(
        metadata, ROOT, args.target, args.tag
    )
    inventory["artifact"]["binary"] = {
        "name": args.binary.name,
        "bytes": len(binary_data),
        "sha256": digest(binary_data),
        "mode": "0755",
    }
    spdx = spdx_document(inventory, root_id, edges)
    members = build_members(
        ROOT, args.binary.name, binary_data, inventory, notices, spdx
    )
    output = prepare_output(args.output_dir)
    stem = f"rust-engineering-mcp-{args.tag}-{args.target}"
    archive = output / f"{stem}.tar.gz"
    sums = output / "SHA256SUMS"
    if archive.exists() or sums.exists():
        raise ValueError("release output already exists; refusing to overwrite")
    with tempfile.TemporaryDirectory(prefix=".release-artifact-", dir=output) as temporary:
        temporary_path = Path(temporary)
        candidate = temporary_path / archive.name
        write_deterministic_archive(candidate, stem, members)
        verify_archive(candidate, stem, members)
        with candidate.open("rb") as candidate_stream:
            archive_hash = hashlib.file_digest(candidate_stream, "sha256").hexdigest()
        sum_data = f"{archive_hash}  {archive.name}\n".encode()
        sum_candidate = temporary_path / "SHA256SUMS"
        sum_candidate.write_bytes(sum_data)
        publish_pair(candidate, sum_candidate, archive, sums)
    print(canonical_json({
        "status": "passed",
        "archive": str(archive),
        "archive_sha256": archive_hash,
        "checksums": str(sums),
        "target": args.target,
        "tag": args.tag,
        "packages": len(inventory["packages"]),
    }).decode(), end="")


if __name__ == "__main__":
    if not __debug__:
        raise RuntimeError("optimized Python mode is rejected")
    main()
