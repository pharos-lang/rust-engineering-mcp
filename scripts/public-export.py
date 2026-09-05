#!/usr/bin/env python3
"""Export a path-sanitized, history-free public snapshot from a Git commit."""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import re
import shutil
import subprocess


ROOT = pathlib.Path(__file__).resolve().parents[1]
TOKEN_PATTERNS = {
    "github_token": re.compile(rb"(?:gh[pousr]_[A-Za-z0-9_]{20,}|github_pat_[A-Za-z0-9_]{20,})"),
    "openai_key": re.compile(rb"sk-[A-Za-z0-9]{20,}"),
    "aws_access_key": re.compile(rb"AKIA[0-9A-Z]{16}"),
}
PUBLIC_TEST_PRIVATE_KEYS = {
    "crates/mcp-server/src/catalog_sync/test-certs/end.key",
}
COMMITISH = re.compile(r"(?:[0-9A-Fa-f]{40}|[A-Za-z0-9][A-Za-z0-9._/-]{0,200})\Z")


def git(*args: str) -> bytes:
    # Arguments are fixed subcommands plus validated commit/object IDs; never a shell.
    return subprocess.check_output(["git", *args], cwd=ROOT)  # NOSONAR


def validate_commitish(value: str) -> str:
    """Reject option injection and ambiguous/pathological revision spellings."""
    if (
        COMMITISH.fullmatch(value) is None
        or value.startswith("-")
        or ".." in value
        or "//" in value
        or value.endswith(("/", ".lock"))
    ):
        raise ValueError("commit must be a full object ID or bounded Git ref name")
    return value


def resolve_commit(value: str) -> str:
    """Resolve one validated revision to exactly one full commit object ID."""
    commitish = validate_commitish(value)
    commit = git(
        "rev-parse", "--verify", "--end-of-options", f"{commitish}^{{commit}}"
    ).decode().strip()
    if re.fullmatch(r"[0-9a-f]{40}", commit) is None:
        raise RuntimeError("Git did not return exactly one full commit object ID")
    return commit


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=pathlib.Path)
    parser.add_argument("--commit", default="HEAD")
    args = parser.parse_args()

    output = args.output.resolve()
    if output == ROOT or ROOT in output.parents:
        if output.parts[: len(ROOT.parts) + 1] != (*ROOT.parts, "target"):
            raise SystemExit("output inside the repository is allowed only below target/")
    if output.exists():
        shutil.rmtree(output)
    output.mkdir(parents=True, mode=0o700)

    commit = resolve_commit(args.commit)
    rows = git("ls-tree", "-rz", commit).split(b"\0")
    replacements = {
        str(pathlib.Path.home()).encode(): b"<LOCAL_HOME>",
        pathlib.Path.home().name.encode(): b"<LOCAL_USER>",
    }
    redactions: list[dict[str, object]] = []
    file_count = 0

    for row in rows:
        if not row:
            continue
        metadata, raw_path = row.split(b"\t", 1)
        mode, kind, object_id = metadata.decode().split()
        if kind != "blob":
            raise SystemExit(f"unsupported Git object type {kind}")
        relative = raw_path.decode()
        destination = output / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        data = git("cat-file", "blob", object_id)

        if mode == "120000":
            link = data.decode()
            if pathlib.PurePosixPath(link).is_absolute() or ".." in pathlib.PurePosixPath(link).parts:
                raise SystemExit(f"unsafe symlink in source tree: {relative}")
            destination.symlink_to(link)
            continue

        public = data
        if b"\0" not in data:
            try:
                data.decode("utf-8")
            except UnicodeDecodeError:
                pass
            else:
                for private, replacement in replacements.items():
                    public = public.replace(private, replacement)
        destination.write_bytes(public)
        if mode == "100755":
            destination.chmod(0o755)
        file_count += 1
        if public != data:
            redactions.append(
                {
                    "path": relative,
                    "original_sha256": sha256(data),
                    "public_sha256": sha256(public),
                    "replacement_scope": "local-home-or-user-path-only",
                }
            )

    findings = []
    for path in sorted(p for p in output.rglob("*") if p.is_file() and not p.is_symlink()):
        relative = path.relative_to(output).as_posix()
        data = path.read_bytes()
        for name, pattern in TOKEN_PATTERNS.items():
            if pattern.search(data):
                findings.append({"path": relative, "pattern": name})
        private_key = re.search(
            rb"-----BEGIN PRIVATE KEY-----\r?\n[A-Za-z0-9+/]", data
        )
        if private_key and relative not in PUBLIC_TEST_PRIVATE_KEYS:
            findings.append({"path": relative, "pattern": "private_key"})
    if findings:
        raise SystemExit(f"credential-like material found: {findings}")

    manifest = {
        "schema": "rust-engineering-mcp-public-snapshot-v1",
        "source_commit": commit,
        "history_policy": "new public root commit; private local history is not exported",
        "sanitization": {
            "rule": "replace the local home path and local username in UTF-8 files",
            "redacted_file_count": len(redactions),
            "files": redactions,
        },
        "credential_scan": {
            "patterns": sorted(TOKEN_PATTERNS) + ["private_key"],
            "findings": [],
            "allowed_public_test_private_keys": sorted(PUBLIC_TEST_PRIVATE_KEYS),
        },
        "exported_files_before_manifest": file_count,
    }
    (output / "PUBLICATION-SNAPSHOT.json").write_text(
        json.dumps(manifest, indent=2, ensure_ascii=False) + "\n"
    )
    print(json.dumps({"status": "passed", **manifest}, indent=2, ensure_ascii=False))


if __name__ == "__main__":
    if not __debug__:
        raise RuntimeError("optimized Python mode is rejected")
    main()
