#!/usr/bin/env python3
"""Contract freeze manifest and before/after diff for the MCP tool surface.

Three subcommands:

  generate --out PATH
      Hash every tool snapshot under crates/mcp-server/tests/snapshots/*-tool.json
      (schemas, annotations, description) into a manifest with per-tool stability
      class (``stable`` or ``preview``).

  verify MANIFEST [--strict]
      Recompute the same hashes from the current snapshots and compare against a
      manifest. Any difference in a ``stable`` tool (name added/removed, schema,
      annotations, description) fails. A difference in a ``preview`` tool is a
      warning unless ``--strict`` is given. A tool_count mismatch always fails.

  diff --base REF --out PATH [--only NAME,NAME,...]
      Compare the tool snapshots at a Git ref against the current working tree.

Only the standard library is used. Git is invoked with fixed argument lists,
never through a shell.
"""
from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
SNAPSHOTS_DIR = ROOT / "crates/mcp-server/tests/snapshots"
SNAPSHOTS_RELATIVE = "crates/mcp-server/tests/snapshots"

PREVIEW_NAMES = frozenset(
    {
        "rust.analyzer.symbols",
        "rust.analyzer.references",
        "rust.analyzer.diagnostics",
        "rust.analyzer.actions",
        "rust.analyzer.action.apply",
    }
)

CANONICAL_DESCRIPTION = "json sort_keys separators(',',':') ensure_ascii=False sha256"


def utc_now() -> str:
    return datetime.datetime.now(datetime.UTC).isoformat().replace("+00:00", "Z")


def canonical_hash(value) -> str:
    encoded = json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    return hashlib.sha256(encoded.encode("utf-8")).hexdigest()


def git(*args: str) -> str:
    return subprocess.run(["git", *args], cwd=ROOT, check=True, capture_output=True, text=True).stdout


def git_bytes(*args: str) -> bytes:
    return subprocess.run(["git", *args], cwd=ROOT, check=True, capture_output=True).stdout


def head_commit() -> str:
    return git("rev-parse", "HEAD").strip()


def tree_is_dirty() -> bool:
    status = git("status", "--porcelain", "--", SNAPSHOTS_RELATIVE)
    return bool(status.strip())


