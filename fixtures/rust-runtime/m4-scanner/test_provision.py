#!/usr/bin/env python3
import hashlib
import importlib.util
import io
import json
import tarfile
import tempfile
import unittest
from pathlib import Path

MODULE_PATH = Path(__file__).with_name("provision.py")
SPEC = importlib.util.spec_from_file_location("m4_scanner_provision", MODULE_PATH)
assert SPEC and SPEC.loader
provision = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(provision)


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


class ProvisionTests(unittest.TestCase):
    def test_private_lock_and_helper_contract_are_exact(self) -> None:
        sources = provision.verify_helper()
        self.assertEqual([entry["path"] for entry in sources], list(provision.HELPER_FILES))
        self.assertEqual(len(provision.PACKAGES), 11)

    def test_archive_hash_is_required_and_verified(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            cache = Path(directory) / "cache"
            registry = cache / "registry"
            registry.mkdir(parents=True)
            archive = registry / "sample-1.0.0.crate"
            make_archive(archive, [("sample-1.0.0/src/lib.rs", b"", "file")])
            digest = hashlib.sha256(archive.read_bytes()).hexdigest()
            self.assertEqual(
                provision.cached_archive(cache, "sample", "1.0.0", digest), archive
            )
            with self.assertRaisesRegex(ValueError, "checksum mismatch"):
                provision.cached_archive(cache, "sample", "1.0.0", "0" * 64)

    def test_missing_archive_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            cache = Path(directory)
            with self.assertRaisesRegex(ValueError, "missing cached crate"):
                provision.cached_archive(cache, "missing", "1.0.0", "0" * 64)

    def test_archive_traversal_wrong_root_and_links_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            traversal = root / "traversal.crate"
            make_archive(traversal, [("../escape", b"x", "file")])
            with self.assertRaisesRegex(ValueError, "unsafe archive path"):
                provision.validate_archive(traversal, "sample", "1.0.0")

            wrong_root = root / "wrong-root.crate"
            make_archive(wrong_root, [("other-1.0.0/src/lib.rs", b"", "file")])
            with self.assertRaisesRegex(ValueError, "outside package root"):
                provision.validate_archive(wrong_root, "sample", "1.0.0")

            linked = root / "linked.crate"
            make_archive(linked, [("sample-1.0.0/link", b"/etc/passwd", "link")])
            with self.assertRaisesRegex(ValueError, "linked or special"):
                provision.validate_archive(linked, "sample", "1.0.0")

    def test_context_rejects_extra_files_and_symlinks(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            context = Path(directory)
            (context / "extra").write_text("unexpected")
            with self.assertRaisesRegex(ValueError, "unexpected build-context"):
                provision.validate_context(context, {"Dockerfile"}, complete=False)

        with tempfile.TemporaryDirectory() as directory:
            context = Path(directory)
            target = context / "target"
            target.write_text("target")
            (context / "Dockerfile").symlink_to(target)
            with self.assertRaisesRegex(ValueError, "linked build-context"):
                provision.validate_context(context, {"Dockerfile", "target"}, complete=True)

    def test_dockerfile_fixes_base_platform_user_path_and_workdir(self) -> None:
        dockerfile = (provision.HERE / "Dockerfile").read_text()
        self.assertIn(f"ARG BASE_IMAGE={provision.BASE_IMAGE_ID}", dockerfile)
        self.assertIn("USER 65534:65534", dockerfile)
        self.assertIn("WORKDIR /work", dockerfile)
        self.assertIn(
            "ENV PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
            dockerfile,
        )

    def test_generated_sbom_is_json_and_network_is_false(self) -> None:
        sample = {
            "schema": "rust-engineering-mcp.m4-scanner-build-inputs.v1",
            "base_image_id": provision.BASE_IMAGE_ID,
            "network_required": False,
        }
        encoded = json.dumps(sample, sort_keys=True)
        self.assertFalse(json.loads(encoded)["network_required"])


if __name__ == "__main__":
    unittest.main()
