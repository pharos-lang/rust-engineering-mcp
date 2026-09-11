#!/usr/bin/env python3
"""Repository documentation hygiene.

Three read-mostly operations used by the evidence layout convention described in
docs/validation/README.md:

  links-check [--report PATH]
      Resolve every Markdown link and every ``docs/...`` path string in the
      living documentation set. Broken links in living documents fail the
      command; broken links in frozen records (agent transcripts, review input
      snapshots) are only reported, because those bytes are never edited.

  apply-moves PLAN [--dry-run] [--report PATH]
      Move files with ``git mv`` following a JSON plan (a list of
      ``{"from": ..., "to": ...}`` entries; ``from`` may be a tracked file or a
      tracked directory), then rewrite the affected Markdown links and
      root-relative path strings in the living set, scripts and Git metadata.
      Receipts and other moved bytes are never edited.

  verify-inventories
      Verify every ``inventory.json`` under ``docs/validation/M*/history`` and
      ``docs/research/**``: retained entries must exist with the recorded
      SHA-256 and byte count; retired entries must be absent from the tree.

Only the standard library is used. Exit status is non-zero on any failure.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import posixpath
import re
import subprocess
import sys
from collections.abc import Iterable

ROOT = pathlib.Path(__file__).resolve().parents[1]

# Living documents: navigable prose whose links must resolve and may be
# rewritten when evidence moves. Everything else that is Markdown is frozen:
# reviewer/agent output and review input snapshots keep their bytes.
LIVING_ROOT_FILES = {"README.md", "CHANGELOG.md", "SECURITY.md", "AGENTS.md", "CONTRIBUTING.md"}
LIVING_PREFIXES = (
    "docs/adr/",
    "docs/roadmap/",
    "docs/prompts/",
    "docs/spec/",
    "docs/release/",
    "docs/research/",
    "docs/validation/",
    "docs/reviews/",
)
FROZEN_PATTERNS = (
    re.compile(r"^docs/reviews/[^/]+/inputs/"),
    re.compile(r"^docs/reviews/[^/]+/prompt\.md$"),
    re.compile(r"^docs/validation/(M3/delegation|m3-delegation)/"),
    re.compile(r"^docs/research/m1-16/corpus/selection/sources/"),
    re.compile(r"^docs/research/m1-16/measurement/results/"),
)
# Files that carry root-relative path strings outside Markdown links.
PATH_STRING_EXTRA = ("scripts/", "docs/release/reproduction/", ".github/")
PATH_STRING_ROOT_FILES = {".gitattributes", ".gitignore"}
PATH_STRING_PREFIXES = (
    "docs/validation/",
    "docs/reviews/",
    "docs/prompts/",
    "docs/release/",
    "docs/research/",
    "PUBLICATION-SNAPSHOT.json",
)

INLINE_LINK = re.compile(r"(!?\[[^\]]*\]\()(<[^>]*>|[^)\s]+)((?:\s+\"[^\"]*\")?\))")
REFERENCE_DEF = re.compile(r"^(\s{0,3}\[(?!\^)[^\]]+\]:\s*)(\S+)", re.MULTILINE)
SCHEME = re.compile(r"^[a-zA-Z][a-zA-Z0-9+.-]*:")


def git(*args: str) -> str:
    return subprocess.run(["git", *args], cwd=ROOT, check=True, capture_output=True, text=True).stdout


def tracked_files() -> list[str]:
    return [p for p in git("ls-files", "-z").split("\0") if p]


def is_markdown(path: str) -> bool:
    return path.endswith(".md")


def is_frozen(path: str) -> bool:
    return any(p.search(path) for p in FROZEN_PATTERNS)


def is_living(path: str) -> bool:
    if not is_markdown(path) or is_frozen(path):
        return False
    if "/" not in path:
        return path in LIVING_ROOT_FILES
    if not path.startswith("docs/"):
        return False
    return path.count("/") == 1 or path.startswith(LIVING_PREFIXES)


def carries_path_strings(path: str) -> bool:
    if is_living(path):
        return True
    if path in PATH_STRING_ROOT_FILES:
        return True
    return path.startswith(PATH_STRING_EXTRA) and path.endswith((".py", ".yml", ".yaml", ".sh", ".mjs"))


def sha256_of(path: pathlib.Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def split_target(raw: str) -> tuple[str, str, str]:
    """Return (prefix, path, suffix) where prefix/suffix hold ``<``/``>`` and ``#fragment``."""
    prefix = suffix = ""
    target = raw
    if target.startswith("<") and target.endswith(">"):
        prefix, suffix, target = "<", ">", target[1:-1]
    if "#" in target:
        target, fragment = target.split("#", 1)
        suffix = "#" + fragment + suffix
    return prefix, target, suffix


