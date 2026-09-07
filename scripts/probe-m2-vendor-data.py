#!/usr/bin/env python3
"""Qualify a Cargo 1.98.1 directory-source fixture in the approved guest.

This is a bounded D05 design experiment. It imports the already qualified D05
Docker harness, never bind-mounts a host path, and never extracts an untrusted
archive on the host. Only validated regular files are materialized.
"""

import datetime
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path, PurePosixPath
import secrets
import sys
import tarfile
import tempfile
import time


ROOT = Path(__file__).resolve().parent.parent
LOCAL_FIXTURE = ROOT / "fixtures/cargo-local-registry"
VENDOR_FIXTURE = ROOT / "fixtures/cargo-vendor-data"
BASE_PATH = ROOT / "scripts/probe-m2-offline-registry.py"
RAW_REPORT = ROOT / "docs/validation/M2-D05-vendor-qualification.json"
EXPECTED_DIRECTORIES = {
    "proc-macro2-1.0.107",
    "quote-1.0.47",
    "unicode-ident-1.0.24",
}
MAX_ARCHIVE_BYTES = 8 * 1024 * 1024
MAX_ENTRIES = 2048
MAX_FILE_BYTES = 2 * 1024 * 1024
MAX_TOTAL_BYTES = 4 * 1024 * 1024
MAX_PATH_BYTES = 240
MAX_DEPTH = 16


def load_base():
    spec = importlib.util.spec_from_file_location("m2_offline_registry_probe", BASE_PATH)
    if spec is None or spec.loader is None:
        raise RuntimeError("unable to load the bounded D05 Docker harness")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    module.EVENTS.clear()
    return module


BASE = load_base()


def sha256(data):
    return "sha256:" + hashlib.sha256(data).hexdigest()


def normalized_path(raw):
    name = raw.removeprefix("./").rstrip("/")
    path = PurePosixPath(name)
    if (
        not name
        or name.startswith("/")
        or len(name.encode("utf-8")) > MAX_PATH_BYTES
        or len(path.parts) > MAX_DEPTH
        or any(part in ("", ".", "..") for part in path.parts)
    ):
        raise AssertionError(f"invalid archive path: {raw!r}")
    return name


def parse_bounded_ustar(data):
    if len(data) > MAX_ARCHIVE_BYTES:
        raise AssertionError("export archive exceeds byte budget")
    files = {}
    metadata = {}
    seen = set()
    total = 0
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:") as source:
        members = source.getmembers()
        if len(members) > MAX_ENTRIES:
            raise AssertionError("export archive exceeds entry budget")
        for member in members:
            if member.name in (".", "./") and member.isdir():
                continue
            name = normalized_path(member.name)
            if name in seen:
                raise AssertionError(f"duplicate archive path: {name}")
            seen.add(name)
            if member.issym() or member.islnk() or member.isdev() or member.isfifo():
                raise AssertionError(f"link or special entry rejected: {name}")
            if member.pax_headers:
                raise AssertionError(f"extended metadata rejected: {name}")
            mode = member.mode & 0o7777
            if mode & 0o7000:
                raise AssertionError(f"privileged mode rejected: {name}")
            if member.isdir():
                metadata[name] = {"kind": "directory", "mode": mode,
                                  "uid": member.uid, "gid": member.gid}
                continue
            if not member.isfile() or member.size > MAX_FILE_BYTES:
                raise AssertionError(f"unsupported or oversized entry: {name}")
            handle = source.extractfile(member)
            content = b"" if handle is None else handle.read()
            if len(content) != member.size:
                raise AssertionError(f"short archive member: {name}")
            total += len(content)
            if total > MAX_TOTAL_BYTES:
                raise AssertionError("export exceeds logical byte budget")
            files[name] = content
            metadata[name] = {"kind": "regular", "mode": mode,
                              "uid": member.uid, "gid": member.gid,
                              "bytes": len(content), "sha256": sha256(content)}
    return files, metadata


