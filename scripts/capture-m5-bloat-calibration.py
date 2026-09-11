#!/usr/bin/env python3
"""Capture the M5-04 bloat calibration on the admitted runtime image.

`docs/validation/M5-04-bloat-calibration.json` used to record a capture taken on
`sha256:e9ecc40d...`, an image `docs/adr/ADR-077-m5-runtime-admission.md` does
not admit. A measurement taken on an image the product forbids is a claim the
product cannot support, so the procedure is a script now, it refuses to measure
on anything but the digest `crates/execution-adapter/src/performance_port.rs`
names, and it writes the receipt itself — the published numbers and the
published bytes therefore come from the same act.

What it produces
----------------

Every case the previous receipt recorded, on the admitted image:

* `release/functions`  — the per-function view of the `release` binary.
* `release/crates`     — the per-crate view of the same binary.
* `release/independent-measurement` — the product's own `stat`, `sha256sum` and
  `readelf -h` of the produced file, compared against the size `cargo-bloat`
  reported. This is the whole basis of ADR-076 §6's "exact size versus
  estimated attribution" separation: if the two disagree the attribution
  describes some other file, and the product downgrades completeness rather
  than publishing a ranking as though it described this one.
* `release_lto via CARGO_PROFILE_RELEASE_LTO=fat` — the only way this product
  reaches an LTO build, with its own independent measurement.
* `--profile release-lto` — NEGATIVE. The analyzer refuses it.
* `CARGO_PROFILE_RELEASE_STRIP=symbols` — NEGATIVE. It has no effect at all.
* `missing --bin target` — NEGATIVE. A `--bin` naming a target the project does
  not have.

The last three are the evidence behind ADR-076 §6. None of them is a product
path: `bloat_arguments` can never emit `--profile`, the product never sets
`CARGO_PROFILE_RELEASE_STRIP`, and `BloatOptions::new` validates the target
name. They are calibration probes and the receipt says so.

Honesty conventions
-------------------

* An exit code that was not observed is not listed as calibrated. Only 0 and 1
  are observed here, so only 0 and 1 appear in `calibrated_exits`;
  `BloatExit::CALIBRATED` stays false.
* A cause inferred from the analyzer's source is labelled `inferred`, never
  reported as measured. ADR-076 §6 was corrected for exactly this: the receipt
  records the failure and its text, not the environment variable the analyzer
  actually exported.
* The row counts are published as observed, against the product's own
  `BLOAT_MAX_ROWS`, together with what that cap drops.

Containment (from `performance_gateway.rs`, the real gateway)
-------------------------------------------------------------

`--network=none`, `--cap-drop=ALL`, `--security-opt no-new-privileges`, the
committed quality seccomp profile, `--read-only` root, uid 65534:65534,
`--pids-limit=128`, `--cpus=1`, `--memory=1g`, `--memory-swap=1g`,
`--shm-size=1m`, `--ipc=private`, `--cgroupns=private`, `--log-driver=none`,
the two `/work` and `/tmp` tmpfs mounts verbatim, `--workdir=/source`, and the
argv of `PerformancePhase::BloatFunctions`, `BloatCrates`, `BloatFileSize`,
`BloatFileDigest` and `BloatFileHeader`. Nothing is pulled and no container ever
has a network.

Where this script differs from the gateway
------------------------------------------

The differences are containment-neutral — the guest sees the same paths with the
same permissions — but they are real and the receipt publishes them:

1. `/source` and `/rust-mcp-vendor`: the gateway ingests a `SourceBundle` and a
   `CargoVendorSnapshot` into tmpfs volumes through `tar` on stdin. This script
   bind-mounts `fixtures/bloat` read-only, and an empty staged directory as the
   vendor, because the fixture has no external dependencies at all (its
   committed `Cargo.lock` holds only its own two packages). Same guest paths,
   same read-only mode; only the way the bytes arrive differs.
2. `/performance`: the same as the gateway — a tmpfs volume, a guardian holding
   it, and `PerformancePhase::ConfigIngest`'s `tar` writing
   `cargo-home/config.toml` with `security_policy::SECURITY_CARGO_CONFIG`'s
   bytes, mounted READ-ONLY into every measuring container.
3. The gateway creates each container with `docker container create`, then
   re-inspects it and compares everything the daemon applied against what the
   phase asked for (`verify_applied`). This script uses `docker run` and does
   not perform that comparison; the flags it passes are the gateway's, but
   nobody re-reads them back out of the daemon here.
4. The gateway carves each phase's deadline out of `BLOAT_BUDGET_MS` after a
   control reserve. This script gives every container the whole 300 s budget as
   a timeout. It never raises it.
5. `VENDOR_SELECTION` is the gateway's two arguments exactly — the directory
   behind the name is declared by the ingested `CARGO_HOME` config, not by a
   second `--config`. (`capture-m5-benchmark-datasets.py` puts both on the
   command line because it has no config volume; this one does.)
6. The gateway bounds each phase's captured output (`BLOAT_OUTPUT` 4 MiB,
   `MEASUREMENT_OUTPUT` 4 KiB). This script refuses an output over the same
   ceilings instead of carrying it silently. The bounds are not raised.

The tmpfs-volume lesson
-----------------------

A `--driver=local --opt=type=tmpfs` volume exists only while some container
holds it: the driver mounts the tmpfs for the first user and discards it when
the last one stops. Every such volume here is therefore held open by a sleeping
guardian container that mounts it READ-ONLY — which is what
`PerformancePhase::TargetGuardian` and `ConfigGuardian` are for — from before
the first writer until after the last reader.

One target volume per OPERATION, not per execution: `execute_operation` creates
one `/work/target` volume and gives every phase of that operation the same one,
so `cargo-bloat`'s own measurement and the product's `stat`/`sha256sum`/`readelf`
oracle observe the same file. Cases that need a different build — LTO, forced
stripping — are different operations and get their own target volume, because a
shared one would have them measure each other's artifacts.

Cleanup
-------

Every container and volume this script creates carries a per-invocation label.
They are removed on every exit path, including failure and interruption, and the
script reports anything that survived instead of assuming it did not.
`fixtures/bloat` is mounted read-only and never written; the script hashes it
before and after and refuses to publish if a byte moved.

Usage
-----

    python3 -B scripts/capture-m5-bloat-calibration.py
"""

from __future__ import annotations

