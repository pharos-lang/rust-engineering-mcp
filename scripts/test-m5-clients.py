#!/usr/bin/env python3
"""Source-bound stock-client qualification harness for the four M5 tools.

The default invocation is a client-free, Docker-free preflight: it re-derives
the advertised inventory from the server sources, checks that every planned
call is answered exactly as the tool sources say, reports the host
preconditions and writes nothing.

There are two gate modes, both closed by default:

``--run``
    The Docker-free matrix.  Every call is a declared refusal the server
    produces before a container can exist.  The server is handed a socket path
    inside the harness's own private directory that is never created, and the
    gate proves afterwards that the path still does not exist, so "no container
    was created" is an assertion and not a claim.

``--run --with-runtime``
    The Docker-free matrix *plus* two real measurements through the qualified
    M5 runtime: a sampled CPU profile and a binary size analysis, each with its
    published artifacts read back as ``rust-quality-artifact://`` Resources.
    This mode needs the real Docker socket, the qualified image, an
    authenticated host cargo vendor tree and the host profiling grant, and it
    starts containers.  Every row in the receipt says which mode produced it.
"""
from __future__ import annotations

import argparse
import importlib.util
import json
import os
import pathlib
import queue
import re
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = pathlib.Path(__file__).resolve().parents[1]
M3_PATH = ROOT / "scripts/test-m3-clients.py"
SESSION = ROOT / "scripts/m5-inspector-session.mjs"
UNIT = ROOT / "scripts/test-m5-clients-unit.py"
CONTROLLER = ROOT / "docs/validation/m1-17-codex-client/controller.py"
ATTEMPTS = ROOT / "docs/validation/m5-clients"
CURRENT = ROOT / "docs/validation/M5-clients.json"
PREFLIGHT = ROOT / "docs/validation/M5-clients-preflight.json"
SERVER = ROOT / "target/release/rust-engineering-mcp"
NODE = pathlib.Path("/Users/cburgosro/.nvm/versions/node/v24.15.0/bin/node")
INSPECTOR = ROOT / "target/m1-17-inspector/node_modules/@modelcontextprotocol/inspector/clients/cli/build/index.js"
INSPECTOR_PACKAGE = ROOT / "target/m1-17-inspector/node_modules/@modelcontextprotocol/inspector/package.json"
DOCKER = pathlib.Path("/Applications/Docker.app/Contents/Resources/bin/docker")
STDIO = ROOT / "crates/mcp-server/src/stdio.rs"
STDIO_DIR = ROOT / "crates/mcp-server/src/stdio"
PROTOCOL_TEST = ROOT / "crates/mcp-server/tests/protocol.rs"
PERFORMANCE_PORT = ROOT / "crates/execution-adapter/src/performance_port.rs"
HOST_CONFIG = ROOT / "crates/mcp-server/src/host_config.rs"

INSPECTOR_VERSION = "2.5.0"
CODEX_VERSION = "codex-cli 0.153.0"
PROFILING_GRANT = "user-space-sampling"

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
PRIOR_TOOLS = M3_TOOLS + M4_TOOLS
EXPECTED_TOOLS = PRIOR_TOOLS + M5_TOOLS

READY_MARKERS = {
    "rust.benchmark.run": ("benchmark.rs", "RUST_MCP_TEST_BENCHMARK_READY"),
    "rust.benchmark.compare": ("benchmark_compare.rs", "RUST_MCP_TEST_BENCHMARK_COMPARE_READY"),
    "rust.profile.flamegraph": ("profile.rs", "RUST_MCP_TEST_PROFILE_READY"),
    "rust.binary.bloat": ("bloat.rs", "RUST_MCP_TEST_BLOAT_READY"),
}
TOOL_MODULES = {
    "rust.benchmark.run": "benchmark",
    "rust.benchmark.compare": "benchmark_compare",
    "rust.profile.flamegraph": "profile",
    "rust.binary.bloat": "bloat",
}

# Repository-relative fixture roots.  Only these names reach a receipt; the
# absolute host paths stay inside the closed server argv.
FIXTURES = {
    "benchmark": "fixtures/benchmark",
    "profile": "fixtures/profile-workload",
    "bloat": "fixtures/bloat",
}
# The runtime mode needs a host-authenticated offline vendor tree.  Both
# measured fixtures resolve entirely from path dependencies (their lockfiles
# name no registry package), so this tree only has to be an *approved* one; an
# empty directory is refused by `cargo-vendor inspect`, which is why the
# checked M4 tree is reused instead of inventing one.
VENDOR_FIXTURE = "fixtures/cargo-vendor-data/vendor"
UNKNOWN_PROJECT_REF = "prj_" + "0" * 32
MISSING_RESOURCE_URI = "rust-quality-artifact://" + "0" * 32 + "/index"

DOCKER_FREE = "docker_free"
RUNTIME = "runtime"

