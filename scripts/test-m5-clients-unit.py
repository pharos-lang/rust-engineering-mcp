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
    def test_runtime_plan_covers_profile_bloat_and_benchmark(self):
        self.assertEqual(
            M5.runtime_tools(),
            ("rust.profile.flamegraph", "rust.binary.bloat", "rust.benchmark.run"))
        for row in M5.runtime_call_plan():
            self.assertEqual(row["mode"], M5.RUNTIME)
            self.assertEqual(row["shape"], "positive")
            self.assertGreaterEqual(row["expect_min_artifacts"], 1)
            self.assertTrue(row["rationale"])

    # CORRECTED for ADR-080 §6: `rust.benchmark.run` is no longer among them.
    def test_the_tool_without_a_client_positive_is_named(self):
        missing = [tool for tool in M5.M5_TOOLS if tool not in M5.runtime_tools()]
        self.assertEqual(missing, ["rust.benchmark.compare"])

    def test_the_log_recovery_rows_publish_logs_and_no_measurement(self):
        """ADR-080 §6: an observed compilation failure and an unrecognized
        harness, each publishing harness logs and no dataset."""
        rows = [row for row in M5.runtime_call_plan()
                if row["tool"] == "rust.benchmark.run"]
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
        broken = ({**M5.RUNTIME_CALL_PLAN[0], "expect_min_artifacts": 0},)
        with mock.patch.object(M5, "RUNTIME_CALL_PLAN", broken):
            with self.assertRaisesRegex(RuntimeError, "must publish evidence"):
                M5.runtime_call_plan()
        blind = ({**M5.RUNTIME_CALL_PLAN[0], "expect_observation": {},
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
        argv = M5.runtime_server_argv(state, socket, vendor, fingerprint)
        base = M5.server_argv(state, socket)
        self.assertEqual(argv[:len(base)], base)
        self.assertEqual(argv[len(base):], [
            "--cargo-vendor-dir", str(vendor),
            "--cargo-vendor-tree-sha256", fingerprint,
            "--allow-profiling", M5.PROFILING_GRANT,
        ])

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
    def test_preflight_is_non_executing_client_free_and_source_bound(self):
        result = M5.preflight()
        self.assertFalse(result["execution_performed"])
        self.assertFalse(result["clients_started"])
        self.assertFalse(result["docker_required"])
        self.assertFalse(result["docker_used"])
        self.assertEqual(result["expected_tools"], list(M5.EXPECTED_TOOLS))
        self.assertEqual(result["clients"]["inspector"]["expected"], M5.INSPECTOR_VERSION)
        self.assertEqual(result["clients"]["codex_app_server"]["expected"], M5.CODEX_VERSION)
        self.assertEqual(set(result["source_sha256"]), {
            "scripts/test-m3-clients.py", "scripts/test-m5-clients.py",
            "scripts/m5-inspector-session.mjs", "scripts/test-m5-clients-unit.py",
            "docs/validation/m1-17-codex-client/controller.py",
        })
        self.assertTrue(all(len(value) == 64 for value in result["source_sha256"].values()))

    # CORRECTED for ADR-080 §6: `rust.benchmark.run` gained two client
    # positives, so only the comparison is left without one.
    def test_preflight_names_the_tool_without_a_client_positive(self):
        result = M5.preflight()
        self.assertEqual(result["tools_without_a_client_positive"],
                         ["rust.benchmark.compare"])

    def test_runtime_preflight_adds_the_runtime_preconditions(self):
        plain = set(M5.preflight(False, None)["preconditions"])
        runtime = set(M5.preflight(True, None)["preconditions"])
        self.assertEqual(runtime - plain, {
            "docker_socket", "docker_binary", "vendor_fixture",
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
        rows = [self._row(planned, client="codex-app-server") for planned in plan]
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
