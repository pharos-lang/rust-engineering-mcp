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
SPEC = importlib.util.spec_from_file_location("m6_provision", MODULE_PATH)
assert SPEC and SPEC.loader
provision = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(provision)


def make_xz_archive(path: Path, members: list[tuple[str, bytes, str]]) -> None:
    with tarfile.open(path, "w:xz") as archive:
        for name, data, kind in members:
            member = tarfile.TarInfo(name)
            if kind == "file":
                member.size = len(data)
                archive.addfile(member, io.BytesIO(data))
            elif kind == "dir":
                member.type = tarfile.DIRTYPE
                archive.addfile(member)
            elif kind == "link":
                member.type = tarfile.SYMTYPE
                member.linkname = data.decode()
                archive.addfile(member)
            elif kind == "hardlink":
                member.type = tarfile.LNKTYPE
                member.linkname = data.decode()
                archive.addfile(member)
            elif kind == "chr":
                member.type = tarfile.CHRTYPE
                member.devmajor = 0
                member.devminor = 0
                archive.addfile(member)
            elif kind == "blk":
                member.type = tarfile.BLKTYPE
                member.devmajor = 0
                member.devminor = 0
                archive.addfile(member)
            elif kind == "fifo":
                member.type = tarfile.FIFOTYPE
                archive.addfile(member)
            else:
                raise ValueError(f"unknown fixture member kind: {kind!r}")


class ManifestParsingTests(unittest.TestCase):
    def test_parses_and_verifies_pinned_entries(self) -> None:
        text = (
            '[pkg.rust-analyzer-preview.target.aarch64-unknown-linux-gnu]\n'
            f'xz_url = "{provision.RUST_ANALYZER["url"]}"\n'
            f'xz_hash = "{provision.RUST_ANALYZER["sha256"]}"\n'
            '[pkg.rust-src.target."*"]\n'
            f'xz_url = "{provision.RUST_SRC["url"]}"\n'
            f'xz_hash = "{provision.RUST_SRC["sha256"]}"\n'
        )
        entries = provision.parse_manifest_entries(text)
        provision.verify_manifest_entries(entries)  # must not raise

    def test_missing_package_fails_closed(self) -> None:
        with self.assertRaisesRegex(ValueError, "manifest missing package"):
            provision.parse_manifest_entries("")

    def test_missing_target_fails_closed(self) -> None:
        text = '[pkg.rust-analyzer-preview.target.other-target]\nxz_url = "x"\nxz_hash = "y"\n'
        with self.assertRaisesRegex(ValueError, "manifest missing target"):
            provision.parse_manifest_entries(text, inputs=(provision.RUST_ANALYZER,))

    def test_url_mismatch_rejected(self) -> None:
        entries = {
            (provision.RUST_ANALYZER["pkg"], provision.RUST_ANALYZER["manifest_target"]): {
                "xz_url": "https://static.rust-lang.org/dist/attacker.tar.xz",
                "xz_hash": provision.RUST_ANALYZER["sha256"],
            }
        }
        with self.assertRaisesRegex(ValueError, "xz_url"):
            provision.verify_manifest_entries(entries, inputs=(provision.RUST_ANALYZER,))

    def test_hash_mismatch_rejected(self) -> None:
        entries = {
            (provision.RUST_ANALYZER["pkg"], provision.RUST_ANALYZER["manifest_target"]): {
                "xz_url": provision.RUST_ANALYZER["url"],
                "xz_hash": "0" * 64,
            }
        }
        with self.assertRaisesRegex(ValueError, "xz_hash"):
            provision.verify_manifest_entries(entries, inputs=(provision.RUST_ANALYZER,))


