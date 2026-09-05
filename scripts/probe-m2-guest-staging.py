#!/usr/bin/env python3
"""Qualify a D04 candidate with disposable Docker objects only.

This is an experiment, not a production gateway or an Accepted ADR. Guest
programs and arguments are fixed below; no project code or host bind mount is
used. Exit 0 means every expected observation and final cleanup check matched.
"""

import base64
import datetime
import hashlib
import io
import json
import os
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
TIMEOUT_SECONDS = 20
EVENTS = []


def sha256(data):
    return "sha256:" + hashlib.sha256(data).hexdigest()


def encoded(data):
    return {
        "length": len(data),
        "sha256": sha256(data),
        "base64": base64.b64encode(data).decode("ascii"),
    }


class Probe:
    def __init__(self, config, nonce):
        self.prefix = [
            str(DOCKER),
            "--config",
            str(config),
            "--host",
            f"unix://{SOCKET}",
        ]
        self.nonce = nonce
        self.label = f"org.rust-mcp.m2-d04={nonce}"
        self.containers = []
        self.volumes = []
        self.observations = []

    def run(self, args, *, stdin=b"", timeout=TIMEOUT_SECONDS, binary=False):
        command = self.prefix + list(args)
        started = time.perf_counter_ns()
        try:
            completed = subprocess.run(
                command,
                input=stdin,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=timeout,
                check=False,
                env={"PATH": "/usr/bin:/bin", "LANG": "C", "LC_ALL": "C"},
            )
            event = {
                "argv": command,
                "exit_code": completed.returncode,
                "stdout": encoded(completed.stdout),
                "stderr": encoded(completed.stderr),
                "duration_ns": time.perf_counter_ns() - started,
            }
            if not binary:
                event["stdout_utf8"] = completed.stdout.decode("utf-8", "strict")
                event["stderr_utf8"] = completed.stderr.decode("utf-8", "strict")
            EVENTS.append(event)
            return completed
        except subprocess.TimeoutExpired as error:
            EVENTS.append(
                {
                    "argv": command,
                    "timeout_seconds": timeout,
                    "stdout": encoded(error.stdout or b""),
                    "stderr": encoded(error.stderr or b""),
                    "duration_ns": time.perf_counter_ns() - started,
                }
            )
            raise

    def require(self, condition, case, **facts):
        self.observations.append({"case": case, "matched": bool(condition), **facts})
        if not condition:
            raise AssertionError(case)

    def ok(self, args, **kwargs):
        result = self.run(args, **kwargs)
        if result.returncode != 0:
            raise RuntimeError(f"Docker command failed: {args!r}")
        return result

    def create_volume(self, suffix, size, inodes):
        name = f"rust-mcp-m2-d04-{self.nonce}-{suffix}"
        options = (
            f"size={size},nr_inodes={inodes},uid=65534,gid=65534,"
            "mode=0700,nosuid,nodev,noexec"
        )
        self.ok(
            [
                "volume",
                "create",
                "--driver=local",
                "--opt=type=tmpfs",
                "--opt=device=tmpfs",
                f"--opt=o={options}",
                f"--label={self.label}",
                name,
            ]
        )
        self.volumes.append(name)
        inspected = json.loads(self.ok(["volume", "inspect", name]).stdout)
        expected = {"type": "tmpfs", "device": "tmpfs", "o": options}
        self.require(
            len(inspected) == 1
            and inspected[0]["Name"] == name
            and inspected[0]["Driver"] == "local"
            and inspected[0]["Scope"] == "local"
            and inspected[0]["Options"] == expected
            and inspected[0]["Labels"] == {"org.rust-mcp.m2-d04": self.nonce},
            f"{suffix}_volume_identity",
            options=inspected[0].get("Options"),
            inspect_sha256=EVENTS[-1]["stdout"]["sha256"],
        )
        return name

    def create_container(self, name, volume, *, entrypoint, arguments, readonly, interactive=False):
        args = [
            "container",
            "create",
            f"--name={name}",
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
            f"--label={self.label}",
            "--env=PATH=/opt/rust/bin:/usr/bin:/bin",
            "--env=HOME=/work",
            "--env=TMPDIR=/tmp",
            "--env=CARGO_HOME=/work/cargo",
            "--env=CARGO_TARGET_DIR=/work/target",
            "--env=CARGO_INCREMENTAL=0",
            "--env=CARGO_NET_OFFLINE=true",
            "--env=RUSTC=/opt/rust/bin/rustc",
            "--env=RUSTDOC=/opt/rust/bin/rustdoc",
            "--env=RUSTFMT=/opt/rust/bin/rustfmt",
            (
                f"--mount=type=volume,source={volume},target=/source,"
                f"{'readonly,' if readonly else ''}volume-nocopy,volume-driver=local"
            ),
        ]
        if interactive:
            args.append("--interactive")
        args.extend([f"--entrypoint={entrypoint}", IMAGE, *arguments])
        self.ok(args)
        self.containers.append(name)
        inspected = json.loads(self.ok(["container", "inspect", name]).stdout)
        self.verify_container(inspected, name, volume, entrypoint, arguments, readonly, interactive)

    def verify_container(self, values, name, volume, entrypoint, arguments, readonly, interactive):
        self.require(len(values) == 1, f"{name}_single_inspect")
        item = values[0]
        host = item["HostConfig"]
        config = item["Config"]
        mounts = item["Mounts"]
        safe = (
            item["Image"] == IMAGE
            and config["Image"] == IMAGE
            and config["User"] == "65534:65534"
            and config["Entrypoint"] == [entrypoint]
            and config["Cmd"] == arguments
            and config["OpenStdin"] is interactive
            and config["Tty"] is False
            and config["Labels"] == {"org.rust-mcp.m2-d04": self.nonce}
            and host["NetworkMode"] == "none"
            and host["ReadonlyRootfs"] is True
            and host["Privileged"] is False
            and host["CapDrop"] == ["ALL"]
            and not host.get("CapAdd")
            and host["PidsLimit"] == 128
            and host["Memory"] == 1073741824
            and host["MemorySwap"] == 1073741824
            and host["NanoCpus"] == 1000000000
            and host["RestartPolicy"]["Name"] == "no"
            and host["LogConfig"]["Type"] == "none"
            and host["Binds"] is None
            and host["VolumesFrom"] is None
            and len(mounts) == 1
            and mounts[0]["Type"] == "volume"
            and mounts[0]["Name"] == volume
            and mounts[0]["Destination"] == "/source"
            and mounts[0]["RW"] is (not readonly)
        )
        self.require(
            safe,
            f"{name}_applied_hardening",
            inspect_sha256=EVENTS[-1]["stdout"]["sha256"],
        )

    def start(self, name, *, stdin=b"", binary=False):
        args = ["container", "start", "--attach"]
        if stdin:
            args.append("--interactive")
        args.append(name)
        return self.run(args, stdin=stdin, binary=binary)

    def start_guardian(self, name):
        result = self.ok(["container", "start", name])
        inspected = json.loads(self.ok(["container", "inspect", name]).stdout)[0]
        self.require(
            inspected["State"]["Running"] is True and inspected["State"]["Pid"] > 0,
            f"{name}_running",
            container_id=inspected["Id"],
            pid=inspected["State"]["Pid"],
            start_stdout=result.stdout.decode().strip(),
        )

    def remove_container(self, name):
        self.ok(["container", "rm", "--force", name])
        if name in self.containers:
            self.containers.remove(name)
        result = self.ok(
            ["container", "ls", "--all", "--filter", f"name=^/{name}$", "--format", "{{.ID}}"]
        )
        self.require(not result.stdout.strip(), f"{name}_removed")

    def remove_volume(self, name):
        self.ok(["volume", "rm", name])
        if name in self.volumes:
            self.volumes.remove(name)
        result = self.ok(["volume", "ls", "--filter", f"name=^{name}$", "--format", "{{.Name}}"])
        self.require(not result.stdout.strip(), f"{name}_removed")

    def labelled_inventory(self):
        containers = self.ok(
            ["container", "ls", "--all", "--filter", f"label={self.label}", "--format", "{{.Names}}"]
        ).stdout.decode().splitlines()
        volumes = self.ok(
            ["volume", "ls", "--filter", f"label={self.label}", "--format", "{{.Name}}"]
        ).stdout.decode().splitlines()
        return {"containers": containers, "volumes": volumes}

    def cleanup(self):
        errors = []
        for name in reversed(self.containers[:]):
            try:
                self.remove_container(name)
            except Exception as error:
                errors.append(f"container {name}: {error}")
        for name in reversed(self.volumes[:]):
            try:
                self.remove_volume(name)
            except Exception as error:
                errors.append(f"volume {name}: {error}")
        try:
            inventory = self.labelled_inventory()
        except Exception as error:
            inventory = {"inventory_error": str(error)}
            errors.append(str(error))
        return inventory, errors


