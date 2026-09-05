# M1-15 offline candidate/bootstrap design — local review only

Current disposition under ADR-048: every artifact and hash below is historical
source-qualification evidence. These core/local archives are not 0.1.0 release
inputs and must not be promoted. The release boundary is a newly built, source-bound
core `aarch64-apple-darwin` archive containing no model, ORT, LanceDB, catalog,
trust, fixture, Docker image or toolchain. Historical receipts remain unchanged.

Prepared 2026-09-04; accepted compiled inputs match main `01a90ab6`. This is not a release,
distribution approval, product-license choice or production publisher designation.
Only candidate artifacts under `target/m1-15-candidate/` are created. No global
installation, package acquisition, model download or Docker provisioning is allowed.

## Build and identity

The source manifest records SHA256 of every selected Rust source, package manifest,
build script, vendored Rust source and workspace/toolchain/lock input. Record HEAD
and dirty status: the pre-commit candidate is not falsely attributed to clean HEAD.
Compare the source manifest before and after each build. Any review correction
requires refreezing/rebuilding; the candidate cannot silently inherit another source.

Use installed exact Cargo/Rust 1.98.1, `--release --locked --offline`, no default
features for core and explicit `--features local` for local. Clear unrelated
environment, set `CARGO_INCREMENTAL=0`, `ORT_SKIP_DOWNLOAD=1` and the verified
`ORT_LIB_LOCATION`; use `target/semantic-compat` and two compiler jobs. Copy the
core binary immediately before the local build overwrites the shared binary path.
Native target cache originally has debug artifacts only: release is a cold optimized
build, possibly tens of minutes. This is an estimate, not a measured promise.

The existing ORT archive was rehashed: 73,696,536 bytes,
`4d53c916ea95f09203324f9aad7b76f75c16d8a4bc98f8a949ea0ac73c07604d`.
Its directory contains only `libonnxruntime.a`, matching the calibrated development
input. Upstream version provenance says ORT 1.24.2; matching this development hash
does not authenticate a production native distribution.

The approved existing E5 path `/private/tmp/rust-mcp-e5-m009/onnx` was also rehashed:
all five files match the development receipt, totaling 487,352,503 bytes. The owner
authorized copying these bytes into this private local review candidate only.

`build-candidates.py` produces bounded logs, a source manifest and build receipt.
Each executable receipt records profile/features, exact command, elapsed time,
bytes and SHA256. The manifest must distinguish binary hash from source/lock hashes.
Core is a degraded-mode/control candidate; ADR027 requires local for eventual M1.

## Candidate layout and native evidence

- `core/bin/rust-engineering-mcp`, `local/bin/rust-engineering-mcp`.
- Shared development fixture bytes and public fixture trust, marked test-only.
- Exact pending product-license notice, current candidate third-party notices,
  inventory and exact upstream native/license supplement, with hashes.
- Per-binary Mach-O `otool -L` and `otool -l` observations, architecture/build target,
  static-native symbol observations where present, source and build receipts.
- Model is a separate asset: verify all five exact sizes/SHA256 from
  `fixtures/semantic/model-receipt.json` if an approved existing model path is supplied.
  No signature, production publisher or model redistribution approval is invented.
- `manifest.json` enumerates retained candidate files, sizes and hashes; transient
  installation/admin state has a separate receipt and never becomes release input.

Dynamic linkage must contain only expected platform libraries/frameworks; report
every actual dependency and fail on unresolved build-tree/private-cache dylib paths.
An absent ONNX dylib is consistent with static linking but is not proof that all
static objects or licensing obligations were audited. Preserve relevant release
build-script linker outputs and selected symbols to strengthen attribution.

The first provisional core release completed in approximately 118 seconds and
produced a 21,057,560-byte arm64 executable. Cargo exited 0 with a warning that its
`rust-objcopy` helper could not load `@rpath/libLLVM.dylib` for debug stripping.
No toolchain repair was attempted. The executable itself starts, and `otool -L`
shows only libiconv and libSystem, not libLLVM. This candidate predates M1-14 review
corrections and must be superseded by a source-matched rebuild; its observations
are not final release readiness evidence.

The existing all-feature inventory is a candidate superset, not a final per-binary
license bill of materials. ADR-047 later licensed original project code, but this
historical candidate predates that metadata change. The native archive directory
has no license files; the checked-in versioned ORT LICENSE and
ThirdPartyNotices provide upstream text evidence. Remaining native component/build
options, Kanaria text and E5 text-packaging questions stay explicit. No compatibility
or redistribution authorization follows from these copies or from cargo-deny results.

