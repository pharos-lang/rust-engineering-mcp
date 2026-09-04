# Audit-only RSA input: never compile

This fixture pins the registry package `rsa = 0.9.6` and the real advisory
RUSTSEC-2023-0071 (Marvin timing side channel). Its `Cargo.lock` is deliberately a
**minimal audit input** containing the root package and vulnerable package identity;
it omits RSA's transitive graph. It is not a resolved build lockfile. Never run Cargo
on this directory. No vulnerable package source is vendored or executed.

The advisory is an exact copy from the official RustSec advisory-db at commit
`d674d8e9e6f78117229abdb7501452ac6c3cf322`:
https://raw.githubusercontent.com/RustSec/advisory-db/d674d8e9e6f78117229abdb7501452ac6c3cf322/crates/rsa/RUSTSEC-2023-0071.md

The registry checksum was read from https://index.crates.io/3/r/rsa for 0.9.6.
`provenance.json` records those sources and the advisory SHA-256. The pinned advisory
has no patched version range. Expected future audit result: RUSTSEC-2023-0071 for
rsa 0.9.6 with this provenance, without claiming a live advisory database.

The Python harness verifies the lock identity, checksum and pinned advisory content
only. That is not evidence that a RustSec matcher or cargo-audit has been integrated.
A future audit adapter must prove its actual match using this input without compiling
or resolving it. RustSec advisory data is distributed under CC0-1.0 by its upstream
repository: https://github.com/RustSec/advisory-db/blob/main/LICENSE.txt
