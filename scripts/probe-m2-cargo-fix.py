#!/usr/bin/env python3
"""Qualify fixed Cargo 1.98.1 `cargo fix` operation in disposable D04 objects."""

import base64
import datetime
import hashlib
import io
import json
from pathlib import Path
import platform
import secrets
import subprocess
import sys
import tarfile
import tempfile
import time


ROOT = Path(__file__).resolve().parent.parent
DOCKER = Path("/usr/local/bin/docker")
SOCKET = Path("/Users/cburgosro/.docker/run/docker.sock")
IMAGE = "sha256:8fac70723a8d04b6ec9633ab721806b8a55f4f083a1b3f988c61bf6a00fa1909"
SECCOMP = ROOT / "crates/execution-adapter/src/seccomp-rust.json"
REPORT = ROOT / "docs/validation/M2-D06-cargo-fix-qualification.json"
SUMMARY = ROOT / "docs/validation/M2-D06-cargo-fix-qualification.md"
TIMEOUT = 30
REQUESTED_ARGV = [
    "/opt/rust/bin/cargo", "fix", "--workspace", "--all-targets", "--default-features",
    "--frozen", "--offline", "--allow-no-vcs", "--allow-dirty", "--allow-staged",
    "--message-format=json", "--color", "never", "--target-dir", "/target",
]
FIX_ARGV = [value for value in REQUESTED_ARGV if value != "--default-features"]


def sha256(data):
    return "sha256:" + hashlib.sha256(data).hexdigest()


def encoded(data):
    return {"length": len(data), "sha256": sha256(data),
            "base64": base64.b64encode(data).decode("ascii")}


def archive(files):
    stream = io.BytesIO()
    with tarfile.open(fileobj=stream, mode="w", format=tarfile.USTAR_FORMAT) as output:
        directories = sorted({"/".join(name.split("/")[:i]) for name in files
                              for i in range(1, len(name.split("/")))})
        for name in directories:
            info = tarfile.TarInfo(name)
            info.type = tarfile.DIRTYPE
            info.mode = 0o700
            info.uid = info.gid = 65534
            info.mtime = 0
            output.addfile(info)
        for name, data in sorted(files.items()):
            info = tarfile.TarInfo(name)
            info.size = len(data)
            info.mode = 0o600
            info.uid = info.gid = 65534
            info.mtime = 0
            output.addfile(info, io.BytesIO(data))
    return stream.getvalue()


def parse_export(data):
    if len(data) > 4 * 1024 * 1024:
        raise AssertionError("source export exceeded qualification bound")
    files = {}
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:") as source:
        members = source.getmembers()
        if len(members) > 128:
            raise AssertionError("source export entry bound exceeded")
        for member in members:
            name = member.name.removeprefix("./").rstrip("/")
            if not name or member.isdir():
                continue
            if (not member.isfile() or member.issym() or member.islnk()
                    or name.startswith("/") or ".." in name.split("/")):
                raise AssertionError(f"unsafe export entry: {member.name!r}")
            handle = source.extractfile(member)
            value = b"" if handle is None else handle.read()
            if len(value) != member.size or len(value) > 1024 * 1024 or name in files:
                raise AssertionError(f"invalid export entry: {member.name!r}")
            files[name] = value
    return files