# Every Docker-free call is answered by a declared result the server produces
# before a container can exist, so that whole matrix is Docker-free by
# construction and not by omission.  `rationale` is published in the receipt.
CALL_PLAN = (
    {
        "tool": "rust.benchmark.run", "shape": "positive", "project": "benchmark",
        "arguments": {"package": "rust-mcp-benchmark-fixture", "bench_target": "perf",
                      "run_count": 1, "timeout_seconds": 60, "execution_mode": "synchronous"},
        "expect_status": "unavailable", "expect_error_code": "MISSING_OFFLINE_DATA",
        "rationale": "benchmark.rs answers the missing host-authenticated cargo vendor tree "
                     "before the worker, the executor and any container",
    },
    {
        "tool": "rust.benchmark.run", "shape": "negative", "project": "benchmark",
        "arguments": {"timeout_seconds": 60, "execution_mode": "task"},
        "expect_status": "blocked", "expect_error_code": "TASKS_REQUIRED",
        "rationale": "M5 owns no JobKind, so benchmark.rs refuses a Tasks request on decode, "
                     "before the runtime is even resolved",
    },
    {
        "tool": "rust.benchmark.compare", "shape": "positive", "project": "benchmark",
        "arguments": {"baseline_artifact_id": "qart_" + "0" * 32,
                      "candidate_artifact_id": "qart_" + "1" * 32, "timeout_seconds": 30},
        "expect_status": "blocked", "expect_error_code": "ARTIFACT_NOT_FOUND",
        "rationale": "the comparison runs no process and creates no container; two store-shaped "
                     "identifiers this project never published resolve to nothing",
    },
    {
        "tool": "rust.benchmark.compare", "shape": "negative", "project": None,
        "arguments": {"baseline_artifact_id": "qart_" + "0" * 32,
                      "candidate_artifact_id": "qart_" + "1" * 32, "timeout_seconds": 30},
        "expect_status": "blocked", "expect_error_code": "ARTIFACT_NOT_FOUND",
        "rationale": "an unopened project authority answers exactly like an unknown identifier; "
                     "the pair is indistinguishable on purpose and needs no container",
    },
    {
        "tool": "rust.profile.flamegraph", "shape": "positive", "project": "profile",
        "arguments": {"binary_target": "rust-mcp-profile-workload", "frequency_hz": 99,
                      "duration_seconds": 5, "timeout_seconds": 120,
                      "execution_mode": "synchronous"},
        "expect_status": "blocked", "expect_error_code": "PROFILING_NOT_AUTHORIZED",
        "rationale": "ADR-074 §2 decides the host grant before discovery, before the vendor tree "
                     "and before any container; this configuration grants none",
    },
    {
        "tool": "rust.profile.flamegraph", "shape": "negative", "project": "profile",
        "arguments": {"binary_target": "rust-mcp-profile-workload", "timeout_seconds": 120,
                      "execution_mode": "task"},
        "expect_status": "blocked", "expect_error_code": "TASKS_REQUIRED",
        "rationale": "the Tasks refusal precedes even the profiling grant, so no capability and "
                     "no container is consulted",
    },
    {
        "tool": "rust.binary.bloat", "shape": "positive", "project": "bloat",
        "arguments": {"binary_target": "rust-mcp-bloat-fixture", "package": "rust-mcp-bloat-fixture",
                      "profile": "release", "timeout_seconds": 120,
                      "execution_mode": "synchronous"},
        "expect_status": "unavailable", "expect_error_code": "MISSING_OFFLINE_DATA",
        "rationale": "bloat.rs answers the missing host-authenticated cargo vendor tree before "
                     "the worker, the analyzer and any container",
    },
    {
        "tool": "rust.binary.bloat", "shape": "negative", "project": "bloat",
        "arguments": {"binary_target": "rust-mcp-bloat-fixture", "timeout_seconds": 120,
                      "execution_mode": "task"},
        "expect_status": "blocked", "expect_error_code": "TASKS_REQUIRED",
        "rationale": "M5 owns no JobKind, so bloat.rs refuses a Tasks request on decode, before "
                     "the runtime is even resolved",
    },
)

# The two calls that produce a real measurement through the qualified runtime.
#
# `rust.profile.flamegraph` reaches `passed`: its observation is complete when
# the sampler ran, lost nothing and published both artifacts.
#
# `rust.binary.bloat` reaches `passed` under ADR-079, and used to be unable to.
# The parser caps functions at `BLOAT_MAX_ROWS = 256` and the native receipt for
# this exact image and fixture (docs/validation/M5-04-runtime.json) recorded 378
# omitted rows for `release` and 234 for `release-lto`; that cap used to become
# `Truncated`, then `observation.complete = false`, then
# `blocked`/`EVIDENCE_INCOMPLETE`, so the success path was unreachable for any
# binary linking `std`.  ADR-079 separates the three concepts: the cap is
# declared coverage in `attribution.ranking_cap`, the response budget is
# declared in `attribution.response_trim`, and only measurement validity decides
# the status.  This row therefore expects `passed` together with the exit, the
# exact measured file and the published artifact.  It is an expectation, not a
# qualification: the receipts are re-captured separately.
RUNTIME_CALL_PLAN = (
    {
        "tool": "rust.profile.flamegraph", "shape": "positive", "project": "profile",
        "arguments": {"binary_target": "rust-mcp-profile-workload", "frequency_hz": 99,
                      "duration_seconds": 2, "timeout_seconds": 300,
                      "execution_mode": "synchronous"},
        "expect_status": "passed", "expect_error_code": None,
        "expect_observation": {"build": "built", "completeness": "complete", "complete": True},
        "expect_positive_fields": ["samples_collected", "stacks_written", "frames_total"],
        "expect_zero_fields": ["samples_lost", "stacks_truncated"],
        "expect_measured": False,
        "expect_min_artifacts": 2,
        "report_fields": ["samples_collected", "samples_lost", "stacks_written",
                          "frames_total", "frames_unresolved", "modules_seen",
                          "completeness", "build"],
        "requires_profiling_grant": True,
        "rationale": "a real sampled CPU profile in the qualified image; both published "
                     "artifacts are read back as Resources",
    },
    {
        "tool": "rust.binary.bloat", "shape": "positive", "project": "bloat",
        "arguments": {"binary_target": "rust-mcp-bloat-fixture",
                      "package": "rust-mcp-bloat-fixture", "profile": "release",
                      "timeout_seconds": 300, "execution_mode": "synchronous"},
        "expect_status": "passed", "expect_error_code": None,
        "expect_observation": {"exit": "passed", "completeness": "complete",
                               "analysis_validated": True},
        "expect_positive_fields": [],
        "expect_zero_fields": [],
        "expect_measured": True,
        "expect_min_artifacts": 1,
        "report_fields": ["exit", "exit_code", "completeness", "analyzer_version",
                          "analysis_validated"],
        "requires_profiling_grant": False,
        "rationale": "a real size analysis in the qualified image; the measured file is exact "
                     "and the published attribution is read back as a Resource",
    },
)

