#!/usr/bin/env python3
"""0.8.0 wire/client qualification matrix (M8-04): Inspector 2.5.0 (deterministic,
authoritative), Codex CLI 0.154.0 (stock, mandatory), Claude Code 2.1.268 and
Gemini CLI (``agy`` 1.2.2) (qualified before being announced).

The default invocation is client-free and Docker-free: it re-derives the
advertised 36-tool inventory from the server's own protocol oracle, recomputes
the canonical contract hashes and the Docker-free negative call plan for the
31 ``stable`` tools, checks host preconditions and writes nothing.

``--preflight``
    Prints exact client/server versions and every precondition, non-executing.

``--run``
    The Docker-free matrix: aborts first if a mandatory precondition is
    unsatisfied. Then: Inspector discovery, contract equality against
    ``tests/baselines/contract-freeze-0.8.0.json``, ``resources/list`` emptiness, one
    structured Docker-free negative row per ``stable`` tool (30 refusals plus
    the single ``rust.catalog.status`` passed-observation row), four
    cross-cutting negatives (unknown tool, invalid args, invalid
    ``project_ref``, unknown field) and the Codex/Claude Code/Gemini CLI
    stock-client turns over the same Docker-free host.

``--run --with-runtime``
    Runs two Inspector sessions instead of one: the same ``docker_free``
    session as plain ``--run`` (an unreachable socket, so its 30-refusal
    negative plan stays valid), plus a second ``runtime`` session against a
    real, calibrated Docker socket that never repeats the Docker-free
    negatives -- it drives one real ``rust.check`` call, its published
    ``rust-artifact://`` Resource, an in-flight ``notifications/cancelled``
    cancellation (confirmed on the wire, then a clean retry) and a second
    ``rust.check`` abandoned by an abrupt mid-call transport close, proven
    torn down by a fresh session succeeding right after and by no orphaned
    ``rust-engineering-mcp serve`` process for that session's state-root.
    The Codex/Claude Code/Gemini CLI stock-client turns move to that same
    runtime host (open -> a real ``rust.project.inspect`` pass -> one
    ``PROJECT_NOT_FOUND`` negative). Additionally composes the M2-M6
    positive coverage by invoking each milestone's own client/runtime
    harness as a subprocess and folding its receipt in (never
    re-implemented here), and drives a stdin-EOF teardown through the
    qualified M2/M3 runtime image.

The five MCP protocol revisions are credited by the ``core`` gate's own
protocol tests (historical receipt: ``docs/validation/M8/core-gate.json`` at 51fa602e); this harness does not
repeat them. A client that does not run is recorded ``unavailable`` and is
never announced as qualified: nothing that did not run is a pass.
"""
from __future__ import annotations

import argparse
import hashlib
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
import threading
import time
import tomllib
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[1]
M3_PATH = ROOT / "scripts/test-m3-clients.py"
M5_PATH = ROOT / "scripts/test-m5-clients.py"
M6_PATH = ROOT / "scripts/test-m6-clients.py"
CONTRACT_FREEZE_PATH = ROOT / "scripts/contract-freeze.py"
SESSION = ROOT / "scripts/m8-inspector-session.mjs"
UNIT = ROOT / "scripts/test-m8-clients-unit.py"
ATTEMPTS = ROOT / "target/qualification/test-m8-clients/clients"
CURRENT = ROOT / "target/qualification/test-m8-clients/clients.json"
FREEZE_MANIFEST = ROOT / "tests/baselines/contract-freeze-0.8.0.json"
SERVER = ROOT / "target/release/rust-engineering-mcp"
NODE = pathlib.Path("/Users/cburgosro/.nvm/versions/node/v24.15.0/bin/node")
CLAUDE = pathlib.Path("/Users/cburgosro/.local/share/claude/versions/2.1.268")
CODEX = pathlib.Path(shutil.which("codex") or "/nonexistent/codex")
AGY = pathlib.Path(shutil.which("agy") or "/nonexistent/agy")
BRIDGE_DIR = ROOT / "target/m1-17-inspector"
INSPECTOR = ROOT / "target/m1-17-inspector/node_modules/@modelcontextprotocol/inspector/clients/cli/build/index.js"
INSPECTOR_PACKAGE = ROOT / "target/m1-17-inspector/node_modules/@modelcontextprotocol/inspector/package.json"
DOCKER = pathlib.Path("/Applications/Docker.app/Contents/Resources/bin/docker")
STDIO = ROOT / "crates/mcp-server/src/stdio.rs"
PROTOCOL_TEST = ROOT / "crates/mcp-server/tests/protocol.rs"

def workspace_version() -> str:
    with open(ROOT / "Cargo.toml", "rb") as handle:
        manifest = tomllib.load(handle)
    try:
        return manifest["workspace"]["package"]["version"]
    except KeyError as exc:
        raise RuntimeError(
            f"Cargo.toml is missing [workspace.package].version: {exc}"
        ) from exc


INSPECTOR_VERSION = "2.5.0"
CODEX_VERSION = "codex-cli 0.154.0"
CLAUDE_VERSION = "2.1.268 (Claude Code)"
AGY_VERSION = "1.2.2"
SERVER_VERSION = workspace_version()
CLAUDE_MODEL = "claude-sonnet-5"
CLAUDE_EFFORT = "medium"
CLAUDE_CLIENT = "claude-code"
CLAUDE_SERVER = "rust_engineering"
CODEX_MODEL = "gpt-5.6-sol"
CODEX_EFFORT = "medium"
GEMINI_MODEL = "gemini-3.8-flash-high"
MCP_TOOL_TIMEOUT_MS = 300_000

# The 31 tools this gate must exercise, in the exact order the server's own
# protocol oracle advertises them (`crates/mcp-server/tests/protocol.rs`).
STABLE_TOOLS = (
    "rust.project.open", "rust.project.inspect", "rust.toolchain.inspect",
    "rust.check", "rust.fmt.check", "rust.clippy", "rust.test",
    "rust.test.nextest", "rust.dependencies.audit", "rust.diagnostics.explain",
    "rust.quality.gate", "rust.catalog.status", "rust.crate.search",
    "rust.crate.inspect", "rust.manifest.patch", "rust.fmt.apply",
    "rust.fix.apply", "rust.dependency.add", "rust.dependency.remove",
    "rust.coverage", "rust.semver.check", "rust.mutation.test",
    "rust.deny", "rust.unsafe.scan", "rust.supply_chain.inspect",
    "rust.quality.gate.v2", "rust.miri", "rust.benchmark.run",
    "rust.benchmark.compare", "rust.profile.flamegraph", "rust.binary.bloat",
)
# The 5 `preview` (ADR-086) analyzer tools: read-only for this gate's purposes,
# credited by the M6 harness; this gate's contract-equality oracle still
# covers all 36.
PREVIEW_TOOLS = (
    "rust.analyzer.symbols", "rust.analyzer.references",
    "rust.analyzer.diagnostics", "rust.analyzer.actions",
    "rust.analyzer.action.apply",
)
EXPECTED_TOOLS = STABLE_TOOLS + PREVIEW_TOOLS

# The qualified M2/M3 Rust runtime image (ADR-031); every stable tool this
# gate calls needs at most this one runtime identity.
RUST_IMAGE = "sha256:384a1742ecc53cdd3a9c0bf36c6f8b66db73ddd118aeeae6e55654ea998ae36a"
FIXTURE = "fixtures/valid-basic"
UNKNOWN_PROJECT_REF = "prj_" + "0" * 32
BOGUS_OPEN_PATH = str(ROOT / "target" / "rust-mcp-m8-clients-never-exists")
DOCKER_FREE = "docker_free"
RUNTIME = "runtime"
CALL_STATUSES = frozenset({"passed", "failed", "blocked", "unavailable", "cancelled"})
REFUSAL_STATUSES = frozenset({"blocked", "unavailable"})
MAX_OUTPUT = 8 * 1024 * 1024

# Tool -> the source file(s) carrying its closed error-code vocabulary, and
# the Serde case convention of that enum. The 5 mutation tools share one
# `Reason` enum in `mutation.rs`, rendered `snake_case`; every other tool
# publishes its own `*Code` enum(s), rendered `SCREAMING_SNAKE_CASE`.
MUTATION_TOOLS = frozenset({
    "rust.manifest.patch", "rust.fmt.apply", "rust.fix.apply",
    "rust.dependency.add", "rust.dependency.remove",
})
TOOL_SOURCES: dict[str, tuple[tuple[pathlib.Path, str], ...]] = {
    "rust.project.open": (
        (ROOT / "crates/mcp-server/src/stdio/project.rs", "BlockedCode"),
        (ROOT / "crates/mcp-server/src/stdio/project.rs", "UnavailableCode"),
    ),
    "rust.project.inspect": ((ROOT / "crates/mcp-server/src/stdio/inspection.rs", "Code"),),
    "rust.toolchain.inspect": ((ROOT / "crates/mcp-server/src/stdio/toolchain.rs", "Code"),),
    "rust.check": ((ROOT / "crates/mcp-server/src/stdio/check.rs", "Code"),),
    "rust.fmt.check": ((ROOT / "crates/mcp-server/src/stdio/format.rs", "Code"),),
    "rust.clippy": ((ROOT / "crates/mcp-server/src/stdio/clippy.rs", "Code"),),
    "rust.test": ((ROOT / "crates/mcp-server/src/stdio/testing.rs", "Code"),),
    "rust.test.nextest": ((ROOT / "crates/mcp-server/src/stdio/nextest.rs", "Code"),),
    "rust.dependencies.audit": ((ROOT / "crates/mcp-server/src/stdio/auditing.rs", "Code"),),
    "rust.diagnostics.explain": ((ROOT / "crates/mcp-server/src/stdio/explaining.rs", "Code"),),
    "rust.quality.gate": ((ROOT / "crates/mcp-server/src/stdio/quality.rs", "Code"),),
    "rust.catalog.status": ((ROOT / "crates/mcp-server/src/stdio/catalog.rs", "Code"),),
    "rust.crate.search": ((ROOT / "crates/mcp-server/src/stdio/crate_search.rs", "Code"),),
    "rust.crate.inspect": ((ROOT / "crates/mcp-server/src/stdio/crate_inspect.rs", "Code"),),
    "rust.manifest.patch": ((ROOT / "crates/mcp-server/src/stdio/mutation.rs", "Reason"),),
    "rust.fmt.apply": ((ROOT / "crates/mcp-server/src/stdio/mutation.rs", "Reason"),),
    "rust.fix.apply": ((ROOT / "crates/mcp-server/src/stdio/mutation.rs", "Reason"),),
    "rust.dependency.add": ((ROOT / "crates/mcp-server/src/stdio/mutation.rs", "Reason"),),
    "rust.dependency.remove": ((ROOT / "crates/mcp-server/src/stdio/mutation.rs", "Reason"),),
    "rust.coverage": ((ROOT / "crates/mcp-server/src/stdio/coverage.rs", "Code"),),
    "rust.semver.check": ((ROOT / "crates/mcp-server/src/stdio/semver.rs", "Code"),),
    "rust.mutation.test": ((ROOT / "crates/mcp-server/src/stdio/mutation_test.rs", "Code"),),
    "rust.deny": ((ROOT / "crates/mcp-server/src/stdio/deny.rs", "Code"),),
    "rust.unsafe.scan": ((ROOT / "crates/mcp-server/src/stdio/unsafe_scan.rs", "Code"),),
    "rust.supply_chain.inspect": ((ROOT / "crates/mcp-server/src/stdio/supply_chain.rs", "Code"),),
    "rust.quality.gate.v2": ((ROOT / "crates/mcp-server/src/stdio/quality_v2.rs", "Code"),),
    "rust.miri": ((ROOT / "crates/mcp-server/src/stdio/miri.rs", "Code"),),
    "rust.benchmark.run": ((ROOT / "crates/mcp-server/src/stdio/benchmark.rs", "Code"),),
    "rust.benchmark.compare": ((ROOT / "crates/mcp-server/src/stdio/benchmark_compare.rs", "Code"),),
    "rust.profile.flamegraph": ((ROOT / "crates/mcp-server/src/stdio/profile.rs", "Code"),),
    "rust.binary.bloat": ((ROOT / "crates/mcp-server/src/stdio/bloat.rs", "Code"),),
}

