Task: I17b — ADR-061 completion.

Result: not complete. I added the APFS free-space admission floor and corrected job-level reservation validation to 64 MiB. The existing skeleton remains unqualified and must not be wired into Stage 1.

Files changed (SHA-256):
- `crates/domain/src/quality_artifact.rs`  
  `16ed9585fc81266db780977f51ddb3f2861c59ba18775657f7d2bbb295cf6792`
- `crates/domain/tests/quality_artifact.rs`  
  `90798c8a9225979fcff2c04dba2eff468d064a22a1cbba4f7c0ca0f9812c3066`
- `crates/application/src/quality_artifact.rs`  
  `c314a4d1509ee3b65791564a2fda386fea2c81acf9785e20a8048cf870019b33`
- `crates/application/tests/quality_artifact.rs`  
  `9cdf7455f8cc57f1bc6d5fd2ce0c100ec325d16c3fa68f8316a52b5b62dca82b`
- `crates/project-adapter/src/filesystem.rs`  
  `df469928ae140e72b7637f3a77aeeb1e823c1e1a66c6f2d38d2367cddace3562`
- `crates/project-adapter/src/lib.rs`  
  `4641e9551ad63ccde6ac3f638ad55e7193342ac5cc57b12075bf08f2e7fa97c7`
- `crates/project-adapter/src/quality_artifact_store.rs`  
  `5ce60dc25e16edadcce66dd1f281c55b2c39d89d2eaed4345589a0051575ddd7`
- `crates/project-adapter/src/filesystem/macos.rs`  
  `203b16850344b2c2782273f1c8fafead73a7f9457923f9e7f62a748a15f22ca8`
- `crates/project-adapter/src/filesystem/macos/quality.rs`  
  `d1291d0f19cb073b738b569beea77a1a4a2adb703e46014df9573eccb722a9eb`
- `crates/project-adapter/src/filesystem/macos/state_primitives.rs`  
  `83a04f2185eea05cb48d8a2510add717fc5c480ccfdac6986be51fe6960bf42c`
- `crates/project-adapter/tests/quality_artifact_store.rs`  
  `ef3c7db28074734f3540b40057aa8d50cd66c004c448ba9bc48f7a7a72005a8e`
- `crates/mcp-server/src/stdio/resources.rs`  
  `ed4d6ccf7e301b80f001550a02029a8ee58018faead1dfe82392a9fa2f2382e2`
- `crates/mcp-server/src/stdio/resources/tests.rs`  
  `6ac9790be28be8ddb95ee357ad05d1168acd27d72b992c41b432f2c966699e0f`

Tests executed:
- `cargo check -p rust-engineering-project --all-targets --locked --offline` — passed.
- `cargo test -p rust-engineering-project --test quality_artifact_store --locked --offline` — passed: 0 passed, 3 ignored.
- `python3 -B scripts/check-architecture.py` — passed.
- `cargo clippy -p rust-engineering-project --all-targets --locked --offline -- -D warnings` — blocked by external `application/src/job.rs` lint.
- `cargo check -p rust-engineering-mcp --all-targets --locked --offline` — blocked by external missing `rust_engineering_domain::nextest` export.

Native ignored test names:
- `native_apfs_quality_two_process_store_lock_is_nonblocking`
- `native_apfs_quality_reservation_preserves_m2_headroom`
- `native_apfs_quality_crash_between_blob_and_descriptor_is_not_served`

Evidence:
- The free-space check uses `fstatfs` under `store.lock` and requires reservation + 49 MiB M2 recovery headroom + 16 MiB quality-control headroom.
- `git diff --check` passed.

Risks / open issues:
- Required ADR-061 work remains: clock watermark, validated reconciliation/quarantine, expiry/prune, descriptor/blob hash revalidation on reads, real pagination, crash/ENOSPC fault port, ArchiveBundle wrapper/revalidation, owner-binding derivation/live-registry bridge, recovery/prune reports, and required native oracle coverage.
- The three ignored tests are placeholders, not qualifying tests; I did not run them because they do not yet exercise the native adapter.
- Shared blockers observed during polling:
  - `crates/application/src/job.rs` changed from a missing `Ok(())` to a `collapsible_if` Clippy violation.
  - `crates/execution-adapter/src/nextest_junit.rs` imports `rust_engineering_domain::nextest`, which is not exported.
- The earlier M2 source-test `Operation not permitted` was not rerun in this turn; no conclusion about sandbox versus fixture location is justified.
