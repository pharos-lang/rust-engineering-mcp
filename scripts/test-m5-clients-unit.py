#!/usr/bin/env python3
"""Benign, Docker-free, client-free unit tests for the M5 client harness."""
from __future__ import annotations

import importlib.util
import json
import pathlib
import tempfile
import unittest
from unittest import mock

ROOT = pathlib.Path(__file__).resolve().parents[1]
SUBJECT = ROOT / "scripts/test-m5-clients.py"
SPEC = importlib.util.spec_from_file_location("m5_clients", SUBJECT)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("M5 harness unavailable")
M5 = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(M5)


class InventoryTests(unittest.TestCase):
    def test_inventory_is_an_exact_extension_of_the_twenty_seven(self):
        self.assertEqual(M5.EXPECTED_TOOLS[:22], M5.M3_TOOLS)
        self.assertEqual(M5.EXPECTED_TOOLS[22:27], M5.M4_TOOLS)
        self.assertEqual(M5.EXPECTED_TOOLS[27:], M5.M5_TOOLS)
        self.assertEqual(len(M5.EXPECTED_TOOLS), 31)
        self.assertEqual(len(set(M5.EXPECTED_TOOLS)), 31)

    def test_inventory_matches_the_servers_own_protocol_oracle(self):
        self.assertEqual(M5.protocol_inventory(), M5.EXPECTED_TOOLS)

    def test_m5_order_is_the_order_stdio_actually_pushes(self):
        pushed = M5.advertised_push_order()
        self.assertEqual(pushed[:5], M5.M4_TOOLS)
        self.assertEqual(pushed[5:], M5.M5_TOOLS)
        self.assertEqual(len(pushed), 9)

    def test_inventory_check_reports_the_binding_sources(self):
        report = M5.inventory_check()
        self.assertEqual(report["count"], 31)
        self.assertEqual(report["previous_count"], 27)
        self.assertTrue(report["previous_unchanged"])
        self.assertEqual(report["m5_appended"], list(M5.M5_TOOLS))

    def test_drifted_inventory_is_a_failure_and_not_a_warning(self):
        drifted = M5.EXPECTED_TOOLS[:-1]
        with mock.patch.object(M5, "protocol_inventory", return_value=drifted):
            with self.assertRaisesRegex(RuntimeError, "drifted"):
                M5.inventory_check()
        reordered = M5.M5_TOOLS[1:] + M5.M5_TOOLS[:1]
        with mock.patch.object(M5, "advertised_push_order",
                               return_value=M5.M4_TOOLS + reordered):
            with self.assertRaisesRegex(RuntimeError, "different order"):
                M5.inventory_check()


class DockerFreePlanTests(unittest.TestCase):
    def test_every_m5_tool_has_a_positive_and_a_negative_shape(self):
        plan = M5.call_plan()
        shapes: dict[str, set[str]] = {tool: set() for tool in M5.M5_TOOLS}
        for row in plan:
            shapes[row["tool"]].add(row["shape"])
        self.assertTrue(all(value == {"positive", "negative"} for value in shapes.values()))
        self.assertEqual(len(plan), 2 * len(M5.M5_TOOLS))

    def test_no_docker_free_call_claims_a_measurement(self):
        for row in M5.call_plan():
            self.assertEqual(row["mode"], M5.DOCKER_FREE)
            self.assertIn(row["expect_status"], {"blocked", "unavailable"})
            self.assertTrue(row["expect_is_error"])
            self.assertTrue(row["rationale"])
            self.assertNotIn("project_ref", row["arguments"])

    def test_every_expected_code_is_declared_by_its_tool_source(self):
        for row in M5.call_plan() + M5.runtime_call_plan():
            if row["expect_error_code"] is not None:
                self.assertIn(row["expect_error_code"], M5.declared_error_codes(row["tool"]))

    def test_declared_codes_use_the_serde_screaming_snake_vocabulary(self):
        self.assertIn("TASKS_REQUIRED", M5.declared_error_codes("rust.benchmark.run"))
        self.assertIn("MISSING_OFFLINE_DATA", M5.declared_error_codes("rust.binary.bloat"))
        self.assertIn("PROFILING_NOT_AUTHORIZED", M5.declared_error_codes("rust.profile.flamegraph"))
        self.assertIn("ARTIFACT_NOT_FOUND", M5.declared_error_codes("rust.benchmark.compare"))
        self.assertIn("EVIDENCE_INCOMPLETE", M5.declared_error_codes("rust.binary.bloat"))
        self.assertEqual(M5.screaming("NotADataset"), "NOT_A_DATASET")

    def test_an_undeclared_expectation_is_refused(self):
        broken = ({**M5.CALL_PLAN[0], "expect_error_code": "NOT_A_REAL_CODE"},)
        with mock.patch.object(M5, "CALL_PLAN", broken):
            with self.assertRaisesRegex(RuntimeError, "is not declared by"):
                M5.call_plan()

    def test_a_docker_free_plan_that_claims_a_passed_result_is_refused(self):
        broken = ({**M5.CALL_PLAN[0], "expect_status": "passed", "expect_error_code": None},)
        with mock.patch.object(M5, "CALL_PLAN", broken):
            with self.assertRaisesRegex(RuntimeError, "no Docker-free call can produce"):
                M5.call_plan()


