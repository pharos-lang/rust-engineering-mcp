#!/usr/bin/env python3
"""Explicit M6 analyzer-runtime calibration; no provisioning, pulls or downloads."""
import datetime
import hashlib
import json
import os
import pathlib
import platform
import re
import signal
import subprocess
import sys
import time

ROOT = pathlib.Path(__file__).resolve().parents[1]
# The native tests own `target/m6-calibration`: `analyzer_native::publish()`
# writes `cut-<cut>.json` there and rebuilds `receipt.json` from them after every
# cut. This gate therefore writes its own receipt somewhere else. Overwriting the
# product's calibration receipt with the gate's would destroy the very evidence
# the gate exists to collect.
# A constant, deliberately: this path is created and written to, and a gate
# whose output location comes from the environment can be pointed anywhere.
OUTPUT = ROOT / "target/m6-runtime-gate"
NATIVE_OUTPUT = ROOT / "target/m6-calibration"
# The three places the admitted M6 digest is written down. `analyzer_gateway.rs`
# is the source of truth and the other two are cross-checks, because the product
# admits an image by comparing against the `APPROVED_M6_IMAGE` constant compiled
# into the gateway: the ADR is the decision that authorised the digest and
# `M6/provisioning.json` is the receipt of the build that produced it, but
# neither is consulted at runtime. Calibrating an image the code does not admit
# is precisely the defect this stage exists to prevent, so a disagreement between
# the three is a loud refusal, never a preference for one of them.
PORT_SOURCE = ROOT / "crates/execution-adapter/src/analyzer_gateway.rs"
ADMISSION_DECISION = ROOT / "docs/adr/ADR-085-m6-runtime-admission.md"
PROVISIONING_RECEIPT = ROOT / "docs/validation/M6/provisioning.json"
NATIVE_SOURCE = ROOT / "crates/execution-adapter/src/analyzer_native.rs"
PACKAGE = "rust-engineering-execution"
# `analyzer_native` is a `#[cfg(test)] mod` of the execution adapter's lib, so the
# harness selection is the module path plus the function name. No cargo feature is
# required: every cut drives the ordinary gateway API.
MODULE = NATIVE_SOURCE.stem
# The refusal cut must run before the positives: it is the cheapest selection in
# the file (it opens the M5 image and expects `Unavailable` before any container
# is created), and it is the one that proves the admission list itself. A stale
# digest, a mis-set RUST_MCP_TEST_IMAGE or a host without the M5 image therefore
# fails in seconds instead of after the long real sessions.
ADMISSION_TEST = "m6_analyzer_refuses_every_runtime_but_the_m6_image"
DIGEST = r"sha256:[0-9a-f]{64}"
NATIVE_RECEIPT_SCHEMA = "rust-engineering-mcp.m6-calibration.v1"
# Statuses a published cut may carry. A cut that could not be made to pass is
# `not_run` with its reason in the cut document; it is never silently a pass.
ALLOWED_CUT_STATUS = {"pass", "not_run"}
# The same default the native calibration compiles in; both read
# RUST_MCP_TEST_DOCKER when the client lives somewhere else.
DEFAULT_DOCKER = "/Applications/Docker.app/Contents/Resources/bin/docker"
STEP_TIMEOUT_VARIABLE = "RUST_MCP_M6_STEP_TIMEOUT_S"
DEFAULT_STEP_TIMEOUT_S = 900


def step_timeout_s(environment=None):
    """The per-selection wall bound, refusing anything that is not a positive integer.

    A stalled selection must become a recorded failure, never an unattended gate
    hang. The default is deliberately far above any legitimate M6 step (the
    ADR-084 total per call is 180 s) and is not a substitute for the in-gateway
    budgets. A malformed override is this gate's own refusal naming the
    variable, not a bare `ValueError` traceback from the conversion.
    """
    environment = os.environ if environment is None else environment
    raw = environment.get(STEP_TIMEOUT_VARIABLE)
    if raw is None:
        return DEFAULT_STEP_TIMEOUT_S
    try:
        seconds = int(raw)
    except ValueError:
        raise RuntimeError(
            f"{STEP_TIMEOUT_VARIABLE}={raw!r} is not an integer number of seconds"
        ) from None
    if seconds <= 0:
        raise RuntimeError(
            f"{STEP_TIMEOUT_VARIABLE}={raw!r} must be a positive number of seconds"
        )
    return seconds


