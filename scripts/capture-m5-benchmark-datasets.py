#!/usr/bin/env python3
"""Capture the M5 criterion datasets on the admitted runtime image.

Until now these captures were produced by hand, and the hand procedure drifted:
`docs/validation/M5-01-benchmark-calibration.json` recorded a capture taken on
`sha256:e9ecc40d...`, an image ADR-077 does not admit. A measurement taken on
an unadmitted image is a claim the product cannot support, so the procedure is
a script now, and the script refuses to measure on anything but the digest
`crates/execution-adapter/src/performance_port.rs` names.

What it produces
----------------

Two sides, `--executions` genuinely independent container runs each:

* `baseline`   — `fixtures/benchmark` exactly as committed.
* `candidate`  — the same fixture with `work_unit` performing `n + n / 4`
  operations instead of `n`, i.e. the 25% more work the committed
  `criterion-candidate.tar` carried. Only the loop bound changes; the inner
  `step` operation, the seed and the other two workloads are untouched.

Each execution is its own container with its own fresh `CRITERION_HOME` volume,
so the export of one execution can carry nothing from another: ADR-073 §4 v2
resamples the *execution*, and that estimate exists only if the executions are
actually separate. The three executions of a side share one target volume,
which is what the product does for the repetitions of a single operation — it
creates one target volume and gives every repetition the same one — so the
binary is compiled once and measured three times rather than recompiled before
every measurement. The two sides are alternated (baseline, candidate, baseline,
...) because ADR-073 §2 says alternation is an operator protocol and not
something the product implements; the receipt records the real order.

The guest hashes its own read-only `/source` before each execution and the
receipt publishes it, so "which bytes did this execution measure" is answered by
the guest rather than asserted by the host.

Containment (ADR-073 §2, as the gateway applies it)
---------------------------------------------------

`--network=none`, `--cap-drop=ALL`, `--security-opt no-new-privileges`, the
committed quality seccomp profile, `--read-only` root, uid 65534:65534,
`--pids-limit=128`, `--cpus=1`, `--memory=1g`, sources mounted read-only, and
the vendor directory selected by `--config` on the cargo command line, where
Cargo's precedence puts it above every config file. Nothing is pulled and no
container ever has a network.

The frozen harness parameters are ADR-073 §2's: warm-up 3 s, measurement 5 s,
`--sample-size 30`, `--noplot`, `--color never`, under `--frozen --offline`.

Cleanup
-------

Every container and volume this script creates carries a per-invocation label.
They are removed on every exit path, including failure and interruption, and
the script reports anything that survived instead of assuming it did not.

Usage
-----

    python3 -B scripts/capture-m5-benchmark-datasets.py

Requires `fixtures/criterion-vendor/vendor` to be materialized; the script
materializes it if it is missing, which is a local, checksum-verified
extraction of the committed archives and never touches the network.
"""

from __future__ import annotations

import argparse
import datetime
import hashlib
import io
import json
import tempfile
import os
import pathlib
import re
import shutil
import signal
import subprocess
import sys
import tarfile
import time
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[1]
DOCKER = "docker"

# The only place the admitted digest lives. Read, never remembered: ADR-077 has
# already replaced the M5 digest once, and a copy in this file would be a
# second source of truth that can go stale exactly the way the old receipt did.
PORT_SOURCE = ROOT / "crates/execution-adapter/src/performance_port.rs"
IMAGE_TAG = "rust-engineering-runtime:1.98.1-arm64-m5"

SECCOMP = ROOT / "crates/execution-adapter/src/seccomp-rust-quality.json"
FIXTURE = ROOT / "fixtures/benchmark"
VENDOR = ROOT / "fixtures/criterion-vendor/vendor"
MATERIALIZER = ROOT / "fixtures/criterion-vendor/materialize.py"
DATASETS = ROOT / "fixtures/benchmark-datasets"
RECEIPT = ROOT / "docs/validation/M5-01-benchmark-calibration.json"
STAGE = ROOT / "target/m5-benchmark-capture"

# ADR-073 §2. Named here so a reader can compare them against
# `performance_gateway.rs` without running anything.
WARM_UP_SECONDS = 3
MEASUREMENT_SECONDS = 5
SAMPLE_SIZE = 30

# `performance_gateway::BENCHMARK_BUDGET_MS`. The capture is held to the same
# wall budget the product gives the whole operation; it is not raised to make a
# slow execution fit.
BENCH_TIMEOUT_S = 900
EXPORT_TIMEOUT_S = 120
CONTROL_TIMEOUT_S = 120

