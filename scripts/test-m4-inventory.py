#!/usr/bin/env python3
"""Re-accredit M4 image pins without starting or executing guest code."""
from __future__ import annotations

import datetime
import hashlib
import json
import os
import pathlib
import platform
import resource
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import time
from typing import Any


ROOT = pathlib.Path(__file__).resolve().parents[1]
IMAGE_CONFIG = ROOT / "docs/validation/M4/runtime-image.json"
OUTPUT = ROOT / "target/m4-runtime-inventory.json"
IMAGE = "sha256:25ed3626e710081a571a86a29521eaf2e890e796afd422ba5e409e0ce1891635"
DOCKER = pathlib.Path("/Applications/Docker.app/Contents/Resources/bin/docker")
SYSROOT = "/opt/miri-sysroot/2026-09-07/aarch64-unknown-linux-gnu"
SYSROOT_TREE_SHA256 = "68324d8d8b2dcb55616ff2e53c7c91f4db78ada40ceafe189f933899d8a1f136"
OWNERSHIP_LABEL = "org.rust-mcp.m4-inventory"
COMMAND_TIMEOUT_SECONDS = 30
FILE_ARCHIVE_LIMIT = 64 * 1024 * 1024
SYSROOT_ARCHIVE_LIMIT = 192 * 1024 * 1024
SYSROOT_MEMBER_LIMIT = 200_000
PINS = {
    "/opt/security/bin/cargo-deny": "e9bcd2f489b8dd22cc3f3fc3452cfa0483cf8b9236a5e2787c54f864d4e77715",
    "/opt/security/bin/rust-mcp-unsafe-helper": "af8af1a021094003cd90023a882d707f7062cc98938bb06a4c105bf75e10120b",
    "/opt/rust-nightly-2026-09-07/bin/rustc": "7a1126253fa42b9ebbc398431f2442620368b606927e0107f99a8051052c2d3d",
    "/opt/rust-nightly-2026-09-07/bin/cargo": "21d5d6819852f96db1d9c15afc9554a46a4ec9f1e0d52dc23aa9be4a81a9ec83",
    "/opt/rust-nightly-2026-09-07/bin/miri": "a3dad3cf43a3b8097b55f6c7ca08386af004c433c4e0e8125d3e62cecdc4ff85",
    "/opt/rust-nightly-2026-09-07/bin/cargo-miri": "7faa3486e6a51ae74cdf404358fb028ddeaaa2bb56604b175d8e9fe8463c6842",
}


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


def save(receipt: dict[str, Any]) -> None:
    OUTPUT.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    temporary = OUTPUT.with_name(".m4-runtime-inventory.json.tmp")
    temporary.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    temporary.chmod(0o600)
    temporary.replace(OUTPUT)