def resolve(source: str, target: str) -> str | None:
    """Resolve a link target against the directory of ``source`` (repo-relative)."""
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


class Tree:
    """Snapshot of the tracked tree plus on-disk directories for resolution."""

    def __init__(self, files: Iterable[str]):
        self.files = set(files)
        self.dirs: set[str] = set()
        for path in self.files:
            parts = path.split("/")
            for depth in range(1, len(parts)):
                self.dirs.add("/".join(parts[:depth]))

    def exists(self, path: str) -> bool:
        return path in self.files or path in self.dirs or path == "."


def ignored_paths(paths: Iterable[str]) -> set[str]:
    """Return the subset of ``paths`` that .gitignore excludes on purpose."""
    candidates = sorted(set(paths))
    if not candidates:
        return set()
    result = subprocess.run(["git", "check-ignore", "--stdin", "-z"], cwd=ROOT, input="\0".join(candidates),
                            capture_output=True, text=True)
    return {p for p in result.stdout.split("\0") if p}


def check_links(report_path: pathlib.Path | None) -> int:
    files = tracked_files()
    tree = Tree(files)
    broken_living: list[dict] = []
    broken_frozen: list[dict] = []
    checked = 0
    for path in files:
        if not is_markdown(path):
            continue
        living = is_living(path)
        if not living and not (path.startswith("docs/") or "/" not in path):
            continue
        text = (ROOT / path).read_text(encoding="utf-8", errors="surrogateescape")
        for match, raw in iter_links(text):
            _, target, _ = split_target(raw)
            resolved = resolve(path, target)
            if resolved is None:
                continue
            checked += 1
            if resolved.startswith("../") or not tree.exists(resolved):
                row = {"file": path, "link": raw, "resolved": resolved,
                       "line": text.count("\n", 0, match.start()) + 1}
                (broken_living if living else broken_frozen).append(row)
    excluded = ignored_paths(r["resolved"] for r in broken_living + broken_frozen)
    excluded_rows = [r for r in broken_living if r["resolved"] in excluded]
    broken_living = [r for r in broken_living if r["resolved"] not in excluded]
    broken_frozen = [r for r in broken_frozen if r["resolved"] not in excluded]
    summary = {"checked": checked, "broken_living": broken_living, "broken_frozen": broken_frozen,
               "excluded_evidence": excluded_rows}
    if report_path:
        report_path.write_text(json.dumps(summary, indent=2) + "\n")
    print(f"links-check: {checked} links resolved; "
          f"{len(broken_living)} broken in living documents; "
          f"{len(excluded_rows)} point at evidence excluded by .gitignore; "
          f"{len(broken_frozen)} broken in frozen records")
    for row in broken_living:
        print(f"  BROKEN {row['file']}:{row['line']} -> {row['link']}")
    return 1 if broken_living else 0


def expand_plan(plan: list[dict], files: list[str]) -> tuple[dict[str, str], dict[str, str]]:
    """Return (file moves old->new, directory moves old->new)."""
    tracked = set(files)
    file_moves: dict[str, str] = {}
    dir_moves: dict[str, str] = {}
    for entry in plan:
        src, dst = entry["from"].rstrip("/"), entry["to"].rstrip("/")
        if src in tracked:
            file_moves[src] = dst
            continue
        members = [f for f in files if f.startswith(src + "/")]
        if not members:
            raise SystemExit(f"plan entry not tracked: {src}")
        dir_moves[src] = dst
        for member in members:
            file_moves[member] = dst + member[len(src):]
    destinations = list(file_moves.values())
    if len(set(destinations)) != len(destinations):
        dupes = sorted({d for d in destinations if destinations.count(d) > 1})
        raise SystemExit(f"plan destinations collide: {dupes[:5]}")
    clashes = sorted(d for d in destinations if d in tracked and d not in file_moves)
    if clashes:
        raise SystemExit(f"plan destinations already tracked: {clashes[:5]}")
    return file_moves, dir_moves