def archive(files):
    stream = io.BytesIO()
    with tarfile.open(fileobj=stream, mode="w", format=tarfile.USTAR_FORMAT) as output:
        directories = sorted({part for name in files for part in parents(name)})
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


def parents(name):
    parts = name.split("/")[:-1]
    return ["/".join(parts[:index]) for index in range(1, len(parts) + 1)]


def parse_archive(data):
    result = {}
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:") as source:
        for member in source.getmembers():
            if member.isdir():
                result[member.name] = {"type": "directory", "mode": member.mode}
            elif member.isfile():
                handle = source.extractfile(member)
                content = b"" if handle is None else handle.read()
                result[member.name] = {
                    "type": "file",
                    "mode": member.mode,
                    "length": len(content),
                    "sha256": sha256(content),
                }
            else:
                result[member.name] = {"type": member.type.decode("ascii", "replace")}
    return result


def guardian(probe, suffix, volume):
    name = f"rust-mcp-m2-d04-{probe.nonce}-{suffix}-guardian"
    probe.create_container(
        name,
        volume,
        entrypoint="/usr/bin/sleep",
        arguments=["2147483647"],
        readonly=True,
    )
    probe.start_guardian(name)
    return name


def attached(probe, suffix, volume, entrypoint, arguments, *, readonly, stdin=b"", binary=False):
    name = f"rust-mcp-m2-d04-{probe.nonce}-{suffix}"
    probe.create_container(
        name,
        volume,
        entrypoint=entrypoint,
        arguments=arguments,
        readonly=readonly,
        interactive=bool(stdin),
    )
    result = probe.start(name, stdin=stdin, binary=binary)
    return name, result