SAFE_PROTOCOL_KEYS = frozenset({
    "client", "direction", "session", "bytes", "sha256", "malformed",
    "method", "tasks_declared", "tasks_advertised", "tool", "resource_scheme",
})
SAFE_CALL_KEYS = frozenset({
    "client", "tool", "shape", "mode", "status", "error_code", "is_error",
    "artifacts_published", "artifacts_read",
    "request_bytes", "request_sha256", "response_bytes", "response_sha256",
})
CALL_CLIENTS = frozenset({"inspector", "codex-app-server"})
CALL_SHAPES = frozenset({"positive", "negative"})
CALL_MODES = frozenset({DOCKER_FREE, RUNTIME})
CALL_STATUSES = frozenset({"passed", "failed", "blocked", "unavailable", "cancelled"})
ERROR_RESULT_STATUSES = frozenset({"blocked", "unavailable", "cancelled"})


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
    paths = (M3_PATH, pathlib.Path(__file__).resolve(), SESSION, UNIT, CONTROLLER)
    return {str(path.relative_to(ROOT)): m3.file_digest(path) for path in paths if path.is_file()}


def m5_image() -> str:
    """The one digest ADR-075/ADR-077 admit for a performance measurement."""
    match = re.search(r'M5_IMAGE: &str =\s*"(sha256:[0-9a-f]{64})"', PERFORMANCE_PORT.read_text())
    if match is None:
        raise RuntimeError("qualified M5 image digest is missing")
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


def advertised_push_order() -> tuple[str, ...]:
    """The order `stdio.rs::list_tools` actually pushes the switched tools."""
    text = STDIO.read_text()
    start = text.index("    async fn list_tools(")
    body = text[start:text.index("    async fn call_tool(", start)]
    names = []
    for module in re.findall(r"if (\w+)::advertised\(\) \{", body):
        source = (STDIO_DIR / f"{module}.rs").read_text()
        match = re.search(r'pub\(super\) const NAME: &str = "([^"]+)";', source)
        if match is None:
            raise RuntimeError(f"tool name constant is missing for {module}")
        names.append(match.group(1))
    if len(names) != len(set(names)) or not names:
        raise RuntimeError("advertised push order is invalid")
    return tuple(names)


def screaming(name: str) -> str:
    return re.sub(r"(?<!^)(?=[A-Z])", "_", name).upper()


def declared_error_codes(tool: str) -> tuple[str, ...]:
    """The closed `Code` vocabulary one M5 tool can publish."""
    source = (STDIO_DIR / f"{TOOL_MODULES[tool]}.rs").read_text()
    start = source.index("enum Code {") + len("enum Code {")
    block = source[start:source.index("\n}", start)]
    codes = tuple(screaming(name) for name in re.findall(r"^\s{4}([A-Z][A-Za-z0-9]*),$", block, re.M))
    if not codes or len(codes) != len(set(codes)):
        raise RuntimeError(f"error code vocabulary is invalid for {tool}")
    return codes


def advertisement_state() -> dict[str, bool]:
    shared = (STDIO_DIR / "security_tool.rs").read_text()
    shared_ready = (
        "pub(super) fn advertised(_test_variable: &str) -> bool" in shared
        and "    true\n}" in shared
    )
    state = {}
    for tool, (name, marker) in READY_MARKERS.items():
        source = (STDIO_DIR / name).read_text()
        if "const ADVERTISEMENT_READY: bool = true;" in source:
            state[tool] = True
        elif "const ADVERTISEMENT_READY: bool = false;" in source:
            state[tool] = False
        elif f'super::security_tool::advertised("{marker}")' in source and shared_ready:
            state[tool] = True
        else:
            raise RuntimeError(f"advertisement switch missing for {tool}")
    return state


def inventory_check() -> dict[str, object]:
    """A count or an order that drifts is a failure here, never a warning."""
    published = protocol_inventory()
    if published != EXPECTED_TOOLS:
        raise RuntimeError("advertised inventory drifted from the server protocol oracle")
    if len(EXPECTED_TOOLS) != 31 or len(set(EXPECTED_TOOLS)) != 31:
        raise RuntimeError("closed tool inventory is invalid")
    if EXPECTED_TOOLS[:27] != PRIOR_TOOLS:
        raise RuntimeError("the twenty-seven previous tools changed name or order")
    pushed = advertised_push_order()
    if pushed[:len(M4_TOOLS)] != M4_TOOLS or pushed[len(M4_TOOLS):] != M5_TOOLS:
        raise RuntimeError("stdio.rs pushes the switched tools in a different order")
    if EXPECTED_TOOLS[27:] != M5_TOOLS:
        raise RuntimeError("the four M5 tools are not appended in the pushed order")
    return {
        "count": len(EXPECTED_TOOLS),
        "previous_unchanged": True,
        "previous_count": len(PRIOR_TOOLS),
        "m5_appended": list(M5_TOOLS),
        "switched_push_order": list(pushed),
        "sources": ["crates/mcp-server/src/stdio.rs", "crates/mcp-server/tests/protocol.rs"],
    }


def check_expectation(row: dict[str, object]) -> None:
    """Both plans agree on this much: a closed tool, shape and error vocabulary."""
    if row["tool"] not in M5_TOOLS or row["shape"] not in CALL_SHAPES:
        raise RuntimeError("call plan names an unknown tool or shape")
    if row["expect_status"] not in CALL_STATUSES:
        raise RuntimeError("call plan names an unknown status")
    if row["expect_error_code"] is None:
        if row["expect_status"] in ERROR_RESULT_STATUSES:
            raise RuntimeError("a refused call must name the declared error code")
    elif row["expect_error_code"] not in declared_error_codes(row["tool"]):
        raise RuntimeError(f"{row['expect_error_code']} is not declared by {row['tool']}")
    if row["project"] is not None and row["project"] not in FIXTURES:
        raise RuntimeError("call plan names an unknown fixture root")
    if "project_ref" in row["arguments"]:
        raise RuntimeError("project authority is resolved by the client, never hard-coded")


