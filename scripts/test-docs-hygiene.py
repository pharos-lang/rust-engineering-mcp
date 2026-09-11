#!/usr/bin/env python3
"""Portable tests for scripts/docs-hygiene.py on a temporary Git repository."""
from __future__ import annotations

import hashlib
import importlib.util
import json
import os
import pathlib
import subprocess
import tempfile
import unittest
from unittest import mock

SUBJECT = pathlib.Path(__file__).with_name("docs-hygiene.py")
SPEC = importlib.util.spec_from_file_location("docs_hygiene", SUBJECT)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("docs-hygiene unavailable")
DH = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(DH)


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


class Repo:
    """A throwaway Git repository with committed files, used as the tool's ROOT."""

    def __init__(self) -> None:
        self.temp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.temp.name).resolve()
        self.git("init", "-q")
        self.git("config", "user.email", "test@example.invalid")
        self.git("config", "user.name", "test")
        self.git("config", "commit.gpgsign", "false")

    def git(self, *args: str) -> str:
        return subprocess.run(["git", *args], cwd=self.root, check=True, capture_output=True, text=True).stdout

    def write(self, path: str, text: str) -> pathlib.Path:
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text, encoding="utf-8")
        return target

    def commit(self, message: str = "snapshot") -> None:
        self.git("add", "-A")
        self.git("commit", "-q", "-m", message)

    def tracked(self) -> list[str]:
        return [p for p in self.git("ls-files", "-z").split("\0") if p]

    def close(self) -> None:
        self.temp.cleanup()


class Classification(unittest.TestCase):
    def test_living_frozen_and_path_string_sets(self) -> None:
        self.assertTrue(DH.is_living("README.md"))
        self.assertTrue(DH.is_living("docs/tools.md"))
        self.assertTrue(DH.is_living("docs/validation/M5/matrix.md"))
        self.assertTrue(DH.is_living("docs/reviews/M4/pkg/review.md"))
        self.assertFalse(DH.is_living("docs/reviews/M4/pkg/inputs/README.md"))
        self.assertFalse(DH.is_living("docs/reviews/pkg/prompt.md"))
        self.assertFalse(DH.is_living("docs/validation/M3/delegation/A01/prompt.md"))
        self.assertFalse(DH.is_living("docs/research/m1-16/measurement/results/analysis/sources.md"))
        self.assertFalse(DH.is_living("docs/prompts/cleanup-repository.md"))
        self.assertFalse(DH.is_living("crates/domain/README.md"))
        self.assertFalse(DH.is_living("docs/validation/M5/core-gate.json"))
        self.assertFalse(DH.is_living("NOTICE"))
        self.assertTrue(DH.carries_path_strings("scripts/gate.py"))
        self.assertTrue(DH.carries_path_strings(".gitignore"))
        self.assertTrue(DH.carries_path_strings("crates/domain/src/lib.rs"))
        self.assertTrue(DH.carries_path_strings("docs/adr/ADR-001.md"))
        self.assertFalse(DH.carries_path_strings("scripts/data.bin"))
        self.assertFalse(DH.carries_path_strings("vendor/x/Cargo.toml"))

    def test_link_target_parsing_and_resolution(self) -> None:
        self.assertEqual(DH.split_target("<a b.md#x>"), ("<", "a b.md", "#x>"))
        self.assertEqual(DH.split_target("a.md#frag"), ("", "a.md", "#frag"))
        self.assertEqual(DH.split_target("a.md"), ("", "a.md", ""))
        self.assertIsNone(DH.resolve("docs/a.md", "https://example.invalid/x"))
        self.assertIsNone(DH.resolve("docs/a.md", "mailto:x@y"))
        self.assertIsNone(DH.resolve("docs/a.md", ""))
        self.assertIsNone(DH.resolve("docs/a.md", "a%20b.md"))
        self.assertEqual(DH.resolve("docs/a.md", "../README.md"), "README.md")
        self.assertEqual(DH.resolve("README.md", "docs/x.md"), "docs/x.md")
        self.assertEqual(DH.resolve("docs/a.md", "/docs/x.md"), "docs/x.md")
        self.assertEqual(DH.relative_link("docs/validation/M5/matrix.md", "docs/validation/M5/core.json"), "core.json")
        self.assertEqual(DH.relative_link("README.md", "docs/x.md"), "docs/x.md")
        links = [raw for _, raw in DH.iter_links("[a](x.md) ![i](img.png)\n[ref]: y.md\n[^note]: Qualified text\n")]
        self.assertEqual(links, ["x.md", "img.png", "y.md"])

    def test_tree_and_path_mapping(self) -> None:
        tree = DH.Tree(["docs/a/b.md", "docs/c.md"])
        self.assertTrue(tree.exists("docs/a"))
        self.assertTrue(tree.exists("docs/a/b.md"))
        self.assertTrue(tree.exists("."))
        self.assertFalse(tree.exists("docs/zz"))
        file_moves = {"docs/old.md": "docs/new.md"}
        dir_moves = {"docs/pkg": "docs/M1/pkg", "docs/pkg/inner": "docs/other"}
        self.assertEqual(DH.map_path("docs/old.md", file_moves, dir_moves), "docs/new.md")
        self.assertEqual(DH.map_path("docs/pkg/x.txt", file_moves, dir_moves), "docs/M1/pkg/x.txt")
        self.assertEqual(DH.map_path("docs/pkg/inner/y", file_moves, dir_moves), "docs/other/y")
        self.assertEqual(DH.map_path("docs/pkg", file_moves, dir_moves), "docs/M1/pkg")
        self.assertEqual(DH.map_path("docs/untouched.md", file_moves, dir_moves), "docs/untouched.md")

    def test_path_string_rewrite_respects_boundaries(self) -> None:
        rewrites: list[dict] = []
        text = ('a docs/validation/M5-runtime.json. b docs/validation/M5-runtime-superseded.json '
                'c docs/validation/m5-clients/attempt-1/x d docs/validation/M5-runtime.json.bak')
        out = DH.rewrite_path_strings("scripts/x.py", text, {"docs/validation/M5-runtime.json": "docs/validation/M5/runtime.json"},
                                      {"docs/validation/m5-clients": "docs/validation/M5/clients"}, rewrites)
        self.assertIn("a docs/validation/M5/runtime.json. b docs/validation/M5-runtime-superseded.json", out)
        self.assertIn("docs/validation/M5/clients/attempt-1/x", out)
        self.assertIn("docs/validation/M5-runtime.json.bak", out)
        self.assertEqual(len(rewrites), 2)
        self.assertEqual(DH.rewrite_path_strings("x", "nothing", {"crates/a": "crates/b"}, {}, rewrites), "nothing")