class RuntimePlanTests(unittest.TestCase):
    # CORRECTED for ADR-080 §6.  This asserted `runtime_tools()` was exactly
    # profile and bloat, which pinned the state the ADR exists to change: the
    # harness-log recovery has to be exercised from a real client, so
    # `rust.benchmark.run` now has runtime rows too.
    def test_runtime_plan_covers_all_four_m5_tools(self):
        self.assertEqual(
            M5.runtime_tools(),
            ("rust.benchmark.run", "rust.benchmark.compare",
             "rust.profile.flamegraph", "rust.binary.bloat"))
        for row in M5.runtime_call_plan():
            self.assertEqual(row["mode"], M5.RUNTIME)
            self.assertEqual(row["shape"], "positive")
            if row["tool"] == "rust.benchmark.compare":
                self.assertEqual(row["expect_min_artifacts"], 0)
            else:
                self.assertGreaterEqual(row["expect_min_artifacts"], 1)
            self.assertTrue(row["rationale"])

    # The capture-backed positives close the final client-visible gap.
    def test_the_tool_without_a_client_positive_is_named(self):
        missing = [tool for tool in M5.M5_TOOLS if tool not in M5.runtime_tools()]
        self.assertEqual(missing, [])

    def test_the_log_recovery_rows_publish_logs_and_no_measurement(self):
        """ADR-080 §6: an observed compilation failure and an unrecognized
        harness, each publishing harness logs and no dataset."""
        rows = [row for row in M5.runtime_call_plan()
                if row["tool"] == "rust.benchmark.run" and row["expect_status"] == "failed"]
        self.assertEqual(len(rows), 2)
        self.assertEqual([row["expect_error_code"] for row in rows],
                         ["OBSERVED_FAILURE", "HARNESS_UNRECOGNIZED"])
        for row in rows:
            # A declared observed result, not a refusal: a row that never
            # reached the runtime would be blocked/unavailable instead.
            self.assertEqual(row["expect_status"], "failed")
            self.assertFalse(row["expect_is_error"])
            self.assertFalse(row["expect_measured"])
            self.assertEqual(row["expect_observation"]["dataset_published"], False)
            self.assertEqual(row["expect_observation"]["exit_run_index"], 1)
            # Every artifact must be a log, and a dataset or a criterion tree
            # would fail the row even though the artifact count would pass.
            self.assertEqual(row["expect_artifact_kinds"],
                             ["harness_stdout", "harness_stderr"])
            self.assertEqual(row["expect_no_artifact_kinds"],
                             ["benchmark_dataset", "criterion_archive"])
            self.assertGreaterEqual(row["expect_min_artifacts"], 1)
            self.assertIn("logs", row["report_fields"])

    def test_benchmark_positive_rows_feed_the_real_comparison(self):
        rows = M5.runtime_call_plan()
        measured = [row for row in rows if row.get("dataset_role")]
        self.assertEqual([row["dataset_role"] for row in measured],
                         ["baseline", "candidate"])
        for row in measured:
            self.assertEqual(row["expect_status"], "passed")
            self.assertEqual(row["expect_observation"]["dataset_published"], True)
            self.assertEqual(row["expect_observation"]["runs_completed"], 1)
            self.assertIn("benchmark_dataset", row["expect_artifact_kinds"])
        comparison = next(row for row in rows
                          if row["tool"] == "rust.benchmark.compare")
        self.assertEqual(comparison["arguments"], {"timeout_seconds": 30})
        self.assertEqual(comparison["compare_dataset_roles"], ["baseline", "candidate"])
        self.assertEqual(comparison["expect_all_verdicts"], "inconclusive")
        self.assertEqual(comparison["expect_inconclusive_reasons"],
                         ["insufficient_executions"])

    def test_a_blind_comparison_plan_is_refused(self):
        comparison = next(row for row in M5.RUNTIME_CALL_PLAN
                          if row["tool"] == "rust.benchmark.compare")
        broken = ({**comparison, "expect_report": {}},)
        with mock.patch.object(M5, "RUNTIME_CALL_PLAN", broken):
            with self.assertRaisesRegex(RuntimeError, "must assert report facts"):
                M5.runtime_call_plan()

    def test_a_plan_naming_an_artifact_kind_the_tool_cannot_publish_is_refused(self):
        broken = list(M5.RUNTIME_CALL_PLAN)
        broken[-1] = dict(broken[-1], expect_artifact_kinds=["flamegraph_svg"])
        with mock.patch.object(M5, "RUNTIME_CALL_PLAN", tuple(broken)):
            with self.assertRaisesRegex(RuntimeError, "cannot publish"):
                M5.runtime_call_plan()

    def test_the_benchmark_artifact_kinds_come_from_the_servers_own_enum(self):
        self.assertEqual(
            M5.declared_artifact_kinds("rust.benchmark.run"),
            frozenset({"benchmark_dataset", "criterion_archive",
                       "harness_stdout", "harness_stderr"}))

    def test_profile_expects_a_real_passed_with_two_artifacts(self):
        row = next(item for item in M5.runtime_call_plan()
                   if item["tool"] == "rust.profile.flamegraph")
        self.assertEqual(row["expect_status"], "passed")
        self.assertIsNone(row["expect_error_code"])
        self.assertFalse(row["expect_is_error"])
        self.assertEqual(row["expect_min_artifacts"], 2)
        self.assertTrue(row["requires_profiling_grant"])
        self.assertEqual(row["expect_observation"]["complete"], True)
        self.assertIn("samples_collected", row["expect_positive_fields"])
        self.assertIn("samples_lost", row["expect_zero_fields"])

    def test_bloat_expects_the_status_the_product_actually_produces(self):
        # ADR-079: the parser still caps functions at BLOAT_MAX_ROWS, but the
        # cap is declared coverage now, not a completeness downgrade, so a real
        # binary reaches passed with the exact measurement in hand.  Before, the
        # cap became Truncated -> complete=false -> blocked/EVIDENCE_INCOMPLETE,
        # which no binary linking std could avoid.
        row = next(item for item in M5.runtime_call_plan()
                   if item["tool"] == "rust.binary.bloat")
        self.assertEqual(row["expect_status"], "passed")
        self.assertIsNone(row["expect_error_code"])
        self.assertTrue(row["expect_measured"])
        self.assertEqual(row["expect_observation"]["exit"], "passed")
        self.assertEqual(row["expect_observation"]["completeness"], "complete")
        self.assertTrue(row["expect_observation"]["analysis_validated"])
        self.assertFalse(row["requires_profiling_grant"])
        source = (M5.STDIO_DIR / "bloat.rs").read_text()
        # Only measurement validity decides the status; neither omission
        # counter is read where the outcome is chosen.
        self.assertIn("if observation.analysis_validated {", source)
        self.assertNotIn("BloatCompleteness::Truncated", source)
        domain = (M5.ROOT / "crates/domain/src/bloat.rs").read_text()
        self.assertIn("pub const BLOAT_MAX_ROWS: usize = 256;", domain)
        self.assertIn("pub fn analysis_validated(&self) -> bool {", domain)

    def test_a_runtime_row_without_evidence_is_refused(self):
        execution = next(row for row in M5.RUNTIME_CALL_PLAN
                         if row["tool"] == "rust.profile.flamegraph")
        broken = ({**execution, "expect_min_artifacts": 0},)
        with mock.patch.object(M5, "RUNTIME_CALL_PLAN", broken):
            with self.assertRaisesRegex(RuntimeError, "must publish evidence"):
                M5.runtime_call_plan()
        blind = ({**execution, "expect_observation": {},
                  "expect_positive_fields": [], "expect_measured": False},)
        with mock.patch.object(M5, "RUNTIME_CALL_PLAN", blind):
            with self.assertRaisesRegex(RuntimeError, "must assert observation facts"):
                M5.runtime_call_plan()

    def test_a_negative_shape_is_refused_in_the_runtime_plan(self):
        broken = ({**M5.RUNTIME_CALL_PLAN[0], "shape": "negative"},)
        with mock.patch.object(M5, "RUNTIME_CALL_PLAN", broken):
            with self.assertRaisesRegex(RuntimeError, "positives only"):
                M5.runtime_call_plan()