def utc_now():
    return datetime.datetime.now(datetime.UTC).isoformat().replace("+00:00", "Z")


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def admitted_image():
    """The digest the product admits, refusing when the recorded copies differ."""
    match = re.search(
        rf'pub const APPROVED_M6_IMAGE: &str =\s*"({DIGEST})"\s*;', PORT_SOURCE.read_text()
    )
    if not match:
        raise RuntimeError(f"APPROVED_M6_IMAGE constant not found in {PORT_SOURCE}")
    image = match.group(1)
    # The ADR may name superseded digests in amendments, which are history and
    # not admission, so this reads the one labelled line that declares what is
    # admitted -- and requires exactly one of them, so a second declaration is a
    # refusal rather than a preference.
    decision = re.findall(
        rf"(?m)^\*\*Digest admitido:\*\* `({DIGEST})`\s*$",
        ADMISSION_DECISION.read_text(),
    )
    built = json.loads(PROVISIONING_RECEIPT.read_text()).get("image_id")
    if decision != [image] or built != image:
        raise RuntimeError(
            "admitted M6 image disagrees across its recorded sources; refusing to "
            f"calibrate an image the product does not admit: {PORT_SOURCE}={image} "
            f"{ADMISSION_DECISION}={decision} {PROVISIONING_RECEIPT}={built}"
        )
    return image


def pinned_analyzer_identity():
    """The version line and binary digest the gateway publishes, cross-checked.

    The constants are the product's; the provisioning receipt is the record of the
    build that produced the image. The native cut reads both out of the live
    guest, but a disagreement between these two host-side copies means the
    calibration would be measuring one thing and the product declaring another,
    so it is refused before the engine is touched.
    """
    source = PORT_SOURCE.read_text()
    version = re.search(
        r'pub\(super\) const ANALYZER_VERSION: &str = "([^"]+)"\s*;', source
    )
    binary = re.search(
        rf'pub\(super\) const ANALYZER_BINARY_SHA256: &str =\s*"({DIGEST})"\s*;', source
    )
    if not version or not binary:
        raise RuntimeError(f"analyzer identity constants not found in {PORT_SOURCE}")
    receipt = json.loads(PROVISIONING_RECEIPT.read_text())
    installed = receipt.get("installed", {}).get("components", [])
    built = next(
        (
            component.get("sha256")
            for component in installed
            if component.get("name") == "rust-analyzer"
        ),
        None,
    )
    if receipt.get("rust_analyzer_version") != version.group(1):
        raise RuntimeError(
            "the pinned analyzer version disagrees with the provisioning receipt: "
            f"{version.group(1)} vs {receipt.get('rust_analyzer_version')}"
        )
    if built is None or f"sha256:{built}" != binary.group(1):
        raise RuntimeError(
            "the pinned analyzer binary digest disagrees with the provisioning "
            f"receipt: {binary.group(1)} vs sha256:{built}"
        )
    return {"version": version.group(1), "binary_sha256": binary.group(1)}


def ignored_tests(path):
    """The `#[ignore]`d top-level `#[test]` functions declared by `path`, in order."""
    names = []
    saw_test = saw_ignore = False
    for line in path.read_text().splitlines():
        if line.startswith("#[test]"):
            saw_test, saw_ignore = True, False
            continue
        if saw_test and line.startswith("#[ignore"):
            saw_ignore = True
            continue
        if saw_test and line.startswith("#["):
            continue
        match = re.match(r"fn ([A-Za-z0-9_]+)\s*\(", line)
        if saw_test and match:
            if saw_ignore:
                names.append(match.group(1))
            saw_test = saw_ignore = False
            continue
        if line.strip():
            saw_test = saw_ignore = False
    return names


def declared_cuts(path):
    """The cut each ignored test publishes, as `{test name: cut name}`.

    Read from the source rather than restated here so the reconciliation below
    compares the gate against the file it runs, and a renamed cut is a refusal
    instead of a silently unchecked document.
    """
    cuts = {}
    current = None
    for line in path.read_text().splitlines():
        match = re.match(r"fn ([A-Za-z0-9_]+)\s*\(", line)
        if match:
            current = match.group(1)
            continue
        opened = re.search(r'Cut::open\("([a-z0-9-]+)"', line)
        if opened and current is not None and current not in cuts:
            cuts[current] = opened.group(1)
    return cuts


