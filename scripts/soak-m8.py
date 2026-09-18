#!/usr/bin/env python3
"""M8-05 SS4 soak calibration for the ``core`` profile of ``rust-engineering-mcp
serve --stdio`` (docs/validation/M8/05.md, "Soak" section).

One long-lived server process, started with ``--project-ttl-secs`` (default
30s), opens the fixed fixture project at startup and reopens it whenever that
TTL elapses (``rust.catalog.status``/``rust.crate.search`` never consume a
``project_ref`` and cannot themselves signal expiry, so reopening is on an
assumed-expiry basis; each reopen is counted). It then runs ``cycles``
repetitions of ``rust.catalog.status -> rust.crate.search`` against a fixed
catalog (fixtures/catalog); the cancellation leg of the plan's cycle is
skipped honestly (there is no cancelable long-running operation in the
``core`` profile without Docker) and recorded as a documented gap rather than
faked. Every ``sample-every`` cycles the harness records RSS, open file
descriptors and child processes over this main phase, then applies the
plan's plateau (first 5% of cycles) and end-of-run failure criteria. The
first calibration run (50 cycles, ``target/m8-soak-calibration.json``) called
``rust.project.open`` on every cycle and failed ``fd_growth`` (8 -> 57): every
open retains fixture handles until its TTL. A separate, optional
``--open-churn`` phase runs *after* the main-phase criteria are evaluated to
produce that FD-growth evidence deliberately (it never counts against
``fd_growth``), then waits ``--ttl-wait-seconds`` (default 35s) and checks the
FD count. Project expiry turned out to be lazy (reaped on the *next*
``rust.project.open``, not by a timer): a wait-only sample stayed flat at 27
in calibration, so ``fd_after_ttl`` is evaluated after the wait *plus one more*
``rust.project.open`` of the same path, which triggers reclamation, against
the plateau + margin as a real pass/fail criterion, replacing any unevidenced
"bounded by TTL, not a leak" assumption with the numbers actually observed.

``--profile local`` is Docker's soak (the plan's SS4.2): this package never
touches Docker, so it is left as an honest ``NotImplementedError`` for the
orchestrator to run in M8-09, never simulated here.

The binary and the receipt destination (``docs/validation/M8/05-soak-core.json``)
are constants derived from ``ROOT``, not CLI flags: the taint engine used by
SonarCloud's Python analysis treats any CLI-supplied path that reaches
``open()``/``subprocess`` as a path traversal / command injection risk
regardless of ``argparse`` validation.
"""
from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import os
import pathlib
import shutil
import subprocess
import tempfile
import threading
import time

ROOT = pathlib.Path(__file__).resolve().parents[1]
DEFAULT_BINARY = ROOT / "target/release/rust-engineering-mcp"
OUT_PATH = ROOT / "docs/validation/M8/05-soak-core.json"
FIXTURE = ROOT / "fixtures/valid-basic"
CATALOG_FIXTURE_DIR = ROOT / "fixtures/catalog"
CATALOG_BUNDLE = CATALOG_FIXTURE_DIR / "fixture-1.tar.zst"
CATALOG_TRUST_SOURCE = CATALOG_FIXTURE_DIR / "fixture-trust.json"
PROTOCOL_VERSION = "2025-06-18"
DISPATCH_BUDGET_MS = 50  # docs/validation/M8/05-budgets.json: dispatch_*_ms
CYCLE_OVERRUN_MULTIPLIER = 3
RSS_GROWTH_MULTIPLIER = 1.2
FD_GROWTH_MARGIN = 10
PLATEAU_FRACTION = 0.05
CLEAN_ENV = {"LANG": "C", "LC_ALL": "C", "TZ": "UTC"}
DEFAULT_PROJECT_TTL_SECS = 30.0
DEFAULT_TTL_WAIT_SECONDS = 35.0
PROFILE_CHOICES = ("core", "local")
PMSET_BINARY = pathlib.Path("/usr/bin/pmset")
CATALOG_STORE_KNOWN_ENTRIES = {"active.bundle", "store.lock", "floor.record"}
CANCEL_CYCLE_GAP_REASON = (
    "core profile has no cancelable long-running operation without Docker "
    "(docs/validation/M8/05.md SS Soak); the cancel leg is a documented gap, not faked"
)