def call_plan() -> list[dict[str, object]]:
    """The Docker-free matrix: nothing here may claim a measurement."""
    covered: dict[str, set[str]] = {tool: set() for tool in M5_TOOLS}
    rows = []
    for row in CALL_PLAN:
        check_expectation(row)
        if row["expect_status"] not in ERROR_RESULT_STATUSES:
            raise RuntimeError("call plan expects a result no Docker-free call can produce")
        covered[row["tool"]].add(row["shape"])
        rows.append({
            "tool": row["tool"], "shape": row["shape"], "mode": DOCKER_FREE,
            "project": row["project"], "arguments": row["arguments"],
            "expect_status": row["expect_status"],
            "expect_error_code": row["expect_error_code"],
            "expect_is_error": row["expect_status"] in ERROR_RESULT_STATUSES,
            "rationale": row["rationale"],
        })
    missing = sorted(tool for tool, shapes in covered.items() if shapes != CALL_SHAPES)
    if missing:
        raise RuntimeError("call plan does not cover both shapes for: " + ", ".join(missing))
    return rows


def runtime_call_plan() -> list[dict[str, object]]:
    """The Docker-backed matrix: every row is a real measurement with evidence."""
    rows = []
    for row in RUNTIME_CALL_PLAN:
        check_expectation(row)
        if row["shape"] != "positive":
            raise RuntimeError("the runtime plan carries positives only")
        if row["expect_min_artifacts"] < 1:
            raise RuntimeError("a real measurement must publish evidence")
        if not (row["expect_observation"] or row["expect_positive_fields"] or row["expect_measured"]):
            raise RuntimeError("a real measurement must assert observation facts")
        rows.append({
            "tool": row["tool"], "shape": row["shape"], "mode": RUNTIME,
            "project": row["project"], "arguments": row["arguments"],
            "expect_status": row["expect_status"],
            "expect_error_code": row["expect_error_code"],
            "expect_is_error": row["expect_status"] in ERROR_RESULT_STATUSES,
            "expect_observation": row["expect_observation"],
            "expect_positive_fields": row["expect_positive_fields"],
            "expect_zero_fields": row["expect_zero_fields"],
            "expect_measured": row["expect_measured"],
            "expect_min_artifacts": row["expect_min_artifacts"],
            "report_fields": row["report_fields"],
            "requires_profiling_grant": row["requires_profiling_grant"],
            "rationale": row["rationale"],
        })
    if not rows:
        raise RuntimeError("the runtime plan is empty")
    return rows


def runtime_tools() -> tuple[str, ...]:
    return tuple(dict.fromkeys(row["tool"] for row in RUNTIME_CALL_PLAN))


def candidate_advertises_m5() -> bool:
    """Necessary, not sufficient: the built candidate must carry the M5 names.

    The authoritative check is the client discovery oracle inside ``--run``;
    this one exists so the Docker-free preflight can refuse a stale binary
    loudly instead of failing halfway through a client session.
    """
    if not SERVER.is_file():
        return False
    needles = [tool.encode() for tool in M5_TOOLS]
    found = set()
    tail = b""
    with SERVER.open("rb") as stream:
        while block := stream.read(1024 * 1024):
            window = tail + block
            found.update(needle for needle in needles if needle in window)
            tail = block[-64:]
    return len(found) == len(needles)


def client_versions() -> dict[str, object]:
    package_version = None
    if INSPECTOR_PACKAGE.is_file():
        package_version = json.loads(INSPECTOR_PACKAGE.read_text()).get("version")
    codex_name = shutil.which("codex")
    codex_version = None
    if codex_name:
        result = subprocess.run([codex_name, "--version"], capture_output=True,
                                text=True, timeout=10, check=False)
        codex_version = result.stdout.strip()
    return {
        "inspector": {"expected": INSPECTOR_VERSION, "observed": package_version,
                      "tasks": True, "resource": True},
        "codex_app_server": {"expected": CODEX_VERSION, "observed": codex_version,
                             "tasks": False, "resource": True},
    }


def runtime_preconditions(socket: str | None) -> dict[str, tuple[bool, str]]:
    """What the Docker-backed mode needs beyond the Docker-free one."""
    host = HOST_CONFIG.read_text()
    path = pathlib.Path(socket) if socket else None
    return {
        "docker_socket": (
            path is not None and path.is_absolute() and path.exists(),
            "an absolute, existing Docker socket (--docker-socket or RUST_MCP_TEST_SOCKET)",
        ),
        "docker_binary": (DOCKER.is_file(), "the pinned docker executable must exist"),
        "vendor_fixture": (
            (ROOT / VENDOR_FIXTURE).is_dir(),
            f"the authenticated offline vendor tree {VENDOR_FIXTURE} must exist",
        ),
        "profiling_grant_supported": (
            f'"--allow-profiling"' in host and f'"{PROFILING_GRANT}"' in host,
            f"the host must accept --allow-profiling {PROFILING_GRANT}",
        ),
        "qualified_image_admitted": (
            "APPROVED_M5_IMAGE" in host,
            "the host must admit the qualified M5 image digest",
        ),
    }


