#!/usr/bin/env python3
"""Tests for scripts/contract-freeze.py: manifest hashing, verify, diff."""
from __future__ import annotations

import contextlib
import hashlib
import importlib.util
import io
import json
import pathlib
import shutil
import tempfile
import unittest
from unittest import mock

ROOT = pathlib.Path(__file__).resolve().parents[1]


def _load(name: str, path: pathlib.Path):
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


CF = _load("rust_mcp_contract_freeze", ROOT / "scripts/contract-freeze.py")

STABLE_NAME = "rust.example.stable"
PREVIEW_NAME = "rust.analyzer.symbols"


def make_tool(
    name: str,
    description: str = "does a thing",
    extra_prop: str = "a",
    read_only_hint: bool = True,
    output_prop: str = "summary",
) -> dict:
    return {
        "name": name,
        "description": description,
        "annotations": {
            "destructiveHint": False,
            "idempotentHint": False,
            "openWorldHint": False,
            "readOnlyHint": read_only_hint,
        },
        "inputSchema": {"type": "object", "properties": {extra_prop: {"type": "string"}}},
        "outputSchema": {"type": "object", "properties": {output_prop: {"type": "string"}}},
    }


def write_tool(directory: pathlib.Path, filename: str, spec: dict) -> None:
    (directory / filename).write_text(json.dumps(spec, indent=2))


def run_capturing_stdout(func, *args, **kwargs):
    buffer = io.StringIO()
    with contextlib.redirect_stdout(buffer):
        code = func(*args, **kwargs)
    return code, buffer.getvalue()


def last_json_line(output: str) -> dict:
    return json.loads(output.strip().splitlines()[-1])