class RepoBacked(unittest.TestCase):
    def setUp(self) -> None:
        self.repo = Repo()
        self.addCleanup(self.repo.close)
        patcher = mock.patch.object(DH, "ROOT", self.repo.root)
        patcher.start()
        self.addCleanup(patcher.stop)

    def seed(self) -> None:
        r = self.repo
        r.write(".gitignore", "*.log\n")
        r.write("README.md", "[status](docs/implementation-status.md) and [receipt](docs/validation/M5-core-gate.json)\n")
        r.write("docs/implementation-status.md",
                "[core](validation/M5-core-gate.json) [dir](validation/m5-clients/) "
                "[attempt](validation/m5-clients/attempt-1/receipt.json) [log](validation/run.log) "
                "[missing](validation/nope.json) [ext](https://example.invalid) `docs/validation/M5-core-gate.json`\n")
        r.write("docs/validation/M5-core-gate.json", '{"status": "passed"}\n')
        r.write("docs/validation/M5-matrix.md", "[core](M5-core-gate.json) [self](../tools.md)\n")
        r.write("docs/validation/m5-clients/attempt-1/receipt.json", "{}\n")
        r.write("docs/validation/m5-clients/attempts.md", "[r](attempt-1/receipt.json) [core](../M5-core-gate.json)\n")
        r.write("docs/tools.md", "plain\n")
        r.write("docs/reviews/pkg/inputs/copy.md", "[broken](../../../nowhere.md)\n")
        r.write("scripts/tool.py", 'RECEIPT = ROOT / "docs/validation/M5-core-gate.json"\nATTEMPTS = "docs/validation/m5-clients"\n')
        r.write("crates/x/src/lib.rs", "//! see docs/validation/M5-core-gate.json\n")
        r.write("docs/validation/run.log", "ignored evidence\n")
        r.commit()

    def test_check_links_reports_living_frozen_and_excluded(self) -> None:
        self.seed()
        report = self.repo.root / "report.json"
        with mock.patch("builtins.print"):
            code = DH.check_links(report)
        summary = json.loads(report.read_text())
        self.assertEqual(code, 1)
        self.assertEqual([r["resolved"] for r in summary["broken_living"]], ["docs/validation/nope.json"])
        self.assertEqual([r["resolved"] for r in summary["excluded_evidence"]], ["docs/validation/run.log"])
        self.assertEqual([r["file"] for r in summary["broken_frozen"]], ["docs/reviews/pkg/inputs/copy.md"])
        self.assertGreaterEqual(summary["checked"], 8)
        # An unstaged new document takes part in resolution and can fix the broken link.
        self.repo.write("docs/validation/nope.json", "{}\n")
        with mock.patch("builtins.print"):
            self.assertEqual(DH.check_links(None), 0)

    def test_apply_moves_relinks_rewrites_and_verifies_bytes(self) -> None:
        self.seed()
        plan = self.repo.root / "plan.json"
        plan.write_text(json.dumps([
            {"from": "docs/validation/M5-core-gate.json", "to": "docs/validation/M5/core-gate.json"},
            {"from": "docs/validation/M5-matrix.md", "to": "docs/validation/M5/matrix.md"},
            {"from": "docs/validation/m5-clients", "to": "docs/validation/M5/clients"},
        ]))
        with mock.patch("builtins.print"):
            self.assertEqual(DH.apply_moves(plan, True, None), 0)
        self.assertIn("docs/validation/M5-core-gate.json", self.repo.tracked())
        report = self.repo.root / "moves.json"
        with mock.patch("builtins.print"):
            self.assertEqual(DH.apply_moves(plan, False, report), 0)
        summary = json.loads(report.read_text())
        self.assertEqual(summary["moved"], 4)
        self.assertEqual(summary["hash_mismatches"], [])
        self.assertEqual(summary["moved_living_documents"], ["docs/validation/M5/clients/attempts.md",
                                                             "docs/validation/M5/matrix.md"])
        self.assertEqual(summary["hashes_verified"], 2)
        tracked = self.repo.tracked()
        self.assertIn("docs/validation/M5/core-gate.json", tracked)
        self.assertIn("docs/validation/M5/clients/attempt-1/receipt.json", tracked)
        self.assertNotIn("docs/validation/M5-core-gate.json", tracked)
        read = lambda p: (self.repo.root / p).read_text()  # noqa: E731
        self.assertIn("[receipt](docs/validation/M5/core-gate.json)", read("README.md"))
        status = read("docs/implementation-status.md")
        self.assertIn("[core](validation/M5/core-gate.json)", status)
        self.assertIn("[dir](validation/M5/clients/)", status)
        self.assertIn("[attempt](validation/M5/clients/attempt-1/receipt.json)", status)
        self.assertIn("[missing](validation/nope.json)", status)
        self.assertIn("[ext](https://example.invalid)", status)
        self.assertIn("`docs/validation/M5/core-gate.json`", status)
        self.assertEqual(read("docs/validation/M5/matrix.md"), "[core](core-gate.json) [self](../../tools.md)\n")
        self.assertEqual(read("docs/validation/M5/clients/attempts.md"),
                         "[r](attempt-1/receipt.json) [core](../core-gate.json)\n")
        self.assertIn('"docs/validation/M5/core-gate.json"', read("scripts/tool.py"))
        self.assertIn('"docs/validation/M5/clients"', read("scripts/tool.py"))
        self.assertIn("docs/validation/M5/core-gate.json", read("crates/x/src/lib.rs"))
        self.assertEqual(read("docs/reviews/pkg/inputs/copy.md"), "[broken](../../../nowhere.md)\n")
        self.assertEqual(read("docs/validation/M5/core-gate.json"), '{"status": "passed"}\n')
        with mock.patch("builtins.print"):
            self.assertEqual(DH.check_links(None), 1)  # only the pre-existing broken link remains

    def test_expand_plan_rejects_untracked_sources_and_collisions(self) -> None:
        self.seed()
        files = self.repo.tracked()
        with self.assertRaises(SystemExit):
            DH.expand_plan([{"from": "docs/validation/absent.json", "to": "x"}], files)
        with self.assertRaises(SystemExit):
            DH.expand_plan([{"from": "docs/validation/M5-core-gate.json", "to": "docs/z.json"},
                            {"from": "docs/validation/M5-matrix.md", "to": "docs/z.json"}], files)
        with self.assertRaises(SystemExit):
            DH.expand_plan([{"from": "docs/validation/M5-core-gate.json", "to": "docs/tools.md"}], files)
        file_moves, dir_moves = DH.expand_plan([{"from": "docs/validation/m5-clients/", "to": "docs/validation/M5/clients"}], files)
        self.assertEqual(dir_moves, {"docs/validation/m5-clients": "docs/validation/M5/clients"})
        self.assertEqual(set(file_moves), {"docs/validation/m5-clients/attempt-1/receipt.json",
                                           "docs/validation/m5-clients/attempts.md"})

    def test_verify_inventories_checks_hashes_presence_and_absence(self) -> None:
        r = self.repo
        good = r.write("docs/validation/M5/history/kept.json", "kept\n")
        living = r.write("docs/validation/M5/history/README.md", "prose\n")
        r.write("docs/validation/M5/history/present.json", "should be gone\n")
        r.write("docs/validation/M5/history/inventory.json", json.dumps({
            "retained": [
                {"path": "kept.json", "sha256": sha256(good.read_bytes()), "bytes": good.stat().st_size},
                {"path": "README.md", "sha256": "stale", "bytes": 0, "living": True},
                {"path": "missing.json", "sha256": "x", "bytes": 1},
                {"path": "kept.json", "sha256": "wrong", "bytes": good.stat().st_size},
            ],
            "retired": [
                {"original_path": "docs/validation/M5/history/present.json", "sha256": "x", "bytes": 1},
                {"original_path": "docs/validation/M5/history/gone.json", "sha256": "x", "bytes": 1},
            ]}))
        r.write("docs/research/m1-16/inventory.json", json.dumps({"retained": [], "retired": []}))
        r.write("docs/reviews/inventory.json", json.dumps({"retained": [], "retired": []}))
        r.commit()
        with mock.patch("builtins.print") as printed:
            self.assertEqual(DH.verify_inventories(), 1)
        messages = " ".join(str(call.args[0]) for call in printed.call_args_list)
        self.assertIn("MISSING", messages)
        self.assertIn("MISMATCH", messages)
        self.assertIn("PRESENT", messages)
        self.assertIn("3 inventories, 3 failures", messages)
        self.assertEqual(living.read_text(), "prose\n")

    def test_main_dispatches_subcommands(self) -> None:
        self.seed()
        with mock.patch("builtins.print"):
            self.assertEqual(DH.main(["links-check"]), 1)
            self.assertEqual(DH.main(["verify-inventories"]), 0)
            plan = self.repo.root / "plan.json"
            plan.write_text(json.dumps([{"from": "docs/tools.md", "to": "docs/guide/tools.md"}]))
            self.assertEqual(DH.main(["apply-moves", str(plan), "--dry-run"]), 0)
            self.assertEqual(DH.main(["apply-moves", str(plan)]), 0)
        self.assertIn("docs/guide/tools.md", self.repo.tracked())
        self.assertEqual((self.repo.root / "docs/validation/M5-matrix.md").read_text(),
                         "[core](M5-core-gate.json) [self](../guide/tools.md)\n")
        self.assertEqual(os.getcwd(), str(self.repo.root))

    def test_sha256_and_ignored_paths(self) -> None:
        self.seed()
        self.assertEqual(DH.sha256_of(self.repo.root / "docs/tools.md"), sha256(b"plain\n"))
        self.assertEqual(DH.ignored_paths(["docs/validation/run.log", "docs/tools.md"]), {"docs/validation/run.log"})
        self.assertEqual(DH.ignored_paths([]), set())
        self.assertIn("docs/tools.md", DH.tracked_files())
        self.repo.write("docs/new.md", "new\n")
        self.assertNotIn("docs/new.md", DH.tracked_files())
        self.assertIn("docs/new.md", DH.tracked_files(include_untracked=True))


if __name__ == "__main__":
    unittest.main()
