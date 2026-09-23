#!/usr/bin/env python3
"""Portable tests for scripts/docs-hygiene.py on a temporary Git repository."""
from __future__ import annotations

import importlib.util
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


class ScopeClassification(unittest.TestCase):
    def test_is_doc_scope(self) -> None:
        self.assertTrue(DH.is_doc_scope("README.md"))
        self.assertTrue(DH.is_doc_scope("CHANGELOG.md"))
        self.assertTrue(DH.is_doc_scope("docs/reference/tools.md"))
        self.assertTrue(DH.is_doc_scope(".planning/deferred-commitments.md"))
        self.assertTrue(DH.is_doc_scope(".planning/nested/plan.md"))
        self.assertFalse(DH.is_doc_scope("crates/domain/README.md"))
        self.assertFalse(DH.is_doc_scope("NOTICE"))
        self.assertTrue(DH.is_doc_scope("AGENTS.md"))  # now checked, see docs-hygiene.py

    def test_is_planning_scope(self) -> None:
        self.assertTrue(DH.is_planning_scope(".planning/x.md"))
        self.assertTrue(DH.is_planning_scope(".planning/nested/x.md"))
        self.assertFalse(DH.is_planning_scope("docs/x.md"))

    def test_in_retired_scan_scope(self) -> None:
        self.assertTrue(DH.in_retired_scan_scope("README.md"))
        self.assertTrue(DH.in_retired_scan_scope("NOTICE"))
        self.assertTrue(DH.in_retired_scan_scope("docs/reference/tools.md"))
        self.assertTrue(DH.in_retired_scan_scope(".planning/deferred-commitments.md"))
        self.assertTrue(DH.in_retired_scan_scope("crates/domain/src/lib.rs"))
        self.assertTrue(DH.in_retired_scan_scope("scripts/gate.py"))
        self.assertTrue(DH.in_retired_scan_scope("fixtures/benchmark/README.md"))
        self.assertTrue(DH.in_retired_scan_scope(".github/workflows/ci.yml"))
        self.assertTrue(DH.in_retired_scan_scope("sonar-project.properties"))
        self.assertTrue(DH.in_retired_scan_scope(".gitignore"))
        self.assertTrue(DH.in_retired_scan_scope("AGENTS.md"))  # now scanned, see docs-hygiene.py
        self.assertFalse(DH.in_retired_scan_scope("scripts/docs-hygiene.py"))
        self.assertFalse(DH.in_retired_scan_scope("scripts/test-docs-hygiene.py"))
        self.assertTrue(DH.in_retired_scan_scope(".planning/anything.md"))
        self.assertFalse(DH.in_retired_scan_scope("fixtures/other/notes.md"))
        self.assertFalse(DH.in_retired_scan_scope("crates/domain/src/lib.rs.orig"))


class LinkParsing(unittest.TestCase):
    def test_split_target(self) -> None:
        self.assertEqual(DH.split_target("<a b.md#x>"), ("<", "a b.md", "#x>"))
        self.assertEqual(DH.split_target("a.md#frag"), ("", "a.md", "#frag"))
        self.assertEqual(DH.split_target("a.md"), ("", "a.md", ""))

    def test_resolve(self) -> None:
        self.assertIsNone(DH.resolve("docs/a.md", "https://example.invalid/x"))
        self.assertIsNone(DH.resolve("docs/a.md", "mailto:x@y"))
        self.assertIsNone(DH.resolve("docs/a.md", ""))
        self.assertIsNone(DH.resolve("docs/a.md", "a%20b.md"))
        self.assertEqual(DH.resolve("docs/a.md", "../README.md"), "README.md")
        self.assertEqual(DH.resolve("README.md", "docs/x.md"), "docs/x.md")
        self.assertEqual(DH.resolve("docs/a.md", "/docs/x.md"), "docs/x.md")

    def test_iter_links(self) -> None:
        text = "[a](x.md) ![i](img.png)\n[ref]: y.md\n[^note]: Qualified text\n"
        links = [raw for _, raw in DH.iter_links(text)]
        self.assertEqual(links, ["x.md", "img.png", "y.md"])


class Tree(unittest.TestCase):
    def test_exists(self) -> None:
        tree = DH.Tree(["docs/a/b.md", "docs/c.md"])
        self.assertTrue(tree.exists("docs/a"))
        self.assertTrue(tree.exists("docs/a/b.md"))
        self.assertTrue(tree.exists("."))
        self.assertFalse(tree.exists("docs/zz"))