def resolve_commit(ref: str) -> str:
    result = subprocess.run(
        ["git", "rev-parse", "--verify", "--end-of-options", f"{ref}^{{commit}}"],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise SystemExit(f"--base {ref!r} is not a valid commit-ish: {result.stderr.strip()}")
    return result.stdout.strip()


def stability_of(name: str) -> str:
    return "preview" if name in PREVIEW_NAMES else "stable"


def tool_entry(name: str, spec: dict, snapshot_bytes: bytes) -> dict:
    return {
        "stability": stability_of(name),
        "annotations": spec.get("annotations", {}),
        "input_schema_sha256": canonical_hash(spec["inputSchema"]),
        "output_schema_sha256": canonical_hash(spec["outputSchema"]),
        "description_sha256": canonical_hash(spec["description"]),
        "snapshot_sha256": hashlib.sha256(snapshot_bytes).hexdigest(),
    }


def load_current_tools() -> dict:
    tools = {}
    for path in sorted(SNAPSHOTS_DIR.glob("*-tool.json")):
        raw = path.read_bytes()
        spec = json.loads(raw)
        name = spec["name"]
        tools[name] = tool_entry(name, spec, raw)
    return tools


def counts(tools: dict) -> tuple[int, int, int]:
    stable = sum(1 for entry in tools.values() if entry["stability"] == "stable")
    preview = sum(1 for entry in tools.values() if entry["stability"] == "preview")
    return len(tools), stable, preview


def require_nonempty_path(value: str, label: str) -> pathlib.Path:
    if not value or not value.strip():
        raise SystemExit(f"{label} must not be empty")
    return pathlib.Path(value)


def cmd_generate(out: str) -> int:
    out_path = require_nonempty_path(out, "--out")
    tools = load_current_tools()
    tool_count, stable_count, preview_count = counts(tools)
    manifest = {
        "format_version": 1,
        "generated_utc": utc_now(),
        "head_commit": head_commit(),
        "tree_dirty": tree_is_dirty(),
        "canonical": CANONICAL_DESCRIPTION,
        "tools": tools,
        "tool_count": tool_count,
        "stable_count": stable_count,
        "preview_count": preview_count,
    }
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
    print(f"generate: wrote {tool_count} tools ({stable_count} stable, {preview_count} preview) to {out_path}")
    return 0


FIELDS_COMPARED = ("annotations", "input_schema_sha256", "output_schema_sha256", "description_sha256")


def cmd_verify(manifest_path_arg: str, strict: bool) -> int:
    manifest_path = require_nonempty_path(manifest_path_arg, "MANIFEST")
    if not manifest_path.exists():
        print(f"verify: manifest not found: {manifest_path}", file=sys.stderr)
        return 1
    manifest = json.loads(manifest_path.read_text())

    format_errors: list[str] = []
    if manifest.get("format_version") != 1:
        format_errors.append(f"format_version: expected 1, got {manifest.get('format_version')!r}")
    if manifest.get("canonical") != CANONICAL_DESCRIPTION:
        format_errors.append(f"canonical: expected {CANONICAL_DESCRIPTION!r}, got {manifest.get('canonical')!r}")

    current = load_current_tools()
    recorded = manifest["tools"]

    stable_changed: list[str] = []
    preview_changed: list[str] = []
    class_changed: list[str] = []

    def bucket(klass: str) -> list[str]:
        return stable_changed if klass == "stable" else preview_changed

    for name in sorted(set(recorded) | set(current)):
        if name not in current:
            bucket(recorded[name]["stability"]).append(f"{name}: removed")
            continue
        if name not in recorded:
            bucket(current[name]["stability"]).append(f"{name}: added")
            continue
        old = recorded[name]
        new = current[name]
        if old["stability"] != new["stability"]:
            class_changed.append(f"{name}: {old['stability']} -> {new['stability']}")
        diffs = [field for field in FIELDS_COMPARED if old.get(field) != new.get(field)]
        if diffs:
            bucket(old["stability"]).append(f"{name}: {', '.join(diffs)} changed")

    tool_count_mismatch = manifest.get("tool_count") != len(current)

    if format_errors:
        print("FORMAT MISMATCH:")
        for line in format_errors:
            print(f"  {line}")
    if class_changed:
        print("CLASS CHANGED (always fails):")
        for line in class_changed:
            print(f"  {line}")
    if stable_changed:
        print("STABLE CHANGED:")
        for line in stable_changed:
            print(f"  {line}")
    if preview_changed:
        print("PREVIEW CHANGED (warning unless --strict):")
        for line in preview_changed:
            print(f"  {line}")
    if tool_count_mismatch:
        print(f"TOOL COUNT MISMATCH: manifest={manifest.get('tool_count')} current={len(current)}")

    failed = (
        bool(format_errors)
        or bool(class_changed)
        or bool(stable_changed)
        or tool_count_mismatch
        or (strict and bool(preview_changed))
    )
    summary = {
        "status": "failed" if failed else "passed",
        "format_errors": format_errors,
        "class_changed": class_changed,
        "stable_changed": stable_changed,
        "preview_changed": preview_changed,
    }
    print(json.dumps(summary, sort_keys=True))
    return 1 if failed else 0


def snapshot_names_at_commit(commit: str) -> list[str]:
    listing = git("ls-tree", "--name-only", "--end-of-options", commit, f"{SNAPSHOTS_RELATIVE}/")
    return [line for line in listing.splitlines() if line.endswith("-tool.json")]


def load_ref_tools(commit: str) -> dict:
    tools = {}
    for path in snapshot_names_at_commit(commit):
        raw = git_bytes("show", "--end-of-options", f"{commit}:{path}")
        spec = json.loads(raw.decode("utf-8"))
        name = spec["name"]
        tools[name] = tool_entry(name, spec, raw)
    return tools


def cmd_diff(base: str, out: str, only: str | None) -> int:
    out_path = require_nonempty_path(out, "--out")
    base_commit = resolve_commit(base)
    base_tools = load_ref_tools(base_commit)
    current_tools = load_current_tools()

    if only:
        allowed = {name.strip() for name in only.split(",") if name.strip()}
        base_tools = {name: entry for name, entry in base_tools.items() if name in allowed}
        current_tools = {name: entry for name, entry in current_tools.items() if name in allowed}

    base_names = set(base_tools)
    current_names = set(current_tools)
    added = sorted(current_names - base_names)
    removed = sorted(base_names - current_names)

    changed = []
    unchanged = []
    for name in sorted(base_names & current_names):
        old = base_tools[name]
        new = current_tools[name]
        keys_changed = []
        if old["description_sha256"] != new["description_sha256"]:
            keys_changed.append("description")
        if old["annotations"] != new["annotations"]:
            keys_changed.append("annotations")
        if old["input_schema_sha256"] != new["input_schema_sha256"]:
            keys_changed.append("inputSchema")
        if old["output_schema_sha256"] != new["output_schema_sha256"]:
            keys_changed.append("outputSchema")
        bytes_identical = old["snapshot_sha256"] == new["snapshot_sha256"]
        if keys_changed:
            changed.append(
                {
                    "name": name,
                    "keys_changed": keys_changed,
                    "input_schema_changed": "inputSchema" in keys_changed,
                    "output_schema_changed": "outputSchema" in keys_changed,
                    "annotations_changed": "annotations" in keys_changed,
                    "bytes_identical": bytes_identical,
                }
            )
        else:
            unchanged.append({"name": name, "bytes_identical": bytes_identical})

    result = {
        "base": base,
        "base_commit": base_commit,
        "head_commit": head_commit(),
        "tree_dirty": tree_is_dirty(),
        "added": added,
        "removed": removed,
        "changed": changed,
        "unchanged": unchanged,
    }
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    print(
        f"diff --base {base}: {len(added)} added, {len(removed)} removed, "
        f"{len(changed)} changed, {len(unchanged)} unchanged -> {out_path}"
    )
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)

    generate = sub.add_parser("generate")
    generate.add_argument("--out", required=True)

    verify = sub.add_parser("verify")
    verify.add_argument("manifest")
    verify.add_argument("--strict", action="store_true")

    diff = sub.add_parser("diff")
    diff.add_argument("--base", required=True)
    diff.add_argument("--out", required=True)
    diff.add_argument("--only", default=None, help="comma-separated tool names to restrict the diff to")

    args = parser.parse_args(argv)
    if args.command == "generate":
        return cmd_generate(args.out)
    if args.command == "verify":
        return cmd_verify(args.manifest, args.strict)
    return cmd_diff(args.base, args.out, args.only)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
