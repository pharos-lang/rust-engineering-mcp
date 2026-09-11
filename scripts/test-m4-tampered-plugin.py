#!/usr/bin/env python3
"""Explicit M4 altered-plugin admission probe; never pulls or starts a container.

This script creates an untagged private image by replacing cargo-deny in a
stopped container layer. It then proves that the real server rejects the new
image identity while the exact approved M4 image remains accepted. The probe
does not claim protection from a hostile Docker daemon or privileged host.
"""
from __future__ import annotations

import argparse
import datetime
import hashlib
import io
import json
import os
import pathlib
import platform
import re
import resource
import shutil
import signal
import stat
import subprocess
import sys
import tarfile
import tempfile
import time
from typing import Any


ROOT = pathlib.Path(__file__).resolve().parents[1]
DOCKER = pathlib.Path("/Applications/Docker.app/Contents/Resources/bin/docker")
IMAGE_CONFIG = ROOT / "docs/validation/M4/runtime-image.json"
APPROVED_IMAGE = (
    "sha256:25ed3626e710081a571a86a29521eaf2e890e796afd422ba5e409e0ce1891635"
)
APPROVED_CARGO_DENY_SHA256 = (
    "e9bcd2f489b8dd22cc3f3fc3452cfa0483cf8b9236a5e2787c54f864d4e77715"
)
PLUGIN_PATH = "/opt/security/bin/cargo-deny"
FIXTURE_BYTES = b"#!/nonexistent/rust-mcp-inert-tampered-plugin\nexit 97\n"
EXECUTION_LABEL = "org.rust-mcp.execution=true"
OWNERSHIP_LABEL = "org.rust-mcp.m4-tampered-plugin"
MAX_CAPTURE_BYTES = 1024 * 1024
MAX_PLUGIN_BYTES = 8 * 1024 * 1024
MAX_ARCHIVE_CAPTURE_BYTES = 16 * 1024 * 1024
COMMAND_TIMEOUT_SECONDS = 60
PRODUCT_TIMEOUT_SECONDS = 15
AMBIGUOUS_SETTLE_SECONDS = 15
USAGE_ERROR = b"Unsupported invocation. Use 'rust-engineering-mcp --help'.\n"
IMAGE_ID_PATTERN = re.compile(rb"(?m)^sha256:[0-9a-f]{64}\r?$")


def utc_now() -> str:
    return datetime.datetime.now(datetime.UTC).isoformat().replace("+00:00", "Z")


def digest_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def digest_file(path: pathlib.Path) -> str:
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def canonical_digest(value: object) -> str:
    encoded = json.dumps(
        value, sort_keys=True, separators=(",", ":"), ensure_ascii=False
    ).encode()
    return digest_bytes(encoded)


def private_tree(path: pathlib.Path) -> dict[str, str]:
    result: dict[str, str] = {}
    if not path.exists():
        return result
    for entry in sorted(path.rglob("*")):
        relative = entry.relative_to(path).as_posix()
        if entry.is_symlink():
            result[relative] = "symlink-rejected"
        elif entry.is_file():
            result[relative] = digest_file(entry)
        elif entry.is_dir():
            result[relative + "/"] = "directory"
        else:
            result[relative] = "special-rejected"
    return result


