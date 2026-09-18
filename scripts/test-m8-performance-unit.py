#!/usr/bin/env python3
"""Hermetic unit tests for scripts/measure-m8-performance.py and scripts/soak-m8.py.

No server process is spawned: statistics, budget comparison, the 2-of-3
regression rule, response validation (P-1), the ``--compare`` CLI (P-2),
insufficient-sample detection (P-3), noise-control re-measurement (P-7),
``ps``/``lsof``/``pgrep``/``pmset`` output parsing and the soak criteria
(including ``fd_after_ttl`` and the catalog-store orphan check, P-5/P-6) are
exercised directly against synthetic data and mocked ``subprocess.run``
doubles.
"""
from __future__ import annotations

import importlib.util
import json
import pathlib
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


PERF = _load("rust_mcp_m8_performance", ROOT / "scripts/measure-m8-performance.py")
SOAK = _load("rust_mcp_m8_soak", ROOT / "scripts/soak-m8.py")


def budget_row(statistic: str, budget: float, unit: str = "ms", n: int = 30) -> dict:
    return {"id": "x", "budget": budget, "unit": unit, "statistic": statistic, "n": n, "provisional": True}


def passed_response(data: dict | None = None) -> dict:
    return {"result": {"isError": False, "structuredContent": {"status": "passed", "data": data or {}}}}


class SummarizeTests(unittest.TestCase):
    def test_p95_uses_nearest_rank_like_m4_summarize(self) -> None:
        values = list(range(1, 31))  # 1..30, matches scripts/summarize-m4-budgets.py's convention
        result = PERF.summarize([float(v) for v in values], budget_row("p95", 1000))
        self.assertEqual(result["p95"], 29.0)  # values[ceil(.95*30)-1] == values[28] == 29
        self.assertEqual(result["max"], 30.0)
        self.assertEqual(result["median"], 15.5)
        self.assertEqual(result["min"], 1.0)

    def test_max_statistic_ignores_percentile(self) -> None:
        result = PERF.summarize([10.0, 999.0, 5.0], budget_row("max", 1000, unit="MiB", n=3))
        self.assertEqual(result["statistic_value"], 999.0)
        self.assertEqual(result["statistic"], "max")

    def test_unsupported_statistic_rejected(self) -> None:
        with self.assertRaises(ValueError):
            PERF.summarize([1.0], budget_row("p99", 10, n=1))

    def test_empty_sample_set_rejected(self) -> None:
        with self.assertRaises(ValueError):
            PERF.summarize([], budget_row("max", 10, n=1))

    def test_fewer_samples_than_budget_n_is_insufficient_not_within(self) -> None:
        # P-3: 1 sample against an n=30 budget must never report "within".
        result = PERF.summarize([1.0], budget_row("max", 1000, n=30))
        self.assertEqual(result["status"], "insufficient_samples")
        self.assertEqual(result["verdict"], "insufficient_samples")
        self.assertEqual(result["n"], 1)
        self.assertEqual(result["required_n"], 30)
        self.assertNotIn("statistic_value", result)

    def test_exactly_n_samples_is_measured(self) -> None:
        result = PERF.summarize([1.0, 2.0, 3.0], budget_row("max", 1000, n=3))
        self.assertEqual(result["status"], "measured")


