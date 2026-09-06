#!/usr/bin/env python3
"""Probe Docker/runc masked-equality operands with the approved M2 guest image."""

import datetime
import hashlib
import json
from pathlib import Path
import secrets
import subprocess
import tempfile
import time


ROOT = Path(__file__).resolve().parent.parent
DOCKER = Path("/usr/local/bin/docker")
SOCKET = Path("/Users/cburgosro/.docker/run/docker.sock")
IMAGE = "sha256:384a1742ecc53cdd3a9c0bf36c6f8b66db73ddd118aeeae6e55654ea998ae36a"
PRODUCTION_PROFILE = ROOT / "crates/execution-adapter/src/seccomp-rust-fix.json"
REPORT = ROOT / "docs/validation/M2-fix-socket-mask.json"
LABEL_KEY = "org.rust-mcp.m2-fix-socket-mask"
TIMEOUT = 30

PROBE_SOURCE = r'''use std::os::raw::c_int;

unsafe extern "C" {
    fn socket(domain: c_int, kind: c_int, protocol: c_int) -> c_int;
    fn close(fd: c_int) -> c_int;
}

fn probe(name: &str, kind: c_int, protocol: c_int) {
    let fd = unsafe { socket(2, kind, protocol) };
    let errno = if fd < 0 {
        std::io::Error::last_os_error().raw_os_error().unwrap_or(-1)
    } else {
        let _ = unsafe { close(fd) };
        0
    };
    println!("{name},{kind},{protocol},{fd},{errno}");
}

fn main() {
    probe("stream", 1, 0);
    probe("stream_cloexec", 1 | 0x80000, 0);
    probe("dgram", 2, 0);
    probe("raw", 3, 0);
    probe("stream_protocol_255", 1, 255);
}
'''


def sha256(data):
    return "sha256:" + hashlib.sha256(data).hexdigest()