class Recorder:
    def __init__(self, receipt: dict[str, Any], socket: pathlib.Path) -> None:
        self.receipt = receipt
        self.socket = socket
        self.docker_prefix = [str(DOCKER), "--host", f"unix://{socket}"]

    def _safe_argv(self, argv: list[str]) -> list[str]:
        hidden = {str(self.socket), f"unix://{self.socket}"}
        return ["<docker-socket>" if item in hidden else item for item in argv]

    def run(
        self,
        argv: list[str],
        *,
        timeout: int = COMMAND_TIMEOUT_SECONDS,
        input_bytes: bytes | None = None,
        accepted: tuple[int, ...] = (0,),
        environment: dict[str, str] | None = None,
        output_limit_bytes: int = MAX_CAPTURE_BYTES,
    ) -> subprocess.CompletedProcess[bytes]:
        if (
            type(output_limit_bytes) is not int
            or output_limit_bytes < 1
            or output_limit_bytes > MAX_ARCHIVE_CAPTURE_BYTES
        ):
            raise ValueError("invalid internal output limit")
        started = time.monotonic()
        record: dict[str, Any] = {
            "argv": self._safe_argv(argv),
            "argv_sha256": canonical_digest(argv),
            "timeout_seconds": timeout,
            "output_limit_bytes": output_limit_bytes,
        }
        self.receipt["commands"].append(record)

        def limit_output_files() -> None:
            maximum = output_limit_bytes + 1
            resource.setrlimit(resource.RLIMIT_FSIZE, (maximum, maximum))

        interrupted: BaseException | None = None
        with (
            tempfile.TemporaryFile() as stdout_file,
            tempfile.TemporaryFile() as stderr_file,
        ):
            try:
                process = subprocess.Popen(  # Closed argv; never a shell.
                    argv,
                    stdin=subprocess.PIPE if input_bytes is not None else subprocess.DEVNULL,
                    stdout=stdout_file,
                    stderr=stderr_file,
                    env=environment,
                    start_new_session=True,
                    preexec_fn=limit_output_files,
                )
            except BaseException as error:
                record.update(
                    status="spawn_failed",
                    seconds=round(time.monotonic() - started, 3),
                    error_type=type(error).__name__,
                )
                raise
            timed_out = False
            try:
                process.communicate(input=input_bytes, timeout=timeout)
            except subprocess.TimeoutExpired:
                timed_out = True
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except OSError:
                    pass
                process.wait()
            except BaseException as error:
                interrupted = error
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except OSError:
                    pass
                process.wait()
            stdout_file.seek(0)
            stderr_file.seek(0)
            stdout = stdout_file.read(output_limit_bytes + 2)
            stderr = stderr_file.read(output_limit_bytes + 2)
        if interrupted is not None:
            record.update(
                status="interrupted",
                seconds=round(time.monotonic() - started, 3),
                exit_code=process.returncode,
                stdout_bytes=len(stdout),
                stdout_sha256=digest_bytes(stdout),
                stderr_bytes=len(stderr),
                stderr_sha256=digest_bytes(stderr),
                error_type=type(interrupted).__name__,
            )
            raise interrupted
        if timed_out:
            record.update(
                status="timed_out",
                seconds=round(time.monotonic() - started, 3),
                stdout_bytes=len(stdout),
                stdout_sha256=digest_bytes(stdout),
                stderr_bytes=len(stderr),
                stderr_sha256=digest_bytes(stderr),
            )
            raise RuntimeError("bounded subprocess timed out")
        record.update(
            status="exited",
            exit_code=process.returncode,
            seconds=round(time.monotonic() - started, 3),
            stdout_bytes=len(stdout),
            stdout_sha256=digest_bytes(stdout),
            stderr_bytes=len(stderr),
            stderr_sha256=digest_bytes(stderr),
        )
        if len(stdout) > output_limit_bytes or len(stderr) > output_limit_bytes:
            record["status"] = "output_limited"
            raise RuntimeError("subprocess output exceeded the evidence bound")
        if process.returncode not in accepted:
            record["status"] = "unexpected_exit"
            raise RuntimeError(
                f"command returned {process.returncode}, expected {accepted}"
            )
        return subprocess.CompletedProcess(argv, process.returncode, stdout, stderr)

    def docker(
        self,
        args: list[str],
        *,
        timeout: int = COMMAND_TIMEOUT_SECONDS,
        accepted: tuple[int, ...] = (0,),
        output_limit_bytes: int = MAX_CAPTURE_BYTES,
    ) -> subprocess.CompletedProcess[bytes]:
        environment = {
            "PATH": "/usr/bin:/bin",
            "LC_ALL": "C",
            "LANG": "C",
        }
        return self.run(
            [*self.docker_prefix, *args],
            timeout=timeout,
            accepted=accepted,
            environment=environment,
            output_limit_bytes=output_limit_bytes,
        )