class ObservationOracleTests(unittest.TestCase):
    def profile_payload(self, **overrides):
        observation = {
            "build": "built", "completeness": "complete", "complete": True,
            "samples_collected": 195, "stacks_written": 2, "frames_total": 2148,
            "samples_lost": 0, "stacks_truncated": 0, "frames_unresolved": 196,
            "modules_seen": 5,
        }
        observation.update(overrides)
        return {"status": "passed", "error_code": None, "data": {
            "observation": observation,
            "artifacts": [
                {"kind": "flamegraph_svg", "uri": "rust-quality-artifact://" + "a" * 32 + "/1",
                 "sha256": "b" * 64, "size_bytes": 3969, "completeness": "complete"},
                {"kind": "collapsed_stacks", "uri": "rust-quality-artifact://" + "a" * 32 + "/2",
                 "sha256": "c" * 64, "size_bytes": 1170, "completeness": "complete"},
            ]}}

    def profile_row(self):
        return next(item for item in M5.runtime_call_plan()
                    if item["tool"] == "rust.profile.flamegraph")

    def test_a_real_profile_observation_is_accepted_and_reported(self):
        checked = M5.check_runtime_observation("Codex", self.profile_row(), self.profile_payload())
        self.assertEqual(len(checked["artifacts"]), 2)
        self.assertEqual(checked["facts"]["samples_collected"], 195)
        self.assertEqual(checked["facts"]["artifacts_published"], 2)

    def test_a_degraded_profile_is_refused(self):
        row = self.profile_row()
        with self.assertRaisesRegex(RuntimeError, "observation.samples_lost is not zero"):
            M5.check_runtime_observation("Codex", row, self.profile_payload(samples_lost=3))
        with self.assertRaisesRegex(RuntimeError, "observation.complete is not True"):
            M5.check_runtime_observation("Codex", row, self.profile_payload(complete=False))
        with self.assertRaisesRegex(RuntimeError, "is not a positive count"):
            M5.check_runtime_observation("Codex", row, self.profile_payload(samples_collected=0))

    def test_too_few_or_invalid_artifacts_are_refused(self):
        payload = self.profile_payload()
        payload["data"]["artifacts"] = payload["data"]["artifacts"][:1]
        with self.assertRaisesRegex(RuntimeError, "published too few artifacts"):
            M5.check_runtime_observation("Codex", self.profile_row(), payload)
        payload = self.profile_payload()
        payload["data"]["artifacts"][0]["uri"] = "file:///etc/passwd"
        with self.assertRaisesRegex(RuntimeError, "invalid artifact descriptor"):
            M5.check_runtime_observation("Codex", self.profile_row(), payload)

    def test_bloat_requires_an_exact_measured_file(self):
        row = next(item for item in M5.runtime_call_plan() if item["tool"] == "rust.binary.bloat")
        # ADR-079's shape: the ranking cap dropped 378 rows and says so, the
        # measurement is valid regardless, and the artifact behind it is
        # complete because the raw report is complete whatever the cap showed.
        payload = {"status": "passed", "error_code": None, "data": {
            "observation": {"exit": "passed", "analysis_validated": True,
                            "completeness": "complete",
                            "exit_code": 0, "analyzer_version": "0.12.1",
                            "attribution": {"estimated": True,
                                            "reported_file_size_bytes": 4574312,
                                            "ranking_cap": {"max_rows": 256,
                                                            "functions_omitted": 378,
                                                            "crates_omitted": 0},
                                            "response_trim": {"budget_bytes": 524288,
                                                              "functions_omitted": 0,
                                                              "crates_omitted": 0}},
                            "measured": {"size_bytes": 4574312, "sha256": "sha256:" + "d" * 64,
                                         "format": "elf64_aarch64",
                                         "analysis_build_symbols_forced": True}},
            "artifacts": [{"kind": "bloat_json", "uri": "rust-quality-artifact://" + "a" * 32 + "/1",
                           "sha256": "e" * 64, "size_bytes": 20480, "completeness": "complete"}]}}
        checked = M5.check_runtime_observation("Inspector", row, payload)
        self.assertEqual(checked["facts"]["exit"], "passed")
        payload["data"]["observation"]["measured"]["analysis_build_symbols_forced"] = False
        with self.assertRaisesRegex(RuntimeError, "no measured binary"):
            M5.check_runtime_observation("Inspector", row, payload)

    def comparison_row(self):
        return next(item for item in M5.runtime_call_plan()
                    if item["tool"] == "rust.benchmark.compare")

    def comparison_payload(self, verdict="inconclusive", reasons=None):
        if reasons is None:
            reasons = ["insufficient_executions"]
        return {"status": "passed", "error_code": None, "data": {
            "baseline_artifact_id": "qart_" + "a" * 32,
            "candidate_artifact_id": "qart_" + "b" * 32,
            "report": {"complete": True, "incompatibility_reasons": [], "compared": 1,
                       "comparisons": [{"key": "m5/control", "verdict": verdict,
                                        "inconclusive_reasons": reasons}]}}}

    def test_store_issued_dataset_ids_are_extracted_and_materialized(self):
        uri = ("rust-quality-artifact://prj_" + "1" * 32 + "/qart_" + "a" * 32
               + "?offset=0&length=42")
        self.assertEqual(M5.artifact_id_from_uri(uri), "qart_" + "a" * 32)
        with self.assertRaisesRegex(RuntimeError, "store-issued identifier"):
            M5.artifact_id_from_uri("qart_" + "a" * 32)
        datasets = {"baseline": "qart_" + "a" * 32,
                    "candidate": "qart_" + "b" * 32}
        args = M5.materialize_arguments(self.comparison_row(), "prj_" + "1" * 32, datasets)
        self.assertEqual(args["baseline_artifact_id"], datasets["baseline"])
        self.assertEqual(args["candidate_artifact_id"], datasets["candidate"])
        with self.assertRaisesRegex(RuntimeError, "not captured"):
            M5.materialize_arguments(self.comparison_row(), "prj_" + "1" * 32, {})

    def test_comparison_requires_the_exact_early_dataset_guard(self):
        checked = M5.check_runtime_comparison(
            "Codex", self.comparison_row(), self.comparison_payload())
        self.assertEqual(checked["artifacts"], [])
        self.assertEqual(checked["facts"]["verdicts"], ["inconclusive"])
        self.assertEqual(checked["facts"]["inconclusive_reasons"],
                         ["insufficient_executions"])
        self.assertNotIn("method_unqualified", checked["facts"]["inconclusive_reasons"])
        with self.assertRaisesRegex(RuntimeError, "directional"):
            M5.check_runtime_comparison(
                "Codex", self.comparison_row(), self.comparison_payload("regression", []))
        with self.assertRaisesRegex(RuntimeError, "dataset guard"):
            M5.check_runtime_comparison(
                "Codex", self.comparison_row(), self.comparison_payload(reasons=[]))
        with self.assertRaisesRegex(RuntimeError, "dataset guard"):
            M5.check_runtime_comparison(
                "Codex", self.comparison_row(),
                self.comparison_payload(reasons=["insufficient_executions",
                                                 "method_unqualified"]))
        empty = self.comparison_payload()
        empty["data"]["report"]["comparisons"] = []
        with self.assertRaisesRegex(RuntimeError, "no benchmark comparisons"):
            M5.check_runtime_comparison("Codex", self.comparison_row(), empty)

    def test_dataset_role_requires_one_unique_pooled_artifact(self):
        row = next(item for item in M5.runtime_call_plan()
                   if item.get("dataset_role") == "baseline")
        artifact = {"kind": "benchmark_dataset",
                    "uri": "rust-quality-artifact://prj_" + "1" * 32
                           + "/qart_" + "a" * 32 + "?offset=0&length=42"}
        datasets = {}
        M5.capture_dataset_id(row, [artifact], datasets)
        self.assertEqual(datasets, {"baseline": "qart_" + "a" * 32})
        with self.assertRaisesRegex(RuntimeError, "invalid or duplicated"):
            M5.capture_dataset_id(row, [artifact], datasets)


