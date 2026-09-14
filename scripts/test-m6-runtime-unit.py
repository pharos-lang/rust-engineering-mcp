#!/usr/bin/env python3
"""Unit tests for the pure functions of scripts/test-m6-runtime.py.

No network, no Docker, no cargo: only the host-side decisions the gate makes
before and after it drives the engine — where it writes, how long a selection
may take, which Docker client it inspects with, and how it refuses a published
calibration that is not the one this run produced.
"""
import datetime
import importlib.util
import json
import pathlib
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]


def _load(name: str, path: pathlib.Path):
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


gate = _load("m6_runtime_gate", ROOT / "scripts/test-m6-runtime.py")


class OutputPathTests(unittest.TestCase):
    def test_the_gate_output_is_a_constant_under_the_repository(self) -> None:
        self.assertEqual(gate.OUTPUT, gate.ROOT / "target/m6-runtime-gate")

    def test_no_environment_variable_can_move_it(self) -> None:
        source = (ROOT / "scripts/test-m6-runtime.py").read_text()
        self.assertNotIn("RUST_MCP_M6_RUNTIME_OUTPUT", source)

    def test_the_gate_never_writes_into_the_native_calibration_receipt(self) -> None:
        self.assertNotEqual(gate.OUTPUT, gate.NATIVE_OUTPUT)


class StepTimeoutTests(unittest.TestCase):
    def test_the_default_applies_when_the_variable_is_absent(self) -> None:
        self.assertEqual(gate.step_timeout_s({}), gate.DEFAULT_STEP_TIMEOUT_S)

    def test_an_explicit_positive_integer_is_honoured(self) -> None:
        self.assertEqual(gate.step_timeout_s({gate.STEP_TIMEOUT_VARIABLE: "42"}), 42)

    def test_a_non_integer_is_this_gate_s_own_refusal(self) -> None:
        for value in ["", "abc", "9.5", "0x10", " "]:
            with self.assertRaises(RuntimeError) as raised:
                gate.step_timeout_s({gate.STEP_TIMEOUT_VARIABLE: value})
            self.assertIn(gate.STEP_TIMEOUT_VARIABLE, str(raised.exception))

    def test_zero_and_negative_values_are_refused_by_name(self) -> None:
        for value in ["0", "-1"]:
            with self.assertRaises(RuntimeError) as raised:
                gate.step_timeout_s({gate.STEP_TIMEOUT_VARIABLE: value})
            self.assertIn(gate.STEP_TIMEOUT_VARIABLE, str(raised.exception))


class DockerClientTests(unittest.TestCase):
    def test_the_known_path_is_used_when_nothing_is_set(self) -> None:
        self.assertEqual(gate.docker_client({}), gate.DEFAULT_DOCKER)

    def test_the_same_override_the_rust_side_reads_is_honoured(self) -> None:
        self.assertEqual(
            gate.docker_client({"RUST_MCP_TEST_DOCKER": "/opt/homebrew/bin/docker"}),
            "/opt/homebrew/bin/docker",
        )

    def test_the_override_name_matches_the_native_calibration(self) -> None:
        native = gate.NATIVE_SOURCE.read_text()
        self.assertIn('var_os("RUST_MCP_TEST_DOCKER")', native)


class DeclaredCutTests(unittest.TestCase):
    def test_every_ignored_test_of_the_real_source_declares_its_cut(self) -> None:
        declared = gate.ignored_tests(gate.NATIVE_SOURCE)
        cuts = gate.declared_cuts(gate.NATIVE_SOURCE)
        self.assertTrue(declared)
        for name in declared:
            self.assertIn(name, cuts)
        self.assertEqual(len(set(cuts[name] for name in declared)), len(declared))

    def test_the_first_cut_opened_by_a_test_is_the_one_recorded(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            source = pathlib.Path(directory) / "native.rs"
            source.write_text(
                "#[test]\n#[ignore = \"x\"]\n"
                'fn a_cut() -> Result<(), Failure> {\n'
                '    let mut cut = Cut::open("m6-09-example", &image);\n'
                "}\n"
            )
            self.assertEqual(gate.declared_cuts(source), {"a_cut": "m6-09-example"})


class StaleDocumentTests(unittest.TestCase):
    FLOOR = datetime.datetime(2026, 9, 12, 10, 0, 0, tzinfo=datetime.UTC)

    def document(self, name, stamp):
        return {"cut": name, "run_started_at": stamp} if stamp else {"cut": name}

    def test_a_document_from_this_run_is_accepted(self) -> None:
        published = {"cuts": [self.document("m6-00-admission", "2026-09-12T10:00:00Z")]}
        self.assertEqual(gate.stale_cut_documents(published, self.FLOOR), {})

    def test_a_document_from_an_earlier_run_is_stale(self) -> None:
        published = {"cuts": [self.document("m6-00-admission", "2026-09-12T09:59:59Z")]}
        self.assertEqual(
            gate.stale_cut_documents(published, self.FLOOR),
            {"m6-00-admission": "2026-09-12T09:59:59Z"},
        )

    def test_a_document_with_no_run_stamp_is_stale(self) -> None:
        published = {"cuts": [self.document("m6-01-identity", None)]}
        self.assertEqual(
            gate.stale_cut_documents(published, self.FLOOR), {"m6-01-identity": None}
        )

    def test_an_unparseable_stamp_is_stale_rather_than_trusted(self) -> None:
        published = {"cuts": [self.document("m6-01-identity", "yesterday")]}
        self.assertEqual(
            gate.stale_cut_documents(published, self.FLOOR), {"m6-01-identity": "yesterday"}
        )

    def test_the_native_cut_document_carries_the_run_stamp_this_check_needs(self) -> None:
        self.assertIn('"run_started_at": utc(*PROCESS_STARTED_UNIX)', gate.NATIVE_SOURCE.read_text())


class ClearNativeOutputTests(unittest.TestCase):
    def test_cut_documents_and_the_merged_receipt_are_removed(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            native = pathlib.Path(directory) / "m6-calibration"
            native.mkdir()
            (native / "cut-m6-00-admission.json").write_text("{}")
            (native / "receipt.json").write_text(json.dumps({"schema": gate.NATIVE_RECEIPT_SCHEMA}))
            (native / "config-schema.json").write_text("{}")
            original_native, original_root = gate.NATIVE_OUTPUT, gate.ROOT
            gate.NATIVE_OUTPUT, gate.ROOT = native, pathlib.Path(directory)
            try:
                removed = gate.clear_native_output()
            finally:
                gate.NATIVE_OUTPUT, gate.ROOT = original_native, original_root
            self.assertEqual(
                sorted(removed),
                ["m6-calibration/cut-m6-00-admission.json", "m6-calibration/receipt.json"],
            )
            self.assertFalse((native / "cut-m6-00-admission.json").exists())
            self.assertFalse((native / "receipt.json").exists())
            # The schema dump is the identity cut's own archive, republished by
            # that cut; removing it would delete evidence this gate cannot
            # rebuild on a failed run.
            self.assertTrue((native / "config-schema.json").exists())

    def test_an_absent_directory_is_not_an_error(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            original_native, original_root = gate.NATIVE_OUTPUT, gate.ROOT
            gate.NATIVE_OUTPUT = pathlib.Path(directory) / "absent"
            gate.ROOT = pathlib.Path(directory)
            try:
                self.assertEqual(gate.clear_native_output(), [])
            finally:
                gate.NATIVE_OUTPUT, gate.ROOT = original_native, original_root


if __name__ == "__main__":
    if not __debug__:
        raise RuntimeError("Optimized Python mode is rejected")
    unittest.main()
