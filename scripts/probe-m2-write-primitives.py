#!/usr/bin/env python3
"""D02 experiment, not a writer: only mutates disposable private APFS fixtures.

Exit 0 means observations match the experiment, NOT that M2 is qualified.
Exit 1 means unexpected observation; 70 infrastructure error; 78 native host absent.
Darwin flags below are checked against the selected local SDK before any fixture.
"""

import ctypes
import datetime
import errno
import fcntl
import hashlib
import json
import os
from pathlib import Path
import platform
import plistlib
import re
import subprocess
import sys
import tempfile
import time


SWAP = 0x02
NOFOLLOW = 0x10
BENEATH = 0x20
SETLEASE = 106
WRITE_LOCK_TYPE = 3
FULLFSYNC = 51
TIMINGS = []


def sha(data):
    return hashlib.sha256(data).hexdigest()


def run_fixed(args):
    start = time.perf_counter_ns()
    try:
        return subprocess.check_output(args, text=True, timeout=15).strip()
    finally:
        TIMINGS.append({"step": args[0], "duration_ns": time.perf_counter_ns() - start})


def main():
    started = time.perf_counter_ns()
    report = {
        "started_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "scope": "disposable fixture experiment; no production writer",
        "platform": platform.platform(),
        "kernel": platform.release(),
        "machine": platform.machine(),
        "uid": os.getuid(),
        "script_sha256": sha(Path(__file__).read_bytes()),
        "observations": [],
        "product_gate": "not_evaluated",
    }
    if sys.platform != "darwin" or platform.machine() != "arm64":
        report["experiment_status"] = "unavailable_native_host"
        print(json.dumps(report, indent=2))
        return 78
    sdk = Path(run_fixed(["/usr/bin/xcrun", "--show-sdk-path"]))
    headers = {
        "stdio": sdk / "usr/include/sys/stdio.h",
        "fcntl": sdk / "usr/include/sys/fcntl.h",
    }
    report["sdk_headers"] = {
        key: {"path": str(path), "sha256": sha(path.read_bytes())}
        for key, path in headers.items()
    }
    expected = {
        "stdio": {"RENAME_SWAP": SWAP, "RENAME_NOFOLLOW_ANY": NOFOLLOW,
                  "RENAME_RESOLVE_BENEATH": BENEATH},
        "fcntl": {"F_SETLEASE": SETLEASE, "F_WRLCK": WRITE_LOCK_TYPE,
                  "F_FULLFSYNC": FULLFSYNC},
    }
    for key, constants in expected.items():
        content = headers[key].read_text()
        for name, value in constants.items():
            match = re.search(r"^#define\s+" + name + r"\s+(0x[0-9a-fA-F]+|[0-9]+)\b",
                              content, re.MULTILINE)
            if not match or int(match[1], 0) != value:
                raise RuntimeError(f"unqualified SDK constant: {name}")
    libc = ctypes.CDLL("/usr/lib/libSystem.B.dylib", use_errno=True)
    rename = libc.renameatx_np
    rename.argtypes = [ctypes.c_int, ctypes.c_char_p, ctypes.c_int,
                       ctypes.c_char_p, ctypes.c_uint]
    rename.restype = ctypes.c_int

    def swap(fd, source, target, flags=SWAP | NOFOLLOW | BENEATH, target_fd=None):
        ctypes.set_errno(0)
        result = rename(fd, os.fsencode(source), fd if target_fd is None else target_fd,
                        os.fsencode(target), flags)
        if result != 0 and ctypes.get_errno() == 0:
            raise RuntimeError("rename returned failure without errno")
        return 0 if result == 0 else ctypes.get_errno()

    def identity(descriptor):
        info = os.fstat(descriptor)
        return {"device": info.st_dev, "inode": info.st_ino}

    def observe(name, expected_result, **facts):
        if "errno" in facts:
            code = facts["errno"]
            facts["errno_name"] = errno.errorcode.get(code, "OK" if code == 0 else "UNKNOWN")
            facts["errno_message"] = os.strerror(code)
        # All paths below belong to this script's controlled disposable fixture.
        snapshots = {}
        for label, path in [("target", root / "src/file"), ("staged", root / "staged"),
                            ("outside", outside / "file"),
                            ("moved_parent_target", outside / "moved/file"),
                            ("moved_root_target", outside / "moved-root/src/file")]:
            if path.is_file():
                info = path.stat()
                data = path.read_bytes()
                snapshots[label] = {"sha256": sha(data), "size": len(data),
                                    "device": info.st_dev, "inode": info.st_ino}
        report["observations"].append({"case": name, "expected_observation": bool(expected_result),
                                       "files_observed": snapshots, **facts})

    with tempfile.TemporaryDirectory(prefix="rust-mcp-d02-") as temp:
        fixture = Path(temp).resolve()
        device = run_fixed(["/bin/df", "-P", str(fixture)]).splitlines()[-1].split()[0]
        disk = plistlib.loads(run_fixed(
            ["/usr/sbin/diskutil", "info", "-plist", device]).encode())
        report["filesystem"] = disk.get("FilesystemType", "unknown")
        if report["filesystem"] != "apfs":
            report["experiment_status"] = "unavailable_apfs"
            print(json.dumps(report, indent=2))
            return 78

        def setup(name):
            root = fixture / name / "root"
            outside = fixture / name / "outside"
            (root / "src").mkdir(parents=True)
            outside.mkdir()
            (root / "src/file").write_bytes(b"before")
            (root / "staged").write_bytes(b"candidate")
            return root, outside, os.open(root, os.O_RDONLY | os.O_DIRECTORY)

        root, outside, fd = setup("basic")
        try:
            code = swap(fd, "staged", "src/file")
            observe("root_relative_swap_positive", code == 0 and
                    (root / "src/file").read_bytes() == b"candidate" and
                    (root / "staged").read_bytes() == b"before", errno=code)
            code = swap(fd, "staged", "src/file", SWAP | 0x80000000)
            observe("unknown_flag_rejected", code == errno.EINVAL, errno=code)
            code = swap(fd, "staged", str(root / "src/file"))
            observe("absolute_path_beneath_rejected", code == errno.ENOTCAPABLE, errno=code)
        finally:
            os.close(fd)

        # Attack controls are intentionally unsafe ONLY inside this temporary tree.
        for protected in [True, False]:
            root, outside, fd = setup(f"symlink-{protected}")
            try:
                (outside / "file").write_bytes(b"canary")
                (root / "link").symlink_to(outside, target_is_directory=True)
                flags = SWAP | NOFOLLOW | BENEATH if protected else SWAP
                code = swap(fd, "staged", "link/file", flags)
                changed = (outside / "file").read_bytes() != b"canary"
                observe(f"symlink_{'denied' if protected else 'attack_control'}",
                        (code == errno.ELOOP and not changed) if protected else (code == 0 and changed),
                        errno=code, outside_changed=changed)
            finally:
                os.close(fd)

        for root_relative in [True, False]:
            root, outside, fd = setup(f"moved-parent-{root_relative}")
            parent_fd = os.open(root / "src", os.O_RDONLY | os.O_DIRECTORY)
            try:
                os.rename(root / "src", outside / "moved")
                code = swap(fd, "staged", "src/file" if root_relative else "file",
                            target_fd=None if root_relative else parent_fd)
                changed = (outside / "moved/file").read_bytes() != b"before"
                observe(f"moved_parent_{'root_path_denied' if root_relative else 'descriptor_attack_control'}",
                        (code == errno.ENOENT and not changed) if root_relative else (code == 0 and changed),
                        errno=code, outside_changed=changed)
            finally:
                os.close(parent_fd)
                os.close(fd)

        root, outside, fd = setup("moved-root")
        try:
            # A root-path preflight occurred before this controlled competing rename.
            if not root.is_dir():
                raise RuntimeError("fixture root absent before controlled move")
            before_identity = identity(fd)
            os.rename(root, outside / "moved-root")
            code = swap(fd, "staged", "src/file")
            changed = (outside / "moved-root/src/file").read_bytes() == b"candidate"
            after_identity = identity(fd)
            observe("root_handle_does_not_pin_configured_namespace", code == 0 and changed
                    and before_identity == after_identity,
                    errno=code, original_root_path_absent=not root.exists(),
                    handle_before=before_identity, handle_after=after_identity,
                    relocated_root_changed=changed)
        finally:
            os.close(fd)

        root, outside, fd = setup("external-writer")
        leaf = os.open(root / "src/file", os.O_RDWR)
        second = os.open(root / "src/file", os.O_RDWR)
        try:
            fcntl.flock(leaf, fcntl.LOCK_EX | fcntl.LOCK_NB)
            busy = False
            try:
                fcntl.flock(second, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                busy = True
            observe("cooperative_flock_control", busy, second_lock_denied=busy)
            preflight = sha((root / "src/file").read_bytes())
            # Fixed Python child; path is constructed exclusively by this fixture.
            child_start = time.perf_counter_ns()
            subprocess.run([sys.executable, "-I", "-c",
                            "import pathlib,sys; pathlib.Path(sys.argv[1]).write_bytes(b'external')",
                            str(root / "src/file")], check=True, timeout=10,
                           env={}, capture_output=True)
            TIMINGS.append({"step": "fixed_uncooperative_python_child",
                            "duration_ns": time.perf_counter_ns() - child_start})
            observe("uncooperative_writer_ignores_flock",
                    (root / "src/file").read_bytes() == b"external",
                    preflight_sha256=preflight,
                    observed_sha256=sha((root / "src/file").read_bytes()))
            code = swap(fd, "staged", "src/file")
            observe("swap_does_not_compare_expected_content", code == 0 and
                    (root / "src/file").read_bytes() == b"candidate" and
                    (root / "staged").read_bytes() == b"external", errno=code,
                    candidate_published_before_conflict_detection=(root / "src/file").read_bytes() == b"candidate",
                    displaced_external_bytes_retained=(root / "staged").read_bytes() == b"external")
            # A second writer updates the visible candidate before a proposed rollback.
            (root / "src/file").write_bytes(b"second-external")
            code = swap(fd, "staged", "src/file")
            observe("swap_back_displaces_newer_visible_update", code == 0 and
                    (root / "src/file").read_bytes() == b"external" and
                    (root / "staged").read_bytes() == b"second-external", errno=code,
                    newer_update_no_longer_at_user_path=(root / "src/file").read_bytes() != b"second-external",
                    newer_bytes_retained_in_staging=(root / "staged").read_bytes() == b"second-external")
        finally:
            os.close(second)
            os.close(leaf)
            os.close(fd)

        root, outside, fd = setup("lease-durability")
        leaf = os.open(root / "src/file", os.O_RDWR)
        try:
            lease_error = 0
            try:
                fcntl.fcntl(leaf, SETLEASE, WRITE_LOCK_TYPE)
            except OSError as error:
                lease_error = error.errno
            observe("file_lease_unavailable_to_current_process", lease_error == errno.EPERM,
                    errno=lease_error, scope="availability only, no positive lease enforcement claim")
            for label, descriptor in [("file", leaf), ("directory", fd)]:
                sync_error = 0
                try:
                    fcntl.fcntl(descriptor, FULLFSYNC)
                except OSError as error:
                    sync_error = error.errno
                observe(f"fullfsync_{label}", sync_error == 0, errno=sync_error,
                        scope="API result only; no power-loss or journal-recovery claim")
        finally:
            os.close(leaf)
            os.close(fd)

    ok = all(item["expected_observation"] for item in report["observations"])
    report["experiment_status"] = "observations_matched" if ok else "unexpected_observation"
    report["product_gate"] = "no_go_current_candidate" if ok else "inconclusive"
    report["limitations"] = ["No production writer, journal or five M2 tools implemented",
        "No proof that all possible host isolation designs are impossible",
        "No EXDEV cross-volume, hardlink, mmap, kernel crash or power-loss qualification",
        "Expected attack success is evidence against the candidate, not M2 success",
        "Temporary fixture tree removed; no user project bytes changed"]
    report["finished_at_utc"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    report["timings"] = TIMINGS
    report["total_duration_ns"] = time.perf_counter_ns() - started
    print(json.dumps(report, indent=2))
    return 0 if ok else 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as error:
        print(json.dumps({"experiment_status": "infrastructure_error",
                          "product_gate": "inconclusive",
                          "error_type": type(error).__name__,
                          "errno": getattr(error, "errno", None),
                          "script_sha256": sha(Path(__file__).read_bytes())}, indent=2))
        sys.exit(70)
