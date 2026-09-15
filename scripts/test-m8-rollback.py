#!/usr/bin/env python3
"""M8-03 D12 §6 rollback/upgrade oracle: two real binaries exercised once over
real state.

Builds `v0.3.0` (default; override with ``--old-tag``) in a `git worktree`
under ``target/m8-rollback/worktree-<tag>`` and the current tree into a
sibling target directory, both with ``cargo build --release --locked
--offline``: the committed lock file pins the same dependency versions the
tag was built with, so no network access is required. If the offline build
fails because a dependency is missing from the local registry cache, this
driver stops and reports it rather than fetching anything.

It then runs the D12 §6 scenarios over temporary state under
``target/m8-rollback/tmp`` (never a hardcoded ``/tmp`` path):

  (a) a committed ``analyzer_action_apply`` journal — a kind unknown to
      v0.3.0 — written by the current tree (via ``cargo test -p
      rust-engineering-project --test rollback_native -- --ignored``, see
      that file) makes ``v0.3.0``'s ``mutation list --json`` fail closed
      without touching the journal, with positive controls proving the kind
      is the cause (v0.8.0 lists the same journal; a sibling
      ``manifest_patch`` control journal of a kind v0.3.0 does know lists
      cleanly under v0.3.0; ``doctor --json`` under v0.8.0 reports
      ``mutation_journals.downgrade_blocked``);
  (b) a real, published M3 quality artifact (written through the native
      fixture's public-API store) and the catalog SQLite store, both written
      by the current tree, are read by ``v0.3.0`` (``quality-artifacts
      recover --json``, ``catalog status --json``);
  (c) a trust-bundle sequence floor advanced by the current tree
      (``catalog import`` of the fixture's sequence-2 bundle) makes
      ``v0.3.0`` reject the fixture's sequence-1 bundle (``CATALOG_ROLLBACK``)
      without losing the accepted state;
  (d) catalog state written by ``v0.3.0`` is read by the current tree
      (upgrade), and ``doctor --json`` validates it; an M2 journal or M3
      artifact written by v0.3.0 is out of reach of any CLI subcommand, so
      that direction is a declared gap, not an assertion.

A receipt is always written to ``docs/validation/M8/03-rollback.json``
(``--out`` overrides the path). A scenario that could not run at all is
marked ``unavailable`` with a reason, never ``passed``. Only the standard
library is used; Git and Cargo are invoked with fixed argument lists, never
through a shell.
"""
from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
WORK_ROOT = ROOT / "target" / "m8-rollback"
FIXTURES_DIR = ROOT / "fixtures" / "catalog"
DEFAULT_RECEIPT_PATH = ROOT / "docs" / "validation" / "M8" / "03-rollback.json"
NATIVE_STATE_ENV = "RUST_MCP_ROLLBACK_STATE_ROOT"
NATIVE_CONTROL_STATE_ENV = "RUST_MCP_ROLLBACK_CONTROL_STATE_ROOT"
NATIVE_PROJECT_ENV = "RUST_MCP_ROLLBACK_PROJECT_ROOT"
NATIVE_QUALITY_STATE_ENV = "RUST_MCP_ROLLBACK_QUALITY_STATE_ROOT"
BUILD_TIMEOUT_SECONDS = 2400
RUN_TIMEOUT_SECONDS = 120
EXCERPT_BYTES = 1000

