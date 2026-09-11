## M1-13 Principal Review — `rust.crate.inspect`

Scope: read-only review of the diff above. One finding rises to actionable; everything else I checked (generation-identity pre/post check, snapshot_fingerprint gating, offset==total vs offset>total semantics, output-budget trimming/`omitted_by_output` recompute, cancellation-overrides-stale-success in `stdio/crate_inspect.rs`, latest_known_stable computed independent of yanked, documentation/source explicit-unknown tagging, 13-tool snapshot count) matches ADR-044 and is exercised by the provided tests, so I'm not re-litigating it.

### P2 — Dependency page ordering check can reject valid pages that share `(name, kind)`
**File:** `crates/application/src/crate_inspect.rs`, `dependency_key` (~line 84) and its use in `validate_page`'s `Dependencies` arm (~line 178):
```rust
fn dependency_key(value: &DependencyRecord) -> (&str, &str) { (name, kind_str) }
...
|| !items.windows(2).all(|pair| dependency_key(&pair[0]) < dependency_key(&pair[1]))
```
The strict-monotonic ordering key is `(name, kind)` only — it ignores `requirement` and `optional`. Two dependency rows with the same package name and the same `DependencyKind` but different `requirement` or `optional` (e.g. target-conditional deps declared under separate `[target.'cfg(...)'.dependencies]` blocks with different version ranges, or a normal + optional-normal split — both legitimate, non-rare Cargo.toml patterns) produce a tie in `dependency_key`. A tie breaks `pair[0] < pair[1]`, so `validate_page` returns `CatalogError::InvalidSnapshot`, and the tool surfaces `CatalogInvalid` ("Catalog facts could not be verified") for an entirely valid crate/version instead of the real data.

No existing test exercises same-name+same-kind duplicates (the fixture only varies kind for the "same" name in `crates/catalog-adapter/tests/crate_inspect.rs`), so this gap isn't currently caught.

**Caveat (unshown context):** I don't have the schema1 DDL for the `dependencies` table. If it enforces `UNIQUE(version_id, name, kind)`, this can't occur and the finding is moot — but nothing in the reviewed files establishes that constraint, and the SQL adapter query (`ORDER BY name, kind`, `crates/catalog-adapter/src/inspect.rs`) does not assume or enforce it either. Recommend confirming the schema constraint, or if duplicates are possible, extending `dependency_key` to include `requirement`/`optional` (and the SQL `ORDER BY`) as a tertiary/quaternary sort key so ties are actually distinguishable in a deterministic order.

### Non-findings worth naming explicitly (not defects, flagging so they aren't re-derived)
- The `-32602`-vs-tool-envelope split for `offset > total` (`CatalogError::InvalidInput` routed to `ErrorData::invalid_params` in `crates/mcp-server/src/stdio/crate_inspect.rs` `output()`) is architecturally unusual — a data-dependent condition surfaced as a protocol error rather than a `Blocked` envelope — but it's explicit ADR-044 policy ("invalid offset beyond total is invalid input") and is directly asserted by `actual_mcp_continuation_detects_restart_generation_and_rejects_invalid_pages`. Not a defect.
- Given current field bounds (description ≤4096, repository ≤2048, ≤50 items/page each a few hundred bytes), the `OutputLimitExceeded`/trim-to-one-then-fail path in `encode_bounded` (`crates/mcp-server/src/stdio/crate_inspect.rs`) looks unreachable in practice — total page size can't approach 512KiB. This is dead-code-in-practice defensive plumbing per ADR, not incorrect; not actionable.

No P0/P1 defects found in the reviewed diff.