def perform_moves(file_moves: dict[str, str], dir_moves: dict[str, str], dry_run: bool) -> None:
    moved: set[str] = set()
    for src, dst in dir_moves.items():
        members = [f for f in file_moves if f.startswith(src + "/")]
        whole = all(file_moves[f] == dst + f[len(src):] for f in members)
        if whole and not (ROOT / dst).exists():
            if not dry_run:
                (ROOT / dst).parent.mkdir(parents=True, exist_ok=True)
                subprocess.run(["git", "mv", "-k", src, dst], cwd=ROOT, check=True)
            moved.update(members)
    for src, dst in file_moves.items():
        if src in moved:
            continue
        if not dry_run:
            (ROOT / dst).parent.mkdir(parents=True, exist_ok=True)
            subprocess.run(["git", "mv", "-k", src, dst], cwd=ROOT, check=True)


def map_path(path: str, file_moves: dict[str, str], dir_moves: dict[str, str]) -> str:
    if path in file_moves:
        return file_moves[path]
    best = None
    for src in dir_moves:
        if path == src or path.startswith(src + "/"):
            if best is None or len(src) > len(best):
                best = src
    if best is not None:
        return dir_moves[best] + path[len(best):]
    return path


def relative_link(source_new: str, target_new: str) -> str:
    base = posixpath.dirname(source_new)
    rel = posixpath.relpath(target_new, base) if base else target_new
    return rel


def rewrite_links(path_new: str, path_old: str, text: str, old_tree: Tree,
                  file_moves: dict[str, str], dir_moves: dict[str, str],
                  rewrites: list[dict]) -> str:
    def replace_inline(match: re.Match) -> str:
        raw = match.group(2)
        new_raw = rewrite_target(raw)
        return match.group(1) + new_raw + match.group(3)

    def replace_reference(match: re.Match) -> str:
        return match.group(1) + rewrite_target(match.group(2))

    def rewrite_target(raw: str) -> str:
        prefix, target, suffix = split_target(raw)
        resolved = resolve(path_old, target)
        if resolved is None or not old_tree.exists(resolved):
            return raw
        mapped = map_path(resolved, file_moves, dir_moves)
        if mapped == resolved and path_new == path_old:
            return raw
        new_target = relative_link(path_new, mapped)
        if target.endswith("/") and not new_target.endswith("/"):
            new_target += "/"
        if new_target == target:
            return raw
        rewrites.append({"file": path_new, "old": raw, "new": prefix + new_target + suffix})
        return prefix + new_target + suffix

    text = INLINE_LINK.sub(replace_inline, text)
    text = REFERENCE_DEF.sub(replace_reference, text)
    return text


def rewrite_path_strings(path: str, text: str, file_moves: dict[str, str],
                         dir_moves: dict[str, str], rewrites: list[dict]) -> str:
    candidates = {k: v for k, v in file_moves.items() if k.startswith(PATH_STRING_PREFIXES)}
    candidates.update({k: v for k, v in dir_moves.items() if k.startswith(PATH_STRING_PREFIXES)})
    if not candidates:
        return text
    pattern = re.compile(
        "(?<![A-Za-z0-9_./-])("
        + "|".join(re.escape(k) for k in sorted(candidates, key=len, reverse=True))
        + r")(?![A-Za-z0-9_-]|\.[A-Za-z0-9])"
    )

    def replace(match: re.Match) -> str:
        old = match.group(1)
        new = candidates[old]
        rewrites.append({"file": path, "old": old, "new": new})
        return new

    return pattern.sub(replace, text)