def filesystem_stat(probe, suffix, volume):
    name, result = attached(
        probe,
        suffix,
        volume,
        "/usr/bin/stat",
        ["--file-system", "--format=%T:%s:%b:%c:%d", "/source"],
        readonly=True,
    )
    probe.require(result.returncode == 0, f"{suffix}_stat_exit", stderr=result.stderr.decode())
    fields = result.stdout.decode().strip().split(":")
    probe.require(len(fields) == 5 and fields[0] == "tmpfs", f"{suffix}_is_tmpfs", output=fields)
    probe.remove_container(name)
    return {
        "type": fields[0],
        "block_size": int(fields[1]),
        "blocks": int(fields[2]),
        "inodes": int(fields[3]),
        "free_inodes": int(fields[4]),
    }


def lifecycle_experiment(probe):
    volume = probe.create_volume("life", "4m", 128)
    keeper = guardian(probe, "life", volume)
    before = filesystem_stat(probe, "life-stat", volume)
    fixture = {"sentinel.bin": b"guardian-kept-bytes\x00\xff", "src/main.rs": b"fn main() {}\n"}
    payload = archive(fixture)
    ingest, ingested = attached(
        probe,
        "life-ingest",
        volume,
        "/usr/bin/tar",
        ["--extract", "--file=-", "--directory=/source", "--no-same-owner", "--no-same-permissions", "--keep-old-files"],
        readonly=False,
        stdin=payload,
    )
    probe.require(ingested.returncode == 0, "lifecycle_ingest_succeeded", archive=encoded(payload))
    probe.remove_container(ingest)

    mutator, mutated = attached(
        probe,
        "life-mutator",
        volume,
        "/usr/bin/dd",
        ["if=/dev/zero", "of=/source/mutated.bin", "bs=1024", "count=1", "status=none"],
        readonly=False,
    )
    probe.require(mutated.returncode == 0, "lifecycle_fixed_mutator_succeeded", stderr=mutated.stderr.decode())
    probe.remove_container(mutator)
    running = probe.ok(
        ["container", "ls", "--filter", f"label={probe.label}", "--format", "{{.Names}}"]
    ).stdout.decode().splitlines()
    probe.require(
        running == [keeper],
        "no_running_mutator_before_export",
        running_owned_containers=running,
    )

    readonly_probe, denied = attached(
        probe,
        "life-readonly",
        volume,
        "/usr/bin/dd",
        ["if=/dev/zero", "of=/source/readonly-probe", "bs=1", "count=1", "status=none"],
        readonly=True,
    )
    probe.require(denied.returncode != 0, "readonly_mount_denied_write", stderr=denied.stderr.decode())
    probe.remove_container(readonly_probe)

    exporter, exported = attached(
        probe,
        "life-export",
        volume,
        "/usr/bin/tar",
        ["--create", "--file=-", "--format=ustar", "--sort=name", "--one-file-system", "--directory=/source", "."],
        readonly=True,
        binary=True,
    )
    tree = parse_archive(exported.stdout) if exported.returncode == 0 else {}
    probe.require(
        exported.returncode == 0
        and exported.stderr == b""
        and tree.get("./sentinel.bin", {}).get("sha256") == sha256(fixture["sentinel.bin"])
        and tree.get("./src/main.rs", {}).get("sha256") == sha256(fixture["src/main.rs"])
        and tree.get("./mutated.bin", {}).get("length") == 1024
        and "./readonly-probe" not in tree,
        "guardian_preserved_complete_export",
        export=encoded(exported.stdout),
        tree=tree,
    )
    probe.remove_container(exporter)
    probe.remove_container(keeper)

    loss, remounted = attached(
        probe,
        "life-loss-check",
        volume,
        "/usr/bin/tar",
        ["--create", "--file=-", "--format=ustar", "--sort=name", "--one-file-system", "--directory=/source", "."],
        readonly=True,
        binary=True,
    )
    remounted_tree = parse_archive(remounted.stdout) if remounted.returncode == 0 else {}
    probe.require(
        remounted.returncode == 0
        and remounted.stderr == b""
        and set(remounted_tree) <= {"."},
        "last_unmount_lost_tmpfs_data",
        tree=remounted_tree,
        export=encoded(remounted.stdout),
    )
    probe.remove_container(loss)
    probe.remove_volume(volume)
    return {"filesystem_before": before, "exported_tree": tree, "remounted_tree": remounted_tree}