def clear_native_output():
    """Empties the cut documents the tests are about to republish.

    `analyzer_native::publish()` rebuilds the merged receipt from every
    `cut-*.json` present, while the source digests it records are recomputed at
    each publish. A document left by an earlier run would therefore be paired
    with today's sources and presented as this run's evidence.
    """
    removed = []
    for path in sorted(NATIVE_OUTPUT.glob("cut-*.json")) + [NATIVE_OUTPUT / "receipt.json"]:
        if path.is_file():
            path.unlink()
            removed.append(str(path.relative_to(ROOT)))
    return removed


def parse_utc(text):
    """A `YYYY-MM-DDTHH:MM:SSZ` stamp as an aware datetime, or `None`."""
    if not isinstance(text, str):
        return None
    try:
        return datetime.datetime.strptime(text, "%Y-%m-%dT%H:%M:%SZ").replace(
            tzinfo=datetime.UTC
        )
    except ValueError:
        return None


def stale_cut_documents(published, floor):
    """Cut documents that do not carry a `run_started_at` at or after `floor`.

    A document from an earlier run — or one with no run stamp at all — is not
    evidence about this run, whatever status it carries.
    """
    stale = {}
    for document in published.get("cuts", []):
        name = document.get("cut")
        stamp = parse_utc(document.get("run_started_at"))
        if stamp is None or stamp < floor:
            stale[name] = document.get("run_started_at")
    return stale


def docker_client(environment=None):
    """The Docker client, honouring the same override the Rust side reads.

    A host that installed the client elsewhere would otherwise degrade this
    evidence to `{"containers_error": ...}` while the calibration itself ran
    fine, which reads as a missing snapshot rather than as a wrong path.
    """
    environment = os.environ if environment is None else environment
    return environment.get("RUST_MCP_TEST_DOCKER", DEFAULT_DOCKER)


def owned_docker_state(socket_path):
    """Bounded, read-only residue evidence for a timed-out selection."""
    docker = docker_client()
    host = f"unix://{socket_path}"
    commands = {
        "containers": [docker, "--host", host, "ps", "-a", "--filter",
                       "label=org.rust-mcp.execution=true", "--format", "{{json .}}"],
        "volumes": [docker, "--host", host, "volume", "ls", "--filter",
                    "label=org.rust-mcp.execution=true", "--format", "{{json .}}"],
    }
    snapshot = {"captured_at": utc_now()}
    for kind, command in commands.items():
        try:
            output = subprocess.check_output(command, cwd=ROOT, text=True,
                                             stderr=subprocess.STDOUT, timeout=10)
            snapshot[kind] = [line for line in output.splitlines() if line]
        except (OSError, subprocess.SubprocessError) as error:
            snapshot[f"{kind}_error"] = type(error).__name__
    return snapshot


def native_receipt():
    """The receipt the tests publish, as it stands after a step.

    Read-only: the tests own `target/m6-calibration` and this gate never writes
    into it. `None` means no native receipt was present, or the document there is
    not the tests' own -- either way the gate records the absence instead of
    guessing.
    """
    published = NATIVE_OUTPUT / "receipt.json"
    try:
        document = json.loads(published.read_text())
    except (OSError, ValueError):
        return None
    if document.get("schema") != NATIVE_RECEIPT_SCHEMA:
        return None
    return document


def native_receipt_digest():
    return None if native_receipt() is None else sha256(NATIVE_OUTPUT / "receipt.json")


