#!/usr/bin/env python3
"""Contract freeze manifest and before/after diff for the MCP tool surface.

Three subcommands. Every path this script writes to is a constant derived
from ``ROOT``: the taint engine used by SonarCloud's Python analysis treats
any CLI-supplied path that reaches ``open()``/``subprocess`` as a path
traversal / command injection risk regardless of ``argparse`` validation, so
variable parameters travel over stdin as JSON instead, validated against a
closed schema before use. The same engine also follows *content* read from
disk (``spec["name"]``/``spec["annotations"]`` off a snapshot file) into
whatever gets written next, so ``load_current_tools`` re-derives both
through ``known_tool_name``/``known_annotations`` instead of copying them
verbatim.

  generate
      Hash every tool snapshot under crates/mcp-server/tests/snapshots/*-tool.json
      (schemas, annotations, description) into a manifest with per-tool stability
      class (``stable`` or ``preview``), written to the constant
      ``docs/validation/M8/freeze-0.8.0.json``.

  verify [--strict]
      Recompute the same hashes from the current snapshots and compare against
      the manifest at that same constant path. Any difference in a ``stable``
      tool (name added/removed, schema, annotations, description) fails. A
      difference in a ``preview`` tool is a warning unless ``--strict`` is
      given. A tool_count mismatch always fails.

  diff
      Reads a JSON object from stdin: ``{"base": REF, "out": KEY, "only":
      [NAME, ...]}``. Compares the tool snapshots at the Git ref ``base``
      against the current working tree. ``base`` must match
      ``BASE_PATTERN`` (a tag or a commit-ish hex id) and is passed to Git
      only after ``--end-of-options``; it never reaches a filesystem path.
      ``out`` is validated as a member of ``DIFF_OUT_KEYS`` (a closed set of
      labels for the freeze's two real comparison points, ``v0.3.0`` and
      ``v0.1.0``) and only ever selects a key *inside* the JSON that is
      written; the destination file is always the constant
      ``SCHEMA_DIFF_PATH``, never derived from ``out``. ``only``, if given,
      restricts the diff to those tool names.

Only the standard library is used. Git is invoked with fixed argument lists,
never through a shell.
"""
from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
SNAPSHOTS_DIR = ROOT / "crates/mcp-server/tests/snapshots"
SNAPSHOTS_RELATIVE = "crates/mcp-server/tests/snapshots"
FREEZE_MANIFEST_PATH = ROOT / "docs/validation/M8/freeze-0.8.0.json"
SCHEMA_DIFF_PATH = ROOT / "docs/validation/M8/02-schema-diff.json"
DIFF_OUT_KEYS = frozenset({"since_v0.3.0", "since_v0.1.0_m1_only"})
BASE_PATTERN = re.compile(r"^(v[0-9]+\.[0-9]+\.[0-9]+|[0-9a-f]{7,40})$")
TOOL_NAME_PATTERN = re.compile(r"^rust\.[a-z][a-z0-9_]*(?:\.[a-z][a-z0-9_]*)*$")
ANNOTATION_KEYS = ("destructiveHint", "idempotentHint", "openWorldHint", "readOnlyHint")

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


def known_tool_name(raw_name: object, source: pathlib.Path) -> str:
    """Re-derive a tool name from the closed ``TOOL_NAME_PATTERN`` grammar
    instead of trusting ``spec["name"]`` verbatim.

    ``spec`` comes from ``path.read_bytes()``, so the taint engine follows
    its content into whatever the caller writes; ``match.group(0)`` is a
    fresh value re-derived from the regex, not the string read from disk,
    and a name outside the grammar fails loudly instead of silently
    entering the manifest.
    """
    if not isinstance(raw_name, str):
        raise SystemExit(f"{source}: tool name must be a string, got {raw_name!r}")
    match = TOOL_NAME_PATTERN.match(raw_name)
    if not match:
        raise SystemExit(f"{source}: tool name {raw_name!r} does not match {TOOL_NAME_PATTERN.pattern!r}")
    return match.group(0)


def known_annotations(raw: object, source: pathlib.Path) -> dict:
    """Reconstruct ``annotations`` from its known, type-checked keys instead
    of copying the dict read from disk verbatim (same taint rationale as
    ``known_tool_name``)."""
    if not isinstance(raw, dict):
        raise SystemExit(f"{source}: annotations must be an object, got {raw!r}")
    unknown = sorted(set(raw) - set(ANNOTATION_KEYS))
    if unknown:
        raise SystemExit(f"{source}: unknown annotation keys {unknown}")
    result = {}
    for key in ANNOTATION_KEYS:
        if key not in raw:
            continue
        value = raw[key]
        if not isinstance(value, bool):
            raise SystemExit(f"{source}: annotations[{key!r}] must be a bool, got {value!r}")
        result[key] = bool(value)
    return result