def byte_quota_experiment(probe):
    volume = probe.create_volume("bytes", "2m", 128)
    keeper = guardian(probe, "bytes", volume)
    fs = filesystem_stat(probe, "bytes-stat", volume)
    writer, result = attached(
        probe,
        "bytes-writer",
        volume,
        "/usr/bin/dd",
        ["if=/dev/zero", "of=/source/fill.bin", "bs=1M", "count=3", "status=none"],
        readonly=False,
    )
    probe.require(
        result.returncode != 0 and b"No space left on device" in result.stderr,
        "byte_quota_returns_enospc",
        exit_code=result.returncode,
        stderr=result.stderr.decode(),
    )
    probe.remove_container(writer)
    stat_name, stat_result = attached(
        probe,
        "bytes-file-stat",
        volume,
        "/usr/bin/stat",
        ["--format=%s", "/source/fill.bin"],
        readonly=True,
    )
    size = int(stat_result.stdout.decode().strip()) if stat_result.returncode == 0 else -1
    probe.require(
        stat_result.returncode == 0 and 0 < size <= 2 * 1024 * 1024,
        "byte_quota_bounded_file",
        observed_size=size,
        configured_bytes=2 * 1024 * 1024,
    )
    probe.remove_container(stat_name)
    probe.remove_container(keeper)
    probe.remove_volume(volume)
    return {"filesystem": fs, "partial_file_bytes": size}