class DownloadTests(unittest.TestCase):
    def test_skips_network_when_bytes_already_verified(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / "artifact.tar.xz"
            destination.write_bytes(b"payload")
            digest = hashlib.sha256(b"payload").hexdigest()

            def unreachable(_url: str) -> bytes:
                raise AssertionError("must not fetch when the cached bytes already verify")

            result = provision.ensure_downloaded("https://example/x", digest, destination, unreachable)
            self.assertEqual(result, destination)

    def test_downloads_and_verifies_new_bytes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / "nested" / "artifact.tar.xz"
            digest = hashlib.sha256(b"payload").hexdigest()
            result = provision.ensure_downloaded(
                "https://example/x", digest, destination, lambda _url: b"payload"
            )
            self.assertEqual(result.read_bytes(), b"payload")
            self.assertFalse(result.with_name(result.name + ".part").exists())

    def test_checksum_mismatch_after_download_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / "artifact.tar.xz"
            with self.assertRaisesRegex(ValueError, "checksum mismatch"):
                provision.ensure_downloaded(
                    "https://example/x", "0" * 64, destination, lambda _url: b"payload"
                )

    def test_stale_cached_bytes_are_redownloaded(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / "artifact.tar.xz"
            destination.write_bytes(b"stale")
            digest = hashlib.sha256(b"fresh").hexdigest()
            result = provision.ensure_downloaded(
                "https://example/x", digest, destination, lambda _url: b"fresh"
            )
            self.assertEqual(result.read_bytes(), b"fresh")

    def test_fetch_url_refuses_hosts_outside_the_pin(self) -> None:
        with self.assertRaisesRegex(ValueError, "refusing to fetch"):
            provision.fetch_url("https://evil.example/dist/x.tar.xz")


class ArchiveValidationTests(unittest.TestCase):
    def test_valid_archive_counts_members(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "sample.tar.xz"
            make_xz_archive(
                archive,
                [
                    ("root", b"", "dir"),
                    ("root/install.sh", b"#!/bin/sh\n", "file"),
                    ("root/LICENSE-MIT", b"MIT", "file"),
                ],
            )
            self.assertEqual(provision.validate_component_archive(archive, "root"), 3)

    def test_traversal_wrong_root_and_links_are_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)

            traversal = root / "traversal.tar.xz"
            make_xz_archive(traversal, [("../escape", b"x", "file")])
            with self.assertRaisesRegex(ValueError, "unsafe archive path"):
                provision.validate_component_archive(traversal, "root")

            wrong_root = root / "wrong-root.tar.xz"
            make_xz_archive(wrong_root, [("other/file", b"x", "file")])
            with self.assertRaisesRegex(ValueError, "outside expected root"):
                provision.validate_component_archive(wrong_root, "root")

            linked = root / "linked.tar.xz"
            make_xz_archive(linked, [("root/link", b"/etc/passwd", "link")])
            with self.assertRaisesRegex(ValueError, "linked or special"):
                provision.validate_component_archive(linked, "root")

    def test_hardlink_member_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "hardlink.tar.xz"
            make_xz_archive(
                archive,
                [
                    ("root", b"", "dir"),
                    ("root/real", b"data", "file"),
                    ("root/hardlink", b"root/real", "hardlink"),
                ],
            )
            with self.assertRaisesRegex(ValueError, "linked or special"):
                provision.validate_component_archive(archive, "root")

    def test_character_device_member_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "chardev.tar.xz"
            make_xz_archive(archive, [("root", b"", "dir"), ("root/dev", b"", "chr")])
            with self.assertRaisesRegex(ValueError, "linked or special"):
                provision.validate_component_archive(archive, "root")

    def test_block_device_member_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "blockdev.tar.xz"
            make_xz_archive(archive, [("root", b"", "dir"), ("root/dev", b"", "blk")])
            with self.assertRaisesRegex(ValueError, "linked or special"):
                provision.validate_component_archive(archive, "root")

    def test_fifo_member_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "fifo.tar.xz"
            make_xz_archive(archive, [("root", b"", "dir"), ("root/pipe", b"", "fifo")])
            with self.assertRaisesRegex(ValueError, "linked or special"):
                provision.validate_component_archive(archive, "root")

    def test_duplicate_member_name_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "duplicate.tar.xz"
            make_xz_archive(
                archive,
                [
                    ("root", b"", "dir"),
                    ("root/file", b"first", "file"),
                    ("root/file", b"second", "file"),
                ],
            )
            with self.assertRaisesRegex(ValueError, "duplicate archive member"):
                provision.validate_component_archive(archive, "root")

    def test_backslash_in_member_name_under_correct_root_is_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "backslash.tar.xz"
            make_xz_archive(archive, [("root", b"", "dir"), ("root/foo\\bar", b"x", "file")])
            with self.assertRaisesRegex(ValueError, "unsafe archive path"):
                provision.validate_component_archive(archive, "root")

    def test_empty_archive_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "empty.tar.xz"
            with tarfile.open(archive, "w:xz"):
                pass
            with self.assertRaisesRegex(ValueError, "empty component archive"):
                provision.validate_component_archive(archive, "root")


class ArchiveSizeBoundsTests(unittest.TestCase):
    def test_member_exceeding_the_per_member_cap_is_rejected(self) -> None:
        original = provision.MAX_MEMBER_BYTES
        provision.MAX_MEMBER_BYTES = 10
        try:
            with tempfile.TemporaryDirectory() as directory:
                archive = Path(directory) / "oversized-member.tar.xz"
                make_xz_archive(archive, [("root", b"", "dir"), ("root/file", b"x" * 11, "file")])
                with self.assertRaisesRegex(ValueError, "archive member too large"):
                    provision.validate_component_archive(archive, "root")
        finally:
            provision.MAX_MEMBER_BYTES = original

    def test_aggregate_size_exceeding_the_archive_cap_is_rejected(self) -> None:
        original = provision.MAX_ARCHIVE_BYTES
        provision.MAX_ARCHIVE_BYTES = 10
        try:
            with tempfile.TemporaryDirectory() as directory:
                archive = Path(directory) / "oversized-archive.tar.xz"
                make_xz_archive(
                    archive,
                    [
                        ("root", b"", "dir"),
                        ("root/a", b"x" * 6, "file"),
                        ("root/b", b"x" * 6, "file"),
                    ],
                )
                with self.assertRaisesRegex(ValueError, "archive too large"):
                    provision.validate_component_archive(archive, "root")
        finally:
            provision.MAX_ARCHIVE_BYTES = original


class OutputTaintTests(unittest.TestCase):
    def test_traversal_in_output_argument_cannot_escape_the_default_directory(self) -> None:
        result = provision.beside_default(provision.OUTPUT_DEFAULT, "../../../etc/passwd")
        self.assertEqual(result, provision.OUTPUT_DEFAULT.parent / "passwd")


class NoticeExtractionTests(unittest.TestCase):
    def test_extracts_only_present_root_notices(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "sample.tar.xz"
            make_xz_archive(
                archive,
                [
                    ("root/LICENSE-APACHE", b"APACHE", "file"),
                    ("root/LICENSE-MIT", b"MIT", "file"),
                    ("root/nested/COPYRIGHT", b"nested, not root", "file"),
                ],
            )
            destination = Path(directory) / "notices"
            collected = provision.extract_root_notices(archive, "root", destination)
            self.assertEqual(sorted(collected), ["LICENSE-APACHE", "LICENSE-MIT"])
            self.assertEqual((destination / "LICENSE-APACHE").read_bytes(), b"APACHE")
            self.assertFalse((destination / "COPYRIGHT").exists())


class ContextValidationTests(unittest.TestCase):
    def test_rejects_extra_files_and_symlinks(self) -> None:
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

    def test_rejects_unexpected_directory(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            context = Path(directory)
            (context / "Dockerfile").write_text("FROM scratch")
            (context / "stray").mkdir()
            with self.assertRaisesRegex(ValueError, "unexpected build-context directory"):
                provision.validate_context(context, {"Dockerfile"}, complete=False)

    def test_incomplete_context_rejected(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            context = Path(directory)
            with self.assertRaisesRegex(ValueError, "incomplete build context"):
                provision.validate_context(context, {"Dockerfile"}, complete=True)


class DockerfileShapeTests(unittest.TestCase):
    def test_dockerfile_fixes_base_platform_user_path_and_workdir(self) -> None:
        dockerfile = (provision.HERE / "Dockerfile").read_text()
        self.assertIn("ARG BASE_IMAGE=rust-engineering-runtime:1.98.1-arm64-m5", dockerfile)
        self.assertIn(provision.BASE_IMAGE_ID, dockerfile)
        self.assertIn("USER 65534:65534", dockerfile)
        self.assertIn("WORKDIR /work", dockerfile)
        self.assertIn(
            "ENV PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
            dockerfile,
        )


class PrepareIntegrationTests(unittest.TestCase):
    """Exercises the full assembly pipeline with fabricated, offline inputs.

    Real download hashes cannot be forged, so this drives `prepare()` through
    its `inputs=`/`manifest_sha256=` injection points with synthetic specs of
    the exact same shape, instead of the module's real pinned constants.
    """

    def test_prepare_assembles_a_complete_verified_context(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fake_a_bytes = io.BytesIO()
            with tarfile.open(fileobj=fake_a_bytes, mode="w:xz") as archive:
                info = tarfile.TarInfo("fake-a-root/install.sh")
                info.size = len(b"#!/bin/sh\n")
                archive.addfile(info, io.BytesIO(b"#!/bin/sh\n"))
                info = tarfile.TarInfo("fake-a-root/LICENSE-MIT")
                info.size = len(b"MIT")
                archive.addfile(info, io.BytesIO(b"MIT"))
            fake_a = fake_a_bytes.getvalue()

            fake_b_bytes = io.BytesIO()
            with tarfile.open(fileobj=fake_b_bytes, mode="w:xz") as archive:
                info = tarfile.TarInfo("fake-b-root/install.sh")
                info.size = len(b"#!/bin/sh\n")
                archive.addfile(info, io.BytesIO(b"#!/bin/sh\n"))
            fake_b = fake_b_bytes.getvalue()

            input_a = {
                "pkg": "rust-analyzer-preview",
                "manifest_target": "aarch64-unknown-linux-gnu",
                "url": provision.DIST_HOST_PREFIX + "fake-a.tar.xz",
                "sha256": hashlib.sha256(fake_a).hexdigest(),
                "archive_root": "fake-a-root",
                "license": "MIT OR Apache-2.0",
                "label": "rust-analyzer",
            }
            input_b = {
                "pkg": "rust-src",
                "manifest_target": "*",
                "url": provision.DIST_HOST_PREFIX + "fake-b.tar.xz",
                "sha256": hashlib.sha256(fake_b).hexdigest(),
                "archive_root": "fake-b-root",
                "license": "MIT OR Apache-2.0",
                "label": "rust-src",
            }
            fake_inputs = (input_a, input_b)
            manifest_url = provision.DIST_HOST_PREFIX + "fake-channel.toml"
            manifest_text = (
                '[pkg.rust-analyzer-preview.target.aarch64-unknown-linux-gnu]\n'
                f'xz_url = "{input_a["url"]}"\n'
                f'xz_hash = "{input_a["sha256"]}"\n'
                '[pkg.rust-src.target."*"]\n'
                f'xz_url = "{input_b["url"]}"\n'
                f'xz_hash = "{input_b["sha256"]}"\n'
            )
            manifest_bytes = manifest_text.encode()
            manifest_sha256 = hashlib.sha256(manifest_bytes).hexdigest()

            payloads = {
                manifest_url: manifest_bytes,
                input_a["url"]: fake_a,
                input_b["url"]: fake_b,
            }

            def fake_fetch(url: str) -> bytes:
                return payloads[url]

            output = root / "m6-provisioning"
            receipt = provision.prepare(
                output,
                fetch=fake_fetch,
                manifest_url=manifest_url,
                manifest_sha256=manifest_sha256,
                inputs=fake_inputs,
            )

            self.assertEqual(receipt["schema"], "rust-engineering-mcp.m6-prepare.v1")
            self.assertEqual(receipt["status"], "prepared_not_built")
            self.assertTrue(receipt["manifest_sha256_verified"])
            self.assertEqual(len(receipt["inputs"]), 2)

            context = output / "build-context"
            self.assertTrue((context / "Dockerfile").is_file())
            self.assertTrue((context / "build.sh").is_file())
            self.assertTrue((context / "fake-a.tar.xz").is_file())
            self.assertTrue((context / "fake-b.tar.xz").is_file())
            self.assertTrue((context / "notices" / "rust-analyzer" / "LICENSE-MIT").is_file())
            self.assertTrue((context / "SHA256SUMS").is_file())
            self.assertTrue((context / "build-inputs.json").is_file())

            build_inputs = json.loads((context / "build-inputs.json").read_text())
            self.assertTrue(build_inputs["network_required"])
            self.assertEqual(build_inputs["manifest"]["sha256"], manifest_sha256)

            # A second run with the same fetch must not need it again: every
            # byte on disk already verifies against the pinned hashes.
            def unreachable(_url: str) -> bytes:
                raise AssertionError("second run must not re-fetch verified bytes")

            receipt_again = provision.prepare(
                output,
                fetch=unreachable,
                manifest_url=manifest_url,
                manifest_sha256=manifest_sha256,
                inputs=fake_inputs,
            )
            self.assertEqual(receipt_again["context_sha256s_digest"], receipt["context_sha256s_digest"])


if __name__ == "__main__":
    unittest.main()
