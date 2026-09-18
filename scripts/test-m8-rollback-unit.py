#!/usr/bin/env python3
"""Hermetic tests for scripts/test-m8-rollback.py: output parsing, receipt
composition and the passed/failed decision. No real `git`/`cargo` process
runs here; every module-level function that would spawn one is replaced with
a double.
"""
from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import pathlib
import subprocess
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


M = _load("rust_mcp_test_m8_rollback", ROOT / "scripts/test-m8-rollback.py")


def completed(returncode: int, stdout: str = "", stderr: str = "") -> subprocess.CompletedProcess:
    return subprocess.CompletedProcess(args=["x"], returncode=returncode, stdout=stdout, stderr=stderr)


def main_silently(argv: list[str]) -> int:
    with contextlib.redirect_stdout(io.StringIO()):
        return M.main(argv)


class JsonOrNoneTests(unittest.TestCase):
    def test_parses_valid_json(self):
        self.assertEqual(M.json_or_none('{"a": 1}'), {"a": 1})

    def test_returns_none_for_invalid_json(self):
        self.assertIsNone(M.json_or_none("not json"))
        self.assertIsNone(M.json_or_none(""))

    def test_returns_none_for_valid_json_that_is_not_an_object(self):
        # R-6: every caller does `.get(...)` on the result; a list or a
        # number would raise AttributeError instead of failing closed.
        self.assertIsNone(M.json_or_none("[1, 2, 3]"))
        self.assertIsNone(M.json_or_none("42"))
        self.assertIsNone(M.json_or_none('"a string"'))
        self.assertIsNone(M.json_or_none("null"))


class ExcerptTests(unittest.TestCase):
    def test_short_text_is_unchanged(self):
        self.assertEqual(M.excerpt("hello", limit=10), "hello")

    def test_long_text_is_truncated_with_head_and_tail(self):
        text = "a" * 50 + "b" * 50
        result = M.excerpt(text, limit=20)
        self.assertIn("...(truncated)...", result)
        self.assertTrue(result.startswith("a" * 10))
        self.assertTrue(result.endswith("b" * 10))


class StepRecordTests(unittest.TestCase):
    def test_omits_stderr_key_when_empty(self):
        record = M.step_record(["cmd", "--flag"], completed(0, stdout="ok", stderr=""))
        self.assertEqual(record["command"], ["cmd", "--flag"])
        self.assertEqual(record["exit_code"], 0)
        self.assertNotIn("stderr_excerpt", record)

    def test_includes_stderr_when_present(self):
        record = M.step_record(["cmd"], completed(1, stdout="", stderr="boom"))
        self.assertEqual(record["stderr_excerpt"], "boom")

    def test_coerces_path_like_arguments_to_strings(self):
        record = M.step_record([pathlib.Path("/bin/x"), "list"], completed(0))
        self.assertEqual(record["command"], ["/bin/x", "list"])


class ScenarioResultTests(unittest.TestCase):
    def test_reason_present_only_when_given(self):
        passed = M.scenario_result("a", "passed", [])
        failed = M.scenario_result("a", "failed", [], reason="because")
        self.assertNotIn("reason", passed)
        self.assertEqual(failed["reason"], "because")

    def test_description_is_looked_up_by_id(self):
        entry = M.scenario_result("d", "passed", [])
        self.assertEqual(entry["description"], M.SCENARIO_DESCRIPTIONS["d"])

    def test_gaps_present_only_when_non_empty(self):
        no_gaps = M.scenario_result("a", "passed", [])
        empty_gaps = M.scenario_result("a", "passed", [], gaps=[])
        with_gaps = M.scenario_result("a", "passed", [], gaps=["a declared gap"])
        self.assertNotIn("gaps", no_gaps)
        self.assertNotIn("gaps", empty_gaps)
        self.assertEqual(with_gaps["gaps"], ["a declared gap"])


class SnapshotDirTests(unittest.TestCase):
    def test_empty_dict_for_missing_directory(self):
        self.assertEqual(M.snapshot_dir(pathlib.Path("/does/not/exist")), {})

    def test_hashes_every_file_by_relative_path(self):
        with tempfile.TemporaryDirectory() as raw:
            base = pathlib.Path(raw)
            (base / "sub").mkdir()
            (base / "sub" / "f.txt").write_bytes(b"hello")
            snapshot = M.snapshot_dir(base)
            self.assertEqual(set(snapshot), {"sub/f.txt"})
            self.assertEqual(snapshot["sub/f.txt"], M.sha256_file(base / "sub" / "f.txt"))