def inode_quota_experiment(probe):
    volume = probe.create_volume("inodes", "4m", 32)
    keeper = guardian(probe, "inodes", volume)
    fs = filesystem_stat(probe, "inodes-stat", volume)
    payload = archive({f"f{index:02d}": b"" for index in range(64)})
    ingest, result = attached(
        probe,
        "inodes-ingest",
        volume,
        "/usr/bin/tar",
        ["--extract", "--file=-", "--directory=/source", "--no-same-owner", "--no-same-permissions", "--keep-old-files"],
        readonly=False,
        stdin=payload,
    )
    probe.require(
        result.returncode != 0 and b"No space left on device" in result.stderr,
        "inode_quota_returns_enospc",
        exit_code=result.returncode,
        stderr=result.stderr.decode(),
    )
    probe.remove_container(ingest)
    count_name, count_result = attached(
        probe,
        "inodes-count",
        volume,
        "/usr/bin/find",
        ["/source", "-mindepth", "1", "-maxdepth", "1", "-type", "f", "-printf", "."],
        readonly=True,
    )
    count = len(count_result.stdout) if count_result.returncode == 0 else -1
    probe.require(
        count_result.returncode == 0 and 0 < count < 64 and fs["inodes"] == 32,
        "inode_quota_bounded_entries",
        observed_files=count,
        configured_inodes=fs["inodes"],
    )
    probe.remove_container(count_name)
    probe.remove_container(keeper)
    probe.remove_volume(volume)
    return {"filesystem": fs, "partial_file_count": count}


def failure_cleanup_experiment(probe):
    volume = probe.create_volume("failure", "1m", 32)
    keeper = guardian(probe, "failure", volume)
    injected = False
    try:
        injected = True
        raise RuntimeError("fixed injected failure before mutator/exporter")
    except RuntimeError as error:
        probe.require(
            injected and str(error) == "fixed injected failure before mutator/exporter",
            "failure_injected",
        )
    probe.remove_container(keeper)
    probe.remove_volume(volume)
    inventory = probe.labelled_inventory()
    probe.require(inventory == {"containers": [], "volumes": []}, "injected_failure_cleanup", inventory=inventory)
    return inventory


def main():
    started = time.perf_counter_ns()
    nonce = secrets.token_hex(8)
    report = {
        "started_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "scope": "D04 disposable qualification; no production gateway or project code",
        "product_gate": "not_evaluated",
        "platform": platform.platform(),
        "machine": platform.machine(),
        "docker_executable": str(DOCKER),
        "docker_socket": str(SOCKET),
        "approved_image": IMAGE,
        "seccomp_sha256": sha256(SECCOMP.read_bytes()),
        "script_sha256": sha256(Path(__file__).read_bytes()),
        "run_nonce": nonce,
    }
    status = 70
    probe = None
    with tempfile.TemporaryDirectory(prefix="rust-mcp-m2-d04-config-") as config_name:
        config = Path(config_name)
        config.chmod(0o700)
        probe = Probe(config, nonce)
        try:
            version = probe.ok(
                ["version", "--format", "{{json .}}"]
            )
            report["docker_version"] = json.loads(version.stdout)
            image = json.loads(probe.ok(["image", "inspect", IMAGE]).stdout)
            probe.require(
                len(image) == 1 and image[0]["Id"] == IMAGE and image[0]["Os"] == "linux" and image[0]["Architecture"] == "arm64",
                "approved_image_identity",
                image_id=image[0]["Id"] if image else None,
            )
            report["image_inspect_sha256"] = EVENTS[-1]["stdout"]["sha256"]
            report["experiments"] = {
                "lifecycle": lifecycle_experiment(probe),
                "byte_quota": byte_quota_experiment(probe),
                "inode_quota": inode_quota_experiment(probe),
                "failure_cleanup": failure_cleanup_experiment(probe),
            }
            report["experiment_status"] = "observations_matched"
            report["product_gate"] = "candidate_qualified_not_accepted"
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
    report["events"] = EVENTS
    report["finished_at_utc"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    report["total_duration_ns"] = time.perf_counter_ns() - started
    report["limitations"] = [
        "This does not implement or accept D04.",
        "Only Docker Engine 29.7.2 on the recorded local arm64 Docker Desktop daemon was exercised.",
        "No Cargo, rustfmt, build script, proc macro, user project, host bind mount, network, pull, install, or image build was used.",
        "The trusted Python parser here records qualification evidence; it is not the production hostile-USTAR decoder.",
    ]
    print(json.dumps(report, indent=2, sort_keys=True))
    return status


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as error:
        print(
            json.dumps(
                {
                    "experiment_status": "startup_failure",
                    "product_gate": "inconclusive",
                    "error_type": type(error).__name__,
                    "error": str(error),
                    "script_sha256": sha256(Path(__file__).read_bytes()),
                },
                indent=2,
            )
        )
        sys.exit(70)