import argparse
import contextlib
import datetime
import hashlib
import io
import json
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
# already replaced the M5 digest once, and a copy in this file would be a second
# source of truth that can go stale exactly the way the old receipt did.
PORT_SOURCE = ROOT / "crates/execution-adapter/src/performance_port.rs"
IMAGE_TAG = "rust-engineering-runtime:1.98.1-arm64-m5"

# The report cap is the product's, and is read from the product for the same
# reason. The receipt reports the rows observed against it; it never changes it.
DOMAIN_SOURCE = ROOT / "crates/domain/src/bloat.rs"

SECCOMP = ROOT / "crates/execution-adapter/src/seccomp-rust-quality.json"
FIXTURE = ROOT / "fixtures/bloat"
RECEIPT = ROOT / "docs/validation/M5-04-bloat-calibration.json"
STAGE = ROOT / "target/m5-bloat-capture"

# The fixture's binary target, from `fixtures/bloat/Cargo.toml`. It is also a
# name `BloatOptions::new` accepts, which the two negative names below are not.
BINARY_TARGET = "rust-mcp-bloat-fixture"
ABSENT_TARGET = "absent-target"

# `performance_gateway`: TARGET_ROOT and `binary_path`. Both profiles build into
# `release/`, because the LTO profile is expressed as an environment variable
# over `release` and never as a profile name.
TARGET_ROOT = "/work/target"
BINARY_PATH = f"{TARGET_ROOT}/release/{BINARY_TARGET}"

# `performance_gateway::BLOAT_BUDGET_MS`, in seconds. The capture is held to the
# same wall budget the product gives the whole operation; it is not raised to
# make a slow phase fit.
BLOAT_TIMEOUT_S = 300
CONTROL_TIMEOUT_S = 120

# `performance_gateway::BLOAT_OUTPUT` and `MEASUREMENT_OUTPUT`, verbatim. An
# output above these is refused here, as the product refuses it, rather than
# being carried into a receipt the product could not have produced.
BLOAT_OUTPUT = 4 * 1024 * 1024
MEASUREMENT_OUTPUT = 4 * 1024

# `mutation_gateway::VOLUME_OPTIONS` and
# `performance_gateway::TARGET_VOLUME_OPTIONS`, verbatim. The target volume is
# the executable one: `noexec` is absent because cargo runs build scripts.
VOLUME_OPTIONS = "size=64m,nr_inodes=8192,uid=65534,gid=65534,mode=0700,nosuid,nodev,noexec"
TARGET_VOLUME_OPTIONS = "size=512m,nr_inodes=65536,uid=65534,gid=65534,mode=0700,nosuid,nodev"

# `PerformancePhase::environment` for a bloat phase: `rust_gateway::environment()`
# with `CARGO_HOME` moved onto the ingested config volume. Sorted, as the
# gateway sorts it before `verify_applied` compares it.
ENVIRONMENT = (
    "CARGO_HOME=/performance/cargo-home",
    "CARGO_INCREMENTAL=0",
    "CARGO_NET_OFFLINE=true",
    "CARGO_TARGET_DIR=/work/target",
    "HOME=/work",
    "PATH=/opt/rust/bin:/usr/bin:/bin",
    "RUSTC=/opt/rust/bin/rustc",
    "RUSTDOC=/opt/rust/bin/rustdoc",
    "RUSTFMT=/opt/rust/bin/rustfmt",
    "TMPDIR=/tmp",
)

# `performance_gateway::VENDOR_SELECTION`, verbatim — two arguments, not four.
# The directory behind the name is declared by the ingested `CARGO_HOME` config
# below, which is where the gateway declares it.
VENDOR_SELECTION = ("--config", 'source.crates-io.replace-with="rust-mcp-vendor"')

# `security_policy::SECURITY_CARGO_CONFIG`, byte for byte.
CARGO_CONFIG = (
    b"[net]\n"
    b"offline = true\n"
    b"[source.crates-io]\n"
    b'replace-with = "rust-mcp-vendor"\n'
    b"[source.rust-mcp-vendor]\n"
    b'directory = "/rust-mcp-vendor"\n'
)

# The files the guest hashes before measuring, so "which bytes did this capture
# describe" is answered by the guest rather than asserted by the host.
FIXTURE_FILES = (
    "Cargo.lock",
    "Cargo.toml",
    "bloat-inner/Cargo.toml",
    "bloat-inner/src/lib.rs",
    "src/main.rs",
)

# `bloat_json::MAX_BLOAT_NAME_BYTES`. Only display data, capped on a UTF-8
# boundary exactly as the product caps it, so a boundary row published here is
# the row the product would publish.
MAX_BLOAT_NAME_BYTES = 512

# `bloat_json::UNATTRIBUTED_CRATE`: the analyzer omits the `crate` key entirely
# for a symbol it cannot attribute, and names that bucket `[Unknown]` in its own
# per-crate view.
UNATTRIBUTED_CRATE = "[Unknown]"

SUPERSEDED_IMAGE = "sha256:e9ecc40d023d9d13ac3539cccb6a944cd1022da2a8b3f86ca61356086b38a209"



def beside_default(default: pathlib.Path, value: object) -> pathlib.Path:
    """Only the file name of a CLI path is honoured, and it lands beside the
    default: an argument can never address a location outside that directory.
    Sonar's taint rules treat every CLI value as attacker-controlled (S2083,
    S8707); `os.path.basename` is the sanitizer they recognise."""
    return default.parent / os.path.basename(os.fspath(value))

def utc_now() -> str:
    return datetime.datetime.now(datetime.UTC).isoformat(timespec="seconds").replace("+00:00", "Z")


class CaptureError(RuntimeError):
    """A refusal or a failed phase. Never a reason to keep measuring."""


def admitted_image() -> str:
    """The one digest `M5_IMAGE` names, read from the source of truth."""
    text = PORT_SOURCE.read_text(encoding="utf-8")
    match = re.search(r'pub const M5_IMAGE: &str =\s*"(sha256:[0-9a-f]{64})";', text)
    if match is None:
        raise CaptureError(f"cannot read M5_IMAGE from {PORT_SOURCE}")
    return match.group(1)


def report_row_cap() -> int:
    """`rust_engineering_domain::bloat::BLOAT_MAX_ROWS`, read from the product."""
    text = DOMAIN_SOURCE.read_text(encoding="utf-8")
    match = re.search(r"pub const BLOAT_MAX_ROWS: usize = (\d+);", text)
    if match is None:
        raise CaptureError(f"cannot read BLOAT_MAX_ROWS from {DOMAIN_SOURCE}")
    return int(match.group(1))


