#!/usr/bin/env python3
"""Provisioning oracle for a fixed first-party Miri fixture; not gateway qualification."""
import datetime
import hashlib
import json
import pathlib
import subprocess
import tempfile
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[2]
DOCKER = ["/Applications/Docker.app/Contents/Resources/bin/docker", "--host", "unix:///Users/cburgosro/.docker/run/docker.sock"]
IMAGE = "sha256:95dddeb5305f10b09b441e3cc4018ebb1a8a296d365c65106327d59f933c64e7"
SYSROOT = "/opt/miri-sysroot/2026-09-07/aarch64-unknown-linux-gnu"
NIGHTLY = "/opt/rust-nightly-2026-09-07/bin"
MANIFEST = b'[package]\nname="m4-miri-probe"\nversion="0.0.0"\nedition="2024"\n'
LOCK = b'version = 4\n[[package]]\nname = "m4-miri-probe"\nversion = "0.0.0"\n'
SOURCE = b'#[test] fn prepared_sysroot_is_used() { assert!(cfg!(miri)); let values = vec![2, 3]; assert_eq!(values.iter().sum::<i32>(), 5); }\n'


def digest(data):
    return "sha256:" + hashlib.sha256(data).hexdigest()


def main():
    image = json.loads(subprocess.check_output(DOCKER + ["image", "inspect", IMAGE]))[0]
    assert image["Id"] == IMAGE and image["Os"] == "linux" and image["Architecture"] == "arm64"
    config = digest(json.dumps(image["Config"], sort_keys=True, separators=(",", ":")).encode())
    assert config == "sha256:7d4e58b9e29b2045c13d71542f7892ee071a6886a1b939c4cbfc3ff7ce40dc45"
    common = DOCKER + ["run", "--rm", "--pull=never", "--network=none", "--read-only",
        "--cap-drop=ALL", "--security-opt=no-new-privileges=true", "--user=65534:65534",
        "--pids-limit=128", "--memory=1g", "--memory-swap=1g", "--cpus=1",
        "--tmpfs=/work:rw,exec,nosuid,nodev,size=256m,mode=1777",
        "--tmpfs=/tmp:rw,noexec,nosuid,nodev,size=64m,mode=1777"]
    tree_command = ["/bin/sh", "-c", "find /opt/miri-sysroot/2026-09-07/aarch64-unknown-linux-gnu -type f -print0 | sort -z | xargs -0 sha256sum | sha256sum"]
    def tree():
        return subprocess.check_output(common + ["--entrypoint=/usr/bin/env", IMAGE, "-i", "PATH=/usr/bin:/bin"] + tree_command, timeout=60).decode().strip()
    before = tree()
    name = "m4-provision-miri-" + uuid.uuid4().hex
    with tempfile.TemporaryDirectory(prefix="m4-miri-probe-") as directory:
        source = pathlib.Path(directory)
        source.chmod(0o755)
        (source / "src").mkdir(mode=0o755)
        for path, data in [("Cargo.toml", MANIFEST), ("Cargo.lock", LOCK), ("src/lib.rs", SOURCE)]:
            (source / path).write_bytes(data)
            (source / path).chmod(0o444)
        command = common + ["--name=" + name, "--workdir=/source",
            "--mount=type=bind,source=" + str(source) + ",target=/source,readonly",
            "--entrypoint=/usr/bin/env", IMAGE, "-i", "PATH=" + NIGHTLY + ":/usr/bin:/bin",
            "HOME=/work", "TMPDIR=/tmp", "CARGO_HOME=/work/cargo", "CARGO_TARGET_DIR=/work/target",
            "CARGO_NET_OFFLINE=true", "CARGO_INCREMENTAL=0", "MIRI_SYSROOT=" + SYSROOT,
            "RUSTC=" + NIGHTLY + "/rustc", "CARGO=" + NIGHTLY + "/cargo",
            NIGHTLY + "/cargo", "miri", "test", "--lib", "--frozen", "--offline",
            "--target=aarch64-unknown-linux-gnu"]
        try:
            result = subprocess.run(command, capture_output=True, timeout=120)
        finally:
            # Timeout is not publication: the exact container must be gone first.
            found = subprocess.check_output(DOCKER + ["ps", "-aq", "--filter=name=^/" + name + "$"])
            if found.strip():
                subprocess.run(DOCKER + ["rm", "-f", name], check=True, capture_output=True)
            assert not subprocess.check_output(DOCKER + ["ps", "-aq", "--filter=name=^/" + name + "$"]).strip()
        assert (source / "Cargo.lock").read_bytes() == LOCK
    after = tree()
    passed = result.returncode == 0 and b"1 passed; 0 failed" in result.stdout and before == after
    receipt = {"schema": "rust-engineering-mcp.m4-prepared-miri-oracle.v1", "observed_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "image_id": IMAGE, "image_config_digest": config, "source_sha256": digest(SOURCE), "sysroot_before": before, "sysroot_after": after,
        "passed": passed, "gateway_approved": False, "rootfs_readonly": True, "source_readonly": True, "network": "none",
        "exit_code": result.returncode, "stdout": result.stdout.decode(), "stderr": result.stderr.decode(), "cleanup_confirmed": True,
        "script_sha256": digest(pathlib.Path(__file__).read_bytes()), "claim": "first-party fixture only; no hostile-project or gateway qualification"}
    (ROOT / "docs/validation/M4-prepared-miri.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps(receipt, indent=2))
    if not passed:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