# The Docker-free negative call plan: one row per `stable` tool, over a host
# with neither a calibrated Docker runtime nor a catalog nor any write grant
# configured -- a structured `blocked`/`unavailable` refusal for every tool
# except the single `observation_only` row (`rust.catalog.status`), whose own
# contract is to always report `passed`. `project_ref_fields` names
# which top-level field(s) the driver fills with the captured `fixture`
# project's own `project_ref` (semver.check takes two); `fingerprint_target`
# says whether the captured fingerprint is injected top-level, nested under
# `action`, or not needed at all. `arguments` is only ever the literal,
# tool-specific remainder -- never a hard-coded ProjectRef or fingerprint.
NEGATIVE_ROWS: tuple[dict[str, object], ...] = (
    {
        "tool": "rust.project.open", "project_ref_fields": (), "fingerprint_target": None,
        "expected_code": "SANDBOX_DENIED", "arguments": {"path": BOGUS_OPEN_PATH},
        "rationale": "a path this host never creates is the one refusal open() can produce "
                     "without any runtime, grant or catalog at all",
    },
    {
        "tool": "rust.project.inspect", "project_ref_fields": ("project_ref",),
        "fingerprint_target": None, "expected_code": "TOOL_NOT_INSTALLED", "arguments": {},
        "rationale": "project.inspect needs the full Docker tuple; this host configures none",
    },
    {
        "tool": "rust.toolchain.inspect", "project_ref_fields": ("project_ref",),
        "fingerprint_target": None, "expected_code": "TOOL_NOT_INSTALLED", "arguments": {},
        "rationale": "toolchain.inspect shares project.inspect's host policy",
    },
    {
        "tool": "rust.check", "project_ref_fields": ("project_ref",),
        "fingerprint_target": None, "expected_code": "TOOL_NOT_INSTALLED", "arguments": {},
        "rationale": "no --rust runtime is calibrated for this host, so no container can exist",
    },
    {
        "tool": "rust.fmt.check", "project_ref_fields": ("project_ref",),
        "fingerprint_target": None, "expected_code": "TOOL_NOT_INSTALLED", "arguments": {},
        "rationale": "fmt.check shares check's host-level runtime refusal",
    },
    {
        "tool": "rust.clippy", "project_ref_fields": ("project_ref",),
        "fingerprint_target": None, "expected_code": "TOOL_NOT_INSTALLED", "arguments": {},
        "rationale": "clippy shares check's host-level runtime refusal",
    },
    {
        "tool": "rust.test", "project_ref_fields": ("project_ref",),
        "fingerprint_target": None, "expected_code": "TOOL_NOT_INSTALLED", "arguments": {},
        "rationale": "test shares check's host-level runtime refusal",
    },
    {
        "tool": "rust.test.nextest", "project_ref_fields": ("project_ref",),
        "fingerprint_target": None, "expected_code": "TOOL_NOT_INSTALLED",
        "arguments": {"execution_mode": "synchronous", "timeout_seconds": 30},
        "rationale": "a synchronous, non-Tasks call reaches the same runtime refusal, never "
                     "TASKS_REQUIRED",
    },
    {
        "tool": "rust.dependencies.audit", "project_ref_fields": ("project_ref",),
        "fingerprint_target": None, "expected_code": "TOOL_NOT_INSTALLED", "arguments": {},
        "rationale": "audit needs the runtime for frozen metadata plus a --rustsec-snapshot "
                     "this host never provides",
    },
    {
        "tool": "rust.diagnostics.explain", "project_ref_fields": (),
        "fingerprint_target": None, "expected_code": "TOOL_NOT_INSTALLED",
        "arguments": {"code": "E0308"},
        "rationale": "explain resolves through the same calibrated gateway; no ProjectRef is "
                     "part of its closed input",
    },
    {
        "tool": "rust.quality.gate", "project_ref_fields": ("project_ref",),
        "fingerprint_target": None, "expected_code": "QUALITY_GATE_UNAVAILABLE",
        "arguments": {"profile": "fast"},
        "rationale": "quality.gate composes fmt/check/clippy, each refused the same way; "
                     "profile is a required closed-enum field with no default",
    },
    {
        "tool": "rust.catalog.status", "project_ref_fields": (),
        "fingerprint_target": None, "expected_code": None,
        "arguments": {}, "observation_only": True,
        "rationale": "an observation tool (crates/mcp-server/src/stdio/catalog.rs), not a "
                     "refusal: with no --catalog-store/--catalog-trust pair configured it "
                     "still reports `passed` and describes the absence as data, matching "
                     "protocol.rs's catalog_status_closed_input_and_explicit_absence_in_all_versions",
    },
    {
        "tool": "rust.crate.search", "project_ref_fields": (),
        "fingerprint_target": None, "expected_code": "CATALOG_UNAVAILABLE",
        "arguments": {"query": "serde"},
        "rationale": "unlike status's own observation contract, search must resolve through "
                     "the unconfigured catalog and refuses",
    },
    {
        "tool": "rust.crate.inspect", "project_ref_fields": (),
        "fingerprint_target": None, "expected_code": "CATALOG_UNAVAILABLE",
        "arguments": {"name": "serde"},
        "rationale": "unlike status's own observation contract, inspect must resolve through "
                     "the unconfigured catalog and refuses",
    },
    {
        "tool": "rust.manifest.patch", "project_ref_fields": ("project_ref",),
        "fingerprint_target": "action", "expected_code": "permission_denied",
        "arguments": {"action": {"mode": "preview", "edit": {
            "operation": "lint_set", "scope": "package", "tool": "rust",
            "name": "unsafe_code", "level": "forbid",
        }}},
        "rationale": "no --allow-manifest-write grant is configured for this project's root",
    },
    {
        "tool": "rust.fmt.apply", "project_ref_fields": ("project_ref",),
        "fingerprint_target": "action", "expected_code": "permission_denied",
        "arguments": {"action": {"mode": "preview"}},
        "rationale": "no --allow-fmt-write grant is configured for this project's root",
    },
    {
        "tool": "rust.fix.apply", "project_ref_fields": ("project_ref",),
        "fingerprint_target": "action", "expected_code": "permission_denied",
        "arguments": {"action": {"mode": "preview"}},
        "rationale": "no --allow-fix-write grant is configured for this project's root",
    },
    {
        "tool": "rust.dependency.add", "project_ref_fields": ("project_ref",),
        "fingerprint_target": "action", "expected_code": "permission_denied",
        "arguments": {"action": {"mode": "preview", "name": "quote", "requirement": "=1.0.47"}},
        "rationale": "no --allow-dependency-add grant is configured for this project's root",
    },
    {
        "tool": "rust.dependency.remove", "project_ref_fields": ("project_ref",),
        "fingerprint_target": "action", "expected_code": "permission_denied",
        "arguments": {"action": {"mode": "preview", "name": "quote"}},
        "rationale": "no --allow-dependency-remove grant is configured for this project's root",
    },
    {
        "tool": "rust.coverage", "project_ref_fields": ("project_ref",),
        "fingerprint_target": None, "expected_code": "TOOL_NOT_INSTALLED", "arguments": {},
        "rationale": "coverage shares check's host-level runtime refusal",
    },
    {
        "tool": "rust.semver.check", "project_ref_fields": ("baseline_project_ref", "candidate_project_ref"),
        "fingerprint_target": None, "expected_code": "TOOL_NOT_INSTALLED", "arguments": {},
        "rationale": "both sides of the comparison resolve through the same unconfigured runtime",
    },
    {
        "tool": "rust.mutation.test", "project_ref_fields": ("project_ref",),
        "fingerprint_target": None, "expected_code": "TOOL_NOT_INSTALLED", "arguments": {},
        "rationale": "mutation.test shares check's host-level runtime refusal",
    },
    {
        "tool": "rust.deny", "project_ref_fields": ("project_ref",),
        "fingerprint_target": None, "expected_code": "SECURITY_POLICY_INVALID",
        "arguments": {"execution_mode": "synchronous", "timeout_seconds": 60},
        "rationale": "deny shares check's host-level runtime refusal (M4 precedent)",
    },
    {
        "tool": "rust.unsafe.scan", "project_ref_fields": ("project_ref",),
        "fingerprint_target": None, "expected_code": "MISSING_OFFLINE_DATA",
        "arguments": {"execution_mode": "synchronous", "timeout_seconds": 60},
        "rationale": "the scanner dials the same unconfigured Docker socket",
    },
    {
        "tool": "rust.supply_chain.inspect", "project_ref_fields": ("project_ref",),
        "fingerprint_target": None, "expected_code": "TOOL_NOT_INSTALLED",
        "arguments": {"execution_mode": "synchronous", "timeout_seconds": 60},
        "rationale": "supply_chain.inspect shares check's host-level runtime refusal",
    },
    {
        "tool": "rust.quality.gate.v2", "project_ref_fields": ("project_ref",),
        "fingerprint_target": None, "expected_code": "TOOL_NOT_INSTALLED",
        "arguments": {"execution_mode": "synchronous", "timeout_seconds": 60, "profile": "strict"},
        "rationale": "quality.gate.v2 shares check's host-level runtime refusal (M4 precedent)",
    },
    {
        "tool": "rust.miri", "project_ref_fields": ("project_ref",),
        "fingerprint_target": None, "expected_code": "MISSING_OFFLINE_DATA",
        "arguments": {"execution_mode": "synchronous", "timeout_seconds": 60},
        "rationale": "miri shares check's host-level runtime refusal",
    },
    {
        "tool": "rust.benchmark.run", "project_ref_fields": ("project_ref",),
        "fingerprint_target": None, "expected_code": "MISSING_OFFLINE_DATA",
        "arguments": {"bench_target": "perf", "run_count": 1, "timeout_seconds": 60,
                      "execution_mode": "synchronous"},
        "rationale": "benchmark.run needs the same runtime this host never calibrates",
    },
    {
        "tool": "rust.benchmark.compare", "project_ref_fields": ("project_ref",),
        "fingerprint_target": None, "expected_code": "ARTIFACT_NOT_FOUND",
        "arguments": {"baseline_artifact_id": "qart_" + "0" * 32,
                      "candidate_artifact_id": "qart_" + "1" * 32, "timeout_seconds": 30},
        "rationale": "the comparison runs no process; two identifiers this project never "
                     "published resolve to nothing (M5 precedent, requires_runtime=None); "
                     "project_ref is a required closed field with no default",
    },
    {
        "tool": "rust.profile.flamegraph", "project_ref_fields": ("project_ref",),
        "fingerprint_target": None, "expected_code": "PROFILING_NOT_AUTHORIZED",
        "arguments": {"binary_target": "rust-mcp-m8-clients-probe", "frequency_hz": 99,
                      "duration_seconds": 2, "timeout_seconds": 60,
                      "execution_mode": "synchronous"},
        "rationale": "profile.flamegraph needs the same runtime this host never calibrates",
    },
    {
        "tool": "rust.binary.bloat", "project_ref_fields": ("project_ref",),
        "fingerprint_target": None, "expected_code": "MISSING_OFFLINE_DATA",
        "arguments": {"binary_target": "rust-mcp-m8-clients-probe", "profile": "release",
                      "timeout_seconds": 60, "execution_mode": "synchronous"},
        "rationale": "binary.bloat needs the same runtime this host never calibrates",
    },
)