def utc_now() -> str:
    return datetime.datetime.now(datetime.UTC).isoformat().replace("+00:00", "Z")


def head_commit() -> str:
    result = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, capture_output=True, text=True, check=True
    )
    return result.stdout.strip()


def head_tree_dirty() -> bool:
    result = subprocess.run(
        ["git", "status", "--porcelain"], cwd=ROOT, capture_output=True, text=True, check=True
    )
    return bool(result.stdout.strip())


def binary_sha256(binary: pathlib.Path) -> str:
    with binary.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def read_rss_mib(pid: int) -> float | None:
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


def count_open_fds(pid: int) -> int | None:
    try:
        result = subprocess.run(
            ["/usr/sbin/lsof", "-a", "-p", str(pid), "-d", "^txt,^cwd,^rtd"],
            capture_output=True,
            text=True,
            timeout=10,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    lines = [line for line in result.stdout.splitlines() if line.strip()]
    if not lines:
        return None
    return max(len(lines) - 1, 0)  # first line is the lsof header


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
    """Raise if a ``tools/call`` response is not an honest pass (P-1)."""
    result = response.get("result")
    if not isinstance(result, dict):
        raise RuntimeError(f"{tool_name} response missing a result object")
    if result.get("isError") is True:
        raise RuntimeError(f"{tool_name} call reported isError=true")
    structured = result.get("structuredContent")
    if not isinstance(structured, dict) or structured.get("status") != "passed":
        raise RuntimeError(f"{tool_name} call did not report structuredContent.status=passed: {structured}")


def evaluate_cycle_calls(status_response: dict, search_response: dict) -> list[dict]:
    """Validate the two calls of one main-phase cycle, recording (not silently
    discarding) any invalid response (P-1)."""
    discards: list[dict] = []
    for tool_name, response in (
        ("rust.catalog.status", status_response),
        ("rust.crate.search", search_response),
    ):
        try:
            validate_tool_result(response, tool_name)
        except RuntimeError as error:
            discards.append({"tool": tool_name, "reason": str(error)})
    return discards


def should_reopen_project_ref(elapsed_since_open_seconds: float, ttl_seconds: float) -> bool:
    """Whether the harness should proactively reopen its ``project_ref`` (P-8):
    the core profile's main-phase calls never consume it, so there is no API to
    probe liveness directly; once the server's own TTL has elapsed, the ref is
    assumed expired."""
    return elapsed_since_open_seconds >= ttl_seconds


def evaluate_fd_after_ttl(
    plateau_fds: float | None,
    fd_count_after_ttl_wait: int | None,
    fd_count_after_reclaim_open: int | None,
) -> dict:
    """P-5: a real pass/fail criterion for FDs observed after the open-churn TTL
    wait plus one reclaiming ``rust.project.open`` call.

    Project expiry is lazy (docs/validation/M8/05.md "Hallazgo del soak"): the
    project registry reaps expired entries on the *next* open, not on a timer
    or on unrelated calls. So the pass/fail gate is evaluated against
    ``fd_count_after_reclaim_open``, not against the pre-reclaim wait sample;
    ``fd_count_after_ttl_wait`` is retained only as informational evidence of
    that bounded retention via ``retained_until_next_open``.
    """
    if plateau_fds is None or fd_count_after_reclaim_open is None:
        return {
            "applicable": False,
            "reason": "open_churn/--ttl-wait-seconds was not configured, or fds could not be sampled",
            "passed": True,
        }
    limit_fds = plateau_fds + FD_GROWTH_MARGIN
    retained_until_next_open = (
        fd_count_after_ttl_wait - plateau_fds if fd_count_after_ttl_wait is not None else None
    )
    return {
        "applicable": True,
        "plateau_fds": plateau_fds,
        "limit_fds": limit_fds,
        "measured_fds": fd_count_after_reclaim_open,
        "retained_until_next_open": retained_until_next_open,
        "passed": fd_count_after_reclaim_open <= limit_fds,
    }


def check_catalog_store_orphans(store_dir: pathlib.Path) -> dict:
    """P-6: the soak's catalog-store is real and inspectable, unlike
    ``--state-root`` (core profile has no Docker runtime); check it for stray
    files instead of passing state_root_orphans by decree."""
    entries = sorted(p.name for p in store_dir.iterdir())
    orphans = [name for name in entries if name not in CATALOG_STORE_KNOWN_ENTRIES]
    return {
        "applicable": True,
        "store_dir_entries": entries,
        "orphans": orphans,
        "passed": not orphans,
    }


def count_children(pid: int) -> int:
    try:
        result = subprocess.run(
            ["/usr/bin/pgrep", "-P", str(pid)], capture_output=True, text=True, timeout=5
        )
    except (OSError, subprocess.TimeoutExpired):
        return 0
    if result.returncode not in (0, 1):  # 1 == pgrep found nothing, not a failure
        return 0
    return len([line for line in result.stdout.splitlines() if line.strip()])


def prepare_catalog(scratch: pathlib.Path, binary: pathlib.Path) -> tuple[pathlib.Path, pathlib.Path]:
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


class ServerProcess:
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
                "clientInfo": {"name": "m8-soak-harness", "version": "1"},
            },
        )
        if response["result"].get("protocolVersion") != PROTOCOL_VERSION:
            raise RuntimeError("negotiated protocol version mismatch")
        self.send({"jsonrpc": "2.0", "method": "notifications/initialized"})
        self.request("tools/list", {})

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
        assert self.process.stdin is not None
        self.process.stdin.close()
        try:
            exit_code = self.process.wait(timeout=30)
        except subprocess.TimeoutExpired as error:
            self.kill()
            raise RuntimeError("server did not exit after stdin EOF") from error
        stderr = self.process.stderr.read() if self.process.stderr else b""
        return exit_code, stderr