def export_bounded(probe, suffix, volume, target):
    result = BASE.attached(
        probe, f"{suffix}-export", mounts=[(volume, target, True)],
        entrypoint="/usr/bin/tar",
        arguments=["--create", "--file=-", "--format=ustar", "--sort=name",
                   "--one-file-system", f"--directory={target}", "."], binary=True,
    )
    probe.require(result.returncode == 0 and not result.stderr,
                  f"{suffix}_bounded_export_succeeded",
                  archive_bytes=len(result.stdout), archive_sha256=sha256(result.stdout))
    files, metadata = parse_bounded_ustar(result.stdout)
    return files, metadata, {
        "archive_bytes": len(result.stdout),
        "archive_sha256": sha256(result.stdout),
        "entry_count": len(metadata),
        "regular_file_count": len(files),
        "logical_file_bytes": sum(len(value) for value in files.values()),
    }


def local_registry_files():
    manifest, files = BASE.fixture_files()
    return manifest, files


def validate_vendor(files, metadata, local_manifest):
    vendor = {}
    vendor_meta = {}
    for name, content in files.items():
        if not name.startswith("vendor/"):
            continue
        relative = name.removeprefix("vendor/")
        top = relative.split("/", 1)[0]
        if top not in EXPECTED_DIRECTORIES or "/" not in relative:
            raise AssertionError(f"unexpected vendor file: {relative}")
        vendor[relative] = content
        vendor_meta[relative] = metadata[name]
    observed_dirs = {name.split("/", 1)[0] for name in vendor}
    if observed_dirs != EXPECTED_DIRECTORIES:
        raise AssertionError(f"unexpected vendor directories: {observed_dirs}")

    checksums = {item["name"]: item["crate_sha256"] for item in local_manifest["packages"]}
    checksum_evidence = {}
    for directory in sorted(EXPECTED_DIRECTORIES):
        package_name = directory.rsplit("-", 1)[0]
        checksum_path = f"{directory}/.cargo-checksum.json"
        if checksum_path not in vendor:
            raise AssertionError(f"missing {checksum_path}")
        checksum = json.loads(vendor[checksum_path])
        expected_package = checksums[package_name]
        if checksum.get("package") != expected_package:
            raise AssertionError(f"package checksum mismatch: {directory}")
        declared = checksum.get("files")
        if not isinstance(declared, dict):
            raise AssertionError(f"invalid files checksum map: {directory}")
        actual_paths = {
            name.removeprefix(directory + "/")
            for name in vendor
            if name.startswith(directory + "/") and name != checksum_path
        }
        if set(declared) != actual_paths:
            raise AssertionError(f"checksum coverage mismatch: {directory}")
        for relative, expected in declared.items():
            actual = hashlib.sha256(vendor[f"{directory}/{relative}"]).hexdigest()
            if actual != expected:
                raise AssertionError(f"file checksum mismatch: {directory}/{relative}")
        checksum_evidence[directory] = {
            "package_sha256": "sha256:" + checksum["package"],
            "covered_regular_files": len(declared),
            "checksum_file_sha256": sha256(vendor[checksum_path]),
        }

    if any(item["kind"] != "regular" for item in vendor_meta.values()):
        raise AssertionError("non-regular vendor content reached materialization")
    return vendor, vendor_meta, checksum_evidence