class SnapshotFixture:
    def __init__(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.dir = pathlib.Path(self.temp.name).resolve()
        write_tool(self.dir, "stable-tool.json", make_tool(STABLE_NAME))
        write_tool(self.dir, "analyzer-symbols-tool.json", make_tool(PREVIEW_NAME))

    def close(self) -> None:
        self.temp.cleanup()


class CanonicalHashTests(unittest.TestCase):
    def test_hash_matches_a_fixed_vector(self) -> None:
        value = {"b": 1, "a": 2}
        expected = hashlib.sha256(b'{"a":2,"b":1}').hexdigest()
        self.assertEqual(CF.canonical_hash(value), expected)

    def test_hash_of_a_string_matches_a_fixed_vector(self) -> None:
        expected = hashlib.sha256(b'"hello"').hexdigest()
        self.assertEqual(CF.canonical_hash("hello"), expected)

    def test_hash_of_a_non_ascii_value_matches_a_fixed_vector(self) -> None:
        value = {"café": "ñ"}
        expected = hashlib.sha256('{"café":"ñ"}'.encode("utf-8")).hexdigest()
        self.assertEqual(CF.canonical_hash(value), expected)


class GenerateVerifyTests(unittest.TestCase):
    def setUp(self) -> None:
        self.fixture = SnapshotFixture()
        self.addCleanup(self.fixture.close)
        self.manifest_path = self.fixture.dir / "manifest.json"
        patches = [
            mock.patch.object(CF, "SNAPSHOTS_DIR", self.fixture.dir),
            mock.patch.object(CF, "FREEZE_MANIFEST_PATH", self.manifest_path),
            mock.patch.object(CF, "head_commit", lambda: "cafefeed"),
            mock.patch.object(CF, "tree_is_dirty", lambda: False),
        ]
        for patcher in patches:
            patcher.start()
            self.addCleanup(patcher.stop)

    def generate(self) -> None:
        code, _ = run_capturing_stdout(CF.cmd_generate)
        self.assertEqual(code, 0)

    def test_generate_then_verify_passes(self) -> None:
        self.generate()
        code, output = run_capturing_stdout(CF.cmd_verify, False)
        self.assertEqual(code, 0)
        summary = last_json_line(output)
        self.assertEqual(
            summary,
            {
                "status": "passed",
                "format_errors": [],
                "class_changed": [],
                "stable_changed": [],
                "preview_changed": [],
            },
        )

    def test_manifest_records_expected_shape(self) -> None:
        self.generate()
        manifest = json.loads(self.manifest_path.read_text())
        self.assertEqual(manifest["tool_count"], 2)
        self.assertEqual(manifest["stable_count"], 1)
        self.assertEqual(manifest["preview_count"], 1)
        self.assertEqual(manifest["tools"][STABLE_NAME]["stability"], "stable")
        self.assertEqual(manifest["tools"][PREVIEW_NAME]["stability"], "preview")
        self.assertEqual(manifest["canonical"], CF.CANONICAL_DESCRIPTION)

    def test_stable_schema_change_fails_verify(self) -> None:
        self.generate()
        write_tool(self.fixture.dir, "stable-tool.json", make_tool(STABLE_NAME, extra_prop="b"))
        code, output = run_capturing_stdout(CF.cmd_verify, False)
        self.assertEqual(code, 1)
        summary = last_json_line(output)
        self.assertEqual(summary["status"], "failed")
        self.assertEqual(len(summary["stable_changed"]), 1)
        self.assertIn(STABLE_NAME, summary["stable_changed"][0])
        self.assertEqual(summary["preview_changed"], [])

    def test_preview_description_change_warns_but_passes_without_strict(self) -> None:
        self.generate()
        write_tool(self.fixture.dir, "analyzer-symbols-tool.json", make_tool(PREVIEW_NAME, description="new text"))
        code, output = run_capturing_stdout(CF.cmd_verify, False)
        self.assertEqual(code, 0)
        summary = last_json_line(output)
        self.assertEqual(summary["status"], "passed")
        self.assertEqual(summary["stable_changed"], [])
        self.assertEqual(len(summary["preview_changed"]), 1)

    def test_preview_description_change_fails_with_strict(self) -> None:
        self.generate()
        write_tool(self.fixture.dir, "analyzer-symbols-tool.json", make_tool(PREVIEW_NAME, description="new text"))
        code, output = run_capturing_stdout(CF.cmd_verify, True)
        self.assertEqual(code, 1)
        summary = last_json_line(output)
        self.assertEqual(summary["status"], "failed")

    def test_new_tool_fails_verify(self) -> None:
        self.generate()
        write_tool(self.fixture.dir, "extra-tool.json", make_tool("rust.example.extra"))
        code, output = run_capturing_stdout(CF.cmd_verify, False)
        self.assertEqual(code, 1)
        summary = last_json_line(output)
        self.assertEqual(summary["status"], "failed")
        self.assertEqual(len(summary["stable_changed"]), 1)
        self.assertIn("rust.example.extra", summary["stable_changed"][0])

    def test_removed_tool_fails_verify(self) -> None:
        self.generate()
        (self.fixture.dir / "stable-tool.json").unlink()
        code, output = run_capturing_stdout(CF.cmd_verify, False)
        self.assertEqual(code, 1)
        summary = last_json_line(output)
        self.assertEqual(summary["status"], "failed")
        self.assertIn(STABLE_NAME, summary["stable_changed"][0])

    def test_stable_description_change_fails_verify(self) -> None:
        self.generate()
        write_tool(self.fixture.dir, "stable-tool.json", make_tool(STABLE_NAME, description="new text"))
        code, output = run_capturing_stdout(CF.cmd_verify, False)
        self.assertEqual(code, 1)
        summary = last_json_line(output)
        self.assertEqual(summary["status"], "failed")
        self.assertIn(STABLE_NAME, summary["stable_changed"][0])

    def test_stable_annotations_change_fails_verify(self) -> None:
        self.generate()
        write_tool(self.fixture.dir, "stable-tool.json", make_tool(STABLE_NAME, read_only_hint=False))
        code, output = run_capturing_stdout(CF.cmd_verify, False)
        self.assertEqual(code, 1)
        summary = last_json_line(output)
        self.assertEqual(summary["status"], "failed")
        self.assertIn(STABLE_NAME, summary["stable_changed"][0])

    def test_stable_output_schema_change_fails_verify(self) -> None:
        self.generate()
        write_tool(self.fixture.dir, "stable-tool.json", make_tool(STABLE_NAME, output_prop="other"))
        code, output = run_capturing_stdout(CF.cmd_verify, False)
        self.assertEqual(code, 1)
        summary = last_json_line(output)
        self.assertEqual(summary["status"], "failed")
        self.assertIn(STABLE_NAME, summary["stable_changed"][0])

    def test_count_only_mismatch_fails_verify(self) -> None:
        self.generate()
        manifest = json.loads(self.manifest_path.read_text())
        manifest["tool_count"] = manifest["tool_count"] + 1
        self.manifest_path.write_text(json.dumps(manifest))
        code, output = run_capturing_stdout(CF.cmd_verify, False)
        self.assertEqual(code, 1)
        summary = last_json_line(output)
        self.assertEqual(summary["status"], "failed")
        self.assertEqual(summary["stable_changed"], [])
        self.assertEqual(summary["class_changed"], [])

    def test_reclassified_tool_fails_verify_even_without_strict(self) -> None:
        self.generate()
        patcher = mock.patch.object(CF, "PREVIEW_NAMES", CF.PREVIEW_NAMES | {STABLE_NAME})
        patcher.start()
        self.addCleanup(patcher.stop)
        write_tool(self.fixture.dir, "stable-tool.json", make_tool(STABLE_NAME, extra_prop="b"))
        code, output = run_capturing_stdout(CF.cmd_verify, False)
        self.assertEqual(code, 1)
        summary = last_json_line(output)
        self.assertEqual(summary["status"], "failed")
        self.assertEqual(len(summary["class_changed"]), 1)
        self.assertIn(STABLE_NAME, summary["class_changed"][0])

    def test_missing_manifest_fails_verify_with_clear_message(self) -> None:
        buffer = io.StringIO()
        with contextlib.redirect_stderr(buffer):
            code = CF.cmd_verify(False)
        self.assertEqual(code, 1)
        self.assertIn(str(self.manifest_path), buffer.getvalue())

    def test_format_version_mismatch_fails_verify(self) -> None:
        self.generate()
        manifest = json.loads(self.manifest_path.read_text())
        manifest["format_version"] = 2
        self.manifest_path.write_text(json.dumps(manifest))
        code, output = run_capturing_stdout(CF.cmd_verify, False)
        self.assertEqual(code, 1)
        summary = last_json_line(output)
        self.assertEqual(summary["status"], "failed")
        self.assertEqual(len(summary["format_errors"]), 1)

    def test_canonical_mismatch_fails_verify(self) -> None:
        self.generate()
        manifest = json.loads(self.manifest_path.read_text())
        manifest["canonical"] = "not the canonical description"
        self.manifest_path.write_text(json.dumps(manifest))
        code, output = run_capturing_stdout(CF.cmd_verify, False)
        self.assertEqual(code, 1)
        summary = last_json_line(output)
        self.assertEqual(summary["status"], "failed")
        self.assertEqual(len(summary["format_errors"]), 1)


KEEP_NAME = "rust.example.keep"
CHANGE_NAME = "rust.example.change"
REMOVE_NAME = "rust.example.remove"
ADD_NAME = "rust.example.add"


class DiffTests(unittest.TestCase):
    """Hermetic: no real git invocation and no dependency on the working tree."""

    OUT_KEY = "since_test"

    def setUp(self) -> None:
        self.fixture_dir = pathlib.Path(tempfile.mkdtemp()).resolve()
        self.addCleanup(shutil.rmtree, self.fixture_dir, ignore_errors=True)
        write_tool(self.fixture_dir, "keep-tool.json", make_tool(KEEP_NAME))
        write_tool(self.fixture_dir, "change-tool.json", make_tool(CHANGE_NAME, extra_prop="b"))
        write_tool(self.fixture_dir, "add-tool.json", make_tool(ADD_NAME))

        self.out_path = self.fixture_dir / "diff.json"

        self.base_snapshots = {
            f"{CF.SNAPSHOTS_RELATIVE}/keep-tool.json": (self.fixture_dir / "keep-tool.json").read_bytes(),
            f"{CF.SNAPSHOTS_RELATIVE}/change-tool.json": json.dumps(
                make_tool(CHANGE_NAME, extra_prop="a"), indent=2
            ).encode("utf-8"),
            f"{CF.SNAPSHOTS_RELATIVE}/remove-tool.json": json.dumps(make_tool(REMOVE_NAME), indent=2).encode(
                "utf-8"
            ),
        }

        def fake_git_bytes(*args: str) -> bytes:
            self.assertEqual(args[0], "show")
            spec = args[-1]
            _, _, path = spec.partition(":")
            return self.base_snapshots[path]

        def fake_snapshot_names_at_commit(commit: str) -> list[str]:
            return sorted(self.base_snapshots)

        patches = [
            mock.patch.object(CF, "SNAPSHOTS_DIR", self.fixture_dir),
            mock.patch.object(CF, "SCHEMA_DIFF_PATH", self.out_path),
            mock.patch.object(CF, "DIFF_OUT_KEYS", frozenset({self.OUT_KEY})),
            mock.patch.object(CF, "resolve_commit", lambda ref: "deadbeef"),
            mock.patch.object(CF, "snapshot_names_at_commit", fake_snapshot_names_at_commit),
            mock.patch.object(CF, "git_bytes", fake_git_bytes),
            mock.patch.object(CF, "head_commit", lambda: "cafefeed"),
            mock.patch.object(CF, "tree_is_dirty", lambda: False),
        ]
        for patcher in patches:
            patcher.start()
            self.addCleanup(patcher.stop)

    def run_diff(self, payload: dict) -> tuple[int, str]:
        with mock.patch.object(CF.sys, "stdin", io.StringIO(json.dumps(payload))):
            return run_capturing_stdout(CF.cmd_diff)

    def test_diff_reports_added_removed_changed_and_unchanged_tools(self) -> None:
        code, _ = self.run_diff({"base": "deadbeef", "out": self.OUT_KEY})
        self.assertEqual(code, 0)
        result = json.loads(self.out_path.read_text())[self.OUT_KEY]

        self.assertEqual(result["base_commit"], "deadbeef")
        self.assertEqual(result["head_commit"], "cafefeed")
        self.assertEqual(result["added"], [ADD_NAME])
        self.assertEqual(result["removed"], [REMOVE_NAME])

        self.assertEqual(len(result["changed"]), 1)
        changed = result["changed"][0]
        self.assertEqual(changed["name"], CHANGE_NAME)
        self.assertEqual(changed["keys_changed"], ["inputSchema"])
        self.assertTrue(changed["input_schema_changed"])
        self.assertFalse(changed["output_schema_changed"])
        self.assertFalse(changed["annotations_changed"])
        self.assertFalse(changed["bytes_identical"])

        self.assertEqual(len(result["unchanged"]), 1)
        unchanged = result["unchanged"][0]
        self.assertEqual(unchanged["name"], KEEP_NAME)
        self.assertTrue(unchanged["bytes_identical"])

    def test_only_restricts_the_diff_to_named_tools(self) -> None:
        code, _ = self.run_diff({"base": "deadbeef", "out": self.OUT_KEY, "only": [KEEP_NAME]})
        self.assertEqual(code, 0)
        result = json.loads(self.out_path.read_text())[self.OUT_KEY]
        self.assertEqual(result["added"], [])
        self.assertEqual(result["removed"], [])
        self.assertEqual(result["changed"], [])
        self.assertEqual([row["name"] for row in result["unchanged"]], [KEEP_NAME])

    def test_a_second_out_key_merges_into_the_shared_destination(self) -> None:
        other_key = "since_other"
        with mock.patch.object(CF, "DIFF_OUT_KEYS", frozenset({self.OUT_KEY, other_key})):
            code, _ = self.run_diff({"base": "deadbeef", "out": self.OUT_KEY})
            self.assertEqual(code, 0)
            code, _ = self.run_diff({"base": "deadbeef", "out": other_key})
            self.assertEqual(code, 0)
        payload = json.loads(self.out_path.read_text())
        self.assertIn(self.OUT_KEY, payload)
        self.assertIn(other_key, payload)

    def test_invalid_base_is_rejected(self) -> None:
        with self.assertRaises(SystemExit):
            self.run_diff({"base": "HEAD", "out": self.OUT_KEY})

    def test_unknown_out_key_is_rejected(self) -> None:
        with self.assertRaises(SystemExit):
            self.run_diff({"base": "deadbeef", "out": "not-a-real-destination"})

    def test_non_object_stdin_is_rejected(self) -> None:
        with mock.patch.object(CF.sys, "stdin", io.StringIO("[1, 2, 3]")):
            with self.assertRaises(SystemExit):
                CF.cmd_diff()

    def test_malformed_json_stdin_is_rejected(self) -> None:
        with mock.patch.object(CF.sys, "stdin", io.StringIO("not json")):
            with self.assertRaises(SystemExit):
                CF.cmd_diff()

    def test_existing_diff_file_with_an_unknown_top_level_key_is_rejected(self) -> None:
        self.out_path.write_text(json.dumps({"not-a-real-destination": {}}))
        with self.assertRaises(SystemExit):
            self.run_diff({"base": "deadbeef", "out": self.OUT_KEY})

    def test_existing_diff_file_with_an_unknown_entry_field_is_rejected(self) -> None:
        corrupt_entry = {
            "base": "deadbeef",
            "base_commit": "deadbeef",
            "head_commit": "cafefeed",
            "tree_dirty": False,
            "added": [],
            "removed": [],
            "changed": [],
            "unchanged": [],
            "unexpected_field": "surprise",
        }
        self.out_path.write_text(json.dumps({self.OUT_KEY: corrupt_entry}))
        with self.assertRaises(SystemExit):
            self.run_diff({"base": "deadbeef", "out": self.OUT_KEY})

    def test_existing_diff_file_with_a_wrong_typed_field_is_rejected(self) -> None:
        corrupt_entry = {
            "base": "deadbeef",
            "base_commit": "deadbeef",
            "head_commit": "cafefeed",
            "tree_dirty": "not-a-bool",
            "added": [],
            "removed": [],
            "changed": [],
            "unchanged": [],
        }
        self.out_path.write_text(json.dumps({self.OUT_KEY: corrupt_entry}))
        with self.assertRaises(SystemExit):
            self.run_diff({"base": "deadbeef", "out": self.OUT_KEY})

    def test_existing_diff_file_with_an_unknown_changed_row_key_is_rejected(self) -> None:
        corrupt_entry = {
            "base": "deadbeef",
            "base_commit": "deadbeef",
            "head_commit": "cafefeed",
            "tree_dirty": False,
            "added": [],
            "removed": [],
            "changed": [
                {
                    "name": CHANGE_NAME,
                    "keys_changed": ["inputSchema"],
                    "input_schema_changed": True,
                    "output_schema_changed": False,
                    "annotations_changed": False,
                    "bytes_identical": False,
                    "unexpected_field": "surprise",
                }
            ],
            "unchanged": [],
        }
        self.out_path.write_text(json.dumps({self.OUT_KEY: corrupt_entry}))
        with self.assertRaises(SystemExit):
            self.run_diff({"base": "deadbeef", "out": self.OUT_KEY})

    def test_existing_diff_file_preserves_a_valid_entry_when_merging_another_key(self) -> None:
        other_key = "since_other"
        with mock.patch.object(CF, "DIFF_OUT_KEYS", frozenset({self.OUT_KEY, other_key})):
            code, _ = self.run_diff({"base": "deadbeef", "out": self.OUT_KEY})
            self.assertEqual(code, 0)
            before = json.loads(self.out_path.read_text())[self.OUT_KEY]
            code, _ = self.run_diff({"base": "deadbeef", "out": other_key})
            self.assertEqual(code, 0)
        after = json.loads(self.out_path.read_text())[self.OUT_KEY]
        self.assertEqual(before, after)


class RealRepositoryClassificationTests(unittest.TestCase):
    def test_classes_cover_exactly_the_36_real_tool_names(self) -> None:
        current = CF.load_current_tools()
        self.assertEqual(len(current), 36)
        stable = {name for name, entry in current.items() if entry["stability"] == "stable"}
        preview = {name for name, entry in current.items() if entry["stability"] == "preview"}
        self.assertEqual(len(stable), 31)
        self.assertEqual(preview, CF.PREVIEW_NAMES)
        self.assertEqual(stable | preview, set(current))


if __name__ == "__main__":
    unittest.main()