class Probe:
    def __init__(self, docker_config, temporary):
        self.prefix = [str(DOCKER), "--config", str(docker_config), "--host", f"unix://{SOCKET}"]
        self.temporary = temporary
        self.nonce = secrets.token_hex(8)
        self.label = f"{LABEL_KEY}={self.nonce}"
        self.containers = []
        self.volumes = []
        self.events = []

    def run(self, args, stdin=None, timeout=TIMEOUT):
        argv = self.prefix + list(args)
        started = time.perf_counter_ns()
        result = subprocess.run(
            argv,
            input=stdin,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=timeout,
            check=False,
            env={"PATH": "/usr/bin:/bin", "LANG": "C", "LC_ALL": "C"},
        )
        event = {
            "argv": argv,
            "exit_code": result.returncode,
            "duration_ns": time.perf_counter_ns() - started,
            "stdout": {
                "bytes": len(result.stdout),
                "sha256": sha256(result.stdout),
            },
            "stderr": {
                "bytes": len(result.stderr),
                "sha256": sha256(result.stderr),
            },
        }
        if len(result.stdout) <= 8192:
            event["stdout"]["utf8"] = result.stdout.decode("utf-8", "replace")
        if len(result.stderr) <= 8192:
            event["stderr"]["utf8"] = result.stderr.decode("utf-8", "replace")
        self.events.append(event)
        return result

    def ok(self, args, **kwargs):
        result = self.run(args, **kwargs)
        if result.returncode:
            raise RuntimeError(f"Docker command failed: {args!r}: {result.stderr!r}")
        return result

    def create_volume(self):
        name = f"rust-mcp-m2-mask-{self.nonce}"
        self.ok(["volume", "create", f"--label={self.label}", name])
        self.volumes.append(name)
        return name

    def common_create(self, name, volume, entrypoint, arguments, readonly_volume, profile=None,
                      interactive=False, user="65534:65534"):
        mount = f"type=volume,source={volume},target=/work,volume-nocopy,volume-driver=local"
        if readonly_volume:
            mount += ",readonly"
        args = [
            "container", "create", f"--name={name}", "--pull=never", "--runtime=runc",
            "--init=false", "--network=none", "--read-only", "--cap-drop=ALL",
            "--security-opt=no-new-privileges=true", "--ipc=private", "--cgroupns=private",
            "--pids-limit=128", "--cpus=1", "--memory=512m", "--memory-swap=512m",
            "--shm-size=1m", "--log-driver=none", "--no-healthcheck", f"--user={user}",
            "--workdir=/work", f"--label={self.label}", "--env=PATH=/opt/rust/bin:/usr/bin:/bin",
            "--env=HOME=/work", "--env=TMPDIR=/work", f"--mount={mount}",
        ]
        if interactive:
            args.append("--interactive")
        if profile is not None:
            args.append(f"--security-opt=seccomp={profile}")
        args.extend([f"--entrypoint={entrypoint}", IMAGE, *arguments])
        self.ok(args)
        self.containers.append(name)
        return json.loads(self.ok(["container", "inspect", name]).stdout)[0]

    def start(self, name, stdin=None):
        args = ["container", "start", "--attach"]
        if stdin is not None:
            args.append("--interactive")
        args.append(name)
        return self.run(args, stdin=stdin)

    def remove_container(self, name):
        self.ok(["container", "rm", "--force", name])
        if name in self.containers:
            self.containers.remove(name)

    def compile_probe(self, volume):
        name = f"rust-mcp-m2-mask-{self.nonce}-compile"
        inspected = self.common_create(
            name, volume, "/opt/rust/bin/rustc",
            ["-", "--edition=2024", "--crate-name=socket_mask_probe", "-O", "-o",
             "/work/socket-mask-probe"],
            False, interactive=True, user="0:0",
        )
        self.require_hardened(inspected, "/opt/rust/bin/rustc", None, expected_user="0:0")
        result = self.start(name, PROBE_SOURCE.encode("utf-8"))
        if result.returncode:
            raise AssertionError(f"guest rustc failed: {result.stderr!r}")
        # Keep the compiler container until final cleanup so the evidence retains
        # both its inspected configuration and the compiled artifact's origin.
        return self.hash_probe(volume)

    def hash_probe(self, volume):
        name = f"rust-mcp-m2-mask-{self.nonce}-hash"
        inspected = self.common_create(
            name, volume, "/usr/bin/sha256sum", ["/work/socket-mask-probe"], True,
        )
        self.require_hardened(inspected, "/usr/bin/sha256sum", None)
        result = self.start(name)
        if result.returncode:
            raise AssertionError(f"guest sha256sum failed: {result.stderr!r}")
        self.remove_container(name)
        return result.stdout.decode("ascii").split()[0]

    def require_hardened(self, item, entrypoint, expected_profile, expected_user="65534:65534"):
        host = item["HostConfig"]
        config = item["Config"]
        if not (
            item["Image"] == IMAGE
            and config["User"] == expected_user
            and config["Entrypoint"] == [entrypoint]
            and host["NetworkMode"] == "none"
            and host["ReadonlyRootfs"] is True
            and host["Privileged"] is False
            and host["CapDrop"] == ["ALL"]
            and not host.get("CapAdd")
            and host["PidsLimit"] == 128
            and host["Memory"] == 512 * 1024 * 1024
            and host["MemorySwap"] == 512 * 1024 * 1024
            and host["NanoCpus"] == 1_000_000_000
            and host["Binds"] is None
        ):
            raise AssertionError("container hardening mismatch")
        applied = [value.removeprefix("seccomp=") for value in host["SecurityOpt"]
                   if value.startswith("seccomp=")]
        if expected_profile is None:
            if applied:
                raise AssertionError("unexpected explicit seccomp profile")
            return None
        if len(applied) != 1:
            raise AssertionError("missing applied seccomp profile")
        applied_json = json.loads(applied[0])
        if applied_json != expected_profile:
            raise AssertionError("Docker inspect profile differs from requested profile")
        return {
            "container_id": item["Id"],
            "applied_profile_sha256": sha256(applied[0].encode("utf-8")),
            "socket_rule": socket_rule(applied_json),
        }

    def execute_case(self, label, volume, profile_path, expected_profile):
        name = f"rust-mcp-m2-mask-{self.nonce}-{label}"
        inspected = self.common_create(
            name, volume, "/work/socket-mask-probe", [], True, profile_path,
        )
        applied = self.require_hardened(inspected, "/work/socket-mask-probe", expected_profile)
        result = self.start(name)
        self.remove_container(name)
        if result.returncode:
            raise AssertionError(f"socket probe {label} failed: {result.stderr!r}")
        observations = parse_observations(result.stdout)
        return {"applied": applied, "observations": observations}

    def cleanup(self):
        failures = []
        for name in reversed(self.containers[:]):
            try:
                self.remove_container(name)
            except Exception as error:
                failures.append(f"container {name}: {error}")
        for name in reversed(self.volumes[:]):
            try:
                self.ok(["volume", "rm", name])
                self.volumes.remove(name)
            except Exception as error:
                failures.append(f"volume {name}: {error}")
        containers = self.ok([
            "container", "ls", "--all", "--filter", f"label={self.label}", "--format",
            "{{.Names}}",
        ]).stdout.decode().splitlines()
        volumes = self.ok([
            "volume", "ls", "--filter", f"label={self.label}", "--format", "{{.Name}}",
        ]).stdout.decode().splitlines()
        return {"failures": failures, "containers": containers, "volumes": volumes}


def socket_rule(profile):
    rules = [rule for rule in profile["syscalls"] if rule.get("names") == ["socket"]]
    if len(rules) != 1:
        raise AssertionError("expected exactly one socket rule")
    return rules[0]