def run(
    arguments: list[str], *, timeout: int, stdin_bytes: bytes | None = None
) -> subprocess.CompletedProcess:
    """Docker, with the argv this script built. Text is decoded permissively:
    a symbol name is project-controlled and must never be able to abort the
    capture by not being UTF-8."""
    completed = subprocess.run(  # noqa: S603 - fixed program, argv built from constants
        [DOCKER, *arguments],
        cwd=ROOT,
        capture_output=True,
        input=stdin_bytes,
        timeout=timeout,
        check=False,
    )
    return subprocess.CompletedProcess(
        completed.args,
        completed.returncode,
        completed.stdout.decode("utf-8", errors="replace"),
        completed.stderr.decode("utf-8", errors="replace"),
    )


def host_fixture_digests() -> dict[str, str]:
    """Every file the host sees under `fixtures/bloat` right now. Taken before
    and after the capture: the fixture is mounted read-only and must come out
    unchanged, with nothing added and nothing removed."""
    return {
        str(path.relative_to(FIXTURE)): hashlib.sha256(path.read_bytes()).hexdigest()
        for path in sorted(FIXTURE.rglob("*"))
        if path.is_file()
    }


class Sandbox:
    """Owns every container and volume this invocation creates, and removes them
    on every exit path. The label is per-invocation, so the sweep can never
    reach a container or volume that belonged to somebody else."""

    def __init__(self) -> None:
        self.nonce = uuid.uuid4().hex[:16]
        self.label = f"rust-mcp-m5-bloat-capture={self.nonce}"
        self.containers: list[str] = []
        self.volumes: list[str] = []
        self.leftovers: list[str] = []

    def volume(self, role: str, options: str) -> str:
        name = f"rust-mcp-m5-bloat-capture-{role}-{self.nonce}"
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
        name = f"rust-mcp-m5-bloat-capture-{role}-{self.nonce}"
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


# -- container argv ----------------------------------------------------------


def containment(sandbox: Sandbox, name: str) -> list[str]:
    """`create_arguments_for_runtime`'s fixed flags, in its order."""
    return [
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
        "--log-driver=none",
        "--no-healthcheck",
        "--tmpfs=/work:rw,exec,nosuid,nodev,size=512m,mode=1777",
        "--tmpfs=/tmp:rw,nosuid,nodev,noexec,size=64m,mode=1777",
        "--workdir=/source",
        "--hostname=sandbox",
        "--user=65534:65534",
        f"--name={name}",
        f"--label={sandbox.label}",
    ]


def guardian_argv(sandbox: Sandbox, image: str, name: str, volume: str, target: str) -> list[str]:
    """`PerformancePhase::TargetGuardian` / `ConfigGuardian`, for their reason.

    A local volume with `type=tmpfs` exists only while some container holds it.
    Without a guardian the next container mounts a brand-new empty tmpfs and the
    previous phase's bytes are simply gone. The guardian sleeps with the volume
    mounted READ-ONLY, so it holds the filesystem open and can write nothing."""
    return [
        "run",
        "--detach",
        *containment(sandbox, name),
        f"--mount=type=volume,source={volume},target={target},readonly,volume-nocopy,volume-driver=local",
        "--entrypoint=/usr/bin/sleep",
        image,
        "3600",
    ]


def config_ingest_argv(sandbox: Sandbox, image: str, name: str, config: str) -> list[str]:
    """`PerformancePhase::ConfigIngest`: the only container that may write to
    `/performance`, and it writes one file whose bytes are the product's."""
    return [
        "run",
        "--rm",
        "--interactive",
        *containment(sandbox, name),
        f"--mount=type=volume,source={config},target=/performance,volume-nocopy,volume-driver=local",
        "--entrypoint=/usr/bin/tar",
        image,
        "--extract",
        "--file=-",
        "--directory=/performance",
        "--no-same-owner",
        "--no-same-permissions",
        "--keep-old-files",
    ]


def analysis_argv(
    sandbox: Sandbox,
    image: str,
    name: str,
    *,
    vendor: pathlib.Path,
    config: str,
    target: str,
    arguments: list[str],
    extra_env: tuple[str, ...] = (),
) -> list[str]:
    """`PerformancePhase::BloatFunctions` / `BloatCrates`.

    Mounts: `/source`, `/rust-mcp-vendor` and `/performance` read-only,
    `/work/target` writable — `permissions()` makes the target writable in
    exactly these two phases and nowhere else."""
    environment = sorted([*ENVIRONMENT, *extra_env])
    return [
        "run",
        "--rm",
        *containment(sandbox, name),
        *[f"--env={value}" for value in environment],
        f"--mount=type=bind,source={FIXTURE},target=/source,readonly",
        f"--mount=type=bind,source={vendor},target=/rust-mcp-vendor,readonly",
        f"--mount=type=volume,source={config},target=/performance,readonly,volume-nocopy,volume-driver=local",
        f"--mount=type=volume,source={target},target={TARGET_ROOT},volume-nocopy,volume-driver=local",
        "--entrypoint=/opt/perf/bin/cargo-bloat",
        image,
        *arguments,
    ]


def measure_argv(
    sandbox: Sandbox, image: str, name: str, *, target: str, program: str, arguments: list[str]
) -> list[str]:
    """`PerformancePhase::BloatFileSize` / `BloatFileDigest` / `BloatFileHeader`.

    The product's own measurement of the produced file. It mounts the target
    volume READ-ONLY and nothing else — no source, no vendor, no config — so
    what it reports cannot have been influenced by the project's bytes, and it
    cannot itself disturb the file it is measuring."""
    return [
        "run",
        "--rm",
        *containment(sandbox, name),
        *[f"--env={value}" for value in ENVIRONMENT],
        f"--mount=type=volume,source={target},target={TARGET_ROOT},readonly,volume-nocopy,volume-driver=local",
        f"--entrypoint={program}",
        image,
        *arguments,
    ]