# `mutation_gateway::VOLUME_OPTIONS` and
# `performance_gateway::TARGET_VOLUME_OPTIONS`, verbatim.
VOLUME_OPTIONS = "size=64m,nr_inodes=8192,uid=65534,gid=65534,mode=0700,nosuid,nodev,noexec"
TARGET_VOLUME_OPTIONS = "size=512m,nr_inodes=65536,uid=65534,gid=65534,mode=0700,nosuid,nodev"

# `rust_gateway::environment()`, with `CARGO_HOME` moved off the read-only image
# onto a private tmpfs — cargo takes a lock under `CARGO_HOME` even offline —
# and `CRITERION_HOME` added, which is `PerformancePhase::BenchRun`'s own
# addition.
ENVIRONMENT = (
    "CARGO_HOME=/performance/cargo-home",
    "CARGO_INCREMENTAL=0",
    "CARGO_NET_OFFLINE=true",
    "CARGO_TARGET_DIR=/work/target",
    "CRITERION_HOME=/criterion",
    "HOME=/work",
    "PATH=/opt/rust/bin:/usr/bin:/bin",
    "RUSTC=/opt/rust/bin/rustc",
    "RUSTDOC=/opt/rust/bin/rustdoc",
    "RUSTFMT=/opt/rust/bin/rustfmt",
    "TMPDIR=/tmp",
)

# `performance_gateway::VENDOR_SELECTION`, plus the directory the ingested
# `CARGO_HOME` config declares in the product. Both go on the command line
# here, which is the precedence the source selection needs and the one the
# receipt has always claimed.
VENDOR_SELECTION = (
    "--config",
    'source.crates-io.replace-with="rust-mcp-vendor"',
    "--config",
    'source.rust-mcp-vendor.directory="/rust-mcp-vendor"',
)

# The candidate source delta: `work_unit` performs 25% more of the identical
# inner operation. Written as an exact before/after so a fixture edit upstream
# makes this script fail loudly instead of measuring something else.
CANDIDATE_BEFORE = """#[inline(never)]
pub fn work_unit(n: u64) -> u64 {
    let mut acc = SEED;
    let mut i = 0;
    while i < n {
"""
CANDIDATE_AFTER = """#[inline(never)]
pub fn work_unit(n: u64) -> u64 {
    let total = n + n / 4;
    let mut acc = SEED;
    let mut i = 0;
    while i < total {
"""

BENCHMARKS = ("reference", "slower_125", "control")



# Argument boundary. Sonar's taint rules (S2083, S8705, S8707) treat every CLI
# value as attacker-controlled; these types keep the operator's arguments but
# refuse anything outside the repository or the temporary directory, and only
# the validated value reaches a path or an argv.
_ALLOWED_ROOTS = tuple(
    os.path.realpath(str(base))
    for base in (ROOT, tempfile.gettempdir(), "/private/tmp", "/tmp")
)


def bounded_path(value: str) -> pathlib.Path:
    """argparse type: an absolute path under the repository or the temp dir."""
    real = os.path.realpath(os.path.expanduser(str(value)))
    for base in _ALLOWED_ROOTS:
        if os.path.commonpath([base, real]) == base:
            return pathlib.Path(real)
    raise argparse.ArgumentTypeError(f"{value!r} is outside the repository and the temporary directory")


def image_reference(value: str) -> str:
    """argparse type: a local Docker image reference, validated before it can
    become a docker argv element."""
    match = re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._:/@-]{0,255}", value)
    if match is None:
        raise argparse.ArgumentTypeError(f"{value!r} is not a local image reference")
    return match.group(0)

def utc_now() -> str:
    return datetime.datetime.now(datetime.UTC).isoformat(timespec="seconds").replace("+00:00", "Z")


class CaptureError(RuntimeError):
    """A refusal or a failed phase. Never a reason to keep measuring."""


def admitted_image() -> str:
    """The one digest `M5_IMAGE` names, read from the source of truth."""
    text = PORT_SOURCE.read_text(encoding="utf-8")
    match = re.search(
        r'pub const M5_IMAGE: &str =\s*"(sha256:[0-9a-f]{64})";', text
    )
    if match is None:
        raise CaptureError(f"cannot read M5_IMAGE from {PORT_SOURCE}")
    return match.group(1)


