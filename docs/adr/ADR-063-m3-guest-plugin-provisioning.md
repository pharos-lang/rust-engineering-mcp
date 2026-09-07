# ADR-063 — M3 guest plugin provisioning

## Status

Accepted 2026-09-06 by the M3 orchestrator after independent reviews V06/V17/V18
(owner provisioning authorization 2026-09-05). Provisioning is verified and the
M3-01 nextest gateway qualification now passes; see
`docs/validation/M3-matrix.md`.

## Context

M3 quality tools are intentionally absent from the approved M1 guest. The owner
authorized, on 2026-09-05, exactly five additions for a later explicit host-only
provisioning run: `cargo-nextest` 0.9.143, `cargo-llvm-cov` 0.9.0,
`llvm-tools-preview` 1.98.1, `cargo-semver-checks` 0.50.0 and `cargo-mutants`
27.1.0. Rust 1.98.1, the pinned Debian ARM64 base, and the M1 receipt remain the
baseline. The MCP runtime must never download, install, or update a plugin.

The local upstream-release snapshot confirms these GNU ARM64 release assets and
facts:

| Component | URL asset/source | SHA-256 | Size | SPDX license |
| --- | --- | --- | ---: | --- |
| cargo-nextest 0.9.143 | `cargo-nextest-0.9.143-aarch64-unknown-linux-gnu.tar.gz` | `2a64b3566a92508550a7ab29c3e8db25472ca37730ecb4d22100b6aa440c2a68` | 11243965 | Apache-2.0 OR MIT |
| cargo-llvm-cov 0.9.0 | `cargo-llvm-cov-aarch64-unknown-linux-gnu.tar.gz` | `9af53b273e50d01d8bde8785de8541f6738cc4375248cd7683aec8b5768b9d21` | 1613151 | Apache-2.0 OR MIT |
| llvm-tools 1.98.1 | `llvm-tools-1.98.1-aarch64-unknown-linux-gnu.tar.xz` | `caaf950c65f3e428247dbe9c173d142b7072b2134962a61924c01e39f6b6dc1e` | 32493908 | Apache-2.0 WITH LLVM-exception |
| cargo-semver-checks 0.50.0 | `cargo-semver-checks-aarch64-unknown-linux-gnu.tar.gz` | `e35f435ea322659381f52e7034bb4f0470108f5b267d29f13cf08152fa4af29b` | 7866573 | Apache-2.0 OR MIT |
| cargo-mutants 27.1.0 | crates.io source build | recorded at provisioning | source build | MIT |

The exact GitHub URLs, release pages, sizes and API digests were checked against
the fetched `releases-summary.json`, rather than inferred from a filename. The
upstream Cargo manifests provide the license fields. The official LLVM `.sha256`
sidecar is recorded in the local source snapshot; license/notice members must be
verified inside that archive during the provisioning run.

## Decision

Extend `fixtures/rust-runtime` with a manifest-gated M3 profile. The existing M1
profile remains selectable without plugins and retains its tag, Rust inputs and
verification contract. `--plugins` selects a distinct M3 output directory and
`rust-engineering-runtime:1.98.1-arm64-m3` tag.

Use the GNU `aarch64-unknown-linux-gnu` artifacts on the Debian glibc base. The
three Cargo release archives contain the expected executable members
`cargo-nextest`, `cargo-llvm-cov` and `cargo-semver-checks`; the build must fail if
those members are absent. Install them at these fixed paths:

```text
/opt/rust/bin/cargo-nextest
/opt/rust/bin/cargo-llvm-cov
/opt/rust/bin/cargo-semver-checks
/opt/rust/bin/cargo-mutants
```

Extract the official LLVM archive only after checking these members:

```text
llvm-tools-1.98.1-aarch64-unknown-linux-gnu/install.sh
llvm-tools-1.98.1-aarch64-unknown-linux-gnu/llvm-tools-preview/lib/rustlib/aarch64-unknown-linux-gnu/bin/llvm-cov
llvm-tools-1.98.1-aarch64-unknown-linux-gnu/llvm-tools-preview/lib/rustlib/aarch64-unknown-linux-gnu/bin/llvm-profdata
```

Run its installer with `--prefix=/opt/rust --disable-ldconfig`. The resulting
`llvm-tools-preview` component and binaries live in the Rust sysroot, so
`cargo-llvm-cov` can use `rustc --print sysroot`. The fixed gateway environment is:

```text
PATH=/opt/rust/bin:/usr/bin:/bin
CARGO_HOME=/opt/rust
LLVM_COV=/opt/rust/lib/rustlib/aarch64-unknown-linux-gnu/bin/llvm-cov
LLVM_PROFDATA=/opt/rust/lib/rustlib/aarch64-unknown-linux-gnu/bin/llvm-profdata
```

The image's default PATH remains the M1 value for configuration compatibility;
the gateway reconstructs the plugin environment explicitly. Cargo's external
subcommand lookup therefore sees `/opt/rust/bin` both as `$CARGO_HOME/bin` and on
PATH, without depending on a peer's PATH. The verifier additionally runs the
`cargo llvm-cov --version` probe with PATH reduced to `/usr/bin:/bin` to exercise
the `$CARGO_HOME/bin` lookup independently.

Build cargo-mutants only in the Docker `builder` stage with:

```text
cargo install cargo-mutants --version 27.1.0 --locked --root /opt/plugins
```

