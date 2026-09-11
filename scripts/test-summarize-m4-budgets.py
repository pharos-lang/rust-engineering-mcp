#!/usr/bin/env python3
"""Portable contract test for the M4 budget summarizer."""
from __future__ import annotations

import hashlib
import importlib.util
import json
import pathlib
import tempfile
import unittest
from unittest import mock


SUBJECT = pathlib.Path(__file__).with_name("summarize-m4-budgets.py")
SPEC = importlib.util.spec_from_file_location("summarize_m4_budgets", SUBJECT)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("M4 budget summarizer unavailable")
M4 = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(M4)


class SummarizeM4BudgetsTests(unittest.TestCase):
    def test_main_validates_every_sample_and_publishes_bound_inputs(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            target = root / "target"
            target.mkdir()
            (root / "docs" / "validation").mkdir(parents=True)
            measurements = []
            for tool_index, tool in enumerate(sorted(M4.TOOLS), start=1):
                for temperature in ("cold", "warm"):
                    for sample in range(30):
                        measurements.append({
                            "tool": tool,
                            "temperature": temperature,
                            "sample": sample,
                            "elapsed_ms": tool_index * 100 + sample,
                            "reply_bytes": 1024 + sample,
                        })
            receipt = {
                "status": "passed",
                "samples_each": 30,
                "binary_sha256": "sha256:" + "a" * 64,
                "image_id": "sha256:" + "b" * 64,
                "calibration_excluded_from_operation_timer": True,
                "cold_definition": "fresh process",
                "measurements": measurements,
            }
            inputs = {"binary_sha256": "a" * 64}
            measured_path = target / "m4-budgets.json"
            inputs_path = target / "m4-budgets-inputs.json"
            measured_path.write_text(json.dumps(receipt))
            inputs_path.write_text(json.dumps(inputs))

            with mock.patch.object(M4, "ROOT", root):
                M4.main()

            destination = root / "docs" / "validation" / "M4"
            summary = json.loads((destination / "budgets.json").read_text())
            self.assertEqual(summary["status"], "passed")
            self.assertEqual(len(summary["groups"]), 10)
            self.assertEqual(summary["groups"][0]["samples"], 30)
            self.assertEqual(summary["groups"][0]["p99_ms"], 129)
            self.assertEqual(
                summary["outputs"][0]["sha256"],
                hashlib.sha256(measured_path.read_bytes()).hexdigest(),
            )
            self.assertEqual(
                (destination / "budgets" / measured_path.name).read_bytes(),
                measured_path.read_bytes(),
            )


if __name__ == "__main__":
    unittest.main()
