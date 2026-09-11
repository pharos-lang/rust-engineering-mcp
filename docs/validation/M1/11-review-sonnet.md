Read-only Claude Code2.1.259; requested/observed claude-sonnet-5, High. No code tools enabled. CLI auxiliary modelUsage includes Haiku; substantive review model confirmed Sonnet5.
Packet SHA256: bb3fdb818800f07dc5471fda1d2a2e2e36fa0835a8dbf8a693d0218c6f0686e3

# M1-11 Catalog Status — Review

Scope note: I only reviewed the files actually shown above. `catalog-adapter/src/audit.rs`, `bundle.rs`, `project-adapter/src/catalog_store.rs`, and the three new test files (`application/tests/catalog_context.rs`, `domain/tests/catalog_context.rs`, `mcp-server/tests/protocol.rs`, the snapshot JSON) were listed as changed but not shown to me, so I can't confirm their content — findings below are scoped accordingly.

## P1

**RustSec `record_count` bound is an unenforced magic number, both ends** — `crates/application/src/catalog_context.rs:168`
```rust
|| !(1..=2048).contains(&rustsec.record_count)
```
- Upper bound: unlike `crate_count > 1000` (`catalog_context.rs:126`), which mirrors a real, enforced bundle protocol limit (task states 80MiB/1000-crate bound), 2048 has no corresponding enforced constant visible anywhere in this diff. RustSec's advisory-db is an open-ended, externally-growing dataset, not bounded by this codebase. If/when the real snapshot exceeds 2048 records, every `rust.catalog.status` call will fail `validate()` and return `ProjectError::Internal` → a hard `ErrorData::internal_error` (`stdio/catalog.rs:177-182`), not a graceful "unavailable" component. That's a full tool outage triggered purely by upstream data growth, with no test guarding the boundary.
- Lower bound: a legitimate but empty snapshot (`record_count == 0`, e.g. a fresh/filtered advisory feed) also hits `Internal` instead of being reported as a normal fact. There's no reason a 0-record snapshot should be a domain-invariant violation.
- Action: confirm whether 2048 is meant to mirror a constant already enforced by the audit adapter (not shown to me); if not, either drop the upper bound or source it from a shared constant, and allow `0` unless truly impossible.

## P2

**"No network acquisition" trust boundary isn't defended at the validation layer** — `crates/application/src/catalog_context.rs:110-114`
```rust
fn provenance(value: &Provenance, kind: SourceKind) -> bool {
    value.source_kind() == kind
        && value.integrity() == IntegrityStatus::Verified
        && value.source_id().as_str().len() <= 256
}
```
`network_used` is never checked. The ADR and tool description both make an explicit promise ("Runtime cannot sync/download... acquisition_allowed=false"), but nothing in `validate()` asserts `!provenance.network_used` for catalog/model/rustsec provenance before it's trusted and surfaced. Currently this likely holds only because adapters never construct a network-derived `Provenance` for these paths — but `validate()`'s whole purpose (per its own doc comment, `domain/catalog_context.rs:1-2`) is to not rely on adapter good behavior alone. Cheap, high-value defense-in-depth fix.

**Missing tests: `rustsec()` OperationalErrorCode → `CatalogComponentUnavailable` mapping** — `crates/mcp-server/src/stdio/catalog/provider.rs:313-322`. Only the success and `Integrity`→`IdentityMismatch` paths are exercised (`provider/tests.rs:288-331`). `UnsupportedPlatform`, `Denied` (via `SandboxDenied`/`NetworkDenied`), `Budget` (`OutputLimitExceeded`), and `Missing` (`ProjectNotFound`/`ToolNotInstalled`) branches are all unverified.

**`load_semantics` entirely untested** — `provider.rs:186-295`. Both the `#[cfg(not(feature = "local"))]` `FeatureDisabled` path and the full `#[cfg(feature = "local")]` path (model load success/failure, index restore, `names != index.crate_names()` → `IdentityMismatch`, `DependencyUnavailable`) have no visible test coverage. This matches the stated "native tests pending" caveat — flagging the precise scope rather than claiming it's a surprise.

## P3 (style/design, not exploitable)

- **Likely-dead `Budget` branch for the `catalog` component** — `provider.rs:87-89, 101-103, 135-137` set `Err(Budget)` on `control.check().is_err()` inside `read_catalog`'s closure, but `control.check()?` immediately after at `provider.rs:177` runs before the `match result` at `provider.rs:179` and — assuming cooperative checks are monotonic once failed (deadline/cancellation don't self-heal) — will itself fail first, short-circuiting `read_catalog` via `?` before the `Budget` value is ever consulted. If so, `CatalogComponentUnavailable::Budget` can never actually reach a client for this component; worth confirming against `InspectionControl::check()`'s exact semantics and simplifying if confirmed unreachable.
- **`semantic_index` breaks envelope symmetry** — `domain/catalog_context.rs:121` keeps `Component<CatalogIndexObservation>` (no `evidence`/freshness), unlike `catalog`/`model`/`rustsec` which all get wrapped with `SnapshotEvidence` via `assess()` (`application/catalog_context.rs:40-67`). Probably intentional (index freshness is implied by catalog's, enforced via `snapshot_fingerprint` equality in `validate()`), but worth an explicit note so it doesn't read as an oversight.
- **Uniform 1-day/7-day freshness thresholds** applied identically to catalog, model, and rustsec (`application/catalog_context.rs:94-96`) — a pinned embedding-model artifact plausibly shouldn't age out the same way a catalog snapshot does. Worth confirming this is an intentional M1-11 simplification.
- **Duplicated bootstrap-blocked construction** — `stdio/catalog.rs:254-256` and `276-285` both build a `SandboxDenied` outcome for the same `bootstrap` condition, just with different message text. Harmless, minor redundancy.

## What checked out correctly (no issues found)

- Ed25519 verify-before-parse ordering, no-follow/symlink/hardlink denial, and floor/active read-tearing protection (`read_catalog`'s before/active/after retry) are all sound and covered by discriminating tests (`symlink_and_hardlink_active_are_denied`, `missing_or_corrupt_floor_never_authenticates_existing_active`, `reserved_newer_floor_preserves_older_active_as_explicit_pending_state`).
- Session-generation pinning (including pinning an *unavailable* initial load, per ADR) is correctly implemented in `CatalogStatusPort::observe` (`provider.rs:355-374`) and matches its test.
- RustSec is correctly re-read independently every call while catalog/model/index are cached — `provider.rs:371` vs. the `state.is_none()` gate at `provider.rs:362`.
- Worker admission/late-success-discard logic (`joined_result`, `stdio/catalog.rs:201-209`) is correct and tested for the Cancelled/TimedOut/Internal interleavings.
- Wire contract is closed (`deny_unknown_fields` throughout, empty `Input`, closed `Component` tagging) and tested against schema for every nested object plus the nullable `reservation` field.