# Four cross-cutting negatives, independent of any one tool's own runtime:
# an unknown tool name, schema-invalid arguments, a well-formed but unopened
# ProjectRef, and an argument outside a closed (`deny_unknown_fields`) schema.
GENERIC_NEGATIVE_ROWS: tuple[dict[str, object], ...] = (
    {"kind": "unknown_tool", "tool": "rust.not.a.real.tool", "arguments": {},
     "rationale": "the tool name is outside the 36-tool advertised inventory"},
    {"kind": "invalid_args", "tool": "rust.project.inspect", "arguments": {"project_ref": 12345},
     "rationale": "project_ref must be a string matching ^prj_[0-9a-f]{32}$, not a number"},
    {"kind": "unknown_project_ref", "tool": "rust.project.inspect",
     "arguments": {"project_ref": UNKNOWN_PROJECT_REF},
     "rationale": "a well-formed ProjectRef this session never opened"},
    {"kind": "unknown_fields", "tool": "rust.catalog.status", "arguments": {"unexpected_field": True},
     "rationale": "catalog.status's input schema is the closed object `{}`"},
)


def load_module(path: pathlib.Path, name: str):
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"{path.name} harness is unavailable")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def load_m3():
    """Reuse M3's bounded subprocess, proxy, digest and receipt primitives."""
    return load_module(M3_PATH, "rust_mcp_m3_clients")


def load_m6():
    """Reuse M6's stock Claude Code transcript parser/validator unchanged:
    both harnesses share the identical 36-tool `EXPECTED_TOOLS` inventory."""
    return load_module(M6_PATH, "rust_mcp_m6_clients")


def load_contract_freeze():
    """Reuse the freeze oracle's own canonical hash formula, byte for byte."""
    return load_module(CONTRACT_FREEZE_PATH, "rust_mcp_contract_freeze")


def canonical_hash(value: object) -> str:
    cf = load_contract_freeze()
    return cf.canonical_hash(value)


def screaming(name: str) -> str:
    return re.sub(r"(?<!^)(?=[A-Z])", "_", name).upper()


def snake(name: str) -> str:
    return re.sub(r"(?<!^)(?=[A-Z])", "_", name).lower()


RENAME_ALL_TRANSFORMS: dict[str, object] = {
    "SCREAMING_SNAKE_CASE": screaming,
    "snake_case": snake,
}


def enum_case_transform(source: str, enum_pos: int, enum_name: str):
    """The case transform an enum's own `#[serde(rename_all = "...")]`
    attribute names, read from the contiguous attribute block immediately
    above the enum -- never guessed from the tool's family, and never
    borrowed from an unrelated enum's own attributes elsewhere in the file."""
    convention = None
    for line in reversed(source[:enum_pos].splitlines()):
        stripped = line.strip()
        if not stripped.startswith("#["):
            break
        match = re.search(r'rename_all\s*=\s*"([^"]+)"', stripped)
        if match:
            convention = match.group(1)
            break
    if convention is None:
        raise RuntimeError(f"{enum_name}: missing a #[serde(rename_all = ...)] attribute")
    if convention not in RENAME_ALL_TRANSFORMS:
        raise RuntimeError(f"{enum_name}: unsupported #[serde(rename_all = \"{convention}\")]")
    return RENAME_ALL_TRANSFORMS[convention]


def enum_variants(block: str) -> list[tuple[str | None, str]]:
    """Parse `VariantName,` entries from an enum body, honoring an explicit
    `#[serde(rename = "...")]` immediately above a variant when present --
    that literal wins over the enum's own `rename_all` transform."""
    variants: list[tuple[str | None, str]] = []
    pending_rename: str | None = None
    for line in block.splitlines():
        stripped = line.strip()
        rename_match = re.fullmatch(r'#\[serde\(rename\s*=\s*"([^"]+)"\)\]', stripped)
        if rename_match:
            pending_rename = rename_match.group(1)
            continue
        variant_match = re.fullmatch(r"([A-Z][A-Za-z0-9]*),", stripped)
        if variant_match:
            variants.append((pending_rename, variant_match.group(1)))
            pending_rename = None
        elif stripped and not stripped.startswith("#["):
            pending_rename = None
    return variants


def declared_error_codes(tool: str) -> frozenset[str]:
    """The closed wire vocabulary a tool can publish, unioned across every
    `*Code`/`Reason` enum its source declares, rendered by that enum's own
    `#[serde(rename_all)]` attribute (an explicit per-variant `#[serde(rename)]`
    overrides the transform for that one variant)."""
    sources = TOOL_SOURCES[tool]
    codes: set[str] = set()
    for path, enum_name in sources:
        source = path.read_text()
        marker = f"enum {enum_name} {{"
        enum_pos = source.index(marker)
        transform = enum_case_transform(source, enum_pos, enum_name)
        start = enum_pos + len(marker)
        block = source[start:source.index("\n}", start)]
        variants = enum_variants(block)
        if not variants:
            raise RuntimeError(f"error code vocabulary is empty for {tool} ({enum_name})")
        codes.update(rename if rename is not None else transform(name) for rename, name in variants)
    if len(codes) < 1:
        raise RuntimeError(f"error code vocabulary is invalid for {tool}")
    return frozenset(codes)


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


def inventory_check() -> dict[str, object]:
    published = protocol_inventory()
    if published != EXPECTED_TOOLS:
        raise RuntimeError("advertised inventory drifted from the server protocol oracle")
    if len(EXPECTED_TOOLS) != 36 or len(set(EXPECTED_TOOLS)) != 36:
        raise RuntimeError("closed tool inventory is invalid")
    if EXPECTED_TOOLS[:31] != STABLE_TOOLS or EXPECTED_TOOLS[31:] != PREVIEW_TOOLS:
        raise RuntimeError("the 31 stable / 5 preview split does not match the advertised order")
    return {
        "count": len(EXPECTED_TOOLS), "stable_count": len(STABLE_TOOLS),
        "preview_count": len(PREVIEW_TOOLS),
        "sources": ["crates/mcp-server/src/stdio.rs", "crates/mcp-server/tests/protocol.rs"],
    }


def check_negative_row(row: dict[str, object]) -> None:
    tool = row["tool"]
    if tool not in STABLE_TOOLS:
        raise RuntimeError(f"negative row names a non-stable tool: {tool}")
    fields = row["project_ref_fields"]
    if not isinstance(fields, tuple) or any(not isinstance(f, str) for f in fields):
        raise RuntimeError(f"{tool}: project_ref_fields must be a tuple of field names")
    if row["fingerprint_target"] not in (None, "top", "action"):
        raise RuntimeError(f"{tool}: unknown fingerprint_target")
    if row["fingerprint_target"] is not None and tool not in MUTATION_TOOLS:
        raise RuntimeError(f"{tool}: only the mutation family needs a captured fingerprint")
    if tool in MUTATION_TOOLS and row["fingerprint_target"] != "action":
        raise RuntimeError(f"{tool}: the mutation family always nests its fingerprint under action")
    arguments = row["arguments"]
    for field in fields:
        if field in arguments:
            raise RuntimeError(f"{tool}: project authority is captured at call time, never hard-coded")
    action = arguments.get("action") if isinstance(arguments, dict) else None
    if isinstance(action, dict) and "expected_project_fingerprint" in action:
        raise RuntimeError(f"{tool}: the fingerprint is captured at call time, never hard-coded")
    if not row["rationale"]:
        raise RuntimeError(f"{tool}: a negative row must publish its rationale")
    if not isinstance(row.get("observation_only", False), bool):
        raise RuntimeError(f"{tool}: observation_only must be a bool")
    codes = declared_error_codes(tool)  # raises if the tool's own vocabulary cannot be read
    observation_only = bool(row.get("observation_only", False))
    expected_code = row.get("expected_code")
    if observation_only:
        if expected_code is not None:
            raise RuntimeError(f"{tool}: an observation_only row never expects an error code")
    else:
        if not expected_code:
            raise RuntimeError(f"{tool}: a refusal row must fix its own expected_code")
        if expected_code not in codes:
            raise RuntimeError(f"{tool}: expected_code {expected_code!r} is outside its own declared vocabulary")


def negative_call_plan() -> list[dict[str, object]]:
    covered = set()
    rows = []
    for row in NEGATIVE_ROWS:
        check_negative_row(row)
        covered.add(row["tool"])
        rows.append({
            "tool": row["tool"], "project_ref_fields": list(row["project_ref_fields"]),
            "fingerprint_target": row["fingerprint_target"], "arguments": row["arguments"],
            "observation_only": bool(row.get("observation_only", False)),
            "declared_codes": sorted(declared_error_codes(row["tool"])),
            "expected_code": row.get("expected_code"),
            "rationale": row["rationale"],
        })
    missing = sorted(set(STABLE_TOOLS) - covered)
    if missing:
        raise RuntimeError("Docker-free negative plan omits: " + ", ".join(missing))
    if len(rows) != len(STABLE_TOOLS):
        raise RuntimeError("Docker-free negative plan does not carry exactly one row per tool")
    return rows


def generic_negative_plan() -> list[dict[str, object]]:
    kinds = {row["kind"] for row in GENERIC_NEGATIVE_ROWS}
    if kinds != {"unknown_tool", "invalid_args", "unknown_project_ref", "unknown_fields"}:
        raise RuntimeError("generic negative plan is missing a required kind")
    for row in GENERIC_NEGATIVE_ROWS:
        if not row["rationale"]:
            raise RuntimeError(f"{row['kind']}: a generic negative must publish its rationale")
    return list(GENERIC_NEGATIVE_ROWS)


def load_freeze_manifest() -> dict[str, object]:
    manifest = json.loads(FREEZE_MANIFEST.read_text())
    if manifest.get("tool_count") != 36 or set(manifest.get("tools", {})) != set(EXPECTED_TOOLS):
        raise RuntimeError("freeze manifest does not name the current 36-tool inventory")
    return manifest