def run(arguments: list[str], *, timeout: int, binary: bool = False) -> subprocess.CompletedProcess:
    return subprocess.run(  # noqa: S603 - fixed program, argv built from constants
        [DOCKER, *arguments],
        cwd=ROOT,
        capture_output=True,
        text=not binary,
        timeout=timeout,
        check=False,
    )


class Sandbox:
    """Owns every container and volume this invocation creates, and removes
    them on every exit path. The label is per-invocation, so the sweep can
    never reach a container or volume that belonged to somebody else."""

    def __init__(self) -> None:
        self.nonce = uuid.uuid4().hex[:16]
        self.label = f"rust-mcp-m5-capture={self.nonce}"
        self.containers: list[str] = []
        self.volumes: list[str] = []
        self.leftovers: list[str] = []

    def volume(self, role: str, options: str) -> str:
        name = f"rust-mcp-m5-capture-{role}-{self.nonce}"
        completed = run(
            [
                "volume",
                "create",
                "--driver=local",
                "--opt=type=tmpfs",
                "--opt=device=tmpfs",
                f"--opt=o={options}",
                f"--label={self.label}",
                name,
            ],
            timeout=CONTROL_TIMEOUT_S,
        )
        if completed.returncode != 0:
            raise CaptureError(f"volume create failed: {completed.stderr.strip()}")
        self.volumes.append(name)
        return name

    def container_name(self, role: str) -> str:
        name = f"rust-mcp-m5-capture-{role}-{self.nonce}"
        self.containers.append(name)
        return name

    def drop_container(self, name: str) -> None:
        run(["rm", "--force", "--volumes", name], timeout=CONTROL_TIMEOUT_S)
        if name in self.containers:
            self.containers.remove(name)

    def drop_volume(self, name: str) -> None:
        run(["volume", "rm", "--force", name], timeout=CONTROL_TIMEOUT_S)
        if name in self.volumes:
            self.volumes.remove(name)

    def cleanup(self) -> list[str]:
        """Remove everything, then look again. What is reported is what the
        daemon still lists, not what this script believes it removed."""
        for name in list(self.containers):
            run(["rm", "--force", "--volumes", name], timeout=CONTROL_TIMEOUT_S)
        for name in list(self.volumes):
            run(["volume", "rm", "--force", name], timeout=CONTROL_TIMEOUT_S)
        for kind, listing in (
            ("container", ["ps", "--all", "--quiet", "--filter", f"label={self.label}"]),
            ("volume", ["volume", "ls", "--quiet", "--filter", f"label={self.label}"]),
        ):
            completed = run(listing, timeout=CONTROL_TIMEOUT_S)
            for identifier in completed.stdout.split():
                remove = ["rm", "--force", "--volumes", identifier]
                if kind == "volume":
                    remove = ["volume", "rm", "--force", identifier]
                run(remove, timeout=CONTROL_TIMEOUT_S)
            completed = run(listing, timeout=CONTROL_TIMEOUT_S)
            self.leftovers.extend(f"{kind} {value}" for value in completed.stdout.split())
        self.containers.clear()
        self.volumes.clear()
        return self.leftovers


def stage_sources() -> dict[str, pathlib.Path]:
    """Copy the fixture out of the working tree and apply the candidate delta
    to the copy. `fixtures/benchmark` is never written to."""
    if STAGE.exists():
        shutil.rmtree(STAGE)
    staged = {}
    for side in ("baseline", "candidate"):
        target = STAGE / side
        shutil.copytree(FIXTURE, target, ignore=shutil.ignore_patterns("target"))
        staged[side] = target
    library = staged["candidate"] / "src/lib.rs"
    text = library.read_text(encoding="utf-8")
    if text.count(CANDIDATE_BEFORE) != 1:
        raise CaptureError(
            "fixtures/benchmark/src/lib.rs no longer contains exactly one "
            "`work_unit` body this script knows how to modify"
        )
    library.write_text(text.replace(CANDIDATE_BEFORE, CANDIDATE_AFTER), encoding="utf-8")
    return staged


