#!/usr/bin/env python3
"""Documentation hygiene for the canonical layout (post documentation-cleanup).

A single ``check`` command runs every rule below against the tracked Git tree
and prints one line per violation. Exit status is non-zero on any failure.
``--report`` additionally writes a JSON summary to
``target/docs-hygiene/check.json``. Only the standard library is used.

Rules:

  (a) Links and anchors -- every relative Markdown link and every same-file or
      cross-file ``#anchor`` fragment in root ``*.md``, ``docs/**/*.md`` and
      ``.planning/*.md``/``.planning/**/*.md`` must resolve. Anchors are
      matched against GitHub's heading-slug algorithm.
  (b) Canonical docs/ layout -- ``docs/`` may contain only ``README.md`` and
      the five canonical subdirectories (``guides``, ``reference``,
      ``architecture``, ``operations``, ``development``). Any other
      top-level entry, including a new folder or a milestone-shaped
      directory (``M0``..``M8``, a version number, ...), fails the check.
      There is no allowlist to grow: a new top-level docs/ entry is always a
      finding, never silently accepted.
  (c) No live references to retired paths -- no in-tree file other than this
      tool's own source may reference a path retired by the documentation
      cleanup (the milestone trees ``docs/{validation,reviews,research,
      roadmap,release,spec,adr,prompts}/`` or the superseded flat docs)
      except as a GitHub permalink pinned to the retirement commit, a frozen
      contract/schema string, or a citation that names the retirement commit
      inline (``at``/``en 51fa602e``).
  (d) Pending plans are tracked -- every ``.planning/*.md`` and
      ``.planning/**/*.md`` file on disk must be committed to Git, so it
      survives a clean export.
  (e) README navigation -- every ``docs/**/*.md`` other than ``docs/README.md``
      itself must be reachable from a relative link somewhere in
      ``docs/README.md``.
  (f) docs/ path strings resolve -- a bare ``docs/...`` path string inside
      ``scripts/**.py``, ``crates/**.rs`` or ``.github/**.yml`` that is not
      already a rule-(c) retired-path finding must resolve to a tracked file
      or directory, catching a stale reference to a *renamed* canonical doc.
      Scoped to ``docs/`` only (not ``crates/``/``scripts/``/``fixtures/``/
      ``tests/``): those other prefixes are also used for synthetic example
      paths in test fixtures and doc comments, which would make a broader
      version of this rule too noisy to keep passing honestly.

No subcommand moves or rewrites files: the historical ``apply-moves`` and
``verify-inventories`` operations existed only to execute the documentation
cleanup itself and have no function once the canonical layout is in place.
"""

from __future__ import annotations

import argparse
import json
import pathlib
import posixpath
import re
import subprocess
import sys
from collections.abc import Iterable

ROOT = pathlib.Path(__file__).resolve().parents[1]
SELF = pathlib.Path(__file__).name

# ---------------------------------------------------------------------------
# Canonical docs/ layout (rule b)
# ---------------------------------------------------------------------------

CANONICAL_DOCS_FILES = {"README.md"}
CANONICAL_DOCS_DIRS = {"guides", "reference", "architecture", "operations", "development"}

# ---------------------------------------------------------------------------
# Retired paths (rule c) -- the documentation-cleanup's own retire-default
# trees, plus the flat living docs it superseded.
# ---------------------------------------------------------------------------

RETIRED_PREFIXES = (
    "docs/validation/", "docs/reviews/", "docs/research/", "docs/roadmap/",
    "docs/release/", "docs/spec/", "docs/adr/", "docs/prompts/",
)
RETIRED_FLAT_FILES = (
    "docs/architecture.md", "docs/catalog-bundle-format.md", "docs/ci.md",
    "docs/client-configuration.md", "docs/compatibility.md",
    "docs/domain-contracts.md", "docs/implementation-status.md",
    "docs/m1-prerequisites.md", "docs/publication.md",
    "docs/security-model.md", "docs/tools.md",
)
RETIRED_TOKEN = re.compile(
    "(?:" + "|".join(re.escape(p) for p in RETIRED_PREFIXES) + r")(?!\w*<)"
    "|" + "|".join(re.escape(f) for f in RETIRED_FLAT_FILES)
)
COMMIT_MARK = "51fa602e"

