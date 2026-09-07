#!/usr/bin/env python3
"""Bounded stock-client qualification plan for negotiated M3 Tasks.

The default mode is Docker-free and only validates the harness. ``--run`` is an
explicit serialized client/Docker gate. Every live attempt gets a new immutable
directory; a current receipt is published only after all assertions pass.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
import pathlib
import queue
import selectors
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[1]
ATTEMPTS = ROOT / "docs/validation/m3-clients"
CURRENT = ROOT / "docs/validation/M3-02-clients.json"
SERVER = ROOT / "target/release/rust-engineering-mcp"
NODE = pathlib.Path("/Users/cburgosro/.nvm/versions/node/v24.15.0/bin/node")
INSPECTOR = ROOT / "target/m1-17-inspector/node_modules/@modelcontextprotocol/inspector/clients/cli/build/index.js"
INSPECTOR_PACKAGE = ROOT / "target/m1-17-inspector/node_modules/@modelcontextprotocol/inspector/package.json"
DOCKER = pathlib.Path("/Applications/Docker.app/Contents/Resources/bin/docker")
IMAGE = "sha256:384a1742ecc53cdd3a9c0bf36c6f8b66db73ddd118aeeae6e55654ea998ae36a"
TASKS = "io.modelcontextprotocol/tasks"
MAX_OUTPUT = 8 * 1024 * 1024
EXPECTED_TOOLS = (
    "rust.project.open", "rust.project.inspect", "rust.toolchain.inspect",
    "rust.check", "rust.fmt.check", "rust.clippy", "rust.test",
    "rust.test.nextest", "rust.dependencies.audit", "rust.diagnostics.explain",
    "rust.quality.gate", "rust.catalog.status", "rust.crate.search",
    "rust.crate.inspect", "rust.manifest.patch", "rust.fmt.apply",
    "rust.fix.apply", "rust.dependency.add", "rust.dependency.remove",
    "rust.coverage", "rust.semver.check", "rust.mutation.test",
)


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def file_digest(path: pathlib.Path) -> str:
    return digest(path.read_bytes())


def save_json(path: pathlib.Path, value: object, exclusive: bool = False) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    flags = os.O_WRONLY | os.O_CREAT | (os.O_EXCL if exclusive else os.O_TRUNC)
    descriptor = os.open(path, flags, 0o600)
    try:
        payload = json.dumps(value, indent=2, sort_keys=True).encode() + b"\n"
        os.write(descriptor, payload)
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def tasks_declared(value: object) -> bool:
    if not isinstance(value, dict):
        return False
    extensions = value.get("extensions")
    return isinstance(extensions, dict) and isinstance(extensions.get(TASKS), dict)


def append_observation(path: pathlib.Path, value: dict[str, object]) -> None:
    payload = json.dumps(value, sort_keys=True, separators=(",", ":")).encode() + b"\n"
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o600)
    try:
        os.write(descriptor, payload)
    finally:
        os.close(descriptor)


def proxy(server_argv: list[str], observation: pathlib.Path, client: str) -> int:
    """Transparent stdio proxy recording only bounded protocol metadata."""
    session = uuid.uuid4().hex
    child = subprocess.Popen(
        server_argv,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=os.environ.copy(),
    )
    lock = threading.Lock()

    def record(direction: str, line: bytes) -> None:
        row: dict[str, object] = {
            "client": client,
            "direction": direction,
            "session": session,
            "bytes": len(line),
            "sha256": digest(line),
        }
        try:
            message = json.loads(line)
        except (UnicodeDecodeError, json.JSONDecodeError):
            row["malformed"] = True
        else:
            if isinstance(message, dict):
                method = message.get("method")
                if isinstance(method, str):
                    row["method"] = method
                if direction == "client" and method == "initialize":
                    capabilities = message.get("params", {}).get("capabilities", {})
                    row["tasks_declared"] = tasks_declared(capabilities)
                if direction == "client" and method == "server/discover":
                    metadata = message.get("params", {}).get("_meta", {})
                    capabilities = metadata.get(
                        "io.modelcontextprotocol/clientCapabilities", {}
                    )
                    row["tasks_declared"] = tasks_declared(capabilities)
                if direction == "server" and "result" in message:
                    capabilities = message.get("result", {}).get("capabilities", {})
                    if isinstance(capabilities, dict) and capabilities:
                        row["tasks_advertised"] = tasks_declared(capabilities)
                if direction == "client" and method == "tools/call":
                    row["tool"] = message.get("params", {}).get("name")
                if direction == "client" and method == "resources/read":
                    uri = message.get("params", {}).get("uri")
                    if isinstance(uri, str):
                        row["resource_scheme"] = uri.partition(":")[0]
        with lock:
            append_observation(observation, row)

    def relay(source, destination, direction: str) -> None:
        while True:
            line = source.readline(MAX_OUTPUT + 1)
            if not line:
                break
            if len(line) > MAX_OUTPUT:
                child.kill()
                break
            record(direction, line.rstrip(b"\n"))
            destination.write(line)
            destination.flush()
        try:
            destination.close()
        except BrokenPipeError:
            pass

    def relay_stderr() -> None:
        observed = 0
        while True:
            block = child.stderr.read(65536)
            if not block:
                break
            observed += len(block)
            if observed <= MAX_OUTPUT:
                sys.stderr.buffer.write(block)
                sys.stderr.buffer.flush()

    threads = [
        threading.Thread(target=relay, args=(sys.stdin.buffer, child.stdin, "client")),
        threading.Thread(target=relay, args=(child.stdout, sys.stdout.buffer, "server")),
        threading.Thread(target=relay_stderr),
    ]
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join()
    return child.wait(timeout=15)


def run_bounded(argv: list[str], cwd: pathlib.Path, timeout: int, artifact: pathlib.Path) -> dict:
    started = time.monotonic()
    child = subprocess.Popen(
        argv,
        cwd=cwd,
        env={key: value for key, value in os.environ.items() if key in {
            "HOME", "PATH", "TMPDIR", "USER", "LOGNAME", "SHELL",
            "RUST_MCP_TEST_SOCKET", "RUST_MCP_TEST_TASKS_READY",
            "RUST_MCP_M3_PASSING", "RUST_MCP_M3_FAILING", "RUST_MCP_M3_SLOW",
        }},
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        start_new_session=True,
    )
    selector = selectors.DefaultSelector()
    buffers = {"stdout": bytearray(), "stderr": bytearray()}
    for name, stream in (("stdout", child.stdout), ("stderr", child.stderr)):
        os.set_blocking(stream.fileno(), False)
        selector.register(stream, selectors.EVENT_READ, name)
    deadline = started + timeout
    overflow = False
    while child.poll() is None or selector.get_map():
        if time.monotonic() >= deadline or overflow:
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        for key, _ in selector.select(0.05):
            block = os.read(key.fileobj.fileno(), 65536)
            if not block:
                selector.unregister(key.fileobj)
                key.fileobj.close()
                continue
            target = buffers[key.data]
            target.extend(block[: max(0, MAX_OUTPUT - len(target))])
            overflow = overflow or len(target) >= MAX_OUTPUT
    child.wait()
    result = {
        "argv_sha256": digest(json.dumps(argv).encode()),
        "exit_code": child.returncode,
        "duration_seconds": round(time.monotonic() - started, 3),
        "stdout_bytes": len(buffers["stdout"]),
        "stdout_sha256": digest(bytes(buffers["stdout"])),
        "stderr_bytes": len(buffers["stderr"]),
        "stderr_sha256": digest(bytes(buffers["stderr"])),
        "overflow": overflow,
    }
    artifact.with_suffix(".stdout").write_bytes(buffers["stdout"])
    artifact.with_suffix(".stderr").write_bytes(buffers["stderr"])
    save_json(artifact, result, exclusive=True)
    if overflow or child.returncode < 0:
        raise RuntimeError(f"client command failed: {artifact.name}")
    return result


def next_attempt() -> pathlib.Path:
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


def server_argv(state: pathlib.Path, socket: str) -> list[str]:
    return [
        str(SERVER), "serve", "--stdio", "--root", str(ROOT / "fixtures/nextest"),
        "--docker", str(DOCKER), "--docker-socket", socket,
        "--state-root", str(state), "--rust-image", IMAGE,
    ]


def inspector_argv(observation: pathlib.Path, state: pathlib.Path, socket: str,
                   method: str, extra: list[str]) -> list[str]:
    proxy_args = json.dumps(server_argv(state, socket), separators=(",", ":"))
    return [
        str(NODE), str(INSPECTOR), sys.executable, str(pathlib.Path(__file__).resolve()),
        "proxy", "--client", "inspector", "--observation", str(observation),
        "--server-argv-json", proxy_args, "--", "--method", method,
        "--format", "json", "--stored-auth-only", *extra,
    ]


def inspector_gate(attempt: pathlib.Path, socket: str) -> dict[str, object]:
    observation = attempt / "protocol.jsonl"
    state = attempt / "state-inspector"
    state.mkdir(mode=0o700)
    commands = [
        ("discovery", "tools/list", ["--strict"]),
        ("positive", "tools/call", ["--tool-name", "rust.project.open", "--tool-args-json",
         json.dumps({"path": str(ROOT / "fixtures/nextest/passing")}, separators=(",", ":"))]),
        ("failure", "tools/call", ["--tool-name", "rust.test.nextest", "--tool-args-json",
         json.dumps({"project_ref": "prj_" + "0" * 32, "execution_mode": "synchronous",
                     "timeout_seconds": 60}, separators=(",", ":"))]),
        ("cancel", "tasks/cancel", ["--task-id", "job_" + "0" * 32]),
        ("resource", "resources/read", ["--uri", "rust-quality-artifact://" + "0" * 32 + "/index"]),
    ]
    results = {}
    for label, method, extra in commands:
        results[label] = run_bounded(
            inspector_argv(observation, state, socket, method, extra),
            attempt, 180, attempt / f"inspector-{label}.json",
        )
        stdout = (attempt / f"inspector-{label}.stdout").read_text()
        stderr = (attempt / f"inspector-{label}.stderr").read_text()
        encoded = stdout.strip() or stderr.strip().splitlines()[-1]
        response = json.loads(encoded)
        payload = response.get("result", response)
        if label in {"discovery", "positive"} and results[label]["exit_code"] != 0:
            raise RuntimeError(f"Inspector {label} did not succeed")
        if label == "failure":
            error = response.get("error")
            if not (
                isinstance(payload, dict) and payload.get("isError") is True
                or isinstance(error, dict) and (
                    error.get("code") == -32602
                    or "project" in str(error.get("message", "")).lower()
                )
            ):
                raise RuntimeError("Inspector failure oracle was not observed")
        if label == "cancel":
            error = response.get("error", {})
            if error.get("code") not in {-32602, -32601} and not any(
                word in str(error.get("message", "")).lower()
                for word in ("task", "method")
            ):
                raise RuntimeError("Inspector unavailable cancellation oracle was not observed")
        if label == "resource" and not isinstance(response.get("error"), dict):
            raise RuntimeError("Inspector missing Resource oracle was not observed")
    discovery = json.loads((attempt / "inspector-discovery.stdout").read_text())
    tools = discovery.get("result", discovery).get("tools")
    if not isinstance(tools, list) or [item.get("name") for item in tools] != list(EXPECTED_TOOLS):
        raise RuntimeError("Inspector ordered inventory mismatch")
    bundle = INSPECTOR
    bridge = ROOT / "target/m1-17-inspector" / f"rust-mcp-{attempt.name}-bridge.mjs"
    suffix = b"\nexport { InspectorClient, createTransportNode };\n"
    try:
        bridge.write_bytes(bundle.read_bytes() + suffix)
        proxy_args = [
            sys.executable, str(pathlib.Path(__file__).resolve()), "proxy",
            "--client", "inspector", "--observation", str(observation),
            "--server-argv-json", json.dumps(server_argv(state, socket), separators=(",", ":")),
        ]
        session_env = {
            "RUST_MCP_M3_PASSING": str(ROOT / "fixtures/nextest/passing"),
            "RUST_MCP_M3_FAILING": str(ROOT / "fixtures/nextest/failing"),
            "RUST_MCP_M3_SLOW": str(ROOT / "fixtures/nextest/slow"),
        }
        previous = {key: os.environ.get(key) for key in session_env}
        os.environ.update(session_env)
        try:
            results["persistent_task_session"] = run_bounded(
                [str(NODE), str(ROOT / "scripts/m3-inspector-session.mjs"),
                 str(bridge), json.dumps(proxy_args, separators=(",", ":"))],
                attempt, 360, attempt / "inspector-task-session.json",
            )
        finally:
            for key, value in previous.items():
                if value is None:
                    os.environ.pop(key, None)
                else:
                    os.environ[key] = value
        if results["persistent_task_session"]["exit_code"] != 0:
            raise RuntimeError("Inspector persistent Tasks session did not succeed")
        task_session = json.loads(
            (attempt / "inspector-task-session.stdout").read_text()
        )
        if not all(task_session.get(key) is True for key in (
            "discovery", "positive", "failure", "cancel", "resource", "task_flow"
        )):
            raise RuntimeError("Inspector persistent Tasks oracle is incomplete")
    finally:
        bridge.unlink(missing_ok=True)
    return {
        "version": json.loads(INSPECTOR_PACKAGE.read_text())["version"],
        "bundle_sha256": file_digest(bundle),
        "bridge_suffix_sha256": digest(suffix),
        "commands": results,
        "persistent_task_session": task_session,
    }


def find_values(value: object, key: str) -> list[object]:
    found = []
    if isinstance(value, dict):
        for name, child in value.items():
            if name == key:
                found.append(child)
            found.extend(find_values(child, key))
    elif isinstance(value, list):
        for child in value:
            found.extend(find_values(child, key))
    return found


def codex_gate(attempt: pathlib.Path, socket: str, codex: pathlib.Path) -> dict[str, object]:
    """Drive the stock Codex app-server directly and with one model turn."""
    observation = attempt / "protocol.jsonl"
    state = attempt / "state-codex"
    state.mkdir(mode=0o700)
    proxy_args = json.dumps(server_argv(state, socket), separators=(",", ":"))
    controller_path = ROOT / "docs/validation/m1-17-codex-client/controller.py"
    specification = importlib.util.spec_from_file_location("m3_codex_controller", controller_path)
    if specification is None or specification.loader is None:
        raise RuntimeError("Codex app-server controller cannot be loaded")
    controller = importlib.util.module_from_spec(specification)
    specification.loader.exec_module(controller)
    # The workspace sandbox denies /bin/ps, which the reusable controller uses
    # only to enrich descendant-process evidence.  The transport already owns
    # a new process group and this gate independently verifies Docker labels,
    # so an empty process snapshot preserves cleanup without disabling a
    # product or protocol assertion.
    controller.Transport.procs = staticmethod(lambda: {})
    controller.TOOLS = EXPECTED_TOOLS
    controller.DISABLED_HOST_SERVERS = ()
    base_overrides = controller.overrides

    def codex_overrides(plan):
        values = base_overrides(plan)
        values["features.code_mode_host"] = True
        values["features.mcp_2026_07_28"] = True
        values["features.skip_host_skill_discovery"] = True
        return values

    controller.overrides = codex_overrides
    server_args = [
        str(pathlib.Path(__file__).resolve()), "proxy", "--client", "codex-app-server",
        "--observation", str(observation), "--server-argv-json", proxy_args,
    ]
    plan = {
        "codex": str(codex), "server_binary": sys.executable,
        "server_args": server_args, "model": "gpt-5.6-sol", "effort": "medium",
    }
    source_home = pathlib.Path(os.environ.get("CODEX_HOME", pathlib.Path.home() / ".codex"))
    auth_source = source_home / "auth.json"
    # The private Codex home holds a copy of auth.json plus the client's own
    # session databases.  That is credential material and per-attempt scratch
    # state, never evidence, so it lives in a temporary directory outside the
    # repository and is destroyed with the gate.  Only transcripts the gate
    # writes itself land under the attempt directory.
    private_home = pathlib.Path(tempfile.mkdtemp(prefix="rust-mcp-codex-home-"))
    os.chmod(private_home, 0o700)
    if not auth_source.is_file():
        shutil.rmtree(private_home, ignore_errors=True)
        raise RuntimeError("Codex auth.json is unavailable")
    shutil.copyfile(auth_source, private_home / "auth.json")
    os.chmod(private_home / "auth.json", 0o600)
    previous = os.environ.get("CODEX_HOME")
    os.environ["CODEX_HOME"] = str(private_home)
    transport = None
    try:
        transport = controller.Transport(controller.command(plan), attempt)
        controller.init(transport, attempt)
        started = controller.thread_start(transport, plan, attempt)
        thread_id = started.get("thread", {}).get("id")
        if not isinstance(thread_id, str):
            raise RuntimeError("Codex thread identity is absent")

        def call(name: str, arguments: dict[str, object], timeout: int = 360) -> dict:
            result = transport.rpc("mcpServer/tool/call", {
                "threadId": thread_id, "server": "rust_engineering",
                "tool": name, "arguments": arguments,
            }, timeout)
            if not isinstance(result, dict):
                raise RuntimeError(f"Codex returned a non-object for {name}")
            return result

        passing_open = call("rust.project.open", {"path": str(ROOT / "fixtures/nextest/passing")}, 60)
        references = [value for value in find_values(passing_open, "project_ref") if isinstance(value, str)]
        if len(set(references)) != 1:
            raise RuntimeError("Codex positive ProjectRef is ambiguous")
        passing = call("rust.test.nextest", {
            "project_ref": references[0], "execution_mode": "auto", "timeout_seconds": 60,
        })
        if passing.get("structuredContent", {}).get("status") != "passed":
            raise RuntimeError("Codex positive nextest call did not pass")
        uris = [value for value in find_values(passing, "uri")
                if isinstance(value, str) and value.startswith("rust-quality-artifact://")]
        if not uris:
            raise RuntimeError("Codex positive call did not publish a quality Resource")
        resource = transport.rpc("mcpServer/resource/read", {
            "threadId": thread_id, "server": "rust_engineering", "uri": uris[0],
        }, 60)
        controller.validate_resource(resource)

        failing_open = call("rust.project.open", {"path": str(ROOT / "fixtures/nextest/failing")}, 60)
        failing_references = [value for value in find_values(failing_open, "project_ref") if isinstance(value, str)]
        if len(set(failing_references)) != 1:
            raise RuntimeError("Codex failing ProjectRef is ambiguous")
        failing = call("rust.test.nextest", {
            "project_ref": failing_references[0], "execution_mode": "auto", "timeout_seconds": 60,
        })
        if failing.get("structuredContent", {}).get("status") != "failed":
            raise RuntimeError("Codex negative nextest call did not preserve test failure")

        prompt = f"""Use only the configured Rust Engineering MCP tools. Open
{ROOT / 'fixtures/nextest/passing'}, call rust.test.nextest with execution_mode auto
and timeout_seconds 60, then report the test status. Do not use any non-MCP capability."""
        turn = transport.rpc("turn/start", {
            "threadId": thread_id, "input": [{"type": "text", "text": prompt}],
        }, 30).get("turn", {})
        turn_id = turn.get("id")
        if not isinstance(turn_id, str):
            raise RuntimeError("Codex model turn did not start")
        completed = False
        observed_calls = set()
        model_events = attempt / "codex-model-events.jsonl"
        deadline = time.monotonic() + 600
        while time.monotonic() < deadline and not completed:
            try:
                event = transport.q.get(timeout=0.25)
            except queue.Empty:
                if transport.failure:
                    raise RuntimeError(transport.failure)
                continue
            append_observation(model_events, event)
            method = event.get("method")
            item = event.get("params", {}).get("item", {})
            if item.get("type") == "mcpToolCall" and isinstance(item.get("tool"), str):
                observed_calls.add(item["tool"])
            if method == "turn/completed" and event.get("params", {}).get("turn", {}).get("id") == turn_id:
                completed = True
        if not completed or not {"rust.project.open", "rust.test.nextest"}.issubset(observed_calls):
            raise RuntimeError("Codex model-directed MCP flow was not observed")
        return {
            "version": subprocess.run([str(codex), "--version"], capture_output=True,
                                      text=True, timeout=10, check=True).stdout.strip(),
            "model": "gpt-5.6-sol", "effort": "medium",
            "positive": True, "failure": True, "quality_resource_read": True,
            "model_turn_completed": True,
            "model_turn_tools": sorted(observed_calls),
            "tasks_declared": False,
            "task_cancel": "not supported by this client",
            "process_monitor": "process-group lifecycle plus Docker label hygiene; /bin/ps denied by workspace sandbox",
        }
    finally:
        try:
            if transport is not None:
                cleanup = transport.close()
                if not cleanup.get("cleanup_verified", False):
                    raise RuntimeError("Codex app-server cleanup was not verified")
        finally:
            if previous is None:
                os.environ.pop("CODEX_HOME", None)
            else:
                os.environ["CODEX_HOME"] = previous
            shutil.rmtree(private_home, ignore_errors=True)


CREDENTIAL_NAMES = ("auth.json", "installation_id", ".netrc", ".env", "credentials.json")


def assert_no_credentials(evidence: pathlib.Path) -> list[str]:
    """Fail the gate if the evidence directory holds credential material.

    Client homes carry OAuth tokens and session databases.  Evidence is only
    the transcripts this gate writes, so any credential-shaped file under an
    attempt means the harness staged secrets into the repository.
    """
    offenders = sorted(
        str(path.relative_to(evidence))
        for path in evidence.rglob("*")
        if path.is_file() and (
            path.name in CREDENTIAL_NAMES
            or ".sqlite" in path.name
        )
    )
    if offenders:
        raise RuntimeError(
            f"credential material staged into {evidence}: " + ", ".join(offenders)
        )
    return offenders


def protocol_summary(path: pathlib.Path, expected_advertised: bool) -> dict[str, object]:
    rows = [json.loads(line) for line in path.read_text().splitlines() if line]
    clients = {}
    for client in sorted({row.get("client") for row in rows if isinstance(row.get("client"), str)}):
        own = [row for row in rows if row.get("client") == client]
        modern_sessions = {
            row["session"] for row in own if row.get("method") == "server/discover"
        }
        declarations = [row["tasks_declared"] for row in own if "tasks_declared" in row]
        advertised = [row["tasks_advertised"] for row in own if "tasks_advertised" in row]
        modern_advertised = [
            row["tasks_advertised"] for row in own
            if row.get("session") in modern_sessions and "tasks_advertised" in row
        ]
        clients[client] = {
            "tasks_declared": sorted(set(declarations)),
            "tasks_advertised": sorted(set(advertised)),
            "methods": sorted({row["method"] for row in own if "method" in row}),
            "sessions": len({row["session"] for row in own}),
        }
        if not declarations or len(set(declarations)) != 1:
            raise RuntimeError(f"ambiguous Tasks declaration for {client}")
        if modern_sessions and set(modern_advertised) != {expected_advertised}:
            raise RuntimeError(f"unexpected modern Tasks advertisement for {client}")
    return {"clients": clients, "row_count": len(rows), "sha256": file_digest(path)}


def self_check() -> dict[str, object]:
    source = (ROOT / "crates/mcp-server/src/stdio.rs").read_text()
    advertised = "const TASKS_ADVERTISEMENT_READY: bool = true;" in source
    if not advertised and "const TASKS_ADVERTISEMENT_READY: bool = false;" not in source:
        raise RuntimeError("single Tasks advertisement switch is missing")
    if len(EXPECTED_TOOLS) != 22 or len(set(EXPECTED_TOOLS)) != 22:
        raise RuntimeError("closed tool inventory is invalid")
    return {
        "ready": True,
        "tasks_advertisement_ready": advertised,
        "tool_count": len(EXPECTED_TOOLS),
        "run_requires_switch": False,
        "runtime_clients": ["Inspector 2.5.0", "Codex CLI 0.153.0 app-server"],
        "codex_driver": "candidate-bound app-server transport reused from scripts/codex-model-qualifier.py evidence",
    }


def run(socket: str) -> int:
    source = (ROOT / "crates/mcp-server/src/stdio.rs").read_text()
    switch_enabled = "const TASKS_ADVERTISEMENT_READY: bool = true;" in source
    if not switch_enabled and "const TASKS_ADVERTISEMENT_READY: bool = false;" not in source:
        raise RuntimeError("single Tasks advertisement switch is missing")
    qualification_override = os.environ.get("RUST_MCP_TEST_TASKS_READY") == "1"
    advertised = switch_enabled or qualification_override
    codex_name = shutil.which("codex")
    codex = pathlib.Path(codex_name) if codex_name else pathlib.Path("/nonexistent/codex")
    required = (SERVER, NODE, INSPECTOR, INSPECTOR_PACKAGE, codex, DOCKER)
    missing = [str(path) for path in required if not path.is_file()]
    if missing:
        raise RuntimeError("missing prerequisites: " + ", ".join(missing))
    if json.loads(INSPECTOR_PACKAGE.read_text()).get("version") != "2.5.0":
        raise RuntimeError("Inspector version is not 2.5.0")
    attempt = next_attempt()
    receipt: dict[str, object] = {
        "schema": "rust-mcp-m3-clients-v1", "status": "failed",
        "image_id": IMAGE, "attempt": attempt.name,
        "tasks_advertised": advertised,
        "advertisement_switch_enabled": switch_enabled,
        "qualification_override": qualification_override,
        "candidate": {"server_sha256": file_digest(SERVER)},
    }
    try:
        receipt["inspector"] = inspector_gate(attempt, socket)
        receipt["codex_app_server"] = codex_gate(attempt, socket, codex)
        receipt["protocol"] = protocol_summary(attempt / "protocol.jsonl", advertised)
        clients = receipt["protocol"]["clients"]
        receipt["client_matrix"] = {
            "inspector": {
                "declares_tasks": clients["inspector"]["tasks_declared"] == [True],
                "discovery": True,
                "positive": True,
                "failure": True,
                "cancel": True,
                "resource": True,
                "task_flow": receipt["inspector"]["persistent_task_session"]["task_flow"],
            },
            "codex_app_server": {
                "declares_tasks": clients["codex-app-server"]["tasks_declared"] == [True],
                "discovery": True,
                "positive": True,
                "failure": True,
                "cancel": False,
                "resource": True,
                "task_flow": False,
            },
        }
        receipt["status"] = "passed"
    except Exception as error:
        receipt["error"] = {"type": type(error).__name__, "message": str(error)}
        raise
    finally:
        # The evidence directory must never hold credential material, whether
        # the gate passed or failed.  A leak demotes the attempt so no current
        # receipt is published from it.
        leak = None
        try:
            assert_no_credentials(attempt)
            receipt["evidence_credential_scan"] = "clean"
        except RuntimeError as scan_error:
            leak = scan_error
            receipt["evidence_credential_scan"] = str(scan_error)
            receipt["status"] = "failed"
        save_json(attempt / "receipt.json", receipt, exclusive=True)
        if leak is not None:
            raise leak
        if receipt["status"] == "passed":
            if CURRENT.exists():
                raise RuntimeError("current client receipt exists; preserve it before rerun")
            save_json(CURRENT, receipt, exclusive=True)
    return 0


def main() -> int:
    parser = argparse.ArgumentParser()
    subcommands = parser.add_subparsers(dest="command")
    proxy_parser = subcommands.add_parser("proxy")
    proxy_parser.add_argument("--client", required=True)
    proxy_parser.add_argument("--observation", type=pathlib.Path, required=True)
    proxy_parser.add_argument("--server-argv-json", required=True)
    parser.add_argument("--run", action="store_true")
    parser.add_argument("--docker-socket", default=os.environ.get("RUST_MCP_TEST_SOCKET"))
    options = parser.parse_args()
    if options.command == "proxy":
        argv = json.loads(options.server_argv_json)
        if not isinstance(argv, list) or not argv or any(not isinstance(item, str) for item in argv):
            raise RuntimeError("invalid closed server argv")
        return proxy(argv, options.observation, options.client)
    if not options.run:
        print(json.dumps(self_check(), sort_keys=True))
        return 0
    if not options.docker_socket or not pathlib.Path(options.docker_socket).is_absolute():
        raise RuntimeError("an absolute RUST_MCP_TEST_SOCKET is required")
    return run(options.docker_socket)


if __name__ == "__main__":
    raise SystemExit(main())