def parse_json(raw: bytes, context: str) -> Any:
    try:
        return json.loads(raw)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise RuntimeError(f"invalid bounded JSON from {context}") from error


def parse_commit_identity(raw: bytes) -> str:
    identities = set(IMAGE_ID_PATTERN.findall(raw))
    if len(identities) != 1:
        raise RuntimeError("docker commit did not return one unique image identity")
    return identities.pop().decode("ascii")


def inspect_image(recorder: Recorder, image: str) -> dict[str, Any]:
    result = recorder.docker(["image", "inspect", image])
    rows = parse_json(result.stdout, "image inspect")
    if not isinstance(rows, list) or len(rows) != 1 or not isinstance(rows[0], dict):
        raise RuntimeError("image inspect returned an unexpected shape")
    return rows[0]


def inspect_container(recorder: Recorder, container: str) -> dict[str, Any]:
    result = recorder.docker(["container", "inspect", container])
    rows = parse_json(result.stdout, "container inspect")
    if not isinstance(rows, list) or len(rows) != 1 or not isinstance(rows[0], dict):
        raise RuntimeError("container inspect returned an unexpected shape")
    return rows[0]


def assert_never_started(row: dict[str, Any], nonce: str, image: str) -> str:
    identity = row.get("Id")
    config = row.get("Config")
    state = row.get("State")
    if not isinstance(identity, str) or not isinstance(config, dict) or not isinstance(state, dict):
        raise RuntimeError("container identity/state missing")
    labels = config.get("Labels")
    if not isinstance(labels, dict) or labels.get(OWNERSHIP_LABEL) != nonce:
        raise RuntimeError("container ownership label mismatch")
    if row.get("Image") != image:
        raise RuntimeError("container base image mismatch")
    if state.get("Status") != "created" or state.get("Running") is not False:
        raise RuntimeError("probe container was started")
    started_at = state.get("StartedAt")
    if not isinstance(started_at, str) or not started_at.startswith("0001-01-01"):
        raise RuntimeError("probe container has a start timestamp")
    return identity


def create_stopped(
    recorder: Recorder, name: str, nonce: str, image: str
) -> str:
    result = recorder.docker(
        [
            "container",
            "create",
            "--pull=never",
            "--name",
            name,
            "--label",
            EXECUTION_LABEL,
            "--label",
            f"{OWNERSHIP_LABEL}={nonce}",
            "--network=none",
            # The container is never started. Its layer must be writable for
            # the deliberate daemon-side cp replacement before commit.
            "--cap-drop=ALL",
            "--security-opt=no-new-privileges",
            "--user=65534:65534",
            "--entrypoint=/usr/bin/false",
            image,
        ]
    )
    returned = result.stdout.decode("ascii", errors="strict").strip()
    row = inspect_container(recorder, name)
    identity = assert_never_started(row, nonce, image)
    if returned != identity:
        raise RuntimeError("docker create identity mismatch")
    return identity


def copy_from_container(
    recorder: Recorder, container: str, destination: pathlib.Path
) -> str:
    if destination.exists():
        raise RuntimeError("copy destination already exists")
    result = recorder.docker(
        ["container", "cp", f"{container}:{PLUGIN_PATH}", "-"],
        output_limit_bytes=MAX_ARCHIVE_CAPTURE_BYTES,
    )
    write_single_plugin_archive(result.stdout, destination)
    if not destination.is_file() or destination.is_symlink():
        raise RuntimeError("copied plugin is not a regular file")
    return digest_file(destination)