class Probe:
    def __init__(self, config, nonce):
        self.prefix = [str(DOCKER), "--config", str(config), "--host", f"unix://{SOCKET}"]
        self.nonce = nonce
        self.label = f"org.rust-mcp.m2-d06={nonce}"
        self.seccomp = SECCOMP
        self.containers = []
        self.volumes = []
        self.events = []
        self.observations = []

    def run(self, args, stdin=b"", timeout=TIMEOUT, binary=False):
        command = self.prefix + list(args)
        started = time.perf_counter_ns()
        try:
            result = subprocess.run(command, input=stdin, stdout=subprocess.PIPE,
                                    stderr=subprocess.PIPE, timeout=timeout, check=False,
                                    env={"PATH": "/usr/bin:/bin", "LANG": "C", "LC_ALL": "C"})
            event = {"argv": command, "exit_code": result.returncode,
                     "stdout": encoded(result.stdout), "stderr": encoded(result.stderr),
                     "duration_ns": time.perf_counter_ns() - started}
            if not binary:
                event["stdout_utf8"] = result.stdout.decode("utf-8", "replace")
                event["stderr_utf8"] = result.stderr.decode("utf-8", "replace")
            self.events.append(event)
            return result
        except subprocess.TimeoutExpired as error:
            self.events.append({"argv": command, "timeout_seconds": timeout,
                                "stdout": encoded(error.stdout or b""),
                                "stderr": encoded(error.stderr or b""),
                                "duration_ns": time.perf_counter_ns() - started})
            raise

    def ok(self, args, **kwargs):
        result = self.run(args, **kwargs)
        if result.returncode:
            raise RuntimeError(f"Docker command failed: {args!r}")
        return result

    def require(self, condition, case, **facts):
        self.observations.append({"case": case, "matched": bool(condition), **facts})
        if not condition:
            raise AssertionError(case)

    def create_volume(self, suffix):
        name = f"rust-mcp-m2-d06-{self.nonce}-{suffix}"
        options = "size=64m,nr_inodes=1024,uid=65534,gid=65534,mode=0700,nosuid,nodev,noexec"
        self.ok(["volume", "create", "--driver=local", "--opt=type=tmpfs",
                 "--opt=device=tmpfs", f"--opt=o={options}", f"--label={self.label}", name])
        self.volumes.append(name)
        item = json.loads(self.ok(["volume", "inspect", name]).stdout)[0]
        self.require(item["Options"] == {"type": "tmpfs", "device": "tmpfs", "o": options}
                     and item["Labels"] == {"org.rust-mcp.m2-d06": self.nonce},
                     f"{suffix}_volume_hardened", inspect_sha256=self.events[-1]["stdout"]["sha256"])
        return name

    def create(self, suffix, volume, entrypoint, arguments, readonly, interactive=False,
               target=False):
        name = f"rust-mcp-m2-d06-{self.nonce}-{suffix}"
        args = ["container", "create", f"--name={name}", "--pull=never", "--runtime=runc",
                "--init=false", "--network=none", "--read-only", "--cap-drop=ALL",
                "--security-opt=no-new-privileges=true", f"--security-opt=seccomp={self.seccomp}",
                "--ipc=private", "--cgroupns=private", "--pids-limit=128", "--cpus=1",
                "--memory=1g", "--memory-swap=1g", "--shm-size=1m", "--log-driver=none",
                "--no-healthcheck", "--tmpfs=/work:rw,exec,nosuid,nodev,size=64m,mode=0700,uid=65534,gid=65534",
                "--tmpfs=/tmp:rw,nosuid,nodev,noexec,size=16m,mode=0700,uid=65534,gid=65534", "--workdir=/source",
                "--hostname=sandbox", "--user=65534:65534", f"--label={self.label}",
                "--env=PATH=/opt/rust/bin:/usr/bin:/bin", "--env=HOME=/work",
                "--env=TMPDIR=/tmp", "--env=CARGO_HOME=/work/cargo",
                "--env=CARGO_INCREMENTAL=0", "--env=CARGO_NET_OFFLINE=true",
                "--env=RUSTC=/opt/rust/bin/rustc", "--env=RUSTDOC=/opt/rust/bin/rustdoc",
                "--env=RUSTFMT=/opt/rust/bin/rustfmt",
                f"--mount=type=volume,source={volume},target=/source,"
                f"{'readonly,' if readonly else ''}volume-nocopy,volume-driver=local"]
        if target:
            args.append("--tmpfs=/target:rw,exec,nosuid,nodev,size=256m,mode=0700,uid=65534,gid=65534")
        if interactive:
            args.append("--interactive")
        args.extend([f"--entrypoint={entrypoint}", IMAGE, *arguments])
        self.ok(args)
        self.containers.append(name)
        item = json.loads(self.ok(["container", "inspect", name]).stdout)[0]
        host, config = item["HostConfig"], item["Config"]
        mount = next(value for value in item["Mounts"] if value["Destination"] == "/source")
        safe = (item["Image"] == IMAGE and config["User"] == "65534:65534"
                and config["Entrypoint"] == [entrypoint] and config["Cmd"] == arguments
                and host["NetworkMode"] == "none" and host["ReadonlyRootfs"] is True
                and host["Privileged"] is False and host["CapDrop"] == ["ALL"]
                and not host.get("CapAdd") and host["PidsLimit"] == 128
                and host["Memory"] == 1073741824 and host["MemorySwap"] == 1073741824
                and host["NanoCpus"] == 1000000000 and host["Binds"] is None
                and mount["Name"] == volume and mount["RW"] is (not readonly)
                and (not target or "/target" in host["Tmpfs"]))
        self.require(safe, f"{suffix}_container_hardened",
                     container_id=item["Id"], inspect_sha256=self.events[-1]["stdout"]["sha256"])
        return name

    def start(self, name, stdin=b"", binary=False, timeout=TIMEOUT):
        args = ["container", "start", "--attach"]
        if stdin:
            args.append("--interactive")
        return self.run([*args, name], stdin=stdin, binary=binary, timeout=timeout)

    def remove_container(self, name):
        self.ok(["container", "rm", "--force", name])
        if name in self.containers:
            self.containers.remove(name)
        absent = self.ok(["container", "ls", "--all", "--filter", f"name=^/{name}$",
                          "--format", "{{.ID}}"])
        self.require(not absent.stdout.strip(), f"{name}_removed")

    def remove_volume(self, name):
        self.ok(["volume", "rm", name])
        if name in self.volumes:
            self.volumes.remove(name)
        absent = self.ok(["volume", "ls", "--filter", f"name=^{name}$",
                          "--format", "{{.Name}}"])
        self.require(not absent.stdout.strip(), f"{name}_removed")

    def ingest(self, suffix, volume, files):
        payload = archive(files)
        name = self.create(suffix, volume, "/usr/bin/tar",
                           ["--extract", "--file=-", "--directory=/source", "--no-same-owner",
                            "--no-same-permissions", "--keep-old-files"], False, True)
        result = self.start(name, payload)
        self.require(result.returncode == 0, f"{suffix}_ingest", archive=encoded(payload),
                     stderr=result.stderr.decode("utf-8", "replace"))
        self.remove_container(name)

    def guardian(self, suffix, volume):
        name = self.create(suffix, volume, "/usr/bin/sleep", ["900"], True)
        self.ok(["container", "start", name])
        state = json.loads(self.ok(["container", "inspect", name]).stdout)[0]["State"]
        self.require(state["Running"] is True, f"{suffix}_guardian_running", pid=state["Pid"])
        return name

    def export(self, suffix, volume):
        name = self.create(suffix, volume, "/usr/bin/tar",
                           ["--create", "--file=-", "--format=ustar", "--sort=name",
                            "--one-file-system", "--directory=/source", "."], True)
        result = self.start(name, binary=True)
        self.require(result.returncode == 0 and not result.stderr, f"{suffix}_export",
                     archive=encoded(result.stdout))
        self.remove_container(name)
        return parse_export(result.stdout)

    def inventory(self):
        containers = self.ok(["container", "ls", "--all", "--filter", f"label={self.label}",
                              "--format", "{{.Names}}"] ).stdout.decode().splitlines()
        volumes = self.ok(["volume", "ls", "--filter", f"label={self.label}",
                           "--format", "{{.Name}}"] ).stdout.decode().splitlines()
        return {"containers": containers, "volumes": volumes}

    def cleanup(self):
        errors = []
        for name in reversed(self.containers[:]):
            try: self.remove_container(name)
            except Exception as error: errors.append(f"container {name}: {error}")
        for name in reversed(self.volumes[:]):
            try: self.remove_volume(name)
            except Exception as error: errors.append(f"volume {name}: {error}")
        try: inventory = self.inventory()
        except Exception as error:
            inventory = {"inventory_error": str(error)}
            errors.append(str(error))
        return inventory, errors


