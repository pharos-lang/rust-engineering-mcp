#!/usr/bin/env python3
"""Discriminating tests for gate receipt timestamps and direct test counts."""
import importlib.util
import io
import os
import fnmatch
import subprocess
import tempfile
from pathlib import Path
import sys
import unittest
from unittest import mock


def load_gate_module():
    path = Path(__file__).with_name("gate.py")
    spec = importlib.util.spec_from_file_location("rust_mcp_gate", path)
    if spec is None or spec.loader is None:
        raise RuntimeError("could not load gate.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


GATE = load_gate_module()


def parse_sonar_properties(text):
    """Parse the key/value subset used by the repository's Sonar properties."""
    properties = {}
    for raw_line in text.splitlines():
        line = raw_line.strip()
        if not line or line.startswith(('#', '!')) or '=' not in line:
            continue
        key, value = line.split('=', 1)
        properties[key.strip()] = value.strip()
    return properties


def rust_source_exclusion_violations(root, properties_text):
    """Return Rust product sources hidden from analysis or coverage."""
    properties = parse_sonar_properties(properties_text)
    excluded = [
        pattern.strip()
        for key in ('sonar.exclusions', 'sonar.coverage.exclusions')
        for pattern in properties.get(key, '').split(',')
        if pattern.strip()
    ]
    sources = sorted(root.glob('crates/*/src/**/*.rs'))
    if not sources:
        raise AssertionError('Rust product source inventory is empty')
    return [
        source.relative_to(root).as_posix()
        for source in sources
        if any(
            fnmatch.fnmatchcase(source.relative_to(root).as_posix(), pattern)
            for pattern in excluded
        )
    ]


def isolated_git_fixture_env(root, inherited):
    """Keep Git executable discovery while isolating fixtures from host config."""
    allowed = ('PATH', 'PATHEXT', 'SYSTEMROOT', 'WINDIR', 'TMPDIR', 'TMP', 'TEMP')
    env = {key: inherited[key] for key in allowed if key in inherited}
    env.update(
        HOME=str(root),
        XDG_CONFIG_HOME=str(root / '.config'),
        GIT_CONFIG_NOSYSTEM='1',
        GIT_CONFIG_GLOBAL=os.devnull,
    )
    return env


class GateReportingTests(unittest.TestCase):
    def test_required_sonar_configuration_is_bound_through_real_git_inventory(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            excludes = root / 'host-excludes'
            excludes.write_text('*.properties\n')
            host_config = root / 'host-gitconfig'
            host_config.write_text(f'[core]\n\texcludesFile = {excludes}\n')
            polluted = dict(os.environ)
            polluted.update(
                GIT_DIR=str(root / 'wrong-git-dir'),
                GIT_INDEX_FILE=str(root / 'wrong-index'),
                GIT_CONFIG_GLOBAL=str(host_config),
                GIT_CONFIG_SYSTEM=str(host_config),
            )
            env = isolated_git_fixture_env(root, polluted)
            subprocess.run(['git', '-c', 'init.defaultBranch=fixture', 'init', '--quiet', str(root)],
                           check=True, capture_output=True, env=env)
            policy = root / 'sonar-project.properties'
            policy.write_text('sonar.coverage.exclusions=\n')
            before = GATE.source_inventory(root, env)
            self.assertEqual([row['path'] for row in before], ['sonar-project.properties'])
            policy.write_text('sonar.coverage.exclusions=crates/**\n')
            after = GATE.source_inventory(root, env)
            self.assertNotEqual(before[0]['sha256'], after[0]['sha256'])

    def test_portable_product_paths_cannot_be_excluded_from_required_coverage(self):
        root = Path(__file__).resolve().parents[1]
        self.assertEqual(
            [],
            rust_source_exclusion_violations(
                root,
                (root / 'sonar-project.properties').read_text(),
            ),
            'Rust product sources must remain in analysis and coverage',
        )

    def test_whitespace_cannot_hide_a_coverage_exclusion_from_the_oracle(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / 'crates' / 'fixture' / 'src' / 'lib.rs'
            source.parent.mkdir(parents=True)
            source.write_text('pub fn fixture() {}\n')
            self.assertEqual(
                ['crates/fixture/src/lib.rs'],
                rust_source_exclusion_violations(
                    root,
                    'sonar.coverage.exclusions = crates/**\n',
                ),
            )

    def test_global_analysis_exclusion_cannot_hide_any_rust_source(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / 'crates' / 'fixture' / 'src' / 'nested' / 'lib.rs'
            source.parent.mkdir(parents=True)
            source.write_text('pub fn fixture() {}\n')
            self.assertEqual(
                ['crates/fixture/src/nested/lib.rs'],
                rust_source_exclusion_violations(
                    root,
                    'sonar.exclusions=crates/fixture/src/**\n',
                ),
            )

    @unittest.skipIf(os.name == "nt", "native symlink fixture")
    def test_source_binding_detects_new_bytes_and_records_links_without_following(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / 'crates').mkdir()
            file = root / 'crates/source.rs'
            file.write_text('before')
            (root / 'crates/link.rs').symlink_to('/nonexistent/outside')
            names = b'crates/source.rs\0crates/link.rs\0docs/changing-report.json\0'
            with mock.patch.object(GATE.subprocess, 'check_output', return_value=names):
                before = GATE.source_inventory(root, {})
                file.write_text('after')
                after = GATE.source_inventory(root, {})
            self.assertEqual(len(before), 2)
            self.assertEqual(before[0]['kind'], 'symlink-target')
            self.assertEqual(before[0], after[0])
            self.assertNotEqual(before[1]['sha256'], after[1]['sha256'])

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

    def test_nonzero_process_is_persisted_before_failure(self):
        report = {"schema": "rust-mcp-gate-report-v2", "steps": []}
        with self.assertRaisesRegex(RuntimeError, r"failed \(7\)"):
            GATE.run_step(
                report,
                lambda: None,
                "failed-command",
                [sys.executable, "-c", "raise SystemExit(7)"],
                {},
                output_stream=io.StringIO(),
            )
        self.assertEqual(report["steps"][0]["status"], "failed")
        self.assertEqual(report["steps"][0]["exit_code"], 7)

    def test_default_output_and_missing_pipe_paths_are_explicit(self):
        report = {"schema": "rust-mcp-gate-report-v2", "steps": []}
        with mock.patch.object(GATE.sys, "stdout", io.StringIO()):
            GATE.run_step(
                report,
                lambda: None,
                "default-output",
                [sys.executable, "-c", "print('ok')"],
                {},
            )
        process = mock.Mock(stdout=None)
        with mock.patch.object(GATE.subprocess, "Popen", return_value=process):
            with self.assertRaisesRegex(RuntimeError, "output pipe unavailable"):
                GATE.run_step(
                    {"schema": "rust-mcp-gate-report-v2", "steps": []},
                    lambda: None,
                    "missing-pipe",
                    [sys.executable, "-c", "pass"],
                    {},
                    output_stream=io.StringIO(),
                )


if __name__ == "__main__":
    unittest.main()
