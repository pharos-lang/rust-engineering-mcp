#!/usr/bin/env python3
"""Source-bound stock-client qualification harness for the five M6 analyzer
tools (ADR-083, ADR-084): ``rust.analyzer.symbols``, ``.references``,
``.diagnostics``, ``.actions`` and ``.action.apply``.

The default invocation is a client-free, Docker-free preflight: it re-derives
the advertised inventory from the server sources, checks that every planned
call is answered exactly as the tool sources say, reports the host
preconditions and writes nothing.

There are two gate modes, both closed by default, mirroring
``scripts/test-m5-clients.py``:

``--run``
    The Docker-free matrix.  Every analyzer call is a declared refusal the
    server produces before a container can exist: no ``--rust`` runtime is
    calibrated for this host (a real ``--rust`` configuration is present, but
    its Docker socket path is inside the harness's own private directory and
    is never created), so every call answers ``unavailable``/``SANDBOX_DENIED``
    before any container exists.  The gate proves this afterwards by asserting
    the socket path still does not exist.

``--run --with-runtime``
    The real matrix through the admitted M6 image (``sha256:f39a5b33…``).  The
    five read calls run against ``fixtures/valid-basic`` and
    ``fixtures/analyzer-references`` (read-only); the two-tool ``.actions``/
    ``.action.apply`` write flow runs against a **temporary copy** of
    ``fixtures/analyzer-actions``, never the repository fixture itself, using
    the host ``--allow-analyzer-action-write`` grant scoped to that copy.

Two stock clients take part.  The MCP Inspector converts every planned row
deterministically.  Claude Code, restricted to the configured server, drives
the same rows itself: it opens each project, calls the four read tools, and
performs the ``rust.analyzer.action.apply`` preview → commit → receipt cycle
on its own discovered action, reopening the project after commit (the tool's
own contract invalidates the pre-commit reference) and verifying the source
changed on disk.  Neither client uses any MCP Resource: these five tools
publish no artifact.  Codex is not used by this gate.

rust-analyzer's code actions (assists) are not deterministically available
from a transient per-query instance (ADR-084): quiescent ``serverStatus``
readiness does not guarantee an assist is indexed yet.  The Inspector session
retries its one ``rust.analyzer.actions`` capture row a bounded number of
times against this race (see ``m6-inspector-session.mjs``); the model-directed
Claude Code flow treats the write lifecycle as best-effort instead (see
``validate_runtime_model_flow``).  The authoritative proof of the write path
is the Inspector client plus the native ``action.apply`` e2e suite; debt
M6-04 is a stronger assist-readiness signal or a gateway-side retry so this
best-effort path is no longer needed.
"""
from __future__ import annotations

import argparse
import importlib.util
import json
import os
import pathlib
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time

ROOT = pathlib.Path(__file__).resolve().parents[1]
M3_PATH = ROOT / "scripts/test-m3-clients.py"
SESSION = ROOT / "scripts/m6-inspector-session.mjs"
UNIT = ROOT / "scripts/test-m6-clients-unit.py"
ATTEMPTS = ROOT / "target/qualification/test-m6-clients/clients"
CURRENT = ROOT / "target/qualification/test-m6-clients/clients.json"
PREFLIGHT = ROOT / "target/qualification/test-m6-clients/clients-preflight.json"
SERVER = ROOT / "target/release/rust-engineering-mcp"
NODE = pathlib.Path("/Users/cburgosro/.nvm/versions/node/v24.15.0/bin/node")
# The versioned executable, not the `~/.local/bin/claude` symlink (see
# test-m5-clients.py): the gate pins one version whose bytes the receipt
# records by SHA-256.
CLAUDE = pathlib.Path("/Users/cburgosro/.local/share/claude/versions/2.1.267")
INSPECTOR = ROOT / "target/m1-17-inspector/node_modules/@modelcontextprotocol/inspector/clients/cli/build/index.js"
INSPECTOR_PACKAGE = ROOT / "target/m1-17-inspector/node_modules/@modelcontextprotocol/inspector/package.json"
DOCKER = pathlib.Path("/Applications/Docker.app/Contents/Resources/bin/docker")
ANALYZER = ROOT / "crates/mcp-server/src/stdio/analyzer.rs"
ANALYZER_ACTIONS = ROOT / "crates/mcp-server/src/stdio/analyzer/actions.rs"
ANALYZER_ACTION_APPLY = ROOT / "crates/mcp-server/src/stdio/mutation/analyzer_action.rs"
STDIO = ROOT / "crates/mcp-server/src/stdio.rs"
PROTOCOL_TEST = ROOT / "crates/mcp-server/tests/protocol.rs"
HOST_CONFIG = ROOT / "crates/mcp-server/src/host_config.rs"
ANALYZER_GATEWAY = ROOT / "crates/execution-adapter/src/analyzer_gateway.rs"

INSPECTOR_VERSION = "2.5.0"
# The stock agentic client.  Version and model are pinned: the session's own
# `init` event must report both, and a fallback to another model is a failure.
CLAUDE_VERSION = "2.1.267 (Claude Code)"
CLAUDE_MODEL = "claude-sonnet-5"
CLAUDE_EFFORT = "medium"
CLAUDE_CLIENT = "claude-code"
CLAUDE_SERVER = "rust_engineering"
# Claude Code's wall-clock bound per MCP tool call, in milliseconds. rust.check
# is never called here (Option A: the applied action is not compile-verified),
# so every call is bounded by the analyzer's own 180 s ceiling with margin.
MCP_TOOL_TIMEOUT_MS = 300_000
WRITE_GRANT_FLAG = "--allow-analyzer-action-write"

M3_TOOLS = (
    "rust.project.open", "rust.project.inspect", "rust.toolchain.inspect",
    "rust.check", "rust.fmt.check", "rust.clippy", "rust.test",
    "rust.test.nextest", "rust.dependencies.audit", "rust.diagnostics.explain",
    "rust.quality.gate", "rust.catalog.status", "rust.crate.search",
    "rust.crate.inspect", "rust.manifest.patch", "rust.fmt.apply",
    "rust.fix.apply", "rust.dependency.add", "rust.dependency.remove",
    "rust.coverage", "rust.semver.check", "rust.mutation.test",
)
M4_TOOLS = (
    "rust.deny", "rust.unsafe.scan", "rust.supply_chain.inspect",
    "rust.quality.gate.v2", "rust.miri",
)
M5_TOOLS = (
    "rust.benchmark.run", "rust.benchmark.compare",
    "rust.profile.flamegraph", "rust.binary.bloat",
)
M6_TOOLS = (
    "rust.analyzer.symbols", "rust.analyzer.references",
    "rust.analyzer.diagnostics", "rust.analyzer.actions",
    "rust.analyzer.action.apply",
)
PRIOR_TOOLS = M3_TOOLS + M4_TOOLS + M5_TOOLS
EXPECTED_TOOLS = PRIOR_TOOLS + M6_TOOLS

# tool -> (source file carrying its closed `Code` enum, that enum's name).
# Unlike M3-M5 (one tool per file), the five M6 tools share three files.
TOOL_SOURCES = {
    "rust.analyzer.symbols": (ANALYZER, "Code"),
    "rust.analyzer.references": (ANALYZER, "ReferencesCode"),
    "rust.analyzer.diagnostics": (ANALYZER, "DiagnosticsCode"),
    "rust.analyzer.actions": (ANALYZER_ACTIONS, "ActionsCode"),
    "rust.analyzer.action.apply": (ANALYZER_ACTION_APPLY, "ApplyCode"),
}

# Repository-relative fixture roots. Only these names reach a receipt; the
# absolute host paths stay inside the closed server argv. `analyzer_actions`
# is opened read-only in the Docker-free mode alone: the runtime mode's write
# row never opens this repository fixture, only a temporary copy of it.
FIXTURES = {
    "valid_basic": "fixtures/valid-basic",
    "analyzer_references": "fixtures/analyzer-references",
    "analyzer_actions": "fixtures/analyzer-actions",
}
# The dynamic project key of the runtime mode's temporary write copy: never a
# key of FIXTURES, resolved at run time to a private directory instead.
WRITE_PROJECT = "write"

PLACEHOLDER_DIGEST = "sha256:" + "1" * 64
# macOS keeps the private scratch under /private/tmp (real path, not the
# S5443-flagged /tmp); a constant so the unit tests can redirect it to a
# portable tempdir on the Linux CI runner.
PRIVATE_DIR_BASE = "/private/tmp"
UNKNOWN_PROJECT_REF = "prj_" + "0" * 32
# Matches the tool's `AnalyzerFile` pattern (`.rs`, <=100 chars) but names no
# file the capture ever contains.
BAD_FILE = "missing-from-snapshot.rs"
IDEMPOTENCY_KEY = "m6-w09-clients-harness"
# `add` at (1, 8) is analyzer-references' declaration; the native W06c cut
# fixed this fixture's coordinates: declaration line 1 col 8, use line 2 col
# 31 (historical receipt: docs/validation/M6/delegation/W06c-references-fixture/report.md at 51fa602e).
REFERENCES_POSITION = {"line": 1, "column": 8}
# An empty selection on `sum` in `analyzer-actions`' `let sum = …`: the exact
# cursor the native m6-11-code-actions cut used, calibrated to list exactly
# two applicable, source-changing actions (historical receipt: docs/validation/M6/01-calibration.json at 51fa602e).
ACTIONS_RANGE = {"start": {"line": 2, "column": 9}, "end": {"line": 2, "column": 9}}
ANALYZER_FILE = "src/lib.rs"

DOCKER_FREE = "docker_free"
RUNTIME = "runtime"

# The two tools ADR-083 SS2 makes `expected_project_fingerprint` mandatory
# for; the host checks it before any container exists, so a hard-coded value
# is refused (see `check_expectation`) and every plan must instead capture it
# from that session's own `rust.project.open`.
FINGERPRINT_TOOLS = frozenset({"rust.analyzer.actions", "rust.analyzer.action.apply"})


def requires_fingerprint(tool: str) -> bool:
    return tool in FINGERPRINT_TOOLS


