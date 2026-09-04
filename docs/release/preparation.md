# M1 release preparation — source published, binaries not approved

Prepared 2026-09-04 against development baseline
`463bab799da4b2cb3999f6f083d91e2dbd8641f9`. This document prepares M1-15;
The owner subsequently approved public source publication in ADR-047. This document
still does not close M1-15 or authorize binary, model or catalog distribution.
The remaining decisions stay in [implementation status](../implementation-status.md).

## Concrete owner decisions

| Decision | Reviewable proposal | Approval/evidence still required |
| --- | --- | --- |
| Product license | Original code is dual-licensed under `MIT OR Apache-2.0`; copyright IUMotion Labs | **Resolved for source** by ADR-047 and root license files; third-party obligations remain separate |
| Distribution | IUMotion Labs publishes source through `pharos-lang/rust-engineering-mcp`; GitHub Releases is the initial binary channel | Source resolved; target notices/native evidence and production catalog-key custody remain required before binary/catalog release |
| Snapshot source | Preserve provenance and licensed source material for each included fact/document | Source/registry terms review and explicit redistribution decision before public sync |
| Platforms | Preserve the specification's Linux/macOS/Windows core/protocol/catalog requirement | Native runners and declared architecture matrix; narrowing requires an explicit principal decision and ADR/spec/status updates |

The license choice is not a legal assurance. Cargo publication remains disabled;
source publication does not authorize model, native or catalog artifacts.

## Notices inventory procedure

Run this procedure on the exact candidate commit, separately for each released
target and feature set. The local candidate inventory below has been executed; this procedure remains
required for the eventual release target and linked binary.

1. Record commit, Cargo.lock SHA256, toolchain identity, target, enabled features,
   binary hash, linker/build inputs and native artifact hashes. Use the pinned
   installed toolchain; absence of cached dependencies is a blocker, not permission
   to fetch. Resolve package metadata with
   `cargo +1.98.1 metadata --locked --offline --format-version 1 --all-features`.
   Metadata does not establish which native objects a final binary contains.
2. Join resolved package IDs to Cargo.lock by name/version/source, retaining
   registry checksums. For each package read its cached manifest's `license` and
   `license-file` and actual LICENSE/COPYING/NOTICE files. Record package ID, version,
   source, checksum, declared SPDX expression, license-text hash, notice-text hash,
   shipped/build-only role and unresolved obligations. Do not infer one license
   from the repository's dominant license or silently discard missing text.
3. Inspect bundled native sources and build-script outputs independently: static
   native libraries and their third-party components are not fully described by
   Rust package SPDX fields. Repeat against the final link/build manifest.
4. Preserve vendored LanceDB provenance and the exact manifest-only patch under
   [ADR-027](../adr/ADR-027-semantic-offline-foundation.md). Its local Cargo.toml
   declares Apache-2.0; this declaration alone is not a complete notice bundle.
5. Join the five model files to
   [model-receipt.json](../../fixtures/semantic/model-receipt.json): E5 revision
   `614241f622f53c4eeff9890bdc4f31cfecc418b3`, tokenizer/config/model hashes,
   and publisher-declared MIT metadata. The receipt records no separate license
   file in the inspected listing. Resolve license-text/notice packaging explicitly;
   the receipt is development provenance, not a signed distribution authorization.
6. For static ORT 1.24.2, retain the exact native hash from the candidate's receipt,
   build options, target and all linked third-party notices. The upstream
   [versioned LICENSE](https://github.com/microsoft/onnxruntime/blob/v1.24.2/LICENSE)
   is MIT; the separate
   [ThirdPartyNotices](https://github.com/microsoft/onnxruntime/blob/v1.24.2/ThirdPartyNotices.txt)
   must also be reviewed against the actual build. Both pages were inspected on
   2026-09-04; neither authenticates the installed binary.
7. Produce a candidate `THIRD_PARTY_NOTICES` and machine-readable inventory outside
   the shipped tree for review, preserving exact license texts. Review all unknown,
   missing and conflicting entries before enabling a licenses gate. Existing
   `cargo deny` advisories/bans/sources success is not license approval.

The immutable [E5 model card](https://huggingface.co/intfloat/multilingual-e5-small/raw/614241f622f53c4eeff9890bdc4f31cfecc418b3/README.md)
was fetched for this preparation. The local receipt remains the source for the
specific development files and their hashes. No model or runtime was downloaded.

## Candidate bundle and offline installation review

Prepare, without publishing: binary with feature `local`, target-specific native
provenance, model bundle, catalog manifest/payload, notices, checksums, versioned
schemas and install/doctor/client instructions. Record compressed and installed
sizes. Do not invent a production key to make a demo look distributable.

Before M1-15 is Done, demonstrate installation into a fresh protected directory
without package/model fetching; verify signatures/hashes and doctor output; start
stdio with explicit trusted roots/runtime policy; exercise all thirteen tools,
Resources, degraded lexical mode and cancellation through actual clients.
Uninstall must identify only owned artifacts. Demonstrate failure for absent or
incompatible assets rather than silently provisioning them.

Read-only host inventory: macOS26.6.2/Darwin25.6.0 ARM64; installed active Rust1.98.1;
only `aarch64-apple-darwin` target observed; Claude Code2.1.259 and Codex CLI0.153.0
installed. This proves neither model access nor client compatibility. Native
Linux/Windows/x86_64 runners, full client qualification, license approval and production signing
remain pending. Inspector2.5.0 was installed in an isolated local directory with
explicit owner approval; its CLI qualification is recorded separately. See [release matrix](../validation/M1-17-matrix.md).

## Executed local preparation

[Inventory](inventory.json) and [candidate exact notices](THIRD_PARTY_NOTICES.candidate.txt)
were regenerated offline from587 resolved packages (579 third parties),991 distinct
local product/third-party texts. Thirty third-party packages had no local text.
[Upstream supplement](upstream-licenses/README.md) retains
original texts at exact VCS revisions for30 of31 third-party gaps, plus versioned
ORT LICENSE/ThirdPartyNotices and the E5 model card. Root/monorepo applicability is
an explicitly recorded inference requiring review. Kanaria0.2.0 source404 and E5
missing license text remain unresolved; declarations are not substituted for text.

These evidence gaps are separate from the owner's product-license decision. Static
ORT object/options attribution and final per-target notice inventories also remain
open. The owner decision cannot make missing third-party/native evidence disappear,
and completing that evidence cannot choose the product license, copyright holder,
publisher/channel or signing-key custodian.

Reproduce current license inputs with `scripts/release-inventory.py --check --ort-dir`
pointing at the recorded installed ORT directory. Check preserves the artifact's
recorded source revision while recomputing lock, package manifests, features,
script and all local text bytes. It does not attest arbitrary source changes or
binary linkage. Git attributes preserve original CRLF/whitespace and byte offsets
for these third-party texts; this exception does not apply to source code.

[Local offline candidates](offline-candidates.md) now supply actual optimized core
and local binaries, source/asset hashes, private installation, eleven passive/import
CLI checks and two active release-doctor checks with joined cleanup. Archives were
stream-read and every member hash verified. These are review artifacts, not a
release or approved distribution. The product grant and source publisher are now
resolved; complete native/model notice assessment, catalog-key custody and native
runners remain pending.