OLD_BINARY = pathlib.Path("/bin/old")
HEAD_BINARY = pathlib.Path("/bin/head")
BLOCKED_JSON = json.dumps({"status": "blocked", "error_code": "recovery_required"})
LIST_ONE_RECORD_JSON = json.dumps({"status": "passed", "records": [{"operation_id": "mut_x"}]})
DOCTOR_DOWNGRADE_BLOCKED_JSON = json.dumps({
    "status": "warning",
    "mutation_journals": {"downgrade_blocked": True, "kinds": {"analyzer_action_apply": {}}},
})
DOCTOR_DOWNGRADE_NOT_BLOCKED_JSON = json.dumps({
    "status": "warning",
    "mutation_journals": {"downgrade_blocked": False, "kinds": {}},
})


class ScenarioATests(unittest.TestCase):
    def test_fails_when_the_native_fixture_does_not_produce_the_journal(self):
        with mock.patch.object(M, "fresh_tmp_dir", return_value=pathlib.Path("/tmp-unused")), \
             mock.patch.object(M, "run", return_value=completed(1, stderr="fixture panicked")) as run_mock:
            result = M.scenario_a(OLD_BINARY, HEAD_BINARY)
        self.assertEqual(result["status"], "failed")
        self.assertEqual(run_mock.call_count, 1)
        self.assertIn("fixture", result["reason"])

    def test_passes_when_every_control_confirms_the_kind_is_the_cause(self):
        with tempfile.TemporaryDirectory(dir=str(ROOT / "target")) as raw:
            tmp = pathlib.Path(raw)
            mutations_dir = tmp / "state" / "rust-mcp-mutations-v1"
            mutations_dir.mkdir(parents=True)
            (mutations_dir / "journal-mut_x.json").write_bytes(b'{"operation":"analyzer_action_apply"}')
            with mock.patch.object(M, "fresh_tmp_dir", return_value=tmp), \
                 mock.patch.object(
                     M, "run",
                     side_effect=[
                         completed(0),  # native fixture
                         completed(1, stdout=BLOCKED_JSON),  # v0.3.0 fails closed on (a)
                         completed(0, stdout=LIST_ONE_RECORD_JSON),  # v0.8.0 lists it fine
                         completed(0, stdout=LIST_ONE_RECORD_JSON),  # v0.3.0 lists the control journal
                         completed(0, stdout=DOCTOR_DOWNGRADE_BLOCKED_JSON),  # doctor flags it
                     ],
                 ):
                result = M.scenario_a(OLD_BINARY, HEAD_BINARY)
        self.assertEqual(result["status"], "passed", result)
        self.assertNotIn("gaps", result)

    def test_fails_when_the_old_binary_reads_the_journal_instead_of_failing_closed(self):
        with tempfile.TemporaryDirectory(dir=str(ROOT / "target")) as raw:
            tmp = pathlib.Path(raw)
            mutations_dir = tmp / "state" / "rust-mcp-mutations-v1"
            mutations_dir.mkdir(parents=True)
            (mutations_dir / "journal-mut_x.json").write_bytes(b'{"operation":"analyzer_action_apply"}')
            passed = json.dumps({"status": "passed", "records": []})
            with mock.patch.object(M, "fresh_tmp_dir", return_value=tmp), \
                 mock.patch.object(
                     M, "run",
                     side_effect=[
                         completed(0),
                         completed(0, stdout=passed),  # v0.3.0 did not fail closed
                         completed(0, stdout=LIST_ONE_RECORD_JSON),
                         completed(0, stdout=LIST_ONE_RECORD_JSON),
                         completed(0, stdout=DOCTOR_DOWNGRADE_BLOCKED_JSON),
                     ],
                 ):
                result = M.scenario_a(OLD_BINARY, HEAD_BINARY)
        self.assertEqual(result["status"], "failed")

    def test_fails_when_the_old_binary_mutates_the_journal(self):
        with tempfile.TemporaryDirectory(dir=str(ROOT / "target")) as raw:
            tmp = pathlib.Path(raw)
            mutations_dir = tmp / "state" / "rust-mcp-mutations-v1"
            mutations_dir.mkdir(parents=True)
            journal = mutations_dir / "journal-mut_x.json"
            journal.write_bytes(b'{"operation":"analyzer_action_apply"}')

            def fake_run(cmd, cwd=None, env=None, timeout=None):
                if cmd[0] == "cargo":
                    return completed(0)
                if cmd[1] == "mutation" and cmd[0] == OLD_BINARY and "control-state" not in str(cmd[4]):
                    journal.write_bytes(b"tampered")
                    return completed(1, stdout=BLOCKED_JSON)
                if cmd[1] == "doctor":
                    return completed(0, stdout=DOCTOR_DOWNGRADE_BLOCKED_JSON)
                return completed(0, stdout=LIST_ONE_RECORD_JSON)

            with mock.patch.object(M, "fresh_tmp_dir", return_value=tmp), \
                 mock.patch.object(M, "run", side_effect=fake_run):
                result = M.scenario_a(OLD_BINARY, HEAD_BINARY)
        self.assertEqual(result["status"], "failed")

    def test_fails_when_v0_8_0_cannot_list_the_same_journal(self):
        with tempfile.TemporaryDirectory(dir=str(ROOT / "target")) as raw:
            tmp = pathlib.Path(raw)
            mutations_dir = tmp / "state" / "rust-mcp-mutations-v1"
            mutations_dir.mkdir(parents=True)
            (mutations_dir / "journal-mut_x.json").write_bytes(b'{"operation":"analyzer_action_apply"}')
            with mock.patch.object(M, "fresh_tmp_dir", return_value=tmp), \
                 mock.patch.object(
                     M, "run",
                     side_effect=[
                         completed(0),
                         completed(1, stdout=BLOCKED_JSON),
                         completed(1, stderr="head binary crashed"),  # positive control (i) fails
                         completed(0, stdout=LIST_ONE_RECORD_JSON),
                         completed(0, stdout=DOCTOR_DOWNGRADE_BLOCKED_JSON),
                     ],
                 ):
                result = M.scenario_a(OLD_BINARY, HEAD_BINARY)
        self.assertEqual(result["status"], "failed")
        self.assertIn("head_lists_the_journal=False", result["reason"])

    def test_fails_when_the_control_journal_does_not_list_cleanly_under_v0_3_0(self):
        with tempfile.TemporaryDirectory(dir=str(ROOT / "target")) as raw:
            tmp = pathlib.Path(raw)
            mutations_dir = tmp / "state" / "rust-mcp-mutations-v1"
            mutations_dir.mkdir(parents=True)
            (mutations_dir / "journal-mut_x.json").write_bytes(b'{"operation":"analyzer_action_apply"}')
            with mock.patch.object(M, "fresh_tmp_dir", return_value=tmp), \
                 mock.patch.object(
                     M, "run",
                     side_effect=[
                         completed(0),
                         completed(1, stdout=BLOCKED_JSON),
                         completed(0, stdout=LIST_ONE_RECORD_JSON),
                         completed(1, stdout=BLOCKED_JSON),  # control journal also rejected
                         completed(0, stdout=DOCTOR_DOWNGRADE_BLOCKED_JSON),
                     ],
                 ):
                result = M.scenario_a(OLD_BINARY, HEAD_BINARY)
        self.assertEqual(result["status"], "failed")
        self.assertIn("control_journal_lists_under_v0_3_0=False", result["reason"])

    def test_fails_when_doctor_reports_downgrade_not_blocked(self):
        with tempfile.TemporaryDirectory(dir=str(ROOT / "target")) as raw:
            tmp = pathlib.Path(raw)
            mutations_dir = tmp / "state" / "rust-mcp-mutations-v1"
            mutations_dir.mkdir(parents=True)
            (mutations_dir / "journal-mut_x.json").write_bytes(b'{"operation":"analyzer_action_apply"}')
            with mock.patch.object(M, "fresh_tmp_dir", return_value=tmp), \
                 mock.patch.object(
                     M, "run",
                     side_effect=[
                         completed(0),
                         completed(1, stdout=BLOCKED_JSON),
                         completed(0, stdout=LIST_ONE_RECORD_JSON),
                         completed(0, stdout=LIST_ONE_RECORD_JSON),
                         completed(0, stdout=DOCTOR_DOWNGRADE_NOT_BLOCKED_JSON),
                     ],
                 ):
                result = M.scenario_a(OLD_BINARY, HEAD_BINARY)
        self.assertEqual(result["status"], "failed")
        self.assertNotIn("gaps", result)

    def test_fails_with_a_declared_gap_when_doctor_state_root_is_unsupported(self):
        # R-1(iii): a doctor build that still needs the full host tuple before
        # it will accept `--state-root` (pre-D-5) rejects the invocation
        # outright; that gap must fail the scenario, exactly like (b)'s
        # quality control -- it must never be silently reported as `passed`
        # for a positive control it could not actually evaluate.
        with tempfile.TemporaryDirectory(dir=str(ROOT / "target")) as raw:
            tmp = pathlib.Path(raw)
            mutations_dir = tmp / "state" / "rust-mcp-mutations-v1"
            mutations_dir.mkdir(parents=True)
            (mutations_dir / "journal-mut_x.json").write_bytes(b'{"operation":"analyzer_action_apply"}')
            with mock.patch.object(M, "fresh_tmp_dir", return_value=tmp), \
                 mock.patch.object(
                     M, "run",
                     side_effect=[
                         completed(0),
                         completed(1, stdout=BLOCKED_JSON),
                         completed(0, stdout=LIST_ONE_RECORD_JSON),
                         completed(0, stdout=LIST_ONE_RECORD_JSON),
                         completed(1, stderr="Unsupported invocation."),  # doctor rejects it
                     ],
                 ):
                result = M.scenario_a(OLD_BINARY, HEAD_BINARY)
        self.assertEqual(result["status"], "failed", result)
        self.assertEqual(len(result["gaps"]), 1)
        self.assertIn("R-1(iii)", result["gaps"][0])