def write_single_plugin_archive(raw: bytes, destination: pathlib.Path) -> None:
    if len(raw) > MAX_ARCHIVE_CAPTURE_BYTES:
        raise RuntimeError("plugin archive exceeded its bound")
    try:
        with tarfile.open(fileobj=io.BytesIO(raw), mode="r:") as archive:
            member = archive.next()
            if member is None or archive.next() is not None:
                raise RuntimeError("plugin archive must contain exactly one member")
            if (
                not member.isfile()
                or member.name.removeprefix("./") != "cargo-deny"
                or member.name.startswith("././")
                or member.pax_headers
                or member.size <= 0
                or member.size > MAX_PLUGIN_BYTES
            ):
                raise RuntimeError("plugin archive member is not admitted")
            stream = archive.extractfile(member)
            if stream is None:
                raise RuntimeError("plugin archive member is unreadable")
            payload = stream.read(MAX_PLUGIN_BYTES + 1)
    except (tarfile.TarError, OSError) as error:
        raise RuntimeError("invalid plugin archive") from error
    if len(payload) != member.size:
        raise RuntimeError("plugin archive member size mismatch")
    with destination.open("xb") as output:
        output.write(payload)


def execution_inventory(recorder: Recorder) -> dict[str, list[str]]:
    containers = recorder.docker(
        [
            "container",
            "ls",
            "--all",
            "--quiet",
            "--filter",
            f"label={EXECUTION_LABEL}",
        ]
    )
    volumes = recorder.docker(
        ["volume", "ls", "--quiet", "--filter", f"label={EXECUTION_LABEL}"]
    )
    return {
        "containers": sorted(containers.stdout.decode("ascii").split()),
        "volumes": sorted(volumes.stdout.decode("ascii").split()),
    }


def product_argv(
    server: pathlib.Path,
    socket: pathlib.Path,
    state_root: pathlib.Path,
    project_root: pathlib.Path,
    image: str,
) -> list[str]:
    return [
        str(server),
        "serve",
        "--stdio",
        "--root",
        str(project_root),
        "--docker",
        str(DOCKER),
        "--docker-socket",
        str(socket),
        "--state-root",
        str(state_root),
        "--rust-image",
        image,
    ]


def product_environment(private_root: pathlib.Path) -> dict[str, str]:
    return {
        "HOME": str(private_root),
        "TMPDIR": str(private_root),
        "PATH": "/usr/bin:/bin",
        "LC_ALL": "C",
        "LANG": "C",
    }


def validate_server(value: str) -> pathlib.Path:
    candidate = pathlib.Path(value)
    if not candidate.is_absolute():
        candidate = ROOT / candidate
    candidate = candidate.resolve()
    allowed = {
        (ROOT / "target/debug/rust-engineering-mcp").resolve(),
        (ROOT / "target/release/rust-engineering-mcp").resolve(),
    }
    if candidate not in allowed:
        raise RuntimeError("server must be the configured target/debug or target/release binary")
    mode = candidate.stat(follow_symlinks=False).st_mode
    if not stat.S_ISREG(mode) or mode & 0o111 == 0:
        raise RuntimeError("configured server is not an executable regular file")
    return candidate


def validate_receipt_path(value: str) -> pathlib.Path:
    path = pathlib.Path(value)
    if not path.is_absolute():
        path = ROOT / path
    path = path.resolve()
    target = (ROOT / "target").resolve()
    if path == target or target not in path.parents or path.name != "receipt.json":
        raise RuntimeError("receipt must be named receipt.json below target/")
    return path


