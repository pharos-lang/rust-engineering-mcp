#!/usr/bin/env python3
"""Benign unit tests for the prepared M4 stock-client harness."""
from __future__ import annotations

import importlib.util
import json
import os
import pathlib
import shutil
import subprocess
import tempfile
import unittest
from unittest import mock

ROOT = pathlib.Path(__file__).resolve().parents[1]
SUBJECT = ROOT / "scripts/test-m4-clients.py"
SPEC = importlib.util.spec_from_file_location("m4_clients", SUBJECT)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("M4 harness unavailable")
M4 = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(M4)


class HarnessTests(unittest.TestCase):
    def test_inventory_is_exact_extension_of_m3(self):
        self.assertEqual(M4.EXPECTED_TOOLS[:22], M4.M3_TOOLS)
        self.assertEqual(M4.EXPECTED_TOOLS[22:], M4.M4_TOOLS)
        self.assertEqual(len(M4.EXPECTED_TOOLS), 27)
        self.assertEqual(len(set(M4.EXPECTED_TOOLS)), 27)

    def test_preflight_is_non_executing_and_source_bound(self):
        result = M4.preflight()
        self.assertFalse(result["execution_performed"])
        self.assertEqual(result["image_id"], M4.IMAGE)
        self.assertEqual(result["clients"]["inspector"]["expected"], "2.5.0")
        self.assertEqual(result["clients"]["codex_app_server"]["expected"], "codex-cli 0.153.0")
        self.assertEqual(set(result["source_sha256"]), {
            "scripts/test-m3-clients.py", "scripts/m3-inspector-session.mjs",
            "scripts/codex-model-qualifier.py", "scripts/test-m4-clients.py",
            "scripts/m4-inspector-session.mjs", "scripts/test-m4-clients-unit.py",
            "docs/validation/m1-17-codex-client/controller.py",
        })
        self.assertTrue(all(len(value) == 64 for value in result["source_sha256"].values()))

    def test_current_switches_are_recognized_without_overriding_them(self):
        state = M4.advertisement_state()
        self.assertEqual(tuple(state), M4.M4_TOOLS)
        self.assertTrue(all(isinstance(value, bool) for value in state.values()))

    def test_run_is_closed_before_any_client_when_advertisement_is_incomplete(self):
        state = dict.fromkeys(M4.M4_TOOLS, False)
        with mock.patch.object(M4, "advertisement_state", return_value=state):
            with self.assertRaisesRegex(RuntimeError, "advertisement switches"):
                M4.run("/private/tmp/nonexistent-m4-unit.sock")

    def test_metadata_validator_accepts_only_m3_proxy_shape(self):
        row = {"client":"inspector","direction":"client","session":"a"*32,
               "bytes":42,"sha256":"b"*64,"method":"tools/call","tool":"rust.miri"}
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / "protocol.jsonl"
            path.write_text(json.dumps(row, separators=(",", ":")) + "\n")
            # protocol_summary requires initialize declarations for each client.
            initialize = {"client":"inspector","direction":"client","session":"a"*32,
                          "bytes":10,"sha256":"c"*64,"method":"initialize","tasks_declared":True}
            response = {"client":"inspector","direction":"server","session":"a"*32,
                        "bytes":10,"sha256":"d"*64,"tasks_advertised":True}
            path.write_text("\n".join(json.dumps(item,separators=(",",":")) for item in
                                      (initialize,response,row)) + "\n")
            self.assertTrue(M4.validate_protocol_metadata(path)["metadata_only"])

    def test_metadata_validator_rejects_payload_and_credentials(self):
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / "protocol.jsonl"
            path.write_text(json.dumps({"client":"x","direction":"client","session":"s",
                "bytes":1,"sha256":"a"*64,"arguments":{"authorization":"secret"}}) + "\n")
            with self.assertRaisesRegex(RuntimeError, "unapproved keys"):
                M4.validate_protocol_metadata(path)

    def test_ustar_header_matches_closed_reader_shape(self):
        header = M4.tar_header("manifest.json", 17)
        self.assertEqual(len(header), 512)
        self.assertEqual(header[257:265], b"ustar\0" + b"00")
        self.assertEqual(header[156], ord("0"))
        expected = int(header[148:156].decode().strip("\0 "), 8)
        actual = sum(32 if 148 <= i < 156 else byte for i, byte in enumerate(header))
        self.assertEqual(expected, actual)

    def test_fresh_fixture_catalog_authenticates_offline(self):
        root = pathlib.Path(tempfile.mkdtemp(prefix="m4-catalog-unit-", dir="/private/tmp"))
        os.chmod(root, 0o700)
        try:
            bundle = M4.fresh_catalog_bundle(root)
            store = root / "store"
            store.mkdir(mode=0o700)
            trust = root / "trust.json"
            shutil.copyfile(M4.ROOT / "fixtures/catalog/fixture-trust.json", trust)
            os.chmod(trust, 0o600)
            result = subprocess.run([str(M4.SERVER), "catalog", "import", str(bundle),
                "--store", str(store), "--trust", str(trust), "--json"],
                capture_output=True, text=True, timeout=30, check=False)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            report = json.loads(result.stdout)
            self.assertEqual(report["status"], "passed")
            self.assertEqual(report["catalog"]["sequence"], 7)
            self.assertEqual(report["catalog"]["evidence"]["freshness"]["state"], "fresh")
            self.assertFalse(report["network_used"])
        finally:
            shutil.rmtree(root)


if __name__ == "__main__":
    unittest.main()