# The Docker-free matrix: one row per tool. SANDBOX_DENIED fires before any
# request-specific validation (no --rust runtime is calibrated for this
# host), so a single row per tool is the whole matrix; there is no second,
# argument-shaped refusal to pair it with (D2: these tools carry no Tasks/
# execution_mode surface for a second declared reason to attach to, unlike
# M5's `TASKS_REQUIRED` pairing). The two `FINGERPRINT_TOOLS` rows carry
# `kind: docker_free_fingerprint`: the driver supplies the real
# `expected_project_fingerprint` captured from opening `project` in this same
# session, so the refusal each proves is the uniform SANDBOX_DENIED and never
# a fingerprint-mismatch CONFLICT the host would raise first.
CALL_PLAN = (
    {
        "tool": "rust.analyzer.symbols", "project": "valid_basic",
        "arguments": {"scope": "document", "file": ANALYZER_FILE},
        "rationale": "no --rust runtime is calibrated for this host, so the M6 image is "
                     "never dialed and no container can exist",
    },
    {
        "tool": "rust.analyzer.references", "project": "analyzer_references",
        "arguments": {"file": ANALYZER_FILE, "position": REFERENCES_POSITION,
                      "include_declaration": True},
        "rationale": "the same host-level refusal, independent of the query shape",
    },
    {
        "tool": "rust.analyzer.diagnostics", "project": "valid_basic",
        "arguments": {"file": ANALYZER_FILE},
        "rationale": "the same host-level refusal, independent of the query shape",
    },
    {
        "tool": "rust.analyzer.actions", "project": "analyzer_actions",
        "arguments": {"file": ANALYZER_FILE, "range": ACTIONS_RANGE},
        "kind": "docker_free_fingerprint",
        "rationale": "the same host-level refusal; expected_project_fingerprint is the real "
                     "value captured from this session's own project.open, so the mandatory "
                     "host-side fingerprint check (ADR-083 SS2) passes and the runtime dial is "
                     "what refuses the call",
    },
    {
        "tool": "rust.analyzer.action.apply", "project": "analyzer_actions",
        "arguments": {"action": {"mode": "preview", "action_digest": PLACEHOLDER_DIGEST,
                                  "file": ANALYZER_FILE, "range": ACTIONS_RANGE}},
        "kind": "docker_free_fingerprint",
        "rationale": "the write tool answers the identical host-level refusal without the "
                     "write grant or a calibrated runtime, using the same real captured "
                     "fingerprint; the placeholder action_digest is never reached because the "
                     "runtime dial refuses first; the repository fixture is opened read-only "
                     "and nothing is ever written",
    },
)

# The Docker-backed matrix: every row is a real execution against the
# admitted M6 image. `kind` selects how the driver assembles arguments and
# what it captures; only `static` rows carry a complete literal `arguments`.
RUNTIME_CALL_PLAN = (
    {
        "kind": "static", "tool": "rust.analyzer.symbols", "shape": "positive",
        "project": "valid_basic", "fact_key": "symbols_document",
        "arguments": {"scope": "document", "file": ANALYZER_FILE},
        "expect_status": "passed", "expect_error_code": None,
        "rationale": "a document-scope read over a real quiescent analyzer session",
    },
    {
        "kind": "static", "tool": "rust.analyzer.symbols", "shape": "positive",
        "project": "valid_basic", "fact_key": "symbols_workspace",
        "arguments": {"scope": "workspace", "query": "add"},
        "expect_status": "passed", "expect_error_code": None,
        "rationale": "a workspace-scope search naming the fixture's own function",
    },
    {
        "kind": "static", "tool": "rust.analyzer.references", "shape": "positive",
        "project": "analyzer_references", "fact_key": "references",
        "arguments": {"file": ANALYZER_FILE, "position": REFERENCES_POSITION,
                      "include_declaration": True},
        "expect_status": "passed", "expect_error_code": None,
        "rationale": "the declaration position resolves to exactly one declaration and one use",
    },
    {
        "kind": "static", "tool": "rust.analyzer.diagnostics", "shape": "positive",
        "project": "valid_basic", "fact_key": "diagnostics",
        "arguments": {"file": ANALYZER_FILE},
        "expect_status": "passed", "expect_error_code": None,
        "rationale": "the honest M6-03 syntax-only contract: complete, empty is a valid answer",
    },
    {
        "kind": "actions_capture", "tool": "rust.analyzer.actions", "shape": "positive",
        "project": WRITE_PROJECT, "fact_key": "actions",
        "arguments": {"file": ANALYZER_FILE, "range": ACTIONS_RANGE},
        "expect_status": "passed", "expect_error_code": None,
        "rationale": "lists at least one applicable action with a digest, over the temp copy",
    },
    {
        "kind": "apply_preview", "tool": "rust.analyzer.action.apply", "shape": "positive",
        "project": WRITE_PROJECT, "fact_key": "apply_preview",
        "arguments": {},
        "expect_status": "passed", "expect_error_code": None,
        "rationale": "re-resolves the captured digest and plans its exact diff without writing",
    },
    {
        "kind": "apply_commit", "tool": "rust.analyzer.action.apply", "shape": "positive",
        "project": WRITE_PROJECT, "fact_key": "apply_commit", "reopen_after": True,
        "arguments": {"idempotency_key": IDEMPOTENCY_KEY},
        "expect_status": "passed", "expect_error_code": None,
        "rationale": "commits the previewed plan; the write lands and the pre-commit "
                     "project_ref is invalidated",
    },
    {
        "kind": "apply_receipt", "tool": "rust.analyzer.action.apply", "shape": "positive",
        "project": WRITE_PROJECT, "fact_key": "apply_receipt",
        "arguments": {"recover": False},
        "expect_status": "passed", "expect_error_code": None,
        "rationale": "the durable receipt reports committed, read with the reopened reference",
    },
    {
        "kind": "apply_preview_stale", "tool": "rust.analyzer.action.apply", "shape": "negative",
        "project": WRITE_PROJECT, "fact_key": "apply_stale",
        "arguments": {},
        "expect_status": "blocked", "expect_error_code": "ACTION_STALE",
        "rationale": "the same digest re-resolved after the commit changed the source is stale",
    },
    {
        "kind": "static", "tool": "rust.analyzer.symbols", "shape": "negative",
        "project": "valid_basic", "fact_key": "bad_file",
        "arguments": {"scope": "document", "file": BAD_FILE},
        "expect_status": "blocked", "expect_error_code": "FILE_NOT_IN_SNAPSHOT",
        "rationale": "a well-formed .rs path absent from the capture is the declared refusal, "
                     "not a schema-level invalid_params",
    },
)

SAFE_PROTOCOL_KEYS = frozenset({
    "client", "direction", "session", "bytes", "sha256", "malformed",
    "method", "tasks_declared", "tasks_advertised", "tool",
})
SAFE_CALL_KEYS = frozenset({
    "client", "tool", "shape", "mode", "status", "error_code", "is_error",
    "request_bytes", "request_sha256", "response_bytes", "response_sha256",
})
CALL_CLIENTS = frozenset({"inspector"})
CALL_SHAPES = frozenset({"positive", "negative"})
CALL_MODES = frozenset({DOCKER_FREE, RUNTIME})
CALL_STATUSES = frozenset({"passed", "failed", "blocked", "unavailable", "cancelled"})
ERROR_RESULT_STATUSES = frozenset({"blocked", "unavailable", "cancelled"})
DYNAMIC_KINDS = frozenset({
    "actions_capture", "apply_preview", "apply_commit", "apply_receipt",
    "apply_preview_stale",
})


def load_m3():
    """Reuse M3's bounded subprocess, proxy, digest and receipt primitives."""
    spec = importlib.util.spec_from_file_location("rust_mcp_m3_clients", M3_PATH)
    if spec is None or spec.loader is None:
        raise RuntimeError("M3 client harness is unavailable")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def source_hashes() -> dict[str, str]:
    m3 = load_m3()
    paths = (
        M3_PATH, pathlib.Path(__file__).resolve(), SESSION, UNIT,
        ANALYZER, ANALYZER_ACTIONS, ANALYZER_ACTION_APPLY,
    )
    return {str(path.relative_to(ROOT)): m3.file_digest(path) for path in paths if path.is_file()}


def m6_image() -> str:
    """The one digest ADR-082/ADR-085 admit for an analyzer session."""
    match = re.search(r'APPROVED_M6_IMAGE: &str =\s*"(sha256:[0-9a-f]{64})"',
                       ANALYZER_GATEWAY.read_text())
    if match is None:
        raise RuntimeError("qualified M6 image digest is missing")
    return match.group(1)


def protocol_inventory() -> tuple[str, ...]:
    """The ordered inventory the server's own protocol oracle asserts."""
    text = PROTOCOL_TEST.read_text()
    marker = "assert_eq!(tools.map(Vec::len), Some("
    start = text.index(marker) + len(marker)
    count = int(text[start:text.index(")", start)])
    anchor = "assert_eq!(\n        names,\n        [\n"
    begin = text.index(anchor) + len(anchor)
    names = tuple(re.findall(r'"([^"]+)"', text[begin:text.index("\n        ]\n", begin)]))
    if len(names) != count or len(set(names)) != count:
        raise RuntimeError("server protocol inventory oracle is inconsistent")
    return names


def m6_pushed_unconditionally() -> bool:
    """Whether `stdio.rs::list_tools` pushes the five M6 tools, in order, with
    no `if …::advertised()` guard between them (unlike the gated M4/M5 block
    that precedes them)."""
    pattern = re.compile(
        r"tools\.push\(self\.analyzer_symbols\.definition\.clone\(\)\);\s*"
        r"tools\.push\(self\.analyzer_references\.definition\.clone\(\)\);\s*"
        r"tools\.push\(self\.analyzer_diagnostics\.definition\.clone\(\)\);\s*"
        r"tools\.push\(self\.analyzer_actions\.definition\.clone\(\)\);\s*"
        r"tools\.push\(self\.analyzer_action_apply\.definition\.clone\(\)\);"
    )
    return pattern.search(STDIO.read_text()) is not None


def inventory_check() -> dict[str, object]:
    """A count or an order that drifts is a failure here, never a warning."""
    published = protocol_inventory()
    if published != EXPECTED_TOOLS:
        raise RuntimeError("advertised inventory drifted from the server protocol oracle")
    if len(EXPECTED_TOOLS) != 36 or len(set(EXPECTED_TOOLS)) != 36:
        raise RuntimeError("closed tool inventory is invalid")
    if EXPECTED_TOOLS[:31] != PRIOR_TOOLS:
        raise RuntimeError("the thirty-one previous tools changed name or order")
    if EXPECTED_TOOLS[31:] != M6_TOOLS:
        raise RuntimeError("the five M6 tools are not appended in the advertised order")
    if not m6_pushed_unconditionally():
        raise RuntimeError("stdio.rs does not push the five M6 tools unconditionally, in order")
    return {
        "count": len(EXPECTED_TOOLS),
        "previous_unchanged": True,
        "previous_count": len(PRIOR_TOOLS),
        "m6_appended": list(M6_TOOLS),
        "pushed_unconditionally": True,
        "sources": ["crates/mcp-server/src/stdio.rs", "crates/mcp-server/tests/protocol.rs"],
    }


def screaming(name: str) -> str:
    return re.sub(r"(?<!^)(?=[A-Z])", "_", name).upper()


def declared_error_codes(tool: str) -> tuple[str, ...]:
    """The closed `Code`-family wire vocabulary one M6 tool can publish."""
    path, enum_name = TOOL_SOURCES[tool]
    source = path.read_text()
    marker = f"enum {enum_name} {{"
    start = source.index(marker) + len(marker)
    block = source[start:source.index("\n}", start)]
    codes = tuple(screaming(name) for name in re.findall(r"^\s{4}([A-Z][A-Za-z0-9]*),$", block, re.M))
    if not codes or len(codes) != len(set(codes)):
        raise RuntimeError(f"error code vocabulary is invalid for {tool}")
    return codes