MANIFEST = (b"[package]\nname = \"fix-probe\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"
            b"\n[features]\ndefault = [\"enabled\"]\nenabled = []\n")
LOCK = b"# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n\n[[package]]\nname = \"fix-probe\"\nversion = \"0.1.0\"\n"
BEFORE = (b"#[cfg(not(feature = \"enabled\"))]\ncompile_error!(\"default feature disabled\");\n\n"
          b"pub fn answer() -> u32 {\n    let mut value = 42;\n    value\n}\n")
AFTER = (b"#[cfg(not(feature = \"enabled\"))]\ncompile_error!(\"default feature disabled\");\n\n"
         b"pub fn answer() -> u32 {\n    let value = 42;\n    value\n}\n")


def compiler_messages(stdout):
    rows = []
    for line in stdout.splitlines():
        try: value = json.loads(line)
        except json.JSONDecodeError: continue
        if value.get("reason") == "compiler-message":
            message = value.get("message", {})
            rows.append({"level": message.get("level"), "code": (message.get("code") or {}).get("code"),
                         "message": message.get("message"),
                         "applicability": sorted({child.get("suggestion_applicability")
                                                  for child in message.get("spans", [])
                                                  if child.get("suggestion_applicability")})})
    return rows


def positive(probe):
    volume = probe.create_volume("positive")
    keeper = probe.guardian("positive-guardian", volume)
    initial = {"Cargo.toml": MANIFEST, "Cargo.lock": LOCK, "src/lib.rs": BEFORE}
    probe.ingest("positive-ingest", volume, initial)
    name = probe.create("positive-fix", volume, FIX_ARGV[0], FIX_ARGV[1:], False, target=True)
    result = probe.start(name)
    messages = compiler_messages(result.stdout.decode("utf-8", "strict"))
    probe.require(result.returncode == 0, "cargo_fix_succeeded", exit_code=result.returncode,
                  stdout=encoded(result.stdout), stderr=encoded(result.stderr), messages=messages)
    probe.remove_container(name)
    running = probe.ok(["container", "ls", "--filter", f"label={probe.label}",
                        "--format", "{{.Names}}"] ).stdout.decode().splitlines()
    probe.require(running == [keeper], "no_mutator_before_export", running=running)
    files = probe.export("positive-export", volume)
    changed = sorted(path for path in set(initial) | set(files) if initial.get(path) != files.get(path))
    probe.require(files.get("src/lib.rs") == AFTER and files.get("Cargo.lock") == LOCK
                  and changed == ["src/lib.rs"], "only_rust_source_changed",
                  changed=changed, before_sha256=sha256(BEFORE), after_sha256=sha256(files.get("src/lib.rs", b"")),
                  lock_sha256=sha256(files.get("Cargo.lock", b"")))
    probe.remove_container(keeper)
    probe.remove_volume(volume)
    return {"exit_code": result.returncode, "compiler_messages": messages, "changed_files": changed,
            "source_before": BEFORE.decode(), "source_after": files["src/lib.rs"].decode(),
            "lock_sha256": sha256(LOCK)}