def bench_argv(image: str, name: str, source: pathlib.Path, output: str, target: str) -> list[str]:
    arguments = [
        "run",
        "--rm",
        "--pull=never",
        "--runtime=runc",
        "--init=false",
        "--network=none",
        "--read-only",
        "--cap-drop=ALL",
        "--security-opt=no-new-privileges=true",
        f"--security-opt=seccomp={SECCOMP}",
        "--ipc=private",
        "--cgroupns=private",
        "--pids-limit=128",
        "--cpus=1",
        "--memory=1g",
        "--memory-swap=1g",
        "--shm-size=1m",
        "--no-healthcheck",
        "--tmpfs=/work:rw,exec,nosuid,nodev,size=512m,mode=1777",
        "--tmpfs=/tmp:rw,nosuid,nodev,noexec,size=64m,mode=1777",
        "--tmpfs=/performance:rw,nosuid,nodev,noexec,size=64m,mode=0700,uid=65534,gid=65534",
        "--workdir=/source",
        "--hostname=sandbox",
        "--user=65534:65534",
        f"--name={name}",
    ]
    arguments += [f"--env={value}" for value in ENVIRONMENT]
    arguments += [
        f"--mount=type=bind,source={source},target=/source,readonly",
        f"--mount=type=bind,source={VENDOR},target=/rust-mcp-vendor,readonly",
        f"--mount=type=volume,source={target},target=/work/target,volume-nocopy,volume-driver=local",
        f"--mount=type=volume,source={output},target=/criterion,volume-nocopy,volume-driver=local",
        "--entrypoint=/opt/rust/bin/cargo",
        image,
        "bench",
        *VENDOR_SELECTION,
        "--frozen",
        "--offline",
        "--color=never",
        "--target-dir=/work/target",
        "--bench=perf",
        "--",
        "--noplot",
        "--color",
        "never",
        "--warm-up-time",
        str(WARM_UP_SECONDS),
        "--measurement-time",
        str(MEASUREMENT_SECONDS),
        "--sample-size",
        str(SAMPLE_SIZE),
    ]
    return arguments


def digest_argv(image: str, name: str, source: pathlib.Path) -> list[str]:
    """What the guest itself says it is about to measure.

    The whole point of this script is that a receipt must not be able to drift
    away from the thing it describes, and "which source did this execution
    build" is exactly as load-bearing as "which image ran it". The guest
    computes it, over the same read-only mount the benchmark uses, so the claim
    is not the host's word about a directory it prepared."""
    return [
        "run",
        "--rm",
        "--pull=never",
        "--runtime=runc",
        "--init=false",
        "--network=none",
        "--read-only",
        "--cap-drop=ALL",
        "--security-opt=no-new-privileges=true",
        f"--security-opt=seccomp={SECCOMP}",
        "--ipc=private",
        "--cgroupns=private",
        "--pids-limit=128",
        "--cpus=1",
        "--memory=1g",
        "--memory-swap=1g",
        "--shm-size=1m",
        "--no-healthcheck",
        "--workdir=/source",
        "--hostname=sandbox",
        "--user=65534:65534",
        f"--name={name}",
        f"--mount=type=bind,source={source},target=/source,readonly",
        "--entrypoint=/usr/bin/sha256sum",
        image,
        "/source/Cargo.lock",
        "/source/Cargo.toml",
        "/source/benches/perf.rs",
        "/source/src/lib.rs",
    ]


def guardian_argv(image: str, name: str, volume: str, target: str = "/criterion") -> list[str]:
    """`PerformancePhase::OutputGuardian` and `TargetGuardian`, for their reason.

    A local volume with `type=tmpfs` exists only while some container holds it:
    the driver mounts the tmpfs for the first user and discards it when the last
    one stops. Without a guardian the export container mounts a brand-new empty
    tmpfs and archives nothing, which is exactly what a first version of this
    script did. The guardian sleeps with the volume mounted READ-ONLY, so it
    holds the filesystem open and can write nothing into it."""
    return [
        "run",
        "--detach",
        "--pull=never",
        "--runtime=runc",
        "--init=false",
        "--network=none",
        "--read-only",
        "--cap-drop=ALL",
        "--security-opt=no-new-privileges=true",
        f"--security-opt=seccomp={SECCOMP}",
        "--ipc=private",
        "--cgroupns=private",
        "--pids-limit=128",
        "--cpus=1",
        "--memory=1g",
        "--memory-swap=1g",
        "--shm-size=1m",
        "--no-healthcheck",
        "--hostname=sandbox",
        "--user=65534:65534",
        f"--name={name}",
        f"--mount=type=volume,source={volume},target={target},readonly,volume-nocopy,volume-driver=local",
        "--entrypoint=/usr/bin/sleep",
        image,
        "3600",
    ]


