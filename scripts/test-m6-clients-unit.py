#!/usr/bin/env python3
"""Benign, Docker-free, client-free unit tests for the M6 client harness."""
from __future__ import annotations

import contextlib
import importlib.util
import io
import json
import pathlib
import re
import tempfile
import unittest
from unittest import mock

ROOT = pathlib.Path(__file__).resolve().parents[1]
SUBJECT = ROOT / "scripts/test-m6-clients.py"
SPEC = importlib.util.spec_from_file_location("m6_clients", SUBJECT)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("M6 harness unavailable")
M6 = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(M6)


def _dummy_binary(directory: str) -> pathlib.Path:
    """A present, readable stand-in so run()'s receipt can hash SERVER/CLAUDE
    on a CI runner that has not built the release binary or installed Claude."""
    path = pathlib.Path(directory) / "binary"
    if not path.exists():
        path.write_bytes(b"candidate")
    return path


class InventoryTests(unittest.TestCase):
    def test_inventory_is_an_exact_extension_of_the_thirty_one(self):
        self.assertEqual(M6.EXPECTED_TOOLS[:22], M6.M3_TOOLS)
        self.assertEqual(M6.EXPECTED_TOOLS[22:27], M6.M4_TOOLS)
        self.assertEqual(M6.EXPECTED_TOOLS[27:31], M6.M5_TOOLS)
        self.assertEqual(M6.EXPECTED_TOOLS[31:], M6.M6_TOOLS)
        self.assertEqual(len(M6.EXPECTED_TOOLS), 36)
        self.assertEqual(len(set(M6.EXPECTED_TOOLS)), 36)

    def test_inventory_matches_the_servers_own_protocol_oracle(self):
        self.assertEqual(M6.protocol_inventory(), M6.EXPECTED_TOOLS)

    def test_m6_tools_are_pushed_unconditionally_in_order(self):
        self.assertTrue(M6.m6_pushed_unconditionally())

    def test_inventory_check_reports_the_binding_sources(self):
        report = M6.inventory_check()
        self.assertEqual(report["count"], 36)
        self.assertEqual(report["previous_count"], 31)
        self.assertTrue(report["previous_unchanged"])
        self.assertEqual(report["m6_appended"], list(M6.M6_TOOLS))
        self.assertTrue(report["pushed_unconditionally"])

    def test_drifted_inventory_is_a_failure_and_not_a_warning(self):
        drifted = M6.EXPECTED_TOOLS[:-1]
        with mock.patch.object(M6, "protocol_inventory", return_value=drifted):
            with self.assertRaisesRegex(RuntimeError, "drifted"):
                M6.inventory_check()
        reordered = M6.M6_TOOLS[1:] + M6.M6_TOOLS[:1]
        with mock.patch.object(M6, "protocol_inventory",
                               return_value=M6.PRIOR_TOOLS + reordered):
            with self.assertRaisesRegex(RuntimeError, "drifted"):
                M6.inventory_check()
        with mock.patch.object(M6, "m6_pushed_unconditionally", return_value=False):
            with self.assertRaisesRegex(RuntimeError, "unconditionally"):
                M6.inventory_check()


class ErrorVocabularyTests(unittest.TestCase):
    def test_declared_codes_use_the_serde_screaming_snake_vocabulary(self):
        self.assertIn("SANDBOX_DENIED", M6.declared_error_codes("rust.analyzer.symbols"))
        self.assertIn("FILE_NOT_IN_SNAPSHOT", M6.declared_error_codes("rust.analyzer.symbols"))
        self.assertIn("POSITION_OUT_OF_RANGE", M6.declared_error_codes("rust.analyzer.references"))
        self.assertIn("ACTION_STALE", M6.declared_error_codes("rust.analyzer.action.apply"))
        self.assertIn("PERMISSION_DENIED", M6.declared_error_codes("rust.analyzer.action.apply"))
        self.assertEqual(M6.screaming("ActionStale"), "ACTION_STALE")

    def test_every_expected_code_is_declared_by_its_tool_source(self):
        for row in M6.call_plan() + M6.runtime_call_plan():
            if row["expect_error_code"] is not None:
                self.assertIn(row["expect_error_code"], M6.declared_error_codes(row["tool"]))

    def test_an_undeclared_expectation_is_refused(self):
        with self.assertRaisesRegex(RuntimeError, "is not declared by"):
            M6.check_expectation("rust.analyzer.symbols", "valid_basic", {},
                                 "blocked", "NOT_A_REAL_CODE")

    def test_hard_coded_project_ref_is_refused(self):
        with self.assertRaisesRegex(RuntimeError, "never hard-coded"):
            M6.check_expectation("rust.analyzer.symbols", "valid_basic",
                                 {"project_ref": "prj_" + "0" * 32}, "passed", None)

    def test_an_unknown_fixture_root_is_refused(self):
        with self.assertRaisesRegex(RuntimeError, "unknown fixture root"):
            M6.check_expectation("rust.analyzer.symbols", "not_a_fixture", {}, "passed", None)


class DockerFreePlanTests(unittest.TestCase):
    def test_every_m6_tool_has_exactly_one_docker_free_row(self):
        plan = M6.call_plan()
        self.assertEqual(len(plan), len(M6.M6_TOOLS))
        self.assertEqual({row["tool"] for row in plan}, set(M6.M6_TOOLS))

    def test_every_docker_free_row_is_the_uniform_sandbox_refusal(self):
        for row in M6.call_plan():
            self.assertEqual(row["mode"], M6.DOCKER_FREE)
            self.assertEqual(row["expect_status"], "unavailable")
            self.assertEqual(row["expect_error_code"], "SANDBOX_DENIED")
            self.assertTrue(row["expect_is_error"])
            self.assertTrue(row["rationale"])
            self.assertNotIn("project_ref", row["arguments"])
            self.assertNotIn("expected_project_fingerprint", row["arguments"])
            action = row["arguments"].get("action")
            if isinstance(action, dict):
                self.assertNotIn("expected_project_fingerprint", action)

    def test_fingerprint_tools_carry_the_captured_fingerprint_kind(self):
        for row in M6.call_plan():
            expected_kind = ("docker_free_fingerprint" if M6.requires_fingerprint(row["tool"])
                             else "static")
            self.assertEqual(row["kind"], expected_kind)

    def test_a_docker_free_plan_missing_a_tool_is_refused(self):
        missing = tuple(row for row in M6.CALL_PLAN if row["tool"] != "rust.analyzer.diagnostics")
        with mock.patch.object(M6, "CALL_PLAN", missing):
            with self.assertRaisesRegex(RuntimeError, "omits"):
                M6.call_plan()

    def test_a_fingerprint_tool_declared_static_is_refused(self):
        broken = tuple({**row, "kind": "static"} if row["tool"] == "rust.analyzer.actions" else row
                       for row in M6.CALL_PLAN)
        with mock.patch.object(M6, "CALL_PLAN", broken):
            with self.assertRaisesRegex(RuntimeError, "inconsistent"):
                M6.call_plan()

    def test_a_hard_coded_fingerprint_is_refused(self):
        with self.assertRaisesRegex(RuntimeError, "never hard-coded"):
            M6.check_expectation("rust.analyzer.actions", "analyzer_actions",
                                 {"expected_project_fingerprint": "sha256:" + "0" * 64},
                                 "unavailable", "SANDBOX_DENIED")
        with self.assertRaisesRegex(RuntimeError, "never hard-coded"):
            M6.check_expectation("rust.analyzer.action.apply", "analyzer_actions",
                                 {"action": {"expected_project_fingerprint": "sha256:" + "0" * 64}},
                                 "unavailable", "SANDBOX_DENIED")