class Docker:
    def __init__(self, socket: pathlib.Path, receipt: dict[str, Any]) -> None:
        self.socket = socket
        self.receipt = receipt
        self.prefix = [str(DOCKER), "--host", f"unix://{socket}"]
        self.environment = {"PATH": "/usr/bin:/bin", "LC_ALL": "C", "LANG": "C"}

    def _safe_argv(self, argv: list[str]) -> list[str]:
        hidden = {str(self.socket), f"unix://{self.socket}"}
        return ["<docker-socket>" if value in hidden else value for value in argv]

    def run(
        self,
        args: list[str],
        *,
        timeout: int = COMMAND_TIMEOUT_SECONDS,
        accepted: tuple[int, ...] = (0,),
    ) -> subprocess.CompletedProcess[bytes]:
        argv = [*self.prefix, *args]
        started = time.monotonic()
        event: dict[str, Any] = {
            "argv": self._safe_argv(argv),
            "argv_sha256": canonical_digest(argv),
            "timeout_seconds": timeout,
        }
        self.receipt["commands"].append(event)
        try:
            result = subprocess.run(  # Closed argv; no shell or caller-supplied Docker action.
                argv,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=timeout,
                check=False,
                env=self.environment,
            )
        except subprocess.TimeoutExpired as error:
            stdout = error.stdout or b""
            stderr = error.stderr or b""
            event.update(
                status="timed_out",
                seconds=round(time.monotonic() - started, 3),
                stdout_bytes=len(stdout),
                stdout_sha256=digest_bytes(stdout),
                stderr_bytes=len(stderr),
                stderr_sha256=digest_bytes(stderr),
            )
            raise RuntimeError("Docker command timed out") from error
        event.update(
            status="exited",
            exit_code=result.returncode,
            seconds=round(time.monotonic() - started, 3),
            stdout_bytes=len(result.stdout),
            stdout_sha256=digest_bytes(result.stdout),
            stderr_bytes=len(result.stderr),
            stderr_sha256=digest_bytes(result.stderr),
        )
        if len(result.stdout) > 1024 * 1024 or len(result.stderr) > 1024 * 1024:
            raise RuntimeError("Docker control output exceeded its bound")
        if result.returncode not in accepted:
            raise RuntimeError(
                f"Docker command returned {result.returncode}, expected {accepted}"
            )
        return result

    def copy_archive(
        self,
        container: str,
        guest_path: str,
        output: pathlib.Path,
        maximum: int,
        timeout: int,
    ) -> None:
        argv = [*self.prefix, "container", "cp", f"{container}:{guest_path}", "-"]
        started = time.monotonic()
        event: dict[str, Any] = {
            "argv": self._safe_argv(argv),
            "argv_sha256": canonical_digest(argv),
            "timeout_seconds": timeout,
            "stdout_file": output.name,
            "stdout_limit_bytes": maximum,
        }
        self.receipt["commands"].append(event)

        def limit_output_file() -> None:
            resource.setrlimit(resource.RLIMIT_FSIZE, (maximum, maximum))

        try:
            with output.open("xb") as stream:
                result = subprocess.run(  # Closed argv and bounded regular-file stdout.
                    argv,
                    stdout=stream,
                    stderr=subprocess.PIPE,
                    timeout=timeout,
                    check=False,
                    env=self.environment,
                    preexec_fn=limit_output_file,
                )
        except subprocess.TimeoutExpired as error:
            stderr = error.stderr or b""
            event.update(
                status="timed_out",
                seconds=round(time.monotonic() - started, 3),
                stderr_bytes=len(stderr),
                stderr_sha256=digest_bytes(stderr),
            )
            raise RuntimeError("Docker copy timed out") from error
        size = output.stat().st_size
        event.update(
            status="exited",
            exit_code=result.returncode,
            seconds=round(time.monotonic() - started, 3),
            stdout_bytes=size,
            stdout_sha256=digest_file(output),
            stderr_bytes=len(result.stderr),
            stderr_sha256=digest_bytes(result.stderr),
        )
        if len(result.stderr) > 1024 * 1024:
            raise RuntimeError("Docker copy stderr exceeded its bound")
        if result.returncode != 0:
            raise RuntimeError(f"Docker copy returned {result.returncode}")
        if size <= 0 or size > maximum:
            raise RuntimeError("Docker copy archive exceeded its bound")


def parse_single_object(raw: bytes, context: str) -> dict[str, Any]:
    try:
        rows = json.loads(raw)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise RuntimeError(f"invalid JSON from {context}") from error
    if not isinstance(rows, list) or len(rows) != 1 or not isinstance(rows[0], dict):
        raise RuntimeError(f"unexpected {context} shape")
    return rows[0]


def inspect_image(docker: Docker) -> dict[str, Any]:
    return parse_single_object(docker.run(["image", "inspect", IMAGE]).stdout, "image inspect")


def validate_image(image: dict[str, Any]) -> None:
    if (
        image.get("Id") != IMAGE
        or image.get("Architecture") != "arm64"
        or image.get("Os") != "linux"
    ):
        raise RuntimeError("approved image identity/platform mismatch")


def inspect_owned_container(
    docker: Docker, name: str, nonce: str, *, require_never_started: bool = True
) -> dict[str, Any] | None:
    inspected = docker.run(["container", "inspect", name], accepted=(0, 1))
    if inspected.returncode == 1:
        return None
    row = parse_single_object(inspected.stdout, "container inspect")
    labels = row.get("Config", {}).get("Labels", {})
    state = row.get("State", {})
    if (
        labels.get(OWNERSHIP_LABEL) != nonce
        or row.get("Image") != IMAGE
        or not isinstance(row.get("Id"), str)
    ):
        raise RuntimeError("refusing container without exact ownership/image")
    never_started = (
        state.get("Status") == "created"
        and state.get("Running") is False
        and str(state.get("StartedAt", "")).startswith("0001-01-01")
    )
    if require_never_started and not never_started:
        raise RuntimeError("inventory container was started")
    row["_rust_mcp_never_started"] = never_started
    return row


