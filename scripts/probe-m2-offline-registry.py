#!/usr/bin/env python3
"""Qualify a bounded Cargo 1.98.1 local-registry fixture in Docker.

This is D05 design evidence, not a production gateway or an accepted decision.
Every guest program and argument is constructed below. The probe uses no host
bind, shell, pull, install, image build, or runtime network.
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
FIXTURE = ROOT / "fixtures/cargo-local-registry"
DOCKER = Path("/usr/local/bin/docker")
SOCKET = Path("/Users/cburgosro/.docker/run/docker.sock")
IMAGE = "sha256:384a1742ecc53cdd3a9c0bf36c6f8b66db73ddd118aeeae6e55654ea998ae36a"
SECCOMP = ROOT / "crates/execution-adapter/src/seccomp-rust.json"
TIMEOUT_SECONDS = 30
EVENTS = []


def sha256(data):
    return "sha256:" + hashlib.sha256(data).hexdigest()


def encoded(data):
    return {
        "length": len(data),
        "sha256": sha256(data),
        "base64": base64.b64encode(data).decode("ascii"),
    }


def parents(name):
    parts = name.split("/")[:-1]
    return ["/".join(parts[:index]) for index in range(1, len(parts) + 1)]


def archive(files, *, readonly=False):
    stream = io.BytesIO()
    with tarfile.open(fileobj=stream, mode="w", format=tarfile.USTAR_FORMAT) as output:
        directories = sorted({part for name in files for part in parents(name)})
        for name in directories:
            info = tarfile.TarInfo(name)
            info.type = tarfile.DIRTYPE
            # The ingest process must create sibling paths before the volume is
            # remounted read-only. Runtime immutability comes from the mount,
            # not archive directory modes.
            info.mode = 0o700
            info.uid = info.gid = 65534
            info.mtime = 0
            output.addfile(info)
        for name, data in sorted(files.items()):
            info = tarfile.TarInfo(name)
            info.size = len(data)
            info.mode = 0o444 if readonly else 0o600
            info.uid = info.gid = 65534
            info.mtime = 0
            output.addfile(info, io.BytesIO(data))
    return stream.getvalue()


def parse_archive(data):
    files = {}
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:") as source:
        for member in source.getmembers():
            if member.isfile():
                handle = source.extractfile(member)
                content = b"" if handle is None else handle.read()
                name = member.name.removeprefix("./")
                files[name] = content
            elif not member.isdir():
                raise AssertionError(f"unexpected exported entry type: {member.name}")
    return files


def tree_fingerprint(files):
    digest = hashlib.sha256()
    for name, data in sorted(files.items()):
        path = name.encode("utf-8")
        digest.update(len(path).to_bytes(8, "little"))
        digest.update(path)
        digest.update(len(data).to_bytes(8, "little"))
        digest.update(data)
    return "sha256:" + digest.hexdigest()


def fixture_files():
    manifest = json.loads((FIXTURE / "manifest.json").read_text())
    files = {}
    for item in manifest["files"]:
        path = FIXTURE / "registry" / item["path"]
        data = path.read_bytes()
        if len(data) != item["bytes"] or hashlib.sha256(data).hexdigest() != item["sha256"]:
            raise RuntimeError(f"fixture integrity mismatch: {item['path']}")
        files[item["path"]] = data
    if tree_fingerprint(files) != manifest["registry_tree_fingerprint"]:
        raise RuntimeError("fixture tree fingerprint mismatch")
    return manifest, files


ENVIRONMENT = [
    "PATH=/opt/rust/bin:/usr/bin:/bin",
    "HOME=/work",
    "TMPDIR=/tmp",
    "CARGO_HOME=/work/cargo",
    "CARGO_TARGET_DIR=/work/target",
    "CARGO_INCREMENTAL=0",
    "CARGO_NET_OFFLINE=true",
    "RUSTC=/opt/rust/bin/rustc",
    "RUSTDOC=/opt/rust/bin/rustdoc",
    "RUSTFMT=/opt/rust/bin/rustfmt",
]


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
        self.label = f"org.rust-mcp.m2-d05={nonce}"
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
                event["stdout_utf8"] = completed.stdout.decode("utf-8", "replace")
                event["stderr_utf8"] = completed.stderr.decode("utf-8", "replace")
            EVENTS.append(event)
            return completed
        except subprocess.TimeoutExpired as error:
            EVENTS.append({
                "argv": command,
                "timeout_seconds": timeout,
                "stdout": encoded(error.stdout or b""),
                "stderr": encoded(error.stderr or b""),
                "duration_ns": time.perf_counter_ns() - started,
            })
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
        name = f"rust-mcp-m2-d05-{self.nonce}-{suffix}"
        options = (
            f"size={size},nr_inodes={inodes},uid=65534,gid=65534,"
            "mode=0700,nosuid,nodev,noexec"
        )
        self.ok([
            "volume", "create", "--driver=local", "--opt=type=tmpfs",
            "--opt=device=tmpfs", f"--opt=o={options}", f"--label={self.label}", name,
        ])
        self.volumes.append(name)
        value = json.loads(self.ok(["volume", "inspect", name]).stdout)
        expected = {"type": "tmpfs", "device": "tmpfs", "o": options}
        self.require(
            len(value) == 1 and value[0]["Name"] == name
            and value[0]["Driver"] == "local" and value[0]["Scope"] == "local"
            and value[0]["Options"] == expected
            and value[0]["Labels"] == {"org.rust-mcp.m2-d05": self.nonce},
            f"{suffix}_volume_identity", options=value[0].get("Options"),
        )
        return name

    def create_container(self, suffix, *, mounts, entrypoint, arguments, interactive=False):
        name = f"rust-mcp-m2-d05-{self.nonce}-{suffix}"
        args = [
            "container", "create", f"--name={name}", "--pull=never", "--runtime=runc",
            "--init=false", "--network=none", "--read-only", "--cap-drop=ALL",
            "--security-opt=no-new-privileges=true", f"--security-opt=seccomp={SECCOMP}",
            "--ipc=private", "--cgroupns=private", "--pids-limit=128", "--cpus=1",
            "--memory=1g", "--memory-swap=1g", "--shm-size=1m", "--log-driver=none",
            "--no-healthcheck", "--tmpfs=/work:rw,exec,nosuid,nodev,size=512m,mode=1777",
            "--tmpfs=/tmp:rw,nosuid,nodev,noexec,size=64m,mode=1777", "--workdir=/source",
            "--hostname=sandbox", "--user=65534:65534", f"--label={self.label}",
        ]
        args.extend(f"--env={item}" for item in ENVIRONMENT)
        for volume, target, readonly in mounts:
            args.append(
                f"--mount=type=volume,source={volume},target={target},"
                f"{'readonly,' if readonly else ''}volume-nocopy,volume-driver=local"
            )
        if interactive:
            args.append("--interactive")
        args.extend([f"--entrypoint={entrypoint}", IMAGE, *arguments])
        self.ok(args)
        self.containers.append(name)
        inspected = json.loads(self.ok(["container", "inspect", name]).stdout)
        self.verify_container(inspected, name, mounts, entrypoint, arguments, interactive)
        return name

    def verify_container(self, values, name, mounts, entrypoint, arguments, interactive):
        self.require(len(values) == 1, f"{name}_single_inspect")
        item = values[0]
        host = item["HostConfig"]
        config = item["Config"]
        actual_mounts = sorted(
            (m["Type"], m["Name"], m["Destination"], m["RW"]) for m in item["Mounts"]
        )
        expected_mounts = sorted(("volume", v, target, not ro) for v, target, ro in mounts)
        safe = (
            item["Image"] == IMAGE and config["Image"] == IMAGE
            and config["User"] == "65534:65534" and config["Entrypoint"] == [entrypoint]
            and config["Cmd"] == arguments and config["OpenStdin"] is interactive
            and config["Tty"] is False
            and config["Labels"] == {"org.rust-mcp.m2-d05": self.nonce}
            and sorted(config["Env"]) == sorted(ENVIRONMENT)
            and host["NetworkMode"] == "none" and host["ReadonlyRootfs"] is True
            and host["Privileged"] is False and host["CapDrop"] == ["ALL"]
            and not host.get("CapAdd") and host["PidsLimit"] == 128
            and host["Memory"] == 1073741824 and host["MemorySwap"] == 1073741824
            and host["NanoCpus"] == 1000000000 and host["RestartPolicy"]["Name"] == "no"
            and host["LogConfig"]["Type"] == "none" and host["Binds"] is None
            and host["VolumesFrom"] is None and actual_mounts == expected_mounts
        )
        self.require(safe, f"{name}_applied_hardening", mounts=actual_mounts)

    def start(self, name, *, stdin=b"", binary=False):
        args = ["container", "start", "--attach"]
        if stdin:
            args.append("--interactive")
        args.append(name)
        return self.run(args, stdin=stdin, binary=binary)

    def remove_container(self, name):
        self.ok(["container", "rm", "--force", name])
        if name in self.containers:
            self.containers.remove(name)
        found = self.ok([
            "container", "ls", "--all", "--filter", f"name=^/{name}$", "--format", "{{.ID}}",
        ])
        self.require(not found.stdout.strip(), f"{name}_removed")

    def remove_volume(self, name):
        self.ok(["volume", "rm", name])
        if name in self.volumes:
            self.volumes.remove(name)
        found = self.ok(["volume", "ls", "--filter", f"name=^{name}$", "--format", "{{.Name}}"])
        self.require(not found.stdout.strip(), f"{name}_removed")

    def inventory(self):
        containers = self.ok([
            "container", "ls", "--all", "--filter", f"label={self.label}", "--format", "{{.Names}}",
        ]).stdout.decode().splitlines()
        volumes = self.ok([
            "volume", "ls", "--filter", f"label={self.label}", "--format", "{{.Name}}",
        ]).stdout.decode().splitlines()
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
            inventory = self.inventory()
        except Exception as error:
            inventory = {"inventory_error": str(error)}
            errors.append(str(error))
        return inventory, errors


def source_files(manifest):
    lock = b'''# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n\n[[package]]\nname = "d05-fixture"\nversion = "0.1.0"\n'''
    return {
        "Cargo.toml": manifest,
        "Cargo.lock": lock,
        "src/lib.rs": b"pub fn fixture() -> usize { 1 }\n",
        "sentinel.bin": b"d05-host-independent-sentinel\x00\xff",
    }


BASIC_MANIFEST = b'''[package]\nname = "d05-fixture"\nversion = "0.1.0"\nedition = "2024"\n\n[dependencies]\nunicode-ident = "=1.0.24"\n'''

TRANSITIVE_MANIFEST = b'''[package]\nname = "d05-fixture"\nversion = "0.1.0"\nedition = "2024"\n\n[features]\ndefault = ["quote-alias"]\n\n[dependencies.quote-alias]\npackage = "quote"\nversion = "=1.0.47"\noptional = true\ndefault-features = false\nfeatures = ["proc-macro"]\n'''

REMOVED_MANIFEST = b'''[package]\nname = "d05-fixture"\nversion = "0.1.0"\nedition = "2024"\n'''

CONFIG = [
    '--config=source.crates-io.replace-with="rust-mcp-offline"',
    '--config=source.rust-mcp-offline.local-registry="/rust-mcp-registry"',
]


def guardian(probe, suffix, volume, target):
    name = probe.create_container(
        f"{suffix}-guardian", mounts=[(volume, target, True)], entrypoint="/usr/bin/sleep",
        arguments=["2147483647"],
    )
    started = probe.ok(["container", "start", name])
    state = json.loads(probe.ok(["container", "inspect", name]).stdout)[0]["State"]
    probe.require(state["Running"] is True and state["Pid"] > 0, f"{suffix}_guardian_running",
                  pid=state["Pid"], start_stdout=started.stdout.decode().strip())
    return name


def attached(probe, suffix, *, mounts, entrypoint, arguments, stdin=b"", binary=False):
    name = probe.create_container(
        suffix, mounts=mounts, entrypoint=entrypoint, arguments=arguments,
        interactive=bool(stdin),
    )
    result = probe.start(name, stdin=stdin, binary=binary)
    probe.remove_container(name)
    return result


def ingest(probe, suffix, volume, target, payload):
    result = attached(
        probe, f"{suffix}-ingest", mounts=[(volume, target, False)], entrypoint="/usr/bin/tar",
        arguments=["--extract", "--file=-", f"--directory={target}", "--no-same-owner",
                   "--no-same-permissions", "--keep-old-files"], stdin=payload,
    )
    probe.require(result.returncode == 0, f"{suffix}_ingest_succeeded",
                  archive_sha256=sha256(payload), stderr=result.stderr.decode("utf-8", "replace"))


def export(probe, suffix, volume, target):
    result = attached(
        probe, f"{suffix}-export", mounts=[(volume, target, True)], entrypoint="/usr/bin/tar",
        arguments=["--create", "--file=-", "--format=ustar", "--sort=name", "--one-file-system",
                   f"--directory={target}", "."], binary=True,
    )
    probe.require(result.returncode == 0 and not result.stderr, f"{suffix}_export_succeeded",
                  stderr=encoded(result.stderr))
    return parse_archive(result.stdout), encoded(result.stdout)


def cargo_metadata(probe, suffix, source, registry, *, frozen):
    args = ["metadata", "--format-version=1", "--frozen" if frozen else "--offline",
            "--manifest-path=/source/Cargo.toml", *CONFIG]
    return attached(
        probe, suffix, mounts=[(source, "/source", False),
                              (registry, "/rust-mcp-registry", True)],
        entrypoint="/opt/rust/bin/cargo", arguments=args,
    )


def make_case(probe, suffix, source, registry):
    source_volume = probe.create_volume(f"{suffix}-source", "8m", 512)
    registry_volume = probe.create_volume(f"{suffix}-registry", "2m", 512)
    source_guardian = guardian(probe, f"{suffix}-source", source_volume, "/source")
    registry_guardian = guardian(probe, f"{suffix}-registry", registry_volume, "/rust-mcp-registry")
    ingest(probe, f"{suffix}-source", source_volume, "/source", archive(source))
    ingest(probe, f"{suffix}-registry", registry_volume, "/rust-mcp-registry",
           archive(registry, readonly=True))
    return source_volume, registry_volume, source_guardian, registry_guardian


def close_case(probe, source_volume, registry_volume, guardians):
    for name in guardians:
        probe.remove_container(name)
    probe.remove_volume(source_volume)
    probe.remove_volume(registry_volume)


def package_names(metadata):
    return sorted((item["name"], item["version"]) for item in metadata["packages"])


def positive_case(probe, registry, *, suffix, manifest, expected_packages):
    initial = source_files(manifest)
    source_volume, registry_volume, sg, rg = make_case(probe, suffix, initial, registry)
    first = cargo_metadata(probe, f"{suffix}-resolve", source_volume, registry_volume, frozen=False)
    probe.require(first.returncode == 0, f"{suffix}_offline_resolution_succeeded",
                  stderr=first.stderr.decode("utf-8", "replace"))
    metadata = json.loads(first.stdout)
    names = package_names(metadata)
    probe.require(names == sorted(expected_packages), f"{suffix}_resolved_exact_packages", packages=names)
    after, export_evidence = export(probe, f"{suffix}-after-resolve", source_volume, "/source")
    probe.require(
        after["Cargo.toml"] == initial["Cargo.toml"]
        and after["src/lib.rs"] == initial["src/lib.rs"]
        and after["sentinel.bin"] == initial["sentinel.bin"]
        and after["Cargo.lock"] != initial["Cargo.lock"],
        f"{suffix}_only_lock_changed", changed=[name for name in sorted(after)
                                                if after[name] != initial.get(name)],
    )
    frozen = cargo_metadata(probe, f"{suffix}-frozen", source_volume, registry_volume, frozen=True)
    probe.require(frozen.returncode == 0, f"{suffix}_second_frozen_succeeded",
                  stderr=frozen.stderr.decode("utf-8", "replace"))
    frozen_metadata = json.loads(frozen.stdout)
    probe.require(package_names(frozen_metadata) == names, f"{suffix}_frozen_same_resolution")
    registry_after, registry_export = export(
        probe, f"{suffix}-registry-after", registry_volume, "/rust-mcp-registry"
    )
    probe.require(registry_after == registry, f"{suffix}_registry_unchanged",
                  registry_fingerprint=tree_fingerprint(registry_after))
    denied = attached(
        probe, f"{suffix}-registry-write-denied",
        mounts=[(registry_volume, "/rust-mcp-registry", True)], entrypoint="/usr/bin/dd",
        arguments=["if=/dev/zero", "of=/rust-mcp-registry/write-probe", "bs=1", "count=1",
                   "status=none"],
    )
    probe.require(denied.returncode != 0, f"{suffix}_registry_mount_readonly",
                  stderr=denied.stderr.decode("utf-8", "replace"))
    close_case(probe, source_volume, registry_volume, [sg, rg])
    return {
        "packages": names,
        "lock_sha256": sha256(after["Cargo.lock"]),
        "lock_utf8": after["Cargo.lock"].decode(),
        "source_export": export_evidence,
        "registry_export": registry_export,
    }


def add_remove_case(probe, registry):
    initial = source_files(TRANSITIVE_MANIFEST)
    source_volume, registry_volume, sg, rg = make_case(probe, "add-remove", initial, registry)
    added = cargo_metadata(probe, "add-resolve", source_volume, registry_volume, frozen=False)
    probe.require(added.returncode == 0, "add_offline_resolution_succeeded",
                  stderr=added.stderr.decode("utf-8", "replace"))
    added_metadata = json.loads(added.stdout)
    expected = sorted([("d05-fixture", "0.1.0"), ("proc-macro2", "1.0.107"),
                       ("quote", "1.0.47"), ("unicode-ident", "1.0.24")])
    probe.require(package_names(added_metadata) == expected,
                  "add_alias_optional_feature_transitive_resolution", packages=package_names(added_metadata))
    after_add, _ = export(probe, "add-export", source_volume, "/source")
    frozen_add = cargo_metadata(probe, "add-frozen", source_volume, registry_volume, frozen=True)
    probe.require(frozen_add.returncode == 0, "add_second_frozen_succeeded",
                  stderr=frozen_add.stderr.decode("utf-8", "replace"))

    replacement = archive({"Cargo.toml": REMOVED_MANIFEST})
    replaced = attached(
        probe, "remove-manifest", mounts=[(source_volume, "/source", False)],
        entrypoint="/usr/bin/tar",
        arguments=["--extract", "--file=-", "--directory=/source", "--no-same-owner",
                   "--no-same-permissions", "--overwrite"], stdin=replacement,
    )
    probe.require(replaced.returncode == 0, "remove_manifest_staged",
                  stderr=replaced.stderr.decode("utf-8", "replace"))
    removed = cargo_metadata(probe, "remove-resolve", source_volume, registry_volume, frozen=False)
    probe.require(removed.returncode == 0, "remove_offline_resolution_succeeded",
                  stderr=removed.stderr.decode("utf-8", "replace"))
    removed_metadata = json.loads(removed.stdout)
    probe.require(package_names(removed_metadata) == [("d05-fixture", "0.1.0")],
                  "remove_pruned_transitive_lock", packages=package_names(removed_metadata))
    after_remove, _ = export(probe, "remove-export", source_volume, "/source")
    frozen_remove = cargo_metadata(probe, "remove-frozen", source_volume, registry_volume, frozen=True)
    probe.require(frozen_remove.returncode == 0, "remove_second_frozen_succeeded",
                  stderr=frozen_remove.stderr.decode("utf-8", "replace"))
    probe.require(
        after_remove["Cargo.toml"] == REMOVED_MANIFEST
        and after_remove["src/lib.rs"] == initial["src/lib.rs"]
        and after_remove["sentinel.bin"] == initial["sentinel.bin"]
        and b'name = "quote"' in after_add["Cargo.lock"]
        and b'name = "quote"' not in after_remove["Cargo.lock"],
        "remove_only_manifest_and_lock_effects",
    )
    close_case(probe, source_volume, registry_volume, [sg, rg])
    return {
        "added_packages": expected,
        "added_lock_sha256": sha256(after_add["Cargo.lock"]),
        "removed_packages": package_names(removed_metadata),
        "removed_lock_sha256": sha256(after_remove["Cargo.lock"]),
    }


def negative_case(probe, full_registry, *, suffix, mutation, expected_error):
    registry = dict(full_registry)
    if mutation == "missing_index":
        del registry["index/qu/ot/quote"]
    elif mutation == "missing_crate":
        del registry["quote-1.0.47.crate"]
    elif mutation == "checksum_mismatch":
        registry["quote-1.0.47.crate"] += b"corrupt"
    else:
        raise AssertionError(mutation)
    initial = source_files(TRANSITIVE_MANIFEST)
    source_volume, registry_volume, sg, rg = make_case(probe, suffix, initial, registry)
    result = cargo_metadata(probe, f"{suffix}-resolve", source_volume, registry_volume, frozen=False)
    stderr = result.stderr.decode("utf-8", "replace")
    probe.require(result.returncode != 0 and expected_error in stderr.lower(),
                  f"{suffix}_denied", exit_code=result.returncode, stderr=stderr)
    after, export_evidence = export(probe, f"{suffix}-source-after", source_volume, "/source")
    unchanged_except_lock = all(after.get(name) == data for name, data in initial.items()
                                if name != "Cargo.lock")
    probe.require(unchanged_except_lock, f"{suffix}_no_nonlock_source_effect",
                  changed=[name for name in sorted(after) if after[name] != initial.get(name)])
    registry_after, _ = export(probe, f"{suffix}-registry-after", registry_volume,
                               "/rust-mcp-registry")
    probe.require(registry_after == registry, f"{suffix}_registry_unchanged")
    close_case(probe, source_volume, registry_volume, [sg, rg])
    return {
        "exit_code": result.returncode,
        "stderr": stderr,
        "staging_changed_files": [name for name in sorted(after) if after[name] != initial.get(name)],
        "source_export": export_evidence,
        "candidate_publishable": False,
    }


def main():
    started = time.perf_counter_ns()
    nonce = secrets.token_hex(8)
    manifest, full_registry = fixture_files()
    basic_registry = {name: data for name, data in full_registry.items()
                      if "unicode-ident" in name}
    report = {
        "started_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "scope": "D05 fixture qualification only; not production acceptance or M2-04 completion",
        "product_gate": "not_evaluated",
        "platform": platform.platform(),
        "machine": platform.machine(),
        "docker_executable": str(DOCKER),
        "docker_socket": str(SOCKET),
        "approved_image": IMAGE,
        "cargo_expected": "cargo 1.98.1 (797e8a9bc 2026-08-05)",
        "seccomp_sha256": sha256(SECCOMP.read_bytes()),
        "script_sha256": sha256(Path(__file__).read_bytes()),
        "fixture_manifest_sha256": sha256((FIXTURE / "manifest.json").read_bytes()),
        "registry_tree_fingerprint": manifest["registry_tree_fingerprint"],
        "index_commit": manifest["index_commit"],
        "index_config_present": (FIXTURE / "registry/index/config.json").exists(),
        "run_nonce": nonce,
    }
    status = 70
    probe = None
    with tempfile.TemporaryDirectory(prefix="rust-mcp-m2-d05-config-") as config_name:
        config = Path(config_name)
        config.chmod(0o700)
        (config / "config.json").write_text("{}\n")
        probe = Probe(config, nonce)
        try:
            version = probe.ok(["version", "--format", "{{json .}}"])
            report["docker_version"] = json.loads(version.stdout)
            image = json.loads(probe.ok(["image", "inspect", IMAGE]).stdout)
            probe.require(len(image) == 1 and image[0]["Id"] == IMAGE
                          and image[0]["Os"] == "linux" and image[0]["Architecture"] == "arm64",
                          "approved_image_identity", image_id=image[0]["Id"] if image else None)
            version_source = {"Cargo.toml": BASIC_MANIFEST, "Cargo.lock": source_files(BASIC_MANIFEST)["Cargo.lock"],
                              "src/lib.rs": b"pub fn fixture() -> usize { 1 }\n"}
            sv, rv, sg, rg = make_case(probe, "version", version_source, basic_registry)
            cargo_version = attached(
                probe, "cargo-version", mounts=[(sv, "/source", True),
                                                 (rv, "/rust-mcp-registry", True)],
                entrypoint="/opt/rust/bin/cargo", arguments=["--version", "--verbose"],
            )
            cargo_version_text = cargo_version.stdout.decode()
            probe.require(cargo_version.returncode == 0 and "cargo 1.98.1" in cargo_version_text
                          and "797e8a9bca276c1c9f9f738d2a20f484fa4eea9d" in cargo_version_text,
                          "cargo_exact_identity", stdout=cargo_version_text)
            close_case(probe, sv, rv, [sg, rg])
            report["cargo_observed"] = cargo_version_text
            report["experiments"] = {
                "basic_no_index_config": positive_case(
                    probe, basic_registry, suffix="basic", manifest=BASIC_MANIFEST,
                    expected_packages=[("d05-fixture", "0.1.0"), ("unicode-ident", "1.0.24")],
                ),
                "alias_optional_feature_transitive_add_remove": add_remove_case(probe, full_registry),
                "missing_index": negative_case(
                    probe, full_registry, suffix="missing-index", mutation="missing_index",
                    expected_error="no matching package named",
                ),
                "missing_crate": negative_case(
                    probe, full_registry, suffix="missing-crate", mutation="missing_crate",
                    expected_error="failed to open",
                ),
                "checksum_mismatch": negative_case(
                    probe, full_registry, suffix="checksum", mutation="checksum_mismatch",
                    expected_error="checksum",
                ),
            }
            report["experiment_status"] = "observations_matched"
            report["product_gate"] = "fixture_candidate_qualified_not_accepted"
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
        "This qualifies only the retained fixture and D05 candidate mechanics.",
        "It does not accept D05, implement a production resolver, or complete M2-04/M2-05.",
        "Only the recorded Docker Desktop arm64 daemon and approved local image were exercised.",
        "The fixture is a bounded crates.io subset, not a live or complete registry snapshot.",
        "Negative cases may alter an ephemeral staging lock before Cargo fails; no candidate is published.",
        "The Python USTAR encoder/parser is experiment code, not the production hostile-input decoder.",
    ]
    print(json.dumps(report, indent=2, sort_keys=True))
    return status


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as error:
        print(json.dumps({
            "experiment_status": "startup_failure", "product_gate": "inconclusive",
            "error_type": type(error).__name__, "error": str(error),
            "script_sha256": sha256(Path(__file__).read_bytes()),
        }, indent=2))
        sys.exit(70)