def preconditions(versions: dict[str, object], with_runtime: bool,
                  socket: str | None) -> dict[str, dict[str, object]]:
    codex_home = pathlib.Path(os.environ.get("CODEX_HOME", pathlib.Path.home() / ".codex"))
    checks: dict[str, tuple[bool, str]] = {
        "candidate_server_binary": (
            SERVER.is_file(),
            "target/release/rust-engineering-mcp must exist",
        ),
        "candidate_advertises_m5": (
            candidate_advertises_m5(),
            "the built candidate must carry the four M5 tool names",
        ),
        "node": (NODE.is_file(), "the pinned Node runtime must exist"),
        "inspector_bundle": (INSPECTOR.is_file(), "the pinned Inspector CLI bundle must exist"),
        "inspector_version": (
            versions["inspector"]["observed"] == INSPECTOR_VERSION,
            f"the pinned Inspector must report {INSPECTOR_VERSION}",
        ),
        "codex_binary": (shutil.which("codex") is not None, "stock Codex must be on PATH"),
        "codex_version": (
            versions["codex_app_server"]["observed"] == CODEX_VERSION,
            f"stock Codex must report {CODEX_VERSION}",
        ),
        "codex_auth": ((codex_home / "auth.json").is_file(), "stock Codex must be authenticated"),
        "codex_controller": (CONTROLLER.is_file(), "the reusable app-server controller must exist"),
        "inspector_session": (SESSION.is_file(), "the M5 Inspector session driver must exist"),
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
    runtime = runtime_call_plan()
    return {
        "schema": "rust-mcp-m5-clients-preflight-v1",
        "status": "ready" if not unsatisfied else "blocked",
        "execution_performed": False,
        "clients_started": False,
        "with_runtime_requested": with_runtime,
        "docker_required": with_runtime,
        "docker_used": False,
        "image_id": m5_image(),
        "expected_tools": list(EXPECTED_TOOLS),
        "m5_tools": list(M5_TOOLS),
        "inventory": inventory_check(),
        "advertisement": advertisement_state(),
        "call_plan": call_plan(),
        "runtime_call_plan": runtime,
        "runtime_tools": list(runtime_tools()),
        "tools_without_a_client_positive": [
            tool for tool in M5_TOOLS if tool not in runtime_tools()
        ],
        "clients": versions,
        "preconditions": checks,
        "unsatisfied": unsatisfied,
        "resource_plan": {
            "docker_free": "no declared refusal publishes a quality artifact, so the Resource "
                           "oracle is the missing-artifact refusal on " + MISSING_RESOURCE_URI,
            "runtime": "every artifact a real measurement publishes is read back as a "
                       "rust-quality-artifact:// Resource",
        },
        "source_sha256": source_hashes(),
        "m3_reuse": ["proxy", "run_bounded", "digest", "file_digest", "save_json",
                     "protocol_summary", "assert_no_credentials", "find_values",
                     "append_observation", "controller.py transport"],
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
        for name in ("artifacts_published", "artifacts_read"):
            if not isinstance(row[name], int) or isinstance(row[name], bool) or row[name] < 0:
                raise RuntimeError("call row artifact count is invalid")
        if row["artifacts_read"] != row["artifacts_published"]:
            raise RuntimeError("call row did not read every artifact it published")
        if expected["mode"] == RUNTIME:
            if row["artifacts_published"] < expected["expect_min_artifacts"]:
                raise RuntimeError(f"{client} {row['tool']} published too few artifacts")
        elif row["artifacts_published"] != 0:
            raise RuntimeError("a Docker-free refusal must publish no artifact")
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
             "--state-root", str(state), "--rust-image", m5_image()]
    return argv


def server_argv(state: pathlib.Path, socket: pathlib.Path) -> list[str]:
    """The Docker-free host configuration.

    No `--cargo-vendor-dir` and no `--allow-profiling`: their absence is what
    makes the three measuring tools answer a declared refusal before a
    container, and `--state-root` is what attaches the durable store the
    comparison reads through.  The docker path and socket are required by the
    host runtime group; neither is ever executed or dialed.
    """
    return base_argv(state, socket)


def runtime_server_argv(state: pathlib.Path, socket: pathlib.Path,
                        vendor: pathlib.Path, fingerprint: str) -> list[str]:
    """The Docker-backed host configuration.

    Exactly three additions over the Docker-free one, each of which the
    handlers demand before they will measure: the host-authenticated offline
    vendor tree (`profile.rs` and `bloat.rs` both answer
    `unavailable`/`MISSING_OFFLINE_DATA` without it), its approved fingerprint,
    and the revocable profiling grant `profile.rs` checks before anything is
    dispatched.  The socket here is the real one.
    """
    return base_argv(state, socket) + [
        "--cargo-vendor-dir", str(vendor),
        "--cargo-vendor-tree-sha256", fingerprint,
        "--allow-profiling", PROFILING_GRANT,
    ]


def vendor_fingerprint(vendor: pathlib.Path) -> str:
    """Authenticate the offline tree with the candidate's own capture."""
    result = subprocess.run(
        [str(SERVER), "cargo-vendor", "inspect", "--directory", str(vendor), "--json"],
        capture_output=True, timeout=120, check=False)
    if len(result.stdout) > 1024 * 1024:
        raise RuntimeError("cargo vendor inspection produced an unbounded report")
    report = json.loads(result.stdout)
    fingerprint = report.get("tree_fingerprint")
    if report.get("status") != "passed" or not isinstance(fingerprint, str) \
            or not re.fullmatch(r"sha256:[0-9a-f]{64}", fingerprint):
        raise RuntimeError("the offline vendor tree was not approved by the candidate")
    return fingerprint


def session_plan(plan: list[dict[str, object]], mode: str, request_timeout_ms: int) -> dict[str, object]:
    return {
        "mode": mode,
        "expected_tools": list(EXPECTED_TOOLS),
        "m5_tools": list(M5_TOOLS),
        "projects": {name: str(ROOT / path) for name, path in FIXTURES.items()},
        "unknown_project_ref": UNKNOWN_PROJECT_REF,
        "missing_resource_uri": MISSING_RESOURCE_URI,
        "request_timeout_ms": request_timeout_ms,
        "calls": plan,
    }


def inspector_gate(attempt: pathlib.Path, mode: str, argv: list[str],
                   plan: list[dict[str, object]], timeout: int,
                   request_timeout_ms: int) -> dict[str, object]:
    m3 = load_m3()
    observation = attempt / "protocol.jsonl"
    state = pathlib.Path(argv[argv.index("--state-root") + 1])
    state.mkdir(mode=0o700, parents=True, exist_ok=True)
    # Keep Node package resolution beside the installed Inspector dependencies.
    bridge = ROOT / "target/m1-17-inspector" / f"m5-{attempt.name}-{mode}-bridge.mjs"
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
             json.dumps(session_plan(plan, mode, request_timeout_ms), separators=(",", ":"))],
            attempt, timeout, attempt / f"inspector-{mode}-session.json",
        )
    finally:
        bridge.unlink(missing_ok=True)
    if result["exit_code"] != 0:
        raise RuntimeError(f"Inspector M5 {mode} session failed")
    outcome = json.loads((attempt / f"inspector-{mode}-session.stdout").read_text())
    if outcome.get("tool_count") != len(EXPECTED_TOOLS) or outcome.get("discovery") is not True:
        raise RuntimeError(f"Inspector M5 {mode} discovery oracle incomplete")
    if outcome.get("mode") != mode:
        raise RuntimeError("Inspector reported a different mode than it was given")
    if outcome.get("resource") is not True:
        raise RuntimeError(f"Inspector M5 {mode} resource oracle incomplete")
    if outcome.get("artifact_resources_read", 0) == 0 and outcome.get("missing_resource_refused") is not True:
        raise RuntimeError("Inspector read no Resource and observed no refusal")
    rows = validate_call_rows(outcome.get("calls"), "inspector", plan)
    return {
        "mode": mode,
        "version": INSPECTOR_VERSION,
        "bundle_sha256": m3.file_digest(INSPECTOR),
        "bridge_suffix_sha256": m3.digest(suffix),
        "session": result,
        "protocol_era": outcome.get("protocol_era"),
        "tasks_declared": outcome.get("tasks_declared"),
        "tasks_advertised": outcome.get("tasks_advertised"),
        "artifact_resources_read": outcome.get("artifact_resources_read"),
        "missing_resource_refused": outcome.get("missing_resource_refused"),
        "runtime_facts": outcome.get("runtime_facts", {}),
        "calls": rows,
    }