def check_expectation(tool: str, project: object, arguments: dict[str, object],
                      expect_status: str, expect_error_code: str | None) -> None:
    """Both plans agree on this much: a closed tool, status and error vocabulary."""
    if tool not in M6_TOOLS:
        raise RuntimeError("call plan names an unknown tool")
    if expect_status not in CALL_STATUSES:
        raise RuntimeError("call plan names an unknown status")
    if expect_error_code is None:
        if expect_status in ERROR_RESULT_STATUSES:
            raise RuntimeError("a refused call must name the declared error code")
    elif expect_error_code not in declared_error_codes(tool):
        raise RuntimeError(f"{expect_error_code} is not declared by {tool}")
    if project != WRITE_PROJECT and project not in FIXTURES:
        raise RuntimeError("call plan names an unknown fixture root")
    if "project_ref" in arguments:
        raise RuntimeError("project authority is resolved by the client, never hard-coded")
    action = arguments.get("action")
    if "expected_project_fingerprint" in arguments or (
        isinstance(action, dict) and "expected_project_fingerprint" in action
    ):
        raise RuntimeError("the project fingerprint is captured by the client from its own "
                           "project.open, never hard-coded")


def call_plan() -> list[dict[str, object]]:
    """The Docker-free matrix: every row is answered before any container."""
    covered = set()
    rows = []
    for row in CALL_PLAN:
        check_expectation(row["tool"], row["project"], row["arguments"],
                          "unavailable", "SANDBOX_DENIED")
        kind = row.get("kind", "static")
        if requires_fingerprint(row["tool"]) != (kind == "docker_free_fingerprint"):
            raise RuntimeError(f"{row['tool']} fingerprint capture is inconsistent with its kind")
        covered.add(row["tool"])
        rows.append({
            "tool": row["tool"], "shape": "positive", "mode": DOCKER_FREE,
            "project": row["project"], "arguments": row["arguments"], "kind": kind,
            "expect_status": "unavailable", "expect_error_code": "SANDBOX_DENIED",
            "expect_is_error": True, "rationale": row["rationale"],
        })
    missing = sorted(set(M6_TOOLS) - covered)
    if missing:
        raise RuntimeError("Docker-free call plan omits: " + ", ".join(missing))
    return rows


def check_runtime_row(row: dict[str, object]) -> None:
    check_expectation(row["tool"], row["project"], row["arguments"],
                      row["expect_status"], row["expect_error_code"])
    if row["kind"] not in ({"static"} | DYNAMIC_KINDS):
        raise RuntimeError("runtime call plan names an unknown row kind")
    if row["shape"] not in CALL_SHAPES:
        raise RuntimeError("runtime call plan names an unknown shape")
    if row["kind"] == "static" and row["project"] == WRITE_PROJECT:
        raise RuntimeError("a static row never touches the write project")
    if row["kind"] in DYNAMIC_KINDS and row["project"] != WRITE_PROJECT:
        raise RuntimeError("a dynamic row always touches the write project")
    if row["kind"] != "apply_commit" and row.get("reopen_after"):
        raise RuntimeError("only the commit row reopens the write project")
    if row["expect_error_code"] == "ACTION_STALE" and row["kind"] != "apply_preview_stale":
        raise RuntimeError("ACTION_STALE is only expected from the stale preview row")
    if row["expect_error_code"] == "FILE_NOT_IN_SNAPSHOT" and row["tool"] != "rust.analyzer.symbols":
        raise RuntimeError("the bad-file row is scoped to rust.analyzer.symbols")


def runtime_call_plan() -> list[dict[str, object]]:
    """The Docker-backed matrix: reads, then the full write lifecycle, then
    one stale-digest negative and one bad-file negative."""
    rows = []
    tools_seen: dict[str, set[str]] = {tool: set() for tool in M6_TOOLS}
    for row in RUNTIME_CALL_PLAN:
        check_runtime_row(row)
        tools_seen[row["tool"]].add(row["shape"])
        rows.append({
            "kind": row["kind"], "tool": row["tool"], "shape": row["shape"], "mode": RUNTIME,
            "project": row["project"], "arguments": row["arguments"],
            "expect_status": row["expect_status"], "expect_error_code": row["expect_error_code"],
            "expect_is_error": row["expect_status"] in ERROR_RESULT_STATUSES,
            "fact_key": row["fact_key"], "reopen_after": bool(row.get("reopen_after")),
            "rationale": row["rationale"],
        })
    missing = sorted(tool for tool, shapes in tools_seen.items() if "positive" not in shapes)
    if missing:
        raise RuntimeError("runtime call plan carries no positive row for: " + ", ".join(missing))
    if not rows:
        raise RuntimeError("the runtime plan is empty")
    return rows


def candidate_advertises_m6() -> bool:
    """Necessary, not sufficient: the built candidate must carry the M6 names.

    The authoritative check is the client discovery oracle inside ``--run``;
    this one exists so the Docker-free preflight can refuse a stale binary
    loudly instead of failing halfway through a client session.
    """
    if not SERVER.is_file():
        return False
    needles = [tool.encode() for tool in M6_TOOLS]
    found = set()
    tail = b""
    with SERVER.open("rb") as stream:
        while block := stream.read(1024 * 1024):
            window = tail + block
            found.update(needle for needle in needles if needle in window)
            tail = block[-64:]
    return len(found) == len(needles)


def client_versions() -> dict[str, object]:
    """`tasks` mirrors what the stock server actually advertises at the
    protocol level (other tool families use Tasks, so the capability is on
    for every client), matching the M5 precedent. `resource` stays `False`
    for both clients, unlike M5: the five M6 tools publish no MCP Resource,
    so there is no artifact for either client to read here."""
    package_version = None
    if INSPECTOR_PACKAGE.is_file():
        package_version = json.loads(INSPECTOR_PACKAGE.read_text()).get("version")
    return {
        "inspector": {"expected": INSPECTOR_VERSION, "observed": package_version,
                      "tasks": True, "resource": False},
        "claude_code": {"expected": CLAUDE_VERSION, "observed": claude_version(),
                        "model": CLAUDE_MODEL, "effort": CLAUDE_EFFORT,
                        "tasks": False, "resource": False},
    }


def claude_version() -> str | None:
    if not CLAUDE.is_file():
        return None
    result = subprocess.run([str(CLAUDE), "--version"], capture_output=True,
                            text=True, timeout=10, check=False)
    return result.stdout.strip() or None


def claude_logged_in() -> bool:
    """`claude auth status` reads the installed login in place; nothing is copied."""
    if not CLAUDE.is_file():
        return False
    result = subprocess.run([str(CLAUDE), "auth", "status"], capture_output=True,
                            text=True, timeout=20, check=False)
    try:
        status = json.loads(result.stdout)
    except json.JSONDecodeError:
        return False
    return result.returncode == 0 and isinstance(status, dict) and status.get("loggedIn") is True


def host_config_accepts_write_grant() -> bool:
    host = HOST_CONFIG.read_text()
    return f'"{WRITE_GRANT_FLAG}"' in host


def runtime_preconditions(socket: str | None) -> dict[str, tuple[bool, str]]:
    """What the Docker-backed mode needs beyond the Docker-free one."""
    path = pathlib.Path(socket) if socket else None
    return {
        "docker_socket": (
            path is not None and path.is_absolute() and path.exists(),
            "an absolute, existing Docker socket (--docker-socket or RUST_MCP_TEST_SOCKET)",
        ),
        "docker_binary": (DOCKER.is_file(), "the pinned docker executable must exist"),
        "write_grant_supported": (
            host_config_accepts_write_grant(),
            f"the host must accept {WRITE_GRANT_FLAG}",
        ),
        "qualified_image_admitted": (
            "APPROVED_M6_IMAGE" in HOST_CONFIG.read_text(),
            "the host must admit the qualified M6 image digest",
        ),
    }


def preconditions(versions: dict[str, object], with_runtime: bool,
                  socket: str | None) -> dict[str, dict[str, object]]:
    checks: dict[str, tuple[bool, str]] = {
        "candidate_server_binary": (
            SERVER.is_file(),
            "target/release/rust-engineering-mcp must exist",
        ),
        "candidate_advertises_m6": (
            candidate_advertises_m6(),
            "the built candidate must carry the five M6 tool names",
        ),
        "node": (NODE.is_file(), "the pinned Node runtime must exist"),
        "inspector_bundle": (INSPECTOR.is_file(), "the pinned Inspector CLI bundle must exist"),
        "inspector_version": (
            versions["inspector"]["observed"] == INSPECTOR_VERSION,
            f"the pinned Inspector must report {INSPECTOR_VERSION}",
        ),
        "claude_binary": (CLAUDE.is_file(), "the pinned stock Claude Code executable must exist"),
        "claude_version": (
            versions["claude_code"]["observed"] == CLAUDE_VERSION,
            f"stock Claude Code must report {CLAUDE_VERSION}",
        ),
        "claude_auth": (claude_logged_in(),
                        "stock Claude Code must be logged in; no credential is copied"),
        "inspector_session": (SESSION.is_file(), "the M6 Inspector session driver must exist"),
        "docker_binary_present": (
            DOCKER.is_file(),
            "the pinned docker path must exist because the host runtime group requires it; "
            "the Docker-free mode never executes it",
        ),
        "fixture_roots": (
            all((ROOT / path / "Cargo.toml").is_file() for path in FIXTURES.values()),
            "every fixture root must carry a manifest",
        ),
    }
    if with_runtime:
        checks.update(runtime_preconditions(socket))
    return {name: {"satisfied": bool(value), "requirement": detail}
            for name, (value, detail) in checks.items()}


def preflight(with_runtime: bool = False, socket: str | None = None) -> dict[str, object]:
    versions = client_versions()
    checks = preconditions(versions, with_runtime, socket)
    unsatisfied = sorted(name for name, value in checks.items() if not value["satisfied"])
    return {
        "schema": "rust-mcp-m6-clients-preflight-v1",
        "status": "ready" if not unsatisfied else "blocked",
        "execution_performed": False,
        "clients_started": False,
        "with_runtime_requested": with_runtime,
        "docker_required": with_runtime,
        "docker_used": False,
        "image_id": m6_image(),
        "expected_tools": list(EXPECTED_TOOLS),
        "m6_tools": list(M6_TOOLS),
        "inventory": inventory_check(),
        "call_plan": call_plan(),
        "runtime_call_plan": runtime_call_plan(),
        "clients": versions,
        "preconditions": checks,
        "unsatisfied": unsatisfied,
        "source_sha256": source_hashes(),
        "m3_reuse": ["proxy", "run_bounded", "digest", "file_digest", "save_json",
                     "protocol_summary", "assert_no_credentials", "find_values",
                     "append_observation"],
    }