def probe_argv(
    sandbox: Sandbox,
    image: str,
    name: str,
    program: str,
    arguments: list[str],
    mounts: tuple[str, ...] = (),
) -> list[str]:
    """A container carrying the same containment and no volume it does not need:
    used to hash the analyzer binary the image ships, to read its version, and to
    hash the fixture the guest is about to measure."""
    return [
        "run",
        "--rm",
        *containment(sandbox, name),
        *[f"--env={value}" for value in ENVIRONMENT],
        *mounts,
        f"--entrypoint={program}",
        image,
        *arguments,
    ]


def bloat_arguments(
    *, crates: bool, binary_target: str = BINARY_TARGET, profile: str | None = None
) -> list[str]:
    """`performance_gateway::bloat_arguments`, verbatim.

    `profile` exists only for the negative case: the product's argv can never
    contain `--profile`, because [`BloatProfile`] is expressed through the
    product-owned environment instead. Passing it here is the probe, not a
    capability."""
    arguments = ["bloat", *VENDOR_SELECTION]
    arguments += ["--profile", profile] if profile else ["--release"]
    arguments += ["--frozen", "--message-format", "json", "-n", "0"]
    arguments.append(f"--bin={binary_target}")
    arguments.append(f"--target-dir={TARGET_ROOT}")
    if crates:
        arguments.append("--crates")
    return arguments


# -- phases ------------------------------------------------------------------


def bounded(completed: subprocess.CompletedProcess, role: str, limit: int) -> None:
    """The product's output ceiling, applied here rather than worked around."""
    size = len(completed.stdout.encode("utf-8", errors="replace"))
    if size > limit:
        raise CaptureError(f"{role}: {size} bytes of stdout is above the product's {limit} ceiling")


@contextlib.contextmanager
def operation(sandbox: Sandbox, image: str, role: str):
    """One operation: one `/work/target` volume, held by one guardian, shared by
    every phase of that operation and by nothing else.

    `execute_operation` creates exactly one target volume per operation, so the
    analyzer's measurement and the product's independent oracle observe the same
    file. Two operations never share one: a build under different profile
    settings is a different file, and a shared volume would have one case
    measure another case's artifact."""
    volume = sandbox.volume(f"target-{role}", TARGET_VOLUME_OPTIONS)
    guardian = sandbox.container_name(f"target-guardian-{role}")
    held = run(guardian_argv(sandbox, image, guardian, volume, TARGET_ROOT), timeout=CONTROL_TIMEOUT_S)
    if held.returncode != 0:
        raise CaptureError(f"{role}: target guardian failed: {held.stderr.strip()}")
    try:
        yield volume
    finally:
        sandbox.drop_container(guardian)
        sandbox.drop_volume(volume)


def analyze(
    sandbox: Sandbox,
    image: str,
    role: str,
    *,
    vendor: pathlib.Path,
    config: str,
    target: str,
    arguments: list[str],
    extra_env: tuple[str, ...] = (),
) -> tuple[list[str], subprocess.CompletedProcess, float]:
    name = sandbox.container_name(role)
    argv = analysis_argv(
        sandbox,
        image,
        name,
        vendor=vendor,
        config=config,
        target=target,
        arguments=arguments,
        extra_env=extra_env,
    )
    started = time.monotonic()
    completed = run(argv, timeout=BLOAT_TIMEOUT_S)
    elapsed = round(time.monotonic() - started, 1)
    bounded(completed, role, BLOAT_OUTPUT)
    sandbox.containers.remove(name)
    return [DOCKER, *argv], completed, elapsed


def measure(
    sandbox: Sandbox, image: str, role: str, *, target: str, program: str, arguments: list[str]
) -> tuple[list[str], subprocess.CompletedProcess]:
    name = sandbox.container_name(role)
    argv = measure_argv(
        sandbox, image, name, target=target, program=program, arguments=arguments
    )
    completed = run(argv, timeout=CONTROL_TIMEOUT_S)
    bounded(completed, role, MEASUREMENT_OUTPUT)
    sandbox.containers.remove(name)
    return [DOCKER, *argv], completed


# -- readers -----------------------------------------------------------------


def report_name(value: str) -> str:
    """`bloat_json::report_name`: a control character refuses the report, and a
    long name is capped on a UTF-8 boundary rather than discarding the row.

    `char::is_control` is the Unicode Cc category, i.e. C0 and C1."""
    if any(ord(character) < 0x20 or 0x7F <= ord(character) <= 0x9F for character in value):
        raise CaptureError("the analyzer emitted a control character in a symbol name")
    encoded = value.encode("utf-8")
    if len(encoded) <= MAX_BLOAT_NAME_BYTES:
        return value
    return encoded[:MAX_BLOAT_NAME_BYTES].decode("utf-8", errors="ignore")


def functions_view(stdout: str, cap: int) -> dict[str, object]:
    """The per-function view, ordered and capped exactly as
    `bloat_json::parse_functions` orders and caps it, so the rows this receipt
    calls kept are the rows the product would keep."""
    document = json.loads(stdout)
    if "functions" not in document or "crates" in document:
        raise CaptureError("the functions view did not carry a functions array")
    rows = document["functions"]
    ordered = sorted(
        (
            {
                "crate_name": report_name(row.get("crate", UNATTRIBUTED_CRATE)),
                "name": report_name(row["name"]),
                "size_bytes": int(row["size"]),
                "unattributed": "crate" not in row,
            }
            for row in rows
        ),
        key=lambda row: (-row["size_bytes"], row["name"], row["crate_name"]),
    )
    kept, omitted = ordered[:cap], ordered[cap:]
    attributed = sum(row["size_bytes"] for row in ordered)
    boundary = {
        "smallest_kept_row": {
            "crate": kept[-1]["crate_name"],
            "name": kept[-1]["name"],
            "size_bytes": kept[-1]["size_bytes"],
        }
        if kept
        else None,
        "largest_omitted_row": {
            "crate": omitted[0]["crate_name"],
            "name": omitted[0]["name"],
            "size_bytes": omitted[0]["size_bytes"],
        }
        if omitted
        else None,
    }
    return {
        "reported_file_size": int(document["file-size"]),
        "text_section_size": int(document["text-section-size"]),
        "rows": len(ordered),
        "rows_without_crate_attribution": sum(1 for row in ordered if row["unattributed"]),
        "attributed_bytes_total": attributed,
        "report_row_cap": cap,
        "rows_kept_by_cap": len(kept),
        "rows_omitted_by_cap": len(omitted),
        "attributed_bytes_kept": sum(row["size_bytes"] for row in kept),
        "attributed_bytes_omitted": sum(row["size_bytes"] for row in omitted),
        **boundary,
    }


