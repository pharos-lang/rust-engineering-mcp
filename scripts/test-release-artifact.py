#!/usr/bin/env python3
"""Focused no-network/no-build tests for release-artifact.py."""

from __future__ import annotations

import importlib.util
import io
import json
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest
from unittest import mock


SCRIPT = Path(__file__).with_name("release-artifact.py")
SPEC = importlib.util.spec_from_file_location("release_artifact", SCRIPT)
release = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
sys.modules[SPEC.name] = release
SPEC.loader.exec_module(release)


def graph_metadata(version: str = "0.1.0") -> dict[str, object]:
    root = "path+file:///work/crates/mcp-server#rust-engineering-mcp@" + version
    normal = "registry+https://example.invalid#index#normal@1.0.0"
    build = "registry+https://example.invalid#index#build@1.0.0"
    dev = "registry+https://example.invalid#index#dev@1.0.0"
    transitive = "registry+https://example.invalid#index#transitive@1.0.0"
    return {
        "workspace_members": [root],
        "packages": [
            {"id": root, "name": "rust-engineering-mcp", "version": version},
            {"id": normal, "name": "normal", "version": "1.0.0"},
            {"id": build, "name": "build", "version": "1.0.0"},
            {"id": dev, "name": "dev", "version": "1.0.0"},
            {"id": transitive, "name": "transitive", "version": "1.0.0"},
        ],
        "resolve": {"nodes": [
            {"id": root, "features": [], "deps": [
                {"pkg": normal, "dep_kinds": [{"kind": None, "target": None}]},
                {"pkg": build, "dep_kinds": [{"kind": "build", "target": None}]},
                {"pkg": dev, "dep_kinds": [{"kind": "dev", "target": None}]},
            ]},
            {"id": normal, "features": [], "deps": [
                {"pkg": transitive, "dep_kinds": [{"kind": None, "target": None}]}
            ]},
            {"id": build, "features": [], "deps": []},
            {"id": dev, "features": [], "deps": []},
            {"id": transitive, "features": [], "deps": []},
        ]},
    }