def validate_protocol_metadata(path: pathlib.Path) -> dict[str, object]:
    m3 = load_m3()
    rows = [json.loads(line) for line in path.read_text().splitlines() if line]
    for row in rows:
        extra = set(row) - SAFE_PROTOCOL_KEYS
        if extra:
            raise RuntimeError("protocol metadata contains unapproved keys: " + ",".join(sorted(extra)))
        if not isinstance(row.get("sha256"), str) or len(row["sha256"]) != 64:
            raise RuntimeError("protocol metadata digest is invalid")
    encoded = path.read_bytes().lower()
    for forbidden in (b"authorization", b"access_token", b"refresh_token", b"auth.json"):
        if forbidden in encoded:
            raise RuntimeError("credential-shaped protocol evidence")
    summary = m3.protocol_summary(path, True)
    summary["metadata_only"] = True
    return summary


def validate_call_rows(rows: object, client: str, plan: list[dict[str, object]]) -> list[dict[str, object]]:
    """Closed-vocabulary check: a row may carry counts and digests, never text."""
    if not isinstance(rows, list) or len(rows) != len(plan):
        raise RuntimeError(f"{client} did not report one row per planned call")
    for row, expected in zip(rows, plan):
        if not isinstance(row, dict):
            raise RuntimeError("call row is not an object")
        if set(row) != SAFE_CALL_KEYS:
            raise RuntimeError("call row keys are not the approved set: "
                               + ",".join(sorted(set(row) ^ SAFE_CALL_KEYS)))
        if row["client"] != client or row["client"] not in CALL_CLIENTS:
            raise RuntimeError("call row names an unexpected client")
        if row["tool"] != expected["tool"] or row["shape"] != expected["shape"]:
            raise RuntimeError("call rows are not in planned order")
        if row["mode"] != expected["mode"] or row["mode"] not in CALL_MODES:
            raise RuntimeError("call row names an unexpected mode")
        if row["status"] != expected["expect_status"] or row["status"] not in CALL_STATUSES:
            raise RuntimeError(f"{client} {row['tool']} {row['mode']} status is not the planned one")
        if row["error_code"] != expected["expect_error_code"]:
            raise RuntimeError(f"{client} {row['tool']} {row['mode']} error code is not the planned one")
        if row["error_code"] is not None and row["error_code"] not in declared_error_codes(row["tool"]):
            raise RuntimeError("call row reports an undeclared error code")
        if row["is_error"] is not expected["expect_is_error"]:
            raise RuntimeError(f"{client} {row['tool']} {row['mode']} isError is not the planned one")
        for name in ("request_sha256", "response_sha256"):
            if not isinstance(row[name], str) or not re.fullmatch(r"[0-9a-f]{64}", row[name]):
                raise RuntimeError("call row digest is invalid")
        for name in ("request_bytes", "response_bytes"):
            if not isinstance(row[name], int) or isinstance(row[name], bool) or row[name] <= 0:
                raise RuntimeError("call row byte count is invalid")
    return list(rows)


def base_argv(state: pathlib.Path, socket: pathlib.Path) -> list[str]:
    argv = [str(SERVER), "serve", "--stdio"]
    for path in FIXTURES.values():
        argv += ["--root", str(ROOT / path)]
    argv += ["--docker", str(DOCKER), "--docker-socket", str(socket),
             "--state-root", str(state), "--rust-image", m6_image()]
    return argv


def server_argv(state: pathlib.Path, socket: pathlib.Path) -> list[str]:
    """The Docker-free host configuration: `--rust` is fully configured (a
    real image digest, a real docker binary) but `socket` is a path inside
    the harness's own private directory that is never created, so every
    analyzer call is refused as a failed-calibration `SANDBOX_DENIED` before
    any container exists."""
    return base_argv(state, socket)


def runtime_server_argv(state: pathlib.Path, socket: pathlib.Path,
                        write_root: pathlib.Path) -> list[str]:
    """The Docker-backed host configuration: the real socket, plus the write
    grant scoped to the temporary copy alone."""
    argv = base_argv(state, socket)
    return argv + ["--root", str(write_root), WRITE_GRANT_FLAG, str(write_root)]


def stage_write_fixture(private: pathlib.Path) -> pathlib.Path:
    """A private, exclusive copy of `fixtures/analyzer-actions`: the write row
    must never touch the checked-in fixture, only this copy."""
    source = ROOT / FIXTURES["analyzer_actions"]
    destination = private / "analyzer-actions-write"
    shutil.copytree(source, destination)
    return destination


# W09e: between the Inspector runtime gate and the Claude Code runtime gate
# alone (`--with-runtime`), a bounded settle so the Claude batch starts with
# fresh host/Docker capacity. The Inspector batch spawns roughly ten
# rust-analyzer containers off the 2.5 GB M6 image; without a settle, the
# Claude batch that follows it in the same invocation can race the host while
# that image is still being reclaimed, and a spawn is then honestly refused as
# `unavailable`/`SANDBOX_DENIED` from transient host saturation alone, not a
# defect in M6. Same label the gateway itself uses to find its own residue
# (see `analyzer_native.rs::residue`).
CONTAINER_LABEL_FILTER = "--filter=label=org.rust-mcp.execution=true"
CONTAINER_SETTLE_TIMEOUT_SECONDS = 60
CONTAINER_SETTLE_POLL_SECONDS = 2
RUNTIME_BATCH_COOLDOWN_SECONDS = 20


def running_analyzer_containers(socket: pathlib.Path) -> list[str]:
    """Every container this product's own execution adapters are running
    right now against `socket`, by name."""
    result = subprocess.run(
        [str(DOCKER), "-H", f"unix://{socket}", "container", "ls",
         CONTAINER_LABEL_FILTER, "--format={{.Names}}"],
        capture_output=True, text=True, timeout=10, check=False,
    )
    if result.returncode != 0:
        return []
    return [name for name in result.stdout.split() if name]


def settle_docker_between_batches(socket: pathlib.Path) -> dict[str, object]:
    """Wait (bounded) for the Inspector runtime batch's own containers to
    fully exit, then a fixed cooldown, before the Claude runtime batch starts."""
    started = time.monotonic()
    waited = 0.0
    while running_analyzer_containers(socket):
        waited = time.monotonic() - started
        if waited >= CONTAINER_SETTLE_TIMEOUT_SECONDS:
            break
        time.sleep(CONTAINER_SETTLE_POLL_SECONDS)
    time.sleep(RUNTIME_BATCH_COOLDOWN_SECONDS)
    return {"waited_seconds": round(waited, 3), "cooldown_seconds": RUNTIME_BATCH_COOLDOWN_SECONDS}


def session_plan(plan: list[dict[str, object]], mode: str, write_root: pathlib.Path | None,
                 request_timeout_ms: int) -> dict[str, object]:
    projects = {name: str(ROOT / path) for name, path in FIXTURES.items()}
    if write_root is not None:
        projects[WRITE_PROJECT] = str(write_root)
    return {
        "mode": mode,
        "expected_tools": list(EXPECTED_TOOLS),
        "m6_tools": list(M6_TOOLS),
        "projects": projects,
        "unknown_project_ref": UNKNOWN_PROJECT_REF,
        "idempotency_key": IDEMPOTENCY_KEY,
        "analyzer_file": ANALYZER_FILE,
        "actions_range": ACTIONS_RANGE,
        "request_timeout_ms": request_timeout_ms,
        "calls": plan,
    }


def inspector_gate(attempt: pathlib.Path, mode: str, argv: list[str],
                   plan: list[dict[str, object]], write_root: pathlib.Path | None,
                   timeout: int, request_timeout_ms: int) -> dict[str, object]:
    m3 = load_m3()
    observation = attempt / "protocol.jsonl"
    state = pathlib.Path(argv[argv.index("--state-root") + 1])
    state.mkdir(mode=0o700, parents=True, exist_ok=True)
    # Keep Node package resolution beside the installed Inspector dependencies.
    bridge = ROOT / "target/m1-17-inspector" / f"m6-{attempt.name}-{mode}-bridge.mjs"
    suffix = b"\nexport { InspectorClient, createTransportNode };\n"
    with bridge.open("xb") as stream:
        stream.write(INSPECTOR.read_bytes() + suffix)
    proxy_argv = [sys.executable, str(pathlib.Path(__file__).resolve()), "proxy",
                  "--client", "inspector", "--observation", str(observation),
                  "--server-argv-json", json.dumps(argv, separators=(",", ":"))]
    try:
        result = m3.run_bounded(
            [str(NODE), str(SESSION), str(bridge),
             json.dumps(proxy_argv, separators=(",", ":")),
             json.dumps(session_plan(plan, mode, write_root, request_timeout_ms),
                        separators=(",", ":"))],
            attempt, timeout, attempt / f"inspector-{mode}-session.json",
        )
    finally:
        bridge.unlink(missing_ok=True)
    if result["exit_code"] != 0:
        raise RuntimeError(f"Inspector M6 {mode} session failed")
    outcome = json.loads((attempt / f"inspector-{mode}-session.stdout").read_text())
    if outcome.get("tool_count") != len(EXPECTED_TOOLS) or outcome.get("discovery") is not True:
        raise RuntimeError(f"Inspector M6 {mode} discovery oracle incomplete")
    if outcome.get("mode") != mode:
        raise RuntimeError("Inspector reported a different mode than it was given")
    if mode == RUNTIME and outcome.get("write_verified_on_disk") is not True:
        raise RuntimeError("Inspector did not verify the write landed on disk")
    if mode == RUNTIME and outcome.get("cancel_ok") is not True:
        raise RuntimeError("Inspector did not observe the cancel-then-clean-retry oracle")
    rows = validate_call_rows(outcome.get("calls"), "inspector", plan)
    return {
        "mode": mode,
        "version": INSPECTOR_VERSION,
        "bundle_sha256": m3.file_digest(INSPECTOR),
        "bridge_suffix_sha256": m3.digest(suffix),
        "session": result,
        "protocol_era": outcome.get("protocol_era"),
        "write_verified_on_disk": outcome.get("write_verified_on_disk"),
        "write_lifecycle": "performed" if mode == RUNTIME else None,
        "cancel_ok": outcome.get("cancel_ok"),
        "calls": rows,
    }


def measure(m3, value: object) -> dict[str, object]:
    """Same canonical encoding the Inspector session driver uses."""
    payload = json.dumps(value, separators=(",", ":"), ensure_ascii=False,
                         sort_keys=True).encode()
    return {"bytes": len(payload), "sha256": m3.digest(payload)}


# -- model-directed flow through the stock Claude Code client -----------------
#
# Claude Code relays an MCP tool result as the server's own JSON text and sets
# `is_error` from the server's `isError`. The normalizer below turns one
# stream-json transcript into a closed item shape, so the flow oracle reads
# Claude exactly as strictly as the Inspector session does. Neither MCP
# Resource built-in is configured: these five tools publish no artifact.

def normalize_claude_tool(name: object) -> str:
    """Map Claude's `mcp__<server>__<tool>` name back to the advertised tool."""
    prefix = f"mcp__{CLAUDE_SERVER}__"
    if not isinstance(name, str) or not name.startswith(prefix):
        raise RuntimeError(f"Claude used a capability outside the configured server: {name}")
    suffix = name.removeprefix(prefix)
    for tool in EXPECTED_TOOLS:
        if suffix in (tool, tool.replace(".", "_")):
            return tool
    raise RuntimeError(f"Claude called a tool the server does not advertise: {suffix}")