class ScenarioBcUnavailableTests(unittest.TestCase):
    def test_marks_both_scenarios_unavailable_when_fixtures_are_missing(self):
        with mock.patch.object(M, "FIXTURES_DIR", pathlib.Path("/does/not/exist")):
            b, c = M.scenario_bc(pathlib.Path("/bin/old"), pathlib.Path("/bin/head"))
        self.assertEqual(b["status"], "unavailable")
        self.assertEqual(c["status"], "unavailable")
        self.assertIn("fixtures", b["reason"])


class ScenarioDUnavailableTests(unittest.TestCase):
    def test_marks_unavailable_when_fixture_one_is_missing(self):
        with mock.patch.object(M, "FIXTURES_DIR", pathlib.Path("/does/not/exist")):
            result = M.scenario_d(pathlib.Path("/bin/old"), pathlib.Path("/bin/head"))
        self.assertEqual(result["status"], "unavailable")


class RecoveryConfirmedTests(unittest.TestCase):
    def test_true_only_with_a_real_validated_artifact_and_no_quarantine(self):
        self.assertTrue(M.recovery_confirmed({
            "status": "passed", "data": {"validated": 1, "quarantined": 0},
        }))
        self.assertTrue(M.recovery_confirmed({
            "status": "passed", "data": {"validated": 3, "quarantined": 0},
        }))

    def test_false_on_an_empty_store_even_though_it_reports_passed(self):
        # R-2: `quality-artifacts recover` over an empty state root reports
        # `validated=0, quarantined=0` and `status: passed` — that must never
        # be read as "an artifact was confirmed".
        self.assertFalse(M.recovery_confirmed({
            "status": "passed", "data": {"validated": 0, "quarantined": 0},
        }))

    def test_false_when_anything_was_quarantined(self):
        self.assertFalse(M.recovery_confirmed({
            "status": "passed", "data": {"validated": 1, "quarantined": 1},
        }))

    def test_false_when_blocked_or_malformed(self):
        self.assertFalse(M.recovery_confirmed({"status": "blocked", "data": None}))
        self.assertFalse(M.recovery_confirmed({"status": "passed", "data": "not-a-dict"}))
        self.assertFalse(M.recovery_confirmed({"status": "passed"}))