def apply_moves(plan_path: pathlib.Path, dry_run: bool, report_path: pathlib.Path | None) -> int:
    plan = json.loads(plan_path.read_text())
    files_before = tracked_files()
    old_tree = Tree(files_before)
    file_moves, dir_moves = expand_plan(plan, files_before)
    hashes_before = {src: sha256_of(ROOT / src) for src in file_moves}
    perform_moves(file_moves, dir_moves, dry_run)
    if dry_run:
        print(f"apply-moves (dry run): {len(file_moves)} files would move")
        return 0
    reverse = {v: k for k, v in file_moves.items()}
    rewrites: list[dict] = []
    for path_new in tracked_files():
        if not carries_path_strings(path_new):
            continue
        path_old = reverse.get(path_new, path_new)
        source = ROOT / path_new
        text = source.read_text(encoding="utf-8", errors="surrogateescape")
        updated = text
        if is_living(path_new):
            updated = rewrite_links(path_new, path_old, updated, old_tree, file_moves, dir_moves, rewrites)
        updated = rewrite_path_strings(path_new, updated, file_moves, dir_moves, rewrites)
        if updated != text:
            source.write_text(updated, encoding="utf-8", errors="surrogateescape")
    # Living documents are rewritten on purpose; every other moved byte must be identical.
    rewritten_docs = sorted(dst for dst in file_moves.values() if is_living(dst))
    mismatched = [src for src, dst in file_moves.items()
                  if not is_living(dst) and sha256_of(ROOT / dst) != hashes_before[src]]
    summary = {"plan": str(plan_path), "moved": len(file_moves),
               "moved_bytes": sum((ROOT / dst).stat().st_size for dst in file_moves.values()),
               "hashes_verified": len(file_moves) - len(rewritten_docs), "hash_mismatches": mismatched,
               "moved_living_documents": rewritten_docs, "rewrites": rewrites,
               "file_moves": file_moves, "dir_moves": dir_moves}
    if report_path:
        report_path.write_text(json.dumps(summary, indent=2) + "\n")
    print(f"apply-moves: {len(file_moves)} files moved ({len(file_moves) - len(rewritten_docs)} byte-identical, "
          f"{len(rewritten_docs)} living documents relinked), {len(rewrites)} references rewritten in "
          f"{len({r['file'] for r in rewrites})} files, {len(mismatched)} hash mismatches")
    return 1 if mismatched else 0


def verify_inventories() -> int:
    failures = 0
    inventories = sorted(
        p for p in ROOT.glob("docs/validation/M*/history/inventory.json")
    ) + sorted(ROOT.glob("docs/research/**/inventory.json"))
    for inventory in inventories:
        data = json.loads(inventory.read_text())
        base = inventory.parent
        retained = retired = 0
        for row in data.get("retained", []):
            target = base / row["path"]
            if not target.is_file():
                print(f"MISSING {inventory.relative_to(ROOT)}: {row['path']}")
                failures += 1
                continue
            if sha256_of(target) != row["sha256"] or target.stat().st_size != row["bytes"]:
                print(f"MISMATCH {inventory.relative_to(ROOT)}: {row['path']}")
                failures += 1
                continue
            retained += 1
        for row in data.get("retired", []):
            if (ROOT / row["original_path"]).exists():
                print(f"PRESENT {inventory.relative_to(ROOT)}: retired {row['original_path']} still in tree")
                failures += 1
                continue
            retired += 1
        print(f"{inventory.relative_to(ROOT)}: {retained} retained verified, {retired} retired absent")
    print(f"verify-inventories: {len(inventories)} inventories, {failures} failures")
    return 1 if failures else 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    check = sub.add_parser("links-check")
    check.add_argument("--report", type=pathlib.Path)
    apply = sub.add_parser("apply-moves")
    apply.add_argument("plan", type=pathlib.Path)
    apply.add_argument("--dry-run", action="store_true")
    apply.add_argument("--report", type=pathlib.Path)
    sub.add_parser("verify-inventories")
    args = parser.parse_args(argv)
    os.chdir(ROOT)
    if args.command == "links-check":
        return check_links(args.report)
    if args.command == "apply-moves":
        return apply_moves(args.plan, args.dry_run, args.report)
    return verify_inventories()


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
