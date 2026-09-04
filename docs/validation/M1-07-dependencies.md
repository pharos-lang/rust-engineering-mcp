# M1-07 explicit development dependency provisioning

2026-09-04. Owner announced crates.io development acquisition before it ran; no
MCP runtime downloads, new image or toolchain. rustsec0.32.0/default-features=false
and cargo-lock11.0.1/default-features=false are exact workspace/adapter pins.
Initial resolution selected cargo-lock11.1.0 and downloaded its two TOML additions;
then the audited11.0.1 API was explicitly pinned, removing those from the lock.
Final change adds only cargo-lock11.0.1, cvss2.2.0, fs-err3.3.1, platforms3.12.0,
rustsec0.32.0. No existing package version was changed or removed.

Commands (CARGO_INCREMENTAL=0, Rust/Cargo1.98.1): cargo fetch --target
 aarch64-apple-darwin; cargo update -p cargo-lock --precise11.0.1; cargo fetch
 --locked --target aarch64-apple-darwin. Subsequent builds/gates locked/offline.
Actual resolve node for RustSec has no enabled features; no Git/HTTP/binary scanning
or dependency-tree. Reuses existing pinned TOML. Checksums and registry identities:
[M1-07-dependency-receipt.json](M1-07-dependency-receipt.json).
Declared dependency licenses do not qualify product distribution; existing M0
licensing and paste1.0.15 maintenance limitations remain. Raw provisioning logs in
 target/M1-07-provision.log, M1-07-lock-pin.log, M1-07-provision-pinned.log.

Resolved feature changes also enable serde support on existing semver/url packages;
unchanged package versions do not imply an unchanged feature graph. The checked
Cargo metadata receipt records the actual RustSec feature set (empty). The RSA
fixture is copied from the installed rustsec0.32.0 test corpus and retains the
RUSTSEC-2023-0071 text; it is test evidence, not a distributable authenticated catalog.
Repository and data-distribution license qualification remains pending before release.