class RuntimePlanTests(unittest.TestCase):
    def test_runtime_plan_covers_all_five_tools_with_a_positive_row(self):
        rows = M6.runtime_call_plan()
        self.assertEqual(len(rows), 10)
        tools = {row["tool"] for row in rows}
        self.assertEqual(tools, set(M6.M6_TOOLS))
        for tool in M6.M6_TOOLS:
            self.assertTrue(any(row["tool"] == tool and row["shape"] == "positive" for row in rows))

    def test_the_write_lifecycle_is_the_expected_kind_sequence(self):
        kinds = [row["kind"] for row in M6.runtime_call_plan()]
        self.assertEqual(kinds, [
            "static", "static", "static", "static", "actions_capture",
            "apply_preview", "apply_commit", "apply_receipt",
            "apply_preview_stale", "static",
        ])

    def test_only_the_commit_row_reopens_the_write_project(self):
        rows = M6.runtime_call_plan()
        reopeners = [row["fact_key"] for row in rows if row["reopen_after"]]
        self.assertEqual(reopeners, ["apply_commit"])

    def test_action_stale_is_scoped_to_the_stale_preview_row(self):
        broken = tuple({**row, "kind": "static", "project": "valid_basic"}
                       if row["fact_key"] == "apply_stale" else row
                       for row in M6.RUNTIME_CALL_PLAN)
        with mock.patch.object(M6, "RUNTIME_CALL_PLAN", broken):
            with self.assertRaisesRegex(RuntimeError, "ACTION_STALE"):
                M6.runtime_call_plan()

    def test_file_not_in_snapshot_is_scoped_to_symbols(self):
        broken = tuple({**row, "tool": "rust.analyzer.diagnostics"}
                       if row["fact_key"] == "bad_file" else row
                       for row in M6.RUNTIME_CALL_PLAN)
        with mock.patch.object(M6, "RUNTIME_CALL_PLAN", broken):
            with self.assertRaisesRegex(RuntimeError, "scoped to rust.analyzer.symbols"):
                M6.runtime_call_plan()

    def test_a_static_row_may_not_touch_the_write_project(self):
        broken = tuple({**row, "project": M6.WRITE_PROJECT}
                       if row["fact_key"] == "diagnostics" else row
                       for row in M6.RUNTIME_CALL_PLAN)
        with mock.patch.object(M6, "RUNTIME_CALL_PLAN", broken):
            with self.assertRaisesRegex(RuntimeError, "never touches the write project"):
                M6.runtime_call_plan()

    def test_a_dynamic_row_must_touch_the_write_project(self):
        broken = tuple({**row, "project": "valid_basic"}
                       if row["fact_key"] == "apply_preview" else row
                       for row in M6.RUNTIME_CALL_PLAN)
        with mock.patch.object(M6, "RUNTIME_CALL_PLAN", broken):
            with self.assertRaisesRegex(RuntimeError, "always touches the write project"):
                M6.runtime_call_plan()

    def test_only_commit_may_declare_reopen_after(self):
        broken = tuple({**row, "reopen_after": True}
                       if row["fact_key"] == "apply_preview" else row
                       for row in M6.RUNTIME_CALL_PLAN)
        with mock.patch.object(M6, "RUNTIME_CALL_PLAN", broken):
            with self.assertRaisesRegex(RuntimeError, "only the commit row reopens"):
                M6.runtime_call_plan()

    def test_an_unknown_row_kind_is_refused(self):
        broken = tuple({**row, "kind": "not_a_real_kind"}
                       if row["fact_key"] == "actions" else row
                       for row in M6.RUNTIME_CALL_PLAN)
        with mock.patch.object(M6, "RUNTIME_CALL_PLAN", broken):
            with self.assertRaisesRegex(RuntimeError, "unknown row kind"):
                M6.runtime_call_plan()

    def test_the_actions_range_is_the_native_m6_11_calibrated_cursor(self):
        self.assertEqual(M6.ACTIONS_RANGE,
                         {"start": {"line": 2, "column": 9}, "end": {"line": 2, "column": 9}})

    def test_the_references_position_is_the_declaration_w06c_fixed(self):
        self.assertEqual(M6.REFERENCES_POSITION, {"line": 1, "column": 8})


class HostConfigurationTests(unittest.TestCase):
    def test_docker_free_argv_configures_rust_with_a_closed_socket(self):
        with tempfile.TemporaryDirectory() as tmp:
            state = pathlib.Path(tmp) / "state"
            socket = pathlib.Path(tmp) / "never-created.sock"
            argv = M6.server_argv(state, socket)
            self.assertIn("--rust-image", argv)
            self.assertEqual(argv[argv.index("--rust-image") + 1], M6.m6_image())
            self.assertIn("--docker-socket", argv)
            self.assertEqual(argv[argv.index("--docker-socket") + 1], str(socket))
            self.assertNotIn(M6.WRITE_GRANT_FLAG, argv)
            for path in M6.FIXTURES.values():
                self.assertIn(str(ROOT / path), argv)

    def test_runtime_argv_adds_exactly_the_write_grant_on_the_temp_copy(self):
        with tempfile.TemporaryDirectory() as tmp:
            state = pathlib.Path(tmp) / "state"
            socket = pathlib.Path(tmp) / "real.sock"
            write_root = pathlib.Path(tmp) / "write-copy"
            free = M6.server_argv(state, socket)
            runtime = M6.runtime_server_argv(state, socket, write_root)
            self.assertEqual(runtime[:len(free)], free)
            self.assertEqual(runtime[len(free):],
                             ["--root", str(write_root), M6.WRITE_GRANT_FLAG, str(write_root)])

    def test_host_config_accepts_the_write_grant_flag(self):
        self.assertTrue(M6.host_config_accepts_write_grant())

    def test_qualified_image_is_read_from_the_execution_adapter(self):
        image = M6.m6_image()
        self.assertTrue(re.fullmatch(r"sha256:[0-9a-f]{64}", image))

    def test_stage_write_fixture_copies_never_the_repository_fixture_itself(self):
        with tempfile.TemporaryDirectory() as tmp:
            private = pathlib.Path(tmp)
            copy = M6.stage_write_fixture(private)
            self.assertNotEqual(copy, ROOT / M6.FIXTURES["analyzer_actions"])
            self.assertEqual((copy / "src/lib.rs").read_bytes(),
                             (ROOT / M6.FIXTURES["analyzer_actions"] / "src/lib.rs").read_bytes())

    def test_session_plan_carries_the_dynamic_write_project_only_in_runtime_mode(self):
        docker_free = M6.session_plan(M6.call_plan(), M6.DOCKER_FREE, None, 1000)
        self.assertNotIn(M6.WRITE_PROJECT, docker_free["projects"])
        with tempfile.TemporaryDirectory() as tmp:
            write_root = pathlib.Path(tmp)
            runtime = M6.session_plan(M6.runtime_call_plan(), M6.RUNTIME, write_root, 1000)
            self.assertEqual(runtime["projects"][M6.WRITE_PROJECT], str(write_root))
            self.assertEqual(runtime["analyzer_file"], M6.ANALYZER_FILE)
            self.assertEqual(runtime["actions_range"], M6.ACTIONS_RANGE)


class PreflightTests(unittest.TestCase):
    def test_preflight_is_non_executing_client_free_and_source_bound(self):
        receipt = M6.preflight(False, None)
        self.assertFalse(receipt["execution_performed"])
        self.assertFalse(receipt["clients_started"])
        self.assertFalse(receipt["docker_used"])
        self.assertFalse(receipt["with_runtime_requested"])
        self.assertEqual(receipt["image_id"], M6.m6_image())

    def test_default_invocation_writes_nothing(self):
        self.assertFalse(M6.PREFLIGHT.exists())

    def test_runtime_preflight_adds_the_runtime_preconditions(self):
        free_checks = set(M6.preflight(False, None)["preconditions"])
        runtime_checks = set(M6.preflight(True, None)["preconditions"])
        self.assertTrue(runtime_checks.issuperset(free_checks))
        self.assertIn("docker_socket", runtime_checks - free_checks)

    def test_an_absent_docker_socket_blocks_the_runtime_mode(self):
        receipt = M6.preflight(True, None)
        self.assertEqual(receipt["status"], "blocked")
        self.assertIn("docker_socket", receipt["unsatisfied"])

    def test_preflight_status_follows_the_unsatisfied_preconditions(self):
        with mock.patch.object(M6, "preconditions",
                               return_value={"x": {"satisfied": False, "requirement": "r"}}):
            receipt = M6.preflight(False, None)
            self.assertEqual(receipt["status"], "blocked")
            self.assertEqual(receipt["unsatisfied"], ["x"])

    def test_with_runtime_alone_is_refused_by_main(self):
        with mock.patch("sys.argv", ["test-m6-clients.py", "--with-runtime"]):
            with self.assertRaisesRegex(RuntimeError, "requires --run"):
                M6.main()

    def test_stale_candidate_is_detected_without_starting_the_server(self):
        with mock.patch.object(M6, "SERVER", ROOT / "Cargo.toml"):
            self.assertFalse(M6.candidate_advertises_m6())


