#!/usr/bin/env python3
"""M8-05: measure startup, dispatch and RSS budgets for the ``core`` (and
self-provisioned ``local``) profile of ``rust-engineering-mcp serve
--stdio``, and compare them against ``tests/baselines/performance-budgets.json``.

Every path this script touches is a constant derived from ``ROOT``: the
taint engine used by SonarCloud's Python analysis treats any CLI-supplied
path that reaches ``open()``/``subprocess`` as a path traversal / command
injection risk regardless of ``argparse`` validation, so there is no
``--binary``/``--out``/``--budgets``/``--catalog-*`` override. ``--compare``
takes short keys (validated against a closed ``^[a-z0-9-]+$`` allowlist)
naming pre-existing receipts under the constant ``target/m8-performance/``
directory, never arbitrary paths.

Scope (docs/architecture/performance.md; historical receipts: docs/validation/M8/05.md,
docs/validation/M8/05-budgets-analysis.md at 51fa602e;
SS2/SS4): startup cold/warm (Popen -> tools/list), dispatch of
``rust.project.open``/``rust.catalog.status`` without Cargo, RSS idle and RSS
peak during dispatch, and the release binary's size. This harness never
spawns Docker and never invokes ``rust.check``: cancel-observed, cleanup and
archive size are out of scope here and are always reported ``unavailable``.

Every server process speaks the legacy wire protocol (``protocolVersion
2025-06-18``): ``initialize`` -> ``notifications/initialized`` -> plain
``tools/list``/``tools/call`` frames, one JSON object per newline on stdio
(see ``crates/mcp-server/tests/protocol.rs::initialize``/``bootstrap``).
"""
from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import math
import os
import pathlib
import platform
import re
import shutil
import statistics
import subprocess
import tempfile
import threading
import time
from collections.abc import Callable

ROOT = pathlib.Path(__file__).resolve().parents[1]
DEFAULT_BINARY = ROOT / "target/release/rust-engineering-mcp"
DEFAULT_BUDGETS = ROOT / "tests/baselines/performance-budgets.json"
OUT_PATH = ROOT / "target/qualification/m8-performance-measurement.json"
RECEIPTS_DIR = ROOT / "target/m8-performance"
COMPARE_OUT_PATH = RECEIPTS_DIR / "regression.json"
RECEIPT_KEY_PATTERN = re.compile(r"^[a-z0-9-]+$")
BUDGETS_SHA_PATTERN = re.compile(r"^sha256:[0-9a-f]{64}$")
MAGNITUDE_ID_PATTERN = re.compile(r"^[a-z][a-z0-9_]*$")
VERDICT_CHOICES = ("within", "over", "insufficient_samples", "unavailable")
PROFILE_CHOICES = ("core", "local")
FIXTURE = ROOT / "fixtures/valid-basic"
CATALOG_FIXTURE_DIR = ROOT / "fixtures/catalog"
CATALOG_BUNDLE = CATALOG_FIXTURE_DIR / "fixture-1.tar.zst"
CATALOG_TRUST_SOURCE = CATALOG_FIXTURE_DIR / "fixture-trust.json"
PROTOCOL_VERSION = "2025-06-18"
RSS_IDLE_SAMPLES = 10
IDLE_SETTLE_SECONDS = 5.0
RSS_SAMPLE_INTERVAL_SECONDS = 0.1
RSS_PEAK_MIN_SAMPLES = 30
PROCESS_TIMEOUT_SECONDS = 30
PMSET_BINARY = pathlib.Path("/usr/bin/pmset")
# Minimal, deterministic environment: same binary/host for every repetition,
# no ambient PATH/locale to perturb allocation or logging (noise controls,
# 05-budgets-analysis.md SS2.1 point 6).
CLEAN_ENV = {"LANG": "C", "LC_ALL": "C", "TZ": "UTC"}
DOCKER_UNAVAILABLE_REASON = "docker_not_used_in_this_package; measured by the orchestrator in M8-09"


def utc_now() -> str:
    return datetime.datetime.now(datetime.UTC).isoformat().replace("+00:00", "Z")


def head_commit() -> str:
    result = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True, check=True
    )
    return result.stdout.strip()