def measure(m3, value: object) -> dict[str, object]:
    """Same canonical encoding the Inspector session driver uses."""
    payload = json.dumps(value, separators=(",", ":"), ensure_ascii=False,
                         sort_keys=True).encode()
    return {"bytes": len(payload), "sha256": m3.digest(payload)}


def check_runtime_observation(client: str, row: dict[str, object],
                              structured: dict[str, object]) -> dict[str, object]:
    """The same observation oracle the Inspector session applies, in Python."""
    label = f"{client} {row['tool']} {row['mode']}"
    data = structured.get("data")
    observation = data.get("observation") if isinstance(data, dict) else None
    if not isinstance(observation, dict):
        raise RuntimeError(f"{label} published no observation")
    for key, value in row["expect_observation"].items():
        if observation.get(key) != value:
            raise RuntimeError(f"{label} observation.{key} is not {value}")
    for key in row["expect_positive_fields"]:
        value = observation.get(key)
        if not isinstance(value, int) or isinstance(value, bool) or value <= 0:
            raise RuntimeError(f"{label} observation.{key} is not a positive count")
    for key in row["expect_zero_fields"]:
        if observation.get(key) != 0:
            raise RuntimeError(f"{label} observation.{key} is not zero")
    if row["expect_measured"]:
        measured = observation.get("measured")
        if (not isinstance(measured, dict)
                or not isinstance(measured.get("size_bytes"), int)
                or measured["size_bytes"] <= 0
                or not re.fullmatch(r"sha256:[0-9a-f]{64}", str(measured.get("sha256")))
                or measured.get("analysis_build_symbols_forced") is not True):
            raise RuntimeError(f"{label} published no measured binary")
    artifacts = data.get("artifacts")
    if not isinstance(artifacts, list) or len(artifacts) < row["expect_min_artifacts"]:
        raise RuntimeError(f"{label} published too few artifacts")
    for artifact in artifacts:
        if (not isinstance(artifact, dict)
                or not str(artifact.get("uri", "")).startswith("rust-quality-artifact://")
                or not re.fullmatch(r"[0-9a-f]{64}", str(artifact.get("sha256")))
                or not isinstance(artifact.get("size_bytes"), int)
                or artifact["size_bytes"] <= 0):
            raise RuntimeError(f"{label} published an invalid artifact descriptor")
    facts = {"artifacts_published": len(artifacts)}
    facts.update({key: observation[key] for key in row["report_fields"] if key in observation})
    return {"artifacts": artifacts, "facts": facts}


