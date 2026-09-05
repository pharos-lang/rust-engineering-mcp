#!/usr/bin/env python3
"""No-build/no-network unit tests for release-smoke.py."""

from __future__ import annotations

import copy
import gzip
import importlib.util
import io
import json
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest
from unittest import mock


SCRIPT = Path(__file__).with_name("release-smoke.py")
SPEC = importlib.util.spec_from_file_location("release_smoke", SCRIPT)
smoke = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = smoke
SPEC.loader.exec_module(smoke)


def inventory(binary: bytes) -> dict[str, object]:
    license_hash = smoke.sha256(b"product license\n")
    third_party_hash = smoke.sha256(b"third party license\n")
    root_id = "path+file://$WORKSPACE/crates/mcp-server#rust-engineering-mcp@0.1.0"
    dep_id = "registry+https://example.invalid/index#dependency@1.0.0"
    return {
        "schema": "rust-engineering-mcp-core-inventory-v1",
        "artifact": {
            "tag": "v0.1.0",
            "version": "0.1.0",
            "target": smoke.SUPPORTED_TARGET,
            "profile": "core-default",
            "binary": {
                "name": "rust-engineering-mcp",
                "bytes": len(binary),
                "sha256": smoke.sha256(binary),
                "mode": "0755",
            },
        },
        "resolution": {
            "command": [
                "cargo",
                "+1.98.1",
                "metadata",
                "--locked",
                "--offline",
                "--filter-platform",
                smoke.SUPPORTED_TARGET,
                "--format-version",
                "1",
            ],
            "edge_kinds": ["normal", "build"],
            "dev_dependencies_included": False,
            "package_count": 2,
        },
        "packages": [
            {
                "id": dep_id,
                "name": "dependency",
                "version": "1.0.0",
                "source": "registry+https://example.invalid/index",
                "lock_checksum": "1" * 64,
                "declared_license": "MIT",
                "workspace_member": False,
                "roles": ["normal"],
                "enabled_features": ["default"],
                "texts": [
                    {
                        "label": "package/LICENSE",
                        "kind": "license_or_copying",
                        "bytes": 20,
                        "sha256": third_party_hash,
                    }
                ],
            },
            {
                "id": root_id,
                "name": "rust-engineering-mcp",
                "version": "0.1.0",
                "source": None,
                "lock_checksum": None,
                "declared_license": "MIT OR Apache-2.0",
                "workspace_member": True,
                "roles": ["root"],
                "enabled_features": ["default"],
                "texts": [
                    {
                        "label": "package/LICENSE",
                        "kind": "license_or_copying",
                        "bytes": 16,
                        "sha256": license_hash,
                    }
                ],
            },
        ],
        "edges": [{"from": root_id, "to": dep_id, "role": "normal"}],
    }


def spdx(value: dict[str, object]) -> dict[str, object]:
    dep, root = value["packages"]
    return {
        "spdxVersion": "SPDX-2.3",
        "dataLicense": "CC0-1.0",
        "SPDXID": "SPDXRef-DOCUMENT",
        "documentNamespace": (
            "https://github.com/pharos-lang/rust-engineering-mcp/sbom/"
            + smoke.sha256(smoke.canonical_json(value))
        ),
        "packages": [
            {
                "SPDXID": "SPDXRef-Package-0001",
                "name": dep["name"],
                "versionInfo": dep["version"],
                "licenseDeclared": dep["declared_license"],
                "filesAnalyzed": False,
                "checksums": [
                    {"algorithm": "SHA256", "checksumValue": dep["lock_checksum"]}
                ],
            },
            {
                "SPDXID": "SPDXRef-Package-0002",
                "name": root["name"],
                "versionInfo": root["version"],
                "licenseDeclared": root["declared_license"],
                "filesAnalyzed": False,
            },
        ],
        "relationships": [
            {
                "spdxElementId": "SPDXRef-DOCUMENT",
                "relationshipType": "DESCRIBES",
                "relatedSpdxElement": "SPDXRef-Package-0002",
            },
            {
                "spdxElementId": "SPDXRef-Package-0002",
                "relationshipType": "DEPENDS_ON",
                "relatedSpdxElement": "SPDXRef-Package-0001",
            },
        ],
    }