def export_argv(image: str, name: str, output: str) -> list[str]:
    return [
        "run",
        "--rm",
        "--pull=never",
        "--runtime=runc",
        "--init=false",
        "--network=none",
        "--read-only",
        "--cap-drop=ALL",
        "--security-opt=no-new-privileges=true",
        f"--security-opt=seccomp={SECCOMP}",
        "--ipc=private",
        "--cgroupns=private",
        "--pids-limit=128",
        "--cpus=1",
        "--memory=1g",
        "--memory-swap=1g",
        "--shm-size=1m",
        "--no-healthcheck",
        "--workdir=/criterion",
        "--hostname=sandbox",
        "--user=65534:65534",
        f"--name={name}",
        f"--mount=type=volume,source={output},target=/criterion,readonly,volume-nocopy,volume-driver=local",
        "--entrypoint=/usr/bin/tar",
        image,
        "--create",
        "--file=-",
        "--format=ustar",
        "--sort=name",
        "--one-file-system",
        "--directory=/criterion",
        ".",
    ]


def measurements(archive: bytes) -> dict[str, dict[str, object]]:
    """The medians criterion itself computed, plus the extremes of the raw
    samples. Nothing here is recomputed from a model: `median_ns` is
    `estimates.json`'s point estimate, and the extremes are the smallest and
    largest observed nanoseconds per iteration."""
    found: dict[str, dict[str, object]] = {}
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:") as bundle:
        members = {
            member.name.removeprefix("./"): member
            for member in bundle.getmembers()
            if member.isfile()
        }
        for bench in BENCHMARKS:
            base = f"m5/{bench}/new"
            try:
                sample = json.loads(bundle.extractfile(members[f"{base}/sample.json"]).read())
                estimates = json.loads(bundle.extractfile(members[f"{base}/estimates.json"]).read())
                identity = json.loads(bundle.extractfile(members[f"{base}/benchmark.json"]).read())
            except KeyError as error:
                raise CaptureError(f"export is missing {error}") from error
            per_iteration = [time / iters for time, iters in zip(sample["times"], sample["iters"], strict=True)]
            found[bench] = {
                "sampling_mode": sample["sampling_mode"],
                "samples": len(per_iteration),
                "median_ns": round(estimates["median"]["point_estimate"], 2),
                "min_ns": round(min(per_iteration), 2),
                "max_ns": round(max(per_iteration), 2),
                "full_id": identity["full_id"],
                "directory_name": identity["directory_name"],
                "group_id": identity["group_id"],
                "function_id": identity["function_id"],
            }
    if len(found) != len(BENCHMARKS):
        raise CaptureError("export does not carry the three m5 benchmarks")
    return found


def capture(
    sandbox: Sandbox, image: str, side: str, index: int, source: pathlib.Path, target: str
) -> dict:
    """One execution: its own CRITERION_HOME, its own container, its own export.

    The target directory is the side's, not the execution's, because that is
    what the product does: `execute_operation` creates ONE target volume and
    gives every repetition of the operation the same one, so repetitions 2 and 3
    measure the binary repetition 1 built. Rebuilding before each measurement
    would be a different protocol from the one the datasets are supposed to
    describe, and it puts a heavy compile immediately before every measurement.
    The build is deterministic, so nothing is shared but the artifacts that were
    identical anyway."""
    role = f"{side}-{index}"
    output = sandbox.volume(f"output-{role}", VOLUME_OPTIONS)
    guardian_name = sandbox.container_name(f"guardian-{role}")
    bench_name = sandbox.container_name(f"bench-{role}")
    export_name = sandbox.container_name(f"export-{role}")
    bench = bench_argv(image, bench_name, source, output, target)
    export = export_argv(image, export_name, output)

    digest_name = sandbox.container_name(f"digest-{role}")
    digested = run(digest_argv(image, digest_name, source), timeout=CONTROL_TIMEOUT_S)
    if digested.returncode != 0:
        raise CaptureError(f"{role}: source digest failed: {digested.stderr.strip()}")
    guest_source = {}
    for line in digested.stdout.splitlines():
        value, path = line.split(None, 1)
        guest_source[path.strip()] = value

    guardian = run(guardian_argv(image, guardian_name, output), timeout=CONTROL_TIMEOUT_S)
    if guardian.returncode != 0:
        raise CaptureError(f"{role}: output guardian failed: {guardian.stderr.strip()}")

    started = time.monotonic()
    completed = run(bench, timeout=BENCH_TIMEOUT_S)
    elapsed = round(time.monotonic() - started, 1)
    if completed.returncode != 0:
        raise CaptureError(
            f"{role}: cargo bench exited {completed.returncode}\n{completed.stderr[-4000:]}"
        )

    exported = run(export, timeout=EXPORT_TIMEOUT_S, binary=True)
    if exported.returncode != 0:
        raise CaptureError(
            f"{role}: export exited {exported.returncode}\n{exported.stderr.decode(errors='replace')[-2000:]}"
        )
    archive = exported.stdout

    sandbox.drop_container(guardian_name)
    sandbox.drop_volume(output)

    path = DATASETS / f"criterion-{side}-{index}.tar"
    path.write_bytes(archive)
    return {
        "file": str(path.relative_to(ROOT)),
        "side": side,
        "execution_index": index,
        "role": (
            "unmodified fixture" if side == "baseline" else "work_unit performs n + n / 4 operations"
        ),
        "sha256": hashlib.sha256(archive).hexdigest(),
        "bytes": len(archive),
        "guest_source_sha256": guest_source,
        "bench_exit_code": completed.returncode,
        "export_exit_code": exported.returncode,
        "wall_seconds": elapsed,
        "benchmarks": measurements(archive),
    }


