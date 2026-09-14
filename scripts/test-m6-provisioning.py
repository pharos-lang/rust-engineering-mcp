#!/usr/bin/env python3
"""Unit tests for the pure functions of scripts/build-m6-runtime.py and their
consistency with fixtures/rust-runtime/m6/provision.py. No network, no Docker."""
import importlib.util
import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[1]


def _load(name: str, path: pathlib.Path):
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


build = _load("m6_build_runtime", ROOT / "scripts/build-m6-runtime.py")
provision = _load("m6_provision_for_tests", ROOT / "fixtures/rust-runtime/m6/provision.py")


class BesideDefaultTests(unittest.TestCase):
    def test_only_the_basename_of_the_value_is_honoured(self) -> None:
        default = pathlib.Path("/repo/docs/validation/M6/provisioning.json")
        result = build.beside_default(default, "/etc/passwd")
        self.assertEqual(result, pathlib.Path("/repo/docs/validation/M6/passwd"))

    def test_traversal_in_the_value_cannot_escape_the_default_directory(self) -> None:
        default = pathlib.Path("/repo/docs/validation/M6/provisioning.json")
        result = build.beside_default(default, "../../../etc/passwd")
        self.assertEqual(result, pathlib.Path("/repo/docs/validation/M6/passwd"))

    def test_default_path_object_round_trips(self) -> None:
        default = pathlib.Path("/repo/target/m6-provisioning")
        self.assertEqual(build.beside_default(default, default), default)


class EvaluateStatusTests(unittest.TestCase):
    def test_all_checks_passing_is_ok(self) -> None:
        self.assertTrue(
            build.evaluate_status(["present"], "off_path", ["present", "present"], True, "clean")
        )

    def test_missing_new_component_fails(self) -> None:
        self.assertFalse(
            build.evaluate_status(["absent"], "off_path", ["present"], True, "clean")
        )

    def test_rust_analyzer_reachable_on_path_fails(self) -> None:
        self.assertFalse(
            build.evaluate_status(["present"], "on_path", ["present"], True, "clean")
        )

    def test_missing_carried_binary_fails(self) -> None:
        self.assertFalse(
            build.evaluate_status(["present"], "off_path", ["present", "absent"], True, "clean")
        )

    def test_missing_rust_src_fails(self) -> None:
        self.assertFalse(
            build.evaluate_status(["present"], "off_path", ["present"], False, "clean")
        )

    def test_build_context_residue_fails(self) -> None:
        self.assertFalse(
            build.evaluate_status(["present"], "off_path", ["present"], True, "residue")
        )


class ReceiptSkeletonTests(unittest.TestCase):
    def test_skeleton_carries_the_fixed_authorization_and_schema_fields(self) -> None:
        skeleton = build.build_receipt_skeleton("2026-09-11T00:00:00Z")
        self.assertEqual(skeleton["schema"], "rust-engineering-mcp.m6-provisioning.v1")
        self.assertEqual(skeleton["authorization"], "docs/roadmap/m6-provisioning-request.md")
        self.assertEqual(skeleton["authorized_by_owner"], "2026-09-11")
        self.assertEqual(skeleton["decision"], "docs/adr/ADR-082-m6-runtime-provisioning.md")
        self.assertEqual(
            skeleton["network_used_for"],
            ["manifest", "rust-analyzer tarball", "rust-src tarball"],
        )
        self.assertEqual(skeleton["build_network"], "none")
        self.assertEqual(skeleton["base_tag"], build.BASE_TAG)
        self.assertEqual(skeleton["base_image_id_expected"], build.BASE_IMAGE_ID)
        self.assertEqual(skeleton["started_at"], "2026-09-11T00:00:00Z")


class ConsistencyWithProvisionTests(unittest.TestCase):
    """The wrapper and the provisioning step must agree on the base image and
    the declared network scope; a drift here would silently widen the pin."""

    def test_base_image_id_matches_provision_py(self) -> None:
        self.assertEqual(build.BASE_IMAGE_ID, provision.BASE_IMAGE_ID)

    def test_base_tag_names_the_m5_image(self) -> None:
        self.assertEqual(build.BASE_TAG, "rust-engineering-runtime:1.98.1-arm64-m5")

    def test_target_tag_names_the_m6_image(self) -> None:
        self.assertEqual(build.TARGET_TAG, "rust-engineering-runtime:1.98.1-arm64-m6")

    def test_new_components_are_off_path_by_contract(self) -> None:
        self.assertEqual(build.ANALYZER_BINARIES, ("/opt/analyzer/bin/rust-analyzer",))

    def test_carried_binaries_span_m3_through_m5(self) -> None:
        self.assertEqual(
            set(build.CARRIED_BINARIES),
            {
                "/opt/rust/bin/cargo",
                "/opt/rust/bin/rustc",
                "/opt/security/bin/cargo-deny",
                "/opt/security/bin/rust-mcp-unsafe-helper",
                "/opt/perf/bin/cargo-bloat",
                "/opt/perf/bin/rust-mcp-profile-helper",
            },
        )


class PresenceReportCommandShapeTests(unittest.TestCase):
    """`presence_report` builds the shell command it hands to the guest; the
    string itself is pure and testable without a container."""

    def test_command_tests_each_binary_in_order(self) -> None:
        captured: dict[str, object] = {}

        def fake_guest_capture(image: str, command: str) -> str:
            captured["image"] = image
            captured["command"] = command
            return "present absent"

        original = build.guest_capture
        build.guest_capture = fake_guest_capture
        try:
            result = build.presence_report("img", ("/a/b", "/c/d"))
        finally:
            build.guest_capture = original
        self.assertEqual(result, ["present", "absent"])
        self.assertEqual(captured["image"], "img")
        self.assertIn("[ -x /a/b ]", captured["command"])
        self.assertIn("[ -x /c/d ]", captured["command"])


if __name__ == "__main__":
    unittest.main()
