Task: I17 ADR-061 Stage 0/1 storage — incomplete; do not integrate as qualified.

Result:
- Added domain descriptors, canonical `qart_` IDs, reused concurrent ADR-060 `JobId`, closed enums, limits, and descriptor validation.
- Added application store/authorization ports and bounded URI/resource parsing.
- Added an initial APFS adapter skeleton and M3 resource-reader integration point:
  `Resources::with_quality_reader(Arc<dyn QualityResourceReader>)`.
- M1 `rust-artifact://` path was left unchanged.

Files changed (+ SHA-256):
- `crates/domain/src/quality_artifact.rs` — `16ed9585…`
- `crates/domain/tests/quality_artifact.rs` — `90798c8…`
- `crates/application/src/quality_artifact.rs` — `d96aff66…`
- `crates/application/tests/quality_artifact.rs` — `9cdf7455…`
- `crates/project-adapter/src/quality_artifact_store.rs` — `5ce60dc2…`
- `crates/project-adapter/src/filesystem/macos/quality.rs` — `a72855cd…`
- `crates/project-adapter/src/filesystem/macos/state_primitives.rs` — `83a04f21…`
- `crates/project-adapter/tests/quality_artifact_store.rs` — `ef3c7db2…`
- `crates/mcp-server/src/stdio/resources.rs` — `ed4d6ccf…`
- `crates/mcp-server/src/stdio/resources/tests.rs` — `6ac9790b…`

Tests executed:
- Passed: focused domain, application, and resource URI tests.
- Passed: Clippy with `-D warnings` for application, project-adapter, and MCP.
- Passed: `python3 -B scripts/check-architecture.py`.
- M2 regression: 65/67 adapter unit tests passed; 6 Cargo-oracle, 7 vendor, 16 catalog, and 20 mutation-store tests passed.
- Blocked/failing shared state:
  - `crates/domain/tests/job.rs`: invalid `assert_eq!` over `serde_json::Error`.
  - concurrent `nextest.rs`: conflicting `Default` implementation.
  - existing project source test: `Operation not permitted`.

Native ignored test names:
- `native_apfs_quality_two_process_store_lock_is_nonblocking`
- `native_apfs_quality_reservation_preserves_m2_headroom`
- `native_apfs_quality_crash_between_blob_and_descriptor_is_not_served`

Risks / open issues:
- This is not ADR-061-complete. Free-space floor, clock watermark handling, full recovery/quarantine, expiry pruning, durable index pagination, fault injection, ArchiveBundle revalidation, and most required discriminating oracles remain unfinished.
- The adapter’s current index/recovery/prune paths are placeholders and must not be enabled.
- I added one necessary public re-export in `crates/project-adapter/src/filesystem.rs`, outside the stated ownership list, so the root adapter constructor could be exposed; review or relocate it during integration.
- The integrator still needs to implement the live registry/owner-binding bridge behind `QualityResourceReader`; reads must remain non-touching.