# M1-17 final local full gate

Status: **passed on native macOS ARM64**. This is candidate-level local evidence,
not cross-platform qualification or release approval.

The 19-stage `python3 scripts/gate.py full` run completed against source commit
`d024c7c72648206266f0d195ffc7040fb444eef6`. The checkout had only the M1-17
Inspector/matrix evidence paths dirty; production source, manifests, tests, CI and
gate scripts matched that commit. Rust and Cargo were 1.98.1.
The commit was checked out on `ai/m1-17-release-qualification` and was also the
`main` tip at gate time; the receipt's original branch label has been corrected.

Every recorded stage passed: fmt, workspace check, strict Clippy, workspace tests,
doctests, architecture, vendor verification, Cargo fixtures, audit, deny, Docker
security, Rust security, audit data, native semantic, catalog import/recovery,
catalog status, crate search, crate inspect and active doctor. The stage durations
sum to 757.737 seconds; `rust-security` accounted for 535.220 seconds. The wrapper
did not record an exact start timestamp, so none is reconstructed.

The semantic and catalog stages used the approved E5/ORT assets and enforced macOS
network-deny path. Post-run checks found no containers or volumes under either
known product label. The [machine report](m1-17-final-gate/full-report.json),
[complete log](m1-17-final-gate/full.log) and [receipt](m1-17-final-gate/receipt.json)
retain commands, source/dirty declaration, platform, versions, status and hashes.
The [derived count receipt](m1-17-final-gate/counts-derived-from-log.json) reports
644 passed/31 ignored in the workspace-test stage and one passed doctest in its
separate stage. It does not relabel their sum as one core-test count.

The original audit stage used `--no-fetch` but did not capture the advisory DB
identity. A focused repetition now binds `cargo audit` and `cargo deny` success to
the exact Cargo.lock, RustSec commit and three pre-existing untracked placeholder
advisories in [audit-focused.json](m1-17-final-gate/audit-focused.json). The base
is not clean and no remote-freshness claim follows. The gate wrapper did not
record an exact start time; the completion timestamp remains explicitly derived
from the original report mtime. Future runs need wrapper-native start/end fields.

A broader seven-pattern [credential scan](m1-17-final-gate/secret-scan-followup.json)
found no matches across the local M1-17 evidence. Absolute local paths are retained
for reproducibility and require sanitization before any publication.

This run does not supply the absent native Linux, Windows or x86 evidence. It also
does not decide product licensing, copyright holder, publisher identity, release
channel or signing-key custody.