def base_members() -> dict[str, bytes]:
    binary = (
        b"\xcf\xfa\xed\xfe"
        + smoke.MACHO_ARM64_CPU.to_bytes(4, "little")
        + b"\x00" * 4
        + smoke.MACHO_EXECUTE.to_bytes(4, "little")
        + b"synthetic executable bytes"
    )
    value = inventory(binary)
    return {
        "rust-engineering-mcp": binary,
        "README.md": b"readme\n",
        "SECURITY.md": b"security\n",
        "LICENSE": b"product license\n",
        "NOTICE": b"notice\n",
        "THIRD_PARTY_NOTICES.txt": (
            b"Rust Engineering MCP third-party notices\nsha256:"
            + smoke.sha256(b"third party license\n").encode()
            + b"\n"
        ),
        "inventory.json": smoke.canonical_json(value),
        "sbom.spdx.json": smoke.canonical_json(spdx(value)),
    }


def manifest_members(members: dict[str, bytes]) -> dict[str, bytes]:
    rows = [
        {
            "path": name,
            "bytes": len(data),
            "sha256": smoke.sha256(data),
            "mode": "0755" if name == "rust-engineering-mcp" else "0644",
        }
        for name, data in sorted(members.items())
    ]
    rows.append(dict(smoke.EXPECTED_SELF_ROW))
    result = dict(members)
    result["MANIFEST.json"] = smoke.canonical_json(
        {
            "schema": "rust-engineering-mcp-release-manifest-v1",
            "hash_algorithm": "SHA-256",
            "members": sorted(rows, key=lambda row: row["path"]),
        }
    )
    return result


def write_candidate(
    directory: Path,
    members: dict[str, bytes],
    *,
    raw_names: dict[str, str] | None = None,
) -> tuple[Path, Path]:
    archive = directory / (
        f"rust-engineering-mcp-v0.1.0-{smoke.SUPPORTED_TARGET}.tar.gz"
    )
    prefix = archive.name.removesuffix(".tar.gz")
    with archive.open("xb") as raw:
        with gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0) as compressed:
            with tarfile.open(fileobj=compressed, mode="w|", format=tarfile.USTAR_FORMAT) as tar:
                for name, data in sorted(members.items()):
                    archive_name = raw_names.get(name, f"{prefix}/{name}") if raw_names else f"{prefix}/{name}"
                    info = tarfile.TarInfo(archive_name)
                    info.size = len(data)
                    info.mode = 0o755 if name == "rust-engineering-mcp" else 0o644
                    info.uid = 0
                    info.gid = 0
                    info.mtime = 0
                    tar.addfile(info, io.BytesIO(data))
    sums = directory / "SHA256SUMS"
    sums.write_text(f"{smoke.sha256(archive.read_bytes())}  {archive.name}\n", encoding="ascii")
    return archive, sums


def rewrite_json(members: dict[str, bytes], name: str, mutate) -> dict[str, bytes]:
    result = dict(members)
    value = json.loads(result[name])
    mutate(value)
    result[name] = smoke.canonical_json(value)
    return result


