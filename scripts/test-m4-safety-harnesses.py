#!/usr/bin/env python3
"""Benign, Docker-free regression tests for M4 safety harness controls."""
from __future__ import annotations

import hashlib
import importlib.util
import io
import pathlib
import signal
import subprocess
import sys
import tarfile
import tempfile
import unittest
from types import ModuleType
from unittest import mock


ROOT = pathlib.Path(__file__).resolve().parents[1]
TAMPERED_PLUGIN = ROOT / "scripts/test-m4-tampered-plugin.py"


def load_subject() -> ModuleType:
    spec = importlib.util.spec_from_file_location("m4_tampered_plugin", TAMPERED_PLUGIN)
    if spec is None or spec.loader is None:
        raise RuntimeError("M4 tampered-plugin harness unavailable")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


M4 = load_subject()


class BoundedRecorderTests(unittest.TestCase):
    def setUp(self) -> None:
        self.socket = pathlib.Path("/private/tmp/m4-secret-unit.sock")
        self.receipt: dict[str, list[dict[str, object]]] = {"commands": []}
        self.recorder = M4.Recorder(self.receipt, self.socket)

    def assert_output_limited(self, descriptor: int, byte: bytes) -> None:
        size = M4.MAX_CAPTURE_BYTES + 1
        program = f'import os; os.write({descriptor}, {byte!r} * {size})'
        with self.assertRaisesRegex(RuntimeError, "output exceeded"):
            self.recorder.run(
                [sys.executable, "-c", program],
                timeout=5,
                environment={"PATH": "/usr/bin:/bin"},
            )

        event = self.receipt["commands"][-1]
        self.assertEqual(event["status"], "output_limited")
        stream = "stdout" if descriptor == 1 else "stderr"
        other = "stderr" if descriptor == 1 else "stdout"
        self.assertEqual(event[f"{stream}_bytes"], size)
        self.assertEqual(event[f"{stream}_sha256"], hashlib.sha256(byte * size).hexdigest())
        self.assertEqual(event[f"{other}_bytes"], 0)
        self.assertEqual(event[f"{other}_sha256"], hashlib.sha256(b"").hexdigest())

    def test_stdout_is_physically_bounded(self) -> None:
        self.assert_output_limited(1, b"o")

    def test_stderr_is_physically_bounded(self) -> None:
        self.assert_output_limited(2, b"e")

    def test_socket_is_redacted_in_cli_and_product_forms(self) -> None:
        argv = [
            "docker",
            "--host",
            f"unix://{self.socket}",
            "server",
            "--docker-socket",
            str(self.socket),
        ]
        self.assertEqual(
            self.recorder._safe_argv(argv),
            [
                "docker",
                "--host",
                "<docker-socket>",
                "server",
                "--docker-socket",
                "<docker-socket>",
            ],
        )

    def test_interruption_records_bounded_evidence_without_real_signal(self) -> None:
        class InterruptedProcess:
            pid = 424242
            returncode: int | None = None

            def communicate(self, *, input: bytes | None, timeout: int) -> None:
                del input, timeout
                raise KeyboardInterrupt("synthetic interruption")

            def wait(self) -> int:
                self.returncode = -signal.SIGKILL
                return self.returncode

        process = InterruptedProcess()
        with (
            mock.patch.object(M4.subprocess, "Popen", return_value=process),
            mock.patch.object(M4.os, "killpg") as killpg,
        ):
            with self.assertRaisesRegex(KeyboardInterrupt, "synthetic interruption"):
                self.recorder.run(["closed-unit-command"], timeout=1)

        killpg.assert_called_once_with(process.pid, signal.SIGKILL)
        event = self.receipt["commands"][-1]
        self.assertEqual(event["status"], "interrupted")
        self.assertEqual(event["error_type"], "KeyboardInterrupt")
        self.assertEqual(event["exit_code"], -signal.SIGKILL)
        self.assertEqual(event["stdout_bytes"], 0)
        self.assertEqual(event["stderr_bytes"], 0)
        self.assertEqual(event["stdout_sha256"], hashlib.sha256(b"").hexdigest())
        self.assertEqual(event["stderr_sha256"], hashlib.sha256(b"").hexdigest())

    def test_large_plugin_copy_uses_bounded_single_member_archive(self) -> None:
        payload = b"p" * (M4.MAX_CAPTURE_BYTES + 1)
        archive_buffer = io.BytesIO()
        with tarfile.open(
            fileobj=archive_buffer, mode="w", format=tarfile.USTAR_FORMAT
        ) as archive:
            member = tarfile.TarInfo("cargo-deny")
            member.mode = 0o555
            member.size = len(payload)
            archive.addfile(member, io.BytesIO(payload))
        archive_bytes = archive_buffer.getvalue()
        self.assertGreater(len(archive_bytes), M4.MAX_CAPTURE_BYTES)

        class SimulatedDocker:
            def __init__(self, recorder: object) -> None:
                self.recorder = recorder
                self.args: list[str] | None = None
                self.output_limit_bytes: int | None = None

            def docker(
                self, args: list[str], *, output_limit_bytes: int
            ) -> object:
                self.args = args
                self.output_limit_bytes = output_limit_bytes
                program = (
                    "import io,os,tarfile;"
                    f"p=b'p'*{len(payload)};"
                    "b=io.BytesIO();"
                    "t=tarfile.open(fileobj=b,mode='w',format=tarfile.USTAR_FORMAT);"
                    "m=tarfile.TarInfo('cargo-deny');m.mode=0o555;m.size=len(p);"
                    "t.addfile(m,io.BytesIO(p));t.close();os.write(1,b.getvalue())"
                )
                return self.recorder.run(
                    [sys.executable, "-c", program],
                    timeout=5,
                    environment={"PATH": "/usr/bin:/bin"},
                    output_limit_bytes=output_limit_bytes,
                )

        simulated = SimulatedDocker(self.recorder)
        with tempfile.TemporaryDirectory() as directory:
            destination = pathlib.Path(directory) / "cargo-deny"
            digest = M4.copy_from_container(simulated, "container-id", destination)
            self.assertEqual(destination.read_bytes(), payload)
        self.assertEqual(digest, hashlib.sha256(payload).hexdigest())
        self.assertEqual(
            simulated.args,
            ["container", "cp", f"container-id:{M4.PLUGIN_PATH}", "-"],
        )
        self.assertEqual(simulated.output_limit_bytes, M4.MAX_ARCHIVE_CAPTURE_BYTES)

    def test_decorated_commit_stdout_has_one_exact_identity(self) -> None:
        identity = "sha256:" + "4" * 64
        decorated = (
            "Flag --pause has been deprecated, and enabled by default. "
            "Use --no-pause to disable pausing during commit.\n"
            f"{identity}\n"
        ).encode()
        self.assertEqual(M4.parse_commit_identity(decorated), identity)
        with self.assertRaisesRegex(RuntimeError, "one unique image identity"):
            M4.parse_commit_identity(decorated + ("sha256:" + "5" * 64).encode())

    def test_unresolved_commit_cannot_claim_verified_cleanup(self) -> None:
        class EmptyInventoryRecorder:
            def __init__(self) -> None:
                self.receipt = {
                    "commands": [
                        {
                            "argv": ["docker", "container", "commit"],
                            "status": "exited",
                            "commit_identity_resolved": False,
                        }
                    ]
                }
                self.calls: list[list[str]] = []

            def docker(self, args: list[str], **kwargs: object) -> object:
                del kwargs
                self.calls.append(args)
                return subprocess.CompletedProcess(args, 0, b"", b"")

        recorder = EmptyInventoryRecorder()
        with (
            tempfile.TemporaryDirectory() as parent,
            mock.patch.object(M4, "AMBIGUOUS_SETTLE_SECONDS", 0),
        ):
            private_root = pathlib.Path(parent) / "private"
            private_root.mkdir()
            cleanup = M4.cleanup(recorder, "nonce", [], set(), private_root)

        self.assertFalse(cleanup["verified"])
        self.assertFalse(cleanup["unresolved_commit_reconciled"])
        self.assertTrue(
            any(error["kind"] == "image" for error in cleanup["errors"])
        )
        image_lists = [
            args for args in recorder.calls if args[:2] == ["image", "ls"]
        ]
        self.assertTrue(image_lists)
        self.assertTrue(all("--all" in args for args in image_lists))


if __name__ == "__main__":
    unittest.main()