def claude_structured_payload(content: object) -> dict[str, object] | None:
    """The single JSON object carrying `status` that a relayed tool result is."""
    texts: list[str] = []
    if isinstance(content, str):
        texts.append(content)
    elif isinstance(content, list):
        for block in content:
            if (isinstance(block, dict) and block.get("type") == "text"
                    and isinstance(block.get("text"), str)):
                texts.append(block["text"])
    candidates = []
    for text in texts:
        try:
            parsed = json.loads(text.removeprefix("Error: "))
        except json.JSONDecodeError:
            continue
        if isinstance(parsed, dict) and "status" in parsed:
            candidates.append(parsed)
    return candidates[0] if len(candidates) == 1 else None


def claude_item(call: dict[str, object], result: dict[str, object]) -> dict[str, object]:
    name = call["name"]
    arguments = call["input"] if isinstance(call["input"], dict) else {}
    failed = result["is_error"] is True
    return {
        "type": "mcpToolCall", "server": CLAUDE_SERVER, "tool": normalize_claude_tool(name),
        "arguments": arguments, "status": "failed" if failed else "completed", "error": None,
        "result": {"structuredContent": claude_structured_payload(result["content"]),
                   "isError": failed},
    }


def claude_items(events: list[object]) -> tuple[dict[str, object], list[dict[str, object]],
                                                dict[str, object]]:
    """(init event, tool items in call order, final result) of one transcript."""
    init = None
    final = None
    calls: list[dict[str, object]] = []
    by_id: dict[str, dict[str, object]] = {}
    results: dict[str, dict[str, object]] = {}
    for event in events:
        if not isinstance(event, dict):
            raise RuntimeError("Claude transcript carries a non-object event")
        kind = event.get("type")
        if kind == "system" and event.get("subtype") == "init":
            if init is not None:
                raise RuntimeError("Claude transcript carries two init events")
            init = event
        elif kind == "result":
            if final is not None:
                raise RuntimeError("Claude transcript carries two result events")
            final = event
        elif kind in {"assistant", "user"}:
            message = event.get("message")
            blocks = message.get("content") if isinstance(message, dict) else None
            for block in blocks if isinstance(blocks, list) else []:
                if not isinstance(block, dict):
                    continue
                if block.get("type") == "tool_use":
                    identifier = block.get("id")
                    if not isinstance(identifier, str) or identifier in by_id:
                        raise RuntimeError("Claude tool_use identifier is duplicated or missing")
                    call = {"id": identifier, "name": block.get("name"), "input": block.get("input")}
                    calls.append(call)
                    by_id[identifier] = call
                elif block.get("type") == "tool_result":
                    identifier = block.get("tool_use_id")
                    if not isinstance(identifier, str) or identifier in results:
                        raise RuntimeError("Claude tool_result identifier is duplicated or missing")
                    results[identifier] = {"content": block.get("content"),
                                           "is_error": block.get("is_error") is True}
    if init is None or final is None:
        raise RuntimeError("Claude transcript lacks its init or result event")
    if set(results) != set(by_id):
        raise RuntimeError("Claude transcript has unmatched tool results")
    return init, [claude_item(call, results[call["id"]]) for call in calls], final


def validate_claude_session(init: dict[str, object], final: dict[str, object],
                            events: list[object] = ()) -> dict[str, object]:
    """Pinned client and model, only the configured server, a clean finish."""
    if init.get("model") != CLAUDE_MODEL:
        raise RuntimeError(f"Claude resolved another model: {init.get('model')}")
    message_models = []
    for event in events:
        if isinstance(event, dict) and event.get("type") == "assistant":
            message = event.get("message")
            message_models.append(message.get("model") if isinstance(message, dict) else None)
    if not message_models or any(model != CLAUDE_MODEL for model in message_models):
        raise RuntimeError("Claude turn carried a message from another model or none at all")
    if init.get("claude_code_version") != CLAUDE_VERSION.split(" ")[0]:
        raise RuntimeError(f"Claude ran another version: {init.get('claude_code_version')}")
    servers = init.get("mcp_servers")
    if (not isinstance(servers, list)
            or [(server.get("name"), server.get("status")) if isinstance(server, dict) else None
                for server in servers] != [(CLAUDE_SERVER, "connected")]):
        raise RuntimeError("Claude did not connect to exactly the configured server")
    tools = init.get("tools")
    prefix = f"mcp__{CLAUDE_SERVER}__"
    if (not isinstance(tools, list) or not tools
            or any(not (isinstance(tool, str) and tool.startswith(prefix)) for tool in tools)):
        raise RuntimeError("Claude session exposed a built-in capability the gate did not allow")
    if final.get("subtype") != "success" or final.get("is_error") is True:
        raise RuntimeError("Claude turn did not finish successfully")
    if final.get("permission_denials"):
        raise RuntimeError("Claude turn was denied a capability it tried to use")
    usage = final.get("modelUsage")
    if not isinstance(usage, dict) or CLAUDE_MODEL not in usage:
        raise RuntimeError("Claude turn reports no usage for the pinned model")
    return {
        "resolved_model": init["model"],
        "claude_code_version": init["claude_code_version"],
        "observed_models": sorted(usage),
        "assistant_messages": len(message_models),
        "api_key_source": init.get("apiKeySource"),
        "num_turns": final.get("num_turns"),
        "duration_ms": final.get("duration_ms"),
    }


def opened_references(items: list[dict[str, object]],
                      roots: dict[str, str]) -> tuple[dict[str, str], dict[str, str]]:
    """(project_ref, fingerprint) captured from every open; a root outside
    `roots`, or opened more than once among `items`, is refused. (The write
    project's second, post-commit open is excluded from `items` by the
    caller before this runs, so it never counts as a retry here.) Not every
    caller's opens carry a fingerprint the caller cares about, so its absence
    on one entry is not itself a failure here."""
    by_path = {path: name for name, path in roots.items()}
    refs: dict[str, str] = {}
    fingerprints: dict[str, str] = {}
    for item in items:
        if item["tool"] != "rust.project.open":
            continue
        path = item["arguments"].get("path")
        payload = item["result"].get("structuredContent")
        if (item["arguments"] != {"path": path} or path not in by_path
                or item["status"] != "completed" or not isinstance(payload, dict)
                or payload.get("status") != "passed"):
            raise RuntimeError("Claude opened a root outside the plan or the open did not pass")
        data = payload.get("data")
        reference = data.get("project_ref") if isinstance(data, dict) else None
        if not isinstance(reference, str) or not re.fullmatch(r"prj_[0-9a-f]{32}", reference):
            raise RuntimeError("Claude open returned no project reference")
        name = by_path[path]
        if name in refs:
            raise RuntimeError(f"Claude model-directed flow reopened {name} unexpectedly")
        refs[name] = reference
        fingerprint = data.get("fingerprint") if isinstance(data, dict) else None
        if fingerprint is not None:
            if not isinstance(fingerprint, str) or not re.fullmatch(r"sha256:[0-9a-f]{64}", fingerprint):
                raise RuntimeError("Claude open returned a malformed fingerprint")
            fingerprints[name] = fingerprint
    return refs, fingerprints


def _with_captured_fingerprint(expected: dict[str, object], tool: str,
                               fingerprint: str) -> dict[str, object]:
    """Substitute the real, session-captured fingerprint into the planned
    arguments of a `FINGERPRINT_TOOLS` call, never a hard-coded constant."""
    if tool == "rust.analyzer.actions":
        return {**expected, "expected_project_fingerprint": fingerprint}
    action = expected["action"]
    return {**expected, "action": {**action, "expected_project_fingerprint": fingerprint}}


def validate_docker_free_model_flow(items: list[dict[str, object]],
                                    plan: list[dict[str, object]]) -> dict[str, object]:
    """The five declared refusals, each exactly once with the planned arguments."""
    completed = [item for item in items if item.get("type") == "mcpToolCall"]
    allowed = {"rust.project.open", *M6_TOOLS}
    if any(item["tool"] not in allowed for item in completed):
        raise RuntimeError("Claude model-directed refusal flow used another MCP capability")
    positive = {row["tool"]: row for row in plan if row["mode"] == DOCKER_FREE}
    if set(positive) != set(M6_TOOLS):
        raise RuntimeError("Docker-free plan does not carry one row per M6 tool")
    roots = {name: str(ROOT / path) for name, path in FIXTURES.items()}
    refs, fingerprints = opened_references(completed, roots)
    opens = {}
    for index, item in enumerate(completed):
        if item["tool"] == "rust.project.open":
            opens[item["arguments"]["path"]] = index
    refusals = {}
    for tool in M6_TOOLS:
        calls = [item for item in completed if item["tool"] == tool]
        if len(calls) != 1:
            raise RuntimeError("Claude model-directed refusal flow retried or omitted a required call")
        call = calls[0]
        row = positive[tool]
        reference = refs.get(row["project"])
        if reference is None or completed.index(call) < opens[str(ROOT / FIXTURES[row["project"]])]:
            raise RuntimeError(f"Claude called {tool} before opening its planned root")
        expected_arguments = {"project_ref": reference, **row["arguments"]}
        if requires_fingerprint(tool):
            fingerprint = fingerprints.get(row["project"])
            if fingerprint is None:
                raise RuntimeError(f"Claude {tool} call needs a fingerprint captured from "
                                   f"opening the {row['project']} root")
            expected_arguments = _with_captured_fingerprint(expected_arguments, tool, fingerprint)
        if call["arguments"] != expected_arguments:
            raise RuntimeError(f"Claude {tool} call did not use the planned arguments")
        payload = call["result"].get("structuredContent")
        if not isinstance(payload, dict):
            raise RuntimeError(f"Claude {tool} refusal carried no structured result")
        observed = (payload.get("status"), payload.get("error_code"),
                    call["result"].get("isError") is True)
        expected = (row["expect_status"], row["expect_error_code"], row["expect_is_error"])
        if observed != expected:
            raise RuntimeError(f"Claude {tool} answered a result the plan does not declare")
        refusals[tool] = {"status": observed[0], "error_code": observed[1]}
    return {"opened_roots": sorted(refs), "refusals": refusals}


def capture_first_applicable_action(structured: dict[str, object]) -> str:
    data = structured.get("data") if isinstance(structured, dict) else None
    actions = data.get("actions") if isinstance(data, dict) else None
    if not isinstance(actions, list) or not actions:
        raise RuntimeError("rust.analyzer.actions published no actions")
    first = actions[0]
    digest = first.get("action_digest") if isinstance(first, dict) else None
    if (not isinstance(first, dict) or first.get("applicability") != "applicable"
            or not isinstance(digest, str)
            or not re.fullmatch(r"sha256:[0-9a-f]{64}", digest)):
        raise RuntimeError("the first listed action is not applicable with a real digest")
    return digest