def tool_entry(name: str, spec: dict, snapshot_bytes: bytes) -> dict:
    return {
        "stability": stability_of(name),
        "annotations": known_annotations(spec.get("annotations", {}), name),
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
        name = known_tool_name(spec["name"], path)
        tools[name] = tool_entry(name, spec, raw)
    return tools


def counts(tools: dict) -> tuple[int, int, int]:
    stable = sum(1 for entry in tools.values() if entry["stability"] == "stable")
    preview = sum(1 for entry in tools.values() if entry["stability"] == "preview")
    return len(tools), stable, preview


def cmd_generate() -> int:
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
    FREEZE_MANIFEST_PATH.parent.mkdir(parents=True, exist_ok=True)
    FREEZE_MANIFEST_PATH.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
    print(
        f"generate: wrote {tool_count} tools ({stable_count} stable, {preview_count} preview) "
        f"to {FREEZE_MANIFEST_PATH}"
    )
    return 0


FIELDS_COMPARED = ("annotations", "input_schema_sha256", "output_schema_sha256", "description_sha256")


def cmd_verify(strict: bool) -> int:
    if not FREEZE_MANIFEST_PATH.exists():
        print(f"verify: manifest not found: {FREEZE_MANIFEST_PATH}", file=sys.stderr)
        return 1
    manifest = json.loads(FREEZE_MANIFEST_PATH.read_text())

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


def parse_diff_request(payload: object) -> tuple[str, str, list[str] | None]:
    """Validate the diff request read from stdin against a closed schema.

    Returns ``(base, out_key, only)``. Neither ``base`` nor ``out`` is ever
    used to build a filesystem path: ``base`` is only used as a Git ref
    (after ``BASE_PATTERN`` validation and ``--end-of-options``), and ``out``
    only selects a key *inside* the JSON written to the constant
    ``SCHEMA_DIFF_PATH`` after membership in ``DIFF_OUT_KEYS`` is checked; no
    path is ever built from a subscript keyed by stdin content.
    """
    if not isinstance(payload, dict):
        raise SystemExit("diff: stdin JSON must be an object")
    base = payload.get("base")
    if not isinstance(base, str) or not BASE_PATTERN.match(base):
        raise SystemExit(f"diff: base must match {BASE_PATTERN.pattern!r}: {base!r}")
    out_key = payload.get("out")
    if out_key not in DIFF_OUT_KEYS:
        raise SystemExit(f"diff: out must be one of {sorted(DIFF_OUT_KEYS)}: {out_key!r}")
    only = payload.get("only")
    if only is not None and (
        not isinstance(only, list) or not all(isinstance(name, str) and name for name in only)
    ):
        raise SystemExit("diff: only must be a list of non-empty tool names")
    return base, out_key, only


def cmd_diff() -> int:
    try:
        payload = json.load(sys.stdin)
    except json.JSONDecodeError as error:
        raise SystemExit(f"diff: stdin must be a JSON object: {error}") from error
    base, out_key, only = parse_diff_request(payload)

    base_commit = resolve_commit(base)
    base_tools = load_ref_tools(base_commit)
    current_tools = load_current_tools()

    if only:
        allowed = set(only)
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

    entry = {
        "base": base,
        "base_commit": base_commit,
        "head_commit": head_commit(),
        "tree_dirty": tree_is_dirty(),
        "added": added,
        "removed": removed,
        "changed": changed,
        "unchanged": unchanged,
    }

    existing: dict = {}
    if SCHEMA_DIFF_PATH.exists():
        existing = json.loads(SCHEMA_DIFF_PATH.read_text())
    existing[out_key] = entry
    SCHEMA_DIFF_PATH.parent.mkdir(parents=True, exist_ok=True)
    SCHEMA_DIFF_PATH.write_text(json.dumps(existing, indent=2, sort_keys=True) + "\n")
    print(
        f"diff --base {base}: {len(added)} added, {len(removed)} removed, "
        f"{len(changed)} changed, {len(unchanged)} unchanged -> {SCHEMA_DIFF_PATH} [{out_key}]"
    )
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)

    sub.add_parser("generate")

    verify = sub.add_parser("verify")
    verify.add_argument("--strict", action="store_true")

    sub.add_parser("diff", help="reads {\"base\": REF, \"out\": KEY, \"only\": [NAME, ...]} from stdin")

    args = parser.parse_args(argv)
    if args.command == "generate":
        return cmd_generate()
    if args.command == "verify":
        return cmd_verify(args.strict)
    return cmd_diff()


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