class ClaudeTranscriptTests(unittest.TestCase):
    """Event shapes copied from a real `claude -p --output-format stream-json` probe."""

    PROJECT_REF = "prj_06638fd78de867ba11cda2cbe0c85bc3"

    @staticmethod
    def init(model="claude-sonnet-5", version="2.1.267", tools=None, servers=None):
        return {"type": "system", "subtype": "init", "model": model,
                "claude_code_version": version, "apiKeySource": "none",
                "mcp_servers": servers if servers is not None
                else [{"name": "rust_engineering", "status": "connected"}],
                "tools": tools if tools is not None
                else ["ListMcpResourcesTool", "ReadMcpResourceTool",
                      "mcp__rust_engineering__rust_benchmark_compare",
                      "mcp__rust_engineering__rust_project_open"]}

    @staticmethod
    def final(subtype="success", is_error=False, denials=(), usage=None):
        return {"type": "result", "subtype": subtype, "is_error": is_error, "num_turns": 5,
                "duration_ms": 11130, "permission_denials": list(denials),
                "modelUsage": usage if usage is not None
                else {"claude-haiku-4-5-20251001": {}, "claude-sonnet-5": {}}}

    @staticmethod
    def use(identifier, name, arguments, model="claude-sonnet-5"):
        return {"type": "assistant", "message": {"model": model, "content": [
            {"type": "tool_use", "id": identifier, "name": name, "input": arguments}]}}

    @staticmethod
    def result(identifier, content, is_error=None, structured=None):
        block = {"type": "tool_result", "tool_use_id": identifier, "content": content}
        if is_error is not None:
            block["is_error"] = is_error
        event = {"type": "user", "message": {"content": [block]}}
        if structured is not None:
            event["tool_use_result"] = structured
        return event

    def transcript(self):
        opened = json.dumps({"data": {"project_ref": self.PROJECT_REF}, "status": "passed",
                             "error_code": None})
        refused = json.dumps({"data": None, "status": "blocked",
                              "error_code": "ARTIFACT_NOT_FOUND"})
        return [
            self.init(),
            {"type": "rate_limit_event", "rate_limit_info": {"status": "allowed"}},
            self.use("t1", "ListMcpResourcesTool", {"server": "rust_engineering"}),
            self.result("t1", "No resources found.", structured=[]),
            self.use("t2", "mcp__rust_engineering__rust_project_open", {"path": "/p"}),
            self.result("t2", opened, structured={"content": opened}),
            self.use("t3", "mcp__rust_engineering__rust_benchmark_compare",
                     {"project_ref": self.PROJECT_REF}),
            self.result("t3", refused, is_error=True, structured="Error: " + refused),
            self.use("t4", "ReadMcpResourceTool",
                     {"server": "rust_engineering", "uri": "rust-quality-artifact://x/y"}),
            self.result("t4", "Resource not found", structured={"contents": [],
                                                                "error": "Resource not found"}),
            self.final(),
        ]

    def test_transcript_normalizes_to_closed_items_in_call_order(self):
        init, items, final = M5.claude_items(self.transcript())
        self.assertEqual([item["tool"] for item in items], [
            "list_mcp_resources", "rust.project.open", "rust.benchmark.compare",
            "read_mcp_resource"])
        self.assertEqual(items[0]["status"], "completed")
        self.assertEqual(items[0]["result"]["resources"], 0)
        self.assertEqual(items[1]["result"]["structuredContent"]["data"]["project_ref"],
                         self.PROJECT_REF)
        self.assertEqual(items[2]["status"], "failed")
        self.assertTrue(items[2]["result"]["isError"])
        self.assertEqual(items[2]["result"]["structuredContent"]["error_code"],
                         "ARTIFACT_NOT_FOUND")
        self.assertEqual(items[3]["status"], "failed")
        self.assertEqual(items[3]["error"], "Resource not found")
        session = M5.validate_claude_session(init, final, self.transcript())
        self.assertEqual(session["resolved_model"], M5.CLAUDE_MODEL)
        self.assertEqual(session["assistant_messages"], 4)
        self.assertEqual(session["observed_models"],
                         ["claude-haiku-4-5-20251001", "claude-sonnet-5"])

    def test_a_read_resource_carries_its_measured_contents(self):
        uri = "rust-quality-artifact://prj_" + "1" * 32 + "/qart_" + "2" * 32 + "?offset=0&length=1"
        def read(structured):
            events = [self.init(), self.use("r", "ReadMcpResourceTool",
                                            {"server": "rust_engineering", "uri": uri}),
                      self.result("r", "ok", structured=structured), self.final()]
            return M5.claude_items(events)[1][0]
        digest = M5.load_m3().digest
        descriptor = {"uri": uri, "sha256": digest(b"a"), "size_bytes": 1}
        inline = read({"contents": [{"uri": uri, "blob": "YQ=="}]})
        evidence = M5.validate_model_resource_read(inline, descriptor)
        self.assertEqual((evidence["kind"], evidence["whole_artifact"]), ("blob", True))
        with self.assertRaisesRegex(RuntimeError, "did not match the issued artifact"):
            M5.validate_model_resource_read(
                inline, {**descriptor, "uri": uri.replace("length=1", "length=2")})
        with self.assertRaisesRegex(RuntimeError, "another artifact"):
            M5.validate_model_resource_read(
                read({"contents": [{"uri": uri + "1", "blob": "YQ=="}]}), descriptor)
        with self.assertRaisesRegex(RuntimeError, "not the chunk it read"):
            M5.validate_model_resource_read(
                read({"contents": [{"uri": uri, "blob": "YWI="}]}), descriptor)
        with self.assertRaisesRegex(RuntimeError, "does not hash to the published artifact"):
            M5.validate_model_resource_read(
                read({"contents": [{"uri": uri, "blob": "Yg=="}]}), descriptor)
        # A chunk shorter than the artifact is measured, never hashed against it.
        prefix = M5.validate_model_resource_read(
            read({"contents": [{"uri": uri, "blob": "Yg=="}]}),
            {**descriptor, "size_bytes": 2})
        self.assertFalse(prefix["whole_artifact"])
        text = read({"contents": [{"uri": uri, "mimeType": "text/plain", "text": "a"}]})
        self.assertEqual(M5.validate_model_resource_read(text, descriptor)["kind"], "text")
        # Claude's own note beside a binary it saved elsewhere is not content.
        note = read({"contents": [{"uri": uri, "mimeType": "application/octet-stream",
                                   "blob": "", "text": "[Resource ...] Binary content saved"}]})
        with self.assertRaisesRegex(RuntimeError, "carried no content"):
            M5.validate_model_resource_read(note, descriptor)
        with tempfile.TemporaryDirectory() as directory:
            saved = pathlib.Path(directory) / "blob.bin"
            saved.write_bytes(b"a")
            item = read({"contents": [{"uri": uri, "mimeType": "application/octet-stream",
                                       "blob": "", "text": "note", "blobSavedTo": str(saved)}]})
            evidence = M5.validate_model_resource_read(item, descriptor)
            self.assertEqual(evidence["kind"], "blob_saved_by_client")
            self.assertEqual(evidence["bytes"], 1)
            missing = read({"contents": [{"uri": uri, "blob": "", "text": "note",
                                          "blobSavedTo": str(saved) + ".gone"}]})
            with self.assertRaisesRegex(RuntimeError, "does not exist"):
                M5.validate_model_resource_read(missing, descriptor)

    def test_an_error_prefixed_payload_still_parses(self):
        refused = json.dumps({"status": "blocked", "error_code": "NOT_A_DATASET"})
        self.assertEqual(M5.claude_structured_payload("Error: " + refused)["error_code"],
                         "NOT_A_DATASET")
        self.assertIsNone(M5.claude_structured_payload("Error: not json"))

    def test_credential_shaped_transcripts_are_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            clean = pathlib.Path(directory) / "clean.jsonl"
            clean.write_text('{"type":"result","status":"PROFILING_NOT_AUTHORIZED"}\n')
            M5.assert_no_credential_text(clean)
            leaky = pathlib.Path(directory) / "leaky.stderr"
            leaky.write_text("debug: Authorization: Bearer sk-ant-abc\n")
            with self.assertRaisesRegex(RuntimeError, "credential-shaped text"):
                M5.assert_no_credential_text(leaky)

    def test_malformed_transcripts_are_refused(self):
        events = self.transcript()
        with self.assertRaisesRegex(RuntimeError, "lacks its init or result"):
            M5.claude_items(events[1:])
        with self.assertRaisesRegex(RuntimeError, "unmatched tool results"):
            M5.claude_items([event for event in events if event.get("type") != "user"])
        duplicated = [*events[:3], events[2], *events[3:]]
        with self.assertRaisesRegex(RuntimeError, "duplicated or missing"):
            M5.claude_items(duplicated)
        foreign = [self.init(), self.use("f", "Bash", {"command": "id"}),
                   self.result("f", "uid=0"), self.final()]
        with self.assertRaisesRegex(RuntimeError, "outside the configured server"):
            M5.claude_items(foreign)
        unknown = [self.init(), self.use("u", "mcp__rust_engineering__rust_nope", {}),
                   self.result("u", "{}"), self.final()]
        with self.assertRaisesRegex(RuntimeError, "does not advertise"):
            M5.claude_items(unknown)

    def test_session_must_be_the_pinned_client_and_model_on_the_configured_server(self):
        final = self.final()
        turn = self.transcript()
        M5.validate_claude_session(self.init(), final, turn)
        with self.assertRaisesRegex(RuntimeError, "another model"):
            M5.validate_claude_session(self.init(model="claude-haiku-4-5"), final, turn)
        # A fallback mid-turn shows up as an assistant message from another
        # model while the pinned one still appears in modelUsage.
        fallback = [*turn, self.use("t9", "ListMcpResourcesTool", {"server": "rust_engineering"},
                                    model="claude-opus-5"),
                    self.result("t9", "No resources found.", structured=[])]
        with self.assertRaisesRegex(RuntimeError, "message from another model"):
            M5.validate_claude_session(self.init(), final, fallback)
        with self.assertRaisesRegex(RuntimeError, "message from another model"):
            M5.validate_claude_session(self.init(), final, [self.init(), final])
        with self.assertRaisesRegex(RuntimeError, "another version"):
            M5.validate_claude_session(self.init(version="2.1.260"), final, turn)
        M5.validate_claude_session(self.init(servers=[
            {"name": "rust_engineering", "status": "connected", "type": "stdio"}]), final, turn)
        with self.assertRaisesRegex(RuntimeError, "exactly the configured server"):
            M5.validate_claude_session(self.init(servers=[
                {"name": "rust_engineering", "status": "connected"},
                {"name": "other", "status": "connected"}]), final, turn)
        with self.assertRaisesRegex(RuntimeError, "built-in capability"):
            M5.validate_claude_session(self.init(tools=["Bash", "ListMcpResourcesTool"]), final, turn)
        with self.assertRaisesRegex(RuntimeError, "did not finish"):
            M5.validate_claude_session(self.init(), self.final(subtype="error_max_turns"), turn)
        with self.assertRaisesRegex(RuntimeError, "denied a capability"):
            M5.validate_claude_session(self.init(), self.final(denials=[{"tool_name": "Bash"}]), turn)
        with self.assertRaisesRegex(RuntimeError, "no usage for the pinned model"):
            M5.validate_claude_session(self.init(), self.final(usage={"claude-opus-5": {}}), turn)