def open_project(server: ServerProcess, path: pathlib.Path) -> str:
    response = server.call_tool("rust.project.open", {"path": str(path)})
    structured = response["result"]["structuredContent"]
    if structured.get("status") != "passed":
        raise RuntimeError(f"rust.project.open did not pass: {structured}")
    return structured["data"]["project_ref"]


def run_cycle(server: ServerProcess) -> tuple[float, list[dict]]:
    started = time.monotonic()
    status_response = server.call_tool("rust.catalog.status", {})
    search_response = server.call_tool("rust.crate.search", {"query": "serde", "mode": "lexical"})
    elapsed_ms = (time.monotonic() - started) * 1000
    return elapsed_ms, evaluate_cycle_calls(status_response, search_response)


def run_open_churn(
    server: ServerProcess, path: pathlib.Path, count: int, ttl_wait_seconds: float
) -> dict:
    """Open the same project ``count`` times to show the expected FD growth
    from unexpired live references, then sample fds again after
    ``ttl_wait_seconds`` (docs/tools.md "rust.project.open"), and once more
    after one additional open of the same path.

    Project expiry is lazy: expired projects are reaped on the *next* open,
    not by a timer or by unrelated calls (docs/validation/M8/05.md "Hallazgo
    del soak"). So the wait-only sample (``fd_count_after_ttl_wait``) is
    informational evidence of bounded retention, while the post-reclaim
    sample (``fd_count_after_reclaim_open``) is what actually demonstrates
    the expired handles were released, and feeds the ``fd_after_ttl``
    criterion (P-5). This runs after the main-phase ``fd_growth`` criterion
    is evaluated, so it never feeds that gate.
    """
    fd_before = count_open_fds(server.pid)
    for _ in range(count):
        open_project(server, path)
    fd_after = count_open_fds(server.pid)
    fd_after_ttl_wait = None
    fd_after_reclaim_open = None
    if ttl_wait_seconds > 0:
        time.sleep(ttl_wait_seconds)
        fd_after_ttl_wait = count_open_fds(server.pid)
        open_project(server, path)
        fd_after_reclaim_open = count_open_fds(server.pid)
    return {
        "count": count,
        "ttl_wait_seconds": ttl_wait_seconds,
        "fd_count_before": fd_before,
        "fd_count_after": fd_after,
        "fd_count_after_ttl_wait": fd_after_ttl_wait,
        "fd_count_after_reclaim_open": fd_after_reclaim_open,
    }


