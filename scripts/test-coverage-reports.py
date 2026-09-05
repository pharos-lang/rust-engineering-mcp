#!/usr/bin/env python3
"""Tests for the fixed-input SonarCloud coverage report validator."""

from contextlib import chdir, redirect_stdout
import importlib.util
from io import StringIO
from pathlib import Path
import tempfile
import unittest


SCRIPT = Path(__file__).with_name("check-coverage-reports.py")
SPEC = importlib.util.spec_from_file_location("check_coverage_reports", SCRIPT)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load coverage validator: {SCRIPT}")
COVERAGE_REPORTS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(COVERAGE_REPORTS)


class CoverageReportTests(unittest.TestCase):
    def test_valid_reports_return_exact_totals_and_main_uses_fixed_paths(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            coverage = root / "coverage"
            coverage.mkdir()
            rust = coverage / "rust.lcov"
            python = coverage / "python.xml"
            rust.write_text(
                "SF:src/lib.rs\nLF:2\nLH:1\nSF:src/main.rs\nLF:3\nLH:2\n",
                encoding="utf-8",
            )
            python.write_text(
                '<coverage lines-valid="4" lines-covered="3"/>', encoding="utf-8"
            )

            self.assertEqual(COVERAGE_REPORTS.validate_lcov(rust), (3, 5))
            self.assertEqual(COVERAGE_REPORTS.validate_cobertura(python), (3, 4))
            output = StringIO()
            with chdir(root), redirect_stdout(output):
                COVERAGE_REPORTS.main()
            self.assertEqual(
                output.getvalue(),
                "coverage inputs valid: Rust 3/5 lines; Python 3/4 lines\n",
            )

    def test_lcov_rejects_empty_and_inconsistent_counters(self) -> None:
        invalid_reports = (
            "",
            "SF:src/lib.rs\nLF:0\nLH:0\n",
            "SF:src/lib.rs\nLF:1\nLH:-1\n",
            "SF:src/lib.rs\nLF:1\nLH:2\n",
        )
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "rust.lcov"
            for report in invalid_reports:
                with self.subTest(report=report):
                    path.write_text(report, encoding="utf-8")
                    with self.assertRaises(RuntimeError):
                        COVERAGE_REPORTS.validate_lcov(path)

    def test_cobertura_rejects_wrong_shape_and_inconsistent_counters(self) -> None:
        invalid_reports = (
            "<report/>",
            '<coverage lines-valid="0" lines-covered="0"/>',
            '<coverage lines-valid="1" lines-covered="-1"/>',
            '<coverage lines-valid="1" lines-covered="2"/>',
        )
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "python.xml"
            for report in invalid_reports:
                with self.subTest(report=report):
                    path.write_text(report, encoding="utf-8")
                    with self.assertRaises(RuntimeError):
                        COVERAGE_REPORTS.validate_cobertura(path)


if __name__ == "__main__":
    unittest.main()
