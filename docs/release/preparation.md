# M1 release preparation — historical pre-publication procedure

Final outcome: M1-15 closed on 2026-09-05 with the stable
[GitHub Release v0.1.0](https://github.com/pharos-lang/rust-engineering-mcp/releases/tag/v0.1.0).
The [public receipt](../validation/m1-17-public-release.json) supersedes this file
for final tag, CI, asset, smoke and provenance facts. This procedure remains the
historical preparation record and does not authorize any excluded artifact.

Prepared 2026-09-04 against development baseline
`463bab799da4b2cb3999f6f083d91e2dbd8641f9`. This document prepares M1-15;
The owner subsequently approved public source publication in ADR-047 and the 0.1.0
artifact/host/catalog boundary in ADR-048. This document alone does not close M1-15
or authorize model, local-feature or catalog distribution.
The completed executable gates are linked from [implementation status](../implementation-status.md).

## Concrete owner decisions

| Decision | Reviewable proposal | Approval/evidence still required |
| --- | --- | --- |
| Product license | Original code is dual-licensed under `MIT OR Apache-2.0`; copyright IUMotion Labs | **Resolved for source** by ADR-047 and root license files; third-party obligations remain separate |
| Distribution | One core `aarch64-apple-darwin` archive through GitHub Releases | Final target closure, SBOM/notices, install/smoke and attestation evidence |
| Snapshot source | No official catalog in 0.1.0; host-supplied catalogs retain provenance | Future official publication requires a new source/terms/trust decision |
| Platforms | macOS26 ARM64/APFS positive; Docker Linux ARM64 guest; Linux/Windows portable/fail-closed | Resolved by ADR-048; no other 0.1.0 artifacts advertised |

The license choice is not a legal assurance. Cargo publication remains disabled;
source publication does not authorize model, native or catalog artifacts.

## Notices inventory procedure

Run this procedure on the exact candidate commit for the single released core
target. The local all-feature inventory below remains source-qualification evidence;
the release procedure must compute the actual target-filtered dependency closure.

1. Record commit, Cargo.lock SHA256, toolchain identity, target, enabled features,
   binary hash, linker/build inputs and native artifact hashes. Use the pinned
   installed toolchain; absence of cached dependencies is a blocker, not permission
   to fetch. Resolve package metadata with
   `cargo +1.98.1 metadata --locked --offline --filter-platform
   aarch64-apple-darwin --format-version 1` using the root package's default core
   features. The separate source-qualification inventory may use `--all-features`;
   it must not be substituted for the shipped closure. Metadata does not establish
   which native objects a final binary contains.
2. Join resolved package IDs to Cargo.lock by name/version/source, retaining
   registry checksums. For each package read its cached manifest's `license` and
   `license-file` and actual LICENSE/COPYING/NOTICE files. Record package ID, version,
   source, checksum, declared SPDX expression, license-text hash, notice-text hash,
   shipped/build-only role and unresolved obligations. Do not infer one license
   from the repository's dominant license or silently discard missing text.
3. Inspect bundled native sources and build-script outputs independently: static
   native libraries and their third-party components are not fully described by
   Rust package SPDX fields. Repeat against the final link/build manifest.
4. For source qualification, preserve vendored LanceDB provenance and the exact manifest-only patch under
   [ADR-027](../adr/ADR-027-semantic-offline-foundation.md). Its local Cargo.toml
   declares Apache-2.0; this declaration alone is not a complete notice bundle.
5. For source qualification, join the five model files to
   [model-receipt.json](../../fixtures/semantic/model-receipt.json): E5 revision
   `614241f622f53c4eeff9890bdc4f31cfecc418b3`, tokenizer/config/model hashes,
   and publisher-declared MIT metadata. The receipt records no separate license
   file in the inspected listing. Resolve license-text/notice packaging explicitly;
   the receipt is development provenance, not a signed distribution authorization.
6. For source qualification, retain the static ORT 1.24.2 identity and notices.
   None of Kanaria, E5, ORT or LanceDB may appear in the core archive closure. For
   ORT retain the exact native hash from the candidate's receipt,
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

Prepare, without publishing, one core `aarch64-apple-darwin` binary with target-specific
dependency inventory, SPDX SBOM, exact notices, manifest, checksums and
install/doctor/client instructions. Record compressed and installed sizes. Reject
model, ORT, LanceDB, catalog, trust, fixture, Docker and toolchain members.

Before M1-15 is Done, demonstrate installation from the final archive into a fresh
protected directory without fetching; verify hashes and doctor output; start stdio,
verify discovery and all thirteen definitions, and exercise the expected structured
degraded/unavailable paths. Full positive tools, Resources, local semantics and
cancellation remain part of the source-bound M1 qualification.
Uninstall must identify only owned artifacts. Demonstrate failure for absent or
incompatible assets rather than silently provisioning them.

Read-only host inventory: macOS26.6.2/Darwin25.6.0 ARM64; installed active Rust1.98.1;
only `aarch64-apple-darwin` target observed; Claude Code2.1.260 and Codex CLI0.153.0
installed. Historical Inspector/direct Codex evidence exists, but a candidate-bound
model-driven run and final Inspector repetition remain pending. Linux/Windows CI is
portable/fail-closed, not positive host qualification. Inspector2.5.0 was installed in an isolated local directory with
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
runs remain pending for final closure. ADR-048 excludes local/model/native assets
from the release archive, so their unresolved redistribution evidence remains a
source-builder limitation rather than a 0.1.0 artifact blocker.

The separate [native retrieval benchmark](../research/m1-16/benchmark/REPORT.md)
records one bounded 8-query/15-crate descriptive run, including latency and sampled
RSS. It does not establish general quality, multilingual coverage or agent utility;
the saturated paired utility pilot remains a distinct experiment with no equivalence
or causal product-value conclusion.