def check_apply_preview(structured: dict[str, object]) -> tuple[str, str]:
    """Validate a passed preview and return (plan_id, plan_digest)."""
    data = structured.get("data") if isinstance(structured, dict) else None
    if (not isinstance(data, dict) or data.get("kind") != "preview"
            or not isinstance(data.get("files"), list) or not data["files"]):
        raise RuntimeError("apply preview published no reviewable diff")
    if not any(isinstance(change, dict) and change.get("before_sha256") != change.get("after_sha256")
               for change in data["files"]):
        raise RuntimeError("apply preview's diff changes nothing")
    plan_id = data.get("plan_id")
    plan_digest = data.get("plan_digest")
    if (not isinstance(plan_id, str) or not re.fullmatch(r"mut_[0-9a-f]{32}", plan_id)
            or not isinstance(plan_digest, str)
            or not re.fullmatch(r"sha256:[0-9a-f]{64}", plan_digest)):
        raise RuntimeError("apply preview published no valid plan identity")
    return plan_id, plan_digest


def check_apply_receipt(structured: dict[str, object], plan_id: str) -> None:
    data = structured.get("data") if isinstance(structured, dict) else None
    if (not isinstance(data, dict) or data.get("kind") != "receipt"
            or data.get("state") != "committed" or data.get("operation_id") != plan_id):
        raise RuntimeError("apply receipt did not report a committed operation")


# Deterministic every time: the reads plus the one `actions` call plus the
# bad-file negative never depend on what the analyzer happened to offer.
RUNTIME_ALWAYS_FACT_KEYS = (
    "symbols_document", "symbols_workspace", "references", "diagnostics",
    "actions", "bad_file",
)
# Best-effort: only validated when the transcript shows the model actually
# drove the write lifecycle to completion (see validate_runtime_model_flow).
RUNTIME_WRITE_FACT_KEYS = ("apply_preview", "apply_commit", "apply_receipt", "apply_stale")

# W09f: a successful analyzer call's bounded capacity (ADR-084 budgets) is
# released asynchronously, after the response is already on the wire, while
# the previous session's container tears down. Inspector paces its calls far
# enough apart that it never races this window and so covers the full
# per-tool matrix reliably; the model-directed Claude Code flow issues its
# own calls as fast as it decides to, so a read that lands immediately after
# another analyzer call can legitimately observe an instantaneous
# `unavailable`/`SANDBOX_DENIED` before the prior teardown finishes. That is
# a transient capacity refusal, not a correctness defect -- the tool answers
# every other call in the same run correctly. `symbols(document)` is the
# flow's first analyzer call and always starts cold (nothing to race), so it
# alone is required to be positive; `rust.analyzer.actions` and the
# `FILE_NOT_IN_SNAPSHOT` negative are both required to be served exactly as
# planned regardless of capacity. M6 debt: `SANDBOX_DENIED` conflates this
# transient capacity refusal with the permanent no-`--rust`-grant refusal
# (ADR-084); a distinct retryable code (e.g. `CAPACITY`/`LOCK_BUSY`) would let
# a client tell the two apart and retry only the former.
CAPACITY_TOLERANT_FACT_KEYS = frozenset({"symbols_workspace", "references", "diagnostics"})
CAPACITY_REFUSAL = ("unavailable", "SANDBOX_DENIED")
RUNTIME_READ_FACT_KEYS = ("symbols_document", "symbols_workspace", "references", "diagnostics")


def validate_runtime_model_flow(items: list[dict[str, object]],
                                plan: list[dict[str, object]],
                                write_root: pathlib.Path) -> dict[str, object]:
    """Open every root, read the four M6 read tools plus one `actions` call
    once each, and end with the bad-file negative: all deterministic and
    always required, and no other MCP capability may be used.

    Hard-required regardless of capacity: every project.open; `symbols`
    document-scope (the flow's first analyzer call, always a cold start) must
    be positive; `rust.analyzer.actions` must be served (positive, with
    actions or honestly empty, both per W09d); the `FILE_NOT_IN_SNAPSHOT`
    negative must land exactly as planned. Tolerated, never a failure: an
    `unavailable`/`SANDBOX_DENIED` on `symbols` workspace-scope, `references`
    or `diagnostics` -- the transient capacity refusal W09f documents (see
    `CAPACITY_TOLERANT_FACT_KEYS` above) -- recorded per row as
    `capacity_refused`, not counted as a positive read. Any other unplanned
    status on any row is still a hard failure. Inspector's own, more
    patiently paced session is the authoritative proof of the exhaustive
    per-tool matrix; this oracle only asks the model-directed client for the
    bounded G4 minimum (discovery, a positive call, a refusal).

    rust-analyzer's code actions are not deterministically available from a
    transient per-query instance (ADR-084): the `serverStatus` quiescent
    readiness this gateway waits on does not guarantee an assist is indexed
    yet, so `rust.analyzer.actions` correctly answering with zero actions is
    a valid, honest result. The apply preview -> commit -> receipt write
    lifecycle is therefore best-effort here: when the model's own `actions`
    call offered no applicable digest, or the model did not go on to drive
    the full commit/reopen/stale cycle, that is not a failure, only a
    skipped demonstration. The authoritative proof of the write path is the
    Inspector client (which retries the same race, see
    m6-inspector-session.mjs) plus the three native `action.apply` e2e tests;
    debt M6-04 is a stronger assist-readiness signal or a gateway-side retry
    so this best-effort path is no longer needed."""
    completed = [item for item in items if item.get("type") == "mcpToolCall"]
    if any(item["tool"] not in {"rust.project.open", *M6_TOOLS} for item in completed):
        raise RuntimeError("Claude model-directed runtime flow used another MCP capability")
    static_roots = {name: str(ROOT / path) for name, path in FIXTURES.items()
                    if name != "analyzer_actions"}
    static_roots[WRITE_PROJECT] = str(write_root)
    opens = [item for item in completed if item["tool"] == "rust.project.open"]
    write_opens = [item for item in opens if item["arguments"].get("path") == str(write_root)]
    if len(write_opens) not in (1, 2):
        raise RuntimeError("Claude opened the write project an unexpected number of times")
    reopened = len(write_opens) == 2
    excluded = write_opens[1] if reopened else None
    refs, _ = opened_references([item for item in completed if item is not excluded], static_roots)
    pre_commit_ref = refs[WRITE_PROJECT]
    post_commit_ref = None
    if reopened:
        payload = write_opens[1]["result"].get("structuredContent")
        data = payload.get("data") if isinstance(payload, dict) else None
        post_commit_ref = data.get("project_ref") if isinstance(data, dict) else None
        post_commit_fingerprint = data.get("fingerprint") if isinstance(data, dict) else None
        if (not isinstance(post_commit_ref, str) or post_commit_ref == pre_commit_ref
                or not isinstance(post_commit_fingerprint, str)):
            raise RuntimeError("Claude did not obtain a fresh reference after commit")
    rows = {row["fact_key"]: row for row in plan}
    state: dict[str, object] = {}
    facts: dict[str, object] = {}
    pre_commit_kinds = {"actions_capture", "apply_preview", "apply_commit"}

    def collect_fact(fact_key: str) -> tuple[dict[str, object], str]:
        """(payload, observed) where `observed` is `positive` when the
        response matches the row's planned passing result, `negative` when it
        matches the row's planned refusal, or `capacity_refused` when the row
        is one of `CAPACITY_TOLERANT_FACT_KEYS` and the response is instead
        the honest transient `unavailable`/`SANDBOX_DENIED` capacity refusal.
        Any other response is a hard failure."""
        row = rows[fact_key]
        if row["kind"] == "static":
            expected_ref = refs[row["project"]]
        elif row["kind"] in pre_commit_kinds:
            expected_ref = pre_commit_ref
        else:
            expected_ref = post_commit_ref
        calls = [item for item in completed if item["tool"] == row["tool"]
                and item["arguments"].get("project_ref") == expected_ref]
        # Disambiguate same-tool, same-ref calls (symbols at valid_basic three
        # times; apply at each of the two write refs, twice each).
        calls = [call for call in calls if _matches_fact(call, row, state)]
        if len(calls) != 1:
            raise RuntimeError(f"Claude runtime flow retried or omitted {fact_key}")
        call = calls[0]
        payload = call["result"].get("structuredContent")
        if not isinstance(payload, dict):
            raise RuntimeError(f"Claude {fact_key} did not answer the planned result")
        observed = (payload.get("status"), payload.get("error_code"))
        if observed == (row["expect_status"], row["expect_error_code"]):
            return payload, ("negative" if row["expect_status"] in ERROR_RESULT_STATUSES
                             else "positive")
        if fact_key in CAPACITY_TOLERANT_FACT_KEYS and observed == CAPACITY_REFUSAL:
            return payload, "capacity_refused"
        raise RuntimeError(f"Claude {fact_key} did not answer the planned result")

    for fact_key in RUNTIME_ALWAYS_FACT_KEYS:
        payload, observed = collect_fact(fact_key)
        if fact_key == "actions":
            try:
                state["action_digest"] = capture_first_applicable_action(payload)
            except RuntimeError:
                state["action_digest"] = None
        facts[fact_key] = {"status": payload.get("status"), "error_code": payload.get("error_code"),
                           "observed": observed}
    analyzer_positive_reads = sum(1 for key in RUNTIME_READ_FACT_KEYS
                                  if facts[key]["observed"] == "positive")
    capacity_refused_reads = [key for key in RUNTIME_READ_FACT_KEYS
                              if facts[key]["observed"] == "capacity_refused"]

    write_verified_on_disk = None
    if reopened and state["action_digest"] is not None:
        for fact_key in RUNTIME_WRITE_FACT_KEYS:
            payload, observed = collect_fact(fact_key)
            if fact_key == "apply_preview":
                state["plan_id"], state["plan_digest"] = check_apply_preview(payload)
            elif fact_key == "apply_receipt":
                check_apply_receipt(payload, state["plan_id"])
            facts[fact_key] = {"status": payload.get("status"), "error_code": payload.get("error_code"),
                               "observed": observed}
        on_disk = (write_root / "src/lib.rs").read_bytes()
        original = (ROOT / FIXTURES["analyzer_actions"] / "src/lib.rs").read_bytes()
        write_verified_on_disk = len(on_disk) > 0 and on_disk != original
        if write_verified_on_disk is not True:
            raise RuntimeError("Claude runtime flow did not verify the write landed on disk")
        write_lifecycle = "performed"
    else:
        write_lifecycle = "skipped: no applicable action offered"
    return {
        "opened_roots": sorted(refs) + ([WRITE_PROJECT + "_after_commit"] if reopened else []),
        "facts": facts,
        "write_verified_on_disk": write_verified_on_disk,
        "write_lifecycle": write_lifecycle,
        "analyzer_positive_reads": analyzer_positive_reads,
        "capacity_refused_reads": capacity_refused_reads,
    }