def codex_gate(attempt: pathlib.Path, mode: str, argv: list[str],
               plan: list[dict[str, object]], codex: pathlib.Path,
               model_turn: bool, call_timeout: int) -> dict[str, object]:
    """Stock app-server conversion gate; no Tasks calls and no auth copy."""
    m3 = load_m3()
    observation = attempt / "protocol.jsonl"
    state = pathlib.Path(argv[argv.index("--state-root") + 1])
    state.mkdir(mode=0o700, parents=True, exist_ok=True)
    spec = importlib.util.spec_from_file_location("m5_codex_controller", CONTROLLER)
    if spec is None or spec.loader is None:
        raise RuntimeError("Codex controller unavailable")
    controller = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(controller)
    controller.TOOLS = EXPECTED_TOOLS
    controller.DISABLED_HOST_SERVERS = ()
    base = controller.overrides

    def overrides(plan_values):
        values = base(plan_values)
        values["features.code_mode_host"] = True
        values["features.mcp_2026_07_28"] = True
        values["features.skip_host_skill_discovery"] = True
        return values

    controller.overrides = overrides
    proxy = [sys.executable, str(pathlib.Path(__file__).resolve()), "proxy",
             "--client", "codex-app-server", "--observation", str(observation),
             "--server-argv-json", json.dumps(argv, separators=(",", ":"))]
    invocation = {"codex": str(codex), "server_binary": proxy[0], "server_args": proxy[1:],
                  "model": "gpt-5.6-sol", "effort": "medium"}
    source_home = pathlib.Path(os.environ.get("CODEX_HOME", pathlib.Path.home() / ".codex"))
    auth = source_home / "auth.json"
    private = pathlib.Path(tempfile.mkdtemp(prefix="rust-mcp-m5-codex-", dir="/private/tmp"))
    os.chmod(private, 0o700)
    if not auth.is_file():
        shutil.rmtree(private, ignore_errors=True)
        raise RuntimeError("Codex auth unavailable")
    # A symlink, never a copy: no credential byte is duplicated for this gate.
    os.symlink(auth, private / "auth.json")
    previous = os.environ.get("CODEX_HOME")
    os.environ["CODEX_HOME"] = str(private)
    transport = None
    try:
        transport = controller.Transport(controller.command(invocation), attempt)
        controller.init(transport, attempt)
        started = controller.thread_start(transport, invocation, attempt)
        thread = started.get("thread", {}).get("id")
        if not isinstance(thread, str):
            raise RuntimeError("Codex thread missing")

        def call(name, arguments, timeout):
            value = transport.rpc("mcpServer/tool/call", {
                "threadId": thread, "server": "rust_engineering",
                "tool": name, "arguments": arguments}, timeout)
            if not isinstance(value, dict):
                raise RuntimeError(f"Codex conversion failed for {name}")
            return value

        refs = {}
        for name, path in FIXTURES.items():
            opened = call("rust.project.open", {"path": str(ROOT / path)}, 60)
            found = {value for value in m3.find_values(opened, "project_ref") if isinstance(value, str)}
            if len(found) != 1:
                raise RuntimeError(f"Codex ProjectRef ambiguous for {name}")
            refs[name] = found.pop()
        rows = []
        facts = {}
        refusal_uris = set()
        resources_read = 0
        for row in plan:
            reference = UNKNOWN_PROJECT_REF if row["project"] is None else refs[row["project"]]
            arguments = {"project_ref": reference, **row["arguments"]}
            result = call(row["tool"], arguments, call_timeout)
            structured = result.get("structuredContent")
            label = f"Codex {row['tool']} {row['mode']} {row['shape']}"
            if not isinstance(structured, dict):
                raise RuntimeError(f"{label} carried no structured result")
            if structured.get("status") != row["expect_status"]:
                raise RuntimeError(f"{label} status was not preserved")
            if structured.get("error_code") != row["expect_error_code"]:
                raise RuntimeError(f"{label} error code was not preserved")
            if bool(result.get("isError")) is not row["expect_is_error"]:
                raise RuntimeError(f"{label} isError was not preserved")
            published = 0
            read = 0
            if row["mode"] == RUNTIME:
                checked = check_runtime_observation("Codex", row, structured)
                published = len(checked["artifacts"])
                for artifact in checked["artifacts"]:
                    controller.validate_resource(transport.rpc("mcpServer/resource/read", {
                        "threadId": thread, "server": "rust_engineering",
                        "uri": artifact["uri"]}, 120))
                    read += 1
                resources_read += read
                facts[row["tool"]] = {**checked["facts"], "artifacts_read": read}
            else:
                refusal_uris.update(
                    value for value in m3.find_values(result, "uri")
                    if isinstance(value, str) and value.startswith("rust-quality-artifact://"))
            request = measure(m3, arguments)
            response = measure(m3, result)
            rows.append({
                "client": "codex-app-server", "tool": row["tool"], "shape": row["shape"],
                "mode": row["mode"], "status": structured["status"],
                "error_code": structured.get("error_code"),
                "is_error": bool(result.get("isError")),
                "artifacts_published": published, "artifacts_read": read,
                "request_bytes": request["bytes"], "request_sha256": request["sha256"],
                "response_bytes": response["bytes"], "response_sha256": response["sha256"],
            })
        for uri in sorted(refusal_uris):
            controller.validate_resource(transport.rpc("mcpServer/resource/read", {
                "threadId": thread, "server": "rust_engineering", "uri": uri}, 60))
            resources_read += 1
        missing_refused = False
        if resources_read == 0:
            try:
                resource = transport.rpc("mcpServer/resource/read", {
                    "threadId": thread, "server": "rust_engineering",
                    "uri": MISSING_RESOURCE_URI}, 60)
                missing_refused = isinstance(resource, dict) and (
                    resource.get("isError") is True or resource.get("error") is not None)
            except RuntimeError:
                missing_refused = True
            if not missing_refused:
                raise RuntimeError("Codex missing-Resource oracle was not observed")

        result_row: dict[str, object] = {
            "mode": mode, "version": CODEX_VERSION, "model": "gpt-5.6-sol", "effort": "medium",
            "tasks_declared": False, "auth_copy": False,
            "task_cancel": "not supported by stock client",
            "artifact_resources_read": resources_read,
            "missing_resource_refused": missing_refused,
            "runtime_facts": facts,
            "model_turn_completed": False,
            "calls": validate_call_rows(rows, "codex-app-server", plan),
        }
        if model_turn:
            prompt = (
                "Use only the configured Rust Engineering MCP tools. Open the three configured "
                "project roots, then call rust.benchmark.run, rust.benchmark.compare, "
                "rust.profile.flamegraph and rust.binary.bloat once each with synchronous "
                "execution. Every one of them will answer a declared refusal on this host; "
                "report each structured status and error_code exactly as returned and do not "
                "retry. Do not use any non-MCP capability."
            )
            turn = transport.rpc("turn/start", {
                "threadId": thread, "input": [{"type": "text", "text": prompt}]}, 30).get("turn", {})
            turn_id = turn.get("id")
            if not isinstance(turn_id, str):
                raise RuntimeError("Codex model turn did not start")
            completed = False
            observed = set()
            events = attempt / f"codex-{mode}-model-events.jsonl"
            deadline = time.monotonic() + 900
            while time.monotonic() < deadline and not completed:
                try:
                    event = transport.q.get(timeout=0.25)
                except queue.Empty:
                    if transport.failure:
                        raise RuntimeError(transport.failure)
                    continue
                m3.append_observation(events, event)
                item = event.get("params", {}).get("item", {})
                if item.get("type") == "mcpToolCall" and isinstance(item.get("tool"), str):
                    observed.add(item["tool"])
                if (event.get("method") == "turn/completed"
                        and event.get("params", {}).get("turn", {}).get("id") == turn_id):
                    completed = True
            if not completed or not set(M5_TOOLS).issubset(observed):
                raise RuntimeError("Codex model-directed M5 flow incomplete")
            result_row["model_turn_completed"] = True
            result_row["model_turn_tools"] = sorted(observed)
            result_row["model_events_sha256"] = m3.file_digest(events)
        return result_row
    finally:
        try:
            if transport is not None:
                cleanup = transport.close()
                if not cleanup.get("cleanup_verified", False):
                    raise RuntimeError("Codex cleanup unverified")
        finally:
            if previous is None:
                os.environ.pop("CODEX_HOME", None)
            else:
                os.environ["CODEX_HOME"] = previous
            shutil.rmtree(private, ignore_errors=True)