SCENARIO_DESCRIPTIONS = {
    "a": (
        "A committed analyzer_action_apply journal — a kind unknown to v0.3.0 "
        "— written by the current tree makes v0.3.0's `mutation list --json` "
        "fail closed, without touching the journal. Positive controls prove "
        "the cause is the kind, not some unrelated incompatibility: v0.8.0's "
        "own `mutation list` passes over the same state root, a sibling "
        "manifest_patch control journal (a kind v0.3.0 does know, written by "
        "the same public-API mechanism) lists cleanly under v0.3.0 in its own "
        "state root, and v0.8.0's `doctor --json` reports "
        "mutation_journals.downgrade_blocked=true for the state root holding "
        "the unknown-kind journal."
    ),
    "b": (
        "M3 quality-artifact state (a real, published artifact written "
        "through the native fixture's public-API store, not this CLI) and "
        "the catalog SQLite store, both written by the current tree, are "
        "read by v0.3.0 (`quality-artifacts recover --json`, `catalog status "
        "--json`); recover must report validated>=1 and quarantined==0 under "
        "both binaries."
    ),
    "c": (
        "A trust-bundle sequence floor advanced by the current tree makes "
        "v0.3.0 reject an older-sequence bundle (floor never regresses) "
        "without losing the accepted state."
    ),
    "d": (
        "Catalog state written by v0.3.0 is read by the current tree "
        "(upgrade), and `doctor --json` validates it. Neither binary can "
        "write an M2 mutation journal or an M3 quality artifact through a "
        "CLI subcommand alone (both need a live MCP session), so v0.3.0 "
        "writing either is declared as a gap in `gaps` below, not asserted."
    ),
}


class DriverError(Exception):
    """Stops the driver before an effect the plan forbids, such as a network fetch."""


def utc_now() -> str:
    return datetime.datetime.now(datetime.UTC).isoformat().replace("+00:00", "Z")


def git(*args: str) -> str:
    return subprocess.run(
        ["git", *args], cwd=ROOT, check=True, capture_output=True, text=True
    ).stdout


def head_commit() -> str:
    return git("rev-parse", "HEAD").strip()


def tree_is_dirty() -> bool:
    return bool(git("status", "--porcelain").strip())