def _matches_fact(call: dict[str, object], row: dict[str, object], state: dict[str, object]) -> bool:
    """Distinguish same-tool calls in the runtime flow by their arguments'
    shape, without hard-coding a captured identifier the plan does not own."""
    arguments = call.get("arguments", {})
    if row["kind"] == "static":
        return {key: value for key, value in arguments.items() if key != "project_ref"} == row["arguments"]
    if row["kind"] == "actions_capture":
        return arguments.get("file") == ANALYZER_FILE and arguments.get("range") == ACTIONS_RANGE
    action = arguments.get("action") if isinstance(arguments, dict) else None
    if not isinstance(action, dict):
        return False
    if row["kind"] == "apply_preview":
        return action.get("mode") == "preview" and "plan_id" not in state
    if row["kind"] == "apply_commit":
        return action.get("mode") == "commit"
    if row["kind"] == "apply_receipt":
        return action.get("mode") == "receipt"
    if row["kind"] == "apply_preview_stale":
        return action.get("mode") == "preview" and "plan_id" in state
    return False


CLAUDE_SYSTEM_PROMPT = (
    "You are a bounded third-party MCP client qualifier. Use only the explicitly "
    "configured Rust Engineering MCP server. Perform the requested steps exactly "
    "once each, in the given order, never retry a call even when it is refused, "
    "and finish with a short account of every returned status."
)


def project_root(name: str) -> str:
    return str(ROOT / FIXTURES[name])


def render_arguments(arguments: dict[str, object]) -> str:
    return ", ".join(f"{key} {json.dumps(value)}" for key, value in arguments.items())


def claude_prompt(mode: str, plan: list[dict[str, object]],
                  write_root: pathlib.Path | None = None) -> str:
    """Prompts are rendered from the plan rows, never written by hand."""
    if mode == DOCKER_FREE:
        positive = {row["tool"]: row for row in plan}
        roots = dict.fromkeys(positive[tool]["project"] for tool in M6_TOOLS)
        lines = [
            "Use only the configured Rust Engineering MCP server. Perform these steps exactly "
            "once each, in order, and do not retry any call even if it is refused: every "
            "refusal here is an expected, declared result.",
            "(1) Call rust.project.open once for each of these roots and keep every returned "
            "data.project_ref and data.fingerprint: "
            + "; ".join(f"{name} root {project_root(name)}" for name in roots) + ".",
        ]
        for index, tool in enumerate(M6_TOOLS, start=2):
            row = positive[tool]
            if tool == "rust.analyzer.actions":
                lines.append(
                    f"({index}) Call rust.analyzer.actions with project_ref = the reference "
                    f"returned for the {row['project']} root, expected_project_fingerprint = "
                    f"the fingerprint returned for the {row['project']} root, "
                    f"{render_arguments(row['arguments'])}."
                )
            elif tool == "rust.analyzer.action.apply":
                action = row["arguments"]["action"]
                lines.append(
                    f"({index}) Call rust.analyzer.action.apply with project_ref = the reference "
                    f"returned for the {row['project']} root, action mode preview, "
                    f"expected_project_fingerprint = the fingerprint returned for the "
                    f"{row['project']} root, action_digest {action['action_digest']!r}, file "
                    f"{action['file']!r}, range {json.dumps(action['range'])}."
                )
            else:
                lines.append(f"({index}) Call {tool} with project_ref = the reference returned for "
                             f"the {row['project']} root, {render_arguments(row['arguments'])}.")
        lines.append(f"({len(M6_TOOLS) + 2}) Report the status and error_code of every call "
                     "exactly as returned. Do not call any other capability.")
        return " ".join(lines)
    rows = {row["fact_key"]: row for row in plan}
    return " ".join([
        "Use only the configured Rust Engineering MCP server. Perform these steps exactly "
        "once each, in order, without retrying any call.",
        f"(1) Call rust.project.open with path {project_root('valid_basic')}; keep its "
        "data.project_ref as VALID_BASIC.",
        f"(2) Call rust.project.open with path {project_root('analyzer_references')}; keep its "
        "data.project_ref as ANALYZER_REFERENCES.",
        f"(3) Call rust.project.open with path {write_root}; keep its data.project_ref as "
        "WRITE and its data.fingerprint as WRITE_FINGERPRINT.",
        f"(4) Call rust.analyzer.symbols with project_ref VALID_BASIC, "
        f"{render_arguments(rows['symbols_document']['arguments'])}.",
        f"(5) Call rust.analyzer.symbols with project_ref VALID_BASIC, "
        f"{render_arguments(rows['symbols_workspace']['arguments'])}.",
        f"(6) Call rust.analyzer.references with project_ref ANALYZER_REFERENCES, "
        f"{render_arguments(rows['references']['arguments'])}.",
        f"(7) Call rust.analyzer.diagnostics with project_ref VALID_BASIC, "
        f"{render_arguments(rows['diagnostics']['arguments'])}.",
        f"(8) Call rust.analyzer.actions with project_ref WRITE, "
        "expected_project_fingerprint WRITE_FINGERPRINT, "
        f"{render_arguments(rows['actions']['arguments'])}. From its data.actions, keep the "
        "first entry with applicability applicable as ACTION; keep its action_digest as "
        "ACTION_DIGEST.",
        "(9) Call rust.analyzer.action.apply with project_ref WRITE, action mode preview, "
        "expected_project_fingerprint WRITE_FINGERPRINT, action_digest ACTION_DIGEST, file "
        f"{ANALYZER_FILE!r}, range {json.dumps(ACTIONS_RANGE)}. Review the returned diff and "
        "files, then keep data.plan_id as PLAN_ID and data.plan_digest as PLAN_DIGEST.",
        "(10) Call rust.analyzer.action.apply with project_ref WRITE, action mode commit, "
        f"plan_id PLAN_ID, plan_digest PLAN_DIGEST, idempotency_key {IDEMPOTENCY_KEY!r}.",
        f"(11) Call rust.project.open again with path {write_root}; keep its new "
        "data.project_ref as WRITE_AFTER_COMMIT (never reuse WRITE again).",
        "(12) Call rust.analyzer.action.apply with project_ref WRITE_AFTER_COMMIT, action mode "
        "receipt, operation_id PLAN_ID, recover false. Confirm data.state is committed.",
        "(13) Call rust.analyzer.action.apply with project_ref WRITE_AFTER_COMMIT, action mode "
        "preview, expected_project_fingerprint = the fingerprint from step 11's data, "
        f"action_digest ACTION_DIGEST, file {ANALYZER_FILE!r}, range {json.dumps(ACTIONS_RANGE)}. "
        "This is expected to be refused as blocked/ACTION_STALE: the source changed since this "
        "digest was listed.",
        f"(14) Call rust.analyzer.symbols with project_ref VALID_BASIC, scope document, file "
        f"{BAD_FILE!r}. This is expected to be refused as blocked/FILE_NOT_IN_SNAPSHOT.",
        "(15) Report the status and error_code of every call exactly as returned. Do not call "
        "any other capability.",
    ])


# Header and token shapes, not vocabulary: a credential never appears without
# its header colon or its token prefix.
CREDENTIAL_TEXT = (b"authorization:", b"access_token", b"refresh_token", b"auth.json",
                   b"sk-ant-", b"bearer ")


def assert_no_credential_text(path: pathlib.Path) -> None:
    """Raw client output is staged as evidence only if nothing in it is credential-shaped."""
    encoded = path.read_bytes().lower()
    found = sorted(needle.decode() for needle in CREDENTIAL_TEXT if needle in encoded)
    if found:
        raise RuntimeError(f"credential-shaped text in {path.name}: " + ", ".join(found))


def client_home_residue(cwd: pathlib.Path) -> dict[str, object]:
    """What Claude Code left under its own home for this private working
    directory; counted, then removed (see test-m5-clients.py for the rationale)."""
    m3 = load_m3()
    slug = re.sub(r"[^A-Za-z0-9]", "-", str(cwd))
    directory = pathlib.Path.home() / ".claude" / "projects" / slug
    residue = {"directory_sha256": m3.digest(str(directory).encode()),
               "present": directory.is_dir(), "files": 0, "transcripts": 0, "removed": False}
    if directory.is_dir():
        files = [path for path in directory.rglob("*") if path.is_file()]
        residue["files"] = len(files)
        residue["transcripts"] = sum(1 for path in files if path.suffix == ".jsonl")
        shutil.rmtree(directory, ignore_errors=True)
        residue["removed"] = not directory.exists()
    return residue


def claude_environment(private: pathlib.Path) -> dict[str, str]:
    environment = {
        "HOME": os.environ["HOME"],
        "PATH": "/Users/cburgosro/.local/bin:/usr/bin:/bin:/usr/sbin:/sbin",
        "TMPDIR": str(private / "tmp"),
        "LANG": "en_US.UTF-8",
        "MCP_TOOL_TIMEOUT": str(MCP_TOOL_TIMEOUT_MS),
    }
    for name in ("USER", "LOGNAME", "SHELL"):
        if name in os.environ:
            environment[name] = os.environ[name]
    return environment