def materialize_fixture(vendor, vendor_meta, local_manifest, generation):
    file_rows = [
        {"path": name, "bytes": len(content), "sha256": hashlib.sha256(content).hexdigest(),
         "source_mode": vendor_meta[name]["mode"]}
        for name, content in sorted(vendor.items())
    ]
    fingerprint = BASE.tree_fingerprint(vendor)
    manifest = {
        "format_version": 1,
        "purpose": "M2 D05 directory-source fixture qualification; not production acceptance",
        "cargo_version": local_manifest["cargo_version"],
        "cargo_commit": local_manifest["cargo_commit"],
        "source_registry_fixture_fingerprint": local_manifest["registry_tree_fingerprint"],
        "source_index_commit": local_manifest["index_commit"],
        "vendor_tree_fingerprint": fingerprint,
        "generation_argv": generation["argv"],
        "packages": local_manifest["packages"],
        "files": file_rows,
        "restrictions": [
            "Only bounded canonical paths, directories, and regular files were accepted.",
            "Symlinks, hardlinks, devices, FIFOs, privileged mode bits, duplicates, and PAX metadata were rejected.",
            "The host wrote each validated file explicitly; no guest archive was extracted on the host.",
            ".cargo-checksum.json detects accidental modification and is not an authentication mechanism.",
        ],
    }
    readme = f"""# Cargo directory-source qualification fixture\n\nThis fixture was generated by Cargo {local_manifest['cargo_version']} from the bounded local-registry fixture and is retained only as M2 D05 design evidence. It contains unpacked `proc-macro2 1.0.107`, `quote 1.0.47`, and `unicode-ident 1.0.24`.\n\nSource index commit: `{local_manifest['index_commit']}`. Vendor tree fingerprint: `{fingerprint}`. Licenses and provenance are recorded in `manifest.json`. Cargo's `.cargo-checksum.json` protects against accidental changes; it is not a security mechanism.\n"""
    generated = dict(vendor)
    generated["../manifest.json"] = (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode()
    generated["../README.md"] = readme.encode()

    if VENDOR_FIXTURE.exists():
        existing_manifest = json.loads((VENDOR_FIXTURE / "manifest.json").read_text())
        if existing_manifest.get("vendor_tree_fingerprint") != fingerprint:
            raise AssertionError("existing vendor fixture differs; refusing replacement")
        for row in file_rows:
            path = VENDOR_FIXTURE / "vendor" / row["path"]
            if not path.is_file() or hashlib.sha256(path.read_bytes()).hexdigest() != row["sha256"]:
                raise AssertionError(f"existing vendor fixture mismatch: {row['path']}")
        return manifest

    VENDOR_FIXTURE.parent.mkdir(parents=True, exist_ok=True)
    stage = Path(tempfile.mkdtemp(prefix=".cargo-vendor-data-", dir=VENDOR_FIXTURE.parent))
    try:
        vendor_root = stage / "vendor"
        for name, content in sorted(vendor.items()):
            destination = vendor_root.joinpath(*PurePosixPath(name).parts)
            destination.parent.mkdir(parents=True, exist_ok=True)
            with destination.open("xb") as output:
                output.write(content)
            destination.chmod(0o644)
        (stage / "manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")
        (stage / "README.md").write_text(readme)
        os.replace(stage, VENDOR_FIXTURE)
    finally:
        if stage.exists():
            stage.rmdir()
    return manifest


VENDOR_CONFIG = [
    '--config=source.crates-io.replace-with="rust-mcp-vendor"',
    '--config=source.rust-mcp-vendor.directory="/rust-mcp-vendor"',
]


def cargo_vendor_metadata(probe, suffix, source, vendor, *, frozen):
    return BASE.attached(
        probe, suffix,
        mounts=[(source, "/source", False), (vendor, "/rust-mcp-vendor", True)],
        entrypoint="/opt/rust/bin/cargo",
        arguments=["metadata", "--format-version=1", "--frozen" if frozen else "--offline",
                   "--manifest-path=/source/Cargo.toml", *VENDOR_CONFIG],
    )


def cargo_vendor_check(probe, suffix, source, vendor):
    return BASE.attached(
        probe, suffix,
        mounts=[(source, "/source", False), (vendor, "/rust-mcp-vendor", True)],
        entrypoint="/opt/rust/bin/cargo",
        arguments=["check", "--frozen", "--manifest-path=/source/Cargo.toml", *VENDOR_CONFIG],
    )


def make_vendor_case(probe, suffix, source_files, vendor_files):
    source = probe.create_volume(f"{suffix}-source", "8m", 512)
    vendor = probe.create_volume(f"{suffix}-vendor", "8m", 4096)
    sg = BASE.guardian(probe, f"{suffix}-source", source, "/source")
    vg = BASE.guardian(probe, f"{suffix}-vendor", vendor, "/rust-mcp-vendor")
    before = time.perf_counter_ns()
    BASE.ingest(probe, f"{suffix}-source", source, "/source", BASE.archive(source_files))
    source_transfer_ns = time.perf_counter_ns() - before
    payload = BASE.archive(vendor_files, readonly=True)
    before = time.perf_counter_ns()
    BASE.ingest(probe, f"{suffix}-vendor", vendor, "/rust-mcp-vendor", payload)
    vendor_transfer_ns = time.perf_counter_ns() - before
    return source, vendor, sg, vg, {
        "source_archive_bytes": len(BASE.archive(source_files)),
        "source_transfer_duration_ns": source_transfer_ns,
        "vendor_archive_bytes": len(payload),
        "vendor_transfer_duration_ns": vendor_transfer_ns,
    }


def close_vendor_case(probe, source, vendor, guardians):
    for item in guardians:
        probe.remove_container(item)
    probe.remove_volume(source)
    probe.remove_volume(vendor)


def compare_transfer(probe, registry_files, vendor_files):
    results = {}
    for label, files, size, inodes in (
        ("local_registry", registry_files, "2m", 512),
        ("vendor", vendor_files, "8m", 4096),
    ):
        volume = probe.create_volume(f"transfer-{label}", size, inodes)
        target = f"/transfer-{label}"
        guard = BASE.guardian(probe, f"transfer-{label}", volume, target)
        payload = BASE.archive(files, readonly=True)
        before = time.perf_counter_ns()
        BASE.ingest(probe, f"transfer-{label}", volume, target, payload)
        duration = time.perf_counter_ns() - before
        exported, _, evidence = export_bounded(
            probe, f"transfer-{label}", volume, target
        )
        probe.require(exported == files, f"transfer_{label}_round_trip")
        probe.remove_container(guard)
        probe.remove_volume(volume)
        results[label] = {
            "archive_bytes": len(payload),
            "regular_file_count": len(files),
            "logical_file_bytes": sum(len(value) for value in files.values()),
            "ingest_lifecycle_duration_ns": duration,
            "export": evidence,
        }
    return results


def generate_vendor(probe, local_manifest, registry_files):
    initial = BASE.source_files(BASE.TRANSITIVE_MANIFEST)
    source, registry, sg, rg = BASE.make_case(probe, "generate", initial, registry_files)
    resolved = BASE.cargo_metadata(probe, "generate-lock", source, registry, frozen=False)
    probe.require(resolved.returncode == 0, "generator_lock_resolved_offline",
                  stderr=resolved.stderr.decode("utf-8", "replace"))
    argv = [
        "vendor", "--offline", "--locked", "--respect-source-config", "--versioned-dirs",
        "--manifest-path=/source/Cargo.toml", *BASE.CONFIG, "/source/vendor",
    ]
    before = time.perf_counter_ns()
    result = BASE.attached(
        probe, "cargo-vendor", mounts=[(source, "/source", False),
                                      (registry, "/rust-mcp-registry", True)],
        entrypoint="/opt/rust/bin/cargo", arguments=argv,
    )
    duration = time.perf_counter_ns() - before
    probe.require(result.returncode == 0, "cargo_vendor_offline_locked_succeeded",
                  stderr=result.stderr.decode("utf-8", "replace"),
                  stdout=result.stdout.decode("utf-8", "replace"))
    files, metadata, export = export_bounded(probe, "generated-source", source, "/source")
    BASE.close_case(probe, source, registry, [sg, rg])
    vendor, vendor_meta, checksum = validate_vendor(files, metadata, local_manifest)
    return vendor, vendor_meta, {
        "argv": ["/opt/rust/bin/cargo", *argv],
        "duration_ns": duration,
        "stdout": result.stdout.decode("utf-8", "replace"),
        "source_export": export,
        "checksum_semantics": checksum,
    }


def verify_cargo_identity(probe):
    source = probe.create_volume("cargo-version-source", "1m", 64)
    guard = BASE.guardian(probe, "cargo-version-source", source, "/source")
    result = BASE.attached(
        probe, "cargo-version", mounts=[(source, "/source", True)],
        entrypoint="/opt/rust/bin/cargo", arguments=["--version", "--verbose"],
    )
    text = result.stdout.decode("utf-8", "replace")
    probe.require(
        result.returncode == 0 and "cargo 1.98.1" in text
        and "797e8a9bca276c1c9f9f738d2a20f484fa4eea9d" in text,
        "cargo_exact_identity", stdout=text,
    )
    probe.remove_container(guard)
    probe.remove_volume(source)
    return text


def positive_case(probe, vendor_files, suffix, manifest, expected):
    initial = BASE.source_files(manifest)
    source, vendor, sg, vg, transfer = make_vendor_case(
        probe, suffix, initial, vendor_files
    )
    before = time.perf_counter_ns()
    first = cargo_vendor_metadata(probe, f"{suffix}-resolve", source, vendor, frozen=False)
    first_ns = time.perf_counter_ns() - before
    probe.require(first.returncode == 0, f"{suffix}_offline_resolution_succeeded",
                  stderr=first.stderr.decode("utf-8", "replace"))
    packages = BASE.package_names(json.loads(first.stdout))
    probe.require(packages == sorted(expected), f"{suffix}_exact_packages", packages=packages)
    after, _, _ = export_bounded(probe, f"{suffix}-source-after", source, "/source")
    changed = [name for name in sorted(after) if after[name] != initial.get(name)]
    probe.require(changed == ["Cargo.lock"], f"{suffix}_only_lock_changed", changed=changed)
    before = time.perf_counter_ns()
    frozen = cargo_vendor_metadata(probe, f"{suffix}-frozen", source, vendor, frozen=True)
    frozen_ns = time.perf_counter_ns() - before
    probe.require(frozen.returncode == 0, f"{suffix}_second_frozen_succeeded",
                  stderr=frozen.stderr.decode("utf-8", "replace"))
    denied = BASE.attached(
        probe, f"{suffix}-vendor-write-denied",
        mounts=[(vendor, "/rust-mcp-vendor", True)], entrypoint="/usr/bin/dd",
        arguments=["if=/dev/zero", "of=/rust-mcp-vendor/write-probe", "bs=1", "count=1",
                   "status=none"],
    )
    probe.require(denied.returncode != 0, f"{suffix}_vendor_mount_readonly",
                  stderr=denied.stderr.decode("utf-8", "replace"))
    vendor_after, _, _ = export_bounded(probe, f"{suffix}-vendor-after", vendor,
                                        "/rust-mcp-vendor")
    probe.require(vendor_after == vendor_files, f"{suffix}_vendor_unchanged")
    close_vendor_case(probe, source, vendor, [sg, vg])
    return {
        "packages": packages,
        "lock_sha256": sha256(after["Cargo.lock"]),
        "approved_candidate_count": 1,
        "transfer": transfer,
        "offline_resolution_duration_ns": first_ns,
        "frozen_resolution_duration_ns": frozen_ns,
    }


def negative_case(probe, full_vendor, suffix, mutation, expected_error):
    vendor_files = dict(full_vendor)
    corrupted_path = None
    if mutation == "missing_data":
        vendor_files = {name: value for name, value in vendor_files.items()
                        if not name.startswith("quote-1.0.47/")}
    elif mutation == "checksum_mismatch":
        checksum = json.loads(vendor_files["quote-1.0.47/.cargo-checksum.json"])
        # Cargo metadata need not read arbitrary source files. Mutate the
        # normalized manifest with valid TOML whitespace so the failure oracle
        # is checksum verification rather than parsing.
        candidates = ["Cargo.toml"] if "Cargo.toml" in checksum["files"] else sorted(checksum["files"])
        corrupted_path = "quote-1.0.47/" + candidates[0]
        vendor_files[corrupted_path] += b"\n# D05-CORRUPTION\n"
    else:
        raise AssertionError(mutation)
    initial = BASE.source_files(BASE.TRANSITIVE_MANIFEST)
    source, vendor, sg, vg, transfer = make_vendor_case(
        probe, suffix, initial, vendor_files
    )
    metadata_result = cargo_vendor_metadata(probe, f"{suffix}-resolve", source, vendor, frozen=False)
    # Metadata resolves from normalized manifests and does not necessarily read
    # every source file. For the checksum mutation, force Cargo to consume the
    # source and record that distinction explicitly.
    result = (cargo_vendor_check(probe, f"{suffix}-check", source, vendor)
              if mutation == "checksum_mismatch" and metadata_result.returncode == 0
              else metadata_result)
    stderr = result.stderr.decode("utf-8", "replace")
    probe.require(result.returncode == 101 and expected_error in stderr.lower(),
                  f"{suffix}_denied", exit_code=result.returncode, stderr=stderr)
    after, _, _ = export_bounded(probe, f"{suffix}-source-after", source, "/source")
    changed = [name for name in sorted(after) if after[name] != initial.get(name)]
    probe.require(all(name == "Cargo.lock" for name in changed),
                  f"{suffix}_no_nonlock_source_effect", changed=changed)
    vendor_after, _, _ = export_bounded(probe, f"{suffix}-vendor-after", vendor,
                                        "/rust-mcp-vendor")
    probe.require(vendor_after == vendor_files, f"{suffix}_vendor_unchanged")
    close_vendor_case(probe, source, vendor, [sg, vg])
    return {
        "exit_code": result.returncode,
        "stderr": stderr,
        "metadata_exit_code": metadata_result.returncode,
        "metadata_stderr": metadata_result.stderr.decode("utf-8", "replace"),
        "mutation": mutation,
        "corrupted_path": corrupted_path,
        "staging_changed_files": changed,
        "approved_candidate_count": 0,
        "transfer": transfer,
    }


def main():
    started = time.perf_counter_ns()
    nonce = secrets.token_hex(8)
    local_manifest, registry_files = local_registry_files()
    report = {
        "started_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "scope": "D05 directory-source comparison only; not production acceptance or M2-04 completion",
        "product_gate": "not_evaluated",
        "approved_image": BASE.IMAGE,
        "cargo_expected": "cargo 1.98.1 (797e8a9bc 2026-08-05)",
        "seccomp_sha256": sha256(BASE.SECCOMP.read_bytes()),
        "script_sha256": sha256(Path(__file__).read_bytes()),
        "source_registry_fixture_fingerprint": local_manifest["registry_tree_fingerprint"],
        "run_nonce": nonce,
        "bounds": {
            "archive_bytes": MAX_ARCHIVE_BYTES, "entries": MAX_ENTRIES,
            "single_file_bytes": MAX_FILE_BYTES, "logical_file_bytes": MAX_TOTAL_BYTES,
            "path_bytes": MAX_PATH_BYTES, "path_depth": MAX_DEPTH,
        },
    }
    status = 70
    probe = None
    with tempfile.TemporaryDirectory(prefix="rust-mcp-m2-d05-vendor-config-") as config_name:
        config = Path(config_name)
        config.chmod(0o700)
        (config / "config.json").write_text("{}\n")
        probe = BASE.Probe(config, nonce)
        try:
            probe.ok(["version", "--format", "{{json .}}"])
            image = json.loads(probe.ok(["image", "inspect", BASE.IMAGE]).stdout)
            probe.require(len(image) == 1 and image[0]["Id"] == BASE.IMAGE,
                          "approved_image_identity", image_id=image[0]["Id"] if image else None)
            report["cargo_observed"] = verify_cargo_identity(probe)
            vendor, vendor_meta, generation = generate_vendor(
                probe, local_manifest, registry_files
            )
            fixture_manifest = materialize_fixture(
                vendor, vendor_meta, local_manifest, generation
            )
            report["generation"] = generation
            report["fixture"] = {
                "vendor_tree_fingerprint": fixture_manifest["vendor_tree_fingerprint"],
                "regular_file_count": len(vendor),
                "logical_file_bytes": sum(len(value) for value in vendor.values()),
                "directory_count": len({parent for name in vendor for parent in BASE.parents(name)}),
                "ustar_bytes": len(BASE.archive(vendor, readonly=True)),
                "metadata_kinds": sorted({item["kind"] for item in vendor_meta.values()}),
                "source_mode_values": sorted({item["mode"] for item in vendor_meta.values()}),
            }
            report["comparison"] = {
                "local_registry_regular_files": len(registry_files),
                "local_registry_logical_bytes": sum(len(value) for value in registry_files.values()),
                "local_registry_ustar_bytes": len(BASE.archive(registry_files, readonly=True)),
                "vendor_regular_files": len(vendor),
                "vendor_logical_bytes": sum(len(value) for value in vendor.values()),
                "vendor_ustar_bytes": len(BASE.archive(vendor, readonly=True)),
            }
            report["comparison"]["same_run_transfer"] = compare_transfer(
                probe, registry_files, vendor
            )
            report["experiments"] = {
                "basic": positive_case(
                    probe, vendor, "vendor-basic", BASE.BASIC_MANIFEST,
                    [("d05-fixture", "0.1.0"), ("unicode-ident", "1.0.24")],
                ),
                "alias_optional_feature_transitive": positive_case(
                    probe, vendor, "vendor-transitive", BASE.TRANSITIVE_MANIFEST,
                    [("d05-fixture", "0.1.0"), ("proc-macro2", "1.0.107"),
                     ("quote", "1.0.47"), ("unicode-ident", "1.0.24")],
                ),
                "missing_data": negative_case(
                    probe, vendor, "vendor-missing", "missing_data", "no matching package named"
                ),
                "checksum_mismatch": negative_case(
                    probe, vendor, "vendor-checksum", "checksum_mismatch", "checksum"
                ),
            }
            report["approved_candidate_count_for_negative_cases"] = 0
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
    report["events"] = BASE.EVENTS
    report["finished_at_utc"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    report["total_duration_ns"] = time.perf_counter_ns() - started
    report["limitations"] = [
        "The fixture proves only the retained three-crate closure on the recorded arm64 Docker daemon.",
        "The experiment does not accept D05, implement production validation, or complete M2-04.",
        "Cargo directory checksums detect accidental modification; they do not authenticate publishers.",
        "A developer must generate vendor data with an already installed Cargo; the MCP runtime never runs host Cargo.",
    ]
    RAW_REPORT.parent.mkdir(parents=True, exist_ok=True)
    RAW_REPORT.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    print(json.dumps({key: report[key] for key in (
        "experiment_status", "product_gate", "fixture", "comparison",
        "approved_candidate_count_for_negative_cases", "final_cleanup_inventory",
        "total_duration_ns",
    ) if key in report}, indent=2, sort_keys=True))
    return status


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as error:
        print(json.dumps({"experiment_status": "startup_failure", "error_type": type(error).__name__,
                          "error": str(error)}, indent=2))
        sys.exit(70)