def evaluate_criteria(samples: list[dict], overruns: list[dict], state_root_applicable: bool) -> dict:
    """Apply docs/validation/M8/05.md's core soak failure criteria to a finished run.

    Exposed standalone (samples/overruns as plain data) so unit tests can feed
    synthetic series without spawning a server.
    """
    if not samples:
        raise ValueError("no samples collected; cannot evaluate soak criteria")
    plateau_index = max(0, round(PLATEAU_FRACTION * (len(samples) - 1)))
    plateau = samples[plateau_index]
    final = samples[-1]

    rss_limit = plateau["rss_mib"] * RSS_GROWTH_MULTIPLIER
    rss_passed = final["rss_mib"] <= rss_limit
    fd_limit = plateau["fd_count"] + FD_GROWTH_MARGIN
    fd_passed = final["fd_count"] <= fd_limit
    overrun_passed = len(overruns) == 0

    criteria = {
        "rss_growth": {
            "plateau_mib": plateau["rss_mib"],
            "final_mib": final["rss_mib"],
            "limit_mib": rss_limit,
            "passed": rss_passed,
        },
        "fd_growth": {
            "plateau_fds": plateau["fd_count"],
            "final_fds": final["fd_count"],
            "limit_fds": fd_limit,
            "passed": fd_passed,
        },
        "state_root_orphans": {
            "applicable": state_root_applicable,
            "reason": None if state_root_applicable else (
                "core profile declares no --rust runtime and no --state-root; "
                "there is no state-root to inspect for this profile (honest gap)"
            ),
            "passed": True,
        },
        "cycle_overrun": {
            "limit_ms": DISPATCH_BUDGET_MS * CYCLE_OVERRUN_MULTIPLIER,
            "count": len(overruns),
            "overruns": overruns,
            "passed": overrun_passed,
        },
    }
    passed = rss_passed and fd_passed and overrun_passed
    return {"plateau_index": plateau_index, "criteria": criteria, "passed": passed}