# Files whose own source must name these strings to detect them, or whose
# retirement notice legitimately explains what moved from where.
RETIRED_TOKEN_SELF_EXEMPT_FILES = {
    f"scripts/{SELF}",
    "scripts/test-docs-hygiene.py",
    # Pure-function unit tests for build-m6-runtime.py's `beside_default`;
    # every occurrence is a synthetic example path argument, not a read of a
    # real file (verified manually during the documentation cleanup).
    "scripts/test-m6-provisioning.py",
}
# Exact (file, line-substring) pairs that are frozen contract text: a `///`
# doc comment on a JsonSchema-derived type, mirrored verbatim in a contract
# snapshot. Never edited by documentation hygiene.
RETIRED_TOKEN_FROZEN_SCHEMA_EXEMPT = (
    ("crates/mcp-server/src/stdio/bloat/schemas.rs", "docs/validation/M5/04-bloat-calibration.json"),
    ("crates/mcp-server/tests/snapshots/binary-bloat-tool.json", "docs/validation/M5/04-bloat-calibration.json"),
)
# A line is also exempt when it is itself the data value of a frozen
# provenance receipt field a script produces (D3: never rewritten to fake
# evidence over current bytes) -- these are constants, not stale citations.
RETIRED_TOKEN_JSON_KEY_EXEMPT = re.compile(
    r'"(decision|authority|source|image_admitted_by|authorization|'
    r'method_under_test|version_claim_source|protocol_revisions_credited_by)"\s*[:\]]'
)

RETIRED_TOKEN_SCAN_GLOBS = (
    "*.md",  # root-level, handled by extension + no "/" check below
    "docs/",
    ".planning/",
    "crates/",
    "scripts/",
    ".github/",
)

# ---------------------------------------------------------------------------
# Link/anchor scope (rule a) and doc-tree scope (rule e)
# ---------------------------------------------------------------------------

ROOT_MD_FILES = {"README.md", "CHANGELOG.md", "SECURITY.md", "CONTRIBUTING.md", "AGENTS.md"}

INLINE_LINK = re.compile(r"(!?\[[^\]]*\]\()(<[^>]*>|[^)\s]+)((?:\s+\"[^\"]*\")?\))")
REFERENCE_DEF = re.compile(r"^(\s{0,3}\[(?!\^)[^\]]+\]:\s*)(\S+)", re.MULTILINE)
SCHEME = re.compile(r"^[a-zA-Z][a-zA-Z0-9+.-]*:")
HEADING = re.compile(r"^(#{1,6})\s+(.*?)\s*#*$")
FENCE = re.compile(r"^(```|~~~)")


def git(*args: str) -> str:
    return subprocess.run(["git", *args], cwd=ROOT, check=True, capture_output=True, text=True).stdout


def tracked_files(include_untracked: bool = False) -> list[str]:
    args = ["ls-files", "-z"]
    if include_untracked:
        args += ["--cached", "--others", "--exclude-standard"]
    return [p for p in git(*args).split("\0") if p]


def is_planning_scope(path: str) -> bool:
    return path.startswith(".planning/")


def is_doc_scope(path: str) -> bool:
    """Files rule (a)'s link/anchor checker covers."""
    if not path.endswith(".md"):
        return False
    if "/" not in path:
        return path in ROOT_MD_FILES
    if path.startswith("docs/"):
        return True
    return is_planning_scope(path)


class Tree:
    def __init__(self, files: Iterable[str]):
        self.files = set(files)
        self.dirs: set[str] = set()
        for path in self.files:
            parts = path.split("/")
            for depth in range(1, len(parts)):
                self.dirs.add("/".join(parts[:depth]))

    def exists(self, path: str) -> bool:
        return path in self.files or path in self.dirs or path == "."


def split_target(raw: str) -> tuple[str, str, str]:
    prefix = suffix = ""
    target = raw
    if target.startswith("<") and target.endswith(">"):
        prefix, suffix, target = "<", ">", target[1:-1]
    if "#" in target:
        target, fragment = target.split("#", 1)
        suffix = "#" + fragment + suffix
    return prefix, target, suffix


def resolve(source: str, target: str) -> str | None:
    if not target or SCHEME.match(target) or target.startswith("//") or "%" in target:
        return None
    if target.startswith("/"):
        return posixpath.normpath(target.lstrip("/"))
    base = posixpath.dirname(source)
    return posixpath.normpath(posixpath.join(base, target)) if base else posixpath.normpath(target)


def iter_links(text: str) -> Iterable[tuple[re.Match, str]]:
    for match in INLINE_LINK.finditer(text):
        yield match, match.group(2)
    for match in REFERENCE_DEF.finditer(text):
        yield match, match.group(2)


def github_slug(heading: str, seen: dict[str, int]) -> str:
    # GitHub's slugger strips anything but word chars/hyphen/space, then
    # replaces each remaining space with a hyphen individually -- it does
    # NOT collapse consecutive spaces, so e.g. "`a` / `b`" (a space, the
    # slash removed, a space) slugs to "a--b", not "a-b".
    text = re.sub(r"[^\w\- ]+", "", heading.strip().lower(), flags=re.UNICODE)
    text = text.replace(" ", "-")
    count = seen.get(text, 0)
    seen[text] = count + 1
    return text if count == 0 else f"{text}-{count}"