class Slugs(unittest.TestCase):
    def test_github_slug_basic(self) -> None:
        seen: dict[str, int] = {}
        self.assertEqual(DH.github_slug("Overview", seen), "overview")
        self.assertEqual(DH.github_slug("ADR-050: `local_coordinated`", seen), "adr-050-local_coordinated")

    def test_github_slug_does_not_collapse_double_hyphen(self) -> None:
        seen: dict[str, int] = {}
        slug = DH.github_slug("`rust.benchmark.run` / `rust.benchmark.compare`", seen)
        self.assertEqual(slug, "rustbenchmarkrun--rustbenchmarkcompare")

    def test_github_slug_dedupes_repeated_headings(self) -> None:
        seen: dict[str, int] = {}
        first = DH.github_slug("Estado actual", seen)
        second = DH.github_slug("Estado actual", seen)
        self.assertEqual(first, "estado-actual")
        self.assertEqual(second, "estado-actual-1")

    def test_heading_slugs_skips_fenced_code(self) -> None:
        text = "# Title\n\n```\n# not a heading\n```\n\n## Real heading\n"
        self.assertEqual(DH.heading_slugs(text), {"title", "real-heading"})


class LinksAndAnchorsRule(unittest.TestCase):
    """Rule (a): every relative link and every #anchor must resolve."""

    def setUp(self) -> None:
        self.repo = Repo()
        self.addCleanup(self.repo.close)
        patcher = mock.patch.object(DH, "ROOT", self.repo.root)
        patcher.start()
        self.addCleanup(patcher.stop)

    def test_positive_all_links_and_anchors_resolve(self) -> None:
        self.repo.write("README.md", "[docs](docs/README.md)\n")
        self.repo.write("docs/README.md", "# Index\n\n[guide](guides/x.md#a-heading)\n")
        self.repo.write("docs/guides/x.md", "# A heading\n")
        self.repo.commit()
        tree = DH.Tree(self.repo.tracked())
        doc_files = [p for p in self.repo.tracked() if DH.is_doc_scope(p)]
        broken, checked = DH.check_links_and_anchors(tree, doc_files)
        self.assertEqual(broken, [])
        self.assertGreaterEqual(checked, 2)

    def test_negative_broken_path_and_broken_anchor(self) -> None:
        self.repo.write("docs/README.md", "# Index\n\n[missing](guides/nope.md) [bad-anchor](guides/x.md#nope)\n")
        self.repo.write("docs/guides/x.md", "# Real heading\n")
        self.repo.commit()
        tree = DH.Tree(self.repo.tracked())
        doc_files = [p for p in self.repo.tracked() if DH.is_doc_scope(p)]
        broken, _ = DH.check_links_and_anchors(tree, doc_files)
        reasons = {row["reason"] for row in broken}
        self.assertIn("target does not exist", reasons)
        self.assertTrue(any("anchor" in r for r in reasons))

    def test_negative_same_file_anchor_must_exist(self) -> None:
        self.repo.write("docs/README.md", "# Index\n\n[self](#not-a-real-heading)\n")
        self.repo.commit()
        tree = DH.Tree(self.repo.tracked())
        doc_files = [p for p in self.repo.tracked() if DH.is_doc_scope(p)]
        broken, _ = DH.check_links_and_anchors(tree, doc_files)
        self.assertEqual(len(broken), 1)
        self.assertIn("not-a-real-heading", broken[0]["reason"])


class DocsLayoutRule(unittest.TestCase):
    """Rule (b): docs/ holds only README.md + the five canonical subdirs."""

    def test_positive_canonical_layout_passes(self) -> None:
        files = ["docs/README.md", "docs/guides/a.md", "docs/reference/b.md",
                 "docs/architecture/c.md", "docs/operations/d.md", "docs/development/e.md"]
        self.assertEqual(DH.check_docs_layout(files), [])

    def test_negative_new_top_level_folder_fails(self) -> None:
        files = ["docs/README.md", "docs/notes/new.md"]
        violations = DH.check_docs_layout(files)
        self.assertEqual(len(violations), 1)
        self.assertIn("docs/notes", violations[0])

    def test_negative_milestone_shaped_folder_fails(self) -> None:
        files = ["docs/README.md", "docs/M9/x.md"]
        violations = DH.check_docs_layout(files)
        self.assertEqual(len(violations), 1)
        self.assertIn("docs/M9", violations[0])