def run_core_soak(
    binary: pathlib.Path,
    cycles: int,
    hours: float,
    sample_every: int,
    scratch: pathlib.Path,
    open_churn: int,
    ttl_wait_seconds: float,
    project_ttl_secs: float,
    operator_attested: bool,
) -> dict:
    store, trust = prepare_catalog(scratch, binary)
    per_cycle_budget_seconds = (hours * 3600.0) / cycles if cycles else 0.0
    server = ServerProcess(
        binary,
        FIXTURE,
        [
            "--catalog-store", str(store),
            "--catalog-trust", str(trust),
            "--project-ttl-secs", str(int(project_ttl_secs)),
        ],
    )
    samples: list[dict] = []
    overruns: list[dict] = []
    call_discards: list[dict] = []
    reopens = 0
    binary_sha256_start = binary_sha256(binary)
    started_utc = utc_now()
    run_started = time.monotonic()
    try:
        server.handshake()
        open_project(server, FIXTURE)
        project_ref_opened_at = time.monotonic()
        for cycle in range(cycles):
            cycle_started = time.monotonic()
            if should_reopen_project_ref(cycle_started - project_ref_opened_at, project_ttl_secs):
                open_project(server, FIXTURE)
                project_ref_opened_at = time.monotonic()
                reopens += 1
            duration_ms, cycle_discards = run_cycle(server)
            for discard in cycle_discards:
                discard["cycle"] = cycle
            call_discards.extend(cycle_discards)
            if duration_ms > DISPATCH_BUDGET_MS * CYCLE_OVERRUN_MULTIPLIER:
                overruns.append({"cycle": cycle, "duration_ms": duration_ms})
            if cycle % sample_every == 0 or cycle == cycles - 1:
                samples.append(
                    {
                        "cycle": cycle,
                        "elapsed_seconds": time.monotonic() - run_started,
                        "rss_mib": read_rss_mib(server.pid),
                        "fd_count": count_open_fds(server.pid),
                        "child_count": count_children(server.pid),
                    }
                )
            remaining = per_cycle_budget_seconds - (time.monotonic() - cycle_started)
            if remaining > 0:
                time.sleep(remaining)

        complete_samples = [
            row for row in samples if row["rss_mib"] is not None and row["fd_count"] is not None
        ]
        if not complete_samples:
            raise RuntimeError("no complete RSS/FD samples were collected during the soak")
        evaluation = evaluate_criteria(complete_samples, overruns, state_root_applicable=False)

        churn = (
            run_open_churn(server, FIXTURE, open_churn, ttl_wait_seconds) if open_churn > 0 else None
        )
        fd_after_ttl = evaluate_fd_after_ttl(
            evaluation["criteria"]["fd_growth"]["plateau_fds"],
            churn["fd_count_after_ttl_wait"] if churn else None,
            churn["fd_count_after_reclaim_open"] if churn else None,
        )
        evaluation["criteria"]["fd_after_ttl"] = fd_after_ttl
        evaluation["passed"] = evaluation["passed"] and fd_after_ttl["passed"]

        catalog_store_orphans = check_catalog_store_orphans(store)
        evaluation["criteria"]["catalog_store_orphans"] = catalog_store_orphans
        evaluation["passed"] = evaluation["passed"] and catalog_store_orphans["passed"]

        exit_code, stderr = server.shutdown()
        if exit_code != 0 or stderr:
            raise RuntimeError(f"unclean soak shutdown: exit={exit_code} stderr_bytes={len(stderr)}")
    except Exception:
        server.kill()
        raise
    finished_utc = utc_now()
    binary_sha256_end = binary_sha256(binary)

    if fd_after_ttl["applicable"]:
        fd_after_ttl_note = (
            f"fd_after_ttl: after the {ttl_wait_seconds}s post-churn wait, open fds were "
            f"{churn['fd_count_after_ttl_wait']} (retained_until_next_open="
            f"{fd_after_ttl['retained_until_next_open']} vs. plateau {fd_after_ttl['plateau_fds']}); "
            f"after one additional rust.project.open of the same path, fds dropped to "
            f"{fd_after_ttl['measured_fds']} against a plateau-based limit of "
            f"{fd_after_ttl['limit_fds']} (passed={fd_after_ttl['passed']})."
        )
    else:
        fd_after_ttl_note = f"fd_after_ttl not measured: {fd_after_ttl['reason']}."

    notes = [
        "project_ref is opened at the start of the main phase and reopened whenever "
        f"--project-ttl-secs ({project_ttl_secs}s) elapses since the last open, since "
        "rust.catalog.status and rust.crate.search never consume a project_ref "
        f"(docs/tools.md) and cannot themselves signal expiry; {reopens} reopens occurred "
        "over this run on that assumed-expiry basis.",
        "fd_growth is evaluated only over the main phase samples above. The optional "
        "--open-churn phase (see open_churn below) intentionally repeats rust.project.open "
        "to show the FD growth from live references; " + fd_after_ttl_note,
        "Project expiry is lazy by design: expired projects are reaped on the next "
        "rust.project.open, not by a timer or by unrelated calls, and are bounded by "
        "--project-ttl-secs plus the session's open-project limit (docs/tools.md "
        "rust.project.open; ADR-030). This soak measures that bound for the fixed "
        "fixture path reopened repeatedly; it does not measure or claim anything about "
        "workloads that open many distinct paths and never reopen them.",
    ]

    return {
        "schema": "rust-mcp-m8-soak-v1",
        "profile": "core",
        "generated_utc": finished_utc,
        "started_utc": started_utc,
        "head_commit": head_commit(),
        "head_tree_dirty": head_tree_dirty(),
        "binary_sha256": f"sha256:{binary_sha256_start}",
        "binary_sha256_end": f"sha256:{binary_sha256_end}",
        "same_binary_all_samples": binary_sha256_start == binary_sha256_end,
        "operator_attested": operator_attested,
        "pmset_batt": battery_status(),
        "cycles_planned": cycles,
        "cycles_completed": cycles,
        "hours_planned": hours,
        "sample_every": sample_every,
        "per_cycle_budget_seconds": per_cycle_budget_seconds,
        "dispatch_budget_ms": DISPATCH_BUDGET_MS,
        "cycle_overrun_ceiling_ms": DISPATCH_BUDGET_MS * CYCLE_OVERRUN_MULTIPLIER,
        "project_ttl_secs": project_ttl_secs,
        "project_ref_reopens": reopens,
        "call_discards": call_discards,
        "samples": samples,
        "plateau_index": evaluation["plateau_index"],
        "criteria": evaluation["criteria"],
        "open_churn": churn,
        "cancel_cycle": {"executed": False, "reason": CANCEL_CYCLE_GAP_REASON},
        "notes": notes,
        "status": "passed" if evaluation["passed"] else "failed",
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--profile", choices=PROFILE_CHOICES, default="core")
    parser.add_argument("--cycles", default="1000")
    parser.add_argument("--hours", default="8")
    parser.add_argument("--sample-every", default="20")
    parser.add_argument("--open-churn", default="20")
    parser.add_argument("--ttl-wait-seconds", default=str(DEFAULT_TTL_WAIT_SECONDS))
    parser.add_argument("--project-ttl-secs", default=str(DEFAULT_PROJECT_TTL_SECS))
    parser.add_argument(
        "--operator-attested",
        action="store_true",
        help="Operator attests foreground-app quiescence was verified before this run (P-7); defaults to false.",
    )
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    # Re-derive from the constant tuple (selection by membership) instead of
    # storing args.profile: argparse's own choices= validation does not stop
    # the taint engine from following the argv string into the receipt.
    profile = next(choice for choice in PROFILE_CHOICES if choice == args.profile)
    if profile == "local":
        raise NotImplementedError(
            "soak-m8.py --profile local is Docker's soak (docs/validation/M8/05.md SS4.2); "
            "this package never touches Docker. The orchestrator runs this profile in M8-09."
        )

    binary = DEFAULT_BINARY
    if not binary.is_file():
        raise FileNotFoundError(f"binary not found: {binary}")
    if not CATALOG_BUNDLE.is_file() or not CATALOG_TRUST_SOURCE.is_file():
        raise FileNotFoundError(f"missing catalog fixtures under {CATALOG_FIXTURE_DIR}")

    cycles = int(args.cycles)
    hours = float(args.hours)
    sample_every = int(args.sample_every)
    open_churn = int(args.open_churn)
    ttl_wait_seconds = float(args.ttl_wait_seconds)
    project_ttl_secs = float(args.project_ttl_secs)
    if cycles < 1 or hours <= 0 or sample_every < 1:
        raise ValueError("--cycles/--sample-every must be positive integers and --hours must be positive")
    if open_churn < 0 or ttl_wait_seconds < 0:
        raise ValueError("--open-churn/--ttl-wait-seconds must not be negative")
    if project_ttl_secs <= 0:
        raise ValueError("--project-ttl-secs must be positive")
    operator_attested = bool(args.operator_attested)

    scratch = pathlib.Path(tempfile.mkdtemp(prefix="m8-soak-", dir=str(ROOT / "target")))
    try:
        receipt = run_core_soak(
            binary,
            cycles,
            hours,
            sample_every,
            scratch,
            open_churn,
            ttl_wait_seconds,
            project_ttl_secs,
            operator_attested,
        )
    finally:
        shutil.rmtree(scratch, ignore_errors=True)

    OUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    OUT_PATH.write_text(json.dumps(receipt, indent=2) + "\n")
    print(f"{receipt['status'].upper()} m8 soak ({profile}) written to {OUT_PATH}")


if __name__ == "__main__":
    if not __debug__:
        raise RuntimeError("Optimized Python mode is rejected")
    main()