The builder records the crate version, the crates.io registry-index checksum when
Cargo's downloaded index exposes it, the downloaded crate SHA-256 when available,
the resulting binary SHA-256, and the final Docker build-log SHA-256. The final
scratch image copies only the cleaned runtime filesystem; registry caches,
Cargo/build directories, package-manager state, installer scripts, and network
tools are not retained in the M3 filesystem. A package inventory and SPDX/notice
source record are retained under `/usr/share/doc/rust-runtime`; the verifier
captures their contents and hashes together with any upstream LICENSE/NOTICE files
found in the archives.

`provision.py` verifies SHA-256 and byte size before Docker, writes an exact
`SHA256SUMS`, rejects every unexpected or linked build-context entry, and records
the source-build and build-log evidence after the build. `verify.py` runs only
bounded, read-only, `network=none` containers as a runtime observation. It probes
all five components, hashes all plugin/LLVM executables, performs an `ldd`-style
missing-library check when `/usr/bin/ldd` exists, requires the exact plugin set,
and records package/license/notice evidence. These checks are not the M3 gateway
calibration or sandbox certification.

A successful provisioning run creates a new immutable image identity. Before that
image can be used, ADR-031's approved image identity must be amended after actual
calibration, the gateway `APPROVED_RUST_IMAGE` constant and configuration
fingerprint must change together, and all affected Docker/native/security gates
must be rerun. Existing M1 image IDs and receipts, including M2 image-config
evidence, remain historical and are preserved.

## Alternatives considered

- **musl artifacts:** rejected for this guest because the approved Debian base is
  glibc-based and the GNU artifacts match its runtime ABI.
- **Host binaries or host PATH:** rejected; it would cross the guest boundary and
  make execution identity non-reproducible.
- **Runtime installation:** rejected; the MCP runtime is deny-by-default and must
  not acquire bytes, access crates.io, or mutate its immutable filesystem.
- **Defer cargo-mutants:** rejected by the owner; no ARM64 release asset exists,
  so a locked source build in a disposable builder is the bounded option.
- **Reuse the M1 image/tag:** rejected; plugin bytes change the image identity and
  require independent calibration and gateway approval.

## Consequences

The M3 image is larger and has a new identity, digest, configuration fingerprint,
SBOM/provenance and license/notice receipt. Apt's mutable package resolution still
means a later explicit rebuild must retain the resulting image ID and all receipts;
this is not a reproducible-build claim. The source-built cargo-mutants binary has
no release-asset digest; its build receipt and binary digest become the evidence.

The image cannot be considered approved merely because Docker build or version
probes succeed. Gateway admission, calibration, source-transfer compatibility and
M3 integration gates remain separate work. No M2 receipt is rewritten or upgraded
in place.

## Provisioning result and M3-01 qualification

P02 produced and verified the immutable image
`sha256:384a1742ecc53cdd3a9c0bf36c6f8b66db73ddd118aeeae6e55654ea998ae36a`.
The provisioning verifier passed 47/47 observations; the source-bound evidence is
`docs/validation/M3-provisioning.json` and its selected identity is recorded in
`docs/validation/M3-image-config.json`. The installed LLVM executables are exactly
under `/opt/rust/lib/rustlib/aarch64-unknown-linux-gnu/bin/`; there is no
`llvm-tools-preview/` path segment in the archive members; installation places
the executables under `/opt/rust/lib/rustlib/aarch64-unknown-linux-gnu/bin/`.

M3-01 switched the gateway to that identity. ADR-064 records the separately
authorized nextest-only seccomp delta for Tokio's anonymous AF_UNIX stream pair;
the base M1 and M2 profiles remain byte-identical. The final exact gate passed
positive nextest and the negative network/pathname-socket controls, and the shared
gateway change was recalibrated with the existing twenty-case Rust security gate.
The current receipts are `docs/validation/M3-01-rust-security.json` and
`docs/validation/M3-01-runtime.json`; the initial denied-syscall receipt remains
`docs/validation/M3-01-runtime-attempt1.json`.

## Built image

P02 built the immutable Linux ARM64 image
`sha256:384a1742ecc53cdd3a9c0bf36c6f8b66db73ddd118aeeae6e55654ea998ae36a` with
configuration digest
`sha256:7d4e58b9e29b2045c13d71542f7892ee071a6886a1b939c4cbfc3ff7ce40dc45`.
Provisioning verification passed 47/47 observations. The source-bound receipts
are [M3-provisioning.json](../validation/M3-provisioning.json),
[M3-image-config.json](../validation/M3-image-config.json),
[P02 delivery](../validation/m3-delegation/P02-provision-run/last-message.md),
and the [M3-01 security receipt](../validation/M3-01-rust-security.json).

## Sources

- `fixtures/rust-runtime/sources.json`
- `fixtures/rust-runtime/provision.py`
- `fixtures/rust-runtime/Dockerfile`
- `fixtures/rust-runtime/verify.py`
- `docs/roadmap/m3-quality.md` (explicit provisioning and G7)
- `docs/roadmap/m2-m8.md` (G7)
- `docs/adr/ADR-031-rust-source-transfer.md`
- `docs/adr/ADR-033-toolchain-inspection.md`
- `docs/validation/M2-image-config.json`
- `docs/validation/artifacts/M1-01-runtime-base-inspect.json`
- `docs/validation/artifacts/M1-01-runtime-image-inspect.json`
- `docs/validation/artifacts/M1-01-runtime-verification.json`
- `docs/validation/m3-delegation/R01-plugins/report.md`
- Local fetched source snapshot: `releases-summary.json`, the four upstream
  `*-Cargo.toml.txt` files, `rust-llvm-tools-sha.txt` and `index.txt`, fetched
  2026-09-05 UTC.