def baseline_socket_denial(probe):
    volume = probe.create_volume("socket-denial")
    keeper = probe.guardian("socket-denial-guardian", volume)
    probe.ingest("socket-denial-ingest", volume,
                 {"Cargo.toml": MANIFEST, "Cargo.lock": LOCK, "src/lib.rs": BEFORE})
    name = probe.create("socket-denial-fix", volume, FIX_ARGV[0], FIX_ARGV[1:], False, target=True)
    result = probe.start(name)
    stderr = result.stderr.decode("utf-8", "replace")
    probe.require(result.returncode != 0 and "failed to bind TCP listener to manage locking" in stderr
                  and "Operation not permitted" in stderr,
                  "production_seccomp_denied_cargo_lock_tcp", exit_code=result.returncode,
                  stdout=encoded(result.stdout), stderr=encoded(result.stderr))
    probe.remove_container(name)
    files = probe.export("socket-denial-export", volume)
    probe.require(files.get("src/lib.rs") == BEFORE and files.get("Cargo.lock") == LOCK,
                  "socket_denial_changed_nothing", paths=sorted(files))
    probe.remove_container(keeper)
    probe.remove_volume(volume)
    return {"exit_code": result.returncode, "stderr": stderr, "changed_files": []}


TCP_OPERATIONS = ["bind", "connect", "listen", "accept4", "getsockname", "setsockopt", "shutdown"]


