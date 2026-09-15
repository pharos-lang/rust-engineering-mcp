#!/usr/bin/env python3
"""Benign, Docker-free, client-free unit tests for the M8 client harness."""
from __future__ import annotations

import importlib.util
import json
import pathlib
import tempfile
import unittest
from unittest import mock

ROOT = pathlib.Path(__file__).resolve().parents[1]
SUBJECT = ROOT / "scripts/test-m8-clients.py"
SPEC = importlib.util.spec_from_file_location("m8_clients", SUBJECT)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("M8 harness unavailable")
M8 = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(M8)


class InventoryTests(unittest.TestCase):
    def test_expected_tools_is_exactly_31_stable_plus_5_preview(self):
        self.assertEqual(M8.EXPECTED_TOOLS, M8.STABLE_TOOLS + M8.PREVIEW_TOOLS)
        self.assertEqual(len(M8.STABLE_TOOLS), 31)
        self.assertEqual(len(M8.PREVIEW_TOOLS), 5)
        self.assertEqual(len(set(M8.EXPECTED_TOOLS)), 36)

    def test_expected_tools_matches_the_servers_own_protocol_oracle(self):
        self.assertEqual(M8.protocol_inventory(), M8.EXPECTED_TOOLS)

    def test_inventory_check_reports_the_binding_sources(self):
        report = M8.inventory_check()
        self.assertEqual(report["count"], 36)
        self.assertEqual(report["stable_count"], 31)
        self.assertEqual(report["preview_count"], 5)

    def test_a_drifted_inventory_is_a_failure_not_a_warning(self):
        with mock.patch.object(M8, "protocol_inventory", return_value=M8.EXPECTED_TOOLS[:-1]):
            with self.assertRaisesRegex(RuntimeError, "drifted"):
                M8.inventory_check()

    def test_a_reordered_stable_preview_split_is_refused(self):
        swapped = M8.PREVIEW_TOOLS + M8.STABLE_TOOLS
        with mock.patch.object(M8, "protocol_inventory", return_value=swapped):
            with self.assertRaisesRegex(RuntimeError, "drifted|split"):
                M8.inventory_check()

    def test_every_stable_tool_has_a_tool_source_entry(self):
        for tool in M8.STABLE_TOOLS:
            self.assertIn(tool, M8.TOOL_SOURCES)


