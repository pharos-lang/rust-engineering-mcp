Done. Full report below.

## Task
Diagnose and fix the flaky expiry-family tests in the ADR-061 durable quality artifact store (`crates/project-adapter/tests/quality_artifact_store.rs`).

## Result
Fixed. The store was correct; the two expiry tests were racing a deadline they could not win. The store now exposes an injectable observed-wall-clock (mirroring its existing `with_fault_injection` hook), both tests name their TTL boundaries exactly instead of sleeping, and every assertion is preserved — two are tightened. 90 loop iterations across three modes now pass 90/90.

## Root cause — (a), wall-clock progression, with the precise mechanism

`UtcInstant` has whole-second resolution, so `wall_now()` truncates: `draft(_, _, 1)` and `short_claim(_, _, _, 1)` did not create a one-second deadline, they created a deadline at `floor(now) + 1`. The runway a test actually had was `1 − frac(now)` seconds — uniform in (0, 1] — and inside it the test had to complete a `fallocate` of up to 8 MiB, a streamed write, an fsync, two renames and three directory fsyncs. When that did not fit, the store correctly refused at `crates/project-adapter/src/filesystem/macos/quality.rs:1119`.

**Captured failure** (iteration 22 of a 30× loop, default `--test-threads`, before any change):

```
running 2 tests
test apfs::expired_evidence_and_claims_stop_being_charged_and_leave_the_volume ... FAILED
test apfs::expiry_reclaims_only_known_bytes_and_reads_never_renew ... ok

---- apfs::expired_evidence_and_claims_stop_being_charged_and_leave_the_volume stdout ----
Error: Expired

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 29 filtered out; finished in 1.73s
```

**Deterministic confirmation.** A temporary probe (since removed) busy-waited to ~20 ms before a whole second — the worst case the suite already drew at random — then ran the failing test's opening sequence, splitting ingest from commit:

```
PROBE reserve=Ok(()) ingest=Ok(4096) publish=Some(Err(Expired))   ×5
PROBE reserve=Ok(()) ingest=Ok(4096) publish=Some(Ok(()))         ×1
```

`reserve` always succeeded, `ingest_member` always succeeded, and `publish_descriptor` failed 5/6. The margin is consumed by the ingest fsyncs, exactly as predicted.

**Ruling out the other three candidates:**
- **(b) race between reclamation and a concurrent read/attach** — no. The error is `Expired`, raised before any reclamation runs; each test owns a private random fixture, and the boundary-aligned probe fails in isolation with no concurrency at all. `--test-threads=1` does not fix it.
- **(c) ordering of `reclaim` inside `reserve`/`reconcile`** — no. The failing call is `publish_descriptor`, which never reclaims, and `reserve` (which does) succeeded in every probe run.
- **(d) APFS timestamp granularity** — no. No filesystem timestamp participates in expiry; the store compares `UtcInstant` values derived from `SystemTime::now()`. The one mtime assertion in the suite (descriptor unchanged by a read) never failed.

## Fix

The cause is fixed, not the assertion.

1. **`crates/application/src/quality_artifact.rs`** — new `QualityClockSource` trait beside `QualityFaultInjection`: a test-only source of the observed wall clock in whole seconds.
2. **`crates/project-adapter/src/filesystem/macos/quality.rs`** — `NativeQualityArtifactStore` gains a `clock: Option<Box<dyn QualityClockSource>>` field, a `#[doc(hidden)] with_clock_source` constructor that re-bases the monotonic origin on the installed source, and a private `observed_wall_seconds()` used by `now()`, `clock_regressed()` and `reconcile()`. With no source installed the code path is byte-identical to before. The hybrid clock is unchanged: `now()` is still the later of the observed reading and the monotonic projection, so an installed source can shorten a TTL but never lengthen one.
3. **`crates/project-adapter/tests/quality_artifact_store.rs`** — `short_claim` is replaced by `claim_at`/`draft_at` (anchored on an explicit instant) plus a `TestClock` the test moves by hand. Both `std::thread::sleep(1_500ms)` calls are gone.
   - `expiry_reclaims_only_known_bytes_and_reads_never_renew` publishes as of an hour ago, moves the injected clock 30 minutes on for the in-session assertions, then lets the **real** operator `prune_expired(&state)` judge the same TTLs — the short one expired 50 minutes before it, the long one due 23 hours after it. The operator entry point keeps its coverage, now with a 50-minute margin instead of a sub-second one.
   - `expired_evidence_and_claims_stop_being_charged_and_leave_the_volume` uses 300-second TTLs and then jumps the injected clock to +600 s.

Every original assertion survives. Two are strengthened: the surviving descriptor's bytes and mtime are now also asserted unchanged *after* the post-expiry reads (previously untested), and the post-prune check is `fs::read(&path)? == before` rather than a length comparison. Both TTL rules still discriminate — reads never renew (`chunk.descriptor.expires_at_utc == long.expires_at_utc`, bytes and mtime identical) and only known bytes are reclaimed (`removed == 1`, `reclaimed_bytes == 9`, `retained == 1`, short blob gone).