def item(tool, arguments, payload, is_error=False, server="rust_engineering"):
    """The closed item shape `claude_items` produces for one MCP tool call."""
    return {"type": "mcpToolCall", "server": server, "tool": tool, "arguments": arguments,
            "status": "failed" if is_error else "completed", "error": None,
            "result": {"structuredContent": payload, "isError": is_error}}


def opened(name, reference):
    return item("rust.project.open", {"path": str(M5.ROOT / M5.FIXTURES[name])},
                {"status": "passed", "error_code": None, "data": {"project_ref": reference}})


class ModelDirectedDockerFreeTests(unittest.TestCase):
    def setUp(self):
        self.plan = M5.call_plan()
        self.positive = {row["tool"]: row for row in self.plan if row["shape"] == "positive"}
        self.refs = {name: "prj_" + str(index) * 32
                     for index, name in enumerate(("benchmark", "profile", "bloat"), start=1)}

    def items(self):
        items = [opened(name, reference) for name, reference in self.refs.items()]
        for tool in M5.M5_TOOLS:
            row = self.positive[tool]
            items.append(item(tool, {"project_ref": self.refs[row["project"]], **row["arguments"]},
                              {"status": row["expect_status"],
                               "error_code": row["expect_error_code"], "data": None},
                              is_error=row["expect_is_error"]))
        return items

    def test_the_four_planned_refusals_are_bound_to_their_opened_roots(self):
        facts = M5.validate_docker_free_model_flow(self.items(), self.plan)
        self.assertEqual(facts["opened_roots"], ["benchmark", "bloat", "profile"])
        self.assertEqual(set(facts["refusals"]), set(M5.M5_TOOLS))
        self.assertEqual(facts["refusals"]["rust.profile.flamegraph"]["error_code"],
                         "PROFILING_NOT_AUTHORIZED")

    def test_retries_foreign_capabilities_wrong_arguments_and_undeclared_results_are_refused(self):
        items = self.items()
        with self.assertRaisesRegex(RuntimeError, "retried or omitted"):
            M5.validate_docker_free_model_flow([*items, items[-1]], self.plan)
        with self.assertRaisesRegex(RuntimeError, "retried or omitted"):
            M5.validate_docker_free_model_flow(items[:-1], self.plan)
        with self.assertRaisesRegex(RuntimeError, "another MCP capability"):
            M5.validate_docker_free_model_flow(
                [*items, item("rust.check", {"project_ref": self.refs["benchmark"]},
                              {"status": "passed"})], self.plan)
        wrong = self.items()
        wrong[-1]["arguments"]["timeout_seconds"] = 1
        with self.assertRaisesRegex(RuntimeError, "planned arguments"):
            M5.validate_docker_free_model_flow(wrong, self.plan)
        undeclared = self.items()
        undeclared[-1]["result"]["structuredContent"]["status"] = "passed"
        with self.assertRaisesRegex(RuntimeError, "does not declare"):
            M5.validate_docker_free_model_flow(undeclared, self.plan)
        unopened = [entry for entry in self.items() if entry["tool"] != "rust.project.open"]
        with self.assertRaisesRegex(RuntimeError, "before opening"):
            M5.validate_docker_free_model_flow(unopened, self.plan)
        outside = self.items()
        outside[0]["arguments"]["path"] = "/Users/somebody/project"
        with self.assertRaisesRegex(RuntimeError, "outside the plan"):
            M5.validate_docker_free_model_flow(outside, self.plan)
        unplanned = [opened("benchmark_compile_error", "prj_" + "9" * 32), *self.items()]
        with self.assertRaisesRegex(RuntimeError, "outside the plan"):
            M5.validate_docker_free_model_flow(unplanned, self.plan)
        reopened = [*self.items(), opened("benchmark", self.refs["benchmark"])]
        with self.assertRaisesRegex(RuntimeError, "retried or omitted"):
            M5.validate_docker_free_model_flow(reopened, self.plan)
        discovery = self.items()
        discovery.insert(3, {"type": "mcpToolCall", "server": "rust_engineering",
                             "tool": "list_mcp_resources",
                             "arguments": {"server": "rust_engineering"},
                             "status": "completed", "error": None,
                             "result": {"structuredContent": None, "resources": 0}})
        with self.assertRaisesRegex(RuntimeError, "another MCP capability"):
            M5.validate_docker_free_model_flow(discovery, self.plan)
        # The four refusals are independent declared results: a client that
        # batches them in another order is accepted, an open after them is not.
        swapped = self.items()
        swapped[-1], swapped[-2] = swapped[-2], swapped[-1]
        M5.validate_docker_free_model_flow(swapped, self.plan)
        late_open = self.items()
        late_open.append(late_open.pop(0))
        with self.assertRaisesRegex(RuntimeError, "before opening|out of order"):
            M5.validate_docker_free_model_flow(late_open, self.plan)
        late_unrelated_open = self.items()
        late_unrelated_open.append(late_unrelated_open.pop(2))
        with self.assertRaisesRegex(RuntimeError, "before opening|out of order"):
            M5.validate_docker_free_model_flow(late_unrelated_open, self.plan)