class EnsureWorktreeTests(unittest.TestCase):
    def test_creates_a_fresh_detached_worktree_at_the_resolved_commit(self):
        with tempfile.TemporaryDirectory(dir=str(ROOT / "target")) as raw:
            work_root = pathlib.Path(raw) / "m8-rollback"
            with mock.patch.object(M, "WORK_ROOT", work_root), \
                 mock.patch.object(M, "git", return_value=""), \
                 mock.patch.object(M.subprocess, "run", return_value=completed(0)) as run_mock:
                worktree, clean = M.ensure_worktree("v0.3.0", "cafef00d")
        self.assertTrue(clean)
        self.assertEqual(worktree, work_root / "worktree-v0.3.0")
        args = run_mock.call_args.args[0]
        self.assertEqual(args[:3], ["git", "worktree", "add"])
        self.assertIn("--end-of-options", args)
        # The resolved commit is passed, never the mutable tag, and after
        # `--end-of-options` so a tag that looks like a flag can't be one.
        self.assertEqual(args[args.index("--end-of-options") + 1], "cafef00d")
        self.assertNotIn("v0.3.0", args)

    def test_reuses_a_registered_worktree_only_after_verifying_head_and_clean(self):
        listing = "worktree /repo/target/m8-rollback/worktree-v0.3.0\nHEAD cafef00d\n\n"
        with mock.patch.object(M, "WORK_ROOT", pathlib.Path("/repo/target/m8-rollback")), \
             mock.patch.object(M, "git", return_value=listing), \
             mock.patch.object(M, "worktree_head", return_value="cafef00d"), \
             mock.patch.object(M, "worktree_is_clean", return_value=True):
            worktree, clean = M.ensure_worktree("v0.3.0", "cafef00d")
        self.assertTrue(clean)
        self.assertEqual(worktree, pathlib.Path("/repo/target/m8-rollback/worktree-v0.3.0"))

    def test_refuses_to_reuse_a_worktree_at_the_wrong_commit(self):
        listing = "worktree /repo/target/m8-rollback/worktree-v0.3.0\nHEAD deadbeef\n\n"
        with mock.patch.object(M, "WORK_ROOT", pathlib.Path("/repo/target/m8-rollback")), \
             mock.patch.object(M, "git", return_value=listing), \
             mock.patch.object(M, "worktree_head", return_value="deadbeef"):
            with self.assertRaises(M.DriverError) as caught:
                M.ensure_worktree("v0.3.0", "cafef00d")
        self.assertIn("deadbeef", str(caught.exception))
        self.assertIn("cafef00d", str(caught.exception))

    def test_refuses_to_reuse_a_dirty_worktree(self):
        listing = "worktree /repo/target/m8-rollback/worktree-v0.3.0\nHEAD cafef00d\n\n"
        with mock.patch.object(M, "WORK_ROOT", pathlib.Path("/repo/target/m8-rollback")), \
             mock.patch.object(M, "git", return_value=listing), \
             mock.patch.object(M, "worktree_head", return_value="cafef00d"), \
             mock.patch.object(M, "worktree_is_clean", return_value=False):
            with self.assertRaises(M.DriverError) as caught:
                M.ensure_worktree("v0.3.0", "cafef00d")
        self.assertIn("local changes", str(caught.exception))