def spread(captures: list[dict], side: str, bench: str) -> float:
    """Peak-to-peak spread of the medians of one benchmark across the
    executions of one side, as a fraction of the smallest of them."""
    medians = [
        entry["benchmarks"][bench]["median_ns"] for entry in captures if entry["side"] == side
    ]
    return round((max(medians) - min(medians)) / min(medians), 4)


def median_of(captures: list[dict], side: str, bench: str) -> float:
    medians = sorted(
        entry["benchmarks"][bench]["median_ns"] for entry in captures if entry["side"] == side
    )
    middle = len(medians) // 2
    if len(medians) % 2 == 1:
        return medians[middle]
    return (medians[middle - 1] + medians[middle]) / 2


def verify_sources(captures: list[dict]) -> None:
    """Every execution of a side must have measured the same bytes, and the two
    sides must differ in `src/lib.rs` and nowhere else. A capture set that fails
    this is not evidence about anything, so it is refused rather than
    published."""
    sides = {}
    for entry in captures:
        digests = entry["guest_source_sha256"]
        previous = sides.setdefault(entry["side"], digests)
        if previous != digests:
            raise CaptureError(
                f"{entry['side']} executions did not measure the same source: "
                f"{previous} vs {digests}"
            )
    if len(sides) == 2:
        baseline, candidate = sides["baseline"], sides["candidate"]
        differing = {path for path in baseline if baseline[path] != candidate.get(path)}
        if differing != {"/source/src/lib.rs"}:
            raise CaptureError(
                f"baseline and candidate differ in {sorted(differing)}, expected only src/lib.rs"
            )