## Offline installation exercise

Create fresh owned mode0700 installation directories beneath the permitted candidate
root. Copy exact binary bytes, verify against the manifest, use executable mode0700.
Keep trust in an owned mode0600 file under its own mode0700 protected parent. Create
each catalog store mode0700 explicitly. Never turn the checkout trust path into an
implicitly accepted operational trust anchor.

Use empty runtime environment and explicit absolute paths. Run `version --json` and
passive `doctor --json` for both binaries; verify exact version, target and
`compiled_local`, and truthful not-configured/not-checked diagnostics. Import only
the checked-in signed development bundle through the real catalog CLI, then run
doctor with explicit store/trust. The expected catalog is publisher `fixture-only`,
channel `test`, sequence1, with stale timestamps preserved. This public test identity
is deliberately forgeable and is never an authorized publisher.

Keep a fixed orphan staging sentinel during passive doctor and verify its bytes and
the durable active/floor hashes remain unchanged; doctor must not take an admin
lease or clean staging. Exercise a missing configured asset and confirm failure
with a bounded JSON diagnostic, without acquiring or repairing anything.

If the five verified E5 files are supplied, copy and rehash them as a separate local
review asset, then exercise configured local-model doctor. An unconfigured passive
doctor cannot qualify native inference or semantic index readiness. Index generation
would require its own explicit bounded rebuild and subsequent real queries.

Active doctor executes calibration and needs the principal's serial Docker slot;
it is not part of this worker's uncoordinated installation smoke. The M1-14 gate's
debug binary result does not by itself certify the copied release executable.
Real-client qualification, thirteen-tool behavior, Resources, degraded mode and
cancellation remain separate gates; do not implement a parallel MCP transport here.

## Removal and promotion

Removal addresses only the freshly created installation subtree and files in its
ownership manifest. Preserve receipts and failed state for review. No global path,
Docker object, user catalog, existing toolchain or cache is removed. Do not copy
candidate outputs into `/usr/local`, Homebrew, application bundles or user profiles.

These archives are not promotion candidates. Their source, lock, binary and asset
hashes remain useful for the full `local` source-bound gate and historical review.
The distinct final core archive must satisfy ADR-048's target closure, SBOM, notices,
manifest, install and smoke gates. M1-15 cannot close from this exercise alone.

## Completed local evidence

The corrected source manifest SHA256 is
`f142775240951dcd3390e186750e36f07d5c4ae12ad4e622eed2e3c3a8427115`.
Its inputs still matched the workspace after installation and packaging. The first
native build completed successfully in 1007.974 seconds but was not promoted because
review changed source while it ran. Its receipts are retained in `history/pre-review`.
The final rebuild and source guard both passed, using the warmed dependency cache.

| Final artifact | Bytes | SHA256 |
| --- | ---: | --- |
| core executable | 21,049,384 | `32b7f921d8a0b4409581c66d8d2ff0756ba250d783f3e52e97392a062e7eeab1` |
| local executable | 271,401,656 | `7a99038be57429e1db32c91d01772e7efd104691828253f45ed3bbb0e9330417` |
| core local-review tar.gz | 8,799,844 | `ba2fc08989fffc4524dbb548e61b33d97b885be4ebb5e144a4ca0a77dc3bf631` |
| local local-review tar.gz | 387,261,136 | `88886b38465d23d99f119265eca865b9ac1551de9557a7e0ecfd76238a8f9c2d` |

Private installation consumed 779,808,479 file bytes, including the copied model.
Both Mach-O arm64 binaries passed local codesign integrity verification; signatures
are ad hoc with no TeamIdentifier. Core links only libiconv/libSystem. Local also
links system Security, CoreFoundation, Foundation, CoreML, libc++ and libobjc.
Neither has a build-cache dylib dependency. The SQLite symbol was observed in both;
the ORT API entry symbol was observed only in local. These are concrete observations,
not complete static-object attribution or notarization evidence.

Eleven final CLI invocations passed their expected outcomes: per variant version,
passive doctor, fixture import, catalog doctor and missing-asset denial; additionally
local configured-model doctor. Passive catalog observations preserved active/floor
hashes and the orphan staging sentinel. Missing assets returned failed diagnostics
without creating the missing directory. Fixture catalog freshness stayed stale;
doctor overall warning is expected with optional unconfigured facilities. The actual
local model observation took 693ms in this one smoke, not a startup benchmark.