class ScenarioBcQualityArtifactTests(unittest.TestCase):
    def _fixtures(self, tmp):
        catalog_fixtures = tmp / "catalog"
        catalog_fixtures.mkdir()
        (catalog_fixtures / "fixture-1.tar.zst").write_bytes(b"one")
        (catalog_fixtures / "fixture-2.tar.zst").write_bytes(b"two")
        (catalog_fixtures / "fixture-trust.json").write_bytes(b"{}")
        return catalog_fixtures

    def test_b_requires_a_real_validated_artifact_under_both_binaries(self):
        with tempfile.TemporaryDirectory(dir=str(ROOT / "target")) as raw:
            tmp = pathlib.Path(raw)
            fixtures = self._fixtures(tmp)
            import_ok = json.dumps({"catalog": {"sequence": 2}})
            status_ok = json.dumps({"catalog": {"sequence": 2}})
            recover_ok = json.dumps({"status": "passed", "data": {"validated": 1, "quarantined": 0}})
            rollback_blocked = json.dumps({"error_code": "CATALOG_ROLLBACK"})
            status2_ok = json.dumps({"catalog": {"sequence": 2, "floor_sequence": 2}})
            with mock.patch.object(M, "FIXTURES_DIR", fixtures), \
                 mock.patch.object(M, "fresh_tmp_dir", side_effect=[tmp / "bc", tmp / "qa"]), \
                 mock.patch.object(M, "run_native_fixture", return_value=(["cargo"], completed(0))), \
                 mock.patch.object(
                     M, "run",
                     side_effect=[
                         completed(0, stdout=import_ok),  # head imports sequence-2
                         completed(0, stdout=status_ok),  # v0.3.0 reads it back
                         completed(0, stdout=recover_ok),  # v0.8.0 recovers the real artifact
                         completed(0, stdout=recover_ok),  # v0.3.0 recovers the same artifact
                         completed(1, stdout=rollback_blocked),  # (c) v0.3.0 rejects sequence-1
                         completed(0, stdout=status2_ok),
                     ],
                 ):
                b, c = M.scenario_bc(OLD_BINARY, HEAD_BINARY)
        self.assertEqual(b["status"], "passed", b)
        self.assertNotIn("gaps", b)
        self.assertEqual(c["status"], "passed", c)

    def test_b_fails_when_recover_only_sees_an_empty_store(self):
        with tempfile.TemporaryDirectory(dir=str(ROOT / "target")) as raw:
            tmp = pathlib.Path(raw)
            fixtures = self._fixtures(tmp)
            import_ok = json.dumps({"catalog": {"sequence": 2}})
            status_ok = json.dumps({"catalog": {"sequence": 2}})
            empty_recover = json.dumps({"status": "passed", "data": {"validated": 0, "quarantined": 0}})
            rollback_blocked = json.dumps({"error_code": "CATALOG_ROLLBACK"})
            status2_ok = json.dumps({"catalog": {"sequence": 2, "floor_sequence": 2}})
            with mock.patch.object(M, "FIXTURES_DIR", fixtures), \
                 mock.patch.object(M, "fresh_tmp_dir", side_effect=[tmp / "bc", tmp / "qa"]), \
                 mock.patch.object(M, "run_native_fixture", return_value=(["cargo"], completed(0))), \
                 mock.patch.object(
                     M, "run",
                     side_effect=[
                         completed(0, stdout=import_ok),
                         completed(0, stdout=status_ok),
                         completed(0, stdout=empty_recover),
                         completed(0, stdout=empty_recover),
                         completed(1, stdout=rollback_blocked),
                         completed(0, stdout=status2_ok),
                     ],
                 ):
                b, _c = M.scenario_bc(OLD_BINARY, HEAD_BINARY)
        self.assertEqual(b["status"], "failed", b)

    def test_b_declares_a_gap_when_the_m3_fixture_did_not_run_on_this_arch(self):
        with tempfile.TemporaryDirectory(dir=str(ROOT / "target")) as raw:
            tmp = pathlib.Path(raw)
            fixtures = self._fixtures(tmp)
            import_ok = json.dumps({"catalog": {"sequence": 2}})
            status_ok = json.dumps({"catalog": {"sequence": 2}})
            rollback_blocked = json.dumps({"error_code": "CATALOG_ROLLBACK"})
            status2_ok = json.dumps({"catalog": {"sequence": 2, "floor_sequence": 2}})
            with mock.patch.object(M, "FIXTURES_DIR", fixtures), \
                 mock.patch.object(M, "fresh_tmp_dir", side_effect=[tmp / "bc", tmp / "qa"]), \
                 mock.patch.object(
                     M, "run_native_fixture",
                     return_value=(["cargo"], completed(0, stdout="running 0 tests\n")),
                 ), \
                 mock.patch.object(
                     M, "run",
                     side_effect=[
                         completed(0, stdout=import_ok),
                         completed(0, stdout=status_ok),
                         completed(1, stdout=rollback_blocked),
                         completed(0, stdout=status2_ok),
                     ],
                 ):
                b, _c = M.scenario_bc(OLD_BINARY, HEAD_BINARY)
        self.assertEqual(b["status"], "failed", b)
        self.assertEqual(len(b["gaps"]), 1)
        self.assertIn("R-2", b["gaps"][0])