class RetiredReferencesRule(unittest.TestCase):
    """Rule (c): no live reference to a retired path outside a permalink/citation."""

    def setUp(self) -> None:
        self.repo = Repo()
        self.addCleanup(self.repo.close)
        patcher = mock.patch.object(DH, "ROOT", self.repo.root)
        patcher.start()
        self.addCleanup(patcher.stop)

    def test_positive_permalink_and_dated_citation_are_clean(self) -> None:
        self.repo.write("docs/architecture/overview.md", (
            "See [ADR-001](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-001.md) "
            "and the historical receipt docs/validation/M1/x.json at 51fa602e.\n"
        ))
        self.repo.commit()
        self.assertEqual(DH.check_retired_references(self.repo.tracked()), [])

    def test_negative_bare_reference_without_commit_mark_fails(self) -> None:
        self.repo.write("docs/architecture/overview.md", "See docs/validation/M1/x.json for detail.\n")
        self.repo.commit()
        violations = DH.check_retired_references(self.repo.tracked())
        self.assertEqual(len(violations), 1)
        self.assertEqual(violations[0]["file"], "docs/architecture/overview.md")

    def test_negative_flat_retired_file_without_commit_mark_fails(self) -> None:
        self.repo.write("SECURITY.md", "See docs/security-model.md for the threat model.\n")
        self.repo.commit()
        violations = DH.check_retired_references(self.repo.tracked())
        self.assertEqual(len(violations), 1)

    def test_positive_json_provenance_value_is_exempt(self) -> None:
        self.repo.write("scripts/build-x.py", '    receipt["decision"] = "docs/adr/ADR-075-x.md"\n')
        self.repo.commit()
        self.assertEqual(DH.check_retired_references(self.repo.tracked()), [])

    def test_positive_template_placeholder_is_exempt(self) -> None:
        self.repo.write("CHANGELOG.md", "Historically organized under `docs/validation/M<n>/`.\n")
        self.repo.commit()
        self.assertEqual(DH.check_retired_references(self.repo.tracked()), [])

    def test_positive_own_source_is_exempt(self) -> None:
        self.repo.write("scripts/docs-hygiene.py", "RETIRED_EXAMPLE = 'docs/validation/M1/x.json'\n")
        self.repo.commit()
        self.assertEqual(DH.check_retired_references(self.repo.tracked()), [])

    def test_negative_agents_md_is_no_longer_exempt(self) -> None:
        self.repo.write("AGENTS.md", "See docs/validation/M1/x.json.\n")
        self.repo.commit()
        violations = DH.check_retired_references(self.repo.tracked())
        self.assertEqual(len(violations), 1)

    def test_negative_out_of_scope_file_type_is_not_scanned(self) -> None:
        self.repo.write("crates/domain/tests/data.txt", "docs/validation/M1/x.json\n")
        self.repo.commit()
        self.assertEqual(DH.check_retired_references(self.repo.tracked()), [])


class PlanningTrackedRule(unittest.TestCase):
    """Rule (d): every .planning/*.md, at any nesting depth, is tracked."""

    def setUp(self) -> None:
        self.repo = Repo()
        self.addCleanup(self.repo.close)
        patcher = mock.patch.object(DH, "ROOT", self.repo.root)
        patcher.start()
        self.addCleanup(patcher.stop)

    def test_positive_tracked_plan_passes(self) -> None:
        self.repo.write(".planning/plan.md", "pending\n")
        self.repo.commit()
        self.assertEqual(DH.check_planning_tracked(self.repo.tracked()), [])

    def test_negative_untracked_plan_fails(self) -> None:
        self.repo.write(".planning/other.md", "seed\n")
        self.repo.commit()
        self.repo.write(".planning/untracked-plan.md", "pending, never git add-ed\n")
        self.assertEqual(DH.check_planning_tracked(self.repo.tracked()), [".planning/untracked-plan.md"])

    def test_negative_untracked_file_in_nested_planning_dir_fails(self) -> None:
        self.repo.write(".planning/other.md", "seed\n")
        self.repo.commit()
        self.repo.write(".planning/scratch/notes.md", "not committed\n")
        self.assertEqual(DH.check_planning_tracked(self.repo.tracked()),
                          [".planning/scratch/notes.md"])