class ReleaseArtifactTests(unittest.TestCase):
    def test_graph_parser_keeps_normal_build_and_excludes_dev(self) -> None:
        metadata = graph_metadata()
        root = release.root_package_id(metadata)
        closure, roles, edges = release.dependency_closure(metadata, root)
        names = {item["name"] for item in metadata["packages"] if item["id"] in closure}
        self.assertEqual(names, {"rust-engineering-mcp", "normal", "build", "transitive"})
        build_id = next(item["id"] for item in metadata["packages"] if item["name"] == "build")
        self.assertEqual(roles[build_id], {"build"})
        self.assertEqual(len(edges), 3)

    def test_tag_must_match_stable_workspace_version(self) -> None:
        metadata = graph_metadata("0.1.1")
        root = release.root_package_id(metadata)
        with self.assertRaisesRegex(ValueError, "does not match"):
            release.validate_tag(metadata, "v0.1.0", root)
        with self.assertRaisesRegex(ValueError, "stable semantic"):
            release.validate_tag(metadata, "v0.1.1-rc.1", root)

    def test_read_regular_rejects_symlink(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target = root / "binary"
            target.write_bytes(b"binary")
            link = root / "link"
            link.symlink_to(target)
            with self.assertRaises(ValueError):
                release.read_regular(link, 1024)

    def test_license_names_are_exact_and_missing_text_fails_closed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "Cargo.toml").write_text("[package]\nname='fixture'\nversion='1.0.0'\n")
            (root / "license.rs").write_text("not a license text")
            (root / "notice_test").write_text("not a notice text")
            package = {
                "name": "fixture",
                "version": "1.0.0",
                "manifest_path": str(root / "Cargo.toml"),
                "license_file": None,
            }
            texts = release.package_texts(package, root, False)
            self.assertEqual(texts, [])
            with self.assertRaisesRegex(ValueError, "fixture 1.0.0"):
                release.require_exact_licenses(["fixture 1.0.0"])
            (root / "COPYRIGHT").write_text("manifest-declared license bytes")
            package["license_file"] = str(root / "COPYRIGHT")
            texts = release.package_texts(package, root, False)
            self.assertEqual(len(texts), 1)
            self.assertIn("manifest-license-file", texts[0][0])
            self.assertEqual(texts[0][2], "license_or_copying")
        for name in ("LICENSE", "LICENSE-MIT", "LICENSE.Apache-2.0", "LICENSE.txt"):
            self.assertIsNotNone(release.TEXT_NAME.fullmatch(name))
        for name in (
            "license.rs",
            "notice_test",
            "LICENSE.rs",
            "LICENSE-vendor.rs",
            "NOTICE-fixture.py",
        ):
            self.assertIsNone(release.TEXT_NAME.fullmatch(name))

    def test_packaging_host_and_thin_macho_arm64_are_required(self) -> None:
        with mock.patch.object(release.platform, "system", return_value="Linux"):
            with self.assertRaisesRegex(ValueError, "Darwin arm64"):
                release.validate_host()
        with mock.patch.object(release.platform, "system", return_value="Darwin"), mock.patch.object(
            release.platform, "machine", return_value="x86_64"
        ):
            with self.assertRaisesRegex(ValueError, "Darwin arm64"):
                release.validate_host()
        arm64 = (
            b"\xcf\xfa\xed\xfe"
            + release.MACHO_ARM64_CPU.to_bytes(4, "little")
            + b"\x00" * 4
            + release.MACHO_EXECUTE.to_bytes(4, "little")
        )
        release.validate_macho_arm64(arm64)
        x86_64 = (
            b"\xcf\xfa\xed\xfe"
            + (0x01000007).to_bytes(4, "little")
            + b"\x00" * 4
            + release.MACHO_EXECUTE.to_bytes(4, "little")
        )
        with self.assertRaisesRegex(ValueError, "not arm64"):
            release.validate_macho_arm64(x86_64)
        dylib = arm64[:12] + (6).to_bytes(4, "little")
        with self.assertRaisesRegex(ValueError, "MH_EXECUTE"):
            release.validate_macho_arm64(dylib)
        with self.assertRaisesRegex(ValueError, "thin"):
            release.validate_macho_arm64(b"\xca\xfe\xba\xbe" + b"\x00" * 12)

    def test_archive_verifier_rejects_traversal(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "bad.tar.gz"
            with tarfile.open(archive, "w:gz") as handle:
                info = tarfile.TarInfo("prefix/../escape")
                info.size = 1
                handle.addfile(info, io.BytesIO(b"x"))
            with self.assertRaisesRegex(ValueError, "unsafe archive path"):
                release.verify_archive(archive, "prefix", [])

    def test_archive_verifier_rejects_duplicates_types_and_hash_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            duplicate = root / "duplicate.tar.gz"
            with tarfile.open(duplicate, "w:gz") as handle:
                for _ in range(2):
                    info = tarfile.TarInfo("prefix/member")
                    info.size = 1
                    handle.addfile(info, io.BytesIO(b"x"))
            with self.assertRaisesRegex(ValueError, "duplicate archive member"):
                release.verify_archive(duplicate, "prefix", [release.Member("member", b"x")])

            nonregular = root / "nonregular.tar.gz"
            with tarfile.open(nonregular, "w:gz") as handle:
                info = tarfile.TarInfo("prefix/member")
                info.type = tarfile.SYMTYPE
                info.linkname = "elsewhere"
                handle.addfile(info)
            with self.assertRaisesRegex(ValueError, "non-regular member"):
                release.verify_archive(nonregular, "prefix", [release.Member("member", b"")])

            mismatched = root / "mismatched.tar.gz"
            with tarfile.open(mismatched, "w:gz") as handle:
                info = tarfile.TarInfo("prefix/member")
                info.size = 1
                handle.addfile(info, io.BytesIO(b"y"))
            with self.assertRaisesRegex(ValueError, "hash mismatch"):
                release.verify_archive(mismatched, "prefix", [release.Member("member", b"x")])

            named_owner = root / "named-owner.tar.gz"
            with tarfile.open(named_owner, "w:gz") as handle:
                info = tarfile.TarInfo("prefix/member")
                info.size = 1
                info.uname = "builder"
                handle.addfile(info, io.BytesIO(b"x"))
            with self.assertRaisesRegex(ValueError, "owner names"):
                release.verify_archive(named_owner, "prefix", [release.Member("member", b"x")])

    def test_deterministic_archive_bytes(self) -> None:
        members = [release.Member("binary", b"payload", 0o755), release.Member("NOTICE", b"notice")]
        with tempfile.TemporaryDirectory() as directory:
            first = Path(directory) / "first.tar.gz"
            second = Path(directory) / "second.tar.gz"
            release.write_deterministic_archive(first, "artifact", members)
            release.write_deterministic_archive(second, "artifact", list(reversed(members)))
            self.assertEqual(first.read_bytes(), second.read_bytes())

    def test_core_asset_exclusion_is_fail_closed(self) -> None:
        with self.assertRaisesRegex(ValueError, "entered core closure"):
            release.ensure_core_only(["serde", "ort-sys", "kanaria"])
        inventory = {"packages": [{"name": "serde"}]}
        manifest = {
            "schema": "rust-engineering-mcp-release-manifest-v1",
            "hash_algorithm": "SHA-256",
            "members": [],
        }
        members = [
            release.Member("inventory.json", release.canonical_json(inventory)),
            release.Member("model.onnx", b"forbidden"),
        ]
        manifest["members"] = [
            *[item.row() for item in sorted(members, key=lambda item: item.path)],
            {
                "path": "MANIFEST.json",
                "bytes": None,
                "sha256": None,
                "mode": "0644",
                "self_reference": "size-and-hash-not-representable-inside-member",
            },
        ]
        members.append(release.Member("MANIFEST.json", release.canonical_json(manifest)))
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "asset.tar.gz"
            release.write_deterministic_archive(archive, "artifact", members)
            with self.assertRaisesRegex(ValueError, "prohibited"):
                release.verify_archive(archive, "artifact", members)

    def test_safe_relative_rejects_noncanonical_paths(self) -> None:
        for value in ("../x", "/absolute", "a/../b", "a\\b", "a/./b"):
            with self.subTest(value=value), self.assertRaises(ValueError):
                release.safe_relative(value)

    def test_partial_publication_removes_only_own_archive_link(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            candidate = root / "candidate"
            sum_candidate = root / "sum-candidate"
            archive = root / "archive"
            sums = root / "SHA256SUMS"
            candidate.write_bytes(b"archive")
            sum_candidate.write_bytes(b"sum")
            real_link = release.os.link
            calls = 0

            def fail_second(source, destination, **kwargs):
                nonlocal calls
                calls += 1
                if calls == 2:
                    raise OSError("injected checksum publication failure")
                return real_link(source, destination, **kwargs)

            with mock.patch.object(release.os, "link", side_effect=fail_second):
                with self.assertRaisesRegex(OSError, "injected"):
                    release.publish_pair(candidate, sum_candidate, archive, sums)
            self.assertFalse(archive.exists())
            self.assertFalse(sums.exists())
            self.assertEqual(candidate.read_bytes(), b"archive")


if __name__ == "__main__":
    unittest.main(verbosity=2)
