All checks are green. Final delivery report:

## Task
Apply review V17c's dispositions (F1–F9 + the missing-test list) to the ADR-061 durable quality artifact store, within the I17c ownership set.

## Result
All nine findings and every listed missing test are implemented and qualified on macOS ARM64/APFS. No external blockers this run: the clippy failures I17c reported in `crates/domain/src/nextest.rs` are gone, so the required clippy command passes cleanly.

## Files changed (SHA-256)
```
88f957213a0090732475d22bbcff64edb9cc338261f05ec2c3a4159c3875fd5b  crates/domain/src/quality_artifact.rs
c30f8e82c2017a869b4d143b8da39140a27dcbebbac4ea202e2b0cf25165d5db  crates/domain/tests/quality_artifact.rs
3a376ab3f380ae8d53805b6e557ce949eb18eb37aabcb75a42fa72cb5413eeec  crates/project-adapter/src/filesystem/macos/quality.rs
6017d360cb74f656f91d4372f0caa497a9cdecee0518553c32dac59fb7515e97  crates/project-adapter/src/filesystem/macos/state_primitives.rs
276c9ae85642b50defedb405583e95ca60c867f6403386a0bfddd8cd809d651c  crates/project-adapter/src/quality_artifact_store.rs
f6a4541216500f3d01d66bf3a521961b37e91ae67a180427d01568cc082fc3b3  crates/project-adapter/tests/quality_artifact_store.rs
e891ebe3c45cd990242659accb9b0109554d2062c0c5825a67c862011b8268fb  crates/execution-adapter/src/mutation_archive.rs
41197533c3e619bc92c9f784955028ff0275ab74134808b15fc2d7985f3d1088  crates/application/src/quality_artifact.rs   (UNCHANGED)
d159dba7a940e98f94efa66f0b5df0679c0dbc23e754edba9eebb315913ea21f  crates/application/tests/quality_artifact.rs (UNCHANGED)
86c2c8168745c66c5129e4eeb7910cb7a53809f024b4c4333b2228d0a936c0e5  crates/project-adapter/src/lib.rs            (UNCHANGED)
586f023020329a82316d87e40df4fdd05147320ea723e40d9971529969de38c7  crates/project-adapter/src/filesystem.rs     (UNCHANGED)
3cd7fe857699ab6d5216bb801a6c34d3e275c17f5a0a4c9437813f29c8f0ffbd  crates/project-adapter/src/filesystem/macos.rs (UNCHANGED)
```