## Files changed

| File | SHA-256 |
|---|---|
| `crates/project-adapter/tests/quality_artifact_store.rs` | `554409aaf101e9730f46ba7e8e1c1fe03cc21be6e7e0f5ee95724b48cf521b42` |
| `crates/project-adapter/src/filesystem/macos/quality.rs` | `271dab3b782336df43508748ddac7119387181484aad69bb61d467b0b37d6940` |
| `crates/application/src/quality_artifact.rs` | `59ffafa6ac68917fd1e0e1b2841e4666266855202491b42289266166c1c94cdf` |

No other file was touched.

## Tests executed

| Run | Result |
|---|---|
| Expiry family, 30× default parallelism (**before fix**) | **29 pass / 1 fail** — reproduction above |
| Boundary-aligned probe, 6× (**before fix**) | **1 pass / 5 `Expired`** |
| Whole file, 30× default parallelism | **30/30** (27 passed, 4 ignored each) |
| Whole file, 30× `--test-threads=1` | **30/30** |
| Whole file, 30× `--include-ignored --test-threads=1` | **30/30** (31 passed each) |
| Whole file, 30× `--include-ignored` at full parallelism | **16/30** — pre-existing, unrelated; see Open issues |
| Same, 20×, skipping the two process-spawning tests | **20/20** |
| `cargo test -p rust-engineering-project --locked --offline` | all green; **`mutation_store` 20/20**, `quality_artifact_store` 27/27, lib 65/65 |
| `cargo clippy -p rust-engineering-project --all-targets --locked --offline -- -D warnings` | clean |
| `cargo clippy -p rust-engineering-application --all-targets --locked --offline -- -D warnings` | clean |
| `cargo check --workspace --all-targets --locked --offline` | clean |

The two expiry tests also dropped from ~1.8 s to 0.28 s.

## Risks

- `with_clock_source` is a `pub` (though `#[doc(hidden)]`) injection point on a durable store, following the precedent already set by `with_fault_injection`. It replaces only the observed reading; the hybrid `max` with the monotonic projection means an installed source fails in the safe direction (a TTL can be shortened, never extended).
- `reconcile()` now reads the observed clock rather than calling `wall_seconds()` directly. Identical in production. Under a deliberately backdated source it would report a clock regression — correct behaviour; the test installs its source after `open()` for that reason.
- The in-session TTL check is no longer exercised against the host clock by these two tests. That coverage is preserved: test 1's operator prune runs on the real clock, and `a_durable_clock_regression_blocks_only_quality_until_recovery` remains entirely host-clock driven.

## Decisions

- Injected the clock rather than lengthening the sleep or loosening an assertion — a longer sleep only lowers the flake rate, it does not remove the dependence on `frac(now)`.
- Kept test 1 on the free `prune_expired(&state)` operator entry point (real clock) instead of switching to the trait method on the injected-clock handle, so that path keeps its happy-path coverage. Backdating the publication is what makes both clocks agree with a wide margin.
- Did not change `UtcInstant`'s second resolution or the `>=` in `is_expired`. Both are ADR-061 semantics and the store's behaviour is correct; the defect was in what the tests assumed about them.
- Removed the diagnostic probe once it had done its job, rather than keeping a timing-dependent test in the suite.

## Open issues

1. **Separate flake: `Busy` under `--include-ignored` at full parallelism.** 14 of 30 runs fail with `Error: Busy` in arbitrary tests (`flood_beyond_the_exact_cap_publishes_nothing`, `a_page_boundary_advances_past_stored_objects_sharing_an_index`, `a_planted_symlink_or_non_regular_object_is_quarantined_not_followed`, and others I did not touch). Isolated to the two `#[ignore]`d process-spawning tests: skipping them gives 20/20, and `--test-threads=1` gives 30/30. Best-supported explanation is that spawning a child from the parallel harness transiently duplicates `store.lock` descriptors held by sibling threads; I did not verify that at the kernel level, so I have recorded it as an observed constraint in the file header rather than asserting the mechanism in code. The project's gate already runs these `--exact --ignored --test-threads=1`, so nothing in CI is affected. Left unfixed — deciding whether to restructure the spawning tests is the owner's call.
2. `advance_watermark` overwrites the durable watermark rather than taking a max with it, so a backdated clock source moves it backwards. Harmless in production (the hybrid clock is non-decreasing within a session) and harmless for the test, but worth knowing if a clock source is ever installed for anything but a test.
3. Any real caller minting a sub-minute TTL races the store's own fsyncs for the same reason these tests did. Nothing in M3 does so today. If short TTLs are ever exposed to clients, the second-resolution anchor deserves an explicit note in ADR-061.
4. Two scratch driver files, `/tmp/s01_loop.sh` and `/tmp/s01_loop.py`, were created early while working out a loop the sandbox would permit; neither was ever executed and the session's permissions do not let me delete outside the working directory. Nothing was left inside the repo — the `target/s01_*.txt` loop logs are removed.