class BudgetComparisonTests(unittest.TestCase):
    def test_within_budget_verdict(self) -> None:
        result = PERF.summarize([10.0, 20.0, 30.0], budget_row("max", 100, n=3))
        self.assertEqual(result["verdict"], "within")

    def test_over_budget_verdict(self) -> None:
        result = PERF.summarize([10.0, 20.0, 300.0], budget_row("max", 100, n=3))
        self.assertEqual(result["verdict"], "over")

    def test_exactly_at_budget_is_within(self) -> None:
        result = PERF.summarize([100.0], budget_row("max", 100, n=1))
        self.assertEqual(result["verdict"], "within")

    def test_unavailable_helper_never_claims_measured_or_passed(self) -> None:
        result = PERF.unavailable(budget_row("p95", 3000), "docker_not_used_in_this_package")
        self.assertEqual(result["status"], "unavailable")
        self.assertEqual(result["verdict"], "unavailable")
        self.assertNotIn("statistic_value", result)
        self.assertNotIn("raw_samples", result)

    def test_global_verdict_over_beats_everything(self) -> None:
        measurements = {
            "a": {"verdict": "over"},
            "b": {"verdict": "unavailable"},
            "c": {"verdict": "within"},
            "d": {"verdict": "insufficient_samples"},
        }
        self.assertEqual(PERF.global_verdict(measurements), "over")

    def test_global_verdict_insufficient_samples_beats_unavailable_and_within(self) -> None:
        measurements = {
            "a": {"verdict": "within"},
            "b": {"verdict": "unavailable"},
            "c": {"verdict": "insufficient_samples"},
        }
        self.assertEqual(PERF.global_verdict(measurements), "insufficient_samples")

    def test_global_verdict_unavailable_beats_within(self) -> None:
        measurements = {"a": {"verdict": "within"}, "b": {"verdict": "unavailable"}}
        self.assertEqual(PERF.global_verdict(measurements), "unavailable")

    def test_global_verdict_all_within(self) -> None:
        measurements = {"a": {"verdict": "within"}, "b": {"verdict": "within"}}
        self.assertEqual(PERF.global_verdict(measurements), "within")


def make_receipt(
    verdicts: dict[str, str], budgets_sha256: str = "sha256:abc", profile: str = "core"
) -> dict:
    return {
        "measurements": {name: {"verdict": verdict} for name, verdict in verdicts.items()},
        "budgets_sha256": budgets_sha256,
        "profile": profile,
    }


class RegressionVerdictTests(unittest.TestCase):
    def test_two_of_three_over_is_regressed(self) -> None:
        receipts = [
            make_receipt({"startup_cold_ms": "over"}),
            make_receipt({"startup_cold_ms": "within"}),
            make_receipt({"startup_cold_ms": "over"}),
        ]
        result = PERF.regression_verdict(receipts)
        self.assertEqual(result["startup_cold_ms"]["outcome"], "regressed")
        self.assertTrue(result["startup_cold_ms"]["regressed"])
        self.assertEqual(result["startup_cold_ms"]["over_count"], 2)

    def test_one_of_three_over_is_not_regressed(self) -> None:
        receipts = [
            make_receipt({"startup_cold_ms": "over"}),
            make_receipt({"startup_cold_ms": "within"}),
            make_receipt({"startup_cold_ms": "within"}),
        ]
        result = PERF.regression_verdict(receipts)
        self.assertEqual(result["startup_cold_ms"]["outcome"], "not_regressed")
        self.assertFalse(result["startup_cold_ms"]["regressed"])
        self.assertEqual(result["startup_cold_ms"]["over_count"], 1)

    def test_missing_magnitude_in_a_receipt_counts_as_unavailable_not_over(self) -> None:
        receipts = [
            make_receipt({"startup_cold_ms": "over"}),
            make_receipt({}),
            make_receipt({"startup_cold_ms": "over"}),
        ]
        result = PERF.regression_verdict(receipts)
        self.assertEqual(result["startup_cold_ms"]["verdicts"], ["over", "unavailable", "over"])
        self.assertEqual(result["startup_cold_ms"]["outcome"], "regressed")

    def test_one_over_one_unavailable_one_within_is_indeterminate(self) -> None:
        # over_count=1 (not decided) and over_count+unavailable=2 (could still tip to
        # "regressed" if the unavailable receipt would have been "over"): undecidable.
        receipts = [
            make_receipt({"startup_cold_ms": "over"}),
            make_receipt({"startup_cold_ms": "unavailable"}),
            make_receipt({"startup_cold_ms": "within"}),
        ]
        result = PERF.regression_verdict(receipts)
        self.assertEqual(result["startup_cold_ms"]["outcome"], "indeterminate")
        self.assertFalse(result["startup_cold_ms"]["regressed"])

    def test_one_unavailable_two_within_is_decisively_not_regressed(self) -> None:
        # Even if the unavailable receipt had been "over", over_count would only reach 1 < 2.
        receipts = [
            make_receipt({"startup_cold_ms": "unavailable"}),
            make_receipt({"startup_cold_ms": "within"}),
            make_receipt({"startup_cold_ms": "within"}),
        ]
        result = PERF.regression_verdict(receipts)
        self.assertEqual(result["startup_cold_ms"]["outcome"], "not_regressed")

    def test_insufficient_samples_counts_as_unavailable_not_within(self) -> None:
        # P-2: two `insufficient_samples` receipts and one `over` must not be
        # decided "not_regressed" -- over_count=1 but over_count+unavailable=3 >= 2,
        # so the outcome is undecidable, not a pass.
        receipts = [
            make_receipt({"startup_cold_ms": "insufficient_samples"}),
            make_receipt({"startup_cold_ms": "insufficient_samples"}),
            make_receipt({"startup_cold_ms": "over"}),
        ]
        result = PERF.regression_verdict(receipts)
        self.assertEqual(result["startup_cold_ms"]["unavailable_count"], 2)
        self.assertEqual(result["startup_cold_ms"]["outcome"], "indeterminate")
        self.assertFalse(result["startup_cold_ms"]["regressed"])

    def test_rejects_wrong_receipt_count(self) -> None:
        for count in (0, 1, 2, 4):
            with self.assertRaises(ValueError):
                PERF.regression_verdict([make_receipt({})] * count)

    def test_rejects_mismatched_budgets_sha256(self) -> None:
        receipts = [
            make_receipt({"a": "within"}, budgets_sha256="sha256:one"),
            make_receipt({"a": "within"}, budgets_sha256="sha256:two"),
            make_receipt({"a": "within"}, budgets_sha256="sha256:one"),
        ]
        with self.assertRaises(ValueError):
            PERF.regression_verdict(receipts)

    def test_rejects_mismatched_profile(self) -> None:
        receipts = [
            make_receipt({"a": "within"}, profile="core"),
            make_receipt({"a": "within"}, profile="local"),
            make_receipt({"a": "within"}, profile="core"),
        ]
        with self.assertRaises(ValueError):
            PERF.regression_verdict(receipts)


