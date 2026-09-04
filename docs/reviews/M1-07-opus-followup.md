# M1-07 focused followup

Claude Code2.1.259, explicit claude-opus-5 Medium, actual model verified, read-only.

## Verdict on the five P1 corrections

Read-only analysis of the pasted sources; I did not run anything and make no claim about gate results. Files not provided (`catalog-adapter/src/audit/tests.rs`, `stdio/auditing/schemas.rs`, `stdio/auditing/tests.rs`, `super::{sql,empty,fingerprint,budget}`, application `Registry::audit`) are called out where they bound a conclusion.

**1. Empty snapshot / count+sequence exposure — resolved.**
`RustSecSnapshot::from_bytes` rejects `records.is_empty()` and `sequence == 0` before construction, so `record_count > 0` holds for any live snapshot (`crates/catalog-adapter/src/audit.rs`, `from_bytes`). Independently, the MCP boundary re-derives it: `classify` requires `snapshot_record_count.is_some_and(|c| c > 0)` and `snapshot_sequence.is_some_and(|s| s > 0)` before `validation_complete` survives, so a non-catalog `DependencyAuditPort` returning `Some(0)` yields `Incomplete`, never `Passed`. `AuditObservation::unavailable()` leaves both `None` and short-circuits earlier at the `snapshot.is_none()` branch. Both counters are bounded (`MAX_RECORDS = 2048`, u32/u64) and carry no host path or source string.

**2. Strict kind/url/precise, locked vs default-None — resolved, with a duplication caveat (see P1 below).**
`lock.rs::same_source` compares `kind`/`url`/`precise` and is used for node identity, dependency edge resolution, and the cross-check against `Lockfile`'s own parse; `SourceId`'s relaxed `Ord/Eq` is never used. The canonical registry is obtained by round-tripping (`source_id(&SourceId::default().to_string())`), i.e. through the same parser that produced the lock sources, so the `precise = "locked"` marker is compared like-for-like. I confirm your reading of the earlier finding: `format!("{REGISTRY}#locked")` differs in `url()` (fragment retained), not in `precise()`, and is asserted rejected in `canonical_registry_comparison_preserves_precise_and_exact_origin`, alongside `?precise=locked`, trailing `/`, credentialed authority, and `git+` kind. Credential-bearing origins are only ever emitted as a SHA-256 digest (`AuditPackage.source_fingerprint`), asserted by `local_external_and_credential_sources_never_become_crates_io`.

**3. Provider classification — resolved.**
`snapshot_read_error` in `stdio/auditing/provider.rs` separates the three classes correctly, and — the substantive fix — only `AuditDataError::Unavailable` is converted to `AuditObservation::unavailable()` in `AuditProvider::audit`. `CommandTimeout → Timeout`, `SandboxDenied/NetworkDenied → SandboxDenied`, `OutputLimitExceeded → Budget` all propagate as errors into `data_error`, so a deadline or policy denial can no longer present as "snapshot unavailable" with retained project evidence. Deadline and sandbox codes route through `operational(...)` to `CommandTimeout`/`SandboxDenied` rather than `AuditSnapshotUnavailable`.

**4. State precedence — resolved.**
Order in `output()` is: provenance kind/network inconsistency → `Blocked/AuditSnapshotInvalid` (data dropped); integrity ≠ `Verified` → `Blocked/AuditIntegrityFailed` (data retained); `SnapshotUnavailable|Stale|UnknownAge` → `Unavailable` with data; then `Passed` only under `state == Passed && validation_complete`; then `Failed` on non-empty findings; else `Blocked/AuditIncomplete`. `classify` runs first and cannot be bypassed, treats `FreshnessState::Live` as unknown age (correct for an offline snapshot), and folds `UnsupportedSources`/`OutputBudget` into `issue.is_none()` so those degrade to `Incomplete`. Findings under a stale snapshot report `Unavailable` with findings retained rather than `Failed` — conservative and consistent with the catalog-side `output.state` assignment; not a defect. `joined_result` discards an `Ok` when the worker was interrupted.

**5. Path caching / retention cap before BFS, bounded display metadata — resolved.**
`cached_paths: Option<(Vec<AuditPath>, u64)>` is per scanned package index, so BFS runs at most once per package rather than once per advisory row. The `findings.len() + informational.len() >= MAX_FINDINGS` check precedes the `graph.paths(index, control)` call, so capped-out findings do not pay for a traversal. `LockGraph::paths` is single-source reverse BFS with `distance`/`next`, O(V+E) with a checkpoint per edge; `path_depth_and_root_count_omissions_count_roots_not_routes` pins that omissions count roots (10 roots → 8 paths + 2 omitted, and 10 omitted past depth), and `exponential_route_graph_keeps_one_representative_in_linear_work` pins linear work under a 1000-checkpoint budget. Display metadata is bounded at ingest: `advisory_id` is grammar-checked `RUSTSEC-dddd-dddd` and the `url` is constructed from it, `title ≤ 512` and `informational ≤ 128` via `valid_text`, requirement lists ≤ 64 entries × ≤ 256 chars (semver `Display`, ASCII), package name/version from the lock's validated ASCII grammar, paths ≤ 8 × ≤ 32.