class ReleaseSmokeTests(unittest.TestCase):
    def validate(self, archive: Path, sums: Path):
        return smoke.validate_archive(
            archive, sums, "v0.1.0", smoke.SUPPORTED_TARGET
        )

    def test_valid_synthetic_archive(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            archive, sums = write_candidate(Path(directory), manifest_members(base_members()))
            contents, evidence = self.validate(archive, sums)
            self.assertEqual(evidence["packages"], 2)
            self.assertEqual(evidence["members"], len(contents))

    def test_checksum_tampering_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            archive, sums = write_candidate(Path(directory), manifest_members(base_members()))
            with archive.open("ab") as handle:
                handle.write(b"tamper")
            with self.assertRaisesRegex(ValueError, "checksum"):
                self.validate(archive, sums)

    def test_traversal_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            members = manifest_members(base_members())
            archive, sums = write_candidate(
                Path(directory), members, raw_names={"README.md": "prefix/../README.md"}
            )
            with self.assertRaisesRegex(ValueError, "unsafe archive path"):
                self.validate(archive, sums)

    def test_manifest_self_row_and_hash_are_rejected_when_tampered(self) -> None:
        for mutation in ("self", "hash"):
            with self.subTest(mutation=mutation), tempfile.TemporaryDirectory() as directory:
                members = manifest_members(base_members())
                manifest = json.loads(members["MANIFEST.json"])
                row = next(row for row in manifest["members"] if row["path"] == "MANIFEST.json")
                if mutation == "self":
                    row["sha256"] = "0" * 64
                else:
                    row = next(row for row in manifest["members"] if row["path"] == "README.md")
                    row["sha256"] = "0" * 64
                members["MANIFEST.json"] = smoke.canonical_json(manifest)
                archive, sums = write_candidate(Path(directory), members)
                with self.assertRaisesRegex(ValueError, "manifest"):
                    self.validate(archive, sums)

    def test_inventory_mismatch_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            members = base_members()
            members = rewrite_json(
                members,
                "inventory.json",
                lambda value: value["artifact"].update(target="x86_64-unknown-linux-gnu"),
            )
            archive, sums = write_candidate(Path(directory), manifest_members(members))
            with self.assertRaisesRegex(ValueError, "inventory artifact target"):
                self.validate(archive, sums)

    def test_spdx_relationship_mismatch_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            members = base_members()
            members = rewrite_json(
                members,
                "sbom.spdx.json",
                lambda value: value["relationships"].pop(),
            )
            archive, sums = write_candidate(Path(directory), manifest_members(members))
            with self.assertRaisesRegex(ValueError, "SPDX relationships"):
                self.validate(archive, sums)

    def test_prohibited_asset_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            members = base_members()
            members["model.onnx"] = b"forbidden"
            archive, sums = write_candidate(Path(directory), manifest_members(members))
            with self.assertRaisesRegex(ValueError, "prohibited release asset"):
                self.validate(archive, sums)

    def test_transcript_validators_reject_incoherent_text_and_tool_set(self) -> None:
        structured = {"status": "blocked", "error_code": "SANDBOX_DENIED", "data": None}
        response = {
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "isError": True,
                "resultType": "complete",
                "structuredContent": structured,
                "content": [{"type": "text", "text": json.dumps(structured)}],
            },
        }
        self.assertEqual(
            smoke.validate_tool_result(response, True, "SANDBOX_DENIED"), structured
        )
        incoherent = copy.deepcopy(response)
        incoherent["result"]["content"][0]["text"] = json.dumps({"status": "passed"})
        with self.assertRaisesRegex(ValueError, "differ"):
            smoke.validate_tool_result(incoherent, True, "SANDBOX_DENIED")

        schema = {
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "additionalProperties": False,
        }
        definitions = [
            {
                "name": name,
                "description": "description",
                "inputSchema": dict(schema),
                "outputSchema": {**schema, "additionalProperties": False},
                "annotations": {
                    "readOnlyHint": True,
                    "destructiveHint": False,
                    "idempotentHint": True,
                    "openWorldHint": False,
                },
            }
            for name in smoke.TOOLS
        ]
        listing = {"result": {"resultType": "complete", "tools": definitions}}
        synthetic_hashes = {
            row["name"]: smoke.sha256(
                smoke.canonical_json(
                    {
                        "inputSchema": row["inputSchema"],
                        "outputSchema": row["outputSchema"],
                    }
                )
            )
            for row in definitions
        }
        with mock.patch.dict(smoke.TOOL_SCHEMA_SHA256, synthetic_hashes, clear=True):
            self.assertEqual(len(smoke.validate_tools(listing)), 13)
            listing["result"]["tools"] = definitions[:-1]
            with self.assertRaisesRegex(ValueError, "thirteen"):
                smoke.validate_tools(listing)

    def test_receipt_no_overwrite_preserves_existing_file(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            receipt = Path(directory) / "receipt.json"
            receipt.write_bytes(b"owner data")
            with self.assertRaises(FileExistsError):
                smoke.write_receipt(receipt, {"status": "passed"})
            self.assertEqual(receipt.read_bytes(), b"owner data")


if __name__ == "__main__":
    unittest.main(verbosity=2)
