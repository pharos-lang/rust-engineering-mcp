# M4 runtime provisioning result

Status: **provisioned; original setup probe superseded by the passing prepared-sysroot oracle; not approved for gateway admission**.

The Technical Owner updated ADR-066 after verifying the pinned source. The
[prepared Miri oracle](M4-prepared-miri.json) passes a first-party `cfg(miri)` test
with source/rootfs read-only, network none and unchanged sysroot tree hashes. The
original setup failure below remains historical evidence; it does not require
more installation or a different nightly. D21 gateway qualification is pending.

The exact accepted manifest was acquired on the host: 247 inputs and 159,061,363
bytes, including 238 unique registry archives. Every size and SHA-256 matched. The
archive validator rejected links, special members, traversal, duplicates and paths
that cannot be indexed safely. The `Cargo.lock` inside the official cargo-deny
0.19.7 crates.io package is byte-identical to the pinned official-tag lock.

Docker built with `--network none --pull=false` from the preflighted local M3 tag.
The final image is
`sha256:95dddeb5305f10b09b441e3cc4018ebb1a8a296d365c65106327d59f933c64e7`.
The M3 tag remains at
`sha256:384a1742ecc53cdd3a9c0bf36c6f8b66db73ddd118aeeae6e55654ea998ae36a`.
No project source or credential path entered the context.

Observed positive evidence includes cargo-deny 0.19.7 as ELF64 AArch64 GNU with
`/lib/ld-linux-aarch64.so.1` and no missing `ldd` library; exact nightly Rust and
Cargo commits; the fixed 114,355,641-byte Miri sysroot; executable hashes; an input
SBOM; preserved third-party license texts; and absence of package managers,
installers, build caches and common network tools. The native verifier used a
read-only container with network none, all capabilities dropped and no new
privileges.

The bounded cargo-deny runtime oracle generated a dependency-free crate only in
container tmpfs. With `--format json --log-level debug --offline --locked`, stdout
was empty and stderr contained 21 valid JSON events, including DEBUG events and a
final summary. Exit 4 correctly represented one deliberate missing-license error;
bans and sources reported zero errors.

The blocking observation is reproducible: `cargo miri setup --print-sysroot
--target aarch64-unknown-linux-gnu` does not only inspect the prepared sysroot in
this nightly. It attempts an atomic rebuild even when `MIRI_SYSROOT` names the
prepared path, then fails closed trying to create `.tmp*` on the read-only
filesystem before printing a path. Satisfying that command by granting writes
would violate the required read-only/no-write oracle. No alternate nightly,
version, wrapper or gateway change was substituted.

The exact pinned source explains the distinction the gateway must preserve.
`src/tools/miri/cargo-miri/src/setup.rs:21-27` sets `only_setup` for the setup
subcommand and returns the configured `MIRI_SYSROOT` early only when the command is
not setup. `src/tools/miri/cargo-miri/src/phases.rs:128-132` calls setup for each
target before forwarding run/test; therefore `MIRI_SYSROOT` makes run/test use the
prepared sysroot without implicit setup, while an explicit setup command retains
its provisioning behavior. The source is the Rust commit fixed by ADR-066.

Raw receipts remain under `target/m4-runtime-provisioning/`. The structured summary
is [M4-provisioning.json](M4-provisioning.json) and the image configuration is
[M4-image-config.json](M4-image-config.json). This result does not run hostile Miri
or project fixtures and does not qualify D21.