class ModelDirectedRuntimeTests(unittest.TestCase):
    def setUp(self):
        self.plan = M5.runtime_call_plan()
        self.rows = {row["fact_key"]: row for row in self.plan}
        self.reference = "prj_" + "1" * 32
        self.baseline = "qart_" + "a" * 32
        self.candidate = "qart_" + "b" * 32
        self.archive = "qart_" + "c" * 32
        self.log = "qart_" + "d" * 32
        self.archive_uri = self.uri(self.archive)

    def uri(self, identifier):
        return f"rust-quality-artifact://{self.reference}/{identifier}?offset=0&length=1"

    def artifact(self, kind, identifier, run_index=1):
        return {"uri": self.uri(identifier), "sha256": M5.load_m3().digest(b"a"), "size_bytes": 1,
                "kind": kind, "run_index": run_index}

    def run_payload(self, dataset, other):
        row = self.rows["benchmark_baseline"]
        observation = {**row["expect_observation"], "exit_code": 0}
        return {"status": "passed", "error_code": None, "data": {
            "observation": observation,
            "artifacts": [self.artifact("benchmark_dataset", dataset, None),
                          self.artifact("criterion_archive", other),
                          self.artifact("harness_stdout", self.log)]}}

    def compare_payload(self, baseline, candidate):
        return {"status": "passed", "error_code": None, "data": {
            "baseline_artifact_id": baseline, "candidate_artifact_id": candidate,
            "report": {"complete": True, "incompatibility_reasons": [], "compared": 3,
                       "comparisons": [{"verdict": "inconclusive",
                                        "inconclusive_reasons": ["insufficient_executions"]}]}}}

    def discovery(self):
        return {"type": "mcpToolCall", "server": "rust_engineering",
                "tool": "list_mcp_resources", "arguments": {"server": "rust_engineering"},
                "status": "completed", "error": None,
                "result": {"structuredContent": None, "resources": 0}}

    def read(self, uri):
        return {"type": "mcpToolCall", "server": "rust_engineering", "tool": "read_mcp_resource",
                "arguments": {"server": "rust_engineering", "uri": uri},
                "status": "completed", "error": None,
                "result": {"structuredContent": None,
                           "content": [{"type": "resource",
                                        "resource": {"uri": uri, "blob": "YQ=="}}]}}

    def items(self):
        run_arguments = {"project_ref": self.reference, **self.rows["benchmark_baseline"]["arguments"]}
        positive = {"project_ref": self.reference, **self.rows["benchmark_compare"]["arguments"],
                    "baseline_artifact_id": self.baseline, "candidate_artifact_id": self.candidate}
        return [
            opened("benchmark", self.reference),
            self.discovery(),
            item("rust.benchmark.run", run_arguments, self.run_payload(self.baseline, self.archive)),
            item("rust.benchmark.run", run_arguments,
                 self.run_payload(self.candidate, "qart_" + "f" * 32)),
            item("rust.benchmark.compare", positive,
                 self.compare_payload(self.baseline, self.candidate)),
            item("rust.benchmark.compare", {**positive, "candidate_artifact_id": self.archive},
                 {"status": "blocked", "error_code": "NOT_A_DATASET", "data": None},
                 is_error=True),
            self.read(self.archive_uri),
        ]

    def test_runtime_model_flow_binds_measurements_comparison_failure_and_resource(self):
        facts = M5.validate_runtime_model_flow(self.items(), self.plan)
        self.assertEqual(facts["measurements"], 2)
        self.assertEqual(facts["failure"], "NOT_A_DATASET")
        self.assertEqual(facts["comparison"]["verdicts"], ["inconclusive"])
        self.assertEqual(facts["comparison"]["inconclusive_reasons"], ["insufficient_executions"])
        self.assertEqual(len(facts["resource_uri_sha256"]), 64)
        self.assertEqual(facts["resource_content"], {
            "kind": "blob", "bytes": 1, "sha256": M5.load_m3().digest(b"a"),
            "whole_artifact": True})

    def test_runtime_model_flow_rejects_prompt_only_retried_or_forged_evidence(self):
        items = self.items()
        with self.assertRaisesRegex(RuntimeError, "retried or omitted"):
            M5.validate_runtime_model_flow(items[:-1], self.plan)
        with self.assertRaisesRegex(RuntimeError, "retried or omitted"):
            M5.validate_runtime_model_flow([*items, items[4]], self.plan)
        with self.assertRaisesRegex(RuntimeError, "another MCP capability"):
            M5.validate_runtime_model_flow(
                [*items, item("rust.binary.bloat", {}, {"status": "passed"})], self.plan)
        same_dataset = self.items()
        same_dataset[3]["result"]["structuredContent"]["data"]["artifacts"][0] = \
            self.artifact("benchmark_dataset", self.baseline, None)
        with self.assertRaisesRegex(RuntimeError, "same dataset identifier"):
            M5.validate_runtime_model_flow(same_dataset, self.plan)
        forged = self.items()
        forged[5]["arguments"]["candidate_artifact_id"] = "qart_" + "9" * 32
        with self.assertRaisesRegex(RuntimeError, "declared comparison failure"):
            M5.validate_runtime_model_flow(forged, self.plan)
        directional = self.items()
        directional[4]["result"]["structuredContent"]["data"]["report"]["comparisons"][0] = {
            "verdict": "regression", "inconclusive_reasons": []}
        with self.assertRaisesRegex(RuntimeError, "directional"):
            M5.validate_runtime_model_flow(directional, self.plan)
        other_resource = self.items()
        other_resource[6] = self.read(self.uri(self.log))
        with self.assertRaisesRegex(RuntimeError, "did not match the issued artifact"):
            M5.validate_runtime_model_flow(other_resource, self.plan)
        out_of_order = self.items()
        out_of_order[4], out_of_order[5] = out_of_order[5], out_of_order[4]
        with self.assertRaisesRegex(RuntimeError, "positive comparison"):
            M5.validate_runtime_model_flow(out_of_order, self.plan)
        late_open = self.items()
        late_open.append(late_open.pop(0))
        with self.assertRaisesRegex(RuntimeError, "out of order"):
            M5.validate_runtime_model_flow(late_open, self.plan)
        unopened = self.items()[1:]
        with self.assertRaisesRegex(RuntimeError, "exactly the benchmark root"):
            M5.validate_runtime_model_flow(unopened, self.plan)


class PromptTests(unittest.TestCase):
    def test_prompts_are_rendered_from_the_plan_rows(self):
        docker_free = M5.claude_prompt(M5.DOCKER_FREE, M5.call_plan())
        for tool in M5.M5_TOOLS:
            self.assertIn(f"Call {tool} with project_ref", docker_free)
        self.assertIn(str(M5.ROOT / M5.FIXTURES["benchmark"]), docker_free)
        self.assertIn('binary_target "rust-mcp-profile-workload"', docker_free)
        runtime = M5.claude_prompt(M5.RUNTIME, M5.runtime_call_plan())
        self.assertIn('bench_target "perf"', runtime)
        self.assertIn("NOT_A_DATASET", runtime)
        self.assertIn("ReadMcpResourceTool", runtime)
        self.assertNotIn("qart_", runtime.replace("qart_ ", ""))