def main():
    if sys.platform != "darwin" or platform.machine() != "arm64":
        raise RuntimeError("M6 runtime is calibrated only on macOS ARM64/Docker Linux ARM64")
    if not os.environ.get("RUST_MCP_TEST_SOCKET"):
        raise RuntimeError("RUST_MCP_TEST_SOCKET required; no socket discovery or substitution")
    step_timeout = step_timeout_s()
    image = admitted_image()
    identity = pinned_analyzer_identity()
    allowed = {"HOME", "PATH", "TMPDIR", "CARGO_HOME", "RUSTUP_HOME", "SDKROOT",
               "DEVELOPER_DIR", "CARGO_TARGET_DIR", "RUST_MCP_TEST_SOCKET"}
    env = {key: value for key, value in os.environ.items() if key in allowed}
    env.update(CARGO_INCREMENTAL="0", CARGO_TERM_COLOR="never", RUST_MCP_TEST_IMAGE=image)
    cargo = pathlib.Path(subprocess.check_output(
        ["rustup", "which", "--toolchain", "1.98.1", "cargo"], env=env, text=True).strip())
    env["PATH"] = str(cargo.parent) + os.pathsep + env.get("PATH", "")
    env["RUSTC"] = str(cargo.with_name("rustc"))
    declared = ignored_tests(NATIVE_SOURCE)
    if ADMISSION_TEST not in declared:
        raise RuntimeError(
            f"{ADMISSION_TEST} is not an ignored test of {NATIVE_SOURCE}; the ordering "
            "this gate depends on no longer matches the source"
        )
    cuts = declared_cuts(NATIVE_SOURCE)
    missing_cuts = [name for name in declared if name not in cuts]
    if missing_cuts:
        raise RuntimeError(
            f"these ignored tests of {NATIVE_SOURCE} publish no cut document, so their verdict "
            f"could not be reconciled with the selections run: {missing_cuts}"
        )
    # Refusal first, then the positives in their declared order.
    order = [ADMISSION_TEST] + [name for name in declared if name != ADMISSION_TEST]
    tests = [f"{MODULE}::{name}" for name in order]
    expected_cuts = {cuts[name] for name in order}
    OUTPUT.mkdir(parents=True, exist_ok=True)
    # Whole seconds: cut documents stamp `run_started_at` to the second, so a
    # floor here keeps a cut starting inside this same second comparable.
    started_floor = datetime.datetime.now(datetime.UTC).replace(microsecond=0)
    receipt = {
        "schema": "rust-mcp-m6-runtime-v1",
        "status": "running",
        "started_at": utc_now(),
        "image_id": image,
        "analyzer_identity": identity,
        "step_timeout_s": step_timeout,
        "selections": tests,
        "expected_cuts": sorted(expected_cuts),
        "cleared_native_documents": clear_native_output(),
        "steps": [],
        "sources": [],
        "configuration_inputs": [],
    }
    for path in sorted((ROOT / "crates").rglob("*.rs")):
        receipt["sources"].append({"path": str(path.relative_to(ROOT)), "sha256": sha256(path)})
    configuration_paths = [
        ROOT / "Cargo.toml", ROOT / "Cargo.lock", ROOT / "rust-toolchain.toml",
        ROOT / "scripts/test-m6-runtime.py",
        ADMISSION_DECISION, PROVISIONING_RECEIPT,
        ROOT / "docs/adr/ADR-084-rust-analyzer-runtime-and-lsp-lifecycle.md",
        *sorted((ROOT / "crates/execution-adapter/src").glob("seccomp*.json")),
        *sorted(path for directory in ["valid-basic", "build-script", "workspace",
                                       "rust-runtime/m6"]
                for path in (ROOT / "fixtures" / directory).rglob("*")
                if path.is_file() and "target" not in path.parts),
    ]
    for path in configuration_paths:
        receipt["configuration_inputs"].append(
            {"path": str(path.relative_to(ROOT)), "sha256": sha256(path)}
        )

    def save():
        (OUTPUT / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")

    try:
        for number, selection in enumerate(tests):
            command = [str(cargo), "test", "--locked", "--offline", "-p", PACKAGE,
                       "--lib", selection, "--", "--exact", "--ignored", "--nocapture",
                       "--test-threads=1"]
            log = OUTPUT / f"{number}.log"
            print(f"M6 RUNTIME {selection}", flush=True)
            started = time.monotonic()
            timed_out = False
            # Its own session, so a stalled cargo/docker client tree is killed
            # whole instead of leaving orphans behind the recorded failure.
            with log.open("wb") as stream:
                process = subprocess.Popen(command, cwd=ROOT, env=env, stdout=stream,
                                           stderr=subprocess.STDOUT, start_new_session=True)
                try:
                    returncode = process.wait(timeout=step_timeout)
                except subprocess.TimeoutExpired:
                    timed_out = True
                    docker_before_kill = owned_docker_state(env["RUST_MCP_TEST_SOCKET"])
                    os.killpg(process.pid, signal.SIGKILL)
                    returncode = process.wait()
                    docker_after_kill = owned_docker_state(env["RUST_MCP_TEST_SOCKET"])
            output = log.read_text(errors="replace")
            # A filtered-out selection exits zero and proves nothing, so exactly
            # one executed case is required rather than merely a zero exit.
            passed = (not timed_out and returncode == 0
                      and "test result: ok. 1 passed; 0 failed; 0 ignored;" in output)
            step = {
                "selection": selection,
                "image_id": image,
                "command": command,
                "status": "passed" if passed else "failed",
                "exit_code": returncode,
                "timed_out": timed_out,
                "expected_executed": 1,
                "seconds": round(time.monotonic() - started, 3),
                "log_sha256": sha256(log),
                "native_receipt_sha256": native_receipt_digest(),
            }
            if timed_out:
                step["owned_docker_before_kill"] = docker_before_kill
                step["owned_docker_after_kill"] = docker_after_kill
            receipt["steps"].append(step)
            save()
            if timed_out:
                raise RuntimeError(
                    f"M6 test exceeded the {step_timeout}s step bound and was killed: {log}")
            if not passed:
                raise RuntimeError(f"M6 test failed or exactly one case did not execute: {log}")
        # The published calibration must describe the same image and must not
        # carry a cut whose status is neither a pass nor a declared `not_run`.
        published = native_receipt()
        if published is None:
            raise RuntimeError(
                f"no {NATIVE_RECEIPT_SCHEMA} receipt under {NATIVE_OUTPUT}; the cuts ran but "
                "published nothing the gate can read"
            )
        if published.get("image_id") != image:
            raise RuntimeError(
                f"the published calibration names {published.get('image_id')}, not {image}"
            )
        statuses = published.get("cut_status", {})
        unexpected = {
            cut: status for cut, status in statuses.items()
            if status not in ALLOWED_CUT_STATUS
        }
        if unexpected or not statuses:
            raise RuntimeError(f"published cut statuses are not all accounted for: {statuses}")
        # Exactly the cuts of the selections that ran: no more (a leftover
        # document from another run) and no fewer (a cut that published
        # nothing). A pass is only a pass for a cut this gate actually ran.
        if set(statuses) != expected_cuts:
            raise RuntimeError(
                "the published cuts are not the selections this gate ran: expected "
                f"{sorted(expected_cuts)}, published {sorted(statuses)}"
            )
        stale = stale_cut_documents(published, started_floor)
        if stale:
            raise RuntimeError(
                "these cut documents were not written by this run, so their verdicts are not "
                f"evidence about the sources hashed here: {stale}"
            )
        receipt["native_receipt"] = {
            "path": str((NATIVE_OUTPUT / "receipt.json").relative_to(ROOT)),
            "sha256": native_receipt_digest(),
            "cut_status": statuses,
            "analyzer_version": published.get("analyzer_version"),
            "analyzer_binary_sha256": published.get("analyzer_binary_sha256"),
            "config_digest": published.get("config_digest"),
        }
        schema = NATIVE_OUTPUT / "config-schema.json"
        if schema.is_file():
            receipt["native_receipt"]["config_schema_sha256"] = sha256(schema)
        receipt["status"] = "passed"
    except BaseException as error:
        receipt.update(status="failed", error=str(error))
        raise
    finally:
        receipt["finished_at"] = utc_now()
        save()
    # A vanished input is a changed input, not a traceback.
    for entry in receipt["sources"] + receipt["configuration_inputs"]:
        path = ROOT / entry["path"]
        if not path.is_file() or sha256(path) != entry["sha256"]:
            receipt.update(status="failed", error="source inputs changed during calibration")
            save()
            raise RuntimeError(receipt["error"])
    print(f"PASS M6 runtime: {OUTPUT / 'receipt.json'}", flush=True)


if __name__ == "__main__":
    if not __debug__:
        raise RuntimeError("Optimized Python mode is rejected")
    main()