def save_receipt(path: pathlib.Path, receipt: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    temporary = path.with_name(".receipt.json.tmp")
    temporary.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    temporary.chmod(0o600)
    temporary.replace(path)


def discover_owned_image(recorder: Recorder, nonce: str) -> list[str]:
    result = recorder.docker(
        [
            "image",
            "ls",
            "--all",
            "--no-trunc",
            "--quiet",
            "--filter",
            f"label={OWNERSHIP_LABEL}={nonce}",
        ],
        timeout=5,
    )
    return sorted(set(result.stdout.decode("ascii").split()))


def owned_container_for_cleanup(
    recorder: Recorder, name: str, nonce: str
) -> dict[str, Any] | None:
    inspected = recorder.docker(
        ["container", "inspect", name], timeout=5, accepted=(0, 1)
    )
    if inspected.returncode == 1:
        return None
    rows = parse_json(inspected.stdout, "cleanup container inspect")
    if not isinstance(rows, list) or len(rows) != 1 or not isinstance(rows[0], dict):
        raise RuntimeError("cleanup container shape")
    row = rows[0]
    labels = row.get("Config", {}).get("Labels", {})
    if labels.get(OWNERSHIP_LABEL) != nonce or not isinstance(row.get("Id"), str):
        raise RuntimeError("refusing to remove container without exact ownership")
    return row


def cleanup(
    recorder: Recorder,
    nonce: str,
    names: list[str],
    known_images: set[str],
    private_root: pathlib.Path,
) -> dict[str, Any]:
    result: dict[str, Any] = {
        "containers_removed": [],
        "images_removed": [],
        "private_root_removed": False,
        "errors": [],
    }
    ambiguous_statuses = {
        "timed_out",
        "interrupted",
        "output_limited",
        "unexpected_exit",
    }
    ambiguous_create = any(
        command.get("status") in ambiguous_statuses
        and "create" in command.get("argv", [])
        for command in recorder.receipt["commands"]
    )
    ambiguous_commit = any(
        command.get("status") in ambiguous_statuses
        and "commit" in command.get("argv", [])
        for command in recorder.receipt["commands"]
    )
    unresolved_commit = any(
        command.get("commit_identity_resolved") is False
        for command in recorder.receipt["commands"]
    )
    ambiguous_commit = ambiguous_commit or unresolved_commit
    result["ambiguous_create_settled"] = not ambiguous_create
    result["ambiguous_commit_settled"] = not ambiguous_commit
    result["unresolved_commit_reconciled"] = not unresolved_commit
    try:
        required_quiet_until = time.monotonic() + (
            AMBIGUOUS_SETTLE_SECONDS if ambiguous_create else 0
        )
        deadline = required_quiet_until + 2
        consecutive_absent = 0
        while consecutive_absent < 2 and time.monotonic() < deadline:
            found = False
            for name in reversed(names):
                row = owned_container_for_cleanup(recorder, name, nonce)
                if row is None:
                    continue
                found = True
                identity = row["Id"]
                state = row.get("State", {})
                never_started = (
                    state.get("Status") == "created"
                    and state.get("Running") is False
                    and str(state.get("StartedAt", "")).startswith("0001-01-01")
                )
                if not never_started:
                    result["errors"].append(
                        {
                            "kind": "container_state",
                            "name": name,
                            "error": "owned container was started",
                        }
                    )
                removed = recorder.docker(
                    ["container", "rm", "--force", identity],
                    timeout=10,
                    accepted=(0, 1),
                )
                if (
                    removed.returncode == 0
                    and identity not in result["containers_removed"]
                ):
                    result["containers_removed"].append(identity)
            if found:
                consecutive_absent = 0
            elif time.monotonic() < required_quiet_until:
                consecutive_absent = 0
            else:
                consecutive_absent += 1
            if consecutive_absent < 2:
                time.sleep(0.1)
        if consecutive_absent != 2:
            raise RuntimeError("owned container cleanup did not reach stable absence")
        result["ambiguous_create_settled"] = True
        for name in names:
            if owned_container_for_cleanup(recorder, name, nonce) is not None:
                raise RuntimeError(f"owned container still exists: {name}")
    except BaseException as error:  # Preserve failure and continue with image/scratch cleanup.
        result["errors"].append({"kind": "container", "error": str(error)})

    try:
        required_quiet_until = time.monotonic() + (
            AMBIGUOUS_SETTLE_SECONDS if ambiguous_commit else 0
        )
        deadline = required_quiet_until + 2
        consecutive_absent = 0
        while consecutive_absent < 2 and time.monotonic() < deadline:
            known_images.update(discover_owned_image(recorder, nonce))
            found = False
            for identity in sorted(known_images):
                inspected = recorder.docker(
                    ["image", "inspect", identity], timeout=5, accepted=(0, 1)
                )
                if inspected.returncode == 1:
                    continue
                found = True
                rows = parse_json(inspected.stdout, "cleanup image inspect")
                if not isinstance(rows, list) or len(rows) != 1 or not isinstance(rows[0], dict):
                    raise RuntimeError("cleanup image shape")
                row = rows[0]
                labels = row.get("Config", {}).get("Labels", {})
                if (
                    identity == APPROVED_IMAGE
                    or row.get("Id") != identity
                    or labels.get(OWNERSHIP_LABEL) != nonce
                ):
                    raise RuntimeError("refusing to remove image without exact ownership")
                if unresolved_commit:
                    result["unresolved_commit_reconciled"] = True
                removed = recorder.docker(
                    ["image", "rm", identity], timeout=10, accepted=(0, 1)
                )
                if removed.returncode == 0 and identity not in result["images_removed"]:
                    result["images_removed"].append(identity)
            remaining = discover_owned_image(recorder, nonce)
            if found or remaining or time.monotonic() < required_quiet_until:
                consecutive_absent = 0
            else:
                consecutive_absent += 1
            if consecutive_absent < 2:
                time.sleep(0.1)
        if consecutive_absent != 2:
            raise RuntimeError("owned image cleanup did not reach stable absence")
        result["ambiguous_commit_settled"] = True
        if discover_owned_image(recorder, nonce):
            raise RuntimeError("owned image still exists")
        if not result["unresolved_commit_reconciled"]:
            raise RuntimeError("commit identity was not reconciled during cleanup")
    except BaseException as error:
        result["errors"].append({"kind": "image", "error": str(error)})
    try:
        shutil.rmtree(private_root)
        result["private_root_removed"] = not private_root.exists()
        if not result["private_root_removed"]:
            raise RuntimeError("private root still exists")
    except BaseException as error:
        result["errors"].append({"kind": "private_root", "error": str(error)})
    result["verified"] = (
        not result["errors"]
        and result["private_root_removed"]
        and result["ambiguous_create_settled"]
        and result["ambiguous_commit_settled"]
        and result["unresolved_commit_reconciled"]
    )
    return result


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--server",
        default=os.environ.get("RUST_MCP_M4_SERVER"),
        required=os.environ.get("RUST_MCP_M4_SERVER") is None,
        help="configured target/debug or target/release rust-engineering-mcp binary",
    )
    parser.add_argument(
        "--receipt",
        default=os.environ.get(
            "RUST_MCP_M4_TAMPER_RECEIPT",
            "target/m4-tampered-plugin/receipt.json",
        ),
    )
    return parser.parse_args()