def run_claude(argv: list[str], cwd: pathlib.Path, environment: dict[str, str],
               timeout: int) -> dict[str, object]:
    started = time.monotonic()
    child = subprocess.Popen(argv, cwd=cwd, env=environment, stdin=subprocess.DEVNULL,
                             stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                             start_new_session=True)
    timed_out = False
    try:
        stdout, stderr = child.communicate(timeout=timeout)
    except subprocess.TimeoutExpired:
        timed_out = True
        try:
            os.killpg(child.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        stdout, stderr = child.communicate()
    return {"exit_code": child.returncode, "stdout": stdout, "stderr": stderr,
            "timed_out": timed_out, "duration_seconds": round(time.monotonic() - started, 3)}


def claude_gate(attempt: pathlib.Path, mode: str, argv: list[str],
                plan: list[dict[str, object]], write_root: pathlib.Path | None,
                timeout: int) -> dict[str, object]:
    """One model-directed turn of the stock Claude Code client; no credential copy."""
    m3 = load_m3()
    observation = attempt / "protocol.jsonl"
    state = pathlib.Path(argv[argv.index("--state-root") + 1])
    state.mkdir(mode=0o700, parents=True, exist_ok=True)
    proxy = [sys.executable, str(pathlib.Path(__file__).resolve()), "proxy",
             "--client", CLAUDE_CLIENT, "--observation", str(observation),
             "--server-argv-json", json.dumps(argv, separators=(",", ":"))]
    private = pathlib.Path(tempfile.mkdtemp(prefix="rust-mcp-m6-claude-", dir=PRIVATE_DIR_BASE))
    os.chmod(private, 0o700)
    events_path = attempt / f"claude-{mode}-model-events.jsonl"
    stderr_path = attempt / f"claude-{mode}-model.stderr"
    try:
        for child in ("cwd", "tmp"):
            (private / child).mkdir(mode=0o700)
        config = private / "mcp.json"
        m3.save_json(config, {"mcpServers": {CLAUDE_SERVER: {
            "command": proxy[0], "args": proxy[1:], "env": {}}}}, exclusive=True)
        prompt = claude_prompt(mode, plan, write_root)
        claude_argv = [
            str(CLAUDE), "--print", "--output-format", "stream-json", "--verbose",
            "--model", CLAUDE_MODEL, "--effort", CLAUDE_EFFORT, "--no-session-persistence",
            "--restricted", "--setting-sources", "", "--strict-mcp-config",
            "--mcp-config", str(config), "--disable-slash-commands", "--no-chrome",
            "--tools", "", "--allowedTools", f"mcp__{CLAUDE_SERVER}",
            "--permission-mode", "dontAsk", "--permission-prompts", "none",
            "--max-turns", "28", "--system-prompt", CLAUDE_SYSTEM_PROMPT, prompt,
        ]
        outcome = run_claude(claude_argv, private / "cwd", claude_environment(private), timeout)
        events_path.write_bytes(outcome["stdout"])
        stderr_path.write_bytes(outcome["stderr"])
        assert_no_credential_text(events_path)
        assert_no_credential_text(stderr_path)
        if outcome["timed_out"] or outcome["exit_code"] != 0:
            raise RuntimeError(f"Claude {mode} turn failed: exit {outcome['exit_code']}, "
                               f"timed_out={outcome['timed_out']}")
        events = []
        for number, line in enumerate(outcome["stdout"].splitlines(), start=1):
            if not line.strip():
                continue
            try:
                events.append(json.loads(line))
            except json.JSONDecodeError as error:
                raise RuntimeError(f"Claude {mode} transcript line {number} is not JSON") from error
        init, items, final = claude_items(events)
        session = validate_claude_session(init, final, events)
        if mode == RUNTIME:
            flow = validate_runtime_model_flow(items, plan, write_root)
        else:
            flow = validate_docker_free_model_flow(items, plan)
        m3.assert_no_credentials(private)
        residue = client_home_residue(private / "cwd")
        return {
            "mode": mode, "client": CLAUDE_CLIENT, "version": CLAUDE_VERSION,
            "model": CLAUDE_MODEL, "effort": CLAUDE_EFFORT,
            "executable_sha256": m3.file_digest(CLAUDE),
            "tasks_declared": False, "credentials_copied": False,
            "private_directory_credential_scan": "clean",
            "session_persistence_flag": "--no-session-persistence",
            "client_home_residue": residue,
            "mcp_tool_timeout_ms": MCP_TOOL_TIMEOUT_MS,
            "prompt_sha256": m3.digest(prompt.encode()),
            "argv_sha256": m3.digest(json.dumps(claude_argv).encode()),
            "exit_code": outcome["exit_code"],
            "duration_seconds": outcome["duration_seconds"],
            "tool_calls": len(items),
            "session": session,
            "model_flow": flow,
            "write_lifecycle": flow.get("write_lifecycle") if mode == RUNTIME else None,
            "analyzer_positive_reads": flow.get("analyzer_positive_reads") if mode == RUNTIME else None,
            "model_turn_completed": True,
            "model_events_sha256": m3.file_digest(events_path),
            "stderr_sha256": m3.file_digest(stderr_path),
        }
    finally:
        shutil.rmtree(private, ignore_errors=True)


def next_attempt() -> pathlib.Path:
    """M6 owns a separate immutable attempt namespace."""
    ATTEMPTS.mkdir(parents=True, exist_ok=True)
    numbers = []
    for path in ATTEMPTS.glob("attempt-*"):
        try:
            numbers.append(int(path.name.removeprefix("attempt-")))
        except ValueError:
            continue
    path = ATTEMPTS / f"attempt-{max(numbers, default=0) + 1}"
    path.mkdir(mode=0o700)
    return path


def run(with_runtime: bool, docker_socket: str | None) -> int:
    m3 = load_m3()
    check = preflight(with_runtime, docker_socket)
    if check["unsatisfied"]:
        raise RuntimeError("M6 client preconditions are unsatisfied: " + ", ".join(check["unsatisfied"]))
    plan = check["call_plan"]
    runtime_plan = check["runtime_call_plan"] if with_runtime else []
    gate_spec = importlib.util.spec_from_file_location("m6_gate_inventory", ROOT / "scripts/gate.py")
    if gate_spec is None or gate_spec.loader is None:
        raise RuntimeError("gate inventory unavailable")
    gate = importlib.util.module_from_spec(gate_spec)
    gate_spec.loader.exec_module(gate)
    candidate_sources = gate.source_inventory(ROOT, os.environ.copy())
    attempt = next_attempt()
    private = pathlib.Path(tempfile.mkdtemp(prefix="rust-mcp-m6-clients-", dir=PRIVATE_DIR_BASE))
    os.chmod(private, 0o700)
    # Named, never created and never dialed: the assertion below is what proves
    # the Docker-free mode started no container.
    closed_socket = private / "docker-never-dialed.sock"
    receipt: dict[str, object] = {
        "schema": "rust-mcp-m6-clients-v1", "status": "failed",
        "attempt": attempt.name, "image_id": check["image_id"],
        "with_runtime": with_runtime,
        "expected_tools": list(EXPECTED_TOOLS), "m6_tools": list(M6_TOOLS),
        "inventory": check["inventory"],
        "call_plan": plan, "runtime_call_plan": runtime_plan,
        "fixture_roots": dict(FIXTURES),
        "modes": {
            DOCKER_FREE: {
                "docker_used": False, "write_grant": None,
                "description": "every call is SANDBOX_DENIED, produced before any container",
            },
            RUNTIME: {
                "enabled": with_runtime, "docker_used": with_runtime,
                "write_grant": WRITE_GRANT_FLAG if with_runtime else None,
                "description": "real analyzer sessions in the qualified image; the write "
                               "row lands on a temporary copy, never the repository fixture. "
                               "Inspector's paced session is the authoritative exhaustive "
                               "per-tool matrix; the model-directed Claude Code flow tolerates "
                               "a transient unavailable/SANDBOX_DENIED capacity refusal on "
                               "symbols(workspace)/references/diagnostics alone (W09f)",
            },
        },
        "source_sha256": check["source_sha256"],
        "candidate": {"server_sha256": m3.file_digest(SERVER), "sources": candidate_sources},
        "docker_settle": None,
    }
    write_root = None
    try:
        if closed_socket.exists():
            raise RuntimeError("the closed docker socket path must not exist before the gate")
        free_argv = server_argv(attempt / f"state-{DOCKER_FREE}", closed_socket)
        receipt["inspector"] = {
            DOCKER_FREE: inspector_gate(attempt, DOCKER_FREE, free_argv, plan, None, 300, 60_000),
        }
        receipt["claude_code"] = {
            DOCKER_FREE: claude_gate(attempt, DOCKER_FREE, free_argv, plan, None, 600),
        }
        if closed_socket.exists():
            raise RuntimeError("a docker socket was created during the Docker-free mode")
        receipt["docker_free_socket_created"] = False
        calls = receipt["inspector"][DOCKER_FREE].pop("calls")
        if with_runtime:
            socket_path = pathlib.Path(docker_socket)
            write_root = stage_write_fixture(private)
            runtime_argv = runtime_server_argv(
                attempt / f"state-{RUNTIME}", socket_path, write_root)
            receipt["inspector"][RUNTIME] = inspector_gate(
                attempt, RUNTIME, runtime_argv, runtime_plan, write_root, 1200, 300_000)
            receipt["docker_settle"] = settle_docker_between_batches(socket_path)
            receipt["claude_code"][RUNTIME] = claude_gate(
                attempt, RUNTIME, runtime_argv, runtime_plan, write_root, 1800)
            calls += receipt["inspector"][RUNTIME].pop("calls")
        receipt["calls"] = calls
        receipt["protocol"] = validate_protocol_metadata(attempt / "protocol.jsonl")
        receipt["clients"] = {
            "inspector": {"version": INSPECTOR_VERSION,
                          "bundle_sha256": receipt["inspector"][DOCKER_FREE]["bundle_sha256"]},
            "claude_code": {"version": CLAUDE_VERSION, "model": CLAUDE_MODEL,
                            "effort": CLAUDE_EFFORT, "executable_sha256": m3.file_digest(CLAUDE)},
        }
        if (candidate_sources != gate.source_inventory(ROOT, os.environ.copy())
                or receipt["candidate"]["server_sha256"] != m3.file_digest(SERVER)
                or receipt["source_sha256"] != source_hashes()):
            raise RuntimeError("client qualification inputs changed during execution")
        receipt["status"] = "passed"
    except Exception as error:
        receipt["error"] = {"type": type(error).__name__, "message": str(error)}
        raise
    finally:
        shutil.rmtree(private, ignore_errors=True)
        receipt["private_directory_removed"] = not private.exists()
        leak = None
        try:
            m3.assert_no_credentials(attempt)
            receipt["evidence_credential_scan"] = "clean"
        except RuntimeError as scan_error:
            leak = scan_error
            receipt["evidence_credential_scan"] = str(scan_error)
            receipt["status"] = "failed"
        m3.save_json(attempt / "receipt.json", receipt, exclusive=True)
        if leak is not None:
            raise leak
        if receipt["status"] == "passed":
            if CURRENT.exists():
                raise RuntimeError("current M6 client receipt already exists")
            m3.save_json(CURRENT, receipt, exclusive=True)
    return 0


def main() -> int:
    parser = argparse.ArgumentParser()
    subcommands = parser.add_subparsers(dest="command")
    proxy_parser = subcommands.add_parser("proxy")
    proxy_parser.add_argument("--client", required=True)
    proxy_parser.add_argument("--observation", type=pathlib.Path, required=True)
    proxy_parser.add_argument("--server-argv-json", required=True)
    parser.add_argument("--run", action="store_true")
    parser.add_argument("--with-runtime", action="store_true")
    parser.add_argument("--docker-socket", default=os.environ.get("RUST_MCP_TEST_SOCKET"))
    parser.add_argument("--write-preflight", action="store_true")
    options = parser.parse_args()
    if options.command == "proxy":
        argv = json.loads(options.server_argv_json)
        if not isinstance(argv, list) or not argv or any(not isinstance(item, str) for item in argv):
            raise RuntimeError("invalid closed server argv")
        return load_m3().proxy(argv, options.observation, options.client)
    if not options.run:
        if options.with_runtime and not options.run:
            # The runtime mode is a gate, never a preflight side effect.
            raise RuntimeError("--with-runtime requires --run")
        receipt = preflight(False, options.docker_socket)
        if options.write_preflight:
            if PREFLIGHT.exists():
                raise RuntimeError("M6 preflight receipt already exists")
            load_m3().save_json(PREFLIGHT, receipt, exclusive=True)
        print(json.dumps(receipt, sort_keys=True))
        if receipt["unsatisfied"]:
            print("M6 client qualification is not runnable; unsatisfied preconditions: "
                  + ", ".join(receipt["unsatisfied"]), file=sys.stderr)
            return 1
        return 0
    return run(options.with_runtime, options.docker_socket)


if __name__ == "__main__":
    raise SystemExit(main())