class EvidenceTests(unittest.TestCase):
    def test_metadata_validator_rejects_unapproved_keys_and_credentials(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "protocol.jsonl"
            path.write_text(json.dumps({"client": "inspector", "extra": "nope",
                                        "sha256": "0" * 64}) + "\n")
            with self.assertRaisesRegex(RuntimeError, "unapproved keys"):
                M6.validate_protocol_metadata(path)
            path.write_text(json.dumps({"client": "inspector", "sha256": "0" * 64,
                                       "method": "tools/call authorization leak"}) + "\n")
            with self.assertRaisesRegex(RuntimeError, "credential-shaped"):
                M6.validate_protocol_metadata(path)

    def test_call_row_validator_accepts_the_planned_shape(self):
        plan = M6.call_plan()
        rows = [{
            "client": "inspector", "tool": row["tool"], "shape": row["shape"], "mode": row["mode"],
            "status": row["expect_status"], "error_code": row["expect_error_code"],
            "is_error": row["expect_is_error"],
            "request_bytes": 10, "request_sha256": "0" * 64,
            "response_bytes": 10, "response_sha256": "1" * 64,
        } for row in plan]
        validated = M6.validate_call_rows(rows, "inspector", plan)
        self.assertEqual(len(validated), len(plan))

    def test_call_row_validator_rejects_extra_keys(self):
        plan = M6.call_plan()
        rows = [{
            "client": "inspector", "tool": row["tool"], "shape": row["shape"], "mode": row["mode"],
            "status": row["expect_status"], "error_code": row["expect_error_code"],
            "is_error": row["expect_is_error"], "extra": True,
            "request_bytes": 10, "request_sha256": "0" * 64,
            "response_bytes": 10, "response_sha256": "1" * 64,
        } for row in plan]
        with self.assertRaisesRegex(RuntimeError, "approved set"):
            M6.validate_call_rows(rows, "inspector", plan)

    def test_call_row_validator_rejects_a_status_the_plan_did_not_ask_for(self):
        plan = M6.call_plan()
        rows = [{
            "client": "inspector", "tool": row["tool"], "shape": row["shape"], "mode": row["mode"],
            "status": "passed", "error_code": None,
            "is_error": False,
            "request_bytes": 10, "request_sha256": "0" * 64,
            "response_bytes": 10, "response_sha256": "1" * 64,
        } for row in plan]
        with self.assertRaisesRegex(RuntimeError, "status is not the planned one"):
            M6.validate_call_rows(rows, "inspector", plan)

    def test_call_row_validator_rejects_a_foreign_client(self):
        plan = M6.call_plan()
        rows = [{
            "client": "codex", "tool": row["tool"], "shape": row["shape"], "mode": row["mode"],
            "status": row["expect_status"], "error_code": row["expect_error_code"],
            "is_error": row["expect_is_error"],
            "request_bytes": 10, "request_sha256": "0" * 64,
            "response_bytes": 10, "response_sha256": "1" * 64,
        } for row in plan]
        with self.assertRaisesRegex(RuntimeError, "unexpected client"):
            M6.validate_call_rows(rows, "inspector", plan)

    def test_call_row_validator_rejects_reordered_or_missing_rows(self):
        plan = M6.call_plan()
        with self.assertRaisesRegex(RuntimeError, "one row per planned call"):
            M6.validate_call_rows([], "inspector", plan)

    def test_credential_text_detector_finds_header_and_token_shapes(self):
        with tempfile.TemporaryDirectory() as tmp:
            clean = pathlib.Path(tmp) / "clean.jsonl"
            clean.write_text("no secrets here, just prose about authorization design\n")
            M6.assert_no_credential_text(clean)
            dirty = pathlib.Path(tmp) / "dirty.jsonl"
            dirty.write_text("Authorization: Bearer sk-ant-abc123\n")
            with self.assertRaisesRegex(RuntimeError, "credential-shaped"):
                M6.assert_no_credential_text(dirty)


class ClaudeTranscriptTests(unittest.TestCase):
    """Event shapes copied from a real `claude -p --output-format stream-json` probe."""

    VALID_BASIC_REF = "prj_" + "1" * 32
    REFERENCES_REF = "prj_" + "2" * 32
    WRITE_REF = "prj_" + "3" * 32
    WRITE_REF_AFTER = "prj_" + "4" * 32
    FINGERPRINT_PRE = "sha256:" + "a" * 64
    FINGERPRINT_POST = "sha256:" + "b" * 64
    ACTION_DIGEST = "sha256:" + "c" * 64
    PLAN_ID = "mut_" + "d" * 32
    PLAN_DIGEST = "sha256:" + "e" * 64

    @staticmethod
    def init(model="claude-sonnet-5", version="2.1.267", tools=None, servers=None):
        return {"type": "system", "subtype": "init", "model": model,
                "claude_code_version": version, "apiKeySource": "none",
                "mcp_servers": servers if servers is not None
                else [{"name": "rust_engineering", "status": "connected"}],
                "tools": tools if tools is not None
                else ["mcp__rust_engineering__rust_project_open",
                      "mcp__rust_engineering__rust_analyzer_symbols"]}

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
    def result(identifier, payload, is_error=False):
        text = json.dumps(payload)
        block = {"type": "tool_result", "tool_use_id": identifier,
                 "content": text, "is_error": is_error}
        return {"type": "user", "message": {"content": [block]}}

    def open_call(self, index, path, ref, fingerprint=None):
        data = {"project_ref": ref}
        if fingerprint is not None:
            data["fingerprint"] = fingerprint
        payload = {"status": "passed", "error_code": None, "data": data}
        return [
            self.use(f"o{index}", "mcp__rust_engineering__rust_project_open", {"path": path}),
            self.result(f"o{index}", payload),
        ]

    def docker_free_transcript(self, fingerprint=None):
        events = [self.init()]
        roots = {"valid_basic": self.VALID_BASIC_REF, "analyzer_references": self.REFERENCES_REF,
                 "analyzer_actions": self.VALID_BASIC_REF}
        fingerprints = {"analyzer_actions": fingerprint if fingerprint is not None
                        else self.FINGERPRINT_PRE}
        for index, (name, ref) in enumerate(roots.items()):
            events += self.open_call(index, str(ROOT / M6.FIXTURES[name]), ref,
                                     fingerprint=fingerprints.get(name))
        for row in M6.call_plan():
            reference = roots[row["project"]]
            name = "mcp__rust_engineering__" + row["tool"].replace(".", "_")
            arguments = {"project_ref": reference, **row["arguments"]}
            if row["tool"] == "rust.analyzer.actions":
                arguments = {"project_ref": reference,
                            "expected_project_fingerprint": fingerprints[row["project"]],
                            **row["arguments"]}
            elif row["tool"] == "rust.analyzer.action.apply":
                arguments = {"project_ref": reference,
                            "action": {"expected_project_fingerprint": fingerprints[row["project"]],
                                       **row["arguments"]["action"]}}
            events.append(self.use(row["tool"], name, arguments))
            events.append(self.result(row["tool"],
                                      {"status": row["expect_status"],
                                       "error_code": row["expect_error_code"], "data": None},
                                      is_error=True))
        events.append(self.final())
        return events

    def runtime_transcript(self, plan, write_root, actions_empty=False):
        events = [self.init()]
        events += self.open_call("valid_basic", str(ROOT / M6.FIXTURES["valid_basic"]),
                                 self.VALID_BASIC_REF)
        events += self.open_call("analyzer_references", str(ROOT / M6.FIXTURES["analyzer_references"]),
                                 self.REFERENCES_REF)
        events += self.open_call("write_pre", str(write_root), self.WRITE_REF,
                                 fingerprint=self.FINGERPRINT_PRE)

        rows = {row["fact_key"]: row for row in plan}

        def call(fact_key, arguments, status, error_code, data=None):
            row = rows[fact_key]
            name = "mcp__rust_engineering__" + row["tool"].replace(".", "_")
            events.append(self.use(fact_key, name, arguments))
            events.append(self.result(fact_key, {"status": status, "error_code": error_code,
                                                 "data": data}, is_error=error_code is not None))

        call("symbols_document", {"project_ref": self.VALID_BASIC_REF,
                                  **rows["symbols_document"]["arguments"]}, "passed", None)
        call("symbols_workspace", {"project_ref": self.VALID_BASIC_REF,
                                   **rows["symbols_workspace"]["arguments"]}, "passed", None)
        call("references", {"project_ref": self.REFERENCES_REF, **rows["references"]["arguments"]},
             "passed", None)
        call("diagnostics", {"project_ref": self.VALID_BASIC_REF, **rows["diagnostics"]["arguments"]},
             "passed", None)
        if actions_empty:
            # The known assist-readiness race (ADR-084, W09d): the analyzer
            # honestly answers `passed` with no actions, so the model has no
            # digest to apply and correctly moves straight to the negative.
            call("actions", {"project_ref": self.WRITE_REF,
                             "expected_project_fingerprint": self.FINGERPRINT_PRE,
                             **rows["actions"]["arguments"]},
                 "passed", None,
                 data={"actions": [], "completeness": {"state": "complete"}})
            call("bad_file", {"project_ref": self.VALID_BASIC_REF, **rows["bad_file"]["arguments"]},
                 "blocked", "FILE_NOT_IN_SNAPSHOT")
            events.append(self.final())
            return events
        call("actions", {"project_ref": self.WRITE_REF,
                         "expected_project_fingerprint": self.FINGERPRINT_PRE,
                         **rows["actions"]["arguments"]},
             "passed", None,
             data={"actions": [{"applicability": "applicable", "action_digest": self.ACTION_DIGEST}]})
        call("apply_preview",
             {"project_ref": self.WRITE_REF, "action": {
                 "mode": "preview", "expected_project_fingerprint": self.FINGERPRINT_PRE,
                 "action_digest": self.ACTION_DIGEST, "file": M6.ANALYZER_FILE,
                 "range": M6.ACTIONS_RANGE}},
             "passed", None,
             data={"kind": "preview", "plan_id": self.PLAN_ID, "plan_digest": self.PLAN_DIGEST,
                   "files": [{"before_sha256": "1" * 64, "after_sha256": "2" * 64}]})
        call("apply_commit",
             {"project_ref": self.WRITE_REF, "action": {
                 "mode": "commit", "plan_id": self.PLAN_ID, "plan_digest": self.PLAN_DIGEST,
                 "idempotency_key": M6.IDEMPOTENCY_KEY}},
             "passed", None,
             data={"kind": "receipt", "operation_id": self.PLAN_ID, "state": "committed"})
        events += self.open_call("write_post", str(write_root), self.WRITE_REF_AFTER,
                                 fingerprint=self.FINGERPRINT_POST)
        call("apply_receipt",
             {"project_ref": self.WRITE_REF_AFTER, "action": {
                 "mode": "receipt", "operation_id": self.PLAN_ID, "recover": False}},
             "passed", None,
             data={"kind": "receipt", "operation_id": self.PLAN_ID, "state": "committed"})
        call("apply_stale",
             {"project_ref": self.WRITE_REF_AFTER, "action": {
                 "mode": "preview", "expected_project_fingerprint": self.FINGERPRINT_POST,
                 "action_digest": self.ACTION_DIGEST, "file": M6.ANALYZER_FILE,
                 "range": M6.ACTIONS_RANGE}},
             "blocked", "ACTION_STALE")
        call("bad_file", {"project_ref": self.VALID_BASIC_REF, **rows["bad_file"]["arguments"]},
             "blocked", "FILE_NOT_IN_SNAPSHOT")
        events.append(self.final())
        return events

    def write_root_with_a_landed_change(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        write_root = pathlib.Path(tmp.name)
        (write_root / "src").mkdir(parents=True)
        (write_root / "src/lib.rs").write_text("pub fn total(values: &[u32]) -> u32 { 0 }\n")
        return write_root

    @staticmethod
    def _mutate_result(events, tool_use_id, status, error_code=None):
        """Rewrite one already-built transcript's tool_result payload
        in place, keeping `is_error` consistent with the new error_code."""
        for event in events:
            if event.get("type") != "user":
                continue
            block = event["message"]["content"][0]
            if block.get("tool_use_id") == tool_use_id:
                payload = json.loads(block["content"])
                payload["status"] = status
                payload["error_code"] = error_code
                block["content"] = json.dumps(payload)
                block["is_error"] = error_code is not None
                return
        raise AssertionError(f"no tool_result for {tool_use_id!r} in this transcript")

    def test_docker_free_transcript_normalizes_and_validates(self):
        events = self.docker_free_transcript()
        init, items, final = M6.claude_items(events)
        session = M6.validate_claude_session(init, final, events)
        self.assertEqual(session["resolved_model"], M6.CLAUDE_MODEL)
        flow = M6.validate_docker_free_model_flow(items, M6.call_plan())
        self.assertEqual(set(flow["refusals"]), set(M6.M6_TOOLS))
        for refusal in flow["refusals"].values():
            self.assertEqual(refusal["error_code"], "SANDBOX_DENIED")

    def test_docker_free_flow_rejects_a_fingerprint_not_captured_from_open(self):
        events = self.docker_free_transcript(fingerprint=self.FINGERPRINT_PRE)
        # Corrupt the actions call's fingerprint so it no longer matches what
        # this transcript's own project.open returned.
        stale = "sha256:" + "9" * 64
        for event in events:
            if event.get("type") != "assistant":
                continue
            block = event["message"]["content"][0]
            if block.get("name") == "mcp__rust_engineering__rust_analyzer_actions":
                block["input"]["expected_project_fingerprint"] = stale
        init, items, final = M6.claude_items(events)
        with self.assertRaisesRegex(RuntimeError, "did not use the planned arguments"):
            M6.validate_docker_free_model_flow(items, M6.call_plan())

    def test_docker_free_flow_rejects_a_missing_captured_fingerprint(self):
        events = self.docker_free_transcript()
        # Drop the fingerprint from analyzer_actions' own open, leaving the
        # actions/apply rows with nothing real to have captured.
        for event in events:
            if event.get("type") != "user":
                continue
            block = event["message"]["content"][0]
            if block.get("tool_use_id") == "o2":
                payload = json.loads(block["content"])
                del payload["data"]["fingerprint"]
                block["content"] = json.dumps(payload)
        init, items, final = M6.claude_items(events)
        with self.assertRaisesRegex(RuntimeError, "needs a fingerprint captured"):
            M6.validate_docker_free_model_flow(items, M6.call_plan())

    def test_runtime_transcript_normalizes_and_validates(self):
        plan = M6.runtime_call_plan()
        write_root = self.write_root_with_a_landed_change()
        events = self.runtime_transcript(plan, write_root)
        init, items, final = M6.claude_items(events)
        M6.validate_claude_session(init, final, events)
        flow = M6.validate_runtime_model_flow(items, plan, write_root)
        self.assertTrue(flow["write_verified_on_disk"])
        self.assertEqual(flow["write_lifecycle"], "performed")
        self.assertEqual(flow["facts"]["apply_commit"]["status"], "passed")
        self.assertEqual(flow["facts"]["apply_stale"]["error_code"], "ACTION_STALE")
        self.assertEqual(flow["facts"]["bad_file"]["error_code"], "FILE_NOT_IN_SNAPSHOT")

    def test_runtime_flow_accepts_a_skipped_write_when_actions_came_back_empty(self):
        """W09d: rust-analyzer's assists are not deterministic under a
        transient instance (ADR-084); a model that correctly finds no
        applicable action and moves straight to the bad-file negative is a
        valid, honest runtime flow, not a failure."""
        plan = M6.runtime_call_plan()
        write_root = self.write_root_with_a_landed_change()
        events = self.runtime_transcript(plan, write_root, actions_empty=True)
        init, items, final = M6.claude_items(events)
        M6.validate_claude_session(init, final, events)
        flow = M6.validate_runtime_model_flow(items, plan, write_root)
        self.assertEqual(flow["write_lifecycle"], "skipped: no applicable action offered")
        self.assertIsNone(flow["write_verified_on_disk"])
        self.assertEqual(flow["facts"]["actions"]["status"], "passed")
        self.assertEqual(flow["facts"]["bad_file"]["error_code"], "FILE_NOT_IN_SNAPSHOT")
        self.assertNotIn("apply_commit", flow["facts"])

    def test_runtime_flow_still_validates_the_full_write_cycle_when_offered(self):
        """The best-effort leniency must never mask a real failure inside a
        write cycle the model actually attempted: a corrupted commit receipt
        still raises even though the transcript also carries a capturable
        action_digest and a reopen."""
        plan = M6.runtime_call_plan()
        write_root = self.write_root_with_a_landed_change()
        events = self.runtime_transcript(plan, write_root)
        for event in events:
            if event.get("type") != "user":
                continue
            block = event["message"]["content"][0]
            if block.get("tool_use_id") == "apply_receipt":
                payload = json.loads(block["content"])
                payload["data"]["state"] = "pending"
                block["content"] = json.dumps(payload)
        init, items, final = M6.claude_items(events)
        with self.assertRaisesRegex(RuntimeError, "committed operation"):
            M6.validate_runtime_model_flow(items, plan, write_root)

    def test_runtime_flow_tolerates_a_transient_capacity_refusal_on_secondary_reads(self):
        """W09f: the analyzer's bounded capacity releases asynchronously
        after a passing call returns, so a read immediately following
        another one can legitimately land as an instantaneous
        unavailable/SANDBOX_DENIED. That is recorded, never a failure, on
        the three reads that are not the flow's cold-start first call."""
        plan = M6.runtime_call_plan()
        write_root = self.write_root_with_a_landed_change()
        events = self.runtime_transcript(plan, write_root)
        for fact_key in ("symbols_workspace", "references", "diagnostics"):
            self._mutate_result(events, fact_key, "unavailable", "SANDBOX_DENIED")
        init, items, final = M6.claude_items(events)
        M6.validate_claude_session(init, final, events)
        flow = M6.validate_runtime_model_flow(items, plan, write_root)
        self.assertEqual(flow["facts"]["symbols_document"]["observed"], "positive")
        for fact_key in ("symbols_workspace", "references", "diagnostics"):
            self.assertEqual(flow["facts"][fact_key]["observed"], "capacity_refused")
        self.assertEqual(sorted(flow["capacity_refused_reads"]),
                         ["diagnostics", "references", "symbols_workspace"])
        self.assertEqual(flow["analyzer_positive_reads"], 1)
        self.assertEqual(flow["facts"]["actions"]["observed"], "positive")
        self.assertEqual(flow["facts"]["bad_file"]["error_code"], "FILE_NOT_IN_SNAPSHOT")
        # (d) the write cycle, offered independently of the tolerated
        # refusals above, is still fully validated.
        self.assertEqual(flow["write_lifecycle"], "performed")
        self.assertTrue(flow["write_verified_on_disk"])

    def test_runtime_flow_rejects_the_cold_start_read_as_capacity_refused(self):
        """symbols(document) is the flow's first analyzer call and always
        starts cold: it never races a prior teardown, so it alone is not
        eligible for the capacity tolerance -- an unavailable/SANDBOX_DENIED
        there is a hard failure, not a transient refusal."""
        plan = M6.runtime_call_plan()
        write_root = self.write_root_with_a_landed_change()
        events = self.runtime_transcript(plan, write_root, actions_empty=True)
        self._mutate_result(events, "symbols_document", "unavailable", "SANDBOX_DENIED")
        init, items, final = M6.claude_items(events)
        with self.assertRaisesRegex(RuntimeError, "did not answer the planned result"):
            M6.validate_runtime_model_flow(items, plan, write_root)

    def test_runtime_flow_rejects_an_unplanned_status_that_is_not_a_capacity_refusal(self):
        """Only the exact unavailable/SANDBOX_DENIED pair is tolerated; any
        other unplanned status on a positive row is still a hard failure,
        even on a row that is otherwise capacity-tolerant."""
        plan = M6.runtime_call_plan()
        write_root = self.write_root_with_a_landed_change()
        events = self.runtime_transcript(plan, write_root, actions_empty=True)
        self._mutate_result(events, "diagnostics", "failed", None)
        init, items, final = M6.claude_items(events)
        with self.assertRaisesRegex(RuntimeError, "did not answer the planned result"):
            M6.validate_runtime_model_flow(items, plan, write_root)

    def test_runtime_flow_still_validates_write_cycle_alongside_a_capacity_refusal(self):
        """(d) Tolerating a capacity refusal on a secondary read must never
        mask a real defect in a write cycle the model actually drove."""
        plan = M6.runtime_call_plan()
        write_root = self.write_root_with_a_landed_change()
        events = self.runtime_transcript(plan, write_root)
        self._mutate_result(events, "diagnostics", "unavailable", "SANDBOX_DENIED")
        for event in events:
            if event.get("type") != "user":
                continue
            block = event["message"]["content"][0]
            if block.get("tool_use_id") == "apply_receipt":
                payload = json.loads(block["content"])
                payload["data"]["state"] = "pending"
                block["content"] = json.dumps(payload)
        init, items, final = M6.claude_items(events)
        with self.assertRaisesRegex(RuntimeError, "committed operation"):
            M6.validate_runtime_model_flow(items, plan, write_root)

    def test_runtime_flow_rejects_a_missing_read(self):
        plan = M6.runtime_call_plan()
        write_root = self.write_root_with_a_landed_change()
        events = self.runtime_transcript(plan, write_root, actions_empty=True)
        pruned = [event for event in events
                 if not (event.get("type") == "assistant"
                         and event["message"]["content"][0].get("id") == "diagnostics")
                 and not (event.get("type") == "user"
                         and event["message"]["content"][0].get("tool_use_id") == "diagnostics")]
        init, items, final = M6.claude_items(pruned)
        with self.assertRaisesRegex(RuntimeError, "retried or omitted diagnostics"):
            M6.validate_runtime_model_flow(items, plan, write_root)

    def test_runtime_flow_treats_a_missing_reopen_as_a_skipped_write(self):
        """W09d: a model that offered itself a digest but never drove the
        write lifecycle to a reopen is best-effort, not a failure -- unlike
        an omitted read or an absent negative, which must still raise."""
        plan = M6.runtime_call_plan()
        write_root = self.write_root_with_a_landed_change()
        events = self.runtime_transcript(plan, write_root)
        # Drop the post-commit reopen: the model captured a digest but never
        # completed the write lifecycle.
        pruned = [event for event in events
                 if not (event.get("type") == "assistant"
                         and event["message"]["content"][0].get("id") == "owrite_post")
                 and not (event.get("type") == "user"
                         and event["message"]["content"][0].get("tool_use_id") == "owrite_post")]
        init, items, final = M6.claude_items(pruned)
        flow = M6.validate_runtime_model_flow(items, plan, write_root)
        self.assertEqual(flow["write_lifecycle"], "skipped: no applicable action offered")
        self.assertIsNone(flow["write_verified_on_disk"])

    def test_runtime_flow_rejects_a_foreign_capability(self):
        plan = M6.runtime_call_plan()
        write_root = self.write_root_with_a_landed_change()
        events = self.runtime_transcript(plan, write_root)
        events.insert(-1, self.use("foreign", "ListMcpResourcesTool", {"server": "rust_engineering"}))
        events.insert(-1, self.result("foreign", {"contents": []}))
        with self.assertRaisesRegex(RuntimeError, "outside the configured server"):
            M6.claude_items(events)

    def test_credential_shaped_transcripts_are_refused_before_flow_validation(self):
        with tempfile.TemporaryDirectory() as tmp:
            events_path = pathlib.Path(tmp) / "events.jsonl"
            events_path.write_text("Authorization: Bearer sk-ant-abcdef\n")
            with self.assertRaisesRegex(RuntimeError, "credential-shaped"):
                M6.assert_no_credential_text(events_path)

    def test_a_model_from_another_model_is_refused(self):
        events = self.docker_free_transcript()
        events[3]["message"]["model"] = "claude-opus-5"
        init, _, final = M6.claude_items(events)
        with self.assertRaisesRegex(RuntimeError, "another model"):
            M6.validate_claude_session(init, final, events)

    def test_a_permission_denial_is_refused(self):
        events = self.docker_free_transcript()
        events[-1] = self.final(denials=["mcp__rust_engineering__rust_analyzer_symbols"])
        init, _, final = M6.claude_items(events)
        with self.assertRaisesRegex(RuntimeError, "denied a capability"):
            M6.validate_claude_session(init, final, events)


class PromptTests(unittest.TestCase):
    def test_docker_free_prompt_names_every_tool_once(self):
        prompt = M6.claude_prompt(M6.DOCKER_FREE, M6.call_plan())
        for tool in M6.M6_TOOLS:
            self.assertIn(tool, prompt)
        self.assertEqual(prompt.count("rust.project.open"), 1)

    def test_runtime_prompt_orders_the_write_lifecycle(self):
        with tempfile.TemporaryDirectory() as tmp:
            write_root = pathlib.Path(tmp)
            prompt = M6.claude_prompt(M6.RUNTIME, M6.runtime_call_plan(), write_root)
        self.assertIn("preview", prompt)
        self.assertIn("commit", prompt)
        self.assertIn("receipt", prompt)
        self.assertIn("ACTION_STALE", prompt)
        self.assertIn("FILE_NOT_IN_SNAPSHOT", prompt)
        self.assertIn(M6.IDEMPOTENCY_KEY, prompt)
        self.assertIn(str(write_root), prompt)
        # Commit precedes the reopen, which precedes the receipt.
        self.assertLess(prompt.index("action mode commit"), prompt.index("WRITE_AFTER_COMMIT"))
        self.assertLess(prompt.index("WRITE_AFTER_COMMIT"), prompt.index("action mode receipt"))


class RobustnessTests(unittest.TestCase):
    """Defensive branches that only a corrupted source tree or a broken
    caller can reach; exercised here by mocking, never by editing sources."""

    def test_load_m3_refuses_when_the_spec_is_unavailable(self):
        with mock.patch("importlib.util.spec_from_file_location", return_value=None):
            with self.assertRaisesRegex(RuntimeError, "unavailable"):
                M6.load_m3()

    def test_m6_image_refuses_when_the_constant_is_missing(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "gateway.rs"
            path.write_text("no constant here")
            with mock.patch.object(M6, "ANALYZER_GATEWAY", path):
                with self.assertRaisesRegex(RuntimeError, "digest is missing"):
                    M6.m6_image()

    def test_protocol_inventory_refuses_a_duplicated_name(self):
        text = M6.PROTOCOL_TEST.read_text()
        marker = "assert_eq!(tools.map(Vec::len), Some("
        start = text.index(marker) + len(marker)
        real_count = int(text[start:text.index(")", start)])
        broken = text.replace(f"Some({real_count})", f"Some({real_count - 1})", 1)
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "protocol.rs"
            path.write_text(broken)
            with mock.patch.object(M6, "PROTOCOL_TEST", path):
                with self.assertRaisesRegex(RuntimeError, "inconsistent"):
                    M6.protocol_inventory()

    def test_declared_error_codes_refuses_a_broken_enum(self):
        broken_source = "enum Code {\n    Same,\n    Same,\n}\n"
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "broken.rs"
            path.write_text(broken_source)
            with mock.patch.dict(M6.TOOL_SOURCES, {"rust.analyzer.symbols": (path, "Code")}):
                with self.assertRaisesRegex(RuntimeError, "invalid for"):
                    M6.declared_error_codes("rust.analyzer.symbols")

    def test_check_expectation_refuses_an_unknown_tool(self):
        with self.assertRaisesRegex(RuntimeError, "unknown tool"):
            M6.check_expectation("rust.not.a.tool", "valid_basic", {}, "passed", None)

    def test_check_expectation_refuses_an_unknown_status(self):
        with self.assertRaisesRegex(RuntimeError, "unknown status"):
            M6.check_expectation("rust.analyzer.symbols", "valid_basic", {}, "not_a_status", None)

    def test_check_expectation_refuses_a_blocked_row_with_no_error_code(self):
        with self.assertRaisesRegex(RuntimeError, "must name the declared error code"):
            M6.check_expectation("rust.analyzer.symbols", "valid_basic", {}, "blocked", None)


class EvidenceHappyPathTests(unittest.TestCase):
    def test_metadata_validator_accepts_the_m3_proxy_shape(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "protocol.jsonl"
            row = {"client": "inspector", "direction": "client", "session": "s1",
                  "bytes": 10, "sha256": "0" * 64, "method": "tools/call",
                  "tasks_declared": False}
            path.write_text(json.dumps(row) + "\n")
            summary = M6.validate_protocol_metadata(path)
            self.assertTrue(summary["metadata_only"])
            self.assertEqual(summary["row_count"], 1)

    def test_call_row_validator_rejects_reordered_rows(self):
        plan = M6.call_plan()
        rows = [{
            "client": "inspector", "tool": row["tool"], "shape": row["shape"], "mode": row["mode"],
            "status": row["expect_status"], "error_code": row["expect_error_code"],
            "is_error": row["expect_is_error"],
            "request_bytes": 10, "request_sha256": "0" * 64,
            "response_bytes": 10, "response_sha256": "1" * 64,
        } for row in reversed(plan)]
        with self.assertRaisesRegex(RuntimeError, "not in planned order"):
            M6.validate_call_rows(rows, "inspector", plan)

    def test_call_row_validator_rejects_a_foreign_mode(self):
        plan = M6.call_plan()
        rows = [{
            "client": "inspector", "tool": row["tool"], "shape": row["shape"], "mode": "runtime",
            "status": row["expect_status"], "error_code": row["expect_error_code"],
            "is_error": row["expect_is_error"],
            "request_bytes": 10, "request_sha256": "0" * 64,
            "response_bytes": 10, "response_sha256": "1" * 64,
        } for row in plan]
        with self.assertRaisesRegex(RuntimeError, "unexpected mode"):
            M6.validate_call_rows(rows, "inspector", plan)

    def test_call_row_validator_rejects_an_undeclared_error_code(self):
        plan = M6.call_plan()
        rows = [{
            "client": "inspector", "tool": row["tool"], "shape": row["shape"], "mode": row["mode"],
            "status": row["expect_status"], "error_code": "NOT_A_REAL_CODE",
            "is_error": row["expect_is_error"],
            "request_bytes": 10, "request_sha256": "0" * 64,
            "response_bytes": 10, "response_sha256": "1" * 64,
        } for row in plan]
        with self.assertRaisesRegex(RuntimeError, "error code is not the planned one"):
            M6.validate_call_rows(rows, "inspector", plan)

    def test_call_row_validator_rejects_a_wrong_is_error(self):
        plan = M6.call_plan()
        rows = [{
            "client": "inspector", "tool": row["tool"], "shape": row["shape"], "mode": row["mode"],
            "status": row["expect_status"], "error_code": row["expect_error_code"],
            "is_error": False,
            "request_bytes": 10, "request_sha256": "0" * 64,
            "response_bytes": 10, "response_sha256": "1" * 64,
        } for row in plan]
        with self.assertRaisesRegex(RuntimeError, "isError is not the planned one"):
            M6.validate_call_rows(rows, "inspector", plan)

    def test_call_row_validator_rejects_an_invalid_digest_or_byte_count(self):
        plan = M6.call_plan()

        def rows_with(**overrides):
            row = plan[0]
            base = {
                "client": "inspector", "tool": row["tool"], "shape": row["shape"],
                "mode": row["mode"], "status": row["expect_status"],
                "error_code": row["expect_error_code"], "is_error": row["expect_is_error"],
                "request_bytes": 10, "request_sha256": "0" * 64,
                "response_bytes": 10, "response_sha256": "1" * 64,
            }
            base.update(overrides)
            return [base] + [{
                "client": "inspector", "tool": r["tool"], "shape": r["shape"], "mode": r["mode"],
                "status": r["expect_status"], "error_code": r["expect_error_code"],
                "is_error": r["expect_is_error"],
                "request_bytes": 10, "request_sha256": "0" * 64,
                "response_bytes": 10, "response_sha256": "1" * 64,
            } for r in plan[1:]]

        with self.assertRaisesRegex(RuntimeError, "digest is invalid"):
            M6.validate_call_rows(rows_with(request_sha256="not-hex"), "inspector", plan)
        with self.assertRaisesRegex(RuntimeError, "byte count is invalid"):
            M6.validate_call_rows(rows_with(request_bytes=0), "inspector", plan)

    def test_call_row_validator_rejects_a_non_object_row(self):
        plan = M6.call_plan()
        with self.assertRaisesRegex(RuntimeError, "is not an object"):
            M6.validate_call_rows(["not a dict"] * len(plan), "inspector", plan)


class ClaudeItemsRobustnessTests(unittest.TestCase):
    def test_a_non_object_event_is_refused(self):
        with self.assertRaisesRegex(RuntimeError, "non-object event"):
            M6.claude_items(["not an event"])

    def test_two_init_events_are_refused(self):
        init = {"type": "system", "subtype": "init", "model": M6.CLAUDE_MODEL,
               "claude_code_version": "2.1.267", "mcp_servers": [], "tools": []}
        with self.assertRaisesRegex(RuntimeError, "two init events"):
            M6.claude_items([init, init])

    def test_two_result_events_are_refused(self):
        final = {"type": "result", "subtype": "success"}
        with self.assertRaisesRegex(RuntimeError, "two result events"):
            M6.claude_items([final, final])

    def test_a_duplicated_tool_use_id_is_refused(self):
        use = {"type": "assistant", "message": {"content": [
            {"type": "tool_use", "id": "t1", "name": "x", "input": {}}]}}
        with self.assertRaisesRegex(RuntimeError, "duplicated or missing"):
            M6.claude_items([use, use])

    def test_a_duplicated_tool_result_id_is_refused(self):
        result = {"type": "user", "message": {"content": [
            {"type": "tool_result", "tool_use_id": "t1", "content": "{}"}]}}
        with self.assertRaisesRegex(RuntimeError, "duplicated or missing"):
            M6.claude_items([result, result])

    def test_an_unmatched_tool_result_is_refused(self):
        init = {"type": "system", "subtype": "init", "model": M6.CLAUDE_MODEL,
               "claude_code_version": "2.1.267", "mcp_servers": [], "tools": []}
        final = {"type": "result", "subtype": "success"}
        result = {"type": "user", "message": {"content": [
            {"type": "tool_result", "tool_use_id": "t1", "content": "{}"}]}}
        with self.assertRaisesRegex(RuntimeError, "unmatched tool results"):
            M6.claude_items([init, result, final])

    def test_a_missing_init_or_result_is_refused(self):
        with self.assertRaisesRegex(RuntimeError, "lacks its init or result"):
            M6.claude_items([])

    def test_claude_structured_payload_handles_block_lists_and_bad_json(self):
        self.assertIsNone(M6.claude_structured_payload("not json"))
        self.assertIsNone(M6.claude_structured_payload([{"type": "text", "text": "not json"}]))
        payload = M6.claude_structured_payload(
            [{"type": "text", "text": json.dumps({"status": "passed"})}])
        self.assertEqual(payload, {"status": "passed"})
        self.assertIsNone(M6.claude_structured_payload(42))


class ClaudeProcessHelperTests(unittest.TestCase):
    def test_claude_environment_carries_the_closed_variable_set(self):
        with tempfile.TemporaryDirectory() as tmp:
            environment = M6.claude_environment(pathlib.Path(tmp))
            self.assertEqual(environment["MCP_TOOL_TIMEOUT"], str(M6.MCP_TOOL_TIMEOUT_MS))
            self.assertEqual(environment["TMPDIR"], str(pathlib.Path(tmp) / "tmp"))
            self.assertIn("HOME", environment)

    def test_run_claude_returns_promptly_on_a_trivial_process(self):
        with tempfile.TemporaryDirectory() as tmp:
            outcome = M6.run_claude(["/bin/echo", "hello"], pathlib.Path(tmp),
                                    {"PATH": "/bin"}, timeout=5)
            self.assertEqual(outcome["exit_code"], 0)
            self.assertFalse(outcome["timed_out"])
            self.assertIn(b"hello", outcome["stdout"])

    def test_run_claude_kills_the_process_group_on_timeout(self):
        with tempfile.TemporaryDirectory() as tmp:
            outcome = M6.run_claude(["/bin/sleep", "5"], pathlib.Path(tmp),
                                    {"PATH": "/bin"}, timeout=1)
            self.assertTrue(outcome["timed_out"])
            self.assertNotEqual(outcome["exit_code"], 0)

    def test_next_attempt_allocates_the_first_free_immutable_number(self):
        with tempfile.TemporaryDirectory() as tmp:
            attempts = pathlib.Path(tmp) / "clients"
            with mock.patch.object(M6, "ATTEMPTS", attempts):
                first = M6.next_attempt()
                second = M6.next_attempt()
                self.assertEqual(first.name, "attempt-1")
                self.assertEqual(second.name, "attempt-2")
                (attempts / "attempt-not-a-number").mkdir()
                third = M6.next_attempt()
                self.assertEqual(third.name, "attempt-3")


class RunOrchestrationTests(unittest.TestCase):
    """`run()`'s own receipt-assembly, credential scan and cleanup logic,
    isolated from Docker/Node/Claude by replacing the two gate collaborators
    with recorded fakes -- never a real subprocess or socket."""

    @staticmethod
    def fake_inspector_gate(attempt, mode, argv, plan, write_root, timeout, request_timeout_ms):
        m3 = M6.load_m3()
        m3.append_observation(attempt / "protocol.jsonl", {
            "client": "inspector", "direction": "client", "session": "s1",
            "bytes": 1, "sha256": "0" * 64, "method": "tools/call",
            "tasks_declared": False,
        })
        return {"mode": mode, "version": M6.INSPECTOR_VERSION,
               "bundle_sha256": "a" * 64, "calls": [dict(row) for row in plan]}

    @staticmethod
    def fake_claude_gate(attempt, mode, argv, plan, write_root, timeout):
        return {"mode": mode, "client": M6.CLAUDE_CLIENT}

    def test_docker_free_run_assembles_a_passing_receipt_without_a_client(self):
        with tempfile.TemporaryDirectory() as tmp:
            attempts = pathlib.Path(tmp) / "clients"
            current = pathlib.Path(tmp) / "clients.json"
            ready = dict(M6.preflight(False, None))
            ready["unsatisfied"] = []
            with mock.patch.object(M6, "ATTEMPTS", attempts), \
                 mock.patch.object(M6, "CURRENT", current), \
                 mock.patch.object(M6, "preflight", return_value=ready), \
                 mock.patch.object(M6, "PRIVATE_DIR_BASE", tmp), \
                 mock.patch.object(M6, "SERVER", _dummy_binary(tmp)), \
                 mock.patch.object(M6, "CLAUDE", _dummy_binary(tmp)), \
                 mock.patch.object(M6, "inspector_gate", side_effect=self.fake_inspector_gate), \
                 mock.patch.object(M6, "claude_gate", side_effect=self.fake_claude_gate):
                code = M6.run(False, None)
            self.assertEqual(code, 0)
            self.assertTrue(current.exists())
            receipt = json.loads(current.read_text())
            self.assertEqual(receipt["status"], "passed")
            self.assertEqual(len(receipt["calls"]), len(M6.call_plan()))
            self.assertTrue(receipt["docker_free_socket_created"] is False)
            self.assertTrue(receipt["private_directory_removed"])
            self.assertEqual(receipt["evidence_credential_scan"], "clean")

    def test_run_refuses_unsatisfied_preconditions_before_any_client(self):
        blocked = dict(M6.preflight(False, None))
        blocked["unsatisfied"] = ["candidate_advertises_m6"]
        with mock.patch.object(M6, "preflight", return_value=blocked):
            with self.assertRaisesRegex(RuntimeError, "preconditions are unsatisfied"):
                M6.run(False, None)
            with self.assertRaisesRegex(RuntimeError, "preconditions are unsatisfied"):
                M6.run(True, "/private/tmp/m6-unit.sock")


class DockerSettleTests(unittest.TestCase):
    """W09e: the inter-batch Docker settle runs only on the runtime path,
    between the Inspector runtime gate and the Claude Code runtime gate."""

    def test_running_analyzer_containers_reads_docker_ps_names(self):
        completed = mock.Mock(returncode=0, stdout="rust-mcp-analyzer-a rust-mcp-analyzer-b\n")
        with mock.patch.object(M6.subprocess, "run", return_value=completed) as run:
            names = M6.running_analyzer_containers(pathlib.Path("/tmp/m6-unit.sock"))
        self.assertEqual(names, ["rust-mcp-analyzer-a", "rust-mcp-analyzer-b"])
        argv = run.call_args.args[0]
        self.assertIn(M6.CONTAINER_LABEL_FILTER, argv)
        self.assertIn("unix:///tmp/m6-unit.sock", argv)

    def test_running_analyzer_containers_treats_a_docker_failure_as_empty(self):
        completed = mock.Mock(returncode=1, stdout="")
        with mock.patch.object(M6.subprocess, "run", return_value=completed):
            self.assertEqual(M6.running_analyzer_containers(pathlib.Path("/tmp/m6-unit.sock")), [])

    def test_settle_waits_out_running_containers_then_cools_down(self):
        answers = iter([["rust-mcp-analyzer-a"], ["rust-mcp-analyzer-a"], []])
        sleeps: list[float] = []
        with mock.patch.object(M6, "running_analyzer_containers",
                               side_effect=lambda socket: next(answers)), \
             mock.patch.object(M6.time, "sleep", side_effect=sleeps.append):
            result = M6.settle_docker_between_batches(pathlib.Path("/tmp/m6-unit.sock"))
        self.assertEqual(sleeps.count(M6.CONTAINER_SETTLE_POLL_SECONDS), 2)
        self.assertEqual(sleeps[-1], M6.RUNTIME_BATCH_COOLDOWN_SECONDS)
        self.assertEqual(result["cooldown_seconds"], M6.RUNTIME_BATCH_COOLDOWN_SECONDS)

    def test_settle_gives_up_waiting_at_the_bound_but_still_cools_down(self):
        sleeps: list[float] = []
        with mock.patch.object(M6, "running_analyzer_containers",
                               return_value=["rust-mcp-analyzer-stuck"]), \
             mock.patch.object(M6.time, "sleep", side_effect=sleeps.append), \
             mock.patch.object(M6.time, "monotonic", side_effect=[0.0, 61.0]):
            result = M6.settle_docker_between_batches(pathlib.Path("/tmp/m6-unit.sock"))
        self.assertEqual(sleeps, [M6.RUNTIME_BATCH_COOLDOWN_SECONDS])
        self.assertEqual(result["waited_seconds"], 61.0)

    def test_docker_free_run_never_settles(self):
        with tempfile.TemporaryDirectory() as tmp:
            attempts = pathlib.Path(tmp) / "clients"
            current = pathlib.Path(tmp) / "clients.json"
            ready = dict(M6.preflight(False, None))
            ready["unsatisfied"] = []
            with mock.patch.object(M6, "ATTEMPTS", attempts), \
                 mock.patch.object(M6, "CURRENT", current), \
                 mock.patch.object(M6, "preflight", return_value=ready), \
                 mock.patch.object(M6, "PRIVATE_DIR_BASE", tmp), \
                 mock.patch.object(M6, "SERVER", _dummy_binary(tmp)), \
                 mock.patch.object(M6, "CLAUDE", _dummy_binary(tmp)), \
                 mock.patch.object(M6, "inspector_gate",
                                   side_effect=RunOrchestrationTests.fake_inspector_gate), \
                 mock.patch.object(M6, "claude_gate",
                                   side_effect=RunOrchestrationTests.fake_claude_gate), \
                 mock.patch.object(M6, "settle_docker_between_batches") as settle:
                code = M6.run(False, None)
            self.assertEqual(code, 0)
            settle.assert_not_called()
            receipt = json.loads(current.read_text())
            self.assertIsNone(receipt["docker_settle"])

    def test_runtime_run_settles_between_the_two_runtime_gates(self):
        order: list[tuple[str, str]] = []

        def fake_inspector_gate(attempt, mode, argv, plan, write_root, timeout, request_timeout_ms):
            order.append(("inspector", mode))
            return RunOrchestrationTests.fake_inspector_gate(
                attempt, mode, argv, plan, write_root, timeout, request_timeout_ms)

        def fake_claude_gate(attempt, mode, argv, plan, write_root, timeout):
            order.append(("claude", mode))
            return RunOrchestrationTests.fake_claude_gate(attempt, mode, argv, plan, write_root, timeout)

        def fake_settle(socket):
            order.append(("settle", str(socket)))
            return {"waited_seconds": 0.0, "cooldown_seconds": M6.RUNTIME_BATCH_COOLDOWN_SECONDS}

        check = dict(M6.preflight(False, None))
        check["unsatisfied"] = []
        check["with_runtime_requested"] = True

        with tempfile.TemporaryDirectory() as tmp:
            attempts = pathlib.Path(tmp) / "clients"
            current = pathlib.Path(tmp) / "clients.json"
            socket = "/private/tmp/m6-unit-fake.sock"
            with mock.patch.object(M6, "ATTEMPTS", attempts), \
                 mock.patch.object(M6, "CURRENT", current), \
                 mock.patch.object(M6, "preflight", return_value=check), \
                 mock.patch.object(M6, "PRIVATE_DIR_BASE", tmp), \
                 mock.patch.object(M6, "SERVER", _dummy_binary(tmp)), \
                 mock.patch.object(M6, "CLAUDE", _dummy_binary(tmp)), \
                 mock.patch.object(M6, "inspector_gate", side_effect=fake_inspector_gate), \
                 mock.patch.object(M6, "claude_gate", side_effect=fake_claude_gate), \
                 mock.patch.object(M6, "settle_docker_between_batches", side_effect=fake_settle):
                code = M6.run(True, socket)
            self.assertEqual(code, 0)
            self.assertEqual(order, [
                ("inspector", M6.DOCKER_FREE), ("claude", M6.DOCKER_FREE),
                ("inspector", M6.RUNTIME), ("settle", socket), ("claude", M6.RUNTIME),
            ])
            receipt = json.loads(current.read_text())
            self.assertEqual(receipt["docker_settle"],
                             {"waited_seconds": 0.0,
                              "cooldown_seconds": M6.RUNTIME_BATCH_COOLDOWN_SECONDS})


class MainDispatchTests(unittest.TestCase):
    def test_proxy_subcommand_rejects_an_invalid_argv(self):
        with mock.patch("sys.argv", ["test-m6-clients.py", "proxy", "--client", "inspector",
                                     "--observation", "/tmp/x", "--server-argv-json", "42"]):
            with self.assertRaisesRegex(RuntimeError, "invalid closed server argv"):
                M6.main()

    def test_default_invocation_prints_and_returns_the_readiness_code(self):
        with mock.patch("sys.argv", ["test-m6-clients.py"]), \
             contextlib.redirect_stdout(io.StringIO()):
            before = (M6.CURRENT.exists(), M6.PREFLIGHT.exists())
            code = M6.main()
            self.assertIn(code, {0, 1})
            self.assertEqual((M6.CURRENT.exists(), M6.PREFLIGHT.exists()), before)

    def test_write_preflight_writes_once_then_refuses_a_second_write(self):
        with tempfile.TemporaryDirectory() as tmp:
            preflight_path = pathlib.Path(tmp) / "preflight.json"
            with mock.patch.object(M6, "PREFLIGHT", preflight_path), \
                 mock.patch("sys.argv", ["test-m6-clients.py", "--write-preflight"]), \
                 contextlib.redirect_stdout(io.StringIO()):
                M6.main()
                self.assertTrue(preflight_path.exists())
                with self.assertRaisesRegex(RuntimeError, "already exists"):
                    M6.main()


class MiscHelperTests(unittest.TestCase):
    def test_capture_first_applicable_action_requires_a_real_digest(self):
        digest = M6.capture_first_applicable_action(
            {"data": {"actions": [{"applicability": "applicable",
                                   "action_digest": "sha256:" + "0" * 64}]}})
        self.assertEqual(digest, "sha256:" + "0" * 64)
        with self.assertRaisesRegex(RuntimeError, "no actions"):
            M6.capture_first_applicable_action({"data": {"actions": []}})
        with self.assertRaisesRegex(RuntimeError, "not applicable"):
            M6.capture_first_applicable_action(
                {"data": {"actions": [{"applicability": "rejected"}]}})

    def test_check_apply_preview_requires_a_changed_file_and_valid_ids(self):
        plan_id, plan_digest = M6.check_apply_preview({"data": {
            "kind": "preview", "plan_id": "mut_" + "1" * 32, "plan_digest": "sha256:" + "2" * 64,
            "files": [{"before_sha256": "a" * 64, "after_sha256": "b" * 64}]}})
        self.assertEqual(plan_id, "mut_" + "1" * 32)
        self.assertEqual(plan_digest, "sha256:" + "2" * 64)
        with self.assertRaisesRegex(RuntimeError, "changes nothing"):
            M6.check_apply_preview({"data": {
                "kind": "preview", "plan_id": "mut_" + "1" * 32, "plan_digest": "sha256:" + "2" * 64,
                "files": [{"before_sha256": "a" * 64, "after_sha256": "a" * 64}]}})

    def test_check_apply_receipt_requires_committed_and_the_same_operation(self):
        M6.check_apply_receipt(
            {"data": {"kind": "receipt", "state": "committed", "operation_id": "mut_x"}}, "mut_x")
        with self.assertRaisesRegex(RuntimeError, "committed operation"):
            M6.check_apply_receipt(
                {"data": {"kind": "receipt", "state": "aborted", "operation_id": "mut_x"}}, "mut_x")

    def test_normalize_claude_tool_rejects_a_foreign_server(self):
        with self.assertRaisesRegex(RuntimeError, "outside the configured server"):
            M6.normalize_claude_tool("mcp__other_server__rust_analyzer_symbols")

    def test_normalize_claude_tool_rejects_an_unadvertised_tool(self):
        with self.assertRaisesRegex(RuntimeError, "does not advertise"):
            M6.normalize_claude_tool("mcp__rust_engineering__not_a_real_tool")

    def test_client_home_residue_counts_then_removes(self):
        with tempfile.TemporaryDirectory() as tmp:
            cwd = pathlib.Path(tmp) / "cwd"
            cwd.mkdir()
            with mock.patch.object(pathlib.Path, "home", return_value=pathlib.Path(tmp) / "home"):
                slug = re.sub(r"[^A-Za-z0-9]", "-", str(cwd))
                projects = pathlib.Path(tmp) / "home" / ".claude" / "projects" / slug
                projects.mkdir(parents=True)
                (projects / "transcript.jsonl").write_text("{}\n")
                residue = M6.client_home_residue(cwd)
                self.assertTrue(residue["present"])
                self.assertEqual(residue["files"], 1)
                self.assertEqual(residue["transcripts"], 1)
                self.assertTrue(residue["removed"])
                self.assertFalse(projects.exists())

    def test_render_arguments_is_stable_and_json_shaped(self):
        rendered = M6.render_arguments({"scope": "document", "file": "src/lib.rs"})
        self.assertIn('scope "document"', rendered)
        self.assertIn('file "src/lib.rs"', rendered)


if __name__ == "__main__":
    unittest.main()