def heading_slugs(text: str) -> set[str]:
    slugs: set[str] = set()
    seen: dict[str, int] = {}
    in_fence = False
    for line in text.splitlines():
        if FENCE.match(line.strip()):
            in_fence = not in_fence
            continue
        if in_fence:
            continue
        match = HEADING.match(line)
        if match:
            slugs.add(github_slug(match.group(2), seen))
    return slugs


def line_of(text: str, offset: int) -> int:
    return text.count("\n", 0, offset) + 1


# ---------------------------------------------------------------------------
# Rule (a): links and anchors
# ---------------------------------------------------------------------------


def check_links_and_anchors(tree: Tree, doc_files: list[str]) -> tuple[list[dict], int]:
    slug_cache: dict[str, set[str]] = {}

    def slugs_of(path: str) -> set[str] | None:
        if path not in slug_cache:
            full = ROOT / path
            if not full.is_file():
                return None
            slug_cache[path] = heading_slugs(full.read_text(encoding="utf-8", errors="surrogateescape"))
        return slug_cache[path]

    broken: list[dict] = []
    checked = 0
    for path in doc_files:
        text = (ROOT / path).read_text(encoding="utf-8", errors="surrogateescape")
        for match, raw in iter_links(text):
            _, target, fragment = split_target(raw)
            if not target and not fragment:
                continue
            checked += 1
            line = line_of(text, match.start())
            if not target:
                resolved_path = path
            else:
                resolved = resolve(path, target)
                if resolved is None:
                    continue
                if not tree.exists(resolved):
                    broken.append({"file": path, "line": line, "link": raw, "reason": "target does not exist"})
                    continue
                resolved_path = resolved
            if fragment:
                anchor = fragment[1:]
                slugs = slugs_of(resolved_path)
                if slugs is not None and anchor not in slugs:
                    broken.append({"file": path, "line": line, "link": raw,
                                   "reason": f"anchor #{anchor} not found in {resolved_path}"})
    return broken, checked


# ---------------------------------------------------------------------------
# Rule (b): canonical docs/ layout
# ---------------------------------------------------------------------------


def check_docs_layout(files: list[str]) -> list[str]:
    top_level: set[str] = set()
    for path in files:
        if not path.startswith("docs/"):
            continue
        rest = path[len("docs/"):]
        top_level.add(rest.split("/", 1)[0])
    violations = []
    for entry in sorted(top_level):
        if entry in CANONICAL_DOCS_FILES:
            continue
        if entry in CANONICAL_DOCS_DIRS:
            continue
        violations.append(f"docs/{entry} is not part of the canonical layout "
                           f"(README.md + {sorted(CANONICAL_DOCS_DIRS)})")
    return violations


# ---------------------------------------------------------------------------
# Rule (c): no live references to retired paths
# ---------------------------------------------------------------------------


def in_retired_scan_scope(path: str) -> bool:
    if path in RETIRED_TOKEN_SELF_EXEMPT_FILES:
        return False
    if path == "NOTICE" or ("/" not in path and path.endswith(".md")):
        return True
    if path.startswith("docs/"):
        return True
    if is_planning_scope(path):
        return True
    if path.startswith("crates/") and path.endswith((".rs", ".md")):
        return True
    if path.startswith("scripts/") and path.endswith((".py", ".mjs", ".sh")):
        return True
    if path.startswith("fixtures/") and path.endswith("README.md"):
        return True
    if path.startswith(".github/"):
        return True
    if path in {"sonar-project.properties", ".gitignore"}:
        return True
    return False


def check_retired_references(files: list[str]) -> list[dict]:
    violations = []
    for path in files:
        if not in_retired_scan_scope(path):
            continue
        full = ROOT / path
        if not full.is_file():
            continue
        text = full.read_text(encoding="utf-8", errors="surrogateescape")
        lines = text.splitlines()
        for match in RETIRED_TOKEN.finditer(text):
            line_no = line_of(text, match.start())
            window_start = max(0, line_no - 5)
            window = "\n".join(lines[window_start:line_no + 2])
            if COMMIT_MARK in window:
                continue
            line_text = lines[line_no - 1] if line_no - 1 < len(lines) else ""
            if RETIRED_TOKEN_JSON_KEY_EXEMPT.search(line_text):
                continue
            if any(path == f and s in line_text for f, s in RETIRED_TOKEN_FROZEN_SCHEMA_EXEMPT):
                continue
            if "OLD_LICENSES_PREFIX" in line_text:
                continue
            violations.append({"file": path, "line": line_no, "text": line_text.strip()[:160]})
    return violations


# ---------------------------------------------------------------------------
# Rule (d): pending plans are tracked
# ---------------------------------------------------------------------------