## Remaining actionable

### P1 — `registry = 'other'` silently drops advisories with no counter or issue

`crates/catalog-adapter/src/audit.rs`, `from_bytes`, registry classification:

```rust
Some(_) => "other",
```

and `audit()` queries `WHERE package=?1 AND registry='crates_io'`. Any record classified `other` is stored but can never match, and nothing records that it happened: `snapshot_record_count` still counts it, no `AuditIssue` is raised, and `validation_complete` stays true. This is the one place in the vertical that reduces coverage without becoming visible — the lock side routes an unrecognised origin to `AuditSource::Unverified` → `unsupported_packages` → `AuditIssue::UnsupportedSources`, but the snapshot side has no counterpart.

Two things make this reachable in practice rather than theoretical:

- The invariant is duplicated, not shared. `from_bytes` hand-rolls `origin.kind() == canonical.kind() && origin.url() == canonical.url() && origin.precise() == Some("locked")` against `rustsec::SourceId::default()` (whose `precise` is `None`), while `lock.rs` uses `same_source` against a round-tripped canonical. If a `cargo-lock`/`rustsec` upgrade changes how `Advisory.metadata.source` is deserialized — or whether the `locked` marker is applied on that path at all — every advisory with an explicit `source` flips to `other` and audits pass silently. `lock.rs`'s test pins the marker for the lockfile parser; I cannot see whether `catalog-adapter/src/audit/tests.rs` pins it for the advisory-metadata parser.
- Fail direction is open. Misclassification removes findings; the tool then reports `Passed` with `validation_complete: true`.

Minimal fix, in order of preference: reject the record at `validate`/ingest (`AuditDataError::InvalidSnapshot`) when `metadata.source` is present and not canonical — a `Collection::Crates` advisory with a non-crates.io source is already anomalous given `collection` is enforced and the package name grammar is ASCII-restricted; the `registry` column and index then become unnecessary. If you prefer to retain such records, count them at ingest and surface a non-`None` `AuditIssue` so `validation_complete` cannot hold. Either way, reuse a single `same_source`-style comparison (lift it into a shared helper) and add the `precise == Some("locked")` regression assertion on the advisory-metadata path, mirroring `canonical_registry_comparison_preserves_precise_and_exact_origin`.

### P2 — brief, only where material

- **`valid_text` misses `Cf`/`Zl`/`Zp`.** `crates/catalog-adapter/src/audit.rs`: `c.is_control()` is Unicode `Cc` only, and the explicit list covers bidi marks/embeddings/isolates. `U+2028`/`U+2029` (`Zl`/`Zp`) are neither, are emitted unescaped by `serde_json`, and render as line breaks in many clients — a title can therefore fabricate a line break in rendered output; `U+FEFF`, `U+200B`, `U+00AD` pass as invisibles. Given you already reject bidi deliberately, closing this to "reject `Cc`, `Cf`, `Zl`, `Zp`" is a one-predicate change and removes the residual display-spoofing surface on `title`, `informational`, and `source_id`.
- **`classify` omits the `created_at` prerequisite** that the catalog layer enforces. `stdio/auditing.rs::classify` checks `observed_at`, `assessed_at` ordering and `FreshnessState`, but not `provenance().created_at().is_some()`, whereas `RustSecSnapshot::audit`'s `times_known` requires both. If the stated property is that the MCP boundary re-derives pass preconditions independently of the port, add the `created_at` check for symmetry.
- **BFS continues after the payload budget is exhausted.** In `audit()`, once `size + bytes > MAX_PAYLOAD` starts rejecting findings, the count-based pre-check still passes (`< MAX_FINDINGS`), so every subsequent matching package pays a full `graph.paths` traversal whose result is immediately discarded. Bounded (≤ ~1024 packages × O(V+E) with V ≤ 1024, E ≤ 8192, all checkpointed, so cancellable and well inside the 120 s deadline) — not a DoS, but a `budget_exhausted` flag gating the `graph.paths` call would remove the waste.
- **Large `unsupported_packages` fails hard with no evidence.** In `audit()`, the initial `size > MAX_PAYLOAD` check returns `Err(Budget)` → `Blocked/AuditBudgetExceeded` with `data: None`, dropping all project evidence. `encode_bounded` at the MCP layer already knows how to pop `unsupported_packages` and account for it in `truncation.unsupported_packages_omitted`, but never gets the chance. Worst case is reachable on paper (1024 entries × ~350 B ≈ 358 KB > 256 KB), though it needs an unusual dependency mix. Truncating with a counter in the catalog layer would match the boundary layer's behaviour; the current path is safe, just lossy.
- **Provider test gap.** `provider.rs` `host_snapshot_errors_preserve_missing_policy_deadline_and_budget` covers seven codes but omits `ToolNotInstalled → Unavailable`, which is the other "missing" code and the one that decides whether project evidence survives. One line.

Nothing in scope looks like a P0.
