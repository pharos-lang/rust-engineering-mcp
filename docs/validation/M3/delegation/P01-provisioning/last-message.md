Task: P01 — Guest plugin provisioning for M3.

Result: Implemented the manifest-gated M3 fixture and Proposed ADR-063. ADR-063 was free. No provisioning, Docker, `cargo install`, or downloads were executed.

Files changed:

- [Dockerfile](/Users/cburgosro/Projects/rust-mcp/fixtures/rust-runtime/Dockerfile) — `33bfa34d7a915d9bc0f632b0a1f0732fc3b92a64ae27f1a6f9f9415f35a3ec5b`
- [README.md](/Users/cburgosro/Projects/rust-mcp/fixtures/rust-runtime/README.md) — `2bc207e4e1737c9b1a8c799aa341b64d938acd70134be4ffb636a7ccc224d3e3`
- [provision.py](/Users/cburgosro/Projects/rust-mcp/fixtures/rust-runtime/provision.py) — `fd583100ca5531fadd25fff9951eafe18efa342742745413cb952fea6d2a0f62`
- [sources.json](/Users/cburgosro/Projects/rust-mcp/fixtures/rust-runtime/sources.json) — `42adea1d2297d37b7d47863fc3b647aa3d3ceff346a1fe7c0b9f538cd44a196b`
- [verify.py](/Users/cburgosro/Projects/rust-mcp/fixtures/rust-runtime/verify.py) — `166a9d8dfcf49fde3fb0620684e6c73029b4550d8d779b88df3a158c4b53a857`
- [ADR-063](/Users/cburgosro/Projects/rust-mcp/docs/adr/ADR-063-m3-guest-plugin-provisioning.md) — `35601a650b7974978383a3cff8b1611625d5465ed3c352b79dd62f403e2f973a`

Tests executed:

- JSON validation: passed.
- `python3 -m py_compile` for both scripts: passed.
- Local release URL/hash/size cross-check against fetched `releases-summary.json`: passed.
- `git diff --check`: passed.
- `scripts/test-fixtures.py` was not run because it does not cover `fixtures/rust-runtime`.

Evidence: exact versions, URLs, hashes, sizes, licenses, paths, gateway environment, source-build receipt fields, notices, SBOM metadata, and M1 receipts are documented.

Risks / open issues:

- Archive member layouts and embedded notices remain unverified offline; the Docker provisioning run now checks them and fails closed.
- The cargo-mutants registry-index checksum may be unavailable; the receipt records that explicitly.
- The new image ID and gateway approval require a later build, calibration, `APPROVED_RUST_IMAGE` update, fingerprint change, and Docker/security gate reruns.

No commits, merges, pushes, or unrelated files were changed.