def private_tcp_profile(path, operations=TCP_OPERATIONS):
    profile = json.loads(SECCOMP.read_text())
    profile["syscalls"].extend([
        {"names": ["socket"], "action": "SCMP_ACT_ALLOW", "args": [
            {"index": 0, "value": 2, "op": "SCMP_CMP_EQ"},
            {"index": 1, "value": 1, "valueTwo": 15, "op": "SCMP_CMP_MASKED_EQ"},
            {"index": 2, "value": 0, "op": "SCMP_CMP_EQ"},
        ]},
        {"names": list(operations), "action": "SCMP_ACT_ALLOW"},
    ])
    data = (json.dumps(profile, indent=2, sort_keys=True) + "\n").encode()
    path.write_bytes(data)
    path.chmod(0o600)
    return {"path": str(path), "sha256": sha256(data),
            "socket_constraints": ["AF_INET", "SOCK_STREAM masked by 0xf", "protocol 0 (TCP for stream)"],
            "socket_operations": list(operations)}


def profile_failure(probe, suffix, expected):
    volume = probe.create_volume(suffix)
    keeper = probe.guardian(f"{suffix}-guardian", volume)
    initial = {"Cargo.toml": MANIFEST, "Cargo.lock": LOCK, "src/lib.rs": BEFORE}
    probe.ingest(f"{suffix}-ingest", volume, initial)
    name = probe.create(f"{suffix}-fix", volume, FIX_ARGV[0], FIX_ARGV[1:], False, target=True)
    result = probe.start(name)
    stderr = result.stderr.decode("utf-8", "replace")
    probe.require(result.returncode != 0 and expected in stderr, f"{suffix}_expected_failure",
                  exit_code=result.returncode, stdout=encoded(result.stdout), stderr=encoded(result.stderr))
    probe.remove_container(name)
    files = probe.export(f"{suffix}-export", volume)
    changed = sorted(path for path in set(initial) | set(files) if initial.get(path) != files.get(path))
    probe.remove_container(keeper)
    probe.remove_volume(volume)
    return {"exit_code": result.returncode, "stderr": stderr, "changed_files": changed}


def unsupported_default_features(probe):
    volume = probe.create_volume("default-features")
    keeper = probe.guardian("default-features-guardian", volume)
    probe.ingest("default-features-ingest", volume,
                 {"Cargo.toml": MANIFEST, "Cargo.lock": LOCK, "src/lib.rs": BEFORE})
    name = probe.create("default-features-fix", volume, REQUESTED_ARGV[0],
                        REQUESTED_ARGV[1:], False, target=True)
    result = probe.start(name)
    stderr = result.stderr.decode("utf-8", "replace")
    probe.require(result.returncode != 0 and "unexpected argument '--default-features'" in stderr,
                  "literal_default_features_flag_rejected", exit_code=result.returncode,
                  stdout=encoded(result.stdout), stderr=encoded(result.stderr))
    probe.remove_container(name)
    files = probe.export("default-features-export", volume)
    probe.require(files.get("src/lib.rs") == BEFORE and files.get("Cargo.lock") == LOCK,
                  "rejected_flag_changed_nothing", paths=sorted(files))
    probe.remove_container(keeper)
    probe.remove_volume(volume)
    return {"exit_code": result.returncode, "stderr": stderr, "changed_files": []}