def cleanup(docker: Docker, name: str, nonce: str, scratch: pathlib.Path) -> dict[str, Any]:
    result: dict[str, Any] = {
        "container_removed": None,
        "container_absent": False,
        "scratch_removed": False,
        "errors": [],
    }
    try:
        row = inspect_owned_container(docker, name, nonce, require_never_started=False)
        if row is not None:
            identity = row["Id"]
            if not row["_rust_mcp_never_started"]:
                result["errors"].append(
                    {"kind": "container_state", "error": "owned container was started"}
                )
            docker.run(["container", "rm", "--force", identity])
            result["container_removed"] = identity
        # A timed-out create is ambiguous: require two separated absent reads so
        # a daemon operation completing just after the first lookup is removed.
        consecutive_absent = 0
        deadline = time.monotonic() + 5
        while consecutive_absent < 2 and time.monotonic() < deadline:
            row = inspect_owned_container(
                docker, name, nonce, require_never_started=False
            )
            if row is None:
                consecutive_absent += 1
                if consecutive_absent < 2:
                    time.sleep(0.1)
                continue
            consecutive_absent = 0
            identity = row["Id"]
            if not row["_rust_mcp_never_started"]:
                result["errors"].append(
                    {"kind": "container_state", "error": "owned container was started"}
                )
            docker.run(["container", "rm", "--force", identity])
            result["container_removed"] = identity
        result["container_absent"] = consecutive_absent == 2
        if not result["container_absent"]:
            raise RuntimeError("owned inventory container remains")
    except BaseException as error:
        result["errors"].append({"kind": "container", "error": str(error)})
    try:
        shutil.rmtree(scratch)
        result["scratch_removed"] = not scratch.exists()
        if not result["scratch_removed"]:
            raise RuntimeError("private scratch remains")
    except BaseException as error:
        result["errors"].append({"kind": "scratch", "error": str(error)})
    result["verified"] = (
        not result["errors"]
        and result["container_absent"]
        and result["scratch_removed"]
    )
    return result


def one_file_from_archive(
    archive: pathlib.Path, guest_path: str
) -> tuple[str, int]:
    with tarfile.open(archive) as tar:
        members = tar.getmembers()
        if len(members) != 1 or not members[0].isfile():
            raise RuntimeError(f"{guest_path} did not produce exactly one regular file")
        name = pathlib.PurePosixPath(members[0].name)
        if ".." in name.parts or name.name != pathlib.PurePosixPath(guest_path).name:
            raise RuntimeError(f"unexpected archive identity for {guest_path}")
        stream = tar.extractfile(members[0])
        if stream is None:
            raise RuntimeError(f"missing archive bytes for {guest_path}")
        return hashlib.file_digest(stream, "sha256").hexdigest(), members[0].size


def sysroot_from_archive(archive: pathlib.Path) -> tuple[str, int, int]:
    rows: list[tuple[str, str, int]] = []
    count = 0
    total = 0
    with tarfile.open(archive) as tar:
        for member in tar:
            count += 1
            if count > SYSROOT_MEMBER_LIMIT:
                raise RuntimeError("sysroot archive member budget exceeded")
            if not (member.isdir() or member.isfile()):
                raise RuntimeError(f"special sysroot member: {member.name}")
            relative = pathlib.PurePosixPath(member.name)
            if (
                not relative.parts
                or relative.parts[0] != "aarch64-unknown-linux-gnu"
                or ".." in relative.parts
                or relative.is_absolute()
            ):
                raise RuntimeError(f"invalid sysroot archive path: {member.name}")
            if member.isfile():
                total += member.size
                if total > SYSROOT_ARCHIVE_LIMIT:
                    raise RuntimeError("sysroot file-byte budget exceeded")
                stream = tar.extractfile(member)
                if stream is None:
                    raise RuntimeError(f"missing sysroot bytes: {member.name}")
                path = SYSROOT + "/" + str(pathlib.PurePosixPath(*relative.parts[1:]))
                rows.append((path, hashlib.file_digest(stream, "sha256").hexdigest(), member.size))
    lines = "".join(digest + "  " + path + "\n" for path, digest, _ in sorted(rows))
    return digest_bytes(lines.encode()), len(rows), sum(row[2] for row in rows)


def input_hashes() -> dict[str, str]:
    return {
        "script_sha256": digest_file(pathlib.Path(__file__).resolve()),
        "image_config_sha256": digest_file(IMAGE_CONFIG),
        "docker_cli_sha256": digest_file(DOCKER),
    }