def crates_view(stdout: str, cap: int) -> dict[str, object]:
    document = json.loads(stdout)
    if "crates" not in document or "functions" in document:
        raise CaptureError("the crates view did not carry a crates array")
    rows = sorted(
        ({"name": report_name(row["name"]), "size_bytes": int(row["size"])} for row in document["crates"]),
        key=lambda row: (-row["size_bytes"], row["name"]),
    )
    return {
        "reported_file_size": int(document["file-size"]),
        "text_section_size": int(document["text-section-size"]),
        "rows": len(rows),
        "report_row_cap": cap,
        "rows_omitted_by_cap": max(0, len(rows) - cap),
        "crates": {row["name"]: row["size_bytes"] for row in rows},
    }


def elf_header(stdout: str) -> list[str]:
    """`measured_format` reads Class and Machine; the receipt also records Type,
    because a PIE and a static executable of the same size are not the same
    artifact and the reader should be able to tell."""
    fields = {}
    for line in stdout.splitlines():
        if ":" in line:
            key, _, value = line.partition(":")
            fields.setdefault(key.strip(), value.strip())
    if "ELF Header" not in stdout:
        raise CaptureError("readelf did not recognise the produced file as ELF")
    return [fields.get("Class", ""), fields.get("Machine", ""), fields.get("Type", "")]


def stderr_tail(completed: subprocess.CompletedProcess, limit: int = 1200) -> str:
    return completed.stderr.strip()[-limit:]


def observed_error(completed: subprocess.CompletedProcess) -> str:
    """The analyzer's failure, as one line, exactly as it printed it."""
    return " / ".join(
        line.strip()
        for line in completed.stderr.strip().splitlines()
        if line.strip() and not line.strip().startswith("Caused by")
    )


# -- the capture -------------------------------------------------------------


def independent_measurement(
    sandbox: Sandbox, image: str, image_role: str, target: str, reported_file_size: int | None
) -> dict[str, object]:
    """The product's own facts about the produced file, and the comparison that
    ADR-076 §6 rests on: `cargo-bloat`'s `file-size` against a size this product
    measured itself, with `sha256sum` and `readelf -h` beside it."""
    size_argv, size = measure(
        sandbox,
        image,
        f"size-{image_role}",
        target=target,
        program="/usr/bin/stat",
        arguments=["--format=%s", BINARY_PATH],
    )
    digest_argv, digested = measure(
        sandbox,
        image,
        f"digest-{image_role}",
        target=target,
        program="/usr/bin/sha256sum",
        arguments=[BINARY_PATH],
    )
    header_argv, header = measure(
        sandbox,
        image,
        f"header-{image_role}",
        target=target,
        program="/usr/bin/readelf",
        arguments=["-h", BINARY_PATH],
    )
    for role, completed in (("stat", size), ("sha256sum", digested), ("readelf", header)):
        if completed.returncode != 0:
            raise CaptureError(f"{image_role}: {role} exited {completed.returncode}: {stderr_tail(completed)}")
    stat_size = int(size.stdout.strip())
    sha256 = digested.stdout.split()[0]
    if len(sha256) != 64 or not all(character in "0123456789abcdef" for character in sha256):
        raise CaptureError(f"{image_role}: sha256sum printed something that is not a digest")
    return {
        "stat_size_bytes": stat_size,
        "sha256": sha256,
        "readelf": elf_header(header.stdout),
        "matches_reported_file_size": reported_file_size == stat_size,
        "reported_file_size": reported_file_size,
        "measurement_exits": {
            "stat": size.returncode,
            "sha256sum": digested.returncode,
            "readelf": header.returncode,
        },
        "argv": {"stat": size_argv, "sha256sum": digest_argv, "readelf": header_argv},
    }