def type_operands(profile):
    args = socket_rule(profile)["args"]
    values = [arg for arg in args if arg["index"] == 1 and arg["op"] == "SCMP_CMP_MASKED_EQ"]
    if len(values) != 1:
        raise AssertionError("expected one masked socket type condition")
    return {"value": values[0]["value"], "valueTwo": values[0]["valueTwo"]}


def corrected_profile(old):
    corrected = json.loads(json.dumps(old))
    args = socket_rule(corrected)["args"]
    masked = next(arg for arg in args if arg["index"] == 1)
    if masked != {"index": 1, "value": 1, "valueTwo": 15, "op": "SCMP_CMP_MASKED_EQ"}:
        raise AssertionError("production socket rule is not the reviewed 1/15 rule")
    masked["value"] = 15
    masked["valueTwo"] = 1
    return corrected


def parse_observations(raw):
    values = {}
    for line in raw.decode("ascii").splitlines():
        name, kind, protocol, fd, errno = line.split(",")
        values[name] = {
            "socket_type": int(kind),
            "protocol": int(protocol),
            "returned_fd": int(fd),
            "errno": int(errno),
        }
    if set(values) != {"stream", "stream_cloexec", "dgram", "raw", "stream_protocol_255"}:
        raise AssertionError("incomplete socket probe output")
    return values


def require_outcomes(old, corrected):
    for name in ["stream", "stream_cloexec"]:
        if old[name]["returned_fd"] < 0 or old[name]["errno"] != 0:
            raise AssertionError(f"old profile denied {name}")
    for name in ["dgram", "stream_protocol_255"]:
        if old[name]["returned_fd"] != -1 or old[name]["errno"] != 1:
            raise AssertionError(f"old profile unexpectedly admitted {name}")
    # EP93 is the guest kernel's EPROTONOSUPPORT result. It distinguishes a
    # syscall that passed the allow rule from the profile's default EPERM.
    if old["raw"]["returned_fd"] != -1 or old["raw"]["errno"] != 93:
        raise AssertionError("old profile did not pass raw protocol 0 to the kernel")
    for name in ["stream", "stream_cloexec"]:
        if corrected[name]["returned_fd"] < 0 or corrected[name]["errno"] != 0:
            raise AssertionError(f"corrected profile denied {name}")
    for name in ["dgram", "raw", "stream_protocol_255"]:
        if corrected[name]["returned_fd"] != -1 or corrected[name]["errno"] != 1:
            raise AssertionError(f"corrected profile unexpectedly admitted {name}")


