Reviewed the three final CLI files (catalog_cli.rs, catalog_cli/floor.rs, catalog_cli/bundle_fixture.rs) plus the test file, focused only on observability, error mapping, and recovery paths.

## Findings

**1. Medium — human-readable output path is completely untested**
`crates/mcp-server/src/catalog_cli.rs:449-457` (the non-JSON branch of `run()`, which prints `floor_sequence`/`reservation_pending`/`network_used` as text) is never exercised. Every test helper `run()` (`crates/mcp-server/tests/catalog_cli.rs:42-53`) hardcodes `--json`. If the plain-text formatter regresses (wrong field, panic on `unwrap_or`, formatting mismatch), CI will not catch it even though this is a user-facing surface. Since this is one of the two "human+JSON" observability paths the task explicitly calls out, at least one test invoking `run` without `--json` and asserting on stdout text should exist before this is considered covered.

**2. Low — floor/state error coalescing reduces diagnosability but is intentional and correctly scoped**
`Floor::parse` (`floor.rs:37-56`) folds several distinct failure modes — malformed JSON, bad checksum, sequence `0`/overflow, hex-format violation, and non-canonical byte roundtrip — into a single `Error::State` → `CATALOG_STATE_INVALID`. Only publisher/channel mismatch is split out as `TrustMismatch`. This is a reasonable anti-oracle design (don't tell an attacker *which* byte of a tampered floor record is wrong) and matches the test at `catalog_cli.rs:130-133`/`399-402` (corrupted floor → `CATALOG_STATE_INVALID`) and `catalog_cli.rs:390-393` (channel change → `CATALOG_TRUST_MISMATCH`). No action needed, just noting the coalescing is deliberate, not an oversight, so it shouldn't be "fixed" into finer-grained codes later without a security review.

**3. Low — dead `#[serde(deny_unknown_fields)]` on output-only structs**
`Report` (`catalog_cli.rs:104-114`) and `CatalogStatus` (`catalog_cli.rs:115-132`) only ever `Serialize`; `deny_unknown_fields` has no effect on serialization and is a no-op here. Harmless but slightly misleading — a reader might assume it does something protective on the JSON emitted to stdout consumers. Not worth blocking on, but should not be copied into future serialize-only DTOs as if it provides schema enforcement.

**4. Informational — `network_used` semantics correctly cover failed attempts**
`network.set(true)` (`catalog_cli.rs:265`) fires unconditionally before `source.fetch()`, so a denied/failed HTTPS attempt still reports `network_used: true`, while a `SyncRemote` that fails before reaching that line (e.g., runtime construction failure) correctly reports `false`. This matches the stated intent ("includes failed transfers") and is consistent with all `network_used` assertions in the tests (`catalog_cli.rs:85`, `300`, `345`). No issue found — confirming this is correct rather than a gap.

**5. Verified correct — monotonicity/hash cross-check between floor and active**
`catalog_cli.rs:215-220`: the combined guard `active.sequence > floor.sequence || (active.sequence == floor.sequence && !floor.matches(active))` correctly catches both "active ahead of its own reservation" (should be impossible under correct commit ordering) and "same sequence but tampered/mismatched hash," both collapsing to `CATALOG_STATE_INVALID`. `active.sequence < floor.sequence` (crash between reserve and activate) is deliberately allowed through as `reservation_pending: true`, matching the recovery test at `catalog_cli.rs:157-208`. This is sound and exercised.

**6. Verified correct — pre-write floor round-trip check**
`catalog_cli.rs:282-283` serializes the new floor, then immediately re-parses it through `Floor::parse` (including trust check and checksum/round-trip verification) before it's ever written to disk via `reserve_floor`. This guards against a serialization bug silently producing a floor record that would later fail to parse (bricking recovery). No issues in the logic; it's a pure defensive read-after-write-before-write check with no side effects if it fails (returns `Err` before touching the store).

**7. Verified correct — semantic index validation ordering**
`catalog_cli.rs:277-286`: `validate_imported_index` runs and must succeed before `reserve_floor`/`commit`, matching the `SEMANTIC_REBUILD_UNAVAILABLE` test at `catalog_cli.rs:319-353`, which asserts none of `active.bundle`/`floor.record`/`staging.bundle`/`floor.staging` exist after a signed-but-corrupt-native-payload import is rejected. Ordering is correct and tested.

**8. Verified correct — key rotation test is real, not simulated**
`bundle_fixture.rs:103-134` (`resign_with_new_key`) uses actual `Ed25519KeyPair::from_seed_unchecked(&[43; 32])` to re-sign the manifest and rewrites `trust.public_key` to the new key's real public bytes; the CLI test (`catalog_cli.rs:361-403`) round-trips this through separate process invocations. `ACTIVE_UNVERIFIED` before rotation-import and successful import of a strictly-newer sequence after are both exercised against genuine signature verification, not a mock.

## Summary
No correctness or security defects found in the final CLI/error/recovery layer itself. The one actionable gap is item **1**: the plain-text (non-`--json`) output branch has zero test coverage despite being a stated deliverable (human+JSON emission of `floor_sequence`/`reservation_pending`). Recommend adding a minimal test that runs without `--json` and checks the text output before calling this surface complete.