class ScenarioDGapsTests(unittest.TestCase):
    def test_always_declares_the_m2_and_m3_upgrade_gaps(self):
        with tempfile.TemporaryDirectory(dir=str(ROOT / "target")) as raw:
            tmp = pathlib.Path(raw)
            fixtures = tmp / "catalog"
            fixtures.mkdir()
            (fixtures / "fixture-1.tar.zst").write_bytes(b"one")
            (fixtures / "fixture-trust.json").write_bytes(b"{}")
            status_ok = json.dumps({"catalog": {"sequence": 1}})
            doctor_ok = json.dumps({"status": "passed"})
            with mock.patch.object(M, "FIXTURES_DIR", fixtures), \
                 mock.patch.object(M, "fresh_tmp_dir", return_value=tmp / "d"), \
                 mock.patch.object(
                     M, "run",
                     side_effect=[
                         completed(0),  # qa_init
                         completed(0),  # catalog import
                         completed(0),  # qa_upgrade
                         completed(0, stdout=status_ok),
                         completed(0, stdout=doctor_ok),
                     ],
                 ):
                result = M.scenario_d(OLD_BINARY, HEAD_BINARY)
        self.assertEqual(result["status"], "passed", result)
        self.assertEqual(len(result["gaps"]), 2)
        self.assertTrue(any("R-7" in gap for gap in result["gaps"]))
        self.assertTrue(any("R-2" in gap for gap in result["gaps"]))