def capture(sandbox: Sandbox, image: str, vendor: pathlib.Path, config: str, cap: int) -> list[dict]:
    """Every case, each on the target volume its own operation owns."""
    cases: list[dict] = []

    # --- release: functions, crates and the independent measurement ----------
    with operation(sandbox, image, "release") as target:
        functions_argv, functions, functions_seconds = analyze(
            sandbox,
            image,
            "release-functions",
            vendor=vendor,
            config=config,
            target=target,
            arguments=bloat_arguments(crates=False),
        )
        if functions.returncode != 0:
            raise CaptureError(f"release/functions exited {functions.returncode}: {stderr_tail(functions)}")
        crates_argv, crates, crates_seconds = analyze(
            sandbox,
            image,
            "release-crates",
            vendor=vendor,
            config=config,
            target=target,
            arguments=bloat_arguments(crates=True),
        )
        if crates.returncode != 0:
            raise CaptureError(f"release/crates exited {crates.returncode}: {stderr_tail(crates)}")
        functions_report = functions_view(functions.stdout, cap)
        crates_report = crates_view(crates.stdout, cap)
        measured = independent_measurement(
            sandbox, image, "release", target, functions_report["reported_file_size"]
        )
    if functions_report["reported_file_size"] != crates_report["reported_file_size"] or (
        functions_report["text_section_size"] != crates_report["text_section_size"]
    ):
        raise CaptureError(
            "the two views disagree about the file they looked at; `attribution` refuses "
            "exactly this, so the capture refuses it too"
        )
    cases.append(
        {
            "case": "release/functions",
            "product_path": True,
            "exit": functions.returncode,
            "wall_seconds": functions_seconds,
            "argv": functions_argv,
            **functions_report,
        }
    )
    cases.append(
        {
            "case": "release/crates",
            "product_path": True,
            "exit": crates.returncode,
            "wall_seconds": crates_seconds,
            "argv": crates_argv,
            **crates_report,
        }
    )
    cases.append(
        {
            "case": "release/independent-measurement",
            "product_path": True,
            "measured_by": "the product itself, in the guest, over a read-only target mount",
            **measured,
        }
    )
    release_size = measured["stat_size_bytes"]
    release_sha256 = measured["sha256"]

    # --- release_lto, the only way this product reaches an LTO build ---------
    with operation(sandbox, image, "release-lto") as target:
        lto_argv, lto, lto_seconds = analyze(
            sandbox,
            image,
            "lto-crates",
            vendor=vendor,
            config=config,
            target=target,
            arguments=bloat_arguments(crates=True),
            extra_env=("CARGO_PROFILE_RELEASE_LTO=fat",),
        )
        if lto.returncode != 0:
            raise CaptureError(f"release_lto exited {lto.returncode}: {stderr_tail(lto)}")
        lto_report = crates_view(lto.stdout, cap)
        lto_measured = independent_measurement(
            sandbox, image, "release-lto", target, lto_report["reported_file_size"]
        )
    cases.append(
        {
            "case": "release_lto via CARGO_PROFILE_RELEASE_LTO=fat",
            "product_path": True,
            "exit": lto.returncode,
            "wall_seconds": lto_seconds,
            "argv": lto_argv,
            "environment_added_by_the_product": "CARGO_PROFILE_RELEASE_LTO=fat",
            # ADR-076 §6: the two profiles differ in the environment and nowhere
            # else. Checked against what actually ran, not asserted.
            "argv_identical_to_release_crates": (
                lto_argv[lto_argv.index(image) + 1 :] == crates_argv[crates_argv.index(image) + 1 :]
            ),
            **lto_report,
            "stat_size_bytes": lto_measured["stat_size_bytes"],
            "sha256": lto_measured["sha256"],
            "readelf": lto_measured["readelf"],
            "matches_reported_file_size": lto_measured["matches_reported_file_size"],
            "smaller_than_release": lto_measured["stat_size_bytes"] < release_size,
            "release_stat_size_bytes": release_size,
        }
    )

    # --- NEGATIVE: `--profile release-lto` -----------------------------------
    with operation(sandbox, image, "profile-flag") as target:
        profile_argv, profile, profile_seconds = analyze(
            sandbox,
            image,
            "profile-flag",
            vendor=vendor,
            config=config,
            target=target,
            arguments=bloat_arguments(crates=False, profile="release-lto"),
        )
    if profile.returncode == 0:
        raise CaptureError("`--profile release-lto` succeeded; ADR-076 §6 rests on it failing")
    cases.append(
        {
            "case": "--profile release-lto",
            "product_path": False,
            "probe": (
                "the gateway argv with `--release` replaced by `--profile release-lto`; "
                "`bloat_arguments` can never emit `--profile`"
            ),
            "exit": profile.returncode,
            "supported": False,
            "wall_seconds": profile_seconds,
            "argv": profile_argv,
            "observed_error": observed_error(profile),
            "stderr": stderr_tail(profile),
            "cause": {
                "claim": (
                    "cargo-bloat 0.12.1 derives CARGO_PROFILE_<NAME>_STRIP from the profile "
                    "name (src/main.rs:690-696); a profile literally named `release-lto` "
                    "yields a key Cargo re-splits over `profile.release`"
                ),
                "basis": "inferred",
                "inferred_from": "the analyzer's source, read; not measured here",
                "measured": (
                    "the exit code and the error text above. The environment variable the "
                    "analyzer actually exported was not observed (ADR-076 §6)"
                ),
            },
        }
    )

    # --- NEGATIVE: CARGO_PROFILE_RELEASE_STRIP=symbols has no effect ---------
    with operation(sandbox, image, "strip-symbols") as target:
        strip_argv, strip, strip_seconds = analyze(
            sandbox,
            image,
            "strip-symbols",
            vendor=vendor,
            config=config,
            target=target,
            arguments=bloat_arguments(crates=False),
            extra_env=("CARGO_PROFILE_RELEASE_STRIP=symbols",),
        )
        if strip.returncode != 0:
            raise CaptureError(f"strip probe exited {strip.returncode}: {stderr_tail(strip)}")
        strip_report = functions_view(strip.stdout, cap)
        strip_measured = independent_measurement(
            sandbox, image, "strip-symbols", target, strip_report["reported_file_size"]
        )
    cases.append(
        {
            "case": "CARGO_PROFILE_RELEASE_STRIP=symbols",
            "product_path": False,
            "probe": (
                "the release argv with the caller's stripping request injected into the "
                "environment; the product never sets this variable"
            ),
            "exit": strip.returncode,
            "wall_seconds": strip_seconds,
            "argv": strip_argv,
            "environment_added_by_the_probe": "CARGO_PROFILE_RELEASE_STRIP=symbols",
            "reported_file_size": strip_report["reported_file_size"],
            "text_section_size": strip_report["text_section_size"],
            "rows": strip_report["rows"],
            "stat_size_bytes": strip_measured["stat_size_bytes"],
            "sha256": strip_measured["sha256"],
            "matches_reported_file_size": strip_measured["matches_reported_file_size"],
            "release_stat_size_bytes": release_size,
            "release_sha256": release_sha256,
            "strip_took_effect": strip_measured["stat_size_bytes"] != release_size
            or strip_measured["sha256"] != release_sha256,
            "cause": {
                "claim": (
                    "the analyzer unconditionally pushes CARGO_PROFILE_<PROFILE>_STRIP=false "
                    "because it needs the symbol table (src/main.rs:694-696)"
                ),
                "basis": "inferred",
                "inferred_from": "the analyzer's source, read; not measured here",
                "measured": (
                    "that the request had no effect: the produced file has the same size and "
                    "the same sha256 as the release case, measured by the product itself"
                ),
            },
        }
    )

    # --- NEGATIVE: a --bin target the project does not have ------------------
    with operation(sandbox, image, "missing-bin") as target:
        missing_argv, missing, missing_seconds = analyze(
            sandbox,
            image,
            "missing-bin",
            vendor=vendor,
            config=config,
            target=target,
            arguments=bloat_arguments(crates=False, binary_target=ABSENT_TARGET),
        )
    if missing.returncode == 0:
        raise CaptureError("a --bin target the project does not have succeeded")
    cases.append(
        {
            "case": "missing --bin target",
            "product_path": False,
            "probe": (
                f"the release argv with --bin={ABSENT_TARGET}, a target `fixtures/bloat` does "
                "not declare. `BloatOptions::new` validates the NAME, never its existence, so "
                "this failure is reachable from a well-formed request"
            ),
            "exit": missing.returncode,
            "wall_seconds": missing_seconds,
            "argv": missing_argv,
            "observed_error": observed_error(missing),
            "stderr": stderr_tail(missing),
        }
    )
    return cases