def next_attempt() -> pathlib.Path:
    """M5 owns a separate immutable attempt namespace."""
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
        raise RuntimeError("M5 client preconditions are unsatisfied: " + ", ".join(check["unsatisfied"]))
    codex_name = shutil.which("codex")
    if codex_name is None:
        raise RuntimeError("stock Codex disappeared between preflight and run")
    codex = pathlib.Path(codex_name)
    plan = check["call_plan"]
    runtime_plan = check["runtime_call_plan"] if with_runtime else []
    gate_spec = importlib.util.spec_from_file_location("m5_gate_inventory", ROOT / "scripts/gate.py")
    if gate_spec is None or gate_spec.loader is None:
        raise RuntimeError("gate inventory unavailable")
    gate = importlib.util.module_from_spec(gate_spec)
    gate_spec.loader.exec_module(gate)
    candidate_sources = gate.source_inventory(ROOT, os.environ.copy())
    attempt = next_attempt()
    private = pathlib.Path(tempfile.mkdtemp(prefix="rust-mcp-m5-clients-", dir="/private/tmp"))
    os.chmod(private, 0o700)
    # Named, never created and never dialed: the assertion below is what proves
    # the Docker-free mode started no container.
    closed_socket = private / "docker-never-dialed.sock"
    vendor = (ROOT / VENDOR_FIXTURE).resolve()
    receipt: dict[str, object] = {
        "schema": "rust-mcp-m5-clients-v1", "status": "failed",
        "attempt": attempt.name, "image_id": check["image_id"],
        "with_runtime": with_runtime,
        "expected_tools": list(EXPECTED_TOOLS), "m5_tools": list(M5_TOOLS),
        "inventory": check["inventory"], "advertisement": check["advertisement"],
        "call_plan": plan, "runtime_call_plan": runtime_plan,
        "fixture_roots": dict(FIXTURES),
        "modes": {
            DOCKER_FREE: {
                "docker_used": False, "profiling_grant": None, "cargo_vendor": None,
                "description": "every call is a declared refusal produced before any container",
            },
            RUNTIME: {
                "enabled": with_runtime,
                "docker_used": with_runtime,
                "profiling_grant": PROFILING_GRANT if with_runtime else None,
                "cargo_vendor": VENDOR_FIXTURE if with_runtime else None,
                "description": "real measurements in the qualified image, artifacts read back "
                               "as rust-quality-artifact:// Resources",
            },
        },
        "runtime_tools": list(runtime_tools()) if with_runtime else [],
        "tools_without_a_client_positive": check["tools_without_a_client_positive"],
        "source_sha256": check["source_sha256"],
        "candidate": {"server_sha256": m3.file_digest(SERVER), "sources": candidate_sources},
    }
    try:
        if closed_socket.exists():
            raise RuntimeError("the closed docker socket path must not exist before the gate")
        free_argv = server_argv(attempt / f"state-{DOCKER_FREE}", closed_socket)
        receipt["inspector"] = {
            DOCKER_FREE: inspector_gate(attempt, DOCKER_FREE, free_argv, plan, 600, 120_000),
        }
        receipt["codex_app_server"] = {
            DOCKER_FREE: codex_gate(attempt, DOCKER_FREE, free_argv, plan, codex, True, 120),
        }
        if closed_socket.exists():
            raise RuntimeError("a docker socket was created during the Docker-free mode")
        receipt["docker_free_socket_created"] = False
        calls = (receipt["inspector"][DOCKER_FREE].pop("calls")
                 + receipt["codex_app_server"][DOCKER_FREE].pop("calls"))
        if with_runtime:
            fingerprint = vendor_fingerprint(vendor)
            receipt["modes"][RUNTIME]["cargo_vendor_tree_sha256"] = fingerprint
            runtime_argv = runtime_server_argv(
                attempt / f"state-{RUNTIME}", pathlib.Path(docker_socket), vendor, fingerprint)
            receipt["inspector"][RUNTIME] = inspector_gate(
                attempt, RUNTIME, runtime_argv, runtime_plan, 1800, 600_000)
            receipt["codex_app_server"][RUNTIME] = codex_gate(
                attempt, RUNTIME, runtime_argv, runtime_plan, codex, False, 600)
            calls += (receipt["inspector"][RUNTIME].pop("calls")
                      + receipt["codex_app_server"][RUNTIME].pop("calls"))
        receipt["calls"] = calls
        receipt["protocol"] = validate_protocol_metadata(attempt / "protocol.jsonl")
        receipt["clients"] = {
            "inspector": {"version": INSPECTOR_VERSION,
                          "bundle_sha256": receipt["inspector"][DOCKER_FREE]["bundle_sha256"]},
            "codex_app_server": {"version": CODEX_VERSION},
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
        # A credential-shaped file under an attempt demotes it, whether the
        # gate passed or failed, so no current receipt is published from it.
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
                raise RuntimeError("current M5 client receipt already exists")
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
                raise RuntimeError("M5 preflight receipt already exists")
            load_m3().save_json(PREFLIGHT, receipt, exclusive=True)
        print(json.dumps(receipt, sort_keys=True))
        if receipt["unsatisfied"]:
            # Refuse loudly: an unsatisfied precondition is a failure of the
            # gate's readiness, never something a caller should read past.
            print("M5 client qualification is not runnable; unsatisfied preconditions: "
                  + ", ".join(receipt["unsatisfied"]), file=sys.stderr)
            return 1
        return 0
    return run(options.with_runtime, options.docker_socket)


if __name__ == "__main__":
    raise SystemExit(main())