def missing_lock(probe):
    volume = probe.create_volume("missing-lock")
    keeper = probe.guardian("missing-lock-guardian", volume)
    probe.ingest("missing-lock-ingest", volume, {"Cargo.toml": MANIFEST, "src/lib.rs": BEFORE})
    name = probe.create("missing-lock-fix", volume, FIX_ARGV[0], FIX_ARGV[1:], False, target=True)
    result = probe.start(name)
    probe.require(result.returncode != 0, "frozen_missing_lock_failed", exit_code=result.returncode,
                  stdout=encoded(result.stdout), stderr=encoded(result.stderr))
    probe.remove_container(name)
    files = probe.export("missing-lock-export", volume)
    probe.require("Cargo.lock" not in files and files.get("src/lib.rs") == BEFORE,
                  "frozen_did_not_generate_lock_or_edit", paths=sorted(files))
    probe.remove_container(keeper)
    probe.remove_volume(volume)
    return {"exit_code": result.returncode, "stderr": result.stderr.decode("utf-8", "replace"),
            "lock_generated": "Cargo.lock" in files}


def cancellation(probe):
    volume = probe.create_volume("cancel")
    keeper = probe.guardian("cancel-guardian", volume)
    build = b"fn main() { std::thread::sleep(std::time::Duration::from_secs(60)); }\n"
    manifest = MANIFEST.replace(b"\n[features]", b"\nbuild = \"build.rs\"\n\n[features]")
    probe.ingest("cancel-ingest", volume,
                 {"Cargo.toml": manifest, "Cargo.lock": LOCK, "build.rs": build, "src/lib.rs": BEFORE})
    name = probe.create("cancel-fix", volume, FIX_ARGV[0], FIX_ARGV[1:], False, target=True)
    probe.ok(["container", "start", name])
    deadline = time.monotonic() + 15
    running = False
    while time.monotonic() < deadline:
        state = json.loads(probe.ok(["container", "inspect", name]).stdout)[0]["State"]
        if state["Running"]:
            running = True
            break
        time.sleep(0.05)
    probe.require(running, "cancel_mutator_observed_running")
    stopped = probe.ok(["container", "stop", "--timeout=1", name])
    state = json.loads(probe.ok(["container", "inspect", name]).stdout)[0]["State"]
    probe.require(not state["Running"], "cancel_terminated_container_tree",
                  stop_stdout=stopped.stdout.decode().strip(), exit_code=state["ExitCode"],
                  oom_killed=state["OOMKilled"])
    probe.remove_container(name)
    running_names = probe.ok(["container", "ls", "--filter", f"label={probe.label}",
                              "--format", "{{.Names}}"] ).stdout.decode().splitlines()
    probe.require(running_names == [keeper], "cancel_left_no_mutator", running=running_names)
    probe.remove_container(keeper)
    probe.remove_volume(volume)
    return {"container_exit_code": state["ExitCode"], "oom_killed": state["OOMKilled"]}


def timeout_cleanup(probe):
    volume = probe.create_volume("timeout")
    keeper = probe.guardian("timeout-guardian", volume)
    build = b"fn main() { std::thread::sleep(std::time::Duration::from_secs(60)); }\n"
    manifest = MANIFEST.replace(b"\n[features]", b"\nbuild = \"build.rs\"\n\n[features]")
    probe.ingest("timeout-ingest", volume,
                 {"Cargo.toml": manifest, "Cargo.lock": LOCK, "build.rs": build, "src/lib.rs": BEFORE})
    name = probe.create("timeout-fix", volume, FIX_ARGV[0], FIX_ARGV[1:], False, target=True)
    timed_out = False
    try:
        probe.start(name, timeout=2)
    except subprocess.TimeoutExpired:
        timed_out = True
    state = json.loads(probe.ok(["container", "inspect", name]).stdout)[0]["State"]
    probe.require(timed_out and state["Running"], "control_timeout_observed_live_mutator",
                  running=state["Running"], pid=state["Pid"])
    probe.ok(["container", "stop", "--timeout=1", name])
    state = json.loads(probe.ok(["container", "inspect", name]).stdout)[0]["State"]
    probe.require(not state["Running"] and not state["OOMKilled"], "timeout_cleanup_terminated_tree",
                  exit_code=state["ExitCode"], oom_killed=state["OOMKilled"])
    probe.remove_container(name)
    running = probe.ok(["container", "ls", "--filter", f"label={probe.label}",
                        "--format", "{{.Names}}"] ).stdout.decode().splitlines()
    probe.require(running == [keeper], "timeout_left_no_mutator", running=running)
    probe.remove_container(keeper)
    probe.remove_volume(volume)
    return {"control_timeout_seconds": 2, "container_exit_code": state["ExitCode"],
            "oom_killed": state["OOMKilled"]}