def host_info() -> dict[str, object]:
    info: dict[str, object] = {
        "machine": platform.machine(),
        "platform": platform.platform(),
        "cpu_count": os.cpu_count(),
    }
    try:
        result = subprocess.run(["sw_vers"], capture_output=True, text=True, timeout=5)
        if result.returncode == 0:
            info["sw_vers"] = result.stdout.strip()
    except (OSError, subprocess.TimeoutExpired):
        pass
    return info


def load_budgets(path: pathlib.Path) -> dict[str, dict]:
    payload = json.loads(path.read_text())
    return {row["id"]: row for row in payload["magnitudes"]}


def binary_stat(binary: pathlib.Path) -> tuple[int, str]:
    size = binary.stat().st_size
    with binary.open("rb") as stream:
        digest = hashlib.file_digest(stream, "sha256").hexdigest()
    return size, digest


def file_sha256(path: pathlib.Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def head_tree_dirty() -> bool:
    result = subprocess.run(
        ["git", "status", "--porcelain"], cwd=ROOT, capture_output=True, text=True, check=True
    )
    return bool(result.stdout.strip())


def battery_status() -> str | None:
    """``pmset -g batt`` output, or ``None`` if unavailable (non-macOS host, no AC/battery info)."""
    if not PMSET_BINARY.is_file():
        return None
    try:
        result = subprocess.run([str(PMSET_BINARY), "-g", "batt"], capture_output=True, text=True, timeout=5)
    except (OSError, subprocess.TimeoutExpired):
        return None
    if result.returncode != 0:
        return None
    return result.stdout.strip()


def validate_tool_result(response: dict, tool_name: str) -> None:
    """Raise if a ``tools/call`` response is not an honest pass (P-1: a refusal or
    ``isError`` response must never be timed as a valid dispatch sample)."""
    result = response.get("result")
    if not isinstance(result, dict):
        raise RuntimeError(f"{tool_name} response missing a result object")
    if result.get("isError") is True:
        raise RuntimeError(f"{tool_name} call reported isError=true")
    structured = result.get("structuredContent")
    if not isinstance(structured, dict) or structured.get("status") != "passed":
        raise RuntimeError(f"{tool_name} call did not report structuredContent.status=passed: {structured}")


def prepare_catalog(scratch: pathlib.Path, binary: pathlib.Path) -> tuple[pathlib.Path, pathlib.Path]:
    """Stage the constant fixture catalog bundle under ``scratch`` (a
    ``target/``-scoped directory this script creates itself, never a
    CLI-supplied path; mirrors ``soak-m8.py::prepare_catalog``)."""
    store = scratch / "catalog-store"
    store.mkdir(parents=True)
    os.chmod(store, 0o700)
    trust = scratch / "catalog-trust.json"
    shutil.copyfile(CATALOG_TRUST_SOURCE, trust)
    os.chmod(trust, 0o600)
    subprocess.run(
        [str(binary), "catalog", "import", str(CATALOG_BUNDLE), "--store", str(store), "--trust", str(trust), "--json"],
        check=True,
        capture_output=True,
        text=True,
    )
    return store, trust


def read_rss_mib(pid: int) -> float | None:
    """Sample resident set size for a live pid via ``ps``; None if it already exited."""
    try:
        result = subprocess.run(
            ["/bin/ps", "-o", "rss=", "-p", str(pid)], capture_output=True, text=True, timeout=5
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    text = result.stdout.strip()
    if result.returncode != 0 or not text:
        return None
    return int(text) / 1024.0


class ServerProcess:
    """One ``serve --stdio`` child, spoken to over newline-delimited JSON-RPC."""

    def __init__(self, binary: pathlib.Path, root: pathlib.Path, extra_args: list[str]):
        self.process = subprocess.Popen(
            [str(binary), "serve", "--stdio", "--root", str(root), *extra_args],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=CLEAN_ENV,
            start_new_session=True,
        )
        self._next_id = 1

    @property
    def pid(self) -> int:
        return self.process.pid

    def send(self, obj: dict) -> None:
        assert self.process.stdin is not None
        self.process.stdin.write((json.dumps(obj, separators=(",", ":")) + "\n").encode("utf-8"))
        self.process.stdin.flush()

    def recv(self) -> dict:
        assert self.process.stdout is not None
        line = self.process.stdout.readline()
        if not line:
            raise RuntimeError("server closed stdout before responding")
        return json.loads(line)

    def request(self, method: str, params: dict) -> dict:
        identifier = self._next_id
        self._next_id += 1
        self.send({"jsonrpc": "2.0", "id": identifier, "method": method, "params": params})
        response = self.recv()
        if response.get("id") != identifier:
            raise RuntimeError(f"response id mismatch for {method}")
        if "error" in response:
            raise RuntimeError(f"{method} returned a protocol error: {response['error']}")
        return response

    def handshake(self) -> None:
        response = self.request(
            "initialize",
            {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": "m8-performance-harness", "version": "1"},
            },
        )
        if response["result"].get("protocolVersion") != PROTOCOL_VERSION:
            raise RuntimeError("negotiated protocol version mismatch")
        self.send({"jsonrpc": "2.0", "method": "notifications/initialized"})

    def tools_list(self) -> dict:
        return self.request("tools/list", {})

    def call_tool(self, name: str, arguments: dict) -> dict:
        return self.request("tools/call", {"name": name, "arguments": arguments})

    def kill(self) -> None:
        if self.process.poll() is None:
            self.process.kill()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                pass

    def shutdown(self) -> tuple[int, bytes]:
        """Close stdin (EOF) and wait for a clean exit; returns (exit_code, stderr)."""
        assert self.process.stdin is not None
        self.process.stdin.close()
        try:
            exit_code = self.process.wait(timeout=PROCESS_TIMEOUT_SECONDS)
        except subprocess.TimeoutExpired as error:
            self.kill()
            raise RuntimeError("server did not exit after stdin EOF") from error
        stderr = self.process.stderr.read() if self.process.stderr else b""
        return exit_code, stderr


def measure_startup(binary: pathlib.Path, repeat: int) -> tuple[dict, list[dict], list[dict], list[dict]]:
    """Cold (Popen -> tools/list) and warm (second tools/list) samples.

    The first clean process is the host page-cache warm-up (05-budgets-analysis.md
    SS2.1 point 4): recorded separately, never counted in the statistics.
    """
    cold_samples: list[dict] = []
    warm_samples: list[dict] = []
    discarded: list[dict] = []
    host_warmup: dict | None = None
    attempt = 0
    max_attempts = (repeat + 1) * 3
    while len(cold_samples) < repeat and attempt < max_attempts:
        attempt += 1
        started = time.monotonic()
        server = ServerProcess(binary, FIXTURE, [])
        try:
            server.handshake()
            tools = server.tools_list()
            cold_ms = (time.monotonic() - started) * 1000
            if not isinstance(tools["result"].get("tools"), list):
                raise RuntimeError("tools/list result missing tools array")
            warm_started = time.monotonic()
            server.tools_list()
            warm_ms = (time.monotonic() - warm_started) * 1000
            exit_code, stderr = server.shutdown()
            if exit_code != 0 or stderr:
                raise RuntimeError(f"unclean shutdown: exit={exit_code} stderr_bytes={len(stderr)}")
        except Exception as error:  # noqa: BLE001 - recorded, not swallowed
            discarded.append({"attempt": attempt, "reason": str(error)})
            server.kill()
            continue
        if host_warmup is None:
            host_warmup = {"cold_ms": cold_ms, "warm_ms": warm_ms}
            continue
        cold_samples.append({"sample": len(cold_samples), "elapsed_ms": cold_ms})
        warm_samples.append({"sample": len(warm_samples), "elapsed_ms": warm_ms})
    if len(cold_samples) < repeat or host_warmup is None:
        raise RuntimeError(
            f"could not collect {repeat} clean startup samples "
            f"({len(cold_samples)} collected, {len(discarded)} discarded)"
        )
    return host_warmup, cold_samples, warm_samples, discarded


def collect_valid_dispatch_samples(
    call: Callable[[], tuple[dict, float]], tool_name: str, repeat: int
) -> tuple[list[dict], list[dict]]:
    """Call ``call`` until ``repeat`` valid (status=passed, isError!=true) dispatch
    samples are collected, discarding and recording any invalid response (P-1)."""
    valid: list[dict] = []
    discarded: list[dict] = []
    attempt = 0
    max_attempts = (repeat + 1) * 3
    while len(valid) < repeat and attempt < max_attempts:
        attempt += 1
        response, elapsed_ms = call()
        try:
            validate_tool_result(response, tool_name)
        except RuntimeError as error:
            discarded.append({"tool": tool_name, "attempt": attempt, "reason": str(error)})
            continue
        valid.append({"sample": len(valid), "elapsed_ms": elapsed_ms})
    if len(valid) < repeat:
        raise RuntimeError(
            f"could not collect {repeat} valid dispatch samples for {tool_name} "
            f"({len(valid)} collected, {len(discarded)} discarded)"
        )
    return valid, discarded


def _dispatch_call(server: ServerProcess, name: str, arguments: dict) -> Callable[[], tuple[dict, float]]:
    def call() -> tuple[dict, float]:
        started = time.monotonic()
        response = server.call_tool(name, arguments)
        return response, (time.monotonic() - started) * 1000

    return call


def measure_dispatch_and_peak(
    binary: pathlib.Path, repeat: int, extra_args: list[str], calls: list[tuple[str, dict]]
) -> tuple[dict[str, list[dict]], list[dict], list[dict]]:
    """Run ``repeat`` valid calls of every entry in ``calls`` on one warmed-up
    process, sampling RSS every 100 ms throughout (the "peak" window), for at
    least ``RSS_PEAK_MIN_SAMPLES`` samples (P-3)."""
    server = ServerProcess(binary, FIXTURE, extra_args)
    per_tool: dict[str, list[dict]] = {name: [] for name, _ in calls}
    discarded: list[dict] = []
    rss_samples: list[dict] = []
    stop = threading.Event()

    def sample_loop() -> None:
        started = time.monotonic()
        while not stop.is_set():
            value = read_rss_mib(server.pid)
            if value is not None:
                rss_samples.append({"t_ms": (time.monotonic() - started) * 1000, "rss_mib": value})
            stop.wait(RSS_SAMPLE_INTERVAL_SECONDS)

    sampler = threading.Thread(target=sample_loop, daemon=True)
    try:
        server.handshake()
        server.tools_list()  # warm-up, not part of the dispatch statistics
        sampler.start()
        for name, arguments in calls:
            valid, tool_discarded = collect_valid_dispatch_samples(
                _dispatch_call(server, name, arguments), name, repeat
            )
            per_tool[name] = valid
            discarded.extend(tool_discarded)
        deadline = time.monotonic() + RSS_PEAK_MIN_SAMPLES * RSS_SAMPLE_INTERVAL_SECONDS * 3
        while len(rss_samples) < RSS_PEAK_MIN_SAMPLES and time.monotonic() < deadline:
            time.sleep(RSS_SAMPLE_INTERVAL_SECONDS)
        stop.set()
        sampler.join(timeout=5)
        exit_code, stderr = server.shutdown()
        if exit_code != 0 or stderr:
            raise RuntimeError(f"unclean dispatch shutdown: exit={exit_code} stderr_bytes={len(stderr)}")
    except Exception:
        stop.set()
        server.kill()
        raise
    return per_tool, rss_samples, discarded


def measure_rss_idle(
    binary: pathlib.Path, samples: int, extra_args: list[str], settle_seconds: float = IDLE_SETTLE_SECONDS
) -> list[dict]:
    results: list[dict] = []
    for index in range(samples):
        server = ServerProcess(binary, FIXTURE, extra_args)
        try:
            server.handshake()
            server.tools_list()
            time.sleep(settle_seconds)
            rss = read_rss_mib(server.pid)
            if rss is None:
                raise RuntimeError("could not sample RSS for an idle process")
            exit_code, stderr = server.shutdown()
            if exit_code != 0 or stderr:
                raise RuntimeError(f"unclean idle shutdown: exit={exit_code} stderr_bytes={len(stderr)}")
        except Exception:
            server.kill()
            raise
        results.append({"sample": index, "rss_mib": rss})
    return results


def nearest_rank(values: list[float], fraction: float) -> float:
    return values[math.ceil(fraction * len(values)) - 1]


def summarize(values: list[float], budget_row: dict) -> dict:
    if not values:
        raise ValueError("cannot summarize an empty sample set")
    ordered = sorted(values)
    required_n = budget_row.get("n")
    if required_n is not None and len(ordered) < required_n:
        # P-3: a sample set smaller than the budget's own n never proves "within";
        # the peak (or whatever the row measures) may simply not have been observed.
        return {
            "status": "insufficient_samples",
            "n": len(ordered),
            "required_n": required_n,
            "budget": budget_row["budget"],
            "unit": budget_row["unit"],
            "statistic": budget_row["statistic"],
            "verdict": "insufficient_samples",
        }
    statistic_name = budget_row["statistic"]
    if statistic_name == "p95":
        statistic_value = nearest_rank(ordered, 0.95)
    elif statistic_name == "max":
        statistic_value = ordered[-1]
    else:
        raise ValueError(f"unsupported statistic: {statistic_name}")
    verdict = "within" if statistic_value <= budget_row["budget"] else "over"
    return {
        "status": "measured",
        "n": len(ordered),
        "min": ordered[0],
        "median": statistics.median(ordered),
        "p95": nearest_rank(ordered, 0.95),
        "max": ordered[-1],
        "statistic": statistic_name,
        "statistic_value": statistic_value,
        "budget": budget_row["budget"],
        "unit": budget_row["unit"],
        "verdict": verdict,
    }


def build_result(values: list[float], budget_row: dict, raw_samples: object) -> dict:
    result = summarize(values, budget_row)
    result["raw_samples"] = raw_samples
    return result


def unavailable(budget_row: dict, reason: str) -> dict:
    return {
        "status": "unavailable",
        "reason": reason,
        "budget": budget_row["budget"],
        "unit": budget_row["unit"],
        "statistic": budget_row["statistic"],
        "verdict": "unavailable",
    }


def global_verdict(measurements: dict[str, dict]) -> str:
    verdicts = {row["verdict"] for row in measurements.values()}
    if "over" in verdicts:
        return "over"
    if "insufficient_samples" in verdicts:
        return "insufficient_samples"
    if "unavailable" in verdicts:
        return "unavailable"
    return "within"


def known_magnitude_id(raw: object, source: str) -> str:
    """Re-derive a magnitude id from the closed ``MAGNITUDE_ID_PATTERN``
    grammar instead of trusting a receipt's ``measurements`` key verbatim.

    A receipt comes from ``path.read_text()``, so the taint engine follows
    its content (including dict keys) into whatever the caller writes next;
    ``match.group(0)`` is a fresh value re-derived from the regex, not the
    string read from disk, and an id outside the grammar fails loudly
    instead of silently entering the comparison receipt."""
    if not isinstance(raw, str):
        raise ValueError(f"{source}: magnitude id must be a string, got {raw!r}")
    match = MAGNITUDE_ID_PATTERN.match(raw)
    if not match:
        raise ValueError(f"{source}: magnitude id {raw!r} does not match {MAGNITUDE_ID_PATTERN.pattern!r}")
    return match.group(0)


def known_verdict(raw: object, source: str) -> str:
    """Reconstruct a per-magnitude verdict by membership in the closed
    ``VERDICT_CHOICES`` set (same taint rationale as ``known_magnitude_id``)."""
    if not isinstance(raw, str):
        raise ValueError(f"{source}: verdict must be a string, got {raw!r}")
    for choice in VERDICT_CHOICES:
        if choice == raw:
            return choice
    raise ValueError(f"{source}: verdict {raw!r} is not one of {VERDICT_CHOICES}")


def known_budgets_sha256(raw: object, source: pathlib.Path) -> str:
    """Re-derive ``budgets_sha256`` from the closed ``BUDGETS_SHA_PATTERN``
    grammar instead of trusting a receipt's field verbatim (same taint
    rationale as ``known_magnitude_id``)."""
    if not isinstance(raw, str):
        raise ValueError(f"{source}: budgets_sha256 must be a string, got {raw!r}")
    match = BUDGETS_SHA_PATTERN.match(raw)
    if not match:
        raise ValueError(f"{source}: budgets_sha256 {raw!r} does not match {BUDGETS_SHA_PATTERN.pattern!r}")
    return match.group(0)


def known_receipt_profile(raw: object, source: pathlib.Path) -> str:
    """Reconstruct a receipt's ``profile`` by membership in the closed
    ``PROFILE_CHOICES`` tuple (same taint rationale as ``known_magnitude_id``)."""
    if not isinstance(raw, str):
        raise ValueError(f"{source}: profile must be a string, got {raw!r}")
    for choice in PROFILE_CHOICES:
        if choice == raw:
            return choice
    raise ValueError(f"{source}: profile {raw!r} is not one of {PROFILE_CHOICES}")


def regression_verdict(receipts: list[dict]) -> dict[str, dict]:
    """2-of-3 regression rule (docs/architecture/performance.md; historical
    receipt: docs/validation/M8/05.md at 51fa602e): exactly 3 consecutive
    receipts sharing the same budgets and profile; a magnitude is only decided
    as regressed/not-regressed when the ``unavailable`` and ``insufficient_samples``
    receipts (if any) cannot change the outcome, and ``indeterminate`` otherwise (P-2)."""
    if len(receipts) != 3:
        raise ValueError("regression_verdict requires exactly 3 consecutive receipts")
    budgets_shas = {receipt.get("budgets_sha256") for receipt in receipts}
    if len(budgets_shas) != 1 or None in budgets_shas:
        raise ValueError("regression_verdict requires the same budgets_sha256 across all 3 receipts")
    profiles = {receipt.get("profile") for receipt in receipts}
    if len(profiles) != 1 or None in profiles:
        raise ValueError("regression_verdict requires the same profile across all 3 receipts")
    magnitude_ids: set[str] = set()
    for receipt in receipts:
        measurements = receipt.get("measurements")
        if not isinstance(measurements, dict):
            raise ValueError("regression_verdict requires each receipt's measurements to be an object")
        magnitude_ids.update(measurements)
    result: dict[str, dict] = {}
    for raw_magnitude_id in sorted(magnitude_ids):
        magnitude_id = known_magnitude_id(raw_magnitude_id, "regression_verdict")
        verdicts = [
            known_verdict(
                receipt["measurements"].get(magnitude_id, {}).get("verdict", "unavailable"),
                "regression_verdict",
            )
            for receipt in receipts
        ]
        over_count = sum(1 for verdict in verdicts if verdict == "over")
        unavailable_count = sum(
            1 for verdict in verdicts if verdict in ("unavailable", "insufficient_samples"))
        if over_count >= 2:
            outcome = "regressed"
        elif over_count + unavailable_count < 2:
            outcome = "not_regressed"
        else:
            outcome = "indeterminate"
        result[magnitude_id] = {
            "verdicts": verdicts,
            "over_count": over_count,
            "unavailable_count": unavailable_count,
            "outcome": outcome,
            "regressed": outcome == "regressed",
        }
    return result


def measure_core(binary: pathlib.Path, repeat: int, budgets: dict[str, dict]) -> tuple[dict, dict]:
    host_warmup, cold_samples, warm_samples, discarded = measure_startup(binary, repeat)
    for row in discarded:
        row["phase"] = "startup"
    measurements: dict[str, dict] = {
        "startup_cold_ms": build_result(
            [row["elapsed_ms"] for row in cold_samples], budgets["startup_cold_ms"], cold_samples
        ),
        "startup_warm_ms": build_result(
            [row["elapsed_ms"] for row in warm_samples], budgets["startup_warm_ms"], warm_samples
        ),
    }

    per_tool, rss_peak_samples, dispatch_discarded = measure_dispatch_and_peak(
        binary,
        repeat,
        [],
        [("rust.project.open", {"path": str(FIXTURE)}), ("rust.catalog.status", {})],
    )
    for row in dispatch_discarded:
        row["phase"] = "dispatch"
    discarded.extend(dispatch_discarded)
    measurements["dispatch_project_open_ms"] = build_result(
        [row["elapsed_ms"] for row in per_tool["rust.project.open"]],
        budgets["dispatch_project_open_ms"],
        per_tool["rust.project.open"],
    )
    measurements["dispatch_catalog_status_ms"] = build_result(
        [row["elapsed_ms"] for row in per_tool["rust.catalog.status"]],
        budgets["dispatch_catalog_status_ms"],
        per_tool["rust.catalog.status"],
    )
    if rss_peak_samples:
        measurements["rss_peak_core_mib"] = build_result(
            [row["rss_mib"] for row in rss_peak_samples], budgets["rss_peak_core_mib"], rss_peak_samples
        )
    else:
        measurements["rss_peak_core_mib"] = unavailable(
            budgets["rss_peak_core_mib"],
            "dispatch cycles completed faster than the 100 ms sampling interval; no RSS sample landed",
        )

    idle_samples = measure_rss_idle(binary, RSS_IDLE_SAMPLES, [])
    measurements["rss_idle_core_mib"] = build_result(
        [row["rss_mib"] for row in idle_samples], budgets["rss_idle_core_mib"], idle_samples
    )

    return measurements, {"host_warmup": host_warmup, "discarded_samples": discarded}


def measure_local(
    binary: pathlib.Path,
    repeat: int,
    budgets: dict[str, dict],
    scratch: pathlib.Path,
) -> dict[str, dict]:
    store, trust = prepare_catalog(scratch, binary)
    extra_args = ["--catalog-store", str(store), "--catalog-trust", str(trust)]

    idle_samples = measure_rss_idle(binary, RSS_IDLE_SAMPLES, extra_args)
    measurements = {
        "rss_idle_local_mib": build_result(
            [row["rss_mib"] for row in idle_samples], budgets["rss_idle_local_mib"], idle_samples
        )
    }
    per_tool, rss_peak_samples, _dispatch_discarded = measure_dispatch_and_peak(
        binary,
        repeat,
        extra_args,
        [
            ("rust.project.open", {"path": str(FIXTURE)}),
            ("rust.catalog.status", {}),
            ("rust.crate.search", {"query": "serde", "mode": "lexical"}),
        ],
    )
    if rss_peak_samples:
        measurements["rss_peak_local_mib"] = build_result(
            [row["rss_mib"] for row in rss_peak_samples], budgets["rss_peak_local_mib"], rss_peak_samples
        )
    else:
        measurements["rss_peak_local_mib"] = unavailable(
            budgets["rss_peak_local_mib"],
            "dispatch cycles completed faster than the 100 ms sampling interval; no RSS sample landed",
        )
    return measurements


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repeat", default="30")
    parser.add_argument("--profile", choices=PROFILE_CHOICES, default="core")
    parser.add_argument(
        "--operator-attested",
        action="store_true",
        help="Operator attests foreground-app quiescence was verified before this run (P-7); defaults to false.",
    )
    parser.add_argument(
        "--compare",
        nargs=3,
        metavar=("KEY1", "KEY2", "KEY3"),
        default=None,
        help=(
            "Apply the 2-of-3 regression rule to exactly 3 consecutive measurement receipts "
            f"instead of measuring (P-2). Each KEY must match {RECEIPT_KEY_PATTERN.pattern!r} "
            f"and name an existing {RECEIPTS_DIR}/KEY.json receipt."
        ),
    )
    return parser.parse_args()


def receipt_path_for_key(key: str) -> tuple[pathlib.Path, str]:
    """Validate ``key`` and return ``(path, validated_key)``.

    ``validated_key`` is ``match.group(0)``, re-derived from the regex
    rather than the original argv string: the taint engine follows
    ``args.compare`` into any receipt content it reaches, and a value
    re-derived from a closed pattern match breaks that flow.
    """
    match = RECEIPT_KEY_PATTERN.match(key)
    if not match:
        raise ValueError(f"--compare key must match {RECEIPT_KEY_PATTERN.pattern!r}: {key!r}")
    validated_key = match.group(0)
    return RECEIPTS_DIR / f"{validated_key}.json", validated_key


def run_compare(receipt_keys: list[str]) -> bool:
    """CLI entry point for the 2-of-3 regression rule (P-2). Returns True if any
    magnitude regressed."""
    resolved = [receipt_path_for_key(key) for key in receipt_keys]
    receipts = [json.loads(path.read_text()) for path, _validated_key in resolved]
    validated_keys = [validated_key for _path, validated_key in resolved]
    verdicts = regression_verdict(receipts)
    regressed = any(row["outcome"] == "regressed" for row in verdicts.values())
    indeterminate = any(row["outcome"] == "indeterminate" for row in verdicts.values())
    payload = {
        "schema": "rust-mcp-m8-performance-regression-v1",
        "generated_utc": utc_now(),
        "receipts": validated_keys,
        "budgets_sha256": known_budgets_sha256(receipts[0]["budgets_sha256"], resolved[0][0]),
        "profile": known_receipt_profile(receipts[0]["profile"], resolved[0][0]),
        "verdicts": verdicts,
        "regressed": regressed,
        "indeterminate": indeterminate,
    }
    COMPARE_OUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    COMPARE_OUT_PATH.write_text(json.dumps(payload, indent=2) + "\n")
    status = "REGRESSED" if regressed else ("INDETERMINATE" if indeterminate else "PASS")
    print(f"{status} m8 performance regression comparison written to {COMPARE_OUT_PATH}")
    return regressed


def main() -> None:
    args = parse_args()
    if args.compare:
        if run_compare(args.compare):
            raise SystemExit(1)
        return

    repeat = int(args.repeat)
    if repeat < 1:
        raise ValueError("--repeat must be a positive integer")
    # Re-derive from the constant tuple (selection by membership) instead of
    # storing args.profile: argparse's own choices= validation does not stop
    # the taint engine from following the argv string into the receipt.
    profile = next(choice for choice in PROFILE_CHOICES if choice == args.profile)
    operator_attested = bool(args.operator_attested)

    binary = DEFAULT_BINARY
    if not binary.is_file():
        raise FileNotFoundError(f"binary not found: {binary}")

    budgets = load_budgets(DEFAULT_BUDGETS)
    budgets_sha256 = file_sha256(DEFAULT_BUDGETS)
    binary_bytes, binary_sha256 = binary_stat(binary)

    measurements, provenance = measure_core(binary, repeat, budgets)
    measurements["binary_size_core_bytes"] = build_result(
        [binary_bytes], budgets["binary_size_core_bytes"], [{"sample": 0, "bytes": binary_bytes}]
    )
    measurements["archive_size_core_bytes"] = unavailable(
        budgets["archive_size_core_bytes"],
        "not measured by measure-m8-performance.py; scope is startup/dispatch/RSS/binary size only",
    )
    measurements["cancel_observed_p95_ms"] = unavailable(
        budgets["cancel_observed_p95_ms"], DOCKER_UNAVAILABLE_REASON
    )
    measurements["cleanup_p95_ms"] = unavailable(budgets["cleanup_p95_ms"], DOCKER_UNAVAILABLE_REASON)

    if profile == "local":
        scratch = pathlib.Path(tempfile.mkdtemp(prefix="m8-performance-", dir=str(ROOT / "target")))
        try:
            measurements.update(measure_local(binary, repeat, budgets, scratch))
        finally:
            shutil.rmtree(scratch, ignore_errors=True)
    else:
        measurements["rss_idle_local_mib"] = unavailable(budgets["rss_idle_local_mib"], "profile_core_requested")
        measurements["rss_peak_local_mib"] = unavailable(budgets["rss_peak_local_mib"], "profile_core_requested")

    binary_relative = str(binary.relative_to(ROOT))

    # P-7: re-hash at the end instead of only declaring the noise control as a constant.
    _end_bytes, binary_sha256_end = binary_stat(binary)
    same_binary_all_samples = binary_sha256_end == binary_sha256

    receipt = {
        "schema": "rust-mcp-m8-performance-measurement-v1",
        "generated_utc": utc_now(),
        "head_commit": head_commit(),
        "head_tree_dirty": head_tree_dirty(),
        "binary": binary_relative,
        "binary_sha256": f"sha256:{binary_sha256}",
        "binary_sha256_end": f"sha256:{binary_sha256_end}",
        "binary_bytes": binary_bytes,
        "profile": profile,
        "repeat": repeat,
        "budgets_sha256": f"sha256:{budgets_sha256}",
        "host": host_info(),
        "noise_controls": {
            "same_binary_all_samples": same_binary_all_samples,
            "serial_execution": True,
            "host_page_cache_warmup_discarded": True,
            "clean_env": sorted(CLEAN_ENV),
            "startup_repeat": repeat,
            "dispatch_repeat": repeat,
            "rss_idle_samples": RSS_IDLE_SAMPLES,
            "rss_peak_sample_interval_seconds": RSS_SAMPLE_INTERVAL_SECONDS,
            "operator_attested": operator_attested,
            "pmset_batt": battery_status(),
            "foreground_app_quiescence": (
                "not automatable on macOS; operator_attested records whether the "
                "operator confirmed quiescence before this run (05-budgets-analysis.md SS2.6)"
            ),
            "cpu_governor_control": (
                "not applicable on macOS host, unlike the guest Linux ADR-073 governor read "
                "(05-budgets-analysis.md SS2.6)"
            ),
        },
        "host_warmup": provenance["host_warmup"],
        "discarded_samples": provenance["discarded_samples"],
        "measurements": measurements,
        "verdict": global_verdict(measurements),
    }

    OUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    OUT_PATH.write_text(json.dumps(receipt, indent=2) + "\n")
    print(f"PASS m8 performance measurement written to {OUT_PATH} (verdict={receipt['verdict']})")


if __name__ == "__main__":
    if not __debug__:
        raise RuntimeError("Optimized Python mode is rejected")
    main()