class ErrorVocabularyTests(unittest.TestCase):
    def test_declared_codes_use_screaming_snake_case_for_ordinary_tools(self):
        codes = M8.declared_error_codes("rust.check")
        self.assertIn("SANDBOX_DENIED", codes)
        self.assertIn("LOCKFILE_UPDATE_REQUIRED", codes)
        for code in codes:
            self.assertEqual(code, code.upper())

    def test_declared_codes_use_snake_case_for_the_mutation_family(self):
        for tool in M8.MUTATION_TOOLS:
            codes = M8.declared_error_codes(tool)
            self.assertIn("permission_denied", codes)
            for code in codes:
                self.assertEqual(code, code.lower())

    def test_project_open_unions_blocked_and_unavailable_codes(self):
        codes = M8.declared_error_codes("rust.project.open")
        self.assertIn("PROJECT_NOT_FOUND", codes)  # BlockedCode
        self.assertIn("TOOL_NOT_INSTALLED", codes)  # UnavailableCode

    def test_every_declared_tool_source_actually_resolves(self):
        for tool in M8.STABLE_TOOLS:
            self.assertTrue(M8.declared_error_codes(tool))

    def test_screaming_and_snake_helpers(self):
        self.assertEqual(M8.screaming("ActionStale"), "ACTION_STALE")
        self.assertEqual(M8.snake("PermissionDenied"), "permission_denied")

    def test_an_invalid_enum_source_is_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "broken.rs"
            path.write_text('#[serde(rename_all = "SCREAMING_SNAKE_CASE")]\nenum Code {\n}\n')
            with mock.patch.dict(M8.TOOL_SOURCES, {"rust.check": ((path, "Code"),)}):
                with self.assertRaisesRegex(RuntimeError, "empty"):
                    M8.declared_error_codes("rust.check")

    def test_an_enum_missing_rename_all_is_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "broken.rs"
            path.write_text("enum Code {\n    Something,\n}\n")
            with mock.patch.dict(M8.TOOL_SOURCES, {"rust.check": ((path, "Code"),)}):
                with self.assertRaisesRegex(RuntimeError, "missing a #\\[serde\\(rename_all"):
                    M8.declared_error_codes("rust.check")

    def test_an_unsupported_rename_all_convention_is_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "broken.rs"
            path.write_text('#[serde(rename_all = "kebab-case")]\nenum Code {\n    Something,\n}\n')
            with mock.patch.dict(M8.TOOL_SOURCES, {"rust.check": ((path, "Code"),)}):
                with self.assertRaisesRegex(RuntimeError, "unsupported"):
                    M8.declared_error_codes("rust.check")

    def test_declared_codes_honors_an_unrelated_enums_rename_all_correctly(self):
        # Two enums in the same file, each with its own `rename_all`: the
        # nearer attribute block (directly above `Code`) must win, never the
        # one belonging to an earlier, unrelated enum.
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "mixed.rs"
            path.write_text(
                '#[serde(rename_all = "snake_case")]\n'
                "enum Other {\n    ThingOne,\n}\n\n"
                '#[derive(Serialize)]\n'
                '#[serde(rename_all = "SCREAMING_SNAKE_CASE")]\n'
                "enum Code {\n    ThingTwo,\n}\n"
            )
            with mock.patch.dict(M8.TOOL_SOURCES, {"rust.check": ((path, "Code"),)}):
                self.assertEqual(M8.declared_error_codes("rust.check"), frozenset({"THING_TWO"}))

    def test_an_explicit_variant_rename_overrides_the_enums_transform(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "renamed.rs"
            path.write_text(
                '#[serde(rename_all = "SCREAMING_SNAKE_CASE")]\n'
                "enum Code {\n"
                '    #[serde(rename = "legacy_code")]\n'
                "    ThingOne,\n"
                "    ThingTwo,\n"
                "}\n"
            )
            with mock.patch.dict(M8.TOOL_SOURCES, {"rust.check": ((path, "Code"),)}):
                self.assertEqual(
                    M8.declared_error_codes("rust.check"), frozenset({"legacy_code", "THING_TWO"}))


class NegativePlanTests(unittest.TestCase):
    def test_negative_plan_covers_exactly_the_31_stable_tools_once_each(self):
        plan = M8.negative_call_plan()
        self.assertEqual(len(plan), 31)
        self.assertEqual({row["tool"] for row in plan}, set(M8.STABLE_TOOLS))

    def test_negative_plan_rows_carry_declared_codes_and_rationale(self):
        for row in M8.negative_call_plan():
            self.assertTrue(row["rationale"])
            self.assertTrue(row["declared_codes"])
            self.assertEqual(row["declared_codes"], sorted(row["declared_codes"]))

    def test_a_negative_plan_missing_a_tool_is_refused(self):
        missing = tuple(row for row in M8.NEGATIVE_ROWS if row["tool"] != "rust.check")
        with mock.patch.object(M8, "NEGATIVE_ROWS", missing):
            with self.assertRaisesRegex(RuntimeError, "omits"):
                M8.negative_call_plan()

    def test_a_hard_coded_project_ref_is_refused(self):
        broken = {**M8.NEGATIVE_ROWS[3], "arguments": {"project_ref": "prj_" + "0" * 32}}
        with self.assertRaisesRegex(RuntimeError, "never hard-coded"):
            M8.check_negative_row(broken)

    def test_a_hard_coded_fingerprint_is_refused(self):
        broken = {**M8.NEGATIVE_ROWS[14],
                 "arguments": {"action": {"expected_project_fingerprint": "sha256:" + "0" * 64}}}
        with self.assertRaisesRegex(RuntimeError, "never hard-coded"):
            M8.check_negative_row(broken)

    def test_a_non_mutation_tool_may_not_declare_a_fingerprint_target(self):
        broken = {**M8.NEGATIVE_ROWS[3], "fingerprint_target": "top"}
        with self.assertRaisesRegex(RuntimeError, "mutation family"):
            M8.check_negative_row(broken)

    def test_a_mutation_tool_must_nest_its_fingerprint_under_action(self):
        broken = {**M8.NEGATIVE_ROWS[14], "fingerprint_target": "top"}
        with self.assertRaisesRegex(RuntimeError, "nests its fingerprint under action"):
            M8.check_negative_row(broken)

    def test_a_row_naming_a_preview_tool_is_refused(self):
        broken = {**M8.NEGATIVE_ROWS[3], "tool": "rust.analyzer.symbols"}
        with self.assertRaisesRegex(RuntimeError, "non-stable"):
            M8.check_negative_row(broken)

    def test_a_row_without_rationale_is_refused(self):
        broken = {**M8.NEGATIVE_ROWS[3], "rationale": ""}
        with self.assertRaisesRegex(RuntimeError, "rationale"):
            M8.check_negative_row(broken)

    def test_a_non_bool_observation_only_is_refused(self):
        broken = {**M8.NEGATIVE_ROWS[3], "observation_only": "yes"}
        with self.assertRaisesRegex(RuntimeError, "observation_only must be a bool"):
            M8.check_negative_row(broken)

    def test_a_refusal_row_missing_its_expected_code_is_refused(self):
        broken = {**M8.NEGATIVE_ROWS[3], "expected_code": None}
        with self.assertRaisesRegex(RuntimeError, "fix its own expected_code"):
            M8.check_negative_row(broken)

    def test_a_refusal_row_with_an_undeclared_expected_code_is_refused(self):
        broken = {**M8.NEGATIVE_ROWS[3], "expected_code": "NOT_A_REAL_CODE"}
        with self.assertRaisesRegex(RuntimeError, "outside its own declared vocabulary"):
            M8.check_negative_row(broken)

    def test_an_observation_only_row_with_an_expected_code_is_refused(self):
        catalog_status = next(row for row in M8.NEGATIVE_ROWS if row["tool"] == "rust.catalog.status")
        broken = {**catalog_status, "expected_code": "SANDBOX_DENIED"}
        with self.assertRaisesRegex(RuntimeError, "observation_only row never expects"):
            M8.check_negative_row(broken)

    def test_semver_check_names_both_project_ref_fields(self):
        row = next(row for row in M8.NEGATIVE_ROWS if row["tool"] == "rust.semver.check")
        self.assertEqual(row["project_ref_fields"], ("baseline_project_ref", "candidate_project_ref"))

    def test_generic_negative_plan_carries_all_four_kinds(self):
        kinds = {row["kind"] for row in M8.generic_negative_plan()}
        self.assertEqual(kinds, {"unknown_tool", "invalid_args", "unknown_project_ref", "unknown_fields"})

    def test_generic_negative_plan_missing_a_kind_is_refused(self):
        pruned = tuple(row for row in M8.GENERIC_NEGATIVE_ROWS if row["kind"] != "unknown_tool")
        with mock.patch.object(M8, "GENERIC_NEGATIVE_ROWS", pruned):
            with self.assertRaisesRegex(RuntimeError, "missing a required kind"):
                M8.generic_negative_plan()


class ContractEqualityTests(unittest.TestCase):
    def _synthetic_manifest(self):
        tools = {}
        for name in M8.EXPECTED_TOOLS:
            tools[name] = {
                "stability": "preview" if name in M8.PREVIEW_TOOLS else "stable",
                "annotations": {"readOnlyHint": True},
                "input_schema_sha256": "a" * 64, "output_schema_sha256": "b" * 64,
                "description_sha256": "c" * 64,
            }
        return {"tools": tools}

    def _observed_from(self, manifest):
        return {name: {"annotations": entry["annotations"],
                       "input_schema_sha256": entry["input_schema_sha256"],
                       "output_schema_sha256": entry["output_schema_sha256"],
                       "description_sha256": entry["description_sha256"]}
               for name, entry in manifest["tools"].items()}

    def test_identical_contract_reports_no_discrepancies(self):
        manifest = self._synthetic_manifest()
        observed = self._observed_from(manifest)
        stable_bad, preview_bad = M8.contract_discrepancies(observed, manifest, frozenset(M8.PREVIEW_TOOLS))
        self.assertEqual(stable_bad, [])
        self.assertEqual(preview_bad, [])

    def test_a_stable_schema_drift_is_reported(self):
        manifest = self._synthetic_manifest()
        observed = self._observed_from(manifest)
        observed["rust.check"]["input_schema_sha256"] = "z" * 64
        stable_bad, preview_bad = M8.contract_discrepancies(observed, manifest, frozenset(M8.PREVIEW_TOOLS))
        self.assertEqual(stable_bad, [{"tool": "rust.check", "field": "input_schema_sha256"}])
        self.assertEqual(preview_bad, [])

    def test_a_preview_schema_drift_is_reported_separately(self):
        manifest = self._synthetic_manifest()
        observed = self._observed_from(manifest)
        observed["rust.analyzer.symbols"]["description_sha256"] = "z" * 64
        stable_bad, preview_bad = M8.contract_discrepancies(observed, manifest, frozenset(M8.PREVIEW_TOOLS))
        self.assertEqual(stable_bad, [])
        self.assertEqual(preview_bad, [{"tool": "rust.analyzer.symbols", "field": "description_sha256"}])

    def test_an_annotations_drift_is_reported(self):
        manifest = self._synthetic_manifest()
        observed = self._observed_from(manifest)
        observed["rust.fmt.apply"]["annotations"] = {"readOnlyHint": False}
        stable_bad, _ = M8.contract_discrepancies(observed, manifest, frozenset(M8.PREVIEW_TOOLS))
        self.assertEqual(stable_bad, [{"tool": "rust.fmt.apply", "field": "annotations"}])

    def test_a_wrong_stability_label_is_reported(self):
        manifest = self._synthetic_manifest()
        manifest["tools"]["rust.check"]["stability"] = "preview"
        observed = self._observed_from(manifest)
        stable_bad, _ = M8.contract_discrepancies(observed, manifest, frozenset(M8.PREVIEW_TOOLS))
        self.assertIn({"tool": "rust.check", "field": "stability"}, stable_bad)

    def test_a_missing_live_tool_is_reported_as_a_presence_mismatch(self):
        manifest = self._synthetic_manifest()
        observed = self._observed_from(manifest)
        del observed["rust.check"]
        stable_bad, _ = M8.contract_discrepancies(observed, manifest, frozenset(M8.PREVIEW_TOOLS))
        self.assertTrue(any(row["tool"] == "rust.check" and row["field"] == "presence" for row in stable_bad))

    def test_the_real_freeze_manifest_loads_and_names_the_current_inventory(self):
        manifest = M8.load_freeze_manifest()
        self.assertEqual(manifest["tool_count"], 36)
        self.assertEqual(set(manifest["tools"]), set(M8.EXPECTED_TOOLS))

    def test_load_freeze_manifest_refuses_a_stale_inventory(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "freeze.json"
            path.write_text(json.dumps({"tool_count": 35, "tools": {}}))
            with mock.patch.object(M8, "FREEZE_MANIFEST", path):
                with self.assertRaisesRegex(RuntimeError, "does not name the current"):
                    M8.load_freeze_manifest()

    def test_canonical_hash_matches_the_documented_formula(self):
        import hashlib
        value = {"b": 1, "a": 2}
        expected = hashlib.sha256(
            json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")
        ).hexdigest()
        self.assertEqual(M8.canonical_hash(value), expected)


def _valid_negative_rows(plan):
    """One correctly-shaped row per planned negative: a structured refusal
    for every tool, except the `observation_only` row (`rust.catalog.status`),
    which always reports its own observed `passed`."""
    rows = []
    for row in plan:
        if row["observation_only"]:
            rows.append({"tool": row["tool"], "status": "passed", "error_code": None, "is_error": False})
        else:
            rows.append({"tool": row["tool"], "status": "unavailable",
                         "error_code": row["expected_code"], "is_error": True})
    return rows


class RowValidationTests(unittest.TestCase):
    def test_negative_plan_has_exactly_one_observation_only_row(self):
        plan = M8.negative_call_plan()
        observation_only = [row["tool"] for row in plan if row["observation_only"]]
        self.assertEqual(observation_only, ["rust.catalog.status"])

    def test_validate_negative_rows_accepts_the_planned_shape(self):
        plan = M8.negative_call_plan()
        rows = _valid_negative_rows(plan)
        validated = M8.validate_negative_rows(rows, plan)
        self.assertEqual(len(validated), len(plan))

    def test_validate_negative_rows_rejects_a_passed_status(self):
        plan = M8.negative_call_plan()
        rows = _valid_negative_rows(plan)
        # rust.check (a refusal row, not the observation_only row) reporting
        # `passed` must still be refused.
        index = next(i for i, row in enumerate(plan) if row["tool"] == "rust.check")
        rows[index] = {"tool": "rust.check", "status": "passed", "error_code": None, "is_error": False}
        with self.assertRaisesRegex(RuntimeError, "did not land on a structured refusal"):
            M8.validate_negative_rows(rows, plan)

    def test_validate_negative_rows_rejects_the_observation_only_row_reporting_a_refusal(self):
        plan = M8.negative_call_plan()
        rows = _valid_negative_rows(plan)
        index = next(i for i, row in enumerate(plan) if row["observation_only"])
        rows[index] = {"tool": "rust.catalog.status", "status": "unavailable",
                       "error_code": "SANDBOX_DENIED", "is_error": True}
        with self.assertRaisesRegex(RuntimeError, "did not land on its observed passed status"):
            M8.validate_negative_rows(rows, plan)

    def test_validate_negative_rows_rejects_an_undeclared_error_code(self):
        plan = M8.negative_call_plan()
        rows = [{"tool": row["tool"], "status": "blocked",
                "error_code": "NOT_A_REAL_CODE", "is_error": True} for row in plan
                if not row["observation_only"]]
        rows += [{"tool": row["tool"], "status": "passed", "error_code": None, "is_error": False}
                 for row in plan if row["observation_only"]]
        rows.sort(key=lambda row: [p["tool"] for p in plan].index(row["tool"]))
        with self.assertRaisesRegex(RuntimeError, "undeclared error code"):
            M8.validate_negative_rows(rows, plan)

    def test_validate_negative_rows_rejects_a_missing_is_error(self):
        plan = M8.negative_call_plan()
        rows = _valid_negative_rows(plan)
        index = next(i for i, row in enumerate(plan) if row["tool"] == "rust.check")
        rows[index] = {"tool": "rust.check", "status": "blocked",
                       "error_code": plan[index]["expected_code"], "is_error": False}
        with self.assertRaisesRegex(RuntimeError, "did not set isError"):
            M8.validate_negative_rows(rows, plan)

    def test_validate_negative_rows_rejects_a_reordered_list(self):
        plan = M8.negative_call_plan()
        rows = list(reversed(_valid_negative_rows(plan)))
        with self.assertRaisesRegex(RuntimeError, "planned order"):
            M8.validate_negative_rows(rows, plan)

    def test_validate_negative_rows_rejects_a_wrong_row_count(self):
        with self.assertRaisesRegex(RuntimeError, "one row per planned negative"):
            M8.validate_negative_rows([], M8.negative_call_plan())

    def test_validate_negative_rows_rejects_a_null_error_code_on_a_refusal(self):
        plan = M8.negative_call_plan()
        rows = _valid_negative_rows(plan)
        index = next(i for i, row in enumerate(plan) if row["tool"] == "rust.check")
        rows[index] = {"tool": "rust.check", "status": "unavailable", "error_code": None, "is_error": True}
        with self.assertRaisesRegex(RuntimeError, "null error code"):
            M8.validate_negative_rows(rows, plan)

    def test_validate_negative_rows_rejects_an_unexpected_but_declared_error_code(self):
        plan = M8.negative_call_plan()
        rows = _valid_negative_rows(plan)
        index = next(i for i, row in enumerate(plan) if row["tool"] == "rust.check")
        # TOOL_NOT_INSTALLED is declared for rust.check but is not the fixed,
        # observed row for this host (the runtime is simply never calibrated).
        rows[index] = {"tool": "rust.check", "status": "blocked",
                       "error_code": "SANDBOX_DENIED", "is_error": True}
        with self.assertRaisesRegex(RuntimeError, "reported SANDBOX_DENIED, expected"):
            M8.validate_negative_rows(rows, plan)

    def test_negative_plan_rows_carry_a_fixed_expected_code_within_their_own_vocabulary(self):
        for row in M8.negative_call_plan():
            if row["observation_only"]:
                self.assertIsNone(row["expected_code"])
            else:
                self.assertIn(row["expected_code"], row["declared_codes"])

    def test_validate_generic_negative_rows_accepts_the_planned_shape(self):
        rows = [
            {"kind": "unknown_tool", "protocol_error": True, "rpc_code": -32601},
            {"kind": "invalid_args", "protocol_error": True, "rpc_code": -32602},
            {"kind": "unknown_project_ref", "status": "blocked", "error_code": "PROJECT_NOT_FOUND", "is_error": True},
            {"kind": "unknown_fields", "protocol_error": True, "rpc_code": -32602},
        ]
        validated = M8.validate_generic_negative_rows(rows)
        self.assertEqual(len(validated), 4)

    def test_validate_generic_negative_rows_rejects_a_swallowed_protocol_error(self):
        rows = [
            {"kind": "unknown_tool", "protocol_error": False, "rpc_code": None},
            {"kind": "invalid_args", "protocol_error": True, "rpc_code": -32602},
            {"kind": "unknown_project_ref", "status": "blocked", "error_code": "PROJECT_NOT_FOUND", "is_error": True},
            {"kind": "unknown_fields", "protocol_error": True, "rpc_code": -32602},
        ]
        with self.assertRaisesRegex(RuntimeError, "protocol boundary"):
            M8.validate_generic_negative_rows(rows)

    def test_validate_generic_negative_rows_rejects_an_unstructured_project_ref_result(self):
        rows = [
            {"kind": "unknown_tool", "protocol_error": True, "rpc_code": -32601},
            {"kind": "invalid_args", "protocol_error": True, "rpc_code": -32602},
            {"kind": "unknown_project_ref", "status": "passed", "error_code": None, "is_error": False},
            {"kind": "unknown_fields", "protocol_error": True, "rpc_code": -32602},
        ]
        with self.assertRaisesRegex(RuntimeError, "structured refusal"):
            M8.validate_generic_negative_rows(rows)

    def test_validate_generic_negative_rows_rejects_the_wrong_rpc_code(self):
        rows = [
            {"kind": "unknown_tool", "protocol_error": True, "rpc_code": -32602},
            {"kind": "invalid_args", "protocol_error": True, "rpc_code": -32602},
            {"kind": "unknown_project_ref", "status": "blocked", "error_code": "PROJECT_NOT_FOUND", "is_error": True},
            {"kind": "unknown_fields", "protocol_error": True, "rpc_code": -32602},
        ]
        with self.assertRaisesRegex(RuntimeError, "expected -32601"):
            M8.validate_generic_negative_rows(rows)


class HostConfigurationTests(unittest.TestCase):
    def test_docker_free_argv_configures_a_real_but_unusable_runtime(self):
        with tempfile.TemporaryDirectory() as tmp:
            state = pathlib.Path(tmp) / "state"
            socket = pathlib.Path(tmp) / "never-created.sock"
            argv = M8.server_argv(state, socket)
            self.assertIn("--rust-image", argv)
            self.assertEqual(argv[argv.index("--rust-image") + 1], M8.RUST_IMAGE)
            self.assertIn("--docker-socket", argv)
            self.assertEqual(argv[argv.index("--docker-socket") + 1], str(socket))
            for flag in ("--allow-fmt-write", "--allow-fix-write", "--allow-manifest-write",
                        "--allow-dependency-add", "--allow-dependency-remove",
                        "--catalog-store", "--catalog-trust"):
                self.assertNotIn(flag, argv)

    def test_stale_candidate_is_detected_without_starting_the_server(self):
        with mock.patch.object(M8, "SERVER", ROOT / "Cargo.toml"):
            self.assertFalse(M8.candidate_advertises_all())

    def test_bogus_open_path_and_eof_fallback_socket_live_under_target(self):
        self.assertTrue(M8.BOGUS_OPEN_PATH.startswith(str(M8.ROOT / "target")))
        self.assertNotIn("/private/tmp", M8.BOGUS_OPEN_PATH)


class PreconditionGatingTests(unittest.TestCase):
    def test_optional_client_preconditions_never_block_run(self):
        checks = {name: {"satisfied": False, "requirement": "r"} for name in M8.OPTIONAL_PRECONDITIONS}
        self.assertEqual(M8.mandatory_unsatisfied(checks), [])

    def test_a_mandatory_precondition_blocks_run(self):
        checks = {
            "candidate_server_binary": {"satisfied": False, "requirement": "r"},
            "claude_auth": {"satisfied": False, "requirement": "r"},
        }
        self.assertEqual(M8.mandatory_unsatisfied(checks), ["candidate_server_binary"])

    def test_all_satisfied_blocks_nothing(self):
        checks = {name: {"satisfied": True, "requirement": "r"} for name in
                 ("candidate_server_binary", *M8.OPTIONAL_PRECONDITIONS)}
        self.assertEqual(M8.mandatory_unsatisfied(checks), [])


class GitReceiptMetadataTests(unittest.TestCase):
    def test_head_commit_reads_git_rev_parse(self):
        fake = mock.Mock(returncode=0, stdout="cafefeed\n")
        with mock.patch("subprocess.run", return_value=fake) as run:
            self.assertEqual(M8.git_head_commit(), "cafefeed")
        self.assertEqual(run.call_args.args[0][:2], ["git", "rev-parse"])

    def test_tree_dirty_is_true_when_porcelain_status_is_non_empty(self):
        fake = mock.Mock(returncode=0, stdout=" M scripts/test-m8-clients.py\n")
        with mock.patch("subprocess.run", return_value=fake):
            self.assertTrue(M8.git_tree_dirty())

    def test_tree_dirty_is_false_on_a_clean_tree(self):
        fake = mock.Mock(returncode=0, stdout="")
        with mock.patch("subprocess.run", return_value=fake):
            self.assertFalse(M8.git_tree_dirty())


class CodexProtocolEvidenceTests(unittest.TestCase):
    def _write(self, tmp, rows):
        path = pathlib.Path(tmp) / "protocol.jsonl"
        path.write_text("\n".join(json.dumps(row) for row in rows) + "\n")
        return path

    def test_missing_observation_reports_no_evidence(self):
        evidence = M8.codex_protocol_evidence(pathlib.Path("/nonexistent-observation.jsonl"))
        self.assertEqual(evidence, {"called_tools": set(), "unknown_tool_wire_refused": False,
                                    "unknown_project_ref_wire_refused": False})

    def test_open_and_inspect_calls_are_collected_from_the_wire(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = self._write(tmp, [
                {"client": "codex", "direction": "client", "method": "tools/call", "tool": "rust.project.open"},
                {"client": "codex", "direction": "server"},
                {"client": "codex", "direction": "client", "method": "tools/call", "tool": "rust.project.inspect"},
                {"client": "codex", "direction": "server"},
                {"client": "claude-code", "direction": "client", "method": "tools/call", "tool": "rust.check"},
            ])
            evidence = M8.codex_protocol_evidence(path)
            self.assertEqual(evidence["called_tools"], {"rust.project.open", "rust.project.inspect"})
            self.assertFalse(evidence["unknown_tool_wire_refused"])
            self.assertFalse(evidence["unknown_project_ref_wire_refused"])

    def test_unknown_tool_refusal_requires_a_server_response_on_the_wire(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = self._write(tmp, [
                {"client": "codex", "direction": "client", "method": "tools/call",
                 "tool": "rust.not.a.real.tool"},
                {"client": "codex", "direction": "server"},
            ])
            evidence = M8.codex_protocol_evidence(path)
            self.assertTrue(evidence["unknown_tool_wire_refused"])

    def test_an_unanswered_call_is_not_counted_as_a_wire_refusal(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = self._write(tmp, [
                {"client": "codex", "direction": "client", "method": "tools/call",
                 "tool": "rust.not.a.real.tool"},
            ])
            evidence = M8.codex_protocol_evidence(path)
            self.assertFalse(evidence["unknown_tool_wire_refused"])

    def test_unknown_project_ref_refusal_requires_the_structured_wire_fields(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = self._write(tmp, [
                {"client": "codex", "direction": "client", "method": "tools/call",
                 "tool": "rust.project.inspect"},
                {"client": "codex", "direction": "server", "structuredContent.status": "blocked",
                 "structuredContent.error_code": "PROJECT_NOT_FOUND"},
            ])
            evidence = M8.codex_protocol_evidence(path)
            self.assertTrue(evidence["unknown_project_ref_wire_refused"])

    def test_an_ordinary_passed_inspect_is_not_counted_as_the_refusal(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = self._write(tmp, [
                {"client": "codex", "direction": "client", "method": "tools/call",
                 "tool": "rust.project.inspect"},
                {"client": "codex", "direction": "server", "structuredContent.status": "passed",
                 "structuredContent.error_code": None},
            ])
            evidence = M8.codex_protocol_evidence(path)
            self.assertFalse(evidence["unknown_project_ref_wire_refused"])

    def test_an_event_only_project_ref_refusal_does_not_count(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = self._write(tmp, [
                {"client": "codex", "direction": "client", "method": "tools/call",
                 "tool": "rust.project.inspect"},
                {"client": "codex", "direction": "server"},
            ])
            evidence = M8.codex_protocol_evidence(path)
            self.assertFalse(evidence["unknown_project_ref_wire_refused"])


class GenericNegativeWireConfirmationTests(unittest.TestCase):
    def test_each_generic_negative_is_confirmed_when_a_server_row_follows(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "protocol.jsonl"
            rows = [{"client": "inspector", "direction": "client", "method": "tools/list"},
                    {"client": "inspector", "direction": "server"}]
            for row in M8.GENERIC_NEGATIVE_ROWS:
                rows.append({"client": "inspector", "direction": "client", "method": "tools/call",
                            "tool": row["tool"]})
                rows.append({"client": "inspector", "direction": "server"})
            path.write_text("\n".join(json.dumps(row) for row in rows) + "\n")
            confirmed = M8.generic_negative_wire_confirmed(path)
            self.assertTrue(all(confirmed.values()))
            self.assertEqual(set(confirmed), {row["kind"] for row in M8.GENERIC_NEGATIVE_ROWS})

    def test_a_short_circuited_call_with_no_server_response_is_not_confirmed(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "protocol.jsonl"
            rows = []
            for row in M8.GENERIC_NEGATIVE_ROWS:
                rows.append({"client": "inspector", "direction": "client", "method": "tools/call",
                            "tool": row["tool"]})
                rows.append({"client": "inspector", "direction": "server"})
            # The last generic negative's request never got a server row: the
            # Inspector SDK short-circuited it client-side.
            rows.pop()
            path.write_text("\n".join(json.dumps(row) for row in rows) + "\n")
            confirmed = M8.generic_negative_wire_confirmed(path)
            self.assertFalse(confirmed[M8.GENERIC_NEGATIVE_ROWS[-1]["kind"]])


class CodexClassificationTests(unittest.TestCase):
    def test_passed_requires_the_wire_refusal(self):
        classification = M8.codex_classification(
            returncode=0, stderr="", open_observed=True, inspect_observed=True,
            unknown_project_ref_wire_refused=True)
        self.assertEqual(classification, "passed")

    def test_an_event_only_refusal_does_not_pass(self):
        classification = M8.codex_classification(
            returncode=0, stderr="", open_observed=True, inspect_observed=True,
            unknown_project_ref_wire_refused=False)
        self.assertEqual(classification, "partial")

    def test_capacity_refused_is_read_from_stderr(self):
        classification = M8.codex_classification(
            returncode=1, stderr="hit a capacity limit", open_observed=False,
            inspect_observed=False, unknown_project_ref_wire_refused=False)
        self.assertEqual(classification, "capacity_refused")


class CodexConfigTests(unittest.TestCase):
    def test_toml_string_escapes_backslashes_and_quotes(self):
        self.assertEqual(M8.toml_string('a"b\\c'), '"a\\"b\\\\c"')

    def test_codex_mcp_config_args_builds_command_and_args_overrides(self):
        args = M8.codex_mcp_config_args("rust_engineering", "/usr/bin/python3", ["proxy", "--client"])
        self.assertEqual(args[0], "-c")
        self.assertIn('mcp_servers.rust_engineering.command="/usr/bin/python3"', args[1])
        self.assertEqual(args[2], "-c")
        self.assertIn('mcp_servers.rust_engineering.args=["proxy","--client"]', args[3])

    def test_codex_mcp_config_args_escapes_a_path_with_a_quote(self):
        args = M8.codex_mcp_config_args("s", 'a"b', [])
        self.assertIn('a\\"b', args[1])


class PreflightTests(unittest.TestCase):
    def test_preflight_is_non_executing_and_client_free(self):
        receipt = M8.preflight(False, None)
        self.assertFalse(receipt["execution_performed"])
        self.assertFalse(receipt["clients_started"])
        self.assertFalse(receipt["docker_used"])
        self.assertEqual(receipt["rust_image"], M8.RUST_IMAGE)

    def test_default_invocation_writes_nothing(self):
        before = M8.CURRENT.read_bytes() if M8.CURRENT.exists() else None
        M8.preflight(False, None)
        after = M8.CURRENT.read_bytes() if M8.CURRENT.exists() else None
        self.assertEqual(before, after)

    def test_runtime_preflight_adds_the_docker_socket_precondition(self):
        free_checks = set(M8.preflight(False, None)["preconditions"])
        runtime_checks = set(M8.preflight(True, None)["preconditions"])
        self.assertIn("docker_socket", runtime_checks - free_checks)
        self.assertEqual(runtime_checks - free_checks, {"docker_socket"})

    def test_preflight_status_follows_unsatisfied_preconditions(self):
        with mock.patch.object(M8, "preconditions",
                               return_value={"x": {"satisfied": False, "requirement": "r"}}):
            receipt = M8.preflight(False, None)
            self.assertEqual(receipt["status"], "blocked")
            self.assertEqual(receipt["unsatisfied"], ["x"])

    def test_with_runtime_alone_is_refused_by_main(self):
        with mock.patch("sys.argv", ["test-m8-clients.py", "--with-runtime"]):
            with self.assertRaisesRegex(RuntimeError, "requires --run"):
                M8.main()


class EvidenceTests(unittest.TestCase):
    def test_credential_text_detector_finds_common_shapes(self):
        with tempfile.TemporaryDirectory() as tmp:
            clean = pathlib.Path(tmp) / "clean.jsonl"
            clean.write_text("no secrets here, just prose\n")
            M8.assert_no_credential_text(clean)
            dirty = pathlib.Path(tmp) / "dirty.jsonl"
            dirty.write_text("Authorization: Bearer sk-ant-abc123\n")
            with self.assertRaisesRegex(RuntimeError, "credential-shaped"):
                M8.assert_no_credential_text(dirty)

    def test_protocol_metadata_validator_rejects_unapproved_keys(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "protocol.jsonl"
            path.write_text(json.dumps({"client": "inspector", "extra": "nope",
                                        "sha256": "0" * 64}) + "\n")
            with self.assertRaisesRegex(RuntimeError, "unapproved keys"):
                M8.validate_protocol_metadata(path)

    def test_protocol_metadata_validator_accepts_the_m3_proxy_shape(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "protocol.jsonl"
            rows = [
                {"client": "inspector", "direction": "client", "session": "s1",
                 "bytes": 10, "sha256": "0" * 64, "method": "initialize",
                 "tasks_declared": False},
                {"client": "inspector", "direction": "client", "session": "s1",
                 "bytes": 10, "sha256": "0" * 64, "method": "resources/read",
                 "resource_scheme": "rust-artifact"},
            ]
            path.write_text("\n".join(json.dumps(row) for row in rows) + "\n")
            summary = M8.validate_protocol_metadata(path)
            self.assertTrue(summary["metadata_only"])
            self.assertEqual(summary["row_count"], 2)

    def test_protocol_metadata_validator_accepts_a_modern_session_advertising_tasks(self):
        # M8 flips TASKS_ADVERTISEMENT_READY; a `server/discover` session must
        # observe `tasks_advertised: true`, unlike the pre-M8 harnesses this
        # oracle was copied from.
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "protocol.jsonl"
            rows = [
                {"client": "gemini-cli", "direction": "client", "session": "s1",
                 "bytes": 10, "sha256": "0" * 64, "method": "server/discover",
                 "tasks_declared": False, "tasks_advertised": True},
            ]
            path.write_text("\n".join(json.dumps(row) for row in rows) + "\n")
            summary = M8.validate_protocol_metadata(path)
            self.assertEqual(summary["clients"]["gemini-cli"]["tasks_advertised"], [True])

    def test_protocol_metadata_validator_rejects_a_modern_session_not_advertising_tasks(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = pathlib.Path(tmp) / "protocol.jsonl"
            rows = [
                {"client": "gemini-cli", "direction": "client", "session": "s1",
                 "bytes": 10, "sha256": "0" * 64, "method": "server/discover",
                 "tasks_declared": False, "tasks_advertised": False},
            ]
            path.write_text("\n".join(json.dumps(row) for row in rows) + "\n")
            with self.assertRaisesRegex(RuntimeError, "unexpected modern Tasks advertisement"):
                M8.validate_protocol_metadata(path)


class LoaderTests(unittest.TestCase):
    def test_load_module_refuses_when_the_spec_is_unavailable(self):
        with mock.patch("importlib.util.spec_from_file_location", return_value=None):
            with self.assertRaisesRegex(RuntimeError, "unavailable"):
                M8.load_module(M8.M3_PATH, "x")

    def test_load_m3_and_load_m6_expose_the_expected_helpers(self):
        m3 = M8.load_m3()
        self.assertTrue(callable(m3.proxy))
        self.assertTrue(callable(m3.run_bounded))
        m6 = M8.load_m6()
        self.assertTrue(callable(m6.claude_items))
        self.assertTrue(callable(m6.validate_claude_session))

    def test_m6_and_m8_agree_on_the_full_36_tool_inventory(self):
        m6 = M8.load_m6()
        self.assertEqual(tuple(m6.EXPECTED_TOOLS), M8.EXPECTED_TOOLS)


class ComposePriorReceiptsTests(unittest.TestCase):
    def test_compose_prior_receipts_reports_a_missing_harness(self):
        with mock.patch.object(M8, "ROOT", pathlib.Path("/nonexistent-root")):
            composed = M8.compose_prior_receipts("prj_socket")
        for entry in composed.values():
            self.assertEqual(entry["status"], "unavailable")

    def test_compose_prior_receipts_folds_a_subprocess_result(self):
        fake = mock.Mock(returncode=0, stdout="ok", stderr="")
        with mock.patch("subprocess.run", return_value=fake) as run:
            composed = M8.compose_prior_receipts("/tmp/fake.sock")
        self.assertEqual(run.call_count, 5)
        for entry in composed.values():
            self.assertEqual(entry["status"], "passed")
            self.assertEqual(entry["exit_code"], 0)


class RunExitCodeTests(unittest.TestCase):
    """`run()` fully mocked below its own gating/receipt logic: Docker-free,
    client-free, and never spawns the real `--run` matrix -- only exercises
    C-1's mandatory-precondition gate, C-4's exit code and the receipt's own
    `head_commit`/`tree_dirty`/`observed_versions` fields."""

    def _versions(self):
        return {
            "inspector": {"observed": "2.5.0"}, "codex": {"observed": "codex-cli 0.154.0"},
            "claude_code": {"observed": None}, "gemini_cli": {"observed": None},
        }

    def _run_with(self, codex_classification, inspector_contract_ok, tmp):
        attempt_dir = pathlib.Path(tmp) / "attempt-1"
        attempt_dir.mkdir()
        m3_stub = mock.Mock()
        m3_stub.file_digest.return_value = "deadbeef"
        m3_stub.assert_no_credentials = mock.Mock(return_value=None)
        with (
            mock.patch.object(M8, "load_m3", return_value=m3_stub),
            mock.patch.object(M8, "client_versions", return_value=self._versions()),
            mock.patch.object(M8, "preconditions", return_value={}),
            mock.patch.object(M8, "mandatory_unsatisfied", return_value=[]),
            mock.patch.object(M8, "next_attempt", return_value=attempt_dir),
            mock.patch.object(M8, "git_head_commit", return_value="cafefeed"),
            mock.patch.object(M8, "git_tree_dirty", return_value=False),
            mock.patch.object(M8, "server_version", return_value={"version": "0.8.0"}),
            mock.patch.object(M8, "run_inspector", return_value={"contract_equality": inspector_contract_ok}),
            mock.patch.object(M8, "codex_gate", return_value={"classification": codex_classification}),
            mock.patch.object(M8, "claude_gate", return_value={"status": "unavailable"}),
            mock.patch.object(M8, "gemini_gate", return_value={"status": "unavailable"}),
            mock.patch.object(M8, "validate_protocol_metadata", return_value={}),
            mock.patch.object(M8, "CURRENT", pathlib.Path(tmp) / "current.json"),
        ):
            return M8.run(False, None)

    def test_run_returns_zero_when_the_receipt_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.assertEqual(self._run_with("passed", True, tmp), 0)

    def test_run_returns_one_when_the_receipt_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.assertEqual(self._run_with("partial", True, tmp), 1)

    def test_run_aborts_before_any_client_when_a_mandatory_precondition_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            with (
                mock.patch.object(M8, "client_versions", return_value=self._versions()),
                mock.patch.object(M8, "preconditions", return_value={
                    "candidate_server_binary": {"satisfied": False, "requirement": "r"}}),
                mock.patch.object(M8, "next_attempt") as next_attempt,
                mock.patch.object(M8, "codex_gate") as codex_gate,
            ):
                with self.assertRaisesRegex(RuntimeError, "mandatory preconditions unsatisfied"):
                    M8.run(False, None)
            next_attempt.assert_not_called()
            codex_gate.assert_not_called()


if __name__ == "__main__":
    unittest.main()