def check_planning_tracked(tracked: list[str]) -> list[str]:
    tracked_set = set(tracked)
    violations = []
    base = ROOT / ".planning"
    if not base.is_dir():
        return violations
    for candidate in sorted(base.rglob("*.md")):
        rel = candidate.relative_to(ROOT).as_posix()
        if not is_planning_scope(rel):
            continue
        if rel not in tracked_set:
            violations.append(rel)
    return violations


# ---------------------------------------------------------------------------
# Rule (e): README navigation
# ---------------------------------------------------------------------------


def check_readme_navigation(files: list[str]) -> list[str]:
    readme = "docs/README.md"
    if readme not in files:
        return ["docs/README.md is missing"]
    text = (ROOT / readme).read_text(encoding="utf-8", errors="surrogateescape")
    linked: set[str] = set()
    for _, raw in iter_links(text):
        _, target, _ = split_target(raw)
        resolved = resolve(readme, target)
        if resolved:
            linked.add(resolved)
    missing = []
    for path in files:
        if path.startswith("docs/") and path.endswith(".md") and path != readme:
            if path not in linked:
                missing.append(path)
    return sorted(missing)


# ---------------------------------------------------------------------------
# Rule (f): bare docs/ path strings resolve
# ---------------------------------------------------------------------------

DOCS_PATH_STRING = re.compile(r'(?<![A-Za-z0-9_./-])docs/[A-Za-z0-9_./-]+')


def check_docs_path_strings(files: list[str], tree: Tree) -> list[dict]:
    violations = []
    for path in files:
        if not ((path.startswith("scripts/") and path.endswith(".py"))
                or (path.startswith("crates/") and path.endswith(".rs"))
                or (path.startswith(".github/") and path.endswith((".yml", ".yaml")))):
            continue
        if path in RETIRED_TOKEN_SELF_EXEMPT_FILES or path == f"scripts/{SELF}":
            continue
        full = ROOT / path
        if not full.is_file():
            continue
        text = full.read_text(encoding="utf-8", errors="surrogateescape")
        for match in DOCS_PATH_STRING.finditer(text):
            candidate = match.group(0).rstrip(".,;:)\"'")
            if candidate.startswith(RETIRED_PREFIXES) or candidate in RETIRED_FLAT_FILES:
                continue  # rule (c) already reports this
            if candidate == "docs/.cargo-config.toml":
                continue  # synthetic negative-fixture filename in a unit test, never a real path
            if tree.exists(candidate):
                continue
            line_no = line_of(text, match.start())
            violations.append({"file": path, "line": line_no, "path": candidate})
    return violations


# ---------------------------------------------------------------------------
# Orchestration
# ---------------------------------------------------------------------------


def report_path() -> pathlib.Path:
    directory = ROOT / "target" / "docs-hygiene"
    directory.mkdir(parents=True, exist_ok=True)
    return directory / "check.json"


def run_check(write_report: bool) -> int:
    tracked = tracked_files()
    tree = Tree(tracked)
    doc_files = [p for p in tracked if is_doc_scope(p)]

    broken_links, links_checked = check_links_and_anchors(tree, doc_files)
    layout_violations = check_docs_layout(tracked)
    retired_violations = check_retired_references(tracked)
    untracked_plans = check_planning_tracked(tracked)
    unreachable_docs = check_readme_navigation(tracked)
    docs_path_strings = check_docs_path_strings(tracked, tree)

    summary = {
        "links_checked": links_checked,
        "broken_links": broken_links,
        "docs_layout_violations": layout_violations,
        "retired_references": retired_violations,
        "untracked_planning_files": untracked_plans,
        "docs_not_linked_from_readme": unreachable_docs,
        "docs_path_string_violations": docs_path_strings,
    }
    total = (len(broken_links) + len(layout_violations) + len(retired_violations)
             + len(untracked_plans) + len(unreachable_docs) + len(docs_path_strings))

    print(f"docs-hygiene: {links_checked} links/anchors checked, {total} violations")
    for row in broken_links:
        print(f"  BROKEN LINK {row['file']}:{row['line']} -> {row['link']} ({row['reason']})")
    for row in layout_violations:
        print(f"  LAYOUT {row}")
    for row in retired_violations:
        print(f"  RETIRED REFERENCE {row['file']}:{row['line']}: {row['text']}")
    for row in untracked_plans:
        print(f"  UNTRACKED PLAN {row}")
    for row in unreachable_docs:
        print(f"  UNREACHABLE FROM docs/README.md: {row}")
    for row in docs_path_strings:
        print(f"  STALE docs/ PATH STRING {row['file']}:{row['line']}: {row['path']}")

    if write_report:
        report_path().write_text(json.dumps(summary, indent=2) + "\n")
    return 1 if total else 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    check = sub.add_parser("check", help="run every hygiene rule against the tracked tree")
    check.add_argument("--report", action="store_true", help="write target/docs-hygiene/check.json")
    args = parser.parse_args(argv)
    if args.command == "check":
        return run_check(args.report)
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