def receipt(image: str, captures: list[dict], order: list[str], commands: dict) -> dict:
    exits = sorted(
        {entry["bench_exit_code"] for entry in captures}
        | {entry["export_exit_code"] for entry in captures}
    )
    return {
        "schema": "rust-engineering-mcp.m5-01-benchmark-calibration.v1",
        "captured_at_utc": utc_now(),
        "image_id": image,
        "image_admitted_by": "docs/adr/ADR-077-m5-runtime-admission.md",
        "decision": "docs/adr/ADR-073-benchmark-method-and-dataset.md",
        "captured_by": "scripts/capture-m5-benchmark-datasets.py",
        "harness": {
            "name": "criterion",
            "version": "0.8.2",
            "source": "fixtures/criterion-vendor (52 pinned archives, offline)",
        },
        "frozen_parameters": {
            "warm_up_seconds": WARM_UP_SECONDS,
            "measurement_seconds": MEASUREMENT_SECONDS,
            "sample_size": SAMPLE_SIZE,
            "plots": "disabled",
            "color": "never",
            "cargo_flags": ["--frozen", "--offline"],
            "criterion_home": "/criterion (per-execution tmpfs volume, noexec)",
        },
        "execution_protocol": {
            "executions_per_side": len([entry for entry in captures if entry["side"] == "baseline"]),
            "order": order,
            "independence": (
                "one container run and one fresh CRITERION_HOME volume per execution; the "
                "side's three executions share one target volume, as the product's repetitions "
                "of a single operation do, so the binary is built once and measured three times"
            ),
            "output_guardian": (
                "a sleeping container holds the tmpfs output volume read-only between the "
                "benchmark and the export, exactly as PerformancePhase::OutputGuardian does; "
                "without it the tmpfs is discarded and the export archives an empty tree"
            ),
            "alternation": (
                "the operator alternates the two sides (ADR-073 §2); the product does not, "
                "and this receipt records the order actually executed"
            ),
        },
        "candidate_modification": {
            "file": "src/lib.rs",
            "function": "work_unit",
            "change": "loop bound n replaced by n + n / 4, i.e. 25% more of the identical inner step",
            "unchanged": ["work_slower", "work_noisy", "step", "SEED", "benches/perf.rs"],
            "applied_to": "a copy under target/m5-benchmark-capture; fixtures/benchmark is never written",
            "verified": (
                "the guest hashed its own read-only /source before every execution; the three "
                "executions of a side hashed identically and the two sides differ only in "
                "src/lib.rs"
            ),
        },
        "sources_as_the_guest_hashed_them": {
            side: next(
                entry["guest_source_sha256"] for entry in captures if entry["side"] == side
            )
            for side in sorted({entry["side"] for entry in captures})
        },
        "source_selection": {
            "how": "--config on the cargo command line, highest Cargo precedence",
            "why": (
                "a project .cargo/config.toml overrides a CARGO_HOME config and could redirect "
                "source.crates-io at project-controlled bytes; the M5 flows refuse a project "
                "Cargo config outright"
            ),
            "arguments": list(VENDOR_SELECTION),
        },
        "containment": {
            "user": "65534:65534",
            "network": "none",
            "cap_drop": "ALL",
            "no_new_privileges": True,
            "seccomp": "seccomp-rust-quality.json",
            "source_mount": "read-only",
            "vendor_mount": "read-only",
            "root_filesystem": "read-only",
            "pids_limit": 128,
            "cpus": 1,
            "memory_bytes": 1073741824,
            "pull": "never",
        },
        "commands": commands,
        "exit_codes_observed": {str(code): "passed" for code in exits if code == 0}
        | {str(code): "observed" for code in exits if code != 0},
        "captures": captures,
        "observed": {
            bench: {
                "baseline_median_of_medians_ns": median_of(captures, "baseline", bench),
                "candidate_median_of_medians_ns": median_of(captures, "candidate", bench),
                "baseline_spread": spread(captures, "baseline", bench),
                "candidate_spread": spread(captures, "candidate", bench),
                "material_threshold": 0.05,
            }
            for bench in BENCHMARKS
        },
        "oracles": {
            "known_direction_different_source": {
                "baseline": "the three criterion-baseline-*.tar",
                "candidate": "the three criterion-candidate-*.tar",
                "benchmark": "m5/reference",
                "why": "work_unit performs 25% more operations in the candidate and only there",
            },
            "unchanged_benchmarks_null": {
                "baseline": "the three criterion-baseline-*.tar",
                "candidate": "the three criterion-candidate-*.tar",
                "benchmarks": ["m5/control", "m5/slower_125"],
                "why": (
                    "work_noisy and work_slower are byte-identical on both sides, so any "
                    "direction reported for them is host drift and not an effect"
                ),
            },
            "same_artifact": {"expected": "same_artifact incompatibility"},
        },
        "limitations": [
            "These medians describe these captures on this host. They are not a calibrated "
            "tolerance and do not generalize.",
            "Only the exit codes listed in exit_codes_observed were observed; nothing else is "
            "calibrated, and BenchmarkExit::CALIBRATED stays false.",
            "The three older captures in fixtures/benchmark-datasets (criterion-run-1.tar, "
            "criterion-run-2.tar, criterion-candidate.tar) were taken on image "
            "sha256:e9ecc40d023d9d13ac3539cccb6a944cd1022da2a8b3f86ca61356086b38a209, which "
            "ADR-077 does not admit. They are retained as parser fixtures and are not evidence "
            "about the admitted runtime.",
            "The fixture's `control` benchmark is not a 1.00x self-compare control; the real "
            "self-compare control is a benchmark whose source is identical on both sides, "
            "measured across independent executions.",
        ],
        "status": "passed",
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--executions", type=int, default=3, help="independent executions per side")
    parser.add_argument("--image", type=image_reference, default=IMAGE_TAG, help="local reference to resolve to M5_IMAGE")
    parser.add_argument("--output", type=bounded_path, default=RECEIPT)
    arguments = parser.parse_args()
    if arguments.executions < 1:
        raise CaptureError("a side needs at least one execution")

    image = admitted_image()
    resolved = run(
        ["image", "inspect", "--format", "{{.Id}}", arguments.image], timeout=CONTROL_TIMEOUT_S
    )
    if resolved.returncode != 0:
        raise CaptureError(
            f"{arguments.image} is not present locally; this script never pulls "
            f"({resolved.stderr.strip()})"
        )
    observed = resolved.stdout.strip()
    if observed != image:
        raise CaptureError(
            f"refusing to measure: {arguments.image} resolves to {observed}, and the only "
            f"admitted M5 image is {image} (crates/execution-adapter/src/performance_port.rs)"
        )

    if not SECCOMP.is_file():
        raise CaptureError(f"missing seccomp profile {SECCOMP}")
    if not VENDOR.is_dir():
        materialized = subprocess.run(  # noqa: S603 - fixed argv
            [sys.executable, "-B", str(MATERIALIZER)], cwd=ROOT, capture_output=True, text=True,
            timeout=600, check=False,
        )
        if materialized.returncode != 0 or not VENDOR.is_dir():
            raise CaptureError(f"vendor tree unavailable: {materialized.stderr[-2000:]}")

    staged = stage_sources()
    sandbox = Sandbox()
    # SIGINT already raises KeyboardInterrupt, which runs the `finally` below.
    # SIGTERM and SIGHUP do not, so they are turned into an exit that does.
    def terminate(number: int, _frame: object) -> None:
        sys.exit(f"interrupted by signal {number}")

    for received in (signal.SIGTERM, signal.SIGHUP):
        signal.signal(received, terminate)

    captures: list[dict] = []
    order: list[str] = []
    commands: dict = {}
    targets: dict[str, str] = {}
    try:
        # One target volume per side, held by a guardian, exactly as
        # `execute_operation` does for the repetitions of one operation.
        for side in ("baseline", "candidate"):
            volume = sandbox.volume(f"target-{side}", TARGET_VOLUME_OPTIONS)
            name = sandbox.container_name(f"target-guardian-{side}")
            held = run(
                guardian_argv(image, name, volume, "/work/target"), timeout=CONTROL_TIMEOUT_S
            )
            if held.returncode != 0:
                raise CaptureError(f"{side}: target guardian failed: {held.stderr.strip()}")
            targets[side] = volume
        for index in range(1, arguments.executions + 1):
            for side in ("baseline", "candidate"):
                print(f"[{utc_now()}] capturing {side} execution {index}", flush=True)
                entry = capture(sandbox, image, side, index, staged[side], targets[side])
                captures.append(entry)
                order.append(f"{side}-{index}")
                if not commands:
                    commands = {
                        "source_digest": [DOCKER, *digest_argv(image, "<container>", staged[side])],
                        "output_guardian": [DOCKER, *guardian_argv(image, "<container>", "<output-volume>")],
                        "target_guardian": [
                            DOCKER,
                            *guardian_argv(image, "<container>", "<target-volume>", "/work/target"),
                        ],
                        "benchmark": [DOCKER, *bench_argv(image, "<container>", staged[side], "<output-volume>", "<target-volume>")],
                        "export": [DOCKER, *export_argv(image, "<container>", "<output-volume>")],
                        "output_volume_create": [
                            DOCKER, "volume", "create", "--driver=local", "--opt=type=tmpfs",
                            "--opt=device=tmpfs", f"--opt=o={VOLUME_OPTIONS}", "<output-volume>",
                        ],
                        "target_volume_create": [
                            DOCKER, "volume", "create", "--driver=local", "--opt=type=tmpfs",
                            "--opt=device=tmpfs", f"--opt=o={TARGET_VOLUME_OPTIONS}", "<target-volume>",
                        ],
                    }
                print(
                    "    "
                    + ", ".join(
                        f"{bench} {entry['benchmarks'][bench]['median_ns']} ns" for bench in BENCHMARKS
                    ),
                    flush=True,
                )
    finally:
        leftovers = sandbox.cleanup()
        shutil.rmtree(STAGE, ignore_errors=True)
        if leftovers:
            print("LEFTOVERS: " + ", ".join(leftovers), file=sys.stderr, flush=True)
        else:
            print("no container or volume from this run survives", flush=True)

    verify_sources(captures)
    document = receipt(image, captures, order, commands)
    arguments.output.write_text(json.dumps(document, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"status": document["status"], "captures": len(captures)}))
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except CaptureError as failure:
        print(f"capture refused: {failure}", file=sys.stderr)
        sys.exit(1)
