#!/usr/bin/env python3
"""Run only the closed, reviewed benign fixture allowlist; not a sandbox."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import selectors
import signal
import subprocess
import tempfile
import time
import tomllib

if not __debug__:
    raise RuntimeError("Fixture verification rejects optimized Python mode")

LIMIT = 512 * 1024
DEADLINE = 30
BENIGN = frozenset({"valid-basic", "borrow-error", "lifetime-error", "clippy-warning",
                    "unsafe", "workspace", "feature-conflict", "build-script"})


def run(argv, cwd, env):
    process = subprocess.Popen(argv, cwd=cwd, env=env, stdin=subprocess.DEVNULL,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                               start_new_session=True)
    output = {"stdout": bytearray(), "stderr": bytearray()}
    deadline = time.monotonic() + DEADLINE
    try:
        with selectors.DefaultSelector() as selector:
            for name, stream in (("stdout", process.stdout), ("stderr", process.stderr)):
                os.set_blocking(stream.fileno(), False)
                selector.register(stream, selectors.EVENT_READ, name)
            while selector.get_map():
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise RuntimeError("fixture subprocess exceeded 30 second deadline")
                for key, _ in selector.select(min(remaining, 0.1)):
                    chunk = os.read(key.fileobj.fileno(), 65536)
                    if not chunk:
                        selector.unregister(key.fileobj)
                        continue
                    output[key.data].extend(chunk)
                    if len(output[key.data]) > LIMIT:
                        raise RuntimeError("fixture subprocess exceeded output limit")
        try:
            process.wait(timeout=max(0.001, deadline - time.monotonic()))
        except subprocess.TimeoutExpired as error:
            raise RuntimeError("fixture subprocess exceeded 30 second deadline") from error
        return process.returncode, bytes(output["stdout"]), bytes(output["stderr"])
    finally:
        # Also clean up same-group descendants if the group leader exited early.
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.wait(timeout=5)
        process.stdout.close()
        process.stderr.close()


def compiler_case(root, cargo, env, name, command="check", flags=(), code=None,
                  line=None, marker=None, fail=False, tests=None, build_script=False):
    if name not in BENIGN:
        raise RuntimeError("fixture is not in the benign host execution allowlist")
    cwd = root / "fixtures" / name
    if cwd.is_symlink() or not cwd.is_dir():
        raise RuntimeError("expected a real fixture directory")
    args = [str(cargo), command, "--locked", "--offline", "--message-format=json"]
    if name == "workspace":
        args.append("--workspace")
    args.extend(flags)
    if command == "test":
        args.extend(["--", "--test-threads=1"])
    status, stdout, stderr = run(args, cwd, env)
    if (status != 0) != fail:
        raise AssertionError(f"{name}/{command}: unexpected exit {status}: "
                             + stderr.decode(errors="replace")[:4096])
    messages = []
    for raw in stdout.splitlines():
        if raw.startswith(b"{"):
            messages.append(json.loads(raw))
    finished = [m for m in messages if m.get("reason") == "build-finished"]
    if len(finished) != 1 or finished[0]["success"] != (not fail):
        raise AssertionError(f"{name}: missing/unexpected Cargo build-finished event")
    diagnostics = [m["message"] for m in messages if m.get("reason") == "compiler-message"]
    if code is not None or marker is not None:
        matching = [d for d in diagnostics
                    if (code is None or (d.get("code") or {}).get("code") == code)
                    and (marker is None or marker in d["message"])]
        if not matching:
            raise AssertionError(f"{name}: missing diagnostic {code or marker}: " + str(diagnostics)[:4096])
        if not any(s["is_primary"] and s["file_name"] == "src/lib.rs"
                   and s["line_start"] == line and s["line_end"] >= line
                   and s["column_start"] > 0 and s["column_end"] > s["column_start"]
                   for d in matching for s in d["spans"]):
            raise AssertionError(f"{name}: expected primary diagnostic span src/lib.rs:{line}")
    if tests is not None and f"{tests} passed; 0 failed".encode() not in stdout:
        raise AssertionError(f"{name}: unit test execution not observed")
    if build_script and not any(m.get("reason") == "build-script-executed" for m in messages):
        raise AssertionError("benign build script execution event missing")
    print(json.dumps({"fixture": name, "command": command, "flags": list(flags),
                      "expected_failure": fail, "diagnostic": code or marker, "ok": True}, sort_keys=True))


def verify_corpus(root):
    manifest = json.loads((root / "fixtures" / "corpus-sha256.json").read_text())
    expected = set(manifest)
    actual = set()
    for name in sorted(BENIGN | {"vulnerable-dependency", "security"}):
        directory = root / "fixtures" / name
        if directory.is_symlink():
            raise RuntimeError("fixture directory is a symlink")
        for path in directory.rglob("*"):
            if path.is_symlink():
                raise RuntimeError("fixture contains a symlink")
            if path.is_file():
                relative = str(path.relative_to(root))
                actual.add(relative)
                if hashlib.sha256(path.read_bytes()).hexdigest() != manifest.get(relative):
                    raise RuntimeError("fixture content differs from reviewed corpus receipt: " + relative)
    if actual != expected:
        raise RuntimeError("fixture files differ from reviewed corpus receipt")


def audit_input(root):
    fixture = root / "fixtures" / "vulnerable-dependency"
    provenance = json.loads((fixture / "provenance.json").read_text())
    advisory = (fixture / "RUSTSEC-2023-0071.md").read_bytes()
    assert hashlib.sha256(advisory).hexdigest() == provenance["advisory_sha256"]
    assert provenance["advisory_db_commit"] == "d674d8e9e6f78117229abdb7501452ac6c3cf322"
    assert provenance["advisory_id"] == "RUSTSEC-2023-0071"
    advisory_metadata = tomllib.loads(advisory.decode().split("```toml\n", 1)[1].split("```", 1)[0])
    assert advisory_metadata["advisory"]["id"] == "RUSTSEC-2023-0071"
    assert advisory_metadata["advisory"]["package"] == "rsa"
    assert advisory_metadata["versions"]["patched"] == []
    lock = tomllib.loads((fixture / "Cargo.lock").read_text())
    rsa = [p for p in lock["package"] if p["name"] == "rsa"]
    assert len(rsa) == 1 and rsa[0]["version"] == "0.9.6"
    assert rsa[0]["checksum"] == provenance["registry_checksum"]
    assert rsa[0]["source"] == "registry+https://github.com/rust-lang/crates.io-index"
    print(json.dumps({"fixture": "vulnerable-dependency", "ok": True,
                      "verification": "static pinned audit input only; no Cargo invocation"}, sort_keys=True))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path, help="trusted staging/repository root containing fixtures/")
    parser.add_argument("--cargo", type=Path, required=True, help="absolute real Cargo 1.98.1 binary, not rustup shim")
    args = parser.parse_args()
    if not args.cargo.is_absolute():
        parser.error("--cargo must be absolute")
    cargo = args.cargo.resolve(strict=True)
    rustc = cargo.parent / "rustc"
    root = args.root.resolve(strict=True)
    verify_corpus(root)
    with tempfile.TemporaryDirectory(prefix="rust-mcp-fixtures-") as temp:
        work = Path(temp)
        for name in ("home", "cargo", "tmp", "target"):
            (work / name).mkdir()
        env = {"PATH": str(cargo.parent) + ":/usr/bin:/bin", "RUSTC": str(rustc),
               "HOME": str(work / "home"), "CARGO_HOME": str(work / "cargo"),
               "TMPDIR": str(work / "tmp"), "CARGO_TARGET_DIR": str(work / "target"),
               "CARGO_INCREMENTAL": "0", "CARGO_NET_OFFLINE": "true", "CARGO_TERM_COLOR": "never", "LC_ALL": "C"}
        for binary, version in ((cargo, b"cargo 1.98.1 "), (rustc, b"rustc 1.98.1 ")):
            status, stdout, _ = run([str(binary), "--version"], root, env)
            if status or not stdout.startswith(version):
                raise RuntimeError("expected real Rust/Cargo 1.98.1 toolchain")
        audit_input(root)
        compiler_case(root, cargo, env, "valid-basic", "test", tests=1)
        compiler_case(root, cargo, env, "borrow-error", code="E0502", line=4, fail=True)
        compiler_case(root, cargo, env, "lifetime-error", code="E0597", line=5, fail=True)
        compiler_case(root, cargo, env, "clippy-warning", "clippy", code="clippy::useless_vec", line=3)
        compiler_case(root, cargo, env, "unsafe", code="unsafe_code", line=3, fail=True)
        compiler_case(root, cargo, env, "workspace", "test", tests=1)
        compiler_case(root, cargo, env, "feature-conflict")
        compiler_case(root, cargo, env, "feature-conflict", flags=("--features", "left"))
        compiler_case(root, cargo, env, "feature-conflict", flags=("--features", "right"))
        compiler_case(root, cargo, env, "feature-conflict", flags=("--features", "left,right"),
                      marker="fixture mutually exclusive features", line=2, fail=True)
        compiler_case(root, cargo, env, "build-script", "test", tests=1, build_script=True)
        generated = list((work / "target").glob("debug/build/fixture-build-script-*/out/generated.rs"))
        assert len(generated) == 1 and generated[0].read_text() == "pub const GENERATED: u32 = 42;\n"
    print(json.dumps({"ok": True, "compiler_cases": 11, "static_audit_inputs": 1,
                      "malicious_fixture_executed": False}, sort_keys=True))


if __name__ == "__main__":
    main()