class ReadmeNavigationRule(unittest.TestCase):
    """Rule (e): every docs/**/*.md is linked from docs/README.md."""

    def test_positive_every_doc_linked(self) -> None:
        files = ["docs/README.md", "docs/guides/a.md", "docs/reference/b.md"]
        with mock.patch.object(DH.pathlib.Path, "read_text",
                                return_value="[a](guides/a.md) [b](reference/b.md)\n"), \
             mock.patch.object(DH, "ROOT", pathlib.Path("/fake")):
            self.assertEqual(DH.check_readme_navigation(files), [])

    def test_negative_missing_link_reported(self) -> None:
        files = ["docs/README.md", "docs/guides/a.md", "docs/reference/orphan.md"]
        with mock.patch.object(DH.pathlib.Path, "read_text", return_value="[a](guides/a.md)\n"), \
             mock.patch.object(DH, "ROOT", pathlib.Path("/fake")):
            self.assertEqual(DH.check_readme_navigation(files), ["docs/reference/orphan.md"])

    def test_negative_missing_readme_reported(self) -> None:
        self.assertEqual(DH.check_readme_navigation(["docs/guides/a.md"]),
                          ["docs/README.md is missing"])


class DocsPathStringRule(unittest.TestCase):
    """Rule (f): a bare docs/ path string in code resolves to a tracked path."""

    def setUp(self) -> None:
        self.repo = Repo()
        self.addCleanup(self.repo.close)
        patcher = mock.patch.object(DH, "ROOT", self.repo.root)
        patcher.start()
        self.addCleanup(patcher.stop)

    def test_positive_existing_path_passes(self) -> None:
        self.repo.write("docs/reference/tools.md", "content\n")
        self.repo.write("scripts/tool.py", 'PATH = ROOT / "docs/reference/tools.md"\n')
        self.repo.commit()
        tree = DH.Tree(self.repo.tracked())
        self.assertEqual(DH.check_docs_path_strings(self.repo.tracked(), tree), [])

    def test_negative_stale_rename_reported(self) -> None:
        self.repo.write("scripts/tool.py", 'PATH = ROOT / "docs/reference/renamed-away.md"\n')
        self.repo.commit()
        tree = DH.Tree(self.repo.tracked())
        violations = DH.check_docs_path_strings(self.repo.tracked(), tree)
        self.assertEqual(len(violations), 1)
        self.assertEqual(violations[0]["path"], "docs/reference/renamed-away.md")

    def test_positive_retired_prefix_not_double_reported(self) -> None:
        self.repo.write("scripts/tool.py", 'PATH = ROOT / "docs/validation/M1/x.json"\n')
        self.repo.commit()
        tree = DH.Tree(self.repo.tracked())
        # rule (c) reports this path; rule (f) must not duplicate it.
        self.assertEqual(DH.check_docs_path_strings(self.repo.tracked(), tree), [])

    def test_positive_synthetic_cargo_config_fixture_is_exempt(self) -> None:
        self.repo.write("crates/domain/src/security.rs", '"docs/.cargo-config.toml",\n')
        self.repo.commit()
        tree = DH.Tree(self.repo.tracked())
        self.assertEqual(DH.check_docs_path_strings(self.repo.tracked(), tree), [])


class EndToEndCheck(unittest.TestCase):
    def setUp(self) -> None:
        self.repo = Repo()
        self.addCleanup(self.repo.close)
        patcher = mock.patch.object(DH, "ROOT", self.repo.root)
        patcher.start()
        self.addCleanup(patcher.stop)

    def test_clean_tree_passes_and_writes_report(self) -> None:
        r = self.repo
        r.write("README.md", "# Product\n")
        r.write("docs/README.md", "# Index\n\n[tools](reference/tools.md)\n")
        r.write("docs/reference/tools.md", "# Tools\n")
        r.commit()
        with mock.patch("builtins.print"):
            code = DH.run_check(True)
        self.assertEqual(code, 0)
        report = (self.repo.root / "target/docs-hygiene/check.json")
        self.assertTrue(report.is_file())

    def test_dirty_tree_fails_with_every_kind_of_violation(self) -> None:
        r = self.repo
        r.write("README.md", "# Product\n")
        r.write("docs/README.md", "# Index\n")  # missing link to tools.md -> rule (e)
        r.write("docs/reference/tools.md", "# Tools\n\n[gone](../nope.md)\n")  # rule (a)
        r.write("docs/legacy/old.md", "docs/validation/M1/x.json\n")  # rule (b) + rule (c)
        r.write(".planning/tracked.md", "seed\n")
        r.commit()
        r.write(".planning/orphan.md", "never committed\n")  # rule (d)
        with mock.patch("builtins.print"):
            code = DH.run_check(False)
        self.assertEqual(code, 1)

    def test_main_check_subcommand(self) -> None:
        r = self.repo
        r.write("README.md", "# Product\n")
        r.write("docs/README.md", "# Index\n")
        r.commit()
        with mock.patch("builtins.print"):
            self.assertEqual(DH.main(["check"]), 0)


if __name__ == "__main__":
    unittest.main()