def main() -> int:
    if not __debug__:
        raise RuntimeError("optimized Python mode is rejected")
    if sys.platform != "darwin" or platform.machine() != "arm64":
        raise RuntimeError("M4 immutable image inventory is macOS ARM64 only")
    socket_value = os.environ.get("RUST_MCP_TEST_SOCKET")
    if socket_value is None:
        raise RuntimeError("RUST_MCP_TEST_SOCKET is required")
    socket = pathlib.Path(socket_value)
    if not socket.is_absolute():
        raise RuntimeError("Docker socket must be absolute")
    docker_mode = DOCKER.stat(follow_symlinks=False).st_mode
    if not stat.S_ISREG(docker_mode) or docker_mode & 0o111 == 0 or DOCKER.is_symlink():
        raise RuntimeError("pinned Docker CLI is not an executable regular file")
    try:
        configured = json.loads(IMAGE_CONFIG.read_text())
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise RuntimeError("invalid M4 runtime image configuration") from error
    if configured.get("image") != IMAGE:
        raise RuntimeError("M4 image configuration does not match the exact inventory image")

    nonce = os.urandom(16).hex()
    name = f"rust-mcp-m4-inventory-{nonce}"
    scratch = pathlib.Path(tempfile.mkdtemp(prefix="rust-mcp-m4-inventory-")).resolve()
    scratch.chmod(0o700)
    receipt: dict[str, Any] = {
        "schema": "rust-mcp-m4-inventory-v2",
        "status": "running",
        "started_at": utc_now(),
        "image_id": IMAGE,
        "guest_code_executed": False,
        "network_used": False,
        "commands": [],
        "files": [],
        "inputs_before": input_hashes(),
    }
    docker = Docker(socket, receipt)
    failure: BaseException | None = None
    save(receipt)
    try:
        image_before = inspect_image(docker)
        validate_image(image_before)
        receipt["image_inspect_before_sha256"] = canonical_digest(image_before)

        created = docker.run(
            [
                "container",
                "create",
                "--pull=never",
                "--name",
                name,
                "--label",
                f"{OWNERSHIP_LABEL}={nonce}",
                "--network=none",
                "--read-only",
                "--cap-drop=ALL",
                "--security-opt=no-new-privileges",
                "--user=65534:65534",
                "--entrypoint=/usr/bin/true",
                IMAGE,
            ]
        )
        row = inspect_owned_container(docker, name, nonce)
        if row is None or created.stdout.decode("ascii").strip() != row["Id"]:
            raise RuntimeError("created container identity mismatch")
        receipt["container"] = {
            "id": row["Id"],
            "never_started": True,
            "configuration_sha256_before": canonical_digest(row.get("Config")),
        }

        for index, (guest_path, expected) in enumerate(PINS.items()):
            archive = scratch / f"file-{index}.tar"
            docker.copy_archive(name, guest_path, archive, FILE_ARCHIVE_LIMIT, 30)
            observed, size = one_file_from_archive(archive, guest_path)
            if observed != expected:
                raise RuntimeError(f"pinned component digest mismatch: {guest_path}")
            receipt["files"].append(
                {"path": guest_path, "sha256": observed, "bytes": size}
            )

        archive = scratch / "sysroot.tar"
        docker.copy_archive(name, SYSROOT, archive, SYSROOT_ARCHIVE_LIMIT, 90)
        tree, file_count, file_bytes = sysroot_from_archive(archive)
        if tree != SYSROOT_TREE_SHA256:
            raise RuntimeError("sysroot tree digest mismatch")
        receipt.update(
            sysroot_tree_sha256=tree,
            sysroot_file_count=file_count,
            sysroot_file_bytes=file_bytes,
        )

        row_after = inspect_owned_container(docker, name, nonce)
        if row_after is None:
            raise RuntimeError("inventory container disappeared before final inspection")
        receipt["container"]["configuration_sha256_after"] = canonical_digest(
            row_after.get("Config")
        )
        if (
            receipt["container"]["configuration_sha256_after"]
            != receipt["container"]["configuration_sha256_before"]
        ):
            raise RuntimeError("inventory container configuration changed")

        image_after = inspect_image(docker)
        validate_image(image_after)
        receipt["image_inspect_after_sha256"] = canonical_digest(image_after)
        if receipt["image_inspect_after_sha256"] != receipt["image_inspect_before_sha256"]:
            raise RuntimeError("approved image inspection changed during inventory")
        receipt["inputs_after"] = input_hashes()
        if receipt["inputs_after"] != receipt["inputs_before"]:
            raise RuntimeError("script, image config, or Docker CLI changed during inventory")
        receipt["inputs_unchanged"] = True
    except BaseException as error:
        failure = error
        receipt["error"] = {"type": type(error).__name__, "message": str(error)}
    finally:
        receipt["cleanup"] = cleanup(docker, name, nonce, scratch)
        receipt["finished_at"] = utc_now()
        if failure is None and receipt["cleanup"]["verified"]:
            receipt["status"] = "passed"
        else:
            receipt["status"] = "failed"
            if failure is None:
                receipt["error"] = {
                    "type": "CleanupError",
                    "message": "owned cleanup was not fully verified",
                }
        save(receipt)

    if receipt["status"] != "passed":
        print(f"FAIL M4 immutable image inventory: {OUTPUT}", file=sys.stderr)
        return 1
    print(f"PASS M4 immutable image inventory: {OUTPUT}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