class RunCompareTests(unittest.TestCase):
    def write_receipts(self, tmp_path: pathlib.Path, receipts: list[dict]) -> list[str]:
        keys = []
        for index, receipt in enumerate(receipts):
            key = f"r{index}"
            (tmp_path / f"{key}.json").write_text(json.dumps(receipt))
            keys.append(key)
        return keys

    def test_compare_writes_regressed_verdict_and_returns_true(self) -> None:
        receipts = [
            make_receipt({"startup_cold_ms": "over"}),
            make_receipt({"startup_cold_ms": "within"}),
            make_receipt({"startup_cold_ms": "over"}),
        ]
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = pathlib.Path(tmp)
            keys = self.write_receipts(tmp_path, receipts)
            compare_out = tmp_path / "compare.json"
            with mock.patch.object(PERF, "RECEIPTS_DIR", tmp_path), mock.patch.object(
                PERF, "COMPARE_OUT_PATH", compare_out
            ):
                regressed = PERF.run_compare(keys)
            self.assertTrue(regressed)
            payload = json.loads(compare_out.read_text())
            self.assertTrue(payload["regressed"])
            self.assertEqual(payload["verdicts"]["startup_cold_ms"]["outcome"], "regressed")

    def test_compare_all_within_returns_false(self) -> None:
        receipts = [make_receipt({"startup_cold_ms": "within"}) for _ in range(3)]
        with tempfile.TemporaryDirectory() as tmp:
            tmp_path = pathlib.Path(tmp)
            keys = self.write_receipts(tmp_path, receipts)
            with mock.patch.object(PERF, "RECEIPTS_DIR", tmp_path), mock.patch.object(
                PERF, "COMPARE_OUT_PATH", tmp_path / "compare.json"
            ):
                regressed = PERF.run_compare(keys)
            self.assertFalse(regressed)

    def test_invalid_key_is_rejected(self) -> None:
        with self.assertRaises(ValueError):
            PERF.receipt_path_for_key("../etc/passwd")

    def test_valid_key_resolves_under_receipts_dir(self) -> None:
        path = PERF.receipt_path_for_key("smoke-run-1")
        self.assertEqual(path, PERF.RECEIPTS_DIR / "smoke-run-1.json")