def main() -> int:
    if not __debug__:
        raise RuntimeError("optimized Python mode is rejected")
    if sys.platform != "darwin" or platform.machine() != "arm64":
        raise RuntimeError("this exact M4 qualification is macOS ARM64 only")
    socket_value = os.environ.get("RUST_MCP_TEST_SOCKET")
    if socket_value is None:
        raise RuntimeError("RUST_MCP_TEST_SOCKET is required")
    socket = pathlib.Path(socket_value)
    if not socket.is_absolute():
        raise RuntimeError("Docker socket must be absolute")
    if not DOCKER.is_file() or DOCKER.is_symlink():
        raise RuntimeError("pinned Docker CLI is unavailable or is a symlink")
    configured = json.loads(IMAGE_CONFIG.read_text())
    if configured.get("image") != APPROVED_IMAGE:
        raise RuntimeError("M4 image configuration changed")
    fixture_sha256 = digest_bytes(FIXTURE_BYTES)
    if fixture_sha256 == APPROVED_CARGO_DENY_SHA256:
        raise RuntimeError("inert fixture does not change the plugin digest")

    arguments = parse_args()
    server = validate_server(arguments.server)
    receipt_path = validate_receipt_path(arguments.receipt)
    private_root = pathlib.Path(
        tempfile.mkdtemp(prefix="rust-mcp-m4-tampered-plugin-")
    ).resolve()
    private_root.chmod(0o700)
    nonce = os.urandom(16).hex()
    names = [
        f"rust-mcp-m4-plugin-original-before-{nonce}",
        f"rust-mcp-m4-plugin-tamper-{nonce}",
        f"rust-mcp-m4-plugin-committed-{nonce}",
        f"rust-mcp-m4-plugin-original-after-{nonce}",
    ]
    receipt: dict[str, Any] = {
        "schema": "rust-mcp-m4-tampered-plugin-v1",
        "status": "running",
        "started_at": utc_now(),
        "approved_image_id": APPROVED_IMAGE,
        "plugin_path": PLUGIN_PATH,
        "network_used": False,
        "containers_started": False,
        "image_published_or_tagged": False,
        "scope": "immutable image identity admission, not a hostile-daemon claim",
        "commands": [],
        "inputs": {
            "script_sha256": digest_file(pathlib.Path(__file__).resolve()),
            "image_config_sha256": digest_file(IMAGE_CONFIG),
            "server_path": str(server.relative_to(ROOT)),
            "server_sha256": digest_file(server),
            "docker_sha256": digest_file(DOCKER),
            "docker_socket_sha256": digest_bytes(os.fsencode(socket)),
            "fixture_sha256": fixture_sha256,
            "fixture_bytes": len(FIXTURE_BYTES),
            "expected_original_plugin_sha256": APPROVED_CARGO_DENY_SHA256,
        },
    }
    recorder = Recorder(receipt, socket)
    known_images: set[str] = set()
    error: BaseException | None = None
    save_receipt(receipt_path, receipt)
    try:
        original = inspect_image(recorder, APPROVED_IMAGE)
        if (
            original.get("Id") != APPROVED_IMAGE
            or original.get("Architecture") != "arm64"
            or original.get("Os") != "linux"
        ):
            raise RuntimeError("approved image identity/platform mismatch")

        fixture = private_root / "inert-cargo-deny"
        fixture.write_bytes(FIXTURE_BYTES)
        fixture.chmod(0o555)
        project_root = private_root / "project"
        project_root.mkdir(mode=0o700)
        (project_root / "INERT-NO-PROJECT-CODE").write_bytes(b"fixture only\n")
        project_before = private_tree(project_root)

        original_before_id = create_stopped(
            recorder, names[0], nonce, APPROVED_IMAGE
        )
        original_before_path = private_root / "cargo-deny-original-before"
        original_before_hash = copy_from_container(
            recorder, original_before_id, original_before_path
        )
        if original_before_hash != APPROVED_CARGO_DENY_SHA256:
            raise RuntimeError("approved image cargo-deny digest mismatch")

        tamper_container_id = create_stopped(
            recorder, names[1], nonce, APPROVED_IMAGE
        )
        recorder.docker(
            ["container", "cp", str(fixture), f"{tamper_container_id}:{PLUGIN_PATH}"]
        )
        copied_fixture = private_root / "cargo-deny-tampered-layer"
        copied_fixture_hash = copy_from_container(
            recorder, tamper_container_id, copied_fixture
        )
        if copied_fixture_hash != fixture_sha256:
            raise RuntimeError("stopped-layer plugin replacement mismatch")
        assert_never_started(
            inspect_container(recorder, tamper_container_id), nonce, APPROVED_IMAGE
        )

        commit = recorder.docker(
            ["container", "commit", "--no-pause", tamper_container_id],
            timeout=120,
        )
        commit_event = receipt["commands"][-1]
        commit_event["commit_identity_resolved"] = False
        tampered_image = parse_commit_identity(commit.stdout)
        if tampered_image == APPROVED_IMAGE:
            raise RuntimeError("altered layer retained the approved image identity")
        known_images.add(tampered_image)
        tampered = inspect_image(recorder, tampered_image)
        labels = tampered.get("Config", {}).get("Labels", {})
        if (
            tampered.get("Id") != tampered_image
            or labels.get(OWNERSHIP_LABEL) != nonce
            or tampered.get("RepoTags") not in (None, [])
        ):
            raise RuntimeError("private committed image identity/tag/ownership mismatch")
        commit_event["commit_identity_resolved"] = True

        committed_container_id = create_stopped(
            recorder, names[2], nonce, tampered_image
        )
        committed_plugin = private_root / "cargo-deny-committed"
        committed_plugin_hash = copy_from_container(
            recorder, committed_container_id, committed_plugin
        )
        if committed_plugin_hash != fixture_sha256:
            raise RuntimeError("committed image does not contain the inert replacement")

        invalid_state = private_root / "invalid-state-must-not-exist"
        inventory_before = execution_inventory(recorder)
        invalid = recorder.run(
            product_argv(
                server, socket, invalid_state, project_root, tampered_image
            ),
            timeout=PRODUCT_TIMEOUT_SECONDS,
            input_bytes=b"",
            accepted=(2,),
            environment=product_environment(private_root),
        )
        inventory_after = execution_inventory(recorder)
        if invalid.stdout != b"" or invalid.stderr != USAGE_ERROR:
            raise RuntimeError("altered image did not reach the closed CLI admission oracle")
        if invalid_state.exists():
            raise RuntimeError("rejected image created product state")
        if inventory_after != inventory_before:
            raise RuntimeError("rejected image changed the product Docker inventory")
        if private_tree(project_root) != project_before:
            raise RuntimeError("rejected image changed the inert project fixture")

        original_state = private_root / "original-state"
        original_state.mkdir(mode=0o700)
        original_control = recorder.run(
            product_argv(
                server, socket, original_state, project_root, APPROVED_IMAGE
            ),
            timeout=PRODUCT_TIMEOUT_SECONDS,
            input_bytes=b"",
            accepted=(0,),
            environment=product_environment(private_root),
        )
        if private_tree(project_root) != project_before:
            raise RuntimeError("approved-image EOF control changed the inert fixture")

        original_after_id = create_stopped(
            recorder, names[3], nonce, APPROVED_IMAGE
        )
        original_after_path = private_root / "cargo-deny-original-after"
        original_after_hash = copy_from_container(
            recorder, original_after_id, original_after_path
        )
        original_after = inspect_image(recorder, APPROVED_IMAGE)
        if (
            original_after.get("Id") != APPROVED_IMAGE
            or original_after_hash != APPROVED_CARGO_DENY_SHA256
            or original_after_hash != original_before_hash
        ):
            raise RuntimeError("approved image or plugin changed during the probe")

        receipt["observation"] = {
            "original_image_id": APPROVED_IMAGE,
            "tampered_image_id": tampered_image,
            "image_identity_changed": True,
            "original_plugin_sha256_before": original_before_hash,
            "tampered_plugin_sha256": committed_plugin_hash,
            "original_plugin_sha256_after": original_after_hash,
            "plugin_digest_changed": True,
            "tampered_admission_exit_code": invalid.returncode,
            "tampered_admission_rejected_before_state": True,
            "tampered_admission_docker_inventory_unchanged": True,
            "approved_image_eof_control_exit_code": original_control.returncode,
            "approved_image_compatibility_preserved": True,
            "project_fixture_unchanged": True,
        }
        inputs_after = {
            "script_sha256": digest_file(pathlib.Path(__file__).resolve()),
            "image_config_sha256": digest_file(IMAGE_CONFIG),
            "server_sha256": digest_file(server),
            "docker_sha256": digest_file(DOCKER),
        }
        inputs_before = {
            key: receipt["inputs"][key]
            for key in inputs_after
        }
        if inputs_after != inputs_before:
            raise RuntimeError("script, image config, server, or Docker CLI changed during probe")
        receipt["inputs_unchanged"] = True
    except BaseException as caught:
        error = caught
        receipt["error"] = {"type": type(caught).__name__, "message": str(caught)}
    finally:
        receipt["cleanup"] = cleanup(
            recorder, nonce, names, known_images, private_root
        )
        receipt["finished_at"] = utc_now()
        if error is None and receipt["cleanup"]["verified"]:
            receipt["status"] = "passed"
        else:
            receipt["status"] = "failed"
            if error is None:
                receipt["error"] = {
                    "type": "CleanupError",
                    "message": "owned cleanup was not fully verified",
                }
        save_receipt(receipt_path, receipt)

    if receipt["status"] != "passed":
        print(f"FAIL M4 tampered plugin: {receipt_path}", file=sys.stderr)
        return 1
    print(f"PASS M4 tampered plugin: {receipt_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