def analyzer_identity(sandbox: Sandbox, image: str) -> dict:
    """The analyzer, as the admitted image carries it. Measured here rather than
    copied from the previous receipt: the version and the binary are properties
    of the image digest, which is the whole reason this capture was redone.

    `cargo-bloat --version` refuses to run (`can be run only via cargo bloat`),
    so the version comes from the subcommand the product itself invokes."""
    name = sandbox.container_name("analyzer-digest")
    digested = run(
        probe_argv(sandbox, image, name, "/usr/bin/sha256sum", ["/opt/perf/bin/cargo-bloat"]),
        timeout=CONTROL_TIMEOUT_S,
    )
    if digested.returncode != 0:
        raise CaptureError(f"cannot hash the analyzer: {stderr_tail(digested)}")
    sandbox.containers.remove(name)
    version_name = sandbox.container_name("analyzer-version")
    version = run(
        probe_argv(
            sandbox, image, version_name, "/opt/perf/bin/cargo-bloat", ["bloat", "--version"]
        ),
        timeout=CONTROL_TIMEOUT_S,
    )
    if version.returncode != 0:
        raise CaptureError(f"cannot read the analyzer version: {stderr_tail(version)}")
    sandbox.containers.remove(version_name)
    return {
        "name": "cargo-bloat",
        "version_reported": version.stdout.strip(),
        "version_read_by": "/opt/perf/bin/cargo-bloat bloat --version, in the admitted image",
        "binary_path": "/opt/perf/bin/cargo-bloat",
        "binary_sha256": digested.stdout.split()[0],
        "binary_sha256_measured_by": "sha256sum, in the admitted image",
    }


def guest_fixture_digests(sandbox: Sandbox, image: str) -> dict[str, str]:
    """What the guest says it is about to measure, over the same read-only mount
    the analysis containers use."""
    name = sandbox.container_name("source-digest")
    digested = run(
        probe_argv(
            sandbox,
            image,
            name,
            "/usr/bin/sha256sum",
            [f"/source/{value}" for value in FIXTURE_FILES],
            mounts=(f"--mount=type=bind,source={FIXTURE},target=/source,readonly",),
        ),
        timeout=CONTROL_TIMEOUT_S,
    )
    if digested.returncode != 0:
        raise CaptureError(f"source digest failed: {stderr_tail(digested)}")
    sandbox.containers.remove(name)
    found = {}
    for line in digested.stdout.splitlines():
        value, path = line.split(None, 1)
        found[path.strip()] = value
    return found