class MainAggregationTests(unittest.TestCase):
    def _patch_build(self, stack):
        stack.enter_context(mock.patch.object(M, "head_commit", return_value="deadbeef"))
        stack.enter_context(mock.patch.object(M, "tree_is_dirty", return_value=False))
        stack.enter_context(mock.patch.object(M, "resolve_tag_commit", return_value="cafef00d"))
        stack.enter_context(mock.patch.object(
            M, "ensure_worktree", return_value=(pathlib.Path("/wt"), True)
        ))
        stack.enter_context(mock.patch.object(
            M, "build_binary",
            side_effect=[pathlib.Path("/bin/old"), pathlib.Path("/bin/head")],
        ))
        stack.enter_context(mock.patch.object(
            M, "read_version",
            side_effect=[{"version": "0.3.0"}, {"version": "0.8.0"}],
        ))
        stack.enter_context(mock.patch.object(M, "workspace_version", return_value="0.8.0"))
        stack.enter_context(mock.patch.object(M, "sha256_file", return_value="sha256:stub"))

    def test_overall_status_passed_only_when_all_four_scenarios_pass(self):
        with contextlib.ExitStack() as stack, tempfile.TemporaryDirectory(dir=str(ROOT / "target")) as raw:
            self._patch_build(stack)
            stack.enter_context(mock.patch.object(
                M, "scenario_a", return_value=M.scenario_result("a", "passed", [])
            ))
            stack.enter_context(mock.patch.object(
                M, "scenario_bc",
                return_value=(M.scenario_result("b", "passed", []), M.scenario_result("c", "passed", [])),
            ))
            stack.enter_context(mock.patch.object(
                M, "scenario_d", return_value=M.scenario_result("d", "passed", [])
            ))
            out_path = pathlib.Path(raw) / "receipt.json"
            code = main_silently(["--out", str(out_path)])
            self.assertEqual(code, 0)
            receipt = json.loads(out_path.read_text())
        self.assertEqual(receipt["status"], "passed")
        self.assertEqual([s["status"] for s in receipt["scenarios"]], ["passed"] * 4)
        self.assertIsNone(receipt["driver_error"])
        self.assertEqual(receipt["binaries"]["old"]["version"], "0.3.0")
        self.assertEqual(receipt["binaries"]["old"]["tree_dirty"], False)
        self.assertEqual(receipt["binaries"]["head"]["version"], "0.8.0")
        self.assertEqual(receipt["binaries"]["head"]["tree_dirty"], False)

    def test_overall_status_failed_when_one_scenario_fails(self):
        with contextlib.ExitStack() as stack, tempfile.TemporaryDirectory(dir=str(ROOT / "target")) as raw:
            self._patch_build(stack)
            stack.enter_context(mock.patch.object(
                M, "scenario_a", return_value=M.scenario_result("a", "failed", [], reason="nope")
            ))
            stack.enter_context(mock.patch.object(
                M, "scenario_bc",
                return_value=(M.scenario_result("b", "passed", []), M.scenario_result("c", "passed", [])),
            ))
            stack.enter_context(mock.patch.object(
                M, "scenario_d", return_value=M.scenario_result("d", "passed", [])
            ))
            out_path = pathlib.Path(raw) / "receipt.json"
            code = main_silently(["--out", str(out_path)])
            self.assertEqual(code, 1)
            receipt = json.loads(out_path.read_text())
        self.assertEqual(receipt["status"], "failed")

    def test_a_never_ran_scenario_is_marked_unavailable_not_passed(self):
        with contextlib.ExitStack() as stack, tempfile.TemporaryDirectory(dir=str(ROOT / "target")) as raw:
            self._patch_build(stack)
            stack.enter_context(mock.patch.object(
                M, "scenario_a", return_value=M.scenario_result("a", "passed", [])
            ))
            stack.enter_context(mock.patch.object(
                M, "scenario_bc",
                return_value=(M.scenario_result("b", "passed", []), M.scenario_result("c", "passed", [])),
            ))
            stack.enter_context(mock.patch.object(M, "scenario_d", side_effect=M.DriverError("boom")))
            out_path = pathlib.Path(raw) / "receipt.json"
            code = main_silently(["--out", str(out_path)])
            self.assertEqual(code, 1)
            receipt = json.loads(out_path.read_text())
        by_id = {s["id"]: s for s in receipt["scenarios"]}
        self.assertEqual(by_id["d"]["status"], "unavailable")
        self.assertEqual(by_id["d"]["reason"], "boom")
        self.assertEqual(receipt["driver_error"], "boom")

    def test_a_driver_error_before_any_scenario_marks_all_four_unavailable(self):
        with contextlib.ExitStack() as stack, tempfile.TemporaryDirectory(dir=str(ROOT / "target")) as raw:
            stack.enter_context(mock.patch.object(M, "head_commit", return_value="deadbeef"))
            stack.enter_context(mock.patch.object(M, "tree_is_dirty", return_value=False))
            stack.enter_context(mock.patch.object(
                M, "resolve_tag_commit", side_effect=M.DriverError("tag missing")
            ))
            out_path = pathlib.Path(raw) / "receipt.json"
            code = main_silently(["--out", str(out_path)])
            self.assertEqual(code, 1)
            receipt = json.loads(out_path.read_text())
        self.assertEqual(receipt["status"], "failed")
        self.assertTrue(all(s["status"] == "unavailable" for s in receipt["scenarios"]))
        self.assertEqual(receipt["driver_error"], "tag missing")
        self.assertIsNone(receipt["binaries"]["old"]["sha256"])

    def test_rejects_a_binary_reporting_the_wrong_version(self):
        with contextlib.ExitStack() as stack, tempfile.TemporaryDirectory(dir=str(ROOT / "target")) as raw:
            stack.enter_context(mock.patch.object(M, "head_commit", return_value="deadbeef"))
            stack.enter_context(mock.patch.object(M, "tree_is_dirty", return_value=False))
            stack.enter_context(mock.patch.object(M, "resolve_tag_commit", return_value="cafef00d"))
            stack.enter_context(mock.patch.object(
                M, "ensure_worktree", return_value=(pathlib.Path("/wt"), True)
            ))
            stack.enter_context(mock.patch.object(
                M, "build_binary",
                side_effect=[pathlib.Path("/bin/old"), pathlib.Path("/bin/head")],
            ))
            stack.enter_context(mock.patch.object(
                M, "read_version",
                side_effect=[{"version": "9.9.9"}, {"version": "0.8.0"}],
            ))
            stack.enter_context(mock.patch.object(M, "workspace_version", return_value="0.8.0"))
            stack.enter_context(mock.patch.object(M, "sha256_file", return_value="sha256:stub"))
            out_path = pathlib.Path(raw) / "receipt.json"
            code = main_silently(["--out", str(out_path)])
            self.assertEqual(code, 1)
            receipt = json.loads(out_path.read_text())
        self.assertIn("9.9.9", receipt["driver_error"])
        self.assertTrue(all(s["status"] == "unavailable" for s in receipt["scenarios"]))

    def test_rejects_a_head_binary_not_matching_the_workspace_version(self):
        with contextlib.ExitStack() as stack, tempfile.TemporaryDirectory(dir=str(ROOT / "target")) as raw:
            stack.enter_context(mock.patch.object(M, "head_commit", return_value="deadbeef"))
            stack.enter_context(mock.patch.object(M, "tree_is_dirty", return_value=False))
            stack.enter_context(mock.patch.object(M, "resolve_tag_commit", return_value="cafef00d"))
            stack.enter_context(mock.patch.object(
                M, "ensure_worktree", return_value=(pathlib.Path("/wt"), True)
            ))
            stack.enter_context(mock.patch.object(
                M, "build_binary",
                side_effect=[pathlib.Path("/bin/old"), pathlib.Path("/bin/head")],
            ))
            stack.enter_context(mock.patch.object(
                M, "read_version",
                side_effect=[{"version": "0.3.0"}, {"version": "0.8.0"}],
            ))
            stack.enter_context(mock.patch.object(M, "workspace_version", return_value="9.9.9-rc.7"))
            stack.enter_context(mock.patch.object(M, "sha256_file", return_value="sha256:stub"))
            out_path = pathlib.Path(raw) / "receipt.json"
            code = main_silently(["--out", str(out_path)])
            self.assertEqual(code, 1)
            receipt = json.loads(out_path.read_text())
        self.assertIn("9.9.9-rc.7", receipt["driver_error"])
        self.assertIn("0.8.0", receipt["driver_error"])
        self.assertTrue(all(s["status"] == "unavailable" for s in receipt["scenarios"]))

    def test_workspace_version_reads_the_synthetic_cargo_toml(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            (root / "Cargo.toml").write_text(
                '[workspace.package]\nversion = "9.9.9-rc.7"\nedition = "2024"\n'
            )
            with mock.patch.object(M, "ROOT", root):
                self.assertEqual(M.workspace_version(), "9.9.9-rc.7")


if __name__ == "__main__":
    unittest.main()
