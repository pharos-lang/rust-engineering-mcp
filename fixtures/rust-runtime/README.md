# Explicit Rust Linux ARM64 runtime provisioning

This host-only fixture installs the exact official Rust 1.98.1 distribution,
including Cargo, standard library, rustfmt and Clippy, at `/opt/rust` without
rustup. It is not called by the MCP server. User authorization is required before
running provisioning because it downloads an OS image, Debian packages and Rust.

The official `rust:1.98.1-bookworm`, `rust:1.98.1-trixie` and `rust:1.98.1` tags
were unavailable when checked on 2026-09-04 UTC. The owner approved building from
the official Rust distribution and pinned official Debian bookworm ARM64 base.
Sources and exact archive hashes are in `sources.json`. Component manifest
versions are copied verbatim and can differ from the Rust release label; actual
Rust/Cargo executable versions are checked independently. The original manifest
was checked against its official `.sha256` sidecar. TLS plus checksums verifies
download integrity; this is not a detached signature or reproducible-build claim.

```text
python3 fixtures/rust-runtime/provision.py --docker /absolute/path/to/docker --host unix:///absolute/path/to/docker.sock --output target/m1-runtime-provisioning
python3 fixtures/rust-runtime/verify.py --docker /absolute/path/to/docker --host unix:///absolute/path/to/docker.sock --output target/m1-runtime-provisioning
```

## M3 plugin profile

On 2026-09-05 the owner explicitly authorized exactly five additions for M3:
`cargo-nextest` 0.9.143, `cargo-llvm-cov` 0.9.0, official `llvm-tools-preview`
1.98.1, `cargo-semver-checks` 0.50.0 and `cargo-mutants` 27.1.0. The M3 profile
uses the GNU `aarch64-unknown-linux-gnu` artifacts because the pinned Debian base
is glibc-based. The first four inputs are the exact release/Rust archives recorded
in `sources.json`; cargo-mutants has no ARM64 release asset and is built from
source with the locked Cargo command in the builder stage. The recorded SPDX
licenses and upstream notice sources are part of the manifest.

Provisioning is a separately launched, host-only operation and is not available to
the MCP runtime. It is the only operation allowed to download these inputs or to
allow Cargo registry network access:

```text
python3 fixtures/rust-runtime/provision.py --docker /absolute/path/to/docker --host unix:///absolute/path/to/docker.sock --output target/m3-runtime-provisioning --plugins
python3 fixtures/rust-runtime/verify.py --docker /absolute/path/to/docker --host unix:///absolute/path/to/docker.sock --output target/m3-runtime-provisioning --plugins
```

`--plugins` selects the separate `rust-engineering-runtime:1.98.1-arm64-m3` tag
and output directory. Omitting it keeps the M1 tag and flow rebuildable without
plugin inputs. The M3 build context is refused if it contains any file other than
the manifest-selected archives, `SHA256SUMS` and the Dockerfile; every archive is
checked for its manifest SHA-256 and byte size before Docker is invoked.

The gateway must reconstruct this environment for plugin execution rather than
relying on the image's default PATH:

```text
PATH=/opt/rust/bin:/usr/bin:/bin
CARGO_HOME=/opt/rust
LLVM_COV=/opt/rust/lib/rustlib/aarch64-unknown-linux-gnu/bin/llvm-cov
LLVM_PROFDATA=/opt/rust/lib/rustlib/aarch64-unknown-linux-gnu/bin/llvm-profdata
```

Cargo subcommands are therefore found in `/opt/rust/bin` through the explicit
`CARGO_HOME/bin`/PATH search; the verifier also exercises the Cargo-home lookup
with PATH reduced to `/usr/bin:/bin`. `llvm-tools-preview` is installed by its official
installer into the Rust sysroot; the two LLVM variables are the fixed fallback.
The verifier records the new image ID, plugin and LLVM executable hashes, shared
library checks, captured package inventory, and license/notice file hashes. A
successful M3 build necessarily produces a new immutable image identity. That
identity, its configuration fingerprint and the calibrated gateway profile must
be added to ADR-031 before the gateway can approve it; the existing M1 identity
and receipts remain unchanged.

Run from the repository root using Python 3.11 or later. The build context contains
only downloaded, hash-verified official archives, checksums and this Dockerfile.
No project source enters it. Provisioning uses the network explicitly. Debian's
signed repositories supply GCC, libc development files, certificates, OpenSSL and
XZ support; transitive package versions are captured by verification. Apt indexes
remain mutable, so rebuilding may change the resulting image ID. Use the recorded
immutable local image ID for execution, never the mutable convenience tag.

The final scratch stage copies the prepared filesystem and starts a fresh image
configuration. Its only environment entry is Docker's standard PATH; its user is
65534:65534 and cwd is `/work`. It declares no command, entrypoint, healthcheck,
volume or on-build trigger. Empty `/source` and `/work` directories exist. Tools
are at `/opt/rust/bin/{rustc,cargo,rustfmt,clippy-driver,cargo-clippy}`; callers must
select fixed paths and rebuild any required PATH explicitly. No rustup proxy or
auto-download is installed.

Verification uses the image ID in non-root, read-only containers without project
mounts, with `network=none`, no capabilities, no-new-privileges, bounded memory,
CPU and PID count. It runs version/hash/package-list commands only. The fixture
never executes hostile project code. A timeout force-removes its named container.
`network=none` is not socket-denial proof; these checks do not certify the gateway
sandbox or authorize any M1 tool. Seccomp calibration and source transfer remain
separate owner-controlled work.

On this host, the verified local image ID after script replay was
`sha256:384a1742ecc53cdd3a9c0bf36c6f8b66db73ddd118aeeae6e55654ea998ae36a`.
Docker reported a matching local RepoDigest under `rust-engineering-runtime`;
this image was not pushed and that RepoDigest is not an official registry artifact.
Rust and Cargo both reported 1.98.1, rustfmt 1.9.0-stable, Clippy 0.1.98,
GCC 12.2.0, and Rust host `aarch64-unknown-linux-gnu`.

Artifacts in the selected output directory include `image-id`, `build.log`,
`base-inspect.json`, `image-inspect.json`, `provisioning-receipt.json`, and
`verification.json`. The latter
records actual tool versions, binary hashes, installed package versions and
verification commands. Keep these artifacts with the image as provisioning evidence.

Official provenance:

- https://hub.docker.com/_/debian
- https://hub.docker.com/_/rust
- https://github.com/docker-library/official-images/blob/master/library/rust
- https://github.com/rust-lang/docker-rust
- https://static.rust-lang.org/dist/channel-rust-1.98.1.toml
- https://static.rust-lang.org/dist/channel-rust-1.98.1.toml.sha256
- https://github.com/nextest-rs/nextest/releases/tag/cargo-nextest-0.9.143
- https://github.com/taiki-e/cargo-llvm-cov/releases/tag/v0.9.0
- https://github.com/obi1kenobi/cargo-semver-checks/releases/tag/v0.50.0
- https://github.com/sourcefrog/cargo-mutants/releases/tag/v27.1.0
- https://static.rust-lang.org/dist/llvm-tools-1.98.1-aarch64-unknown-linux-gnu.tar.xz.sha256