def source_hashes() -> dict[str, str]:
    m3 = load_m3()
    paths = (
        M3_PATH, M5_PATH, CONTRACT_FREEZE_PATH, pathlib.Path(__file__).resolve(),
        SESSION, UNIT, FREEZE_MANIFEST,
    )
    return {str(path.relative_to(ROOT)): m3.file_digest(path) for path in paths if path.is_file()}


def server_argv(state: pathlib.Path, socket: pathlib.Path) -> list[str]:
    """The Docker-free host: a full but unusable runtime tuple (a real image
    digest, a real docker binary, a socket path this harness never creates),
    no catalog pair and no write grant -- so every stable tool's own
    precondition (runtime dial, catalog configuration or write grant) is the
    thing that refuses the call, never a schema-level rejection."""
    return [
        str(SERVER), "serve", "--stdio", "--root", str(ROOT / FIXTURE),
        "--docker", str(DOCKER), "--docker-socket", str(socket),
        "--state-root", str(state), "--rust-image", RUST_IMAGE,
    ]


def candidate_advertises_all() -> bool:
    if not SERVER.is_file():
        return False
    needles = [tool.encode() for tool in EXPECTED_TOOLS]
    found = set()
    tail = b""
    with SERVER.open("rb") as stream:
        while block := stream.read(1024 * 1024):
            window = tail + block
            found.update(needle for needle in needles if needle in window)
            tail = block[-64:]
    return len(found) == len(needles)


def server_version() -> dict[str, object] | None:
    if not SERVER.is_file():
        return None
    result = subprocess.run([str(SERVER), "version", "--json"], capture_output=True,
                            text=True, timeout=10, check=False)
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError:
        return None


def inspector_version() -> str | None:
    if INSPECTOR_PACKAGE.is_file():
        return json.loads(INSPECTOR_PACKAGE.read_text()).get("version")
    return None


def binary_version(path: pathlib.Path, flag: str = "--version") -> str | None:
    if not path.is_file():
        return None
    result = subprocess.run([str(path), flag], capture_output=True, text=True,
                            timeout=10, check=False)
    return result.stdout.strip() or None


def claude_logged_in() -> bool:
    if not CLAUDE.is_file():
        return False
    result = subprocess.run([str(CLAUDE), "auth", "status"], capture_output=True,
                            text=True, timeout=20, check=False)
    try:
        status = json.loads(result.stdout)
    except json.JSONDecodeError:
        return False
    return result.returncode == 0 and isinstance(status, dict) and status.get("loggedIn") is True


def client_versions() -> dict[str, object]:
    return {
        "inspector": {"expected": INSPECTOR_VERSION, "observed": inspector_version()},
        "codex": {"expected": CODEX_VERSION, "observed": binary_version(CODEX),
                  "model": CODEX_MODEL, "effort": CODEX_EFFORT, "mandatory": True},
        "claude_code": {"expected": CLAUDE_VERSION, "observed": binary_version(CLAUDE),
                        "model": CLAUDE_MODEL, "effort": CLAUDE_EFFORT, "mandatory": False},
        "gemini_cli": {"expected": AGY_VERSION, "observed": binary_version(AGY),
                      "model": GEMINI_MODEL, "mandatory": False},
    }