class ValidateToolResultPerfTests(unittest.TestCase):
    def test_passed_isError_false_is_valid(self) -> None:
        PERF.validate_tool_result(passed_response(), "rust.catalog.status")  # does not raise

    def test_isError_true_is_rejected(self) -> None:
        response = {"result": {"isError": True, "structuredContent": {"status": "passed"}}}
        with self.assertRaises(RuntimeError):
            PERF.validate_tool_result(response, "rust.catalog.status")

    def test_status_not_passed_is_rejected(self) -> None:
        response = {"result": {"isError": False, "structuredContent": {"status": "blocked"}}}
        with self.assertRaises(RuntimeError):
            PERF.validate_tool_result(response, "rust.catalog.status")

    def test_missing_structured_content_is_rejected(self) -> None:
        response = {"result": {"isError": False}}
        with self.assertRaises(RuntimeError):
            PERF.validate_tool_result(response, "rust.catalog.status")

    def test_missing_result_is_rejected(self) -> None:
        with self.assertRaises(RuntimeError):
            PERF.validate_tool_result({}, "rust.catalog.status")


class CollectValidDispatchSamplesTests(unittest.TestCase):
    def test_all_valid_calls_collect_repeat_samples(self) -> None:
        calls = iter([(passed_response(), 1.0), (passed_response(), 2.0), (passed_response(), 3.0)])
        valid, discarded = PERF.collect_valid_dispatch_samples(lambda: next(calls), "t", 3)
        self.assertEqual([row["elapsed_ms"] for row in valid], [1.0, 2.0, 3.0])
        self.assertEqual(discarded, [])

    def test_invalid_responses_are_discarded_and_retried(self) -> None:
        bad = {"result": {"isError": True, "structuredContent": {"status": "passed"}}}
        calls = iter([(bad, 1.0), (passed_response(), 2.0), (passed_response(), 3.0)])
        valid, discarded = PERF.collect_valid_dispatch_samples(lambda: next(calls), "t", 2)
        self.assertEqual(len(valid), 2)
        self.assertEqual(len(discarded), 1)
        self.assertEqual(discarded[0]["tool"], "t")

    def test_exhausting_attempts_without_enough_valid_samples_raises(self) -> None:
        bad = {"result": {"isError": True, "structuredContent": {"status": "passed"}}}
        with self.assertRaises(RuntimeError):
            PERF.collect_valid_dispatch_samples(lambda: (bad, 1.0), "t", 3)


