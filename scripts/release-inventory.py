#!/usr/bin/env python3
"""Offline candidate inventory; declarations/texts are evidence, not legal approval.

No dependency installation, network, build scripts, or binary linkage inference.
Use --check to reproduce existing outputs without writing them.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "docs/release"
TEXT_NAME = re.compile(r"^(?:licen[sc]e|copying|notice|third[-_]?party[-_]?notices)(?:$|[._-])", re.I)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def command(*args):
    return subprocess.check_output(args, cwd=ROOT, text=True).strip()


def location(path):
    path = Path(path)
    try:
        return str(path.relative_to(ROOT))
    except ValueError:
        marker = "/registry/src/"
        if marker in str(path):
            return "$CARGO_REGISTRY_SRC/" + str(path).split(marker, 1)[1]
        return str(path)


def package_id(value):
    return value.replace(str(ROOT), "$WORKSPACE")


def text_kind(path):
    return "notice" if "notice" in Path(path).name.lower() else "license_or_copying"


def texts(root, explicit=None):
    """Walk only the selected package/artifact tree; do not follow symlinks."""
    candidates = set()
    skipped = []
    for directory, dirs, files in os.walk(root, followlinks=False):
        dirs[:] = sorted(d for d in dirs if d not in {".git", "target"}
                         and not (Path(directory) / d).is_symlink())
        for filename in sorted(files):
            if TEXT_NAME.match(filename):
                candidates.add(Path(directory) / filename)
    if explicit:
        explicit_path = Path(explicit)
        if explicit_path.resolve().is_relative_to(Path(root).resolve()):
            candidates.add(explicit_path)
        else:
            skipped.append({"path": location(explicit_path),
                            "reason": "declared_license_file_outside_selected_package_not_read"})
    found = []
    for path in sorted(candidates):
        if path.is_symlink() or not path.is_file():
            skipped.append({"path": location(path), "reason": "missing_or_nonregular_license_file"})
            continue
        # Explicit names only: do not copy arbitrary neighboring cache content.
        raw = path.read_bytes()
        found.append((path, raw))
    return found, skipped


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ort-dir", type=Path, required=True,
                        help="Existing approved native artifact directory; never provisioned")
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    source_commit = command("git", "rev-parse", "HEAD")
    if args.check:
        # Recheck the recorded inventory inputs, not the commit containing this artifact.
        # Lock, package manifests, features, texts and this script are still recomputed.
        source_commit = json.loads((OUTPUT / "inventory.json").read_text())["git_commit"]
        if not re.fullmatch(r"[0-9a-f]{40}", source_commit):
            raise SystemExit("Invalid recorded source revision")
    lock_bytes = (ROOT / "Cargo.lock").read_bytes()
    lock = tomllib.loads(lock_bytes.decode())
    lock_by_key = {(p["name"], p["version"], p.get("source")): p for p in lock["package"]}
    metadata_command = ["cargo", "+1.98.1", "metadata", "--locked", "--offline",
                        "--format-version", "1", "--all-features"]
    metadata = json.loads(command(*metadata_command))
    nodes = {n["id"]: n for n in metadata["resolve"]["nodes"]}
    roles = {pid: set() for pid in nodes}
    for node in nodes.values():
        for dep in node["deps"]:
            for kind in dep["dep_kinds"]:
                roles[dep["pkg"]].add(kind["kind"] or "normal")
    members = set(metadata["workspace_members"])
    product_license_files = [
        ROOT / "LICENSE",
        ROOT / "LICENSE-APACHE",
        ROOT / "LICENSE-MIT",
        ROOT / "NOTICE",
    ]
    for path in product_license_files:
        if not path.is_file():
            raise SystemExit(f"Missing product license/notice file: {path.relative_to(ROOT)}")
    notices = bytearray(
        b"CANDIDATE THIRD-PARTY TEXT INVENTORY -- NOT APPROVED FOR DISTRIBUTION\n"
        b"Exact locally found texts follow; byte offsets/hashes are in inventory.json.\n"
        b"Includes resolved all-target normal/build/dev dependencies and nested source texts.\n"
        b"Presence is not license compatibility, fulfillment of obligations, or linkage evidence.\n"
        b"Missing texts and native/model gaps must be reviewed before distribution.\n"
    )

    def append_texts(owner, files):
        entries = []
        for path, raw in files:
            reference = location(path)
            header = f"\n===== {owner} :: {reference} :: sha256:{sha(raw)} =====\n".encode()
            notices.extend(header)
            offset = len(notices)
            notices.extend(raw)
            notices.extend(b"\n===== END EXACT SOURCE BYTES =====\n")
            entries.append({"path": reference, "kind": text_kind(path), "bytes": len(raw), "sha256": sha(raw),
                            "candidate_offset": offset, "candidate_length": len(raw)})
        return entries

    packages = []
    for package in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"], p["id"])):
        if package["id"] not in nodes:
            continue
        key = package["name"], package["version"], package["source"]
        if key not in lock_by_key:
            raise SystemExit(f"Resolved package missing from Cargo.lock: {key}")
        locked = lock_by_key[key]
        manifest = Path(package["manifest_path"])
        manifest_raw = manifest.read_bytes()
        declared = tomllib.loads(manifest_raw.decode()).get("package", {})
        explicit = package.get("license_file")
        if explicit and not Path(explicit).is_absolute():
            explicit = manifest.parent / explicit
        found, skipped = texts(manifest.parent, explicit)
        workspace = package["id"] in members
        if workspace:
            found = [(path, path.read_bytes()) for path in product_license_files]
        unresolved = ["final_binary_linkage_and_license_obligations_not_assessed"]
        if not package.get("license"):
            unresolved.append("missing_declared_spdx_expression")
        if not found:
            unresolved.append("no_local_license_notice_text_found")
        if not any(text_kind(path) == "license_or_copying" for path, _ in found):
            unresolved.append("no_local_license_or_copying_text_found")
        if skipped:
            unresolved.append("missing_or_skipped_declared_or_discovered_text")
        if workspace:
            # Original project code is not part of the third-party notices body.
            text_entries = [{"path": location(p), "kind": text_kind(p), "bytes": len(b), "sha256": sha(b)} for p, b in found]
            unresolved.append("original_project_code_excluded_from_third_party_notice_body")
        else:
            text_entries = append_texts(f"{package['name']} {package['version']}", found)
        packages.append({
            "id": package_id(package["id"]), "name": package["name"], "version": package["version"],
            "source": package["source"], "lock_checksum": locked.get("checksum"),
            "manifest": location(manifest), "manifest_sha256": sha(manifest_raw),
            "declared_license": package.get("license"),
            "manifest_license_field": declared.get("license"),
            "license_file": location(explicit) if explicit else None,
            "workspace_member": workspace,
            "vendored": not workspace and package["source"] is None,
            "enabled_features": sorted(nodes[package["id"]]["features"]),
            "incoming_dependency_kinds": sorted(roles[package["id"]]),
            "target_kinds": sorted({k for t in package["targets"] for k in t["kind"]}),
            "shipped_build_role": "unresolved; metadata edge kinds are not final linkage",
            "texts": text_entries, "skipped_texts": skipped, "unresolved": unresolved,
        })

    ort_dir = args.ort_dir.resolve(strict=True)
    ort_library = ort_dir / "libonnxruntime.a"
    with ort_library.open("rb") as handle:
        ort_hash = hashlib.file_digest(handle, "sha256").hexdigest()
    ort_texts, ort_skipped = texts(ort_dir)
    receipt_path = ROOT / "fixtures/semantic/model-receipt.json"
    receipt_raw = receipt_path.read_bytes()
    receipt = json.loads(receipt_raw)
    native = {
        "onnxruntime": {
            "version_claim_source": "docs/validation/M0-09.md",
            "version_claim": "1.24.2", "target": "aarch64-apple-darwin",
            "artifact": str(ort_library), "bytes": ort_library.stat().st_size, "sha256": ort_hash,
            "matches_recorded_development_sha256": ort_hash == "4d53c916ea95f09203324f9aad7b76f75c16d8a4bc98f8a949ea0ac73c07604d",
            "local_text_search_root": str(ort_dir),
            "texts": append_texts("native ONNX Runtime development artifact", ort_texts),
            "skipped_texts": ort_skipped,
            "unresolved": ["static_archive_component_linkage_and_build_options_not_verified",
                           "upstream_license_declaration_does_not_authenticate_local_binary"]
                          + ([] if ort_texts else ["no_license_or_third_party_notices_in_native_artifact_directory"]),
        },
        "e5_model": {
            "receipt": location(receipt_path), "receipt_sha256": sha(receipt_raw),
            "repository": receipt["repository"], "revision": receipt["revision"],
            "publisher_declared_license": receipt["license_provenance"]["publisher_declared_spdx"],
            "files_from_receipt": receipt["files"], "actual_model_files_rehashed": False,
            "texts": [], "unresolved": ["receipt_records_no_separate_license_file",
                                         "model_license_notice_packaging_unresolved",
                                         "development_receipt_is_not_distribution_authorization"],
        },
    }
    patch = ROOT / "vendor/lancedb-manifest-only.patch"
    vendored = {
        "package": "lancedb 0.31.0", "decision": "docs/adr/ADR-027-semantic-offline-foundation.md",
        "published_crate_sha256_from_adr": "2bd0b54bb1cdd075efa5a8827ec16dcf5c0781253cd88e63988c174915c53fe2",
        "patch": location(patch), "patch_sha256": sha(patch.read_bytes()),
        "verification_script": "scripts/verify-vendor.py",
        "upstream_archive_reverified_this_run": False,
    }
    if (ROOT / "Cargo.lock").read_bytes() != lock_bytes:
        raise SystemExit("Cargo.lock changed during inventory; rerun on stable inputs")
    inventory = {
        "schema": "rust-mcp-release-inventory-candidate-v1", "approval": "none; preparatory evidence only",
        "git_commit": source_commit, "cargo_lock_sha256": sha(lock_bytes),
        "script_sha256": sha(Path(__file__).read_bytes()), "metadata_command": metadata_command,
        "rustc": command("rustc", "+1.98.1", "-vV"),
        "resolution_scope": "all-features all-targets workspace resolve graph, including build/dev; no filter-platform",
        "license_text_scan": "recursive LICENSE/LICENCE/COPYING/NOTICE/ThirdPartyNotices names plus license-file; symlinks skipped; source text presence is not obligation classification",
        "scan_limits": ["embedded_source_header_licenses_not_extracted",
                        "nested_fixture_and_bundled_native_texts_not_classified_as_shipped",
                        "declared_license_expressions_not_independently_validated_as_SPDX",
                        "workspace_is_in_development_not_a_frozen_release_candidate"],
        "binary_hash": None, "final_link_manifest": None,
        "product_license": {
            "spdx": "MIT OR Apache-2.0",
            "copyright": "2026 IUMotion Labs",
            "texts": [{"path": location(p), "bytes": p.stat().st_size,
                       "sha256": sha(p.read_bytes())} for p in product_license_files],
            "decision": "docs/adr/ADR-047-publication-license-and-delivery.md",
        },
        "packages": packages, "vendored_provenance": vendored, "native_and_model": native,
        "summary": {"resolved_packages": len(packages),
                    "third_party_packages": sum(not p["workspace_member"] for p in packages),
                    "packages_without_declared_license": sum(not p["declared_license"] for p in packages),
                    "packages_without_local_text": sum(not p["texts"] for p in packages),
                    "text_files": len({t["path"] for p in packages for t in p["texts"]}) + len(ort_texts)},
        "candidate_notices_sha256": sha(notices),
    }
    outputs = {OUTPUT / "inventory.json": (json.dumps(inventory, indent=2, sort_keys=True) + "\n").encode(),
               OUTPUT / "THIRD_PARTY_NOTICES.candidate.txt": bytes(notices)}
    for path, data in outputs.items():
        if args.check:
            if not path.is_file() or path.read_bytes() != data:
                raise SystemExit(f"Not reproducible: {path.relative_to(ROOT)}")
        else:
            path.write_bytes(data)
        print(f"{sha(data)}  {path.relative_to(ROOT)}")
    print(json.dumps(inventory["summary"], sort_keys=True))


if __name__ == "__main__":
    main()