def main():
    production_bytes = PRODUCTION_PROFILE.read_bytes()
    production = json.loads(production_bytes)
    corrected = corrected_profile(production)
    with tempfile.TemporaryDirectory(prefix="rust-mcp-m2-mask-") as directory:
        temporary = Path(directory)
        docker_config = temporary / "docker-config"
        docker_config.mkdir(mode=0o700)
        old_path = temporary / "old.json"
        corrected_path = temporary / "corrected.json"
        old_path.write_text(json.dumps(production, separators=(",", ":")), encoding="utf-8")
        corrected_path.write_text(json.dumps(corrected, separators=(",", ":")), encoding="utf-8")
        probe = Probe(docker_config, temporary)
        cleanup = None
        failure = None
        version = None
        image = None
        packages = None
        binary_hash = None
        old_result = None
        corrected_result = None
        try:
            version = json.loads(probe.ok(["version", "--format", "{{json .}}"] ).stdout)
            image = json.loads(probe.ok(["image", "inspect", IMAGE]).stdout)[0]
            packages = probe.ok([
                "run", "--rm", "--pull=never", "--network=none", "--read-only",
                "--cap-drop=ALL", "--security-opt=no-new-privileges=true", "--user=65534:65534",
                "--entrypoint=/usr/bin/dpkg-query", IMAGE, "-W", "-f=${Package}=${Version}\\n",
                "libseccomp2", "libc6",
            ]).stdout.decode("utf-8")
            volume = probe.create_volume()
            binary_hash = probe.compile_probe(volume)
            old_result = probe.execute_case("old", volume, old_path, production)
            corrected_result = probe.execute_case("corrected", volume, corrected_path, corrected)
            require_outcomes(old_result["observations"], corrected_result["observations"])
        except Exception as error:
            failure = repr(error)
            raise
        finally:
            cleanup = probe.cleanup()
            report = {
                "schema_version": 1,
                "recorded_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds"),
                "objective": "Resolve the reviewed Docker seccomp SCMP_CMP_MASKED_EQ operand order with an applied-profile guest probe.",
                "status": "passed" if failure is None and not cleanup["failures"] and not cleanup["containers"] and not cleanup["volumes"] else "failed",
                "finding": {
                    "review": "docs/reviews/M2-final-security-opus.json P0-1",
                    "review_model": "claude-opus-5",
                    "reviewed_operands": {"value": 1, "valueTwo": 15},
                    "corrected_operands": {"value": 15, "valueTwo": 1},
                    "review_disposition": "P0 rejected: the reviewed claim that 1/15 cannot match is empirically false and omits libseccomp's datum mask fixup.",
                    "corrected_severity": "P1 policy precision: 1/15 admits odd base socket types to the kernel; this probe observed no successful unintended socket and does not establish authority expansion.",
                    "conclusion": "The production 1/15 rule admits AF_INET SOCK_STREAM with protocol 0, including SOCK_CLOEXEC. It also lets SOCK_RAW protocol 0 reach the kernel, which returns EPROTONOSUPPORT. The tightened 15/1 rule preserves both STREAM cases and rejects RAW with the profile's EPERM; both profiles reject DGRAM and protocol 255.",
                    "semantics": "libseccomp treats value as the mask and valueTwo as the datum, then masks the datum before emitting BPF. Thus 1/15 becomes (type & 1) == (15 & 1), while 15/1 becomes (type & 15) == (1 & 15).",
                },
                "profiles": {
                    "production_path": str(PRODUCTION_PROFILE),
                    "production_sha256": sha256(production_bytes),
                    "old_operands": type_operands(production),
                    "corrected_operands": type_operands(corrected),
                    "old_temporary_sha256": sha256(old_path.read_bytes()),
                    "corrected_temporary_sha256": sha256(corrected_path.read_bytes()),
                },
                "probe": {
                    "script_sha256": sha256(Path(__file__).read_bytes()),
                    "source_sha256": sha256(PROBE_SOURCE.encode("utf-8")),
                    "guest_binary_sha256": "sha256:" + binary_hash if binary_hash else None,
                    "old": old_result,
                    "corrected": corrected_result,
                },
                "runtime": {
                    "approved_image_id": image.get("Id") if image else None,
                    "approved_image_repo_digests": image.get("RepoDigests") if image else None,
                    "approved_image_architecture": image.get("Architecture") if image else None,
                    "approved_image_os": image.get("Os") if image else None,
                    "docker": version,
                    "guest_packages": packages.splitlines() if packages else None,
                    "qualification_note": "runc applies the profile; the guest libseccomp package is recorded as image provenance and is not the library that constructs the host-side filter.",
                    "filter_builder_versions": {
                        "runc": "1.3.6 (Docker reports v1.3.6-0-g491b69ba)",
                        "runc_pinned_libseccomp_golang": "v0.10.0",
                        "host_libseccomp": "not exposed by Docker inspect/version; semantics were verified through the applied filter rather than inferred from the guest package",
                    },
                },
                "primary_sources": [
                    {
                        "url": "https://raw.githubusercontent.com/seccomp/libseccomp/v2.5.4/src/db.c",
                        "claim": "Pinned libseccomp 2.5.4 maps datum_a to mask and datum_b to datum, then masks the datum before BPF generation.",
                    },
                    {
                        "url": "https://github.com/seccomp/libseccomp-golang/blob/v0.10.0/seccomp.go",
                        "claim": "libseccomp-golang documents MakeCondition operand handling and masked equality.",
                    },
                    {
                        "url": "https://github.com/opencontainers/runc/blob/v1.3.6/libcontainer/seccomp/seccomp_linux.go",
                        "claim": "runc passes config Arg.Value and Arg.ValueTwo to libseccomp MakeCondition in that order.",
                    },
                    {
                        "url": "https://raw.githubusercontent.com/opencontainers/runc/v1.3.6/go.mod",
                        "claim": "runc 1.3.6 pins libseccomp-golang v0.10.0.",
                    },
                ],
                "controls": {
                    "image_pull": False,
                    "network_mode": "none",
                    "host_binds": False,
                    "project_fixture_executed": False,
                    "probe_compilation": "Fixed source embedded in this script, compiled inside the approved guest into a disposable Docker-managed volume.",
                    "compiler_user": "0:0 with every capability dropped; probe execution user is 65534:65534",
                    "production_profile_modified": False,
                    "profile_difference": "Only socket argument 1 value/valueTwo changed from 1/15 to 15/1 in the corrected temporary copy.",
                },
                "cleanup": cleanup,
                "failure": failure,
                "events": probe.events,
            }
            REPORT.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    if report["status"] != "passed":
        raise SystemExit("probe or cleanup failed")
    print(json.dumps({
        "status": report["status"],
        "old": report["probe"]["old"]["observations"],
        "corrected": report["probe"]["corrected"]["observations"],
        "report": str(REPORT),
    }, indent=2))


if __name__ == "__main__":
    main()