## Finding → fix location → test
| ID | Fix | Location | Test |
|---|---|---|---|
| F1 | Parent probe is now exactly M2's (`uid` + `mode & 0o022 == 0`); store's own dirs stay `0700`; failure is the new `QualityArtifactError::UnsupportedStateRoot`; comment corrected | `state_primitives.rs:36-52` (`qualified_state_root`), `:130-146`; `domain/src/quality_artifact.rs:81-84,104` | `apfs::an_operator_state_root_is_qualified_exactly_as_m2_qualifies_it` (0755 opens + publishes + child dirs 0700; 0777/0770/0707 → `UnsupportedStateRoot` from `open`/`recover`/`prune_expired` with no directory created) |
| F2+F3 | One `reclaim` routine under `store.lock`, run in `reconcile`, at the start of `reserve` and by `prune_expired`: expired pairs, expired/absent records + `.part`/`.dtmp`/`.rtmp`, stale `.trunc`; accounting also refuses to charge expired evidence | `quality.rs:500-560` (`reclaim`), `:640-650` (reconcile), `:454` (accounting), `:900` (reserve), `:1180` (prune) | `apfs::expired_evidence_and_claims_stop_being_charged_and_leave_the_volume` (8 MiB `.part` reclaimed on next reserve; owner regains the full 128 MiB, which the 4 KiB expired descriptor alone would have denied; on-disk bytes ≤ global cap), `apfs::expiry_reclaims_only_known_bytes_and_reads_never_renew` (operator report unchanged: 1 removed / 9 bytes / 1 retained) |
| F4 | Duplicate `(job_id, member_index)` rejected at publish; cursor is the whole key `m<10 digits>_<artifact-id>` (49 bytes) and rows sort/advance on `(member_index, artifact_id)` | `quality.rs:94-120` (`encode_cursor`/`parse_cursor`), `:1010-1016` (publish), `:1120-1150` (paging) | `apfs::two_members_of_one_job_cannot_share_a_member_index`, `apfs::a_page_boundary_advances_past_stored_objects_sharing_an_index` (planted duplicate at the boundary: page 1 = 64 rows, page 2 = the planted row only, no repeat), `apfs::a_job_index_page_is_bounded_...` (grammar negatives incl. the old 11-byte cursor) |
| F5 | Tightened to quarantine: surplus is only truncated when the `<artifact>.trunc` marker this store wrote durably **before** the blob rename matches artifact, size and capacity; otherwise `SizeMismatch` | `quality.rs:140-150` (`TruncationMarker`), `:760-790` (verify), `:1020-1040` + `:1090-1100` (publish writes/consumes) | `apfs::a_blob_longer_than_its_descriptor_is_quarantined_without_a_marker` (appended bytes with matching prefix hash → quarantined), `apfs::crash_between_descriptor_and_directory_fsync_survives_when_complete` (marker present + blob long before restart, completed and marker consumed after) |
| F6 | Module header records the integrator's obligation; wrapper returns `ArchiveBundleStats { entries }` | `quality.rs:31-40` (header), `mutation_archive.rs:798-870` | `quality_archive_tests::a_bounded_regular_archive_canonicalizes_without_extraction` (entries = 5), `..._over_count_are_rejected` (entries = the 128 ceiling) |
| F7 | `release` reads the stored record and compares it to `ReservationRecord::of(caller)` before unlinking; absent record is a no-op | `quality.rs:940-955` | `apfs::release_only_honours_the_exact_claim_it_was_given` (three mismatched claims → `Unauthorized`, record and `.part` intact; double release is a no-op) |
| F8 | cfg gate is `all(target_os = "macos", target_arch = "aarch64")` | `quality_artifact_store.rs:1-16`, test module gates | `unsupported::unsupported_platform_rejects_before_any_effect` (cfg-gated; still not executable on this host) |
| F9 | `attach` is public: a read-only view, no lock, no reconciliation; `open` = attach + reconcile; publication paths unchanged | `quality.rs:250-262` | `apfs::a_reader_attaches_while_another_session_holds_the_store_lock` (attach + chunk + index page succeed under a held lock; `reserve`/`reconcile_recover`/`open` → `Busy`) |
| Missing test: symlink/non-regular | Descriptor identity probed before parsing; rejected blobs quarantined by listing so a symlink is moved out too | `quality.rs:730-742`, `:700-706` | `apfs::a_planted_symlink_or_non_regular_object_is_quarantined_not_followed` (symlinked blob, directory blob, symlinked descriptor → `not_private_regular_file`; outside file untouched) |
| Missing test: two-process quota | — | — | `apfs::native_apfs_quality_two_processes_share_one_global_quota_view` (ignored; process A spends 128 MiB, B sees the charge → `QuotaExceeded`, a second owner still fits, A's claims not displaced) |
| Missing test: `UtcInstant` | — | — | `domain::instants_cover_the_calendar_and_the_range_boundaries` (2038-01-19T03:14:08Z, 9999-12-31T23:59:59Z + overflow, 1970 lower bound, 2100-02-29 rejected / 2100-03-01 exact, 2000 & 2024 leap days) |

## Tests executed
| Command | Exit | Result |
|---|---|---|
| `cargo test -p rust-engineering-project --test quality_artifact_store` | 0 | **27 passed**, 4 ignored |
| `... --test quality_artifact_store -- --ignored --test-threads=1` | 0 | **4 passed** (+1 no-op helper each for the two spawned children) |
| `cargo test -p rust-engineering-project --test mutation_store` | 0 | **20/20 unchanged** |
| `cargo test -p rust-engineering-project -p rust-engineering-domain -p rust-engineering-application --locked --offline` | 0 | 48 test targets ok, 0 failed |
| `cargo test -p rust-engineering-execution --lib mutation_archive` | 0 | 12 passed (10 pre-existing M2 + 2 quality) |
| `cargo clippy -p rust-engineering-project -p rust-engineering-application -p rust-engineering-domain --all-targets --locked --offline -- -D warnings` | 0 | clean (I17c's `nextest.rs` failures no longer present) |
| `cargo clippy -p rust-engineering-execution --lib -- -D warnings` | 0 | clean |
| `rustfmt --edition 2024 --check <my files>` | 0 | clean |
| `python3 -B scripts/check-architecture.py` | 0 | PASS |

## Decisions
1. **F1 variant**: added `QualityArtifactError::UnsupportedStateRoot` rather than reusing `UnsupportedPlatform`, so the integrator can distinguish "this host cannot run the store" from "this operator root does not qualify". No code outside my files matches on this enum, so the addition is source-compatible.
2. **Operator report preserved**: because `reconcile` now reclaims, the free function `prune_expired` would have reported zero. It now gates on a lock-free `clock_regressed()` read of the watermark instead of a full `reconcile`, so the operator's `PruneReport` still counts what pruning removed and pruning still never re-bases the clock.
3. **Malformed records are still evidence**: `reclaim` skips `.reserve` files it cannot read, leaving reconciliation to quarantine them rather than silently unlinking them.
4. **Cursor grammar changed** (11 → 49 bytes, `m<10 digits>_<qart_…>`). It stays well under `QUALITY_CURSOR_MAX_BYTES`; the mcp-server encoder test uses an arbitrary cursor string and is unaffected.
5. **F5 marker placement**: `reservation/<artifact>.trunc`, written and fsynced before the blob rename, consumed after the descriptor directory fsync; an orphan marker (crash before commit) is reclaimed, so a marker can never license a later surplus.
6. `revalidate_quality_archive` now returns `(Vec<u8>, ArchiveBundleStats)`. It is still `#[allow(dead_code)]`/unwired, so no caller outside its own tests changes.

## Risks
- The `.trunc` marker adds one durable write plus two fsyncs to every publication whose member cap exceeds the actual size (the common case). That is the cost of proving surplus ownership; it is bounded and off the read path.
- Another session is editing this worktree concurrently (`mutation_archive.rs` was reformatted mid-run by that session; my hunks survived intact and I re-verified the file after).

## Open issues
1. **`quality-artifacts recover|prune` CLI is still absent** (`main.rs` is outside my ownership). In-product reclamation no longer depends on it — `reconcile` and every `reserve` reclaim — but the operator-facing command remains unowned.
2. **ADR-061's shared-primitive refactor of M2** (`mutation.rs`) is still not done, by ownership; `state_primitives.rs` now uses M2's *exact* probe rule, so the two agree behaviourally even though the code is still duplicated.
3. The non-macOS/`UnsupportedPlatform` path compiles under the new cfg but cannot execute on this aarch64 host.
4. The content-parser oracle rows and guest-side `GuestArtifactEgress` rejection remain outside this package.