def write_summary(report):
    positive_result = report.get("experiments", {}).get("positive", {})
    missing = report.get("experiments", {}).get("missing_lock", {})
    cancel = report.get("experiments", {}).get("cancellation", {})
    cargo_version = report.get('cargo_version', 'unknown').splitlines()[0]
    text = f"""# M2 D06 Cargo fix qualification

Status: **{report['product_gate']}**. This is runtime qualification evidence, not an ADR or production implementation.

## Qualified invocation

`{' '.join(FIX_ARGV)}`

The initially proposed literal `--default-features` spelling was rejected by Cargo 1.98.1. Default features are selected by omitting both `--no-default-features` and `--all-features`.

The approved {cargo_version} image failed under the unchanged `seccomp-rust.json` profile because Cargo binds a TCP listener for locking. The positive run used the recorded private experimental profile, `--network=none`, a read-only container root, a bounded writable source tmpfs, and an isolated executable `/target` tmpfs.

## Results

- Positive fixture exit: `{positive_result.get('exit_code')}`; changed paths: `{positive_result.get('changed_files')}`.
- Existing `Cargo.lock` remained `{positive_result.get('lock_sha256')}`.
- Missing lock exit: `{missing.get('exit_code')}`; generated lock: `{missing.get('lock_generated')}`.
- Cancellation exit: `{cancel.get('container_exit_code')}`; OOM: `{cancel.get('oom_killed')}`.
- Control timeout cleanup exit: `{report.get('experiments', {}).get('timeout_cleanup', {}).get('container_exit_code')}`.
- Final owned-object inventory: `{report.get('final_cleanup_inventory')}`.

The experimental profile admits AF_INET stream sockets with protocol 0 and `bind`, `connect`, `listen`, `accept4`, `getsockname`, `setsockopt`, and `shutdown`. The runs directly discriminate the initial socket denial and that `setsockopt` and `shutdown` are required. The remaining operations were qualified as a group and were not individually minimized. The result qualifies this exact command and fixture only. It does not prove behavior for dependency-bearing workspaces, build scripts other than the fixed cancellation fixture, proc macros, every compiler diagnostic, or all Docker platforms. `--network=none` records namespace isolation and external-interface removal; it does not deny loopback TCP.

## Official sources

- [Cargo fix command](https://doc.rust-lang.org/nightly/cargo/commands/cargo-fix.html) documents target, feature, VCS, frozen, and offline behavior.
- [Cargo fix implementation at the runtime commit](https://github.com/rust-lang/cargo/blob/797e8a9bca276c1c9f9f738d2a20f484fa4eea9d/src/cargo/ops/fix/mod.rs) shows the TCP lock client and bounded iterative rustc/rustfix execution used by Cargo 1.98.1.
"""
    SUMMARY.write_text(text)


