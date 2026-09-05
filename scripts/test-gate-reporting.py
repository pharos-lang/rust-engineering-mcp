#!/usr/bin/env python3
"""Discriminating tests for gate receipt timestamps and direct test counts."""
import importlib.util
import io
from pathlib import Path
import sys
import unittest


def load_gate_module():
    path = Path(__file__).with_name("gate.py")
    spec = importlib.util.spec_from_file_location("rust_mcp_gate", path)
    if spec is None or spec.loader is None:
        raise RuntimeError("could not load gate.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


GATE = load_gate_module()


class GateReportingTests(unittest.TestCase):
    def test_rust_summary_preserves_each_counter(self):
        parsed = GATE.parse_test_summary_line(
            "test result: ok. 12 passed; 1 failed; 2 ignored; 3 measured; 4 filtered out"
        )
        self.assertEqual(
            parsed,
            {
                "runner": "rust-test-harness",
                "status": "ok",
                "passed": 12,
                "failed": 1,
                "ignored": 2,
                "measured": 3,
                "filtered_out": 4,
            },
        )

    def test_python_unittest_summary_is_counted(self):
        self.assertEqual(
            GATE.parse_test_summary_line("Ran 7 tests in 0.123s"),
            {"runner": "python-unittest", "executed": 7},
        )

    def test_unrelated_output_is_not_invented_as_a_count(self):
        self.assertIsNone(GATE.parse_test_summary_line("644 tests were expected"))

    def test_timestamp_is_explicit_utc(self):
        timestamp = GATE.utc_now()
        self.assertTrue(timestamp.endswith("Z"))
        self.assertIn("T", timestamp)

    def test_run_step_persists_v2_timestamps_and_direct_counts(self):
        report = {"schema": "rust-mcp-gate-report-v2", "steps": []}
        saved = []
        GATE.run_step(
            report,
            lambda: saved.append(len(report["steps"])),
            "stub-tests",
            [sys.executable, "-c", "print('Ran 3 tests in 0.001s')"],
            {},
            require_test_groups=True,
            output_stream=io.StringIO(),
        )
        self.assertGreaterEqual(len(saved), 2)
        row = report["steps"][0]
        self.assertEqual(row["status"], "passed")
        self.assertEqual(row["counts"]["python_unittest_executed"], 3)
        self.assertEqual(len(row["counts"]["test_groups"]), 1)
        self.assertTrue(row["started_at"].endswith("Z"))
        self.assertTrue(row["finished_at"].endswith("Z"))

    def test_required_test_summary_cannot_silently_disappear(self):
        report = {"schema": "rust-mcp-gate-report-v2", "steps": []}
        with self.assertRaisesRegex(RuntimeError, "evidence failed"):
            GATE.run_step(
                report,
                lambda: None,
                "missing-summary",
                [sys.executable, "-c", "print('looks fine')"],
                {},
                require_test_groups=True,
                output_stream=io.StringIO(),
            )
        self.assertEqual(report["steps"][0]["status"], "failed")
        self.assertIn("evidence_error", report["steps"][0])


if __name__ == "__main__":
    unittest.main()