class HostConfigurationTests(unittest.TestCase):
    def test_docker_free_argv_is_closed_and_grants_nothing(self):
        state = pathlib.Path("/private/tmp/m5-unit-state")
        socket = pathlib.Path("/private/tmp/m5-unit-never-dialed.sock")
        argv = M5.server_argv(state, socket)
        self.assertEqual(argv[:3], [str(M5.SERVER), "serve", "--stdio"])
        self.assertEqual(argv.count("--root"), len(M5.FIXTURES))
        # No vendor tree and no profiling grant: their absence is the reason
        # three of the four tools answer before any container.
        self.assertNotIn("--cargo-vendor-dir", argv)
        self.assertNotIn("--allow-profiling", argv)
        self.assertIn("--state-root", argv)
        self.assertEqual(argv[argv.index("--docker-socket") + 1], str(socket))
        self.assertFalse(socket.exists())
        self.assertEqual(argv[argv.index("--rust-image") + 1], M5.m5_image())

    def test_runtime_argv_adds_exactly_the_grant_and_the_offline_tree(self):
        state = pathlib.Path("/private/tmp/m5-unit-state")
        socket = pathlib.Path("/private/tmp/m5-unit-real.sock")
        vendor = (M5.ROOT / M5.VENDOR_FIXTURE).resolve()
        fingerprint = "sha256:" + "f" * 64
        capture = pathlib.Path("/private/tmp/m5-unit-capture")
        capture_fingerprint = "sha256:" + "e" * 64
        argv = M5.runtime_server_argv(
            state, socket, vendor, fingerprint, capture, capture_fingerprint)
        base = M5.server_argv(state, socket)
        self.assertEqual(argv[:len(base)], base)
        self.assertEqual(argv[len(base):], [
            "--cargo-vendor-dir", str(vendor),
            "--cargo-vendor-tree-sha256", fingerprint,
            "--vendor-capture", str(capture),
            "--vendor-capture-tree-sha256", capture_fingerprint,
            "--allow-profiling", M5.PROFILING_GRANT,
        ])

    def test_capture_discovery_requires_one_regular_digest_named_artifact(self):
        with tempfile.TemporaryDirectory() as directory:
            store = pathlib.Path(directory)
            self.assertIsNone(M5.find_vendor_capture(store))
            artifact = store / ("a" * 64)
            artifact.write_bytes(b"capture")
            self.assertEqual(M5.find_vendor_capture(store),
                             (artifact.resolve(), "sha256:" + "a" * 64))
            (store / ("b" * 64)).symlink_to(artifact)
            self.assertEqual(M5.find_vendor_capture(store),
                             (artifact.resolve(), "sha256:" + "a" * 64))
            (store / ("b" * 64)).unlink()
            (store / ("b" * 64)).write_bytes(b"second")
            self.assertIsNone(M5.find_vendor_capture(store))

    def test_both_handlers_really_demand_the_offline_vendor_tree(self):
        # The runtime mode supplies one because the handlers require it, not
        # because the harness prefers it.
        for module in ("profile.rs", "bloat.rs"):
            source = (M5.STDIO_DIR / module).read_text()
            self.assertIn("let Some(vendor) = runtime.vendor.clone() else {", source)
            self.assertIn("Code::MissingOfflineData", source)

    def test_profile_demands_the_host_grant_before_anything_is_dispatched(self):
        source = (M5.STDIO_DIR / "profile.rs").read_text()
        self.assertIn("Code::ProfilingNotAuthorized", source)
        self.assertLess(source.index("Code::ProfilingNotAuthorized"),
                        source.index("let Some(vendor) = runtime.vendor.clone() else {"))

    def test_vendor_fixture_is_disjoint_from_every_project_root(self):
        vendor = (M5.ROOT / M5.VENDOR_FIXTURE).resolve()
        for path in M5.FIXTURES.values():
            root = (M5.ROOT / path).resolve()
            self.assertFalse(str(vendor).startswith(str(root) + "/"))
            self.assertFalse(str(root).startswith(str(vendor) + "/"))

    def test_measured_fixtures_name_no_registry_package(self):
        # Why an approved-but-unrelated vendor tree is sound here.
        for name in ("profile", "bloat"):
            lock = (M5.ROOT / M5.FIXTURES[name] / "Cargo.lock").read_text()
            self.assertNotIn("source = ", lock)
            self.assertNotIn("checksum = ", lock)

    def test_qualified_image_is_read_from_the_execution_adapter(self):
        image = M5.m5_image()
        self.assertRegex(image, r"^sha256:[0-9a-f]{64}$")
        self.assertIn(image, M5.PERFORMANCE_PORT.read_text())

    def test_fixture_roots_are_repository_relative_and_real(self):
        for name, path in M5.FIXTURES.items():
            self.assertFalse(path.startswith("/"), name)
            self.assertTrue((M5.ROOT / path / "Cargo.toml").is_file(), name)

    def test_session_plan_carries_no_expectation_the_harness_does_not_own(self):
        plan = M5.session_plan(M5.call_plan(), M5.DOCKER_FREE, 120_000)
        self.assertEqual(plan["expected_tools"], list(M5.EXPECTED_TOOLS))
        self.assertEqual(plan["unknown_project_ref"], "prj_" + "0" * 32)
        self.assertEqual(set(plan["projects"]), set(M5.FIXTURES))
        self.assertEqual(plan["mode"], M5.DOCKER_FREE)


class PreflightTests(unittest.TestCase):
    def test_direction_guard_is_separate_source_bound_evidence(self):
        evidence = M5.direction_guard_evidence()
        self.assertEqual(evidence["qualified"], False)
        self.assertEqual(evidence["runtime_guard"], "insufficient_executions")
        self.assertEqual(evidence["exercised_by_runtime_comparison"], False)
        self.assertEqual(evidence["unit_test"],
                         "only_the_frozen_constant_qualifies_a_comparison")

    def test_preflight_is_non_executing_client_free_and_source_bound(self):
        result = M5.preflight()
        self.assertFalse(result["execution_performed"])
        self.assertFalse(result["clients_started"])
        self.assertFalse(result["docker_required"])
        self.assertFalse(result["docker_used"])
        self.assertEqual(result["expected_tools"], list(M5.EXPECTED_TOOLS))
        self.assertEqual(result["clients"]["inspector"]["expected"], M5.INSPECTOR_VERSION)
        self.assertEqual(result["clients"]["claude_code"]["expected"], M5.CLAUDE_VERSION)
        self.assertEqual(result["clients"]["claude_code"]["model"], M5.CLAUDE_MODEL)
        self.assertNotIn("codex_app_server", result["clients"])
        self.assertEqual(set(result["source_sha256"]), {
            "scripts/test-m3-clients.py", "scripts/test-m5-clients.py",
            "scripts/m5-inspector-session.mjs", "scripts/test-m5-clients-unit.py",
        })
        self.assertTrue(all(len(value) == 64 for value in result["source_sha256"].values()))

    # The capture-backed benchmark and its real comparison cover all four.
    def test_preflight_names_the_tool_without_a_client_positive(self):
        result = M5.preflight()
        self.assertEqual(result["tools_without_a_client_positive"],
                         [])

    def test_runtime_preflight_adds_the_runtime_preconditions(self):
        plain = set(M5.preflight(False, None)["preconditions"])
        runtime = set(M5.preflight(True, None)["preconditions"])
        self.assertEqual(runtime - plain, {
            "docker_socket", "docker_binary", "vendor_fixture",
            "vendor_capture",
            "profiling_grant_supported", "qualified_image_admitted"})
        self.assertTrue(M5.preflight(True, None)["docker_required"])

    def test_an_absent_docker_socket_blocks_the_runtime_mode(self):
        result = M5.preflight(True, "/private/tmp/m5-unit-absent.sock")
        self.assertIn("docker_socket", result["unsatisfied"])
        self.assertEqual(result["status"], "blocked")

    def test_preflight_status_follows_the_unsatisfied_preconditions(self):
        result = M5.preflight()
        self.assertEqual(result["status"], "blocked" if result["unsatisfied"] else "ready")
        for name in result["unsatisfied"]:
            self.assertFalse(result["preconditions"][name]["satisfied"])
            self.assertTrue(result["preconditions"][name]["requirement"])

    def test_current_switches_are_recognized_without_overriding_them(self):
        state = M5.advertisement_state()
        self.assertEqual(tuple(state), M5.M5_TOOLS)
        self.assertTrue(all(isinstance(value, bool) for value in state.values()))

    def test_default_invocation_writes_nothing(self):
        before = M5.CURRENT.exists(), M5.PREFLIGHT.exists(), M5.ATTEMPTS.exists()
        with mock.patch("sys.argv", ["test-m5-clients.py"]), mock.patch("builtins.print"):
            code = M5.main()
        self.assertIn(code, {0, 1})
        self.assertEqual((M5.CURRENT.exists(), M5.PREFLIGHT.exists(), M5.ATTEMPTS.exists()), before)

    def test_with_runtime_alone_is_refused(self):
        with mock.patch("sys.argv", ["test-m5-clients.py", "--with-runtime"]):
            with self.assertRaisesRegex(RuntimeError, "requires --run"):
                M5.main()

    def test_run_is_closed_before_any_client_when_a_precondition_is_missing(self):
        blocked = dict(M5.preflight())
        blocked["unsatisfied"] = ["candidate_advertises_m5"]
        with mock.patch.object(M5, "preflight", return_value=blocked):
            with self.assertRaisesRegex(RuntimeError, "preconditions are unsatisfied"):
                M5.run(False, None)
            with self.assertRaisesRegex(RuntimeError, "preconditions are unsatisfied"):
                M5.run(True, "/private/tmp/m5-unit.sock")

    def test_stale_candidate_is_detected_without_starting_the_server(self):
        with tempfile.TemporaryDirectory() as directory:
            stale = pathlib.Path(directory) / "rust-engineering-mcp"
            stale.write_bytes(b"rust.miri\x00rust.deny\x00" * 64)
            with mock.patch.object(M5, "SERVER", stale):
                self.assertFalse(M5.candidate_advertises_m5())
            fresh = pathlib.Path(directory) / "fresh"
            fresh.write_bytes(b"\x00".join(tool.encode() for tool in M5.M5_TOOLS))
            with mock.patch.object(M5, "SERVER", fresh):
                self.assertTrue(M5.candidate_advertises_m5())

    def test_vendor_fingerprint_refuses_an_unapproved_tree(self):
        refused = mock.Mock(stdout=json.dumps({
            "status": "blocked", "error_code": "invalid_cargo_data",
            "tree_fingerprint": None}).encode())
        with mock.patch.object(M5.subprocess, "run", return_value=refused):
            with self.assertRaisesRegex(RuntimeError, "was not approved"):
                M5.vendor_fingerprint(pathlib.Path("/private/tmp/m5-unit-vendor"))
        approved = mock.Mock(stdout=json.dumps({
            "status": "passed", "tree_fingerprint": "sha256:" + "1" * 64}).encode())
        with mock.patch.object(M5.subprocess, "run", return_value=approved):
            self.assertEqual(M5.vendor_fingerprint(pathlib.Path("/private/tmp/m5-unit-vendor")),
                             "sha256:" + "1" * 64)