class NoiseControlHelperTests(unittest.TestCase):
    def test_file_sha256_matches_hashlib(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "f.bin"
            path.write_bytes(b"hello world")
            import hashlib

            expected = hashlib.sha256(b"hello world").hexdigest()
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "f.bin"
            path.write_bytes(b"hello world")
            self.assertEqual(PERF.file_sha256(path), expected)

    def test_head_tree_dirty_true_when_porcelain_has_output(self) -> None:
        completed = mock.Mock(returncode=0, stdout=" M scripts/foo.py\n")
        with mock.patch.object(PERF.subprocess, "run", return_value=completed):
            self.assertTrue(PERF.head_tree_dirty())

    def test_head_tree_dirty_false_when_clean(self) -> None:
        completed = mock.Mock(returncode=0, stdout="")
        with mock.patch.object(PERF.subprocess, "run", return_value=completed):
            self.assertFalse(PERF.head_tree_dirty())

    def test_battery_status_none_when_pmset_absent(self) -> None:
        with mock.patch.object(PERF.pathlib.Path, "is_file", return_value=False):
            self.assertIsNone(PERF.battery_status())

    def test_battery_status_returns_stdout_when_present(self) -> None:
        completed = mock.Mock(returncode=0, stdout="Now drawing from 'AC Power'\n")
        with mock.patch.object(PERF.pathlib.Path, "is_file", return_value=True):
            with mock.patch.object(PERF.subprocess, "run", return_value=completed):
                self.assertEqual(PERF.battery_status(), "Now drawing from 'AC Power'")

    def test_battery_status_none_on_nonzero_exit(self) -> None:
        completed = mock.Mock(returncode=1, stdout="")
        with mock.patch.object(PERF.pathlib.Path, "is_file", return_value=True):
            with mock.patch.object(PERF.subprocess, "run", return_value=completed):
                self.assertIsNone(PERF.battery_status())


class PsParsingTests(unittest.TestCase):
    def test_read_rss_mib_converts_kib_to_mib(self) -> None:
        completed = mock.Mock(returncode=0, stdout=" 131072\n")
        with mock.patch.object(PERF.subprocess, "run", return_value=completed) as run:
            value = PERF.read_rss_mib(4242)
        self.assertEqual(value, 128.0)
        args = run.call_args.args[0]
        self.assertEqual(args, ["/bin/ps", "-o", "rss=", "-p", "4242"])

    def test_read_rss_mib_returns_none_for_exited_process(self) -> None:
        completed = mock.Mock(returncode=1, stdout="")
        with mock.patch.object(PERF.subprocess, "run", return_value=completed):
            self.assertIsNone(PERF.read_rss_mib(4242))

    def test_read_rss_mib_returns_none_on_timeout(self) -> None:
        with mock.patch.object(
            PERF.subprocess, "run", side_effect=PERF.subprocess.TimeoutExpired(cmd="ps", timeout=5)
        ):
            self.assertIsNone(PERF.read_rss_mib(4242))


class FakeServer:
    """Minimal ServerProcess double for the soak's project.open helpers."""

    def __init__(self, project_refs: list[str], fd_sequence: list[int]):
        self.pid = 4242
        self._project_refs = list(project_refs)
        self._fd_sequence = list(fd_sequence)
        self.calls: list[tuple[str, dict]] = []

    def call_tool(self, name: str, arguments: dict) -> dict:
        self.calls.append((name, arguments))
        assert name == "rust.project.open"
        project_ref = self._project_refs.pop(0)
        return {"result": {"structuredContent": {"status": "passed", "data": {"project_ref": project_ref}}}}


class OpenProjectTests(unittest.TestCase):
    def test_open_project_returns_project_ref_from_structured_content(self) -> None:
        server = FakeServer(["prj_aaaa"], [])
        result = SOAK.open_project(server, pathlib.Path("/fixture"))
        self.assertEqual(result, "prj_aaaa")
        self.assertEqual(server.calls, [("rust.project.open", {"path": "/fixture"})])

    def test_open_project_raises_when_status_is_not_passed(self) -> None:
        class BlockedServer(FakeServer):
            def call_tool(self, name: str, arguments: dict) -> dict:
                return {"result": {"structuredContent": {"status": "blocked", "errors": [{"code": "X"}]}}}

        with self.assertRaises(RuntimeError):
            SOAK.open_project(BlockedServer([], []), pathlib.Path("/fixture"))


class OpenChurnTests(unittest.TestCase):
    def test_run_open_churn_reports_fd_growth_and_skips_wait_by_default(self) -> None:
        server = FakeServer(["prj_a", "prj_b", "prj_c"], [])
        with mock.patch.object(SOAK, "count_open_fds", side_effect=[8, 11]) as fds:
            result = SOAK.run_open_churn(server, pathlib.Path("/fixture"), 3, 0)
        self.assertEqual(len(server.calls), 3)
        self.assertEqual(result["count"], 3)
        self.assertEqual(result["fd_count_before"], 8)
        self.assertEqual(result["fd_count_after"], 11)
        self.assertIsNone(result["fd_count_after_ttl_wait"])
        self.assertIsNone(result["fd_count_after_reclaim_open"])
        self.assertEqual(fds.call_count, 2)

    def test_run_open_churn_samples_fds_after_ttl_wait_and_reclaim_open(self) -> None:
        server = FakeServer(["prj_a", "prj_b"], [])
        with mock.patch.object(SOAK, "count_open_fds", side_effect=[8, 9, 9, 10]):
            with mock.patch.object(SOAK.time, "sleep") as sleep:
                result = SOAK.run_open_churn(server, pathlib.Path("/fixture"), 1, 5.0)
        sleep.assert_called_once_with(5.0)
        # one open for the churn itself, one more to trigger lazy reclamation
        self.assertEqual(len(server.calls), 2)
        self.assertEqual(result["fd_count_after_ttl_wait"], 9)
        self.assertEqual(result["fd_count_after_reclaim_open"], 10)


class ValidateToolResultSoakTests(unittest.TestCase):
    def test_passed_is_valid(self) -> None:
        SOAK.validate_tool_result(passed_response(), "rust.catalog.status")  # does not raise

    def test_isError_true_is_rejected(self) -> None:
        response = {"result": {"isError": True, "structuredContent": {"status": "passed"}}}
        with self.assertRaises(RuntimeError):
            SOAK.validate_tool_result(response, "rust.catalog.status")

    def test_status_not_passed_is_rejected(self) -> None:
        response = {"result": {"isError": False, "structuredContent": {"status": "failed"}}}
        with self.assertRaises(RuntimeError):
            SOAK.validate_tool_result(response, "rust.crate.search")


class EvaluateCycleCallsTests(unittest.TestCase):
    def test_two_valid_responses_produce_no_discards(self) -> None:
        discards = SOAK.evaluate_cycle_calls(passed_response(), passed_response())
        self.assertEqual(discards, [])

    def test_invalid_status_response_is_recorded_not_silently_dropped(self) -> None:
        bad = {"result": {"isError": True, "structuredContent": {"status": "passed"}}}
        discards = SOAK.evaluate_cycle_calls(bad, passed_response())
        self.assertEqual(len(discards), 1)
        self.assertEqual(discards[0]["tool"], "rust.catalog.status")

    def test_both_invalid_records_both(self) -> None:
        bad = {"result": {"isError": True, "structuredContent": {"status": "passed"}}}
        discards = SOAK.evaluate_cycle_calls(bad, bad)
        self.assertEqual({row["tool"] for row in discards}, {"rust.catalog.status", "rust.crate.search"})


class ShouldReopenProjectRefTests(unittest.TestCase):
    def test_elapsed_below_ttl_does_not_reopen(self) -> None:
        self.assertFalse(SOAK.should_reopen_project_ref(10.0, 30.0))

    def test_elapsed_at_ttl_reopens(self) -> None:
        self.assertTrue(SOAK.should_reopen_project_ref(30.0, 30.0))

    def test_elapsed_past_ttl_reopens(self) -> None:
        self.assertTrue(SOAK.should_reopen_project_ref(45.0, 30.0))


class EvaluateFdAfterTtlTests(unittest.TestCase):
    def test_not_applicable_without_churn_data(self) -> None:
        result = SOAK.evaluate_fd_after_ttl(None, None, None)
        self.assertFalse(result["applicable"])
        self.assertTrue(result["passed"])

    def test_not_applicable_without_reclaim_open_sample(self) -> None:
        # e.g. --ttl-wait-seconds 0: the wait-only sample was never taken either.
        result = SOAK.evaluate_fd_after_ttl(
            plateau_fds=8, fd_count_after_ttl_wait=None, fd_count_after_reclaim_open=None
        )
        self.assertFalse(result["applicable"])
        self.assertTrue(result["passed"])

    def test_reclaim_open_within_margin_passes_even_if_wait_only_sample_was_high(self) -> None:
        # This is the lazy-expiration case observed in calibration: the wait-only
        # sample stays flat at the post-churn peak (27, well above plateau + 10),
        # but the reclaiming open drops it back down, and that is what the gate
        # evaluates.
        result = SOAK.evaluate_fd_after_ttl(
            plateau_fds=8, fd_count_after_ttl_wait=27, fd_count_after_reclaim_open=10
        )
        self.assertTrue(result["applicable"])
        self.assertTrue(result["passed"])
        self.assertEqual(result["limit_fds"], 18)
        self.assertEqual(result["measured_fds"], 10)
        self.assertEqual(result["retained_until_next_open"], 19)

    def test_reclaim_open_beyond_margin_fails(self) -> None:
        result = SOAK.evaluate_fd_after_ttl(
            plateau_fds=8, fd_count_after_ttl_wait=27, fd_count_after_reclaim_open=19
        )
        self.assertFalse(result["passed"])

    def test_retained_until_next_open_is_none_without_wait_only_sample(self) -> None:
        result = SOAK.evaluate_fd_after_ttl(
            plateau_fds=8, fd_count_after_ttl_wait=None, fd_count_after_reclaim_open=10
        )
        self.assertTrue(result["applicable"])
        self.assertIsNone(result["retained_until_next_open"])


class CheckCatalogStoreOrphansTests(unittest.TestCase):
    def test_only_known_entries_passes(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            store = pathlib.Path(tmp)
            (store / "active.bundle").write_bytes(b"x")
            (store / "store.lock").write_bytes(b"x")
            (store / "floor.record").write_bytes(b"x")
            result = SOAK.check_catalog_store_orphans(store)
        self.assertTrue(result["passed"])
        self.assertEqual(result["orphans"], [])

    def test_stray_file_is_reported_as_orphan_and_fails(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            store = pathlib.Path(tmp)
            (store / "active.bundle").write_bytes(b"x")
            (store / "unexpected.leftover").write_bytes(b"x")
            result = SOAK.check_catalog_store_orphans(store)
        self.assertFalse(result["passed"])
        self.assertEqual(result["orphans"], ["unexpected.leftover"])


class SoakNoiseControlHelperTests(unittest.TestCase):
    def test_head_tree_dirty_true_when_porcelain_has_output(self) -> None:
        completed = mock.Mock(returncode=0, stdout=" M scripts/foo.py\n")
        with mock.patch.object(SOAK.subprocess, "run", return_value=completed):
            self.assertTrue(SOAK.head_tree_dirty())

    def test_battery_status_none_when_pmset_absent(self) -> None:
        with mock.patch.object(SOAK.pathlib.Path, "is_file", return_value=False):
            self.assertIsNone(SOAK.battery_status())

    def test_battery_status_returns_stdout_when_present(self) -> None:
        completed = mock.Mock(returncode=0, stdout="Now drawing from 'AC Power'\n")
        with mock.patch.object(SOAK.pathlib.Path, "is_file", return_value=True):
            with mock.patch.object(SOAK.subprocess, "run", return_value=completed):
                self.assertEqual(SOAK.battery_status(), "Now drawing from 'AC Power'")


class SoakParsingTests(unittest.TestCase):
    def test_count_open_fds_subtracts_lsof_header(self) -> None:
        completed = mock.Mock(returncode=0, stdout="COMMAND PID USER FD TYPE\nx 1 y 3r REG\nx 1 y 4u REG\n")
        with mock.patch.object(SOAK.subprocess, "run", return_value=completed):
            self.assertEqual(SOAK.count_open_fds(1), 2)

    def test_count_open_fds_none_when_lsof_produces_nothing(self) -> None:
        completed = mock.Mock(returncode=1, stdout="")
        with mock.patch.object(SOAK.subprocess, "run", return_value=completed):
            self.assertIsNone(SOAK.count_open_fds(1))

    def test_count_children_counts_pgrep_lines(self) -> None:
        completed = mock.Mock(returncode=0, stdout="111\n222\n")
        with mock.patch.object(SOAK.subprocess, "run", return_value=completed):
            self.assertEqual(SOAK.count_children(1), 2)

    def test_count_children_pgrep_no_match_is_zero_not_a_failure(self) -> None:
        completed = mock.Mock(returncode=1, stdout="")
        with mock.patch.object(SOAK.subprocess, "run", return_value=completed):
            self.assertEqual(SOAK.count_children(1), 0)

    def test_count_children_unexpected_exit_code_is_zero(self) -> None:
        completed = mock.Mock(returncode=2, stdout="")
        with mock.patch.object(SOAK.subprocess, "run", return_value=completed):
            self.assertEqual(SOAK.count_children(1), 0)


def synthetic_samples(rss_series: list[float], fd_series: list[float]) -> list[dict]:
    return [
        {"cycle": i * 20, "elapsed_seconds": float(i), "rss_mib": rss, "fd_count": fd, "child_count": 0}
        for i, (rss, fd) in enumerate(zip(rss_series, fd_series))
    ]


class SoakCriteriaTests(unittest.TestCase):
    def test_stable_series_passes(self) -> None:
        samples = synthetic_samples([100.0] * 10, [8] * 10)
        result = SOAK.evaluate_criteria(samples, overruns=[], state_root_applicable=False)
        self.assertTrue(result["passed"])
        self.assertTrue(result["criteria"]["rss_growth"]["passed"])
        self.assertTrue(result["criteria"]["fd_growth"]["passed"])

    def test_rss_growth_beyond_plateau_times_1_2_fails(self) -> None:
        rss_series = [100.0] + [100.0] * 8 + [130.0]  # final = plateau * 1.3 > limit (* 1.2)
        samples = synthetic_samples(rss_series, [8] * 10)
        result = SOAK.evaluate_criteria(samples, overruns=[], state_root_applicable=False)
        self.assertFalse(result["passed"])
        self.assertFalse(result["criteria"]["rss_growth"]["passed"])

    def test_rss_growth_within_1_2_multiplier_passes(self) -> None:
        rss_series = [100.0] + [100.0] * 8 + [115.0]
        samples = synthetic_samples(rss_series, [8] * 10)
        result = SOAK.evaluate_criteria(samples, overruns=[], state_root_applicable=False)
        self.assertTrue(result["criteria"]["rss_growth"]["passed"])

    def test_fd_growth_beyond_plateau_plus_10_fails(self) -> None:
        fd_series = [8] * 9 + [20]  # final - plateau (8) == 12 > margin (10)
        samples = synthetic_samples([100.0] * 10, fd_series)
        result = SOAK.evaluate_criteria(samples, overruns=[], state_root_applicable=False)
        self.assertFalse(result["passed"])
        self.assertFalse(result["criteria"]["fd_growth"]["passed"])

    def test_fd_growth_within_margin_passes(self) -> None:
        fd_series = [8] * 9 + [17]  # final - plateau (8) == 9 <= margin (10)
        samples = synthetic_samples([100.0] * 10, fd_series)
        result = SOAK.evaluate_criteria(samples, overruns=[], state_root_applicable=False)
        self.assertTrue(result["criteria"]["fd_growth"]["passed"])

    def test_any_cycle_overrun_fails_regardless_of_rss_fd(self) -> None:
        samples = synthetic_samples([100.0] * 10, [8] * 10)
        overruns = [{"cycle": 42, "duration_ms": 999.0}]
        result = SOAK.evaluate_criteria(samples, overruns=overruns, state_root_applicable=False)
        self.assertFalse(result["passed"])
        self.assertFalse(result["criteria"]["cycle_overrun"]["passed"])

    def test_state_root_not_applicable_is_documented_not_failed(self) -> None:
        samples = synthetic_samples([100.0] * 10, [8] * 10)
        result = SOAK.evaluate_criteria(samples, overruns=[], state_root_applicable=False)
        orphans = result["criteria"]["state_root_orphans"]
        self.assertFalse(orphans["applicable"])
        self.assertTrue(orphans["passed"])
        self.assertIsNotNone(orphans["reason"])

    def test_empty_samples_rejected(self) -> None:
        with self.assertRaises(ValueError):
            SOAK.evaluate_criteria([], overruns=[], state_root_applicable=False)

    def test_plateau_is_the_first_5_percent_of_cycles(self) -> None:
        # 21 samples (indices 0..20): 5% of 20 rounds to index 1.
        rss_series = [100.0, 200.0] + [200.0] * 19
        fd_series = [8] * 21
        samples = synthetic_samples(rss_series, fd_series)
        result = SOAK.evaluate_criteria(samples, overruns=[], state_root_applicable=False)
        self.assertEqual(result["plateau_index"], 1)
        self.assertEqual(result["criteria"]["rss_growth"]["plateau_mib"], 200.0)


if __name__ == "__main__":
    unittest.main()
