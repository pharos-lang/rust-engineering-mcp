## M1-12 Read-Only Review Findings

**Scope reviewed:** ADR-043, domain/application crate-search types and use case, SQLite adapter, MCP tool wiring, and the excerpted `CatalogProvider::search`. No edits made.

### P2 — Divergent crate-name validators risk an untyped `internal_error` for data faults

`crates/application/src/crate_search.rs:48-54` (`name_valid`) validates every lexical/semantic candidate name (ASCII alnum + `_`/`-`, len ≤ 64) *before* calling `select()`. But `crates/catalog-adapter/src/search.rs:56` re-validates the same identity independently via `records::valid_name`, returning `CatalogError::InvalidInput` on failure. If the adapter's validator is ever stricter (or simply drifts) relative to the application-layer one — e.g. after a future crates.io charset change is patched in only one place — a candidate that already passed `name_valid` can fail adapter validation. That `InvalidInput` propagates through `select()` (`crates/application/src/crate_search.rs:169`) to `search_crates`, and `crates/mcp-server/src/stdio/crate_search.rs:118` maps `CatalogError::InvalidInput` to `ErrorData::internal_error("Crate search validation failed", None)` — a raw JSON-RPC error rather than the closed `Outcome::Unavailable{CatalogInvalid}` contract used for every other snapshot-integrity fault (line 107-110). This is the one place a data/index inconsistency (not a client input problem) can bypass the typed error contract. Recommend either sharing a single `valid_name`/`name_valid` implementation between `application` and `catalog-adapter`, or mapping adapter-side `InvalidInput` for select-by-candidate-name to `CatalogError::InvalidSnapshot` so it surfaces as `CatalogInvalid` like every other corruption case, instead of `internal_error`.

### P3 — Redundant/dead defensive check

`crates/application/src/crate_search.rs:204`: `version.published_at.is_some_and(|v| v > i64::MAX as u64)`. The value only ever originates from `u64::try_from(row.get::<_, Option<i64>>(5)?...)` in `crates/catalog-adapter/src/search.rs:77`, so it can never exceed `i64::MAX`. Harmless but dead defense-in-depth; not a functional defect, no action required unless the storage type changes.

### Verified correct (no defect) — noted so it isn't re-litigated

- FTS5 query construction (`crates/catalog-adapter/src/search.rs:35-40`) quotes and AND-joins each term as a bound parameter; no SQL or FTS syntax injection (confirmed against `fts_terms_are_literal_and_never_expand_or_or_prefix_syntax`).
- Candidate windows are correctly capped at 50/channel and the union-select cache boundary (`crates/application/src/crate_search.rs:164-165`) is exactly bounded at 100 with no off-by-one.
- RRF fusion `1/(60+rank)`, descending sort, name tie-break, and per-mode ranking (`crates/application/src/crate_search.rs:326-361`) match ADR-043 exactly.
- `latest_known_stable` selection is correctly independent of `filters` and retains its yanked bit (`crates/catalog-adapter/src/search.rs:96-99` vs. ADR line 43).
- MSRV parsing/comparison (leading-zero rejection, 2-3 part canonical form, u64 numeric — not lexicographic — comparison, unknown/unstable/build-metadata exclusion only when a filter is supplied) is correct and consistent between the wire schema regex (`crates/mcp-server/src/stdio/crate_search.rs:18`) and `MsrvVersion::parse` (`crates/domain/src/crate_search.rs:20-46`).
- Semantic-channel identity invalidation (unknown/duplicate/negative/non-finite distance) correctly triggers lexical fallback with filters preserved; a stale FTS↔crates mismatch on the lexical side correctly hard-fails as `InvalidSnapshot` rather than silently dropping a result, consistent with SQLite being sole authority.
- Output-budget trimming (`crates/mcp-server/src/stdio/crate_search.rs:173-186`) pops lowest-ranked results, increments `omitted_by_output`, correctly falls back to `OutputLimitExceeded` when a single metadata-only result still doesn't fit, and terminates without an infinite loop; `eligible`/`limit_truncated`/`returned`/`omitted_by_output` bookkeeping is internally consistent (matches `complete_result_budget_trims_suffix_without_cropping_facts_or_losing_counts`).
- Cancellation checks (`control.check()`) are present at every I/O and loop boundary in `search_crates`, and `encode_bounded` re-checks cancellation each trim iteration before any success can be published (matches `cancellation_during_budget_trimming_cannot_publish_success`).
- 64-version and 128-advisory-ID sentinel queries (`LIMIT 65` / `LIMIT 129`) correctly reject snapshots that violate the stated import invariants rather than silently truncating.
- `provider.search` holds the shared `state` Mutex for the whole search (including the nested `block_on`), which is redundant given the joined-worker semaphore(1) already fully serializes calls, but is not a correctness defect.

No P0 or P1 defects were found in ranking, filtering, fallback, authority, or budget handling.