def resolve_tag_commit(tag: str) -> str:
    result = subprocess.run(
        ["git", "rev-parse", "--verify", "--end-of-options", f"{tag}^{{commit}}"],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise DriverError(f"tag {tag!r} does not resolve to a commit: {result.stderr.strip()}")
    return result.stdout.strip()


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return f"sha256:{digest.hexdigest()}"


def snapshot_dir(path: Path) -> dict[str, str]:
    """Relative path -> sha256, used to prove a read-only command touched nothing."""
    snapshot: dict[str, str] = {}
    if not path.is_dir():
        return snapshot
    for child in sorted(path.rglob("*")):
        if child.is_file():
            snapshot[str(child.relative_to(path))] = sha256_file(child)
    return snapshot


def run(cmd: list[str], cwd: Path | None = None, env: dict | None = None,
        timeout: int = RUN_TIMEOUT_SECONDS) -> subprocess.CompletedProcess:
    return subprocess.run(
        [str(part) for part in cmd],
        cwd=str(cwd) if cwd is not None else None,
        env=env,
        capture_output=True,
        text=True,
        timeout=timeout,
    )


def json_or_none(text: str):
    """The parsed JSON object, or ``None`` for invalid JSON *or* valid JSON
    that isn't an object (a list, a number, ...) — every caller immediately
    does ``.get(...)`` on the result, which raises ``AttributeError`` on
    anything else."""
    try:
        payload = json.loads(text)
    except (json.JSONDecodeError, ValueError):
        return None
    return payload if isinstance(payload, dict) else None


def excerpt(text: str, limit: int = EXCERPT_BYTES) -> str:
    if len(text) <= limit:
        return text
    half = limit // 2
    return f"{text[:half]}...(truncated)...{text[-half:]}"


def step_record(cmd: list[str], result: subprocess.CompletedProcess) -> dict:
    entry = {
        "command": [str(part) for part in cmd],
        "exit_code": result.returncode,
        "stdout_excerpt": excerpt(result.stdout or ""),
    }
    if result.stderr:
        entry["stderr_excerpt"] = excerpt(result.stderr)
    return entry


def scenario_result(
    scenario_id: str,
    status: str,
    steps: list[dict],
    reason: str | None = None,
    gaps: list[str] | None = None,
) -> dict:
    entry = {
        "id": scenario_id,
        "description": SCENARIO_DESCRIPTIONS[scenario_id],
        "status": status,
        "steps": steps,
    }
    if reason is not None:
        entry["reason"] = reason
    if gaps:
        entry["gaps"] = gaps
    return entry


def ensure_tmp_root() -> Path:
    tmp_root = WORK_ROOT / "tmp"
    tmp_root.mkdir(parents=True, exist_ok=True)
    return tmp_root


def fresh_tmp_dir(label: str) -> Path:
    return Path(tempfile.mkdtemp(dir=str(ensure_tmp_root()), prefix=f"{label}-"))


def worktree_head(worktree: Path) -> str:
    result = subprocess.run(
        ["git", "-C", str(worktree), "rev-parse", "--verify", "--end-of-options", "HEAD"],
        capture_output=True, text=True,
    )
    if result.returncode != 0:
        raise DriverError(f"git rev-parse HEAD in {worktree} failed: {result.stderr.strip()}")
    return result.stdout.strip()


def worktree_is_clean(worktree: Path) -> bool:
    result = subprocess.run(
        ["git", "-C", str(worktree), "status", "--porcelain"],
        capture_output=True, text=True,
    )
    if result.returncode != 0:
        raise DriverError(f"git status in {worktree} failed: {result.stderr.strip()}")
    return result.stdout.strip() == ""


def ensure_worktree(tag: str, commit: str) -> tuple[Path, bool]:
    """Returns the worktree path and whether it is clean and at ``commit``.

    A worktree this driver creates fresh is always clean and at ``commit``
    (Git just checked it out `--detach`); one a prior run left registered is
    reused only after both are verified, never assumed.
    """
    worktree = WORK_ROOT / f"worktree-{tag}"
    listing = git("worktree", "list", "--porcelain")
    registered = any(
        block.splitlines()[0] == f"worktree {worktree}"
        for block in listing.split("\n\n")
        if block.splitlines()
    )
    if registered:
        head = worktree_head(worktree)
        if head != commit:
            raise DriverError(
                f"worktree {worktree} is registered at {head}, expected {commit}; "
                "remove it with `git worktree remove` before retrying"
            )
        if not worktree_is_clean(worktree):
            raise DriverError(
                f"worktree {worktree} has local changes; a v0.3.0 binary built from it "
                "would not be the real v0.3.0. Run `git -C "
                f"{worktree} status` and clean it, or `git worktree remove` it, before retrying"
            )
        return worktree, True
    if worktree.exists():
        # A stale directory left over from an interrupted prior run, not a
        # registered worktree; remove it before asking Git to create one.
        shutil.rmtree(worktree)
    WORK_ROOT.mkdir(parents=True, exist_ok=True)
    result = subprocess.run(
        ["git", "worktree", "add", "--detach", str(worktree), "--end-of-options", commit],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise DriverError(f"git worktree add failed: {result.stderr.strip()}")
    return worktree, True


def build_binary(build_cwd: Path, target_dir: Path) -> Path:
    target_dir.mkdir(parents=True, exist_ok=True)
    result = subprocess.run(
        [
            "cargo", "build", "--release", "--locked", "--offline",
            "-p", "rust-engineering-mcp",
            "--target-dir", str(target_dir),
        ],
        cwd=str(build_cwd),
        capture_output=True,
        text=True,
        timeout=BUILD_TIMEOUT_SECONDS,
    )
    if result.returncode != 0:
        raise DriverError(
            f"cargo build --offline failed in {build_cwd}; stopping per D12 §6 "
            "instead of fetching dependencies over the network.\n"
            f"stdout(tail): {excerpt(result.stdout, 4000)}\n"
            f"stderr(tail): {excerpt(result.stderr, 4000)}"
        )
    binary = target_dir / "release" / "rust-engineering-mcp"
    if not binary.is_file():
        raise DriverError(f"expected a release binary at {binary}, none was produced")
    return binary


def read_version(binary: Path) -> dict:
    result = run([binary, "version", "--json"])
    if result.returncode != 0:
        raise DriverError(f"{binary} version --json failed: {result.stderr.strip()}")
    payload = json_or_none(result.stdout)
    if payload is None:
        raise DriverError(f"{binary} version --json did not print JSON: {result.stdout!r}")
    return payload


MUTATION_JOURNAL_FIXTURE_TEST = (
    "leaves_a_committed_journal_of_a_kind_unknown_to_v0_3_0_and_a_known_control_journal"
)
QUALITY_ARTIFACT_FIXTURE_TEST = (
    "quality_artifact_fixture::leaves_a_validated_m3_quality_artifact_for_an_older_binary_to_read"
)


def run_native_fixture(test_name: str, env: dict) -> tuple[list, subprocess.CompletedProcess]:
    cmd = [
        "cargo", "test", "-p", "rust-engineering-project", "--locked", "--offline",
        "--test", "rollback_native", "--", "--ignored", "--exact", test_name,
    ]
    return cmd, run(cmd, cwd=ROOT, env=env, timeout=BUILD_TIMEOUT_SECONDS)


def scenario_a(old_binary: Path, head_binary: Path) -> dict:
    tmp = fresh_tmp_dir("scenario-a")
    state_root = tmp / "state"
    control_state_root = tmp / "control-state"
    project_root = tmp / "project"
    env = dict(os.environ)
    env[NATIVE_STATE_ENV] = str(state_root)
    env[NATIVE_CONTROL_STATE_ENV] = str(control_state_root)
    env[NATIVE_PROJECT_ENV] = str(project_root)
    fixture_cmd, fixture_result = run_native_fixture(MUTATION_JOURNAL_FIXTURE_TEST, env)
    steps = [step_record(fixture_cmd, fixture_result)]
    if fixture_result.returncode != 0:
        return scenario_result(
            "a", "failed", steps,
            reason="the native fixture (tests/rollback_native.rs) did not leave the journals",
        )

    mutations_dir = state_root / "rust-mcp-mutations-v1"
    before = snapshot_dir(mutations_dir)

    # v0.3.0 must fail closed on the unknown-kind journal, without touching it.
    list_cmd = [old_binary, "mutation", "list", "--state-root", state_root, "--json"]
    list_result = run(list_cmd)
    steps.append(step_record(list_cmd, list_result))
    after = snapshot_dir(mutations_dir)
    payload = json_or_none(list_result.stdout) or {}
    untouched = before == after and bool(before)
    failed_closed = list_result.returncode != 0 and payload.get("status") == "blocked"
    right_error = payload.get("error_code") == "recovery_required"

    # Positive control (i): v0.8.0's own `mutation list` passes over the same
    # state root, proving the journal is well-formed and readable — only the
    # older binary's narrower kind vocabulary rejects it.
    head_list_cmd = [head_binary, "mutation", "list", "--state-root", state_root, "--json"]
    head_list_result = run(head_list_cmd)
    steps.append(step_record(head_list_cmd, head_list_result))
    head_payload = json_or_none(head_list_result.stdout) or {}
    head_records = head_payload.get("records")
    head_ok = (
        head_list_result.returncode == 0
        and head_payload.get("status") == "passed"
        and isinstance(head_records, list)
        and len(head_records) >= 1
    )

    # Positive control (ii): a sibling `manifest_patch` control journal — a
    # kind v0.3.0 does know, written by the same public-API mechanism as the
    # unknown-kind journal above — lists cleanly under v0.3.0 from its own,
    # disjoint state root. This is what proves the (a) rejection above is
    # caused by the operation kind specifically, and not by some unrelated
    # incompatibility (a lock, a permission, a malformed envelope) that would
    # produce the same `recovery_required` regardless of kind.
    control_list_cmd = [
        old_binary, "mutation", "list", "--state-root", control_state_root, "--json",
    ]
    control_list_result = run(control_list_cmd)
    steps.append(step_record(control_list_cmd, control_list_result))
    control_payload = json_or_none(control_list_result.stdout) or {}
    control_records = control_payload.get("records")
    control_ok = (
        control_list_result.returncode == 0
        and control_payload.get("status") == "passed"
        and isinstance(control_records, list)
        and len(control_records) == 1
    )

    # Positive control (iii): `doctor --json` under v0.8.0 flags the state
    # root holding the unknown-kind journal as blocking a downgrade (D-1),
    # which needs `doctor` to accept a bare `--state-root` without the full
    # serve-style host tuple (D-5). If `doctor` fails or does not return
    # `mutation_journals.downgrade_blocked`, that is a gap in this control,
    # not a pass -- consistent with scenario (b)'s quality control below.
    doctor_cmd = [head_binary, "doctor", "--json", "--state-root", state_root]
    doctor_result = run(doctor_cmd)
    steps.append(step_record(doctor_cmd, doctor_result))
    doctor_payload = json_or_none(doctor_result.stdout)
    journals = (
        doctor_payload.get("mutation_journals")
        if isinstance(doctor_payload, dict)
        else None
    )
    gaps = []
    if doctor_result.returncode == 0 and isinstance(journals, dict) and "downgrade_blocked" in journals:
        doctor_ok = journals["downgrade_blocked"] is True
    else:
        doctor_ok = False
        gaps.append(
            "R-1(iii): `doctor --json --state-root` did not return "
            "mutation_journals.downgrade_blocked (D-1 and/or D-5 not yet landed); "
            "this positive control is unverified, not passed."
        )

    ok = untouched and failed_closed and right_error and head_ok and control_ok and doctor_ok
    reason = None
    if not ok:
        reason = (
            f"v0.3.0 mutation list did not fail closed as expected "
            f"(exit={list_result.returncode}, error_code={payload.get('error_code')!r}, "
            f"journal_untouched={untouched}); head_lists_the_journal={head_ok}; "
            f"control_journal_lists_under_v0_3_0={control_ok}; "
            f"doctor_downgrade_blocked_ok={doctor_ok}"
        )
    return scenario_result("a", "passed" if ok else "failed", steps, reason, gaps=gaps)


def recovery_confirmed(payload: dict) -> bool:
    """True only if `quality-artifacts recover --json` reports a real,
    validated artifact and nothing quarantined — never on an empty store,
    where `validated` and `quarantined` are both trivially `0`."""
    data = payload.get("data")
    return (
        payload.get("status") == "passed"
        and isinstance(data, dict)
        and data.get("validated", 0) >= 1
        and data.get("quarantined", 0) == 0
    )


def prepare_catalog_dirs(root: Path) -> tuple[Path, Path]:
    store = root / "store"
    store.mkdir(parents=True)
    os.chmod(store, 0o700)
    trust = root / "trust.json"
    shutil.copyfile(FIXTURES_DIR / "fixture-trust.json", trust)
    os.chmod(trust, 0o600)
    return store, trust


def scenario_bc(old_binary: Path, head_binary: Path) -> tuple[dict, dict]:
    fixture_one = FIXTURES_DIR / "fixture-1.tar.zst"
    fixture_two = FIXTURES_DIR / "fixture-2.tar.zst"
    if not fixture_one.is_file() or not fixture_two.is_file():
        reason = f"missing catalog fixtures under {FIXTURES_DIR}"
        return (
            scenario_result("b", "unavailable", [], reason),
            scenario_result("c", "unavailable", [], reason),
        )

    tmp = fresh_tmp_dir("scenario-bc")
    store, trust = prepare_catalog_dirs(tmp)

    import_cmd = [
        head_binary, "catalog", "import", fixture_two,
        "--store", store, "--trust", trust, "--json",
    ]
    import_result = run(import_cmd)
    import_steps = [step_record(import_cmd, import_result)]
    import_payload = json_or_none(import_result.stdout) or {}
    if import_result.returncode != 0 or import_payload.get("catalog", {}).get("sequence") != 2:
        reason = "the current tree could not import the sequence-2 fixture bundle"
        return (
            scenario_result("b", "unavailable", import_steps, reason),
            scenario_result("c", "unavailable", import_steps, reason),
        )

    status_cmd = [old_binary, "catalog", "status", "--store", store, "--trust", trust, "--json"]
    status_result = run(status_cmd)
    b_steps = import_steps + [step_record(status_cmd, status_result)]
    status_payload = json_or_none(status_result.stdout) or {}
    b_catalog_ok = (
        status_result.returncode == 0
        and status_payload.get("catalog", {}).get("sequence") == 2
    )

    # A real, published M3 artifact (R-2): `quality-artifacts recover` alone
    # never creates one, it only reconciles an existing store, so an empty
    # state root would trivially report `validated=0, quarantined=0` and
    # prove nothing. The native fixture publishes one through the store's
    # public API first.
    qa_tmp = fresh_tmp_dir("scenario-b-quality-artifacts")
    qa_state_root = qa_tmp / "state"
    qa_state_root.mkdir(parents=True)
    qa_env = dict(os.environ)
    qa_env[NATIVE_QUALITY_STATE_ENV] = str(qa_state_root)
    qa_fixture_cmd, qa_fixture_result = run_native_fixture(QUALITY_ARTIFACT_FIXTURE_TEST, qa_env)
    b_steps += [step_record(qa_fixture_cmd, qa_fixture_result)]
    b_gaps: list[str] = []
    qa_head_payload: dict = {}
    qa_old_payload: dict = {}
    if "running 0 tests" in (qa_fixture_result.stdout or ""):
        # The M3 native store is macOS+aarch64 only
        # (`crates/project-adapter/src/quality_artifact_store.rs`); on any
        # other host the fixture matches no test and this driver exits 0
        # having done nothing. Declare the gap rather than claim `passed`.
        b_quality_ok = False
        b_gaps.append(
            "R-2: the M3 native fixture is macOS/aarch64-only and did not run on this "
            "host, so quality_artifacts_ok could not be confirmed here."
        )
    elif qa_fixture_result.returncode != 0:
        b_quality_ok = False
    else:
        qa_head_cmd = [
            head_binary, "quality-artifacts", "recover", "--state-root", qa_state_root, "--json",
        ]
        qa_head_result = run(qa_head_cmd)
        qa_old_cmd = [
            old_binary, "quality-artifacts", "recover", "--state-root", qa_state_root, "--json",
        ]
        qa_old_result = run(qa_old_cmd)
        b_steps += [step_record(qa_head_cmd, qa_head_result), step_record(qa_old_cmd, qa_old_result)]
        qa_head_payload = json_or_none(qa_head_result.stdout) or {}
        qa_old_payload = json_or_none(qa_old_result.stdout) or {}
        b_quality_ok = (
            qa_head_result.returncode == 0
            and recovery_confirmed(qa_head_payload)
            and qa_old_result.returncode == 0
            and recovery_confirmed(qa_old_payload)
        )

    b_ok = b_catalog_ok and b_quality_ok
    b_reason = None
    if not b_ok:
        b_reason = (
            f"catalog_status_ok={b_catalog_ok} (sequence={status_payload.get('catalog', {}).get('sequence')}), "
            f"quality_artifacts_ok={b_quality_ok} "
            f"(head_data={qa_head_payload.get('data')!r}, old_data={qa_old_payload.get('data')!r})"
        )
    b = scenario_result("b", "passed" if b_ok else "failed", b_steps, b_reason, gaps=b_gaps)

    rollback_cmd = [old_binary, "catalog", "import", fixture_one, "--store", store, "--trust", trust, "--json"]
    rollback_result = run(rollback_cmd)
    rollback_payload = json_or_none(rollback_result.stdout) or {}
    status2_cmd = [old_binary, "catalog", "status", "--store", store, "--trust", trust, "--json"]
    status2_result = run(status2_cmd)
    status2_payload = json_or_none(status2_result.stdout) or {}
    c_steps = [
        step_record(rollback_cmd, rollback_result),
        step_record(status2_cmd, status2_result),
    ]
    c_ok = (
        rollback_result.returncode != 0
        and rollback_payload.get("error_code") == "CATALOG_ROLLBACK"
        and status2_result.returncode == 0
        and status2_payload.get("catalog", {}).get("sequence") == 2
        and status2_payload.get("catalog", {}).get("floor_sequence") == 2
    )
    c_reason = None
    if not c_ok:
        c_reason = (
            f"rollback_exit={rollback_result.returncode}, "
            f"rollback_error_code={rollback_payload.get('error_code')!r}, "
            f"post_sequence={status2_payload.get('catalog', {}).get('sequence')}, "
            f"post_floor={status2_payload.get('catalog', {}).get('floor_sequence')}"
        )
    c = scenario_result("c", "passed" if c_ok else "failed", c_steps, c_reason)
    return b, c


def scenario_d(old_binary: Path, head_binary: Path) -> dict:
    fixture_one = FIXTURES_DIR / "fixture-1.tar.zst"
    if not fixture_one.is_file():
        return scenario_result("d", "unavailable", [], f"missing {fixture_one}")

    tmp = fresh_tmp_dir("scenario-d")
    qa_state_root = tmp / "quality-state"
    qa_state_root.mkdir(parents=True)
    store, trust = prepare_catalog_dirs(tmp)

    # Neither binary can write an M2 mutation journal or an M3 quality
    # artifact through a CLI subcommand alone — both are only ever produced
    # over a live MCP session (`serve`), and this driver cannot compile a
    # test against the v0.3.0 binary's internals to reach that API (R-2,
    # R-7). These two `recover`/`list` calls against an empty state root are
    # kept as a structural smoke check only (the CLI itself must still run
    # cleanly against an empty store); they do not, and cannot, prove that
    # v0.3.0-written M2/M3 state upgrades cleanly. That gap is declared below
    # instead of implied by a `passed` scenario.
    qa_init_cmd = [old_binary, "quality-artifacts", "recover", "--state-root", qa_state_root, "--json"]
    qa_init_result = run(qa_init_cmd)
    import_cmd = [old_binary, "catalog", "import", fixture_one, "--store", store, "--trust", trust, "--json"]
    import_result = run(import_cmd)
    qa_upgrade_cmd = [head_binary, "quality-artifacts", "recover", "--state-root", qa_state_root, "--json"]
    qa_upgrade_result = run(qa_upgrade_cmd)
    status_cmd = [head_binary, "catalog", "status", "--store", store, "--trust", trust, "--json"]
    status_result = run(status_cmd)
    doctor_cmd = [
        head_binary, "doctor",
        "--catalog-store", store, "--catalog-trust", trust,
        "--json",
    ]
    doctor_result = run(doctor_cmd)

    steps = [
        step_record(qa_init_cmd, qa_init_result),
        step_record(import_cmd, import_result),
        step_record(qa_upgrade_cmd, qa_upgrade_result),
        step_record(status_cmd, status_result),
        step_record(doctor_cmd, doctor_result),
    ]
    status_payload = json_or_none(status_result.stdout) or {}
    doctor_payload = json_or_none(doctor_result.stdout) or {}
    ok = (
        qa_init_result.returncode == 0
        and import_result.returncode == 0
        and qa_upgrade_result.returncode == 0
        and status_result.returncode == 0
        and status_payload.get("catalog", {}).get("sequence") == 1
        and doctor_result.returncode == 0
        and doctor_payload.get("status") != "failed"
    )
    reason = None
    if not ok:
        reason = (
            f"qa_init={qa_init_result.returncode}, import={import_result.returncode}, "
            f"qa_upgrade={qa_upgrade_result.returncode}, status={status_result.returncode} "
            f"(sequence={status_payload.get('catalog', {}).get('sequence')}), "
            f"doctor_exit={doctor_result.returncode}, doctor_status={doctor_payload.get('status')!r}"
        )
    gaps = [
        "R-7: v0.3.0 cannot write an M2 mutation journal through a CLI subcommand (only a "
        "live MCP session can), so (d) does not exercise an upgrade of a v0.3.0-written "
        "journal. Format compatibility of the legacy v1 journal encoding is instead covered "
        "by the unit tests `legacy_v1_receipt_is_read_only_and_explicit_recovery_migrates_to_v2` "
        "and `terminal_legacy_v1_replay_migrates_only_after_exact_binding` in "
        "crates/project-adapter/tests/support/native_mutation.rs.",
        "R-2: v0.3.0 cannot publish an M3 quality artifact through a CLI subcommand either, "
        "so `quality-artifacts recover` above only smoke-tests an empty store in both "
        "directions; it does not prove a v0.3.0-written artifact upgrades cleanly.",
    ]
    return scenario_result("d", "passed" if ok else "failed", steps, reason, gaps=gaps)


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--old-tag", default="v0.3.0", help="tag to build as the old binary")
    parser.add_argument("--out", default=str(DEFAULT_RECEIPT_PATH), help="receipt output path")
    args = parser.parse_args(argv)

    generated_utc = utc_now()
    head = head_commit()
    dirty = tree_is_dirty()

    context: dict = {
        "old_commit": None,
        "old_binary": None,
        "head_binary": None,
        "old_version": None,
        "head_version": None,
        "old_tree_dirty": None,
    }
    scenarios: list[dict] = []
    driver_error: str | None = None

    try:
        context["old_commit"] = resolve_tag_commit(args.old_tag)
        worktree, old_tree_clean = ensure_worktree(args.old_tag, context["old_commit"])
        context["old_tree_dirty"] = not old_tree_clean
        old_binary = build_binary(worktree, WORK_ROOT / f"target-{args.old_tag}")
        head_binary = build_binary(ROOT, WORK_ROOT / "target-head")
        context["old_binary"] = old_binary
        context["head_binary"] = head_binary

        old_version = read_version(old_binary)
        head_version = read_version(head_binary)
        context["old_version"] = old_version.get("version")
        context["head_version"] = head_version.get("version")
        if old_version.get("version") != args.old_tag.lstrip("v"):
            raise DriverError(
                f"old binary reports version {old_version.get('version')!r}, "
                f"expected {args.old_tag.lstrip('v')!r}"
            )
        if head_version.get("version") != "0.8.0":
            raise DriverError(
                f"current-tree binary reports version {head_version.get('version')!r}, expected '0.8.0'"
            )

        scenarios.append(scenario_a(old_binary, head_binary))
        b, c = scenario_bc(old_binary, head_binary)
        scenarios.extend([b, c])
        scenarios.append(scenario_d(old_binary, head_binary))
    except DriverError as error:
        driver_error = str(error)
    except subprocess.TimeoutExpired as error:
        driver_error = f"timed out: {error}"

    by_id = {scenario_id: None for scenario_id in SCENARIO_DESCRIPTIONS}
    for entry in scenarios:
        by_id[entry["id"]] = entry
    for scenario_id, entry in by_id.items():
        if entry is None:
            by_id[scenario_id] = scenario_result(
                scenario_id, "unavailable", [],
                driver_error or "did not run",
            )
    ordered = [by_id[scenario_id] for scenario_id in ("a", "b", "c", "d")]
    overall_status = "passed" if all(s["status"] == "passed" for s in ordered) else "failed"

    receipt = {
        "format_version": 1,
        "generated_utc": generated_utc,
        "head_commit": head,
        "head_tree_dirty": dirty,
        "old_tag": args.old_tag,
        "old_commit": context["old_commit"],
        "binaries": {
            "old": {
                "path": str(context["old_binary"]) if context["old_binary"] else None,
                "sha256": sha256_file(context["old_binary"]) if context["old_binary"] else None,
                "version": context["old_version"],
                "tree_dirty": context["old_tree_dirty"],
            },
            "head": {
                "path": str(context["head_binary"]) if context["head_binary"] else None,
                "sha256": sha256_file(context["head_binary"]) if context["head_binary"] else None,
                "version": context["head_version"],
                "tree_dirty": dirty,
            },
        },
        "driver_error": driver_error,
        "scenarios": ordered,
        "status": overall_status,
    }

    out_path = Path(args.out)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(receipt, indent=2, sort_keys=False) + "\n", encoding="utf-8")
    print(json.dumps({"status": overall_status, "out": str(out_path), "driver_error": driver_error}, indent=2))
    return 0 if overall_status == "passed" else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