All installation CLI invocations ran with an empty environment under OS network
denial, with IPv4/IPv6 loopback positive and denial controls. No Docker or project
execution occurred in this exercise. Each archive was reread without extraction and
every member's size and hash checked against its embedded manifest. Installation used
the hashed unpacked candidate directory; no archive-installer qualification is claimed.

Evidence under `target/m1-15-candidate`: `build-receipt.json`, `source-files.json`,
`manifest.json`, `installation-receipt.json`, `installation-manifest.json`,
`archive-receipt.json`, `native-build-inputs.json`, `native-license-evidence.json`,
`build-warning-summary.json`, per-command JSON/logs and otool/codesign observations.
The native license check rehashed28 local package texts and two ORT upstream texts.
The release-cache observation captured8 native build outputs and8 archive identities;
it explicitly does not claim a complete final linked-object map.

Remaining technical warnings: failed rust-objcopy stripping due to its missing LLVM
dylib, and local linker compact-unwind limits for a greater-than16MiB `__eh_frame`.
The linker warns of possible exception-handling performance impact. Neither warning
was hidden or repaired by installation. Product/native/model licensing, production
trust and complete client qualification were separate approvals/gates at the time.
ADR-048 later excluded local/model/catalog assets and additional native artifacts
from 0.1.0 without retroactively changing these observations.

After the principal refreshed release metadata, the candidate inventory was recopied
and verified at SHA256 `cbbca0d52613c341f55580088766c76f2c2be0b64df8a5ef672f1be0dd35948a`.
The unchanged notices SHA256 is
`2656a07517fbfcc6b278db7a3c850536944397d1d59d85e93ed9739e8660d5a5`.
Manifest and archives were rebuilt and every archived payload reverified; the table
above contains those final post-refresh hashes. Binaries were not rebuilt again.
`accepted-source-receipt.json` compares all238 selected input files against both
the working tree and Git main commit `01a90ab6dea32d94fec271139bff51847cdfb261`;
both comparisons matched exactly. Earlier archive receipts remain explicitly
superseded in `history/pre-inventory-refresh`.

M1-17 subsequently rechecked the same 238-entry source manifest against candidate
commit `d024c7c72648206266f0d195ffc7040fb444eef6` and the working tree. Zero selected
inputs changed or mismatched, so the local binary is source-input-equivalent to
that candidate. See [m1-17-source-equivalence.json](candidate/m1-17-source-equivalence.json).
This does not claim a rebuild or reproducible-build equality.

## Principal active verification of both installed release binaries

Both copied core and local executables completed actual `doctor --active`: fixed
calibration followed by rustc/Cargo/component inventory, exact approved image and
three distinct execution fingerprints. The local invocation also observed its
copied E5 model. Both returned exit0 with empty stderr and no owned Docker
containers or volumes remaining. See [active receipt](candidate/active-release-receipt.json).
The host profile denied IP outbound traffic (IPv4/IPv6 positive connect controls
outside and PermissionError inside), while allowing the Unix Docker socket.
Project execution additionally used the existing calibrated container network denial.
This profile is not a claim of denying every host network operation.

Checked-in [receipts](candidate/) preserve bounded machine evidence; large binaries,
archives and command logs remain in the ignored local target directory.
[Reproduction](reproduction/README.md) preserves the exact scripts and explains
their controlled input/ownership scope. No generic installer for hostile archives
is introduced; the product catalog importer remains the authenticated boundary.

Read `build-receipt.json` together with `accepted-source-receipt.json`: the former
preserves the actual pre-commit HEAD and dirty status; the latter establishes
identity with merged main01a90ab6 for all238 compiled inputs. Historical build
metadata is not rewritten to pretend the build ran after that merge.

[Independent Sonnet5 review](../validation/M1-15-review-sonnet.md) found no
High/Critical issue; [principal disposition](../validation/M1-15-review-disposition.md)
records the two Low observations and trust-boundary qualifications.

Integrated commit20e7e70 / merge93023be. Post-merge smoke rehashed all238 source
inputs and both installed binary identities; both version JSON observations passed.
[Receipt](candidate/postmerge-receipt.json). No compilation or product source changed
in this documentation/artifact-evidence unit.