class EvidenceTests(unittest.TestCase):
    def test_metadata_validator_accepts_only_the_m3_proxy_shape(self):
        row = {"client": "inspector", "direction": "client", "session": "a" * 32,
               "bytes": 42, "sha256": "b" * 64, "method": "tools/call",
               "tool": "rust.benchmark.run"}
        initialize = {"client": "inspector", "direction": "client", "session": "a" * 32,
                      "bytes": 10, "sha256": "c" * 64, "method": "initialize",
                      "tasks_declared": True}
        response = {"client": "inspector", "direction": "server", "session": "a" * 32,
                    "bytes": 10, "sha256": "d" * 64, "tasks_advertised": True}
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / "protocol.jsonl"
            path.write_text("\n".join(json.dumps(item, separators=(",", ":"))
                                      for item in (initialize, response, row)) + "\n")
            self.assertTrue(M5.validate_protocol_metadata(path)["metadata_only"])

    def test_metadata_validator_rejects_payload_and_credentials(self):
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / "protocol.jsonl"
            path.write_text(json.dumps({"client": "x", "direction": "client", "session": "s",
                                        "bytes": 1, "sha256": "a" * 64,
                                        "arguments": {"authorization": "secret"}}) + "\n")
            with self.assertRaisesRegex(RuntimeError, "unapproved keys"):
                M5.validate_protocol_metadata(path)

    def _row(self, planned, **overrides):
        artifacts = planned.get("expect_min_artifacts", 0)
        row = {"client": "inspector", "tool": planned["tool"], "shape": planned["shape"],
               "mode": planned["mode"], "status": planned["expect_status"],
               "error_code": planned["expect_error_code"],
               "is_error": planned["expect_is_error"],
               "artifacts_published": artifacts, "artifacts_read": artifacts,
               "request_bytes": 120, "request_sha256": "a" * 64,
               "response_bytes": 240, "response_sha256": "b" * 64}
        row.update(overrides)
        return row

    def test_call_row_validator_accepts_both_planned_matrices(self):
        for plan in (M5.call_plan(), M5.runtime_call_plan()):
            rows = [self._row(planned) for planned in plan]
            self.assertEqual(len(M5.validate_call_rows(rows, "inspector", plan)), len(plan))

    def test_call_row_validator_rejects_extra_keys_and_free_text(self):
        plan = M5.call_plan()
        rows = [self._row(planned) for planned in plan]
        rows[0]["project_path"] = "/Users/somebody/secret/project"
        with self.assertRaisesRegex(RuntimeError, "approved set"):
            M5.validate_call_rows(rows, "inspector", plan)

    def test_call_row_validator_rejects_a_status_the_plan_did_not_ask_for(self):
        plan = M5.call_plan()
        rows = [self._row(planned) for planned in plan]
        rows[0]["status"] = "passed"
        with self.assertRaisesRegex(RuntimeError, "status is not the planned one"):
            M5.validate_call_rows(rows, "inspector", plan)

    def test_a_docker_free_row_may_not_claim_an_artifact(self):
        plan = M5.call_plan()
        rows = [self._row(planned) for planned in plan]
        rows[0]["artifacts_published"] = 1
        rows[0]["artifacts_read"] = 1
        with self.assertRaisesRegex(RuntimeError, "must publish no artifact"):
            M5.validate_call_rows(rows, "inspector", plan)

    def test_a_runtime_row_must_publish_and_read_its_evidence(self):
        plan = M5.runtime_call_plan()
        rows = [self._row(planned) for planned in plan]
        rows[0]["artifacts_published"] = 1
        rows[0]["artifacts_read"] = 1
        with self.assertRaisesRegex(RuntimeError, "published too few artifacts"):
            M5.validate_call_rows(rows, "inspector", plan)
        rows = [self._row(planned) for planned in plan]
        rows[0]["artifacts_read"] = 0
        with self.assertRaisesRegex(RuntimeError, "did not read every artifact"):
            M5.validate_call_rows(rows, "inspector", plan)

    def test_call_row_validator_rejects_a_wrong_mode(self):
        plan = M5.call_plan()
        rows = [self._row(planned) for planned in plan]
        rows[0]["mode"] = M5.RUNTIME
        with self.assertRaisesRegex(RuntimeError, "unexpected mode"):
            M5.validate_call_rows(rows, "inspector", plan)

    def test_call_row_validator_rejects_reordered_or_missing_rows(self):
        plan = M5.call_plan()
        rows = [self._row(planned) for planned in plan]
        with self.assertRaisesRegex(RuntimeError, "one row per planned call"):
            M5.validate_call_rows(rows[:-1], "inspector", plan)
        rows[0], rows[1] = rows[1], rows[0]
        with self.assertRaisesRegex(RuntimeError, "planned order"):
            M5.validate_call_rows(rows, "inspector", plan)

    def test_call_row_validator_rejects_a_foreign_client(self):
        plan = M5.call_plan()
        rows = [self._row(planned, client="claude-code") for planned in plan]
        with self.assertRaisesRegex(RuntimeError, "unexpected client"):
            M5.validate_call_rows(rows, "inspector", plan)

    def test_call_row_validator_rejects_invalid_digests_and_counts(self):
        plan = M5.call_plan()
        rows = [self._row(planned) for planned in plan]
        rows[0]["request_sha256"] = "not-a-digest"
        with self.assertRaisesRegex(RuntimeError, "digest is invalid"):
            M5.validate_call_rows(rows, "inspector", plan)
        rows[0]["request_sha256"] = "a" * 64
        rows[0]["response_bytes"] = 0
        with self.assertRaisesRegex(RuntimeError, "byte count is invalid"):
            M5.validate_call_rows(rows, "inspector", plan)


if __name__ == "__main__":
    unittest.main()