def receipt(
    image: str,
    analyzer: dict,
    cases: list[dict],
    guest_source: dict[str, str],
    host_source: dict[str, str],
    cap: int,
) -> dict:
    exits = sorted({case["exit"] for case in cases if "exit" in case})
    names = {0: "passed", 1: "analysis_failed"}
    functions = next(case for case in cases if case["case"] == "release/functions")
    measurement = next(case for case in cases if case["case"] == "release/independent-measurement")
    return {
        "schema": "rust-engineering-mcp.m5-04-bloat-calibration.v1",
        "captured_at_utc": utc_now(),
        "image_id": image,
        "image_admitted_by": "docs/adr/ADR-077-m5-runtime-admission.md",
        "decision": "docs/adr/ADR-076-m5-performance-contracts.md",
        "captured_by": "scripts/capture-m5-bloat-calibration.py",
        "supersedes": {
            "image_id": SUPERSEDED_IMAGE,
            "why": (
                "the previous capture ran on an image ADR-077 does not admit; a measurement "
                "taken on a forbidden image is a claim the product cannot support"
            ),
            "note": (
                "every number here was re-measured, not carried over. ADR-077's amendment "
                "records that cargo-bloat came out byte-identical across the M5 image change "
                "(e3eaea0d…, 1 644 120 bytes) and only the profile helper differs, so numbers "
                "agreeing with the superseded capture is expected. That agreement is not what "
                "makes this capture valid; the admitted image is"
            ),
        },
        "analyzer": analyzer,
        "fixture": "fixtures/bloat",
        "fixture_sha256_as_the_guest_hashed_it": guest_source,
        "fixture_sha256_on_the_host_after_the_capture": host_source,
        "containment": {
            "taken_from": "crates/execution-adapter/src/performance_gateway.rs",
            "user": "65534:65534",
            "network": "none",
            "cap_drop": "ALL",
            "no_new_privileges": True,
            "seccomp": "seccomp-rust-quality.json",
            "source_mount": "read-only",
            "vendor_mount": "read-only",
            "config_mount": "read-only",
            "target_mount": "writable only in the two analyzer phases; read-only in the three "
            "measuring phases",
            "root_filesystem": "read-only",
            "pids_limit": 128,
            "cpus": 1,
            "memory_bytes": 1073741824,
            "pull": "never",
            "cargo_flags": ["--frozen"],
            "vendor_selection": list(VENDOR_SELECTION),
            "cargo_home": "/performance/cargo-home, ingested, read-only",
        },
        "differences_from_the_gateway": [
            "The gateway ingests /source and /rust-mcp-vendor into tmpfs volumes from a "
            "SourceBundle and a CargoVendorSnapshot; this capture bind-mounts fixtures/bloat "
            "read-only and an empty staged directory as the vendor, because the fixture has no "
            "external dependencies. Same guest paths, same read-only mode.",
            "The gateway creates each container with `docker container create` and then compares "
            "everything the daemon applied against what the phase asked for (verify_applied). "
            "This capture uses `docker run` and performs no such re-inspection.",
            "The gateway carves each phase's deadline out of BLOAT_BUDGET_MS after a control "
            "reserve; this capture gives each container the whole 300 s budget as a timeout.",
            "The three negative cases are not product paths: bloat_arguments can never emit "
            "--profile, the product never sets CARGO_PROFILE_RELEASE_STRIP, and BloatOptions "
            "validates the target name.",
        ],
        "report_bounds": {
            "row_cap": cap,
            "row_cap_source": "rust_engineering_domain::bloat::BLOAT_MAX_ROWS",
            "bloat_output_bytes": BLOAT_OUTPUT,
            "measurement_output_bytes": MEASUREMENT_OUTPUT,
            "name_bytes": MAX_BLOAT_NAME_BYTES,
        },
        "independent_measurement": {
            "why": (
                "ADR-076 §6: the file's size is measured by the product and is exact; every "
                "per-function and per-crate number is an estimate the analyzer produced. The "
                "two are never merged, and a disagreement makes the attribution describe some "
                "other file"
            ),
            "analyzer_reported_file_size": functions["reported_file_size"],
            "product_measured_size_bytes": measurement["stat_size_bytes"],
            "agree": measurement["matches_reported_file_size"],
        },
        "cases": cases,
        "calibrated_exits": {str(code): names[code] for code in exits if code in names},
        "exit_codes_observed": [int(code) for code in exits],
        "limitations": [
            "The measured file is an ANALYSIS build: the analyzer forces strip off, so the "
            "binary is not byte-identical to what a project asking for stripping would ship. "
            "Recorded in the DTO as analysis_build_symbols_forced.",
            "A stripped report is unreachable through this analyzer at all.",
            "`--profile` is unusable with Cargo 1.98.1; LTO is expressed through a "
            "product-owned environment variable instead.",
            "Only ELF64/AArch64 is exercised. Mach-O and PE are not qualified; WASM is "
            "unsupported by the analyzer.",
            "Only the exit codes in exit_codes_observed were observed; every other code "
            "remains uncalibrated and BloatExit::CALIBRATED stays false. 101 "
            "(CompilationFailed) was not produced by this capture and is not calibrated.",
            "The two `cause` fields are inferred from the analyzer's source and are labelled "
            "`inferred`; what was measured is the failure, its text, and the absence of any "
            "effect.",
            "These numbers describe this fixture, on this image, on this host. They are not a "
            "size budget and do not generalize to another project.",
        ],
        "status": "passed",
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--image", default=IMAGE_TAG, help=f"must be the local tag {IMAGE_TAG}; kept because receipts record it")
    parser.add_argument("--output", type=pathlib.Path, default=RECEIPT)
    arguments = parser.parse_args()
    if image_reference != IMAGE_TAG:
        parser.error(f"--image must be {IMAGE_TAG}; this script measures the admitted image only")
    image_reference = IMAGE_TAG  # a constant, never the argv string
    output_path = beside_default(RECEIPT, output_path)

    image = admitted_image()
    cap = report_row_cap()
    resolved = run(
        ["image", "inspect", "--format", "{{.Id}}", image_reference], timeout=CONTROL_TIMEOUT_S
    )
    if resolved.returncode != 0:
        raise CaptureError(
            f"{image_reference} is not present locally; this script never pulls "
            f"({resolved.stderr.strip()})"
        )
    observed = resolved.stdout.strip()
    if observed != image:
        raise CaptureError(
            f"refusing to measure: {image_reference} resolves to {observed}, and the only "
            f"admitted M5 image is {image} (crates/execution-adapter/src/performance_port.rs)"
        )

    if not SECCOMP.is_file():
        raise CaptureError(f"missing seccomp profile {SECCOMP}")
    for name in FIXTURE_FILES:
        if not (FIXTURE / name).is_file():
            raise CaptureError(f"the fixture is missing {name}")
    before = host_fixture_digests()

    # The fixture has no external dependencies, so the vendor directory the
    # product's source selection names is legitimately empty. It exists because
    # the selection must resolve, not because anything is resolved through it.
    if STAGE.exists():
        shutil.rmtree(STAGE)
    vendor = STAGE / "vendor"
    vendor.mkdir(parents=True)

    sandbox = Sandbox()

    # SIGINT already raises KeyboardInterrupt, which runs the `finally` below.
    # SIGTERM and SIGHUP do not, so they are turned into an exit that does.
    def terminate(number: int, _frame: object) -> None:
        sys.exit(f"interrupted by signal {number}")

    for received in (signal.SIGTERM, signal.SIGHUP):
        signal.signal(received, terminate)

    cases: list[dict] = []
    analyzer: dict = {}
    guest_source: dict[str, str] = {}
    try:
        config = sandbox.volume("config", VOLUME_OPTIONS)
        guardian = sandbox.container_name("config-guardian")
        held = run(
            guardian_argv(sandbox, image, guardian, config, "/performance"),
            timeout=CONTROL_TIMEOUT_S,
        )
        if held.returncode != 0:
            raise CaptureError(f"config guardian failed: {held.stderr.strip()}")
        ingest_name = sandbox.container_name("config-ingest")
        buffer = io.BytesIO()
        with tarfile.open(fileobj=buffer, mode="w", format=tarfile.USTAR_FORMAT) as bundle:
            info = tarfile.TarInfo("cargo-home/config.toml")
            info.size = len(CARGO_CONFIG)
            info.mode = 0o644
            bundle.addfile(info, io.BytesIO(CARGO_CONFIG))
        ingested = run(
            config_ingest_argv(sandbox, image, ingest_name, config),
            timeout=CONTROL_TIMEOUT_S,
            stdin_bytes=buffer.getvalue(),
        )
        if ingested.returncode != 0:
            raise CaptureError(f"config ingest failed: {ingested.stderr.strip()}")
        sandbox.containers.remove(ingest_name)

        analyzer = analyzer_identity(sandbox, image)
        guest_source = guest_fixture_digests(sandbox, image)
        print(f"[{utc_now()}] analyzer {analyzer['version_reported']} {analyzer['binary_sha256'][:12]}", flush=True)
        cases = capture(sandbox, image, vendor, config, cap)
        for case in cases:
            print(
                f"    {case['case']}: exit {case.get('exit', 'n/a')}"
                f" reported {case.get('reported_file_size', 'n/a')}"
                f" measured {case.get('stat_size_bytes', 'n/a')}",
                flush=True,
            )
    finally:
        leftovers = sandbox.cleanup()
        shutil.rmtree(STAGE, ignore_errors=True)
        if leftovers:
            print("LEFTOVERS: " + ", ".join(leftovers), file=sys.stderr, flush=True)
        else:
            print("no container or volume from this run survives", flush=True)

    after = host_fixture_digests()
    if before != after:
        raise CaptureError(
            "fixtures/bloat changed during the capture; it is mounted read-only and must not"
        )
    expected = {f"/source/{name}": after[name] for name in FIXTURE_FILES}
    if guest_source != expected:
        raise CaptureError(
            f"the guest hashed something other than the committed fixture: {guest_source} "
            f"vs {expected}"
        )

    document = receipt(image, analyzer, cases, guest_source, after, cap)
    output_path.write_text(json.dumps(document, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"status": document["status"], "cases": len(cases)}))
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except CaptureError as failure:
        print(f"capture refused: {failure}", file=sys.stderr)
        sys.exit(1)
