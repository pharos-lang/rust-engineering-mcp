#!/usr/bin/env python3
"""Focused tests for public snapshot revision argument containment."""

import importlib.util
from pathlib import Path
import unittest


def load_exporter():
    path = Path(__file__).with_name("public-export.py")
    spec = importlib.util.spec_from_file_location("rust_mcp_public_export", path)
    if spec is None or spec.loader is None:
        raise RuntimeError("could not load public-export.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


EXPORTER = load_exporter()


class PublicExportTests(unittest.TestCase):
    def test_bounded_refs_and_full_object_ids_are_accepted(self):
        for value in ["HEAD", "main", "refs/tags/v0.1.0", "a" * 40]:
            with self.subTest(value=value):
                self.assertEqual(EXPORTER.validate_commitish(value), value)

    def test_options_ranges_lockfiles_and_pathological_refs_are_rejected(self):
        for value in [
            "--upload-pack=evil",
            "main..other",
            "refs//heads/main",
            "refs/heads/main/",
            "refs/heads/main.lock",
            "refs/heads/with space",
            "a" * 202,
        ]:
            with self.subTest(value=value):
                with self.assertRaisesRegex(ValueError, "bounded Git ref"):
                    EXPORTER.validate_commitish(value)

    def test_git_resolves_head_to_exactly_one_object_id(self):
        observed = EXPORTER.git(
            "rev-parse", "--verify", "--end-of-options", "HEAD^{commit}"
        ).decode().strip()
        self.assertRegex(observed, r"\A[0-9a-f]{40}\Z")


if __name__ == "__main__":
    unittest.main()