def main():
    started = time.perf_counter_ns()
    nonce = secrets.token_hex(8)
    report = {"started_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
              "scope": "D06 Cargo fix disposable qualification; no production or ADR change",
              "product_gate": "inconclusive", "platform": platform.platform(),
              "machine": platform.machine(), "approved_image": IMAGE,
              "docker_socket": str(SOCKET), "requested_argv": REQUESTED_ARGV,
              "qualified_argv": FIX_ARGV,
              "seccomp_sha256": sha256(SECCOMP.read_bytes()),
              "script_sha256": sha256(Path(__file__).read_bytes()), "run_nonce": nonce}
    status = 70
    probe = None
    with tempfile.TemporaryDirectory(prefix="rust-mcp-m2-d06-config-") as config_name:
        config = Path(config_name)
        config.chmod(0o700)
        probe = Probe(config, nonce)
        try:
            report["docker_version"] = json.loads(probe.ok(["version", "--format", "{{json .}}"] ).stdout)
            image = json.loads(probe.ok(["image", "inspect", IMAGE]).stdout)[0]
            probe.require(image["Id"] == IMAGE and image["Os"] == "linux" and image["Architecture"] == "arm64",
                          "approved_image_identity", image_id=image["Id"])
            version_volume = probe.create_volume("version")
            version_name = probe.create("version", version_volume, "/opt/rust/bin/cargo",
                                        ["--version", "--verbose"], True)
            version = probe.start(version_name)
            probe.require(version.returncode == 0, "cargo_version", stdout=version.stdout.decode())
            report["cargo_version"] = version.stdout.decode().strip()
            probe.remove_container(version_name)
            probe.remove_volume(version_volume)
            experiments = {
                "unsupported_default_features": unsupported_default_features(probe),
                "production_seccomp_denial": baseline_socket_denial(probe),
            }
            no_setsockopt = config / "seccomp-cargo-fix-no-setsockopt.json"
            variants = {}
            variants["without_setsockopt"] = private_tcp_profile(
                no_setsockopt, [value for value in TCP_OPERATIONS if value != "setsockopt"])
            probe.seccomp = no_setsockopt
            experiments["setsockopt_required"] = profile_failure(
                probe, "no-setsockopt", "failed to bind TCP listener to manage locking")
            no_shutdown = config / "seccomp-cargo-fix-no-shutdown.json"
            variants["without_shutdown"] = private_tcp_profile(
                no_shutdown, [value for value in TCP_OPERATIONS if value != "shutdown"])
            probe.seccomp = no_shutdown
            experiments["shutdown_required"] = profile_failure(
                probe, "no-shutdown", "failed to shutdown")
            private_profile = config / "seccomp-cargo-fix-experiment.json"
            report["experimental_seccomp"] = private_tcp_profile(private_profile)
            report["experimental_seccomp_variants"] = variants
            probe.seccomp = private_profile
            experiments.update({"positive": positive(probe), "missing_lock": missing_lock(probe),
                                "cancellation": cancellation(probe),
                                "timeout_cleanup": timeout_cleanup(probe)})
            report["experiments"] = experiments
            report["experiment_status"] = "observations_matched"
            report["product_gate"] = "exact_fixture_qualified_not_accepted"
            status = 0
        except Exception as error:
            report["experiment_status"] = "infrastructure_or_observation_failure"
            report["error_type"] = type(error).__name__
            report["error"] = str(error)
            status = 1 if isinstance(error, AssertionError) else 70
        finally:
            inventory, cleanup_errors = probe.cleanup()
            report["final_cleanup_inventory"] = inventory
            report["cleanup_errors"] = cleanup_errors
            report["observations"] = probe.observations
            if inventory != {"containers": [], "volumes": []} or cleanup_errors:
                report["experiment_status"] = "cleanup_uncertain"
                report["product_gate"] = "inconclusive"
                status = 70
    report["events"] = probe.events
    report["finished_at_utc"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    report["total_duration_ns"] = time.perf_counter_ns() - started
    report["limitations"] = [
        "This qualifies only the exact fixed argv and fixtures; it does not accept D06 or implement fix.apply.",
        "No host path was mounted and no host Cargo, pull, install, image build, or external network was used.",
        "The unchanged seccomp-rust profile rejects Cargo's TCP locking listener; the private profile is experiment-only.",
        "Only setsockopt and shutdown were individually removed; other admitted TCP operations were qualified as a group.",
        "Docker network=none isolates external interfaces but does not by itself deny every loopback syscall.",
        "Cancellation was forced during a fixed benign build script sleep and verifies container removal, not every daemon failure mode.",
    ]
    REPORT.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    write_summary(report)
    print(json.dumps({"status": report["experiment_status"], "gate": report["product_gate"],
                      "cleanup": report["final_cleanup_inventory"]}, sort_keys=True))
    return status


if __name__ == "__main__":
    sys.exit(main())
