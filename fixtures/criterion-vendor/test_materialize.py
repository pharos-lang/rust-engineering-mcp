#!/usr/bin/env python3
"""Tests for the criterion vendor materializer.

    python3 -B -m unittest discover -s fixtures/criterion-vendor -p 'test_*.py'
"""

import hashlib
import importlib.util
import io
import json
import tarfile
import tempfile
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("materialize.py")
SPEC = importlib.util.spec_from_file_location("criterion_vendor_materialize", MODULE_PATH)
assert SPEC and SPEC.loader
materialize = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(materialize)

HERE = Path(__file__).resolve().parent


def make_archive(path: Path, members: list[tuple[str, bytes, str]]) -> None:
    with tarfile.open(path, "w:gz") as archive:
        for name, data, kind in members:
            member = tarfile.TarInfo(name)
            if kind == "file":
                member.size = len(data)
                archive.addfile(member, io.BytesIO(data))
            else:
                member.type = tarfile.SYMTYPE
                member.linkname = data.decode()
                archive.addfile(member)


def sha256_bytes(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def manifest_for(root: Path, name: str, version: str, files: int, size: int) -> Path:
    archive = root / f"{name}-{version}.crate"
    digest = sha256_bytes(archive)
    document = {
        "schema": materialize.SCHEMA,
        "target": "aarch64-unknown-linux-gnu",
        "criterion_version": "0.8.2",
        "network_used": False,
        "storage": "crate-archives",
        "packages": [
            {
                "name": name,
                "version": version,
                "sha256": digest,
                "archive_sha256": digest,
                "archive_bytes": archive.stat().st_size,
                "license": "MIT OR Apache-2.0",
                "files": files,
                "bytes": size,
            }
        ],
    }
    path = root / "INVENTORY.json"
    path.write_text(json.dumps(document, indent=2) + "\n")
    return path


class ValidateArchiveTests(unittest.TestCase):
    def test_parent_traversal_member_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "traversal.crate"
            make_archive(archive, [("../escape", b"x", "file")])
            with self.assertRaisesRegex(ValueError, "unsafe archive path"):
                materialize.validate_archive(archive, "sample", "1.0.0")

    def test_symlink_member_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "linked.crate"
            make_archive(archive, [("sample-1.0.0/link", b"/etc/passwd", "link")])
            with self.assertRaisesRegex(ValueError, "linked or special"):
                materialize.validate_archive(archive, "sample", "1.0.0")

    def test_member_outside_package_root_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "wrong-root.crate"
            make_archive(archive, [("other-1.0.0/src/lib.rs", b"", "file")])
            with self.assertRaisesRegex(ValueError, "outside package root"):
                materialize.validate_archive(archive, "sample", "1.0.0")

    def test_duplicate_member_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "duplicate.crate"
            make_archive(
                archive,
                [
                    ("sample-1.0.0/src/lib.rs", b"a", "file"),
                    ("sample-1.0.0/src/lib.rs", b"b", "file"),
                ],
            )
            with self.assertRaisesRegex(ValueError, "duplicate archive member"):
                materialize.validate_archive(archive, "sample", "1.0.0")

    def test_empty_archive_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "empty.crate"
            make_archive(archive, [])
            with self.assertRaisesRegex(ValueError, "empty crate archive"):
                materialize.validate_archive(archive, "sample", "1.0.0")


class VerifyTests(unittest.TestCase):
    def test_checksum_mismatch_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            make_archive(root / "sample-1.0.0.crate", [("sample-1.0.0/src/lib.rs", b"", "file")])
            manifest_path = manifest_for(root, "sample", "1.0.0", 1, 0)
            document = json.loads(manifest_path.read_text())
            document["packages"][0]["sha256"] = "0" * 64
            document["packages"][0]["archive_sha256"] = "0" * 64
            manifest_path.write_text(json.dumps(document))
            with self.assertRaisesRegex(ValueError, "archive checksum mismatch"):
                materialize.materialize(manifest_path, root / "vendor")
            self.assertFalse((root / "vendor").exists(), "nothing may be written on failure")

    def test_disagreeing_checksum_fields_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            make_archive(root / "sample-1.0.0.crate", [("sample-1.0.0/src/lib.rs", b"", "file")])
            manifest_path = manifest_for(root, "sample", "1.0.0", 1, 0)
            document = json.loads(manifest_path.read_text())
            document["packages"][0]["archive_sha256"] = "1" * 64
            manifest_path.write_text(json.dumps(document))
            with self.assertRaisesRegex(ValueError, "disagree"):
                materialize.materialize(manifest_path, root / "vendor")

    def test_missing_archive_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            make_archive(root / "sample-1.0.0.crate", [("sample-1.0.0/src/lib.rs", b"", "file")])
            manifest_path = manifest_for(root, "sample", "1.0.0", 1, 0)
            (root / "sample-1.0.0.crate").unlink()
            with self.assertRaisesRegex(ValueError, "missing pinned archive"):
                materialize.materialize(manifest_path, root / "vendor")

    def test_unexpected_schema_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest_path = root / "INVENTORY.json"
            manifest_path.write_text(json.dumps({"schema": "something.else", "packages": []}))
            with self.assertRaisesRegex(ValueError, "unexpected manifest schema"):
                materialize.materialize(manifest_path, root / "vendor")


class MaterializeTests(unittest.TestCase):
    def test_successful_materialization_writes_cargo_checksum(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            body = b"pub fn f() {}\n"
            make_archive(
                root / "sample-1.0.0.crate",
                [
                    ("sample-1.0.0/Cargo.toml", b"[package]\n", "file"),
                    ("sample-1.0.0/src/lib.rs", body, "file"),
                ],
            )
            manifest_path = manifest_for(root, "sample", "1.0.0", 2, len(b"[package]\n") + len(body))
            expected = json.loads(manifest_path.read_text())["packages"][0]["sha256"]

            receipt = materialize.materialize(manifest_path, root / "vendor")
            self.assertEqual(receipt["status"], "materialized")
            self.assertEqual(receipt["packages"], 1)
            self.assertFalse(receipt["network_used"])

            package_root = root / "vendor" / "sample-1.0.0"
            checksum = json.loads((package_root / ".cargo-checksum.json").read_text())
            self.assertEqual(checksum["package"], expected)
            self.assertEqual(
                sorted(checksum["files"]), ["Cargo.toml", "src/lib.rs"]
            )
            self.assertEqual(
                checksum["files"]["src/lib.rs"], hashlib.sha256(body).hexdigest()
            )
            self.assertNotIn(".cargo-checksum.json", checksum["files"])
            self.assertEqual((package_root / "src/lib.rs").read_bytes(), body)

    def test_manifest_file_and_byte_counts_are_enforced(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            make_archive(root / "sample-1.0.0.crate", [("sample-1.0.0/src/lib.rs", b"x", "file")])
            manifest_path = manifest_for(root, "sample", "1.0.0", 99, 1)
            with self.assertRaisesRegex(ValueError, "disagrees with the manifest"):
                materialize.materialize(manifest_path, root / "vendor")

    def test_verify_only_writes_nothing(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            make_archive(root / "sample-1.0.0.crate", [("sample-1.0.0/src/lib.rs", b"x", "file")])
            manifest_path = manifest_for(root, "sample", "1.0.0", 1, 1)
            receipt = materialize.verify_only(manifest_path)
            self.assertEqual(receipt["status"], "verified")
            self.assertEqual(receipt["packages"], 1)
            self.assertFalse((root / "vendor").exists())


class CommittedManifestTests(unittest.TestCase):
    def test_committed_archives_verify(self) -> None:
        receipt = materialize.verify_only(HERE / "INVENTORY.json")
        self.assertEqual(receipt["status"], "verified")
        self.assertEqual(receipt["packages"], 52)
        self.assertFalse(receipt["network_used"])

    def test_every_manifest_package_has_a_committed_archive(self) -> None:
        document = json.loads((HERE / "INVENTORY.json").read_text())
        self.assertEqual(document["storage"], "crate-archives")
        names = {f"{p['name']}-{p['version']}.crate" for p in document["packages"]}
        on_disk = {path.name for path in HERE.glob("*.crate")}
        self.assertEqual(names, on_disk)


if __name__ == "__main__":
    unittest.main()
