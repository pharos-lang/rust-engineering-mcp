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
    The Docker-free matrix plus two benchmark measurements, their comparison,
    a sampled CPU profile and a binary size analysis through the qualified M5
    runtime. Every published artifact is read back as a
    ``rust-quality-artifact://`` Resource.
    This mode needs the real Docker socket, the qualified image, an
    authenticated host cargo vendor tree and the host profiling grant, and it
    starts containers.  Every row in the receipt says which mode produced it.

Two stock clients take part.  The MCP Inspector converts every planned row
deterministically and reads every published Resource.  Claude Code, restricted
to the configured server and to the MCP Resource tools, runs the two
model-directed flows: the four declared refusals in the Docker-free mode, and
in the runtime mode two measurements of its own, their comparison, a declared
``NOT_A_DATASET`` refusal against one of its own non-dataset artifacts and a
native read of that artifact as a Resource.  Codex is not used by this gate.
"""
from __future__ import annotations

import argparse
import base64
import binascii
import importlib.util
import json
import os
import pathlib
import re
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import time

ROOT = pathlib.Path(__file__).resolve().parents[1]
M3_PATH = ROOT / "scripts/test-m3-clients.py"
SESSION = ROOT / "scripts/m5-inspector-session.mjs"
UNIT = ROOT / "scripts/test-m5-clients-unit.py"
ATTEMPTS = ROOT / "docs/validation/m5-clients"
CURRENT = ROOT / "docs/validation/M5-clients.json"
PREFLIGHT = ROOT / "docs/validation/M5-clients-preflight.json"
SERVER = ROOT / "target/release/rust-engineering-mcp"
NODE = pathlib.Path("/Users/cburgosro/.nvm/versions/node/v24.15.0/bin/node")
CLAUDE = pathlib.Path("/Users/cburgosro/.local/bin/claude")
INSPECTOR = ROOT / "target/m1-17-inspector/node_modules/@modelcontextprotocol/inspector/clients/cli/build/index.js"
INSPECTOR_PACKAGE = ROOT / "target/m1-17-inspector/node_modules/@modelcontextprotocol/inspector/package.json"
DOCKER = pathlib.Path("/Applications/Docker.app/Contents/Resources/bin/docker")
STDIO = ROOT / "crates/mcp-server/src/stdio.rs"
STDIO_DIR = ROOT / "crates/mcp-server/src/stdio"
PROTOCOL_TEST = ROOT / "crates/mcp-server/tests/protocol.rs"
PERFORMANCE_PORT = ROOT / "crates/execution-adapter/src/performance_port.rs"
HOST_CONFIG = ROOT / "crates/mcp-server/src/host_config.rs"
BENCHMARK_COMPARE_DOMAIN = ROOT / "crates/domain/src/benchmark_compare.rs"

INSPECTOR_VERSION = "2.5.0"
# The stock agentic client.  Version and model are pinned: the session's own
# `init` event must report both, and a fallback to another model is a failure.
CLAUDE_VERSION = "2.1.267 (Claude Code)"
CLAUDE_MODEL = "claude-sonnet-5"
CLAUDE_EFFORT = "medium"
CLAUDE_CLIENT = "claude-code"
CLAUDE_SERVER = "rust_engineering"
# The only built-ins the session keeps: native MCP Resource discovery and read.
CLAUDE_RESOURCE_TOOLS = ("ListMcpResourcesTool", "ReadMcpResourceTool")
# Claude Code's wall-clock bound per MCP tool call, in milliseconds.  A
# synchronous measurement may legitimately take its whole 300 s budget.
MCP_TOOL_TIMEOUT_MS = 900_000
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
    # ADR-080 §6: the harness-log recovery needs a run that really fails to
    # compile.  This root resolves the approved criterion and its bench does
    # not typecheck, so the tool observes the harness AND a compilation
    # failure, and the compiler's own text is the only evidence there is.
    "benchmark_compile_error": "fixtures/benchmark-compile-error",
    "profile": "fixtures/profile-workload",
    "bloat": "fixtures/bloat",
}
# The runtime mode needs a host-authenticated offline vendor tree.  Both
# measured fixtures resolve entirely from path dependencies (their lockfiles
# name no registry package), so this tree only has to be an *approved* one; an
# empty directory is refused by `cargo-vendor inspect`, which is why the
# checked M4 tree is reused instead of inventing one.
VENDOR_FIXTURE = "fixtures/cargo-vendor-data/vendor"
VENDOR_CAPTURE_STORE = "fixtures/criterion-vendor/capture"
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

# The runtime calls that produce real evidence through the qualified runtime.
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
        "tool": "rust.benchmark.run", "shape": "positive", "project": "benchmark",
        "arguments": {"package": "rust-mcp-benchmark-fixture", "bench_target": "perf",
                      "run_count": 1, "timeout_seconds": 300,
                      "execution_mode": "synchronous"},
        "expect_status": "passed", "expect_error_code": None,
        "expect_observation": {"harness": {"harness": "criterion", "version": "0.8.2"},
                               "exit": "passed", "dataset_published": True,
                               "dataset_omission": None, "runs_requested": 1,
                               "runs_completed": 1, "exit_run_index": 1,
                               "complete": True},
        "expect_positive_fields": [], "expect_zero_fields": [],
        "expect_measured": False, "expect_min_artifacts": 2,
        "expect_artifact_kinds": ["benchmark_dataset", "criterion_archive",
                                  "harness_stdout", "harness_stderr"],
        "expect_no_artifact_kinds": [],
        "report_fields": ["harness", "exit", "exit_code", "exit_run_index",
                          "runs_requested", "runs_completed", "dataset_published",
                          "dataset_omission", "complete"],
        "requires_profiling_grant": False,
        "dataset_role": "baseline", "fact_key": "benchmark_baseline",
        "rationale": "a real criterion measurement through the provisioned ADR-078 capture; "
                     "its store-issued dataset identifier is retained for the comparison and "
                     "every published artifact is read back as a Resource",
    },
    {
        "tool": "rust.benchmark.run", "shape": "positive", "project": "benchmark",
        "arguments": {"package": "rust-mcp-benchmark-fixture", "bench_target": "perf",
                      "run_count": 1, "timeout_seconds": 300,
                      "execution_mode": "synchronous"},
        "expect_status": "passed", "expect_error_code": None,
        "expect_observation": {"harness": {"harness": "criterion", "version": "0.8.2"},
                               "exit": "passed", "dataset_published": True,
                               "dataset_omission": None, "runs_requested": 1,
                               "runs_completed": 1, "exit_run_index": 1,
                               "complete": True},
        "expect_positive_fields": [], "expect_zero_fields": [],
        "expect_measured": False, "expect_min_artifacts": 2,
        "expect_artifact_kinds": ["benchmark_dataset", "criterion_archive",
                                  "harness_stdout", "harness_stderr"],
        "expect_no_artifact_kinds": [],
        "report_fields": ["harness", "exit", "exit_code", "exit_run_index",
                          "runs_requested", "runs_completed", "dataset_published",
                          "dataset_omission", "complete"],
        "requires_profiling_grant": False,
        "dataset_role": "candidate", "fact_key": "benchmark_candidate",
        "rationale": "a second independent criterion measurement through the same verified "
                     "capture; its actual dataset identifier becomes the candidate and every "
                     "published artifact is read back as a Resource",
    },
    {
        "tool": "rust.benchmark.compare", "shape": "positive", "project": "benchmark",
        "arguments": {"timeout_seconds": 30},
        "compare_dataset_roles": ["baseline", "candidate"],
        "expect_status": "passed", "expect_error_code": None,
        "expect_observation": {}, "expect_positive_fields": [], "expect_zero_fields": [],
        "expect_measured": False, "expect_min_artifacts": 0,
        "expect_artifact_kinds": [], "expect_no_artifact_kinds": [],
        "expect_report": {"complete": True, "incompatibility_reasons": []},
        "expect_all_verdicts": "inconclusive",
        "expect_inconclusive_reasons": ["insufficient_executions"],
        "report_fields": ["compared", "complete", "incompatibility_reasons"],
        "requires_profiling_grant": False, "fact_key": "benchmark_compare",
        "rationale": "compare the two store-issued one-execution datasets without inventing "
                     "identifiers; the frozen method withholds every verdict because neither "
                     "side has enough independent executions",
    },
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
    # ---- ADR-080 §6: the harness-log recovery, from a real client ----------
    #
    # Both rows below are the cases the old contract left with nothing to
    # fetch.  Each publishes no dataset and no criterion tree, so every
    # artifact the row asserts IS a harness log, and each one is read back as a
    # `rust-quality-artifact://` Resource by the same oracle the two rows above
    # use.  `expect_status` is `failed` — a declared observed result with
    # `isError: false`, not a refusal — so neither row can pass by having been
    # blocked before the runtime.
    #
    # `exit` is deliberately NOT asserted.  `BenchmarkExit::CALIBRATED` is
    # false: no Docker receipt has yet pinned which code `cargo bench` returns
    # for a rustc failure, and `100` (`compilation_failed`) versus `101`
    # (`benchmark_failed`) is exactly what a receipt is supposed to settle.
    # Asserting one here would freeze a guess as an expectation.  What is
    # asserted is what ADR-080 changes: no dataset, the omission the adapter
    # observed, and published logs.
    {
        "tool": "rust.benchmark.run", "shape": "positive", "project": "benchmark_compile_error",
        "arguments": {"package": "rust-mcp-benchmark-compile-error-fixture",
                      "bench_target": "perf", "run_count": 1, "timeout_seconds": 300,
                      "execution_mode": "synchronous"},
        "expect_status": "failed", "expect_error_code": "OBSERVED_FAILURE",
        "expect_observation": {"dataset_published": False,
                               "dataset_omission": "execution_failed",
                               "runs_requested": 1, "exit_run_index": 1},
        "expect_positive_fields": [],
        "expect_zero_fields": [],
        "expect_measured": False,
        # The bench does not typecheck, so rustc writes to stderr and at least
        # that one log member exists.  Whether cargo also wrote to stdout is
        # not this row's business.
        "expect_min_artifacts": 1,
        "expect_artifact_kinds": ["harness_stdout", "harness_stderr"],
        "expect_no_artifact_kinds": ["benchmark_dataset", "criterion_archive"],
        "report_fields": ["exit", "exit_code", "exit_run_index", "runs_completed",
                          "dataset_published", "dataset_omission", "logs"],
        "requires_profiling_grant": False,
        "rationale": "an observed compilation failure: the compiler's own text is published as "
                     "a per-repetition harness log artifact and read back as a Resource, which "
                     "is the evidence ADR-076 promised and the server did not publish",
    },
    {
        "tool": "rust.benchmark.run", "shape": "positive", "project": "bloat",
        "arguments": {"package": "rust-mcp-bloat-fixture", "run_count": 1,
                      "timeout_seconds": 300, "execution_mode": "synchronous"},
        "expect_status": "failed", "expect_error_code": "HARNESS_UNRECOGNIZED",
        "expect_observation": {"harness": {"harness": "unrecognized"}, "dataset_published": False,
                               "dataset_omission": "harness_unrecognized",
                               "runs_requested": 1, "exit_run_index": 1},
        "expect_positive_fields": [],
        "expect_zero_fields": [],
        "expect_measured": False,
        "expect_min_artifacts": 1,
        "expect_artifact_kinds": ["harness_stdout", "harness_stderr"],
        "expect_no_artifact_kinds": ["benchmark_dataset", "criterion_archive"],
        "report_fields": ["exit", "exit_code", "exit_run_index", "runs_completed",
                          "harness", "dataset_published", "dataset_omission", "logs"],
        "requires_profiling_grant": False,
        "rationale": "an unrecognized harness: the native oracle already proves the adapter "
                     "captures logs here, and this row proves the client can fetch them — the "
                     "case that used to publish no artifact at all",
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
CALL_CLIENTS = frozenset({"inspector"})
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
    paths = (M3_PATH, pathlib.Path(__file__).resolve(), SESSION, UNIT)
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


def declared_artifact_kinds(tool: str) -> frozenset[str]:
    """The closed artifact-kind vocabulary one M5 tool can publish.

    Re-derived from the tool's own `ArtifactKind` enum, in the `snake_case`
    serde renders it, so a plan cannot name a kind the server has no way to
    emit — and adding a kind to the server without teaching the matrix about it
    stays visible here rather than passing silently.
    """
    source = (STDIO_DIR / f"{TOOL_MODULES[tool]}.rs").read_text()
    start = source.index("enum ArtifactKind {") + len("enum ArtifactKind {")
    block = source[start:source.index("\n}", start)]
    names = re.findall(r"^\s{4}([A-Z][A-Za-z0-9]*),$", block, re.M)
    kinds = frozenset(screaming(name).lower() for name in names)
    if not kinds or len(kinds) != len(names):
        raise RuntimeError(f"artifact kind vocabulary is invalid for {tool}")
    return kinds


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


BENCHMARK_RUN_SNAPSHOT = ROOT / "crates/mcp-server/tests/snapshots/benchmark-run-tool.json"


def harness_variants() -> dict[str, frozenset[str]]:
    """Tag -> property names of every `Harness` variant in the frozen contract.

    `observation.harness` is an internally tagged object (`{"harness":
    "criterion", "version": ...}`, `{"harness": "unrecognized"}`), never a bare
    string; attempt-5 was stopped by a plan row that expected the string.
    """
    # The snapshot is the frozen tool definition; its result contract is the
    # `outputSchema`, whose `$defs` carry the `Harness` variants.
    output = json.loads(BENCHMARK_RUN_SNAPSHOT.read_text())["outputSchema"]
    definitions = output.get("$defs") or output.get("definitions") or {}
    variants = {}
    for variant in definitions["Harness"]["oneOf"]:
        properties = variant["properties"]
        variants[properties["harness"]["const"]] = frozenset(properties)
    if not variants:
        raise RuntimeError("frozen benchmark contract declares no harness variant")
    return variants


def check_harness_expectation(row: dict[str, object]) -> None:
    expected = row.get("expect_observation", {}).get("harness")
    if expected is None:
        return
    variants = harness_variants()
    tag = expected.get("harness") if isinstance(expected, dict) else None
    if tag not in variants or set(expected) - variants[tag]:
        raise RuntimeError(f"{row['tool']} expects a harness shape the frozen contract "
                           f"does not declare: {expected!r}")


def check_expectation(row: dict[str, object]) -> None:
    check_harness_expectation(row)
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
    """The Docker-backed matrix: every row is a real execution with evidence.

    Not every row is a *measurement*.  ADR-080 §6 adds two rows whose whole
    point is that the execution produced no dataset — an observed compilation
    failure and an unrecognized harness — and whose evidence is the harness
    logs the server now publishes. The comparison row instead consumes the two
    real dataset identifiers from the preceding calls and asserts the complete
    report. Execution rows publish and read at least one artifact and assert
    observation facts rather than only a status.
    """
    rows = []
    for row in RUNTIME_CALL_PLAN:
        check_expectation(row)
        if row["shape"] != "positive":
            raise RuntimeError("the runtime plan carries positives only")
        comparison = row["tool"] == "rust.benchmark.compare"
        if comparison:
            if (row.get("compare_dataset_roles") != ["baseline", "candidate"]
                    or row["expect_min_artifacts"] != 0):
                raise RuntimeError("a comparison must consume both captured datasets")
            if (not row.get("expect_report") or not row.get("expect_all_verdicts")
                    or not row.get("expect_inconclusive_reasons")):
                raise RuntimeError("a comparison must assert report facts")
        else:
            if row["expect_min_artifacts"] < 1:
                raise RuntimeError("a real execution must publish evidence")
            if not (row["expect_observation"] or row["expect_positive_fields"]
                    or row["expect_measured"]):
                raise RuntimeError("a real execution must assert observation facts")
        kinds = tuple(row.get("expect_artifact_kinds", ()))
        forbidden = tuple(row.get("expect_no_artifact_kinds", ()))
        if set(kinds) & set(forbidden):
            raise RuntimeError("an artifact kind is both required and forbidden")
        if not comparison:
            declared = declared_artifact_kinds(row["tool"])
            if not (set(kinds) | set(forbidden)) <= declared:
                raise RuntimeError("the plan names an artifact kind the tool cannot publish")
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
            # Closed sets, both optional: every artifact the call publishes
            # must be one of `expect_artifact_kinds`, and none may be one of
            # `expect_no_artifact_kinds`.  The second is what makes a
            # log-recovery row discriminating: a run that quietly published a
            # dataset would satisfy the artifact count and fail here.
            "expect_artifact_kinds": list(kinds),
            "expect_no_artifact_kinds": list(forbidden),
            "report_fields": row["report_fields"],
            "requires_profiling_grant": row["requires_profiling_grant"],
            "rationale": row["rationale"],
            "dataset_role": row.get("dataset_role"),
            "compare_dataset_roles": row.get("compare_dataset_roles"),
            "expect_report": row.get("expect_report", {}),
            "expect_all_verdicts": row.get("expect_all_verdicts"),
            "expect_inconclusive_reasons": row.get("expect_inconclusive_reasons", []),
            "fact_key": row.get("fact_key", row["tool"]),
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
    return {
        "inspector": {"expected": INSPECTOR_VERSION, "observed": package_version,
                      "tasks": True, "resource": True},
        "claude_code": {"expected": CLAUDE_VERSION, "observed": claude_version(),
                        "model": CLAUDE_MODEL, "effort": CLAUDE_EFFORT,
                        "tasks": False, "resource": True},
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
        "vendor_capture": (
            find_vendor_capture(capture_store()) is not None,
            "exactly one regular, digest-named ADR-078 vendor capture must be provisioned",
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


def direction_guard_evidence() -> dict[str, object]:
    """Bind the global direction guard to its source and dedicated unit test.

    The one-execution client datasets stop at `insufficient_executions`, before
    this guard is consulted. This records the separate evidence without
    pretending the runtime comparison exercised it.
    """
    source = BENCHMARK_COMPARE_DOMAIN.read_text()
    declaration = "pub const METHOD_QUALIFIED_FOR_DIRECTION: bool = false;"
    test_name = "only_the_frozen_constant_qualifies_a_comparison"
    if declaration not in source or f"fn {test_name}()" not in source:
        raise RuntimeError("benchmark direction guard source evidence is missing")
    return {
        "qualified": False,
        "source": str(BENCHMARK_COMPARE_DOMAIN.relative_to(ROOT)),
        "unit_test": test_name,
        "exercised_by_runtime_comparison": False,
        "runtime_guard": "insufficient_executions",
    }


def preconditions(versions: dict[str, object], with_runtime: bool,
                  socket: str | None) -> dict[str, dict[str, object]]:
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
        "claude_binary": (CLAUDE.is_file(), "the pinned stock Claude Code executable must exist"),
        "claude_version": (
            versions["claude_code"]["observed"] == CLAUDE_VERSION,
            f"stock Claude Code must report {CLAUDE_VERSION}",
        ),
        "claude_auth": (claude_logged_in(),
                        "stock Claude Code must be logged in; no credential is copied"),
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
        "direction_guard": direction_guard_evidence(),
        "tools_without_a_client_positive": [
            tool for tool in M5_TOOLS if tool not in runtime_tools()
        ],
        "clients": versions,
        "preconditions": checks,
        "unsatisfied": unsatisfied,
        "resource_plan": {
            "docker_free": "no declared refusal publishes a quality artifact, so the Resource "
                           "oracle is the missing-artifact refusal on " + MISSING_RESOURCE_URI,
            "runtime": "every artifact a real execution publishes is read back as a "
                       "rust-quality-artifact:// Resource, including the per-repetition "
                       "harness logs of a run that published no dataset (ADR-080)",
        },
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
                        vendor: pathlib.Path, fingerprint: str,
                        capture: pathlib.Path, capture_fingerprint: str) -> list[str]:
    """The Docker-backed host configuration.

    The handlers demand a host-authenticated offline
    vendor tree (`profile.rs` and `bloat.rs` both answer
    `unavailable`/`MISSING_OFFLINE_DATA` without it), its approved fingerprint,
    a provisioned ADR-078 capture and its declared tree digest for benchmarks,
    and the revocable profiling grant `profile.rs` checks before dispatch.
    The socket here is the real one.
    """
    return base_argv(state, socket) + [
        "--cargo-vendor-dir", str(vendor),
        "--cargo-vendor-tree-sha256", fingerprint,
        "--vendor-capture", str(capture),
        "--vendor-capture-tree-sha256", capture_fingerprint,
        "--allow-profiling", PROFILING_GRANT,
    ]


def capture_store() -> pathlib.Path:
    configured = os.environ.get("RUST_MCP_TEST_VENDOR_CAPTURE_STORE")
    return pathlib.Path(configured).resolve() if configured else (ROOT / VENDOR_CAPTURE_STORE).resolve()


def find_vendor_capture(store: pathlib.Path) -> tuple[pathlib.Path, str] | None:
    """Find the single regular artifact whose name declares its tree digest."""
    if not store.is_dir():
        return None
    found = []
    for path in store.iterdir():
        try:
            regular = stat.S_ISREG(path.stat(follow_symlinks=False).st_mode)
        except OSError:
            continue
        if regular and re.fullmatch(r"[0-9a-f]{64}", path.name):
            found.append(path)
    if len(found) != 1:
        return None
    return found[0].resolve(), "sha256:" + found[0].name


def provisioned_vendor_capture() -> tuple[pathlib.Path, str]:
    found = find_vendor_capture(capture_store())
    if found is None:
        raise RuntimeError("exactly one digest-named ADR-078 vendor capture is required")
    return found


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


def artifact_id_from_uri(uri: object) -> str:
    match = re.fullmatch(
        r"rust-quality-artifact://prj_[0-9a-f]{32}/(qart_[0-9a-f]{32})"
        r"\?offset=0&length=[1-9][0-9]*",
        str(uri),
    )
    if match is None:
        raise RuntimeError("dataset artifact URI carries no store-issued identifier")
    return match.group(1)


def materialize_arguments(row: dict[str, object], reference: str,
                          datasets: dict[str, str]) -> dict[str, object]:
    arguments = {"project_ref": reference, **row["arguments"]}
    roles = row.get("compare_dataset_roles")
    if roles is None:
        return arguments
    if roles != ["baseline", "candidate"]:
        raise RuntimeError("comparison dataset roles are invalid")
    try:
        arguments["baseline_artifact_id"] = datasets["baseline"]
        arguments["candidate_artifact_id"] = datasets["candidate"]
    except KeyError as error:
        raise RuntimeError("comparison dataset was not captured from a preceding run") from error
    return arguments


def capture_dataset_id(row: dict[str, object], artifacts: list[dict[str, object]],
                       datasets: dict[str, str]) -> None:
    role = row.get("dataset_role")
    if role is None:
        return
    if role not in {"baseline", "candidate"} or role in datasets:
        raise RuntimeError("benchmark dataset role is invalid or duplicated")
    published = [artifact for artifact in artifacts
                 if artifact.get("kind") == "benchmark_dataset"]
    if len(published) != 1:
        raise RuntimeError("benchmark run did not publish exactly one pooled dataset")
    artifact_id = artifact_id_from_uri(published[0].get("uri"))
    if artifact_id in datasets.values():
        raise RuntimeError("independent benchmark runs published the same dataset identifier")
    datasets[str(role)] = artifact_id


# -- model-directed flows through the stock Claude Code client ----------------
#
# Claude Code relays an MCP tool result as the server's own JSON text and sets
# `is_error` from the server's `isError`; its two MCP Resource built-ins put
# their structured outcome in the `user` event's `tool_use_result`.  The
# normalizer below turns one stream-json transcript into a closed item shape,
# so the two flow oracles read Claude exactly as strictly as any other client.

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
    if name == "ListMcpResourcesTool":
        listed = result["structured"]
        return {
            "type": "mcpToolCall", "server": arguments.get("server"),
            "tool": "list_mcp_resources", "arguments": arguments,
            "status": "failed" if failed or not isinstance(listed, list) else "completed",
            "error": None,
            "result": {"structuredContent": None,
                       "resources": len(listed) if isinstance(listed, list) else None},
        }
    if name == "ReadMcpResourceTool":
        structured = result["structured"] if isinstance(result["structured"], dict) else {}
        contents = structured.get("contents")
        error = structured.get("error")
        content = ([{"type": "resource", "resource": entry}
                    for entry in contents if isinstance(entry, dict)]
                   if isinstance(contents, list) else [])
        return {
            "type": "mcpToolCall", "server": arguments.get("server"),
            "tool": "read_mcp_resource", "arguments": arguments,
            "status": "failed" if failed or error is not None or not content else "completed",
            "error": error if isinstance(error, str) else None,
            "result": {"structuredContent": None, "content": content},
        }
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
                                           "is_error": block.get("is_error") is True,
                                           "structured": event.get("tool_use_result")}
    if init is None or final is None:
        raise RuntimeError("Claude transcript lacks its init or result event")
    if set(results) != set(by_id):
        raise RuntimeError("Claude transcript has unmatched tool results")
    return init, [claude_item(call, results[call["id"]]) for call in calls], final


def validate_claude_session(init: dict[str, object], final: dict[str, object],
                            events: list[object] = ()) -> dict[str, object]:
    """Pinned client and model, only the configured server, a clean finish.

    `modelUsage` legitimately lists the small auxiliary model Claude Code uses
    beside the session model, so it cannot prove which model ran the turn. The
    `model` of every `assistant` message can: each one must be the pinned model.
    """
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
            or any(not (tool in CLAUDE_RESOURCE_TOOLS
                        or (isinstance(tool, str) and tool.startswith(prefix))) for tool in tools)):
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
                      planned: frozenset[str]) -> dict[str, str]:
    """Fixture name -> project_ref for every open; roots outside the plan are refused."""
    roots = {str(ROOT / FIXTURES[name]): name for name in planned}
    refs: dict[str, str] = {}
    for item in items:
        if item["tool"] != "rust.project.open":
            continue
        path = item["arguments"].get("path")
        payload = item["result"].get("structuredContent")
        if (item["arguments"] != {"path": path} or path not in roots
                or item["status"] != "completed" or not isinstance(payload, dict)
                or payload.get("status") != "passed"):
            raise RuntimeError("Claude opened a root outside the plan or the open did not pass")
        data = payload.get("data")
        reference = data.get("project_ref") if isinstance(data, dict) else None
        if not isinstance(reference, str) or not re.fullmatch(r"prj_[0-9a-f]{32}", reference):
            raise RuntimeError("Claude open returned no project reference")
        if roots[path] in refs:
            raise RuntimeError("Claude model-directed flow retried or omitted a required call")
        refs[roots[path]] = reference
    return refs


def resource_chunk_length(uri: str) -> int:
    match = re.search(r"[?&]length=([1-9][0-9]*)$", uri)
    if match is None:
        raise RuntimeError("Resource URI declares no chunk length")
    return int(match.group(1))


def resource_content_evidence(resource: dict[str, object],
                              descriptor: dict[str, object]) -> dict[str, object]:
    """What the client really received, measured; a client-side note is not content.

    Claude Code returns small text Resources inline, but writes a binary
    Resource to a file of its own and puts a human note in `text` beside
    `blobSavedTo`.  That note names the artifact without carrying it, so the
    evidence here is the decoded blob, the saved file, or inline text of a
    non-binary type — never the note — and its length must be the chunk's.
    """
    m3 = load_m3()
    expected_bytes = resource_chunk_length(str(resource.get("uri")))
    blob = resource.get("blob")
    saved = resource.get("blobSavedTo")
    text = resource.get("text")
    mime = resource.get("mimeType")
    if isinstance(blob, str) and blob:
        try:
            data = base64.b64decode(blob, validate=True)
        except (binascii.Error, ValueError) as error:
            raise RuntimeError("Claude model Resource blob is not base64") from error
        evidence = {"kind": "blob", "bytes": len(data), "sha256": m3.digest(data)}
    elif isinstance(saved, str) and saved:
        path = pathlib.Path(saved)
        if not path.is_file():
            raise RuntimeError("Claude reported a saved Resource blob that does not exist")
        data = path.read_bytes()
        evidence = {"kind": "blob_saved_by_client", "bytes": len(data), "sha256": m3.digest(data)}
    elif (isinstance(text, str) and text and isinstance(mime, str)
            and not mime.startswith("application/octet-stream")):
        data = text.encode()
        evidence = {"kind": "text", "mime_type": mime, "bytes": len(data), "sha256": m3.digest(data)}
    else:
        raise RuntimeError("Claude model Resource read carried no content")
    if evidence["bytes"] != expected_bytes:
        raise RuntimeError("Claude model Resource content length is not the chunk it read")
    # A chunk that covers the whole artifact must hash to the digest the
    # server published with the descriptor; a prefix can only be measured.
    evidence["whole_artifact"] = expected_bytes == descriptor.get("size_bytes")
    if evidence["whole_artifact"] and evidence["sha256"] != descriptor.get("sha256"):
        raise RuntimeError("Claude model Resource content does not hash to the published artifact")
    return evidence


def validate_model_resource_read(item: dict[str, object],
                                 descriptor: dict[str, object]) -> dict[str, object]:
    """Require one completed native read of the session-issued artifact, with content."""
    expected_uri = str(descriptor.get("uri"))
    if (item.get("server") != CLAUDE_SERVER or item.get("tool") != "read_mcp_resource"
            or item.get("arguments") != {"server": CLAUDE_SERVER, "uri": expected_uri}
            or item.get("status") != "completed" or item.get("error") is not None):
        raise RuntimeError("Claude model Resource read did not match the issued artifact")
    result = item.get("result")
    content = result.get("content") if isinstance(result, dict) else None
    if not isinstance(content, list) or len(content) != 1:
        raise RuntimeError("Claude model Resource read carried no content")
    block = content[0]
    resource = block.get("resource") if isinstance(block, dict) else None
    if not isinstance(resource, dict) or resource.get("uri") != expected_uri:
        raise RuntimeError("Claude model Resource read returned another artifact")
    return resource_content_evidence(resource, descriptor)


def validate_docker_free_model_flow(items: list[dict[str, object]],
                                    plan: list[dict[str, object]]) -> dict[str, object]:
    """The four declared refusals, each exactly once with the planned arguments."""
    completed = [item for item in items if item.get("type") == "mcpToolCall"]
    allowed = {"rust.project.open", *M5_TOOLS}
    if any(item["tool"] not in allowed for item in completed):
        raise RuntimeError("Claude model-directed refusal flow used another MCP capability")
    positive = {row["tool"]: row for row in plan
                if row["shape"] == "positive" and row["mode"] == DOCKER_FREE}
    if set(positive) != set(M5_TOOLS):
        raise RuntimeError("Docker-free plan does not carry one positive row per M5 tool")
    refs = opened_references(completed, frozenset(row["project"] for row in positive.values()))
    opens = {}
    for index, item in enumerate(completed):
        if item["tool"] == "rust.project.open":
            opens[item["arguments"]["path"]] = index
    refusals = {}
    for tool in M5_TOOLS:
        calls = [item for item in completed if item["tool"] == tool]
        if len(calls) != 1:
            raise RuntimeError("Claude model-directed refusal flow retried or omitted a required call")
        call = calls[0]
        row = positive[tool]
        reference = refs.get(row["project"])
        if reference is None or completed.index(call) < opens[str(ROOT / FIXTURES[row["project"]])]:
            raise RuntimeError(f"Claude called {tool} before opening its planned root")
        if call["arguments"] != {"project_ref": reference, **row["arguments"]}:
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
    # Every open precedes every refusal.  The four refusals are independent
    # declared results, each bound above to its own plan row and root, so their
    # relative order is not a product fact and a client that batches them is
    # not refused for it.
    first_refusal = min(completed.index(item) for item in completed if item["tool"] in M5_TOOLS)
    if max(opens.values()) > first_refusal:
        raise RuntimeError("Claude model-directed refusal flow ran out of order")
    return {"opened_roots": sorted(refs), "refusals": refusals}


def validate_runtime_model_flow(items: list[dict[str, object]],
                                plan: list[dict[str, object]]) -> dict[str, object]:
    """Open, discover, measure twice, compare, be refused, read: exactly once each."""
    completed = [item for item in items if item.get("type") == "mcpToolCall"]
    allowed = {"rust.project.open", "list_mcp_resources", "rust.benchmark.run",
               "rust.benchmark.compare", "read_mcp_resource"}
    if any(item["tool"] not in allowed for item in completed):
        raise RuntimeError("Claude model-directed runtime flow used another MCP capability")
    refs = opened_references(completed, frozenset({"benchmark"}))
    if set(refs) != {"benchmark"}:
        raise RuntimeError("Claude model-directed runtime flow did not open exactly the benchmark root")
    reference = refs["benchmark"]
    rows = {row["fact_key"]: row for row in plan if row["mode"] == RUNTIME}
    discoveries = [item for item in completed if item["tool"] == "list_mcp_resources"]
    runs = [item for item in completed if item["tool"] == "rust.benchmark.run"]
    comparisons = [item for item in completed if item["tool"] == "rust.benchmark.compare"]
    reads = [item for item in completed if item["tool"] == "read_mcp_resource"]
    if len(discoveries) != 1 or len(runs) != 2 or len(comparisons) != 2 or len(reads) != 1:
        raise RuntimeError("Claude model-directed runtime flow retried or omitted a required call")
    discovery = discoveries[0]
    if (discovery.get("server") != CLAUDE_SERVER
            or discovery.get("arguments") != {"server": CLAUDE_SERVER}
            or discovery.get("status") != "completed" or discovery.get("error") is not None):
        raise RuntimeError("Claude model Resource discovery was not completed")

    datasets: dict[str, str] = {}
    other_artifacts: dict[str, dict[str, object]] = {}
    for role, run in zip(("baseline", "candidate"), runs):
        row = rows[f"benchmark_{role}"]
        if run["arguments"] != {"project_ref": reference, **row["arguments"]}:
            raise RuntimeError(f"Claude {role} measurement did not use the planned arguments")
        payload = run["result"].get("structuredContent")
        if (run["status"] != "completed" or not isinstance(payload, dict)
                or payload.get("status") != row["expect_status"]
                or payload.get("error_code") != row["expect_error_code"]):
            raise RuntimeError(f"Claude {role} measurement did not pass")
        checked = check_runtime_observation("Claude", row, payload)
        capture_dataset_id(row, checked["artifacts"], datasets)
        for artifact in checked["artifacts"]:
            if artifact.get("kind") != "benchmark_dataset":
                other_artifacts[artifact_id_from_uri(artifact["uri"])] = artifact
    if not other_artifacts:
        raise RuntimeError("Claude measurements published no artifact of another kind")

    compare_row = rows["benchmark_compare"]
    positive_arguments = {"project_ref": reference, **compare_row["arguments"],
                          "baseline_artifact_id": datasets["baseline"],
                          "candidate_artifact_id": datasets["candidate"]}
    positive, negative = comparisons
    positive_payload = positive["result"].get("structuredContent")
    positive_data = positive_payload.get("data") if isinstance(positive_payload, dict) else None
    if (positive["arguments"] != positive_arguments or positive["status"] != "completed"
            or not isinstance(positive_payload, dict) or positive_payload.get("status") != "passed"
            or not isinstance(positive_data, dict)
            or positive_data.get("baseline_artifact_id") != datasets["baseline"]
            or positive_data.get("candidate_artifact_id") != datasets["candidate"]):
        raise RuntimeError("Claude model positive comparison was not observed")
    comparison = check_runtime_comparison("Claude", compare_row, positive_payload)["facts"]
    negative_payload = negative["result"].get("structuredContent")
    non_dataset = negative["arguments"].get("candidate_artifact_id")
    if (non_dataset not in other_artifacts
            or negative["arguments"] != {**positive_arguments, "candidate_artifact_id": non_dataset}
            or negative["status"] != "failed" or not isinstance(negative_payload, dict)
            or negative_payload.get("status") != "blocked"
            or negative_payload.get("error_code") != "NOT_A_DATASET"):
        raise RuntimeError("Claude model declared comparison failure was not observed")
    resource_uri = str(other_artifacts[non_dataset]["uri"])
    resource_content = validate_model_resource_read(reads[0], other_artifacts[non_dataset])

    positions = [completed.index(item) for item in (
        next(item for item in completed if item["tool"] == "rust.project.open"),
        discovery, runs[0], runs[1], positive, negative, reads[0])]
    if positions != sorted(positions):
        raise RuntimeError("Claude model-directed runtime flow ran out of order")
    return {
        "discovery": "list_mcp_resources",
        "measurements": 2,
        "positive": "rust.benchmark.compare",
        "comparison": comparison,
        "failure": "NOT_A_DATASET",
        "resource_uri_sha256": load_m3().digest(resource_uri.encode()),
        "resource_content": resource_content,
    }


def check_runtime_comparison(client: str, row: dict[str, object],
                             structured: dict[str, object]) -> dict[str, object]:
    label = f"{client} {row['tool']} {row['mode']}"
    data = structured.get("data")
    report = data.get("report") if isinstance(data, dict) else None
    if not isinstance(report, dict):
        raise RuntimeError(f"{label} published no comparison report")
    for key, value in row["expect_report"].items():
        if report.get(key) != value:
            raise RuntimeError(f"{label} report.{key} is not {value}")
    comparisons = report.get("comparisons")
    if not isinstance(comparisons, list) or not comparisons:
        raise RuntimeError(f"{label} published no benchmark comparisons")
    expected_verdict = row["expect_all_verdicts"]
    expected_reasons = row["expect_inconclusive_reasons"]
    for comparison in comparisons:
        if not isinstance(comparison, dict) or comparison.get("verdict") != expected_verdict:
            raise RuntimeError(f"{label} claimed a directional benchmark verdict")
        reasons = comparison.get("inconclusive_reasons")
        if reasons != expected_reasons:
            raise RuntimeError(f"{label} inconclusive reasons do not match the dataset guard")
    facts = {key: report[key] for key in row["report_fields"] if key in report}
    facts["verdicts"] = sorted({comparison["verdict"] for comparison in comparisons})
    facts["inconclusive_reasons"] = expected_reasons
    facts["artifacts_published"] = 0
    return {"artifacts": [], "facts": facts}


def check_runtime_result(client: str, row: dict[str, object],
                         structured: dict[str, object]) -> dict[str, object]:
    if row["tool"] == "rust.benchmark.compare":
        return check_runtime_comparison(client, row, structured)
    return check_runtime_observation(client, row, structured)


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
    allowed = set(row.get("expect_artifact_kinds", ()))
    forbidden = set(row.get("expect_no_artifact_kinds", ()))
    for artifact in artifacts:
        if (not isinstance(artifact, dict)
                or not str(artifact.get("uri", "")).startswith("rust-quality-artifact://")
                or not re.fullmatch(r"[0-9a-f]{64}", str(artifact.get("sha256")))
                or not isinstance(artifact.get("size_bytes"), int)
                or artifact["size_bytes"] <= 0):
            raise RuntimeError(f"{label} published an invalid artifact descriptor")
        kind = artifact.get("kind")
        if kind in forbidden:
            raise RuntimeError(f"{label} published a forbidden artifact kind {kind}")
        if allowed and kind not in allowed:
            raise RuntimeError(f"{label} published an unplanned artifact kind {kind}")
        # ADR-080 §2: everything but the pooled dataset names its repetition,
        # and the dataset is the only member allowed to leave it absent.
        index = artifact.get("run_index")
        if kind == "benchmark_dataset":
            if index is not None:
                raise RuntimeError(f"{label} gave the pooled dataset a run_index")
        elif row["tool"] == "rust.benchmark.run" and not (
                isinstance(index, int) and not isinstance(index, bool) and index >= 1):
            raise RuntimeError(f"{label} published {kind} without its repetition")
    facts = {"artifacts_published": len(artifacts)}
    facts.update({key: observation[key] for key in row["report_fields"] if key in observation})
    return {"artifacts": artifacts, "facts": facts}


CLAUDE_SYSTEM_PROMPT = (
    "You are a bounded third-party MCP client qualifier. Use only the explicitly "
    "configured Rust Engineering MCP server and the MCP Resource tools. Perform the "
    "requested steps exactly once each, in the given order, never retry a call even "
    "when it is refused, and finish with a short account of every returned status."
)


def project_root(name: str) -> str:
    return str(ROOT / FIXTURES[name])


def render_arguments(arguments: dict[str, object]) -> str:
    return ", ".join(f"{key} {json.dumps(value)}" for key, value in arguments.items())


def claude_prompt(mode: str, plan: list[dict[str, object]]) -> str:
    """Prompts are rendered from the plan rows, never written by hand."""
    if mode == DOCKER_FREE:
        positive = {row["tool"]: row for row in plan if row["shape"] == "positive"}
        roots = dict.fromkeys(positive[tool]["project"] for tool in M5_TOOLS)
        lines = [
            "Use only the configured Rust Engineering MCP server. Perform these steps exactly "
            "once each, in order, and do not retry any call even if it is refused: every "
            "refusal here is an expected, declared result.",
            "(1) Call rust.project.open once for each of these roots and keep every returned "
            "data.project_ref: " + "; ".join(f"{name} root {project_root(name)}" for name in roots) + ".",
        ]
        for index, tool in enumerate(M5_TOOLS, start=2):
            row = positive[tool]
            lines.append(f"({index}) Call {tool} with project_ref = the reference returned for "
                         f"the {row['project']} root, {render_arguments(row['arguments'])}.")
        lines.append(f"({len(M5_TOOLS) + 2}) Report the status and error_code of every call "
                     "exactly as returned. Do not call any other capability.")
        return " ".join(lines)
    rows = {row["fact_key"]: row for row in plan}
    baseline = rows["benchmark_baseline"]
    compare = rows["benchmark_compare"]
    return " ".join([
        "Use only the configured Rust Engineering MCP server and the MCP Resource tools. "
        "Perform these steps exactly once each, in order, without retrying any call.",
        f"(1) Call rust.project.open with path {project_root('benchmark')}; keep data.project_ref "
        "and use it as project_ref in every later call.",
        f"(2) List the MCP resources of server {CLAUDE_SERVER} with ListMcpResourcesTool.",
        f"(3) Call rust.benchmark.run with {render_arguments(baseline['arguments'])}; this real "
        "measurement may take several minutes, wait for it. From its data.artifacts keep the "
        "artifact of kind benchmark_dataset as BASELINE (its identifier is the qart_ segment of "
        "its uri, without the query string), and keep the artifact of kind criterion_archive: "
        "its full uri as ARCHIVE_URI and its qart_ identifier as ARCHIVE_ID.",
        "(4) Call rust.benchmark.run again with exactly the same arguments and keep its "
        "benchmark_dataset identifier as CANDIDATE.",
        f"(5) Call rust.benchmark.compare with {render_arguments(compare['arguments'])}, "
        "baseline_artifact_id BASELINE and candidate_artifact_id CANDIDATE; this is the "
        "positive comparison.",
        f"(6) Call rust.benchmark.compare again with {render_arguments(compare['arguments'])}, "
        "baseline_artifact_id BASELINE and candidate_artifact_id ARCHIVE_ID; that artifact has "
        "another kind and must return the declared NOT_A_DATASET refusal.",
        f"(7) Read ARCHIVE_URI from server {CLAUDE_SERVER} with ReadMcpResourceTool.",
        "(8) Report the status, error_code and every comparison verdict exactly as returned. "
        "Do not call any other capability.",
    ])


# Header and token shapes, not vocabulary: a model may legitimately write the
# word "authorization" in prose (attempt-3 did), a credential never appears
# without its header colon or its token prefix.
CREDENTIAL_TEXT = (b"authorization:", b"access_token", b"refresh_token", b"auth.json",
                   b"sk-ant-", b"bearer ")


def assert_no_credential_text(path: pathlib.Path) -> None:
    """Raw client output is staged as evidence only if nothing in it is credential-shaped."""
    encoded = path.read_bytes().lower()
    found = sorted(needle.decode() for needle in CREDENTIAL_TEXT if needle in encoded)
    if found:
        raise RuntimeError(f"credential-shaped text in {path.name}: " + ", ".join(found))


def client_home_residue(cwd: pathlib.Path) -> dict[str, object]:
    """What Claude Code left under its own home for this private working directory.

    `--no-session-persistence` keeps the conversation out of the resumable
    session list, but the client still writes tool results it saved to disk
    (a binary Resource, for instance) under `~/.claude/projects/<cwd slug>/`.
    The receipt states what was found; the harness then removes that directory
    because it exists only for this run's throwaway cwd.
    """
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
    # Claude's installed login is read in place by the CLI; restricted mode,
    # empty setting sources, strict MCP config and no other built-ins keep that
    # account home out of the model's workspace.  Nothing is copied.
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
                plan: list[dict[str, object]], timeout: int) -> dict[str, object]:
    """One model-directed turn of the stock Claude Code client; no credential copy."""
    m3 = load_m3()
    observation = attempt / "protocol.jsonl"
    state = pathlib.Path(argv[argv.index("--state-root") + 1])
    state.mkdir(mode=0o700, parents=True, exist_ok=True)
    proxy = [sys.executable, str(pathlib.Path(__file__).resolve()), "proxy",
             "--client", CLAUDE_CLIENT, "--observation", str(observation),
             "--server-argv-json", json.dumps(argv, separators=(",", ":"))]
    private = pathlib.Path(tempfile.mkdtemp(prefix="rust-mcp-m5-claude-", dir="/private/tmp"))
    os.chmod(private, 0o700)
    events_path = attempt / f"claude-{mode}-model-events.jsonl"
    stderr_path = attempt / f"claude-{mode}-model.stderr"
    try:
        for child in ("cwd", "tmp"):
            (private / child).mkdir(mode=0o700)
        config = private / "mcp.json"
        m3.save_json(config, {"mcpServers": {CLAUDE_SERVER: {
            "command": proxy[0], "args": proxy[1:], "env": {}}}}, exclusive=True)
        prompt = claude_prompt(mode, plan)
        claude_argv = [
            str(CLAUDE), "--print", "--output-format", "stream-json", "--verbose",
            "--model", CLAUDE_MODEL, "--effort", CLAUDE_EFFORT, "--no-session-persistence",
            "--restricted", "--setting-sources", "", "--strict-mcp-config",
            "--mcp-config", str(config), "--disable-slash-commands", "--no-chrome",
            "--tools", ",".join(CLAUDE_RESOURCE_TOOLS),
            "--allowedTools", ",".join((f"mcp__{CLAUDE_SERVER}", *CLAUDE_RESOURCE_TOOLS)),
            "--permission-mode", "dontAsk", "--permission-prompts", "none",
            "--max-turns", "24", "--system-prompt", CLAUDE_SYSTEM_PROMPT, prompt,
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
            flow = validate_runtime_model_flow(items, plan)
            resources_read = 1
        else:
            flow = validate_docker_free_model_flow(items, plan)
            resources_read = 0
        # Derived, not asserted: nothing credential-shaped may exist in the
        # private directory this gate created, and whatever the client wrote
        # under its own home for this cwd is counted and removed.
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
            "builtins": list(CLAUDE_RESOURCE_TOOLS),
            "mcp_tool_timeout_ms": MCP_TOOL_TIMEOUT_MS,
            "prompt_sha256": m3.digest(prompt.encode()),
            "argv_sha256": m3.digest(json.dumps(claude_argv).encode()),
            "exit_code": outcome["exit_code"],
            "duration_seconds": outcome["duration_seconds"],
            "tool_calls": len(items),
            "artifact_resources_read": resources_read,
            "session": session,
            "model_flow": flow,
            "model_turn_completed": True,
            "model_events_sha256": m3.file_digest(events_path),
            "stderr_sha256": m3.file_digest(stderr_path),
        }
    finally:
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
        "direction_guard": check["direction_guard"],
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
        receipt["claude_code"] = {
            DOCKER_FREE: claude_gate(attempt, DOCKER_FREE, free_argv, plan, 900),
        }
        if closed_socket.exists():
            raise RuntimeError("a docker socket was created during the Docker-free mode")
        receipt["docker_free_socket_created"] = False
        calls = receipt["inspector"][DOCKER_FREE].pop("calls")
        if with_runtime:
            fingerprint = vendor_fingerprint(vendor)
            capture, capture_fingerprint = provisioned_vendor_capture()
            receipt["modes"][RUNTIME]["cargo_vendor_tree_sha256"] = fingerprint
            receipt["modes"][RUNTIME]["vendor_capture"] = str(capture.relative_to(ROOT)) \
                if capture.is_relative_to(ROOT) else capture.name
            receipt["modes"][RUNTIME]["vendor_capture_tree_sha256"] = capture_fingerprint
            runtime_argv = runtime_server_argv(
                attempt / f"state-{RUNTIME}", pathlib.Path(docker_socket), vendor, fingerprint,
                capture, capture_fingerprint)
            receipt["inspector"][RUNTIME] = inspector_gate(
                attempt, RUNTIME, runtime_argv, runtime_plan, 1800, 600_000)
            receipt["claude_code"][RUNTIME] = claude_gate(
                attempt, RUNTIME, runtime_argv, runtime_plan, 2400)
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
