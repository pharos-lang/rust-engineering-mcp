#!/usr/bin/env python3
"""Portable in-process tests for release-inventory.py."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock


SCRIPT = Path(__file__).with_name("release-inventory.py")
SPEC = importlib.util.spec_from_file_location("release_inventory", SCRIPT)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class ReleaseInventoryTests(unittest.TestCase):
    def test_small_helpers_have_stable_classification(self):
        self.assertEqual(
            MODULE.sha(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        )
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with mock.patch.object(MODULE, "ROOT", root):
                self.assertEqual(MODULE.location(root / "Cargo.toml"), "Cargo.toml")
                self.assertEqual(
                    MODULE.location(Path("/cache/registry/src/index/pkg/LICENSE")),
                    "$CARGO_REGISTRY_SRC/index/pkg/LICENSE",
                )
                self.assertEqual(MODULE.package_id(f"path+file://{root}#demo"),
                                 "path+file://$WORKSPACE#demo")
        self.assertEqual(MODULE.text_kind("NOTICE.txt"), "notice")
        self.assertEqual(MODULE.text_kind("LICENSE-MIT"), "license_or_copying")

    def test_text_scan_is_recursive_bounded_and_does_not_follow_links(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "package"
            nested = root / "nested"
            nested.mkdir(parents=True)
            (root / "LICENSE").write_text("license")
            (nested / "Third-Party-Notices.txt").write_text("notice")
            (nested / "ordinary.txt").write_text("ignored")
            (root / "target").mkdir()
            (root / "target" / "LICENSE").write_text("ignored target")
            outside = Path(temporary) / "OUTSIDE-LICENSE"
            outside.write_text("outside")
            found, skipped = MODULE.texts(root, outside)
            self.assertEqual(
                [path.relative_to(root).as_posix() for path, _ in found],
                ["LICENSE", "nested/Third-Party-Notices.txt"],
            )
            self.assertEqual(
                skipped,
                [{
                    "path": str(outside),
                    "reason": "declared_license_file_outside_selected_package_not_read",
                }],
            )

    def test_main_generates_and_rechecks_a_minimal_inventory(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            output = root / "out"
            output.mkdir()
            crate = root / "crates" / "demo"
            crate.mkdir(parents=True)
            manifest = crate / "Cargo.toml"
            manifest.write_text(
                "[package]\nname='demo'\nversion='0.1.0'\nlicense='MIT OR Apache-2.0'\n"
            )
            (root / "Cargo.lock").write_text(
                "version = 4\n[[package]]\nname = 'demo'\nversion = '0.1.0'\n"
            )
            for name in ["LICENSE", "LICENSE-APACHE", "LICENSE-MIT", "NOTICE"]:
                (root / name).write_text(f"{name} bytes\n")
            vendor = root / "vendor"
            vendor.mkdir()
            (vendor / "lancedb-manifest-only.patch").write_text("patch\n")
            receipt_path = root / "fixtures" / "semantic" / "model-receipt.json"
            receipt_path.parent.mkdir(parents=True)
            receipt_path.write_text(json.dumps({
                "repository": "intfloat/e5-small-v2",
                "revision": "fixed",
                "license_provenance": {"publisher_declared_spdx": "MIT"},
                "files": [{"path": "model.onnx", "sha256": "00"}],
            }))
            ort = root / "native"
            ort.mkdir()
            (ort / "libonnxruntime.a").write_bytes(b"archive")
            (ort / "LICENSE").write_text("native license\n")

            package_id = f"path+file://{root}#demo@0.1.0"
            metadata = {
                "packages": [{
                    "id": package_id,
                    "name": "demo",
                    "version": "0.1.0",
                    "source": None,
                    "manifest_path": str(manifest),
                    "license": "MIT OR Apache-2.0",
                    "license_file": None,
                    "targets": [{"kind": ["lib"]}],
                }],
                "workspace_members": [package_id],
                "resolve": {"nodes": [{
                    "id": package_id,
                    "features": ["default"],
                    "deps": [],
                }]},
            }

            def command(*args):
                if args[:3] == ("git", "rev-parse", "HEAD"):
                    return "a" * 40
                if args and args[0] == "cargo":
                    return json.dumps(metadata)
                if args and args[0] == "rustc":
                    return "rustc 1.98.1\nhost: aarch64-apple-darwin"
                self.fail(f"unexpected command: {args}")

            def run(check):
                arguments = [str(SCRIPT), "--ort-dir", str(ort), "--output", str(output)]
                if check:
                    arguments.append("--check")
                with mock.patch.object(MODULE, "ROOT", root), \
                     mock.patch.object(MODULE, "command", side_effect=command), \
                     mock.patch.object(sys, "argv", arguments), \
                     contextlib.redirect_stdout(io.StringIO()) as stdout:
                    MODULE.main()
                self.assertIn("resolved_packages", stdout.getvalue())

            run(False)
            inventory = json.loads((output / "inventory.json").read_text())
            self.assertEqual(inventory["git_commit"], "a" * 40)
            self.assertEqual(inventory["summary"]["resolved_packages"], 1)
            self.assertEqual(inventory["summary"]["third_party_packages"], 0)
            self.assertEqual(inventory["packages"][0]["id"],
                             "path+file://$WORKSPACE#demo@0.1.0")
            self.assertEqual(
                inventory["native_and_model"]["onnxruntime"]["sha256"],
                MODULE.sha(b"archive"),
            )
            self.assertTrue((output / "THIRD_PARTY_NOTICES.candidate.txt").is_file())
            run(True)


if __name__ == "__main__":
    unittest.main()