def git_head_commit() -> str:
    result = subprocess.run(["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True,
                            text=True, timeout=10, check=True)
    return result.stdout.strip()


def git_tree_dirty() -> bool:
    result = subprocess.run(["git", "status", "--porcelain"], cwd=ROOT, capture_output=True,
                            text=True, timeout=10, check=True)
    return bool(result.stdout.strip())


def preconditions(versions: dict[str, object], with_runtime: bool,
                  socket: str | None) -> dict[str, dict[str, object]]:
    checks: dict[str, tuple[bool, str]] = {
        "candidate_server_binary": (SERVER.is_file(), "target/release/rust-engineering-mcp must exist"),
        "candidate_advertises_36": (candidate_advertises_all(),
                                    "the built candidate must carry all 36 tool names"),
        "candidate_version_0_8_0": (
            (server_version() or {}).get("version") == SERVER_VERSION,
            f"the candidate must self-report version {SERVER_VERSION}",
        ),
        "freeze_manifest": (FREEZE_MANIFEST.is_file(), "the 0.8.0 freeze manifest must exist"),
        "node": (NODE.is_file(), "the pinned Node runtime must exist"),
        "inspector_bundle": (INSPECTOR.is_file(), "the pinned Inspector CLI bundle must exist"),
        "inspector_version": (versions["inspector"]["observed"] == INSPECTOR_VERSION,
                              f"the pinned Inspector must report {INSPECTOR_VERSION}"),
        "inspector_session": (SESSION.is_file(), "the M8 Inspector session driver must exist"),
        "codex_binary": (CODEX.is_file(), "the codex CLI must be on PATH (mandatory client)"),
        "codex_version": (versions["codex"]["observed"] == CODEX_VERSION,
                          f"stock Codex must report {CODEX_VERSION}"),
        "claude_binary": (CLAUDE.is_file(), "the pinned stock Claude Code executable must exist"),
        "claude_version": (versions["claude_code"]["observed"] == CLAUDE_VERSION,
                          f"stock Claude Code must report {CLAUDE_VERSION}"),
        "claude_auth": (claude_logged_in(), "stock Claude Code must be logged in; no credential is copied"),
        "gemini_binary": (AGY.is_file(), "the agy (Gemini CLI) executable must be on PATH"),
        "gemini_version": (versions["gemini_cli"]["observed"] == AGY_VERSION,
                          f"agy must report {AGY_VERSION}"),
        "docker_binary_present": (DOCKER.is_file(),
                                  "the pinned docker path must exist; the Docker-free mode never executes it"),
        "fixture_root": ((ROOT / FIXTURE / "Cargo.toml").is_file(), "the fixture root must carry a manifest"),
    }
    if with_runtime:
        path = pathlib.Path(socket) if socket else None
        checks["docker_socket"] = (
            path is not None and path.is_absolute() and path.exists(),
            "an absolute, existing Docker socket (--docker-socket or RUST_MCP_TEST_SOCKET)",
        )
    return {name: {"satisfied": bool(value), "requirement": detail}
            for name, (value, detail) in checks.items()}


# Preconditions tied only to an optional client (Claude Code / Gemini CLI):
# an unmet one there is never a reason for `--run` to abort, since that
# client is `unavailable`-tolerant and never itself announced as qualified.
OPTIONAL_PRECONDITIONS = frozenset({
    "claude_binary", "claude_version", "claude_auth", "gemini_binary", "gemini_version",
})


def mandatory_unsatisfied(checks: dict[str, dict[str, object]]) -> list[str]:
    return sorted(name for name, value in checks.items()
                  if name not in OPTIONAL_PRECONDITIONS and not value["satisfied"])


def preflight(with_runtime: bool = False, socket: str | None = None) -> dict[str, object]:
    versions = client_versions()
    checks = preconditions(versions, with_runtime, socket)
    unsatisfied = sorted(name for name, value in checks.items() if not value["satisfied"])
    return {
        "schema": "rust-mcp-m8-clients-preflight-v1",
        "status": "ready" if not unsatisfied else "blocked",
        "execution_performed": False, "clients_started": False,
        "with_runtime_requested": with_runtime, "docker_required": with_runtime,
        "docker_used": False, "rust_image": RUST_IMAGE,
        "expected_tools": list(EXPECTED_TOOLS), "stable_tools": list(STABLE_TOOLS),
        "preview_tools": list(PREVIEW_TOOLS),
        "inventory": inventory_check(),
        "negative_call_plan": negative_call_plan(),
        "generic_negative_plan": generic_negative_plan(),
        "clients": versions,
        "pinned_versions": {
            "inspector": INSPECTOR_VERSION,
            "codex": CODEX_VERSION,
            "claude_code": CLAUDE_VERSION,
            "gemini_cli": AGY_VERSION,
        },
        "preconditions": checks,
        "unsatisfied": unsatisfied,
        "source_sha256": source_hashes(),
        "protocol_revisions_credited_by": "docs/validation/M8/core-gate.json",
        "m3_reuse": ["run_bounded", "digest", "file_digest", "save_json",
                     "protocol_summary", "assert_no_credentials", "find_values",
                     "append_observation", "tasks_declared"],
        "composed_runtime_positives_from": [
            "scripts/test-m2-clients.py", "scripts/test-m3-clients.py",
            "scripts/test-m4-clients.py", "scripts/test-m5-clients.py",
            "scripts/test-m6-clients.py",
        ],
    }


def wire_proxy(server_argv: list[str], observation: pathlib.Path, client: str) -> int:
    """Same transparent stdio proxy as `test-m3-clients.py`'s own (bounded
    protocol metadata only), plus two additional safe fields captured from
    the server's own `tools/call` response -- `structuredContent.status` and
    `structuredContent.error_code`, both closed short enums the tool's own
    contract already publishes -- so a structured refusal (e.g. Codex's
    mandatory `PROJECT_NOT_FOUND` negative) can be confirmed from the wire
    itself, never from a client's own transcript."""
    m3 = load_m3()
    session = uuid.uuid4().hex
    child = subprocess.Popen(
        server_argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE, env=os.environ.copy(),
    )
    lock = threading.Lock()
    pending_tool_call = [False]

    def record(direction: str, line: bytes) -> None:
        row: dict[str, object] = {
            "client": client, "direction": direction, "session": session,
            "bytes": len(line), "sha256": m3.digest(line),
        }
        try:
            message = json.loads(line)
        except (UnicodeDecodeError, json.JSONDecodeError):
            row["malformed"] = True
        else:
            if isinstance(message, dict):
                method = message.get("method")
                if isinstance(method, str):
                    row["method"] = method
                if direction == "client" and method == "initialize":
                    capabilities = message.get("params", {}).get("capabilities", {})
                    row["tasks_declared"] = m3.tasks_declared(capabilities)
                if direction == "client" and method == "server/discover":
                    metadata = message.get("params", {}).get("_meta", {})
                    capabilities = metadata.get(
                        "io.modelcontextprotocol/clientCapabilities", {})
                    row["tasks_declared"] = m3.tasks_declared(capabilities)
                if direction == "server" and "result" in message:
                    capabilities = message.get("result", {}).get("capabilities", {})
                    if isinstance(capabilities, dict) and capabilities:
                        row["tasks_advertised"] = m3.tasks_declared(capabilities)
                if direction == "client" and method == "tools/call":
                    row["tool"] = message.get("params", {}).get("name")
                if direction == "client" and method == "resources/read":
                    uri = message.get("params", {}).get("uri")
                    if isinstance(uri, str):
                        row["resource_scheme"] = uri.partition(":")[0]
                if direction == "client" and method == "tools/call":
                    pending_tool_call[0] = True
                elif direction == "server":
                    if pending_tool_call[0] and "result" in message:
                        structured = message.get("result", {}).get("structuredContent")
                        if isinstance(structured, dict):
                            if isinstance(structured.get("status"), str):
                                row["structuredContent.status"] = structured["status"]
                            error_code = structured.get("error_code")
                            if "error_code" in structured and (
                                    error_code is None or isinstance(error_code, str)):
                                row["structuredContent.error_code"] = error_code
                    pending_tool_call[0] = False
        with lock:
            m3.append_observation(observation, row)

    def relay(source, destination, direction: str) -> None:
        while True:
            line = source.readline(MAX_OUTPUT + 1)
            if not line:
                break
            if len(line) > MAX_OUTPUT:
                child.kill()
                break
            record(direction, line.rstrip(b"\n"))
            destination.write(line)
            destination.flush()
        try:
            destination.close()
        except BrokenPipeError:
            pass

    def relay_stderr() -> None:
        observed = 0
        while True:
            block = child.stderr.read(65536)
            if not block:
                break
            observed += len(block)
            if observed <= MAX_OUTPUT:
                sys.stderr.buffer.write(block)
                sys.stderr.buffer.flush()

    threads = [
        threading.Thread(target=relay, args=(sys.stdin.buffer, child.stdin, "client")),
        threading.Thread(target=relay, args=(child.stdout, sys.stdout.buffer, "server")),
        threading.Thread(target=relay_stderr),
    ]
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join()
    return child.wait(timeout=15)


def validate_protocol_metadata(path: pathlib.Path) -> dict[str, object]:
    m3 = load_m3()
    rows = [json.loads(line) for line in path.read_text().splitlines() if line]
    safe_keys = frozenset({"client", "direction", "session", "bytes", "sha256", "malformed",
                           "method", "tasks_declared", "tasks_advertised", "tool",
                           "resource_scheme", "structuredContent.status",
                           "structuredContent.error_code"})
    for row in rows:
        extra = set(row) - safe_keys
        if extra:
            raise RuntimeError("protocol metadata contains unapproved keys: " + ",".join(sorted(extra)))
        if not isinstance(row.get("sha256"), str) or len(row["sha256"]) != 64:
            raise RuntimeError("protocol metadata digest is invalid")
    assert_no_credential_text(path)
    # M8 flips ADR-060's single switch (stdio.rs TASKS_ADVERTISEMENT_READY):
    # every modern session now advertises Tasks, unlike the M3/M5 harnesses
    # this function's `False` literal was copied from.
    summary = m3.protocol_summary(path, True)
    summary["metadata_only"] = True
    return summary


CREDENTIAL_MARKERS = (b"authorization", b"access_token", b"refresh_token", b"auth.json",
                      b"sk-ant-", b"sk-", b"bearer ")


def assert_no_credential_text(path: pathlib.Path) -> None:
    encoded = path.read_bytes().lower()
    for forbidden in CREDENTIAL_MARKERS:
        if forbidden in encoded:
            raise RuntimeError(f"credential-shaped evidence in {path.name}")


def validate_negative_rows(rows: object, plan: list[dict[str, object]]) -> list[dict[str, object]]:
    """Every row must land on a declared refusal, never an error code the
    tool's own source does not publish -- except the one `observation_only`
    row (`rust.catalog.status`), whose own contract is to always succeed and
    report absence as data, never as a refusal."""
    if not isinstance(rows, list) or len(rows) != len(plan):
        raise RuntimeError("Inspector did not report one row per planned negative")
    safe_keys = frozenset({"tool", "status", "error_code", "is_error"})
    for row, expected in zip(rows, plan):
        if not isinstance(row, dict) or set(row) != safe_keys:
            raise RuntimeError("negative row keys are not the approved set")
        if row["tool"] != expected["tool"]:
            raise RuntimeError("negative rows are not in planned order")
        if expected["observation_only"]:
            if row["status"] != "passed" or row["is_error"] is not False or row["error_code"] is not None:
                raise RuntimeError(f"{row['tool']} did not land on its observed passed status: {row}")
            continue
        if row["status"] not in REFUSAL_STATUSES:
            raise RuntimeError(f"{row['tool']} did not land on a structured refusal: {row['status']}")
        code = row["error_code"]
        if code is None:
            raise RuntimeError(f"{row['tool']} refusal reported a null error code")
        if code not in expected["declared_codes"]:
            raise RuntimeError(f"{row['tool']} reported an undeclared error code: {code}")
        if code != expected["expected_code"]:
            raise RuntimeError(
                f"{row['tool']} reported {code}, expected {expected['expected_code']}")
        if row["is_error"] is not True:
            raise RuntimeError(f"{row['tool']} refusal did not set isError")
    return list(rows)


# The JSON-RPC error each protocol-boundary generic negative must report:
# `MethodNotFound` for a tool name outside the advertised inventory,
# `InvalidParams` for arguments a schema (or its `deny_unknown_fields` closure)
# rejects. `unknown_project_ref` is not protocol-boundary: it is a well-formed
# call that lands on the tool's own structured refusal, not a wire error.
GENERIC_RPC_CODES: dict[str, int] = {
    "unknown_tool": -32601, "invalid_args": -32602, "unknown_fields": -32602,
}


def validate_generic_negative_rows(rows: object) -> list[dict[str, object]]:
    if not isinstance(rows, list) or len(rows) != len(GENERIC_NEGATIVE_ROWS):
        raise RuntimeError("Inspector did not report one row per generic negative")
    by_kind = {row["kind"]: row for row in GENERIC_NEGATIVE_ROWS}
    for row in rows:
        if not isinstance(row, dict) or "kind" not in row or row["kind"] not in by_kind:
            raise RuntimeError("generic negative row is malformed")
        if row["kind"] in GENERIC_RPC_CODES:
            if row.get("protocol_error") is not True:
                raise RuntimeError(f"{row['kind']} must be rejected at the protocol boundary")
            expected_code = GENERIC_RPC_CODES[row["kind"]]
            if row.get("rpc_code") != expected_code:
                raise RuntimeError(
                    f"{row['kind']} reported JSON-RPC code {row.get('rpc_code')}, "
                    f"expected {expected_code}")
        elif row["kind"] == "unknown_project_ref":
            if row.get("status") not in REFUSAL_STATUSES or row.get("is_error") is not True:
                raise RuntimeError("unknown_project_ref must land on a structured refusal")
    return list(rows)


def generic_negative_wire_confirmed(observation: pathlib.Path) -> dict[str, bool]:
    """The last `len(GENERIC_NEGATIVE_ROWS)` `tools/call` requests the
    Inspector session sent are, in order, exactly the generic negatives; each
    must be immediately followed by one server-direction row over the same
    wire, proving the request actually reached the server rather than being
    short-circuited by the Inspector SDK's own client-side validation."""
    rows = [json.loads(line) for line in observation.read_text().splitlines() if line]
    inspector_rows = [row for row in rows if row.get("client") == "inspector"]
    call_indices = [i for i, row in enumerate(inspector_rows)
                    if row.get("direction") == "client" and row.get("method") == "tools/call"]
    tail = call_indices[-len(GENERIC_NEGATIVE_ROWS):]
    if len(tail) != len(GENERIC_NEGATIVE_ROWS):
        raise RuntimeError("protocol.jsonl does not carry one call per generic negative")
    confirmed: dict[str, bool] = {}
    for plan_row, index in zip(GENERIC_NEGATIVE_ROWS, tail):
        following = inspector_rows[index + 1] if index + 1 < len(inspector_rows) else None
        confirmed[plan_row["kind"]] = following is not None and following.get("direction") == "server"
    return confirmed


def contract_discrepancies(observed: dict[str, object], manifest: dict[str, object],
                           preview_names: frozenset[str]) -> tuple[list[dict[str, object]], list[dict[str, object]]]:
    """(stable discrepancies, preview discrepancies) between the live
    Inspector discovery and the frozen manifest, mirroring
    `scripts/contract-freeze.py verify`'s own stable/preview split."""
    stable_bad: list[dict[str, object]] = []
    preview_bad: list[dict[str, object]] = []
    for name in EXPECTED_TOOLS:
        live = observed.get(name)
        frozen = manifest["tools"].get(name)
        if live is None or frozen is None:
            (preview_bad if name in preview_names else stable_bad).append(
                {"tool": name, "field": "presence", "live": live is not None, "frozen": frozen is not None})
            continue
        mismatches = []
        for field in ("input_schema_sha256", "output_schema_sha256", "description_sha256"):
            if live[field] != frozen[field]:
                mismatches.append(field)
        if live["annotations"] != frozen["annotations"]:
            mismatches.append("annotations")
        expected_stability = "preview" if name in preview_names else "stable"
        if frozen["stability"] != expected_stability:
            mismatches.append("stability")
        for field in mismatches:
            (preview_bad if name in preview_names else stable_bad).append({"tool": name, "field": field})
    return stable_bad, preview_bad


def run_inspector(attempt: pathlib.Path, mode: str, argv: list[str], timeout: int,
                  observed_version: str | None, write_root: pathlib.Path | None = None) -> dict[str, object]:
    m3 = load_m3()
    observation = attempt / "protocol.jsonl"
    state = pathlib.Path(argv[argv.index("--state-root") + 1])
    state.mkdir(mode=0o700, parents=True, exist_ok=True)
    bridge = BRIDGE_DIR / f"m8-{attempt.name}-{mode}-bridge.mjs"
    suffix = b"\nexport { InspectorClient, createTransportNode };\n"
    with bridge.open("xb") as stream:
        stream.write(INSPECTOR.read_bytes() + suffix)
    proxy_argv = [sys.executable, str(pathlib.Path(__file__).resolve()), "proxy",
                  "--client", "inspector", "--observation", str(observation),
                  "--server-argv-json", json.dumps(argv, separators=(",", ":"))]
    manifest = load_freeze_manifest()
    # The Docker-free negative call plan asserts every `stable` tool refuses
    # for lack of a calibrated runtime/catalog/grant; a `runtime` session
    # runs those same calls against a host that *has* a real runtime, so the
    # refusals this plan expects (e.g. `rust.project.inspect`'s
    # `TOOL_NOT_INSTALLED`) would instead land `passed`. Only the `docker_free`
    # session plans and validates these rows; `runtime` composes its own
    # positive oracle below (`rust.check`, its Resource, cancellation).
    negative_rows = negative_call_plan() if mode == DOCKER_FREE else []
    generic_negatives = generic_negative_plan() if mode == DOCKER_FREE else []
    plan = {
        "mode": mode, "expected_tools": list(EXPECTED_TOOLS),
        "preview_tools": list(PREVIEW_TOOLS), "freeze": manifest["tools"],
        "fixture": str(ROOT / FIXTURE), "unknown_project_ref": UNKNOWN_PROJECT_REF,
        "request_timeout_ms": MCP_TOOL_TIMEOUT_MS,
        "negative_rows": negative_rows, "generic_negatives": generic_negatives,
        "write_root": str(write_root) if write_root else None,
    }
    try:
        result = m3.run_bounded(
            [str(NODE), str(SESSION), str(bridge),
             json.dumps(proxy_argv, separators=(",", ":")), json.dumps(plan, separators=(",", ":"))],
            attempt, timeout, attempt / f"inspector-{mode}-session.json",
        )
    finally:
        bridge.unlink(missing_ok=True)
    if result["exit_code"] != 0:
        raise RuntimeError(f"Inspector M8 {mode} session failed")
    outcome = json.loads((attempt / f"inspector-{mode}-session.stdout").read_text())
    if outcome.get("tool_count") != len(EXPECTED_TOOLS) or outcome.get("discovery") is not True:
        raise RuntimeError(f"Inspector M8 {mode} discovery oracle incomplete")
    if outcome.get("resources_list") != []:
        raise RuntimeError("resources/list is not empty")
    stable_bad, preview_bad = contract_discrepancies(outcome["contract"], manifest, frozenset(PREVIEW_TOOLS))
    if mode == DOCKER_FREE:
        negatives = validate_negative_rows(outcome.get("negative_rows"), negative_rows)
        generics = validate_generic_negative_rows(outcome.get("generic_negatives"))
        wire_confirmed = generic_negative_wire_confirmed(observation)
        if not all(wire_confirmed.values()):
            raise RuntimeError(
                "a protocol-boundary generic negative never reached the wire: "
                + ", ".join(kind for kind, ok in wire_confirmed.items() if not ok))
    else:
        if outcome.get("negative_rows") or outcome.get("generic_negatives"):
            raise RuntimeError("a runtime session must not run the Docker-free negative plan")
        negatives, generics, wire_confirmed = [], [], {}
    report: dict[str, object] = {
        "version": observed_version, "mode": mode,
        "bundle_sha256": m3.file_digest(INSPECTOR), "bridge_suffix_sha256": m3.digest(suffix),
        "session": result,
        "contract_equality": len(stable_bad) == 0,
        "stable_discrepancies": stable_bad, "preview_discrepancies": preview_bad,
        "resources_list_empty": True,
        "negative_rows": negatives, "generic_negatives": generics,
        "generic_negatives_wire_confirmed": wire_confirmed,
        "wire_confirmation": "positional",
        "wire_confirmation_note": (
            "the server-direction row following each call is matched by wire "
            "position, not by request id -- the proxy's safe_keys does not "
            "record `id`, so this is sound for the Inspector's sequential "
            "session but would not hold against a client that interleaves calls"
        ),
    }
    if mode == RUNTIME:
        if outcome.get("runtime_check_status") != "passed":
            raise RuntimeError("Inspector runtime rust.check row did not pass")
        if outcome.get("resource_read_ok") is not True:
            raise RuntimeError("Inspector did not read back the rust-artifact:// Resource")
        if outcome.get("cancel_ok") is not True:
            raise RuntimeError("Inspector did not observe the cancel-then-clean-retry oracle")
        cancellation_wire_confirmed = runtime_cancellation_wire_confirmed(observation)
        if not cancellation_wire_confirmed:
            raise RuntimeError("the cancelled call's notifications/cancelled never reached the wire")
        if outcome.get("eof_new_session_ok") is not True:
            raise RuntimeError("a fresh session after the client's mid-call EOF did not see the tool inventory")
        orphans = assert_no_orphan_server(state)
        if orphans:
            raise RuntimeError(
                "a rust-engineering-mcp process orphaned by the client's mid-call EOF is still "
                f"running for state-root {state}: {orphans}")
        report.update(
            runtime_check_status=outcome["runtime_check_status"],
            resource_read_ok=True, cancel_ok=True,
            cancellation_wire_confirmed=True,
            eof_new_session_ok=True, eof_prior_pid=outcome.get("eof_prior_pid"),
            eof_no_orphan_process=True,
        )
    return report


def runtime_cancellation_wire_confirmed(observation: pathlib.Path) -> bool:
    """The `runtime` Inspector session's own G4 oracle cancels a mid-flight
    `rust.check` via the SDK's `cancelToolCall()`, which (on stdio) carries
    the cancellation to the server as a `notifications/cancelled` client
    row -- this must be confirmed on the wire itself, never inferred only
    from the local promise rejecting."""
    if not observation.is_file():
        return False
    rows = [json.loads(line) for line in observation.read_text().splitlines() if line]
    return any(row.get("client") == "inspector" and row.get("direction") == "client"
               and row.get("method") == "notifications/cancelled" for row in rows)


def assert_no_orphan_server(state: pathlib.Path, timeout: float = 10.0) -> list[str]:
    """G5's own oracle: after the `runtime` Inspector session ends (its
    mid-call stdin-EOF teardown and the fresh session that followed it), no
    `rust-engineering-mcp serve` process scoped to this session's own
    `--state-root` may still be running. `pgrep -f` matches full command
    lines, so the state-root's own path -- unique per attempt -- is enough
    of a needle without matching an unrelated session. Polls briefly: the
    server's own teardown on EOF is asynchronous from this process's point
    of view."""
    deadline = time.monotonic() + timeout
    while True:
        try:
            result = subprocess.run(["/usr/bin/pgrep", "-f", str(state)],
                                    capture_output=True, text=True, timeout=5)
        except FileNotFoundError:
            return []
        pids = [line for line in result.stdout.splitlines() if line.strip()]
        if not pids or time.monotonic() >= deadline:
            return pids
        time.sleep(0.2)


def eof_gate(socket: pathlib.Path, timeout: int = 30) -> dict[str, object]:
    """Send one in-flight `tools/call`, close stdin (EOF) without reading the
    response, and require the server to exit within `timeout` seconds. Only
    meaningful with a real runtime configured: it proves the gateway tears
    down its worker on a client that vanishes mid-call, not merely on a
    graceful `shutdown`."""
    with tempfile.TemporaryDirectory(prefix="rust-mcp-m8-eof-", dir=str(ROOT / "target")) as tmp:
        state = pathlib.Path(tmp) / "state"
        argv = server_argv(state, socket)
        child = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                 stderr=subprocess.DEVNULL, env=os.environ.copy())
        assert child.stdin is not None
        init = {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": "2026-07-28", "capabilities": {},
            "clientInfo": {"name": "rust-mcp-m8-eof-probe", "version": "1"},
        }}
        opened = {"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {
            "name": "rust.check", "arguments": {"project_ref": UNKNOWN_PROJECT_REF},
        }}
        try:
            child.stdin.write((json.dumps(init) + "\n").encode())
            child.stdin.write((json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n").encode())
            child.stdin.write((json.dumps(opened) + "\n").encode())
            child.stdin.flush()
        finally:
            child.stdin.close()
        started = time.monotonic()
        try:
            child.wait(timeout=timeout)
            exited_within_bound = True
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait(timeout=10)
            exited_within_bound = False
        return {
            "exited_on_eof": exited_within_bound,
            "exit_code": child.returncode,
            "seconds": round(time.monotonic() - started, 3),
        }


def compose_prior_receipts(socket: str) -> dict[str, object]:
    """Reuse, never re-implement: fold the M2-M6 harnesses' own `--run`
    receipts (their own positive coverage) into this gate's evidence."""
    composed: dict[str, object] = {}
    for script in ("test-m2-clients.py", "test-m3-clients.py", "test-m4-clients.py",
                   "test-m5-clients.py", "test-m6-clients.py"):
        path = ROOT / "scripts" / script
        if not path.is_file():
            composed[script] = {"status": "unavailable", "reason": "harness missing"}
            continue
        result = subprocess.run(
            [sys.executable, "-B", str(path), "--run", "--docker-socket", socket],
            cwd=ROOT, capture_output=True, text=True, timeout=3600, check=False,
        )
        composed[script] = {
            "exit_code": result.returncode,
            "status": "passed" if result.returncode == 0 else "failed",
            "stdout_tail": result.stdout[-2000:], "stderr_tail": result.stderr[-2000:],
        }
    return composed


def next_attempt() -> pathlib.Path:
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


def toml_string(value: str) -> str:
    return '"' + value.replace("\\", "\\\\").replace('"', '\\"') + '"'


def codex_mcp_config_args(server_name: str, command: str, args: list[str]) -> list[str]:
    """The `-c mcp_servers.<name>.command=...`/`args=[...]` overrides `codex
    exec` accepts: an ephemeral MCP server registration, scoped to one
    invocation, that never touches `~/.codex/config.toml`."""
    args_toml = "[" + ",".join(toml_string(a) for a in args) + "]"
    return [
        "-c", f"mcp_servers.{server_name}.command={toml_string(command)}",
        "-c", f"mcp_servers.{server_name}.args={args_toml}",
    ]


def codex_protocol_evidence(observation: pathlib.Path) -> dict[str, object]:
    """The Codex oracle, derived only from the wire this run's own proxy
    recorded for `client == "codex"`: `rust.project.open` and
    `rust.project.inspect` must both have been called, and the mandatory
    negative -- `rust.project.inspect` on the well-formed but never-opened
    `UNKNOWN_PROJECT_REF` -- must land on a structured `PROJECT_NOT_FOUND`
    refusal observed on the wire itself (`structuredContent.status`/
    `.error_code` on the server-direction row immediately following the
    call), not merely in the model's own transcript. The unknown-tool call
    is optional and informative only: a model that discovers the real
    36-tool inventory correctly never emits it, so its absence is by design,
    not a failure."""
    if not observation.is_file():
        return {"called_tools": set(), "unknown_tool_wire_refused": False,
                "unknown_project_ref_wire_refused": False}
    rows = [json.loads(line) for line in observation.read_text().splitlines() if line]
    codex_rows = [row for row in rows if row.get("client") == "codex"]
    calls = [row for row in codex_rows
             if row.get("direction") == "client" and row.get("method") == "tools/call"]
    called_tools = {row.get("tool") for row in calls}
    unknown_tool_wire_refused = False
    unknown_project_ref_wire_refused = False
    for index, row in enumerate(codex_rows):
        if not (row.get("direction") == "client" and row.get("method") == "tools/call"):
            continue
        following = codex_rows[index + 1] if index + 1 < len(codex_rows) else None
        if following is None or following.get("direction") != "server":
            continue
        if row.get("tool") == "rust.not.a.real.tool":
            unknown_tool_wire_refused = True
        if (row.get("tool") == "rust.project.inspect"
                and following.get("structuredContent.status") in REFUSAL_STATUSES
                and following.get("structuredContent.error_code") == "PROJECT_NOT_FOUND"):
            unknown_project_ref_wire_refused = True
    return {"called_tools": called_tools, "unknown_tool_wire_refused": unknown_tool_wire_refused,
            "unknown_project_ref_wire_refused": unknown_project_ref_wire_refused}


def codex_classification(returncode: int, stderr: str, open_observed: bool, inspect_observed: bool,
                          unknown_project_ref_wire_refused: bool) -> str:
    """`passed` requires the `PROJECT_NOT_FOUND` refusal observed on the wire
    itself; a refusal seen only in the model's own transcript does not
    count -- that is the weak oracle C-2 asked to retire. The optional
    unknown-tool step never gates this classification."""
    if returncode == 0 and open_observed and inspect_observed and unknown_project_ref_wire_refused:
        return "passed"
    return "capacity_refused" if "capacity" in stderr.lower() else "partial"


def codex_gate(attempt: pathlib.Path, socket: pathlib.Path, observed_version: str | None,
               timeout: int = 600) -> dict[str, object]:
    """One stock `codex exec` turn: discovery (36) -> project.open -> one
    read -> one negative -> summary, over the same Docker-free host, routed
    through the closed proxy so every wire message is recorded. ``timeout``
    is 600s for the Docker-free host and 900s for ``--with-runtime`` (a real
    Docker socket makes ``rust.project.inspect`` a genuine container call,
    not a host-level refusal, so the model's own turn runs longer)."""
    m3 = load_m3()
    observation = attempt / "protocol.jsonl"
    state = attempt / "state-codex"
    state.mkdir(mode=0o700)
    proxy_argv = [sys.executable, str(pathlib.Path(__file__).resolve()), "proxy",
                  "--client", "codex", "--observation", str(observation),
                  "--server-argv-json", json.dumps(server_argv(state, socket), separators=(",", ":"))]
    source_home = pathlib.Path(os.environ.get("CODEX_HOME", pathlib.Path.home() / ".codex"))
    auth_source = source_home / "auth.json"
    private_home = pathlib.Path(tempfile.mkdtemp(prefix="rust-mcp-m8-codex-home-", dir=str(ROOT / "target")))
    os.chmod(private_home, 0o700)
    if not auth_source.is_file():
        shutil.rmtree(private_home, ignore_errors=True)
        raise RuntimeError("Codex auth.json is unavailable")
    shutil.copyfile(auth_source, private_home / "auth.json")
    os.chmod(private_home / "auth.json", 0o600)
    env = os.environ.copy()
    env["CODEX_HOME"] = str(private_home)
    prompt = (
        f"Use only the configured rust_engineering MCP tools. List the available tools, "
        f"open {ROOT / FIXTURE}, call rust.project.inspect on the opened project, then call "
        f"rust.project.inspect again with project_ref \"{UNKNOWN_PROJECT_REF}\" -- a "
        f"well-formed reference this session never opened -- and report its refusal. "
        f"Optionally, if you want to, also call a tool named rust.not.a.real.tool and report "
        f"its refusal. Finish with a one-line summary of what happened. Do not use any "
        f"non-MCP capability."
    )
    argv = [
        str(CODEX), "exec", "--json", "--skip-git-repo-check", "-s", "read-only",
        "-m", CODEX_MODEL,
        *codex_mcp_config_args(CLAUDE_SERVER, sys.executable, proxy_argv[1:]),
        prompt,
    ]
    try:
        result = subprocess.run(argv, cwd=ROOT, env=env, capture_output=True, text=True,
                                timeout=timeout, check=False)
    finally:
        shutil.rmtree(private_home, ignore_errors=True)
    events_path = attempt / "codex-events.jsonl"
    events_path.write_text(result.stdout)
    assert_no_credential_text(events_path)
    events = [json.loads(line) for line in result.stdout.splitlines() if line.strip().startswith("{")]
    evidence = codex_protocol_evidence(observation)
    unknown_tool_event_refused = any(
        "rust.not.a.real.tool" in str(value)
        for event in events
        for value in (*m3.find_values(event, "tool"), *m3.find_values(event, "name")))
    open_observed = "rust.project.open" in evidence["called_tools"]
    inspect_observed = "rust.project.inspect" in evidence["called_tools"]
    unknown_tool_wire_refused = evidence["unknown_tool_wire_refused"]
    unknown_project_ref_wire_refused = evidence["unknown_project_ref_wire_refused"]
    classification = codex_classification(
        result.returncode, result.stderr, open_observed, inspect_observed,
        unknown_project_ref_wire_refused)
    return {
        "version": observed_version, "model": CODEX_MODEL, "effort": CODEX_EFFORT,
        "exit_code": result.returncode, "classification": classification,
        "tool_calls_observed": sorted(tool for tool in evidence["called_tools"] if tool),
        "protocol_evidence": {
            "project_open_observed": open_observed,
            "project_inspect_observed": inspect_observed,
            "unknown_project_ref_refused_on_wire": unknown_project_ref_wire_refused,
            "unknown_tool_refused_on_wire": unknown_tool_wire_refused,
            "unknown_tool_event_refused": unknown_tool_event_refused,
        },
        "events_sha256": m3.file_digest(events_path),
    }


CLAUDE_SYSTEM_PROMPT = (
    "You are a bounded third-party MCP client qualifier. Use only the explicitly "
    "configured rust_engineering MCP server. Perform the requested steps exactly "
    "once each, in the given order, never retry a call even when it is refused, "
    "and finish with a short account of every returned status."
)


def claude_environment(private: pathlib.Path) -> dict[str, str]:
    environment = {
        "HOME": os.environ["HOME"],
        "PATH": "/Users/cburgosro/.local/bin:/usr/bin:/bin:/usr/sbin:/sbin",
        "TMPDIR": str(private / "tmp"), "LANG": "en_US.UTF-8",
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


def claude_gate(attempt: pathlib.Path, socket: pathlib.Path, with_runtime: bool,
                observed_version: str | None) -> dict[str, object]:
    """One model-directed turn of stock Claude Code over a subset: discovery,
    two reads, one negative and (with a real runtime) one Resource read."""
    m3 = load_m3()
    m6 = load_m6()
    observation = attempt / "protocol.jsonl"
    state = attempt / "state-claude"
    state.mkdir(mode=0o700)
    argv = server_argv(state, socket)
    proxy = [sys.executable, str(pathlib.Path(__file__).resolve()), "proxy",
             "--client", CLAUDE_CLIENT, "--observation", str(observation),
             "--server-argv-json", json.dumps(argv, separators=(",", ":"))]
    private = pathlib.Path(tempfile.mkdtemp(prefix="rust-mcp-m8-claude-", dir=str(ROOT / "target")))
    os.chmod(private, 0o700)
    events_path = attempt / "claude-model-events.jsonl"
    stderr_path = attempt / "claude-model.stderr"
    try:
        for child in ("cwd", "tmp"):
            (private / child).mkdir(mode=0o700)
        config = private / "mcp.json"
        m3.save_json(config, {"mcpServers": {CLAUDE_SERVER: {
            "command": proxy[0], "args": proxy[1:], "env": {}}}}, exclusive=True)
        prompt = (
            f"Use only the configured rust_engineering MCP server. Perform these steps exactly "
            f"once each, in order, without retrying any call. (1) Call rust.project.open with "
            f"path {ROOT / FIXTURE}; keep its data.project_ref. (2) Call rust.project.inspect "
            f"with that project_ref. (3) Call rust.catalog.status with no arguments. (4) Call a "
            f"tool named rust.not.a.real.tool and report its refusal. (5) Report the status and "
            f"error_code of every call exactly as returned. Do not call any other capability."
        )
        claude_argv = [
            str(CLAUDE), "--print", "--output-format", "stream-json", "--verbose",
            "--model", CLAUDE_MODEL, "--effort", CLAUDE_EFFORT, "--no-session-persistence",
            "--restricted", "--setting-sources", "", "--strict-mcp-config",
            "--mcp-config", str(config), "--disable-slash-commands", "--no-chrome",
            "--tools", "", "--allowedTools", f"mcp__{CLAUDE_SERVER}",
            "--permission-mode", "dontAsk", "--permission-prompts", "none",
            "--max-turns", "12", "--system-prompt", CLAUDE_SYSTEM_PROMPT, prompt,
        ]
        outcome = run_claude(claude_argv, private / "cwd", claude_environment(private), 600)
        events_path.write_bytes(outcome["stdout"])
        stderr_path.write_bytes(outcome["stderr"])
        assert_no_credential_text(events_path)
        assert_no_credential_text(stderr_path)
        if outcome["timed_out"] or outcome["exit_code"] != 0:
            return {
                "status": "unavailable", "reason": f"Claude turn failed: exit {outcome['exit_code']}, "
                                                    f"timed_out={outcome['timed_out']}",
                "exit_code": outcome["exit_code"],
            }
        events = [json.loads(line) for line in outcome["stdout"].splitlines() if line.strip()]
        pinned_from = m6.CLAUDE_VERSION
        m6.CLAUDE_VERSION = CLAUDE_VERSION
        try:
            init, items, final = m6.claude_items(events)
            session = m6.validate_claude_session(init, final, events)
        except Exception as error:
            # Transcript-shape or pinned-version drift against the shared M6
            # validator is this optional client's own failure, never a reason
            # to abort the Codex/Gemini turns still owed by this run.
            return {
                "status": "unavailable",
                "reason": f"Claude session did not validate: {error}",
                "model_events_sha256": m3.file_digest(events_path),
            }
        m3.assert_no_credentials(private)
        tool_calls_observed = sorted({item["tool"] for item in items if isinstance(item.get("tool"), str)})
        return {
            "status": "passed", "client": CLAUDE_CLIENT, "version": observed_version,
            "model": CLAUDE_MODEL, "effort": CLAUDE_EFFORT,
            "executable_sha256": m3.file_digest(CLAUDE),
            "tool_calls": len(items), "tool_calls_observed": tool_calls_observed,
            "session": session, "model_events_sha256": m3.file_digest(events_path),
            "validator_version_pin_overridden_from": pinned_from,
        }
    finally:
        shutil.rmtree(private, ignore_errors=True)


def gemini_gate(attempt: pathlib.Path, socket: pathlib.Path, observed_version: str | None) -> dict[str, object]:
    """One `agy` turn over the real HOME (``agy`` keeps its credentials
    there, so a private HOME can never authenticate); only the MCP server
    registration is isolated -- one uniquely named server added before the
    turn and removed in ``finally``, with ``agy mcp list`` captured on both
    sides of that bracket so no other configured server is disturbed."""
    m3 = load_m3()
    observation = attempt / "protocol.jsonl"
    state = attempt / "state-gemini"
    state.mkdir(mode=0o700)
    proxy_argv = [sys.executable, str(pathlib.Path(__file__).resolve()), "proxy",
                  "--client", "gemini-cli", "--observation", str(observation),
                  "--server-argv-json", json.dumps(server_argv(state, socket), separators=(",", ":"))]
    env = os.environ.copy()
    name = f"rust_engineering_m8_{attempt.name.replace('-', '_')}"
    mcp_list_before = subprocess.run([str(AGY), "mcp", "list"], cwd=ROOT, env=env,
                                     capture_output=True, text=True, timeout=30, check=False)
    added = subprocess.run([str(AGY), "mcp", "add", name, sys.executable, "--", *proxy_argv[1:]],
                           cwd=ROOT, env=env, capture_output=True, text=True, timeout=30, check=False)
    if added.returncode != 0:
        mcp_list_after = subprocess.run([str(AGY), "mcp", "list"], cwd=ROOT, env=env,
                                        capture_output=True, text=True, timeout=30, check=False)
        return {
            "version": observed_version, "model": GEMINI_MODEL, "status": "unavailable",
            "reason": "agy mcp add did not succeed", "isolated_home": False,
            "evidence": (added.stdout + added.stderr)[-4000:],
            "mcp_list_before": mcp_list_before.stdout, "mcp_list_after": mcp_list_after.stdout,
        }
    enabled = subprocess.run([str(AGY), "mcp", "enable", name], cwd=ROOT, env=env,
                             capture_output=True, text=True, timeout=30, check=False)
    prompt = (
        f"Use only the configured {name} MCP tools. List the available tools, open "
        f"{ROOT / FIXTURE}, call rust.project.inspect on the opened project, then call "
        f"a tool named rust.not.a.real.tool and report its refusal."
    )
    try:
        result = subprocess.run(
            [str(AGY), "--model", GEMINI_MODEL, "-p", prompt, "--output-format", "json", "--sandbox"],
            cwd=ROOT, env=env, capture_output=True, text=True, timeout=600, check=False,
        )
    finally:
        subprocess.run([str(AGY), "mcp", "remove", name], cwd=ROOT, env=env,
                       capture_output=True, text=True, timeout=30, check=False)
        mcp_list_after = subprocess.run([str(AGY), "mcp", "list"], cwd=ROOT, env=env,
                                        capture_output=True, text=True, timeout=30, check=False)
    events_path = attempt / "gemini-events.json"
    events_path.write_text(result.stdout)
    assert_no_credential_text(events_path)
    common = {
        "version": observed_version, "model": GEMINI_MODEL, "isolated_home": False,
        "mcp_server_name": name, "mcp_enable_exit_code": enabled.returncode,
        "mcp_list_before": mcp_list_before.stdout, "mcp_list_after": mcp_list_after.stdout,
    }
    if result.returncode != 0:
        return {
            **common, "status": "unavailable",
            "reason": "agy -p did not complete successfully",
            "exit_code": result.returncode, "evidence_sha256": m3.file_digest(events_path),
            "evidence": (result.stdout + result.stderr)[-4000:],
        }
    try:
        parsed = json.loads(result.stdout)
    except json.JSONDecodeError:
        parsed = None
    denied = [entry for entry in (parsed.get("denied_actions") if isinstance(parsed, dict) else [])
             if isinstance(entry, dict) and entry.get("action") == "mcp"]
    if denied:
        return {
            **common, "status": "unavailable",
            "reason": "agy -p denied the MCP tool call in headless mode with no permission rule "
                      "configured (not forced with --dangerously-skip-permissions)",
            "denied_actions": denied, "evidence_sha256": m3.file_digest(events_path),
            "evidence": result.stdout[-4000:],
        }
    tool_calls = sorted({str(v) for v in (m3.find_values(parsed, "tool") if parsed is not None else [])
                        if isinstance(v, str)})
    observed_open = any("rust.project.open" in call for call in tool_calls) or (
        "rust.project.open" in result.stdout)
    return {
        **common, "status": "passed" if observed_open else "partial",
        "tool_calls_observed": tool_calls, "evidence_sha256": m3.file_digest(events_path),
    }


def unavailable_turn(reason: str) -> dict[str, object]:
    return {"status": "unavailable", "classification": "unavailable", "reason": reason}


def codex_turn(attempt: pathlib.Path, socket: pathlib.Path, observed_version: str | None,
               timeout: int) -> dict[str, object]:
    """A model turn that expires or raises never kills the harness: it is
    recorded `unavailable` with its partial artifacts left on disk, and the
    remaining turns still run. The global `status` still requires Codex's
    own `classification` to land `passed` (W28c)."""
    try:
        return codex_gate(attempt, socket, observed_version, timeout=timeout)
    except subprocess.TimeoutExpired:
        return unavailable_turn(f"timeout after {timeout} s")
    except Exception as error:
        return unavailable_turn(f"{type(error).__name__}: {error}")


def claude_turn(attempt: pathlib.Path, socket: pathlib.Path, with_runtime: bool,
                observed_version: str | None, timeout: int = 600) -> dict[str, object]:
    """See `codex_turn`: `claude_gate` already turns its own subprocess
    timeout into an `unavailable` status internally, but anything raised
    before or after that call (e.g. a private-HOME setup failure) is still
    caught here so this optional client can never abort the run."""
    try:
        return claude_gate(attempt, socket, with_runtime, observed_version)
    except subprocess.TimeoutExpired:
        return unavailable_turn(f"timeout after {timeout} s")
    except Exception as error:
        return unavailable_turn(f"{type(error).__name__}: {error}")


def gemini_turn(attempt: pathlib.Path, socket: pathlib.Path, observed_version: str | None,
                timeout: int = 600) -> dict[str, object]:
    """See `codex_turn`: `gemini_gate` cleans up its MCP server registration
    in a `finally`, but a `subprocess.run` timeout still raises through it,
    so it is caught here rather than aborting the run."""
    try:
        return gemini_gate(attempt, socket, observed_version)
    except subprocess.TimeoutExpired:
        return unavailable_turn(f"timeout after {timeout} s")
    except Exception as error:
        return unavailable_turn(f"{type(error).__name__}: {error}")


def save_receipt(attempt: pathlib.Path, receipt: dict[str, object], final: bool) -> None:
    """Write `receipt.json` after every block, not only at the end: a
    harness death between blocks (a model turn hanging past its own bounded
    timeout, killed from outside) still leaves the evidence gathered so far
    on disk instead of nothing (W28c). Every write but the last reports
    `status: "running"`."""
    m3 = load_m3()
    snapshot = dict(receipt)
    if not final:
        snapshot["status"] = "running"
    m3.save_json(attempt / "receipt.json", snapshot, exclusive=False)


def run(with_runtime: bool, socket: str | None) -> int:
    m3 = load_m3()
    if with_runtime and (not socket or not pathlib.Path(socket).is_absolute()):
        raise RuntimeError("--with-runtime requires an absolute --docker-socket")
    versions = client_versions()
    checks = preconditions(versions, with_runtime, socket)
    blocking = mandatory_unsatisfied(checks)
    if blocking:
        raise RuntimeError("mandatory preconditions unsatisfied: " + ", ".join(blocking))
    attempt = next_attempt()
    receipt: dict[str, object] = {
        "schema": "rust-mcp-m8-clients-v1", "status": "failed", "attempt": attempt.name,
        "server_version": server_version(), "rust_image": RUST_IMAGE,
        "candidate": {"server_sha256": m3.file_digest(SERVER)},
        "head_commit": git_head_commit(), "tree_dirty": git_tree_dirty(),
        "observed_versions": versions,
    }
    try:
        # The Docker-free Inspector session's own negative call plan (30
        # refusals asserting the runtime is unreachable) must run against a
        # host whose socket genuinely never resolves -- even in
        # `--with-runtime`, where a real, absolute socket is otherwise in
        # play for the model turns below. Sharing one socket between the two
        # was M8-04's own `--with-runtime` regression: a real runtime made
        # the Docker-free refusals land `passed` instead.
        docker_free_socket = ROOT / "target" / f"m8-never-{attempt.name}.sock"
        model_turn_socket = pathlib.Path(socket) if with_runtime else docker_free_socket
        docker_free_argv = server_argv(attempt / "state-docker-free", docker_free_socket)
        # Deterministic, authoritative evidence first (W28c): both Inspector
        # sessions run before any model turn, so a model turn that hangs past
        # its own bounded timeout can never take that evidence down with it.
        receipt["inspector"] = {DOCKER_FREE: run_inspector(
            attempt, DOCKER_FREE, docker_free_argv, 900, versions["inspector"]["observed"])}
        save_receipt(attempt, receipt, final=False)
        if with_runtime:
            runtime_socket = pathlib.Path(socket)
            runtime_argv = server_argv(attempt / "state-runtime", runtime_socket)
            receipt["inspector"][RUNTIME] = run_inspector(
                attempt, RUNTIME, runtime_argv, 900, versions["inspector"]["observed"])
            save_receipt(attempt, receipt, final=False)
        # Docker makes `rust.project.inspect` a genuine container call in
        # `--with-runtime`, not a host-level refusal, so Codex's own turn is
        # given 900s there instead of 600s (W28c).
        codex_timeout = 900 if with_runtime else 600
        receipt["codex"] = codex_turn(
            attempt, model_turn_socket, versions["codex"]["observed"], codex_timeout)
        save_receipt(attempt, receipt, final=False)
        receipt["claude_code"] = claude_turn(
            attempt, model_turn_socket, with_runtime, versions["claude_code"]["observed"])
        save_receipt(attempt, receipt, final=False)
        receipt["gemini_cli"] = gemini_turn(
            attempt, model_turn_socket, versions["gemini_cli"]["observed"])
        save_receipt(attempt, receipt, final=False)
        if with_runtime:
            receipt["eof_gate"] = eof_gate(runtime_socket)
            receipt["composed_prior_positives"] = compose_prior_receipts(socket)
            save_receipt(attempt, receipt, final=False)
        receipt["protocol"] = validate_protocol_metadata(attempt / "protocol.jsonl")
        codex_ok = receipt["codex"].get("classification") == "passed"
        claude_ok = receipt["claude_code"].get("status") == "passed"
        gemini_ok = receipt["gemini_cli"].get("status") == "passed"
        inspector_ok = (
            receipt["inspector"][DOCKER_FREE]["contract_equality"]
            and (not with_runtime or receipt["inspector"][RUNTIME]["contract_equality"])
        )
        # Mandatory: Inspector + Codex. Claude Code / Gemini CLI are qualified
        # before being announced but an `unavailable` optional client never
        # fails the gate and is never itself announced as qualified.
        failed_clients = [name for name, ok in (
            ("inspector", inspector_ok), ("codex", codex_ok),
        ) if not ok]
        receipt["status"] = "passed" if not failed_clients else "failed"
        receipt["qualified"] = {
            "inspector": inspector_ok, "codex": codex_ok,
            "claude_code": "passed" if claude_ok else "unavailable",
            "gemini_cli": "passed" if gemini_ok else "unavailable",
        }
    except Exception as error:
        receipt["error"] = {"type": type(error).__name__, "message": str(error)}
        raise
    finally:
        leak = None
        try:
            m3.assert_no_credentials(attempt)
            receipt["evidence_credential_scan"] = "clean"
        except RuntimeError as scan_error:
            leak = scan_error
            receipt["evidence_credential_scan"] = str(scan_error)
            receipt["status"] = "failed"
        save_receipt(attempt, receipt, final=True)
        if leak is not None:
            raise leak
        if receipt["status"] == "passed":
            if CURRENT.exists():
                raise RuntimeError("current client receipt exists; preserve it before rerun")
            m3.save_json(CURRENT, receipt, exclusive=True)
    return 0 if receipt["status"] == "passed" else 1


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subcommands = parser.add_subparsers(dest="command")
    proxy_parser = subcommands.add_parser("proxy")
    proxy_parser.add_argument("--client", required=True)
    proxy_parser.add_argument("--observation", required=True)
    proxy_parser.add_argument("--server-argv-json", required=True)
    parser.add_argument("--preflight", action="store_true")
    parser.add_argument("--run", action="store_true")
    parser.add_argument("--with-runtime", action="store_true")
    parser.add_argument("--docker-socket", default=os.environ.get("RUST_MCP_TEST_SOCKET"))
    options = parser.parse_args()
    if options.command == "proxy":
        argv = json.loads(options.server_argv_json)
        if not isinstance(argv, list) or not argv or any(not isinstance(item, str) for item in argv):
            raise RuntimeError("invalid closed server argv")
        return wire_proxy(argv, pathlib.Path(options.observation), options.client)
    if options.with_runtime and not (options.run or options.preflight):
        raise RuntimeError("--with-runtime requires --run or --preflight")
    if options.run:
        return run(options.with_runtime, options.docker_socket)
    print(json.dumps(preflight(options.with_runtime, options.docker_socket), sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
