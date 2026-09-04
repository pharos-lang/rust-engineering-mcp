# ADR-043 — Bounded catalog retrieval modes and authoritative filters

## Status

Accepted for implementation,2026-09-04. M1-12 starts from clean main8ce95be,
after M1-11 integration and smoke. No M1 closure or M2 scope change.

## Context

The owner explicitly requires lexical, semantic and hybrid search. Foundation
ADR-017/027 lexical-first merging cannot express semantic-only or fuse a full
lexical page; its scores are discarded. SQLite remains authoritative and the
verified M1-11 generation already retains the model and restored native index.
The specification input is conceptual; this ADR fixes the public M1 contract.

## Decision

Add only rust.crate.search (tool12). Closed input: query, optional mode(default
hybrid), limit(default10,1..50), filters(default empty): msrv_lte(optional canonical
major.minor[.patch]), allow_yanked(defaultfalse), include_prerelease(defaultfalse).
Query retains the existing256UTF-8-byte/16term/control-character constraints.
No SQL, arbitrary FTS syntax, paths, refresh, model or network authority in input.

New bounded retrieval use case preserves the foundation APIs. At most50 candidates
per channel, union at most100. SQLite FTS5 uses literal escaped AND terms and
explicit bm25(crate_fts), lower score first, name tie break. Semantic retrieval uses
verified E5 and restored Lance L2 (squared Euclidean for the pinned implementation),
lower distance first, name tie break. All scores must be finite. They describe
retrieval only, never crate quality, safety or current registry completeness.

Lexical mode uses only lexical candidates. Successful semantic mode uses only
semantic candidates. Hybrid ranks the union by sum(1/(60+one_based_channel_rank)),
highest first, name tie break; it retains original per-channel rank/score evidence.
This deterministic rank fusion avoids equating BM25 and vector score scales.
The50-candidate window is explicit; no global has_more/completeness assertion.
Filters apply to all selected candidates before the final result limit. Output
reports examined/filtered/eligible counts, limit truncation and output omissions.

For every candidate, SQLite selects the highest known SemVer version satisfying
filters across all <=64 known versions. With msrv_lte, absent/non-canonical unstable
MSRV cannot prove compatibility and is excluded; without it, preserve the raw
nullable declared MSRV. Stable means no prerelease; yanked remains an independent
fact/filter. latest_known_stable is independent of selection filters and retains
its yanked bit. Expose selected version/license/MSRV/published time and snapshot
listed advisory IDs. An empty list never means secure or complete RustSec coverage.
Repository, description and version facts come only from SQLite, never the index.

Missing, disabled, invalid or incompatible semantic components, inference/index
errors trigger explicit lexical fallback with the same filters. A semantic candidate
with unknown identity, duplicate ID or invalid distance invalidates the semantic
channel. Cancellation/deadline is never converted to lexical success. SQLite errors
remain operational failures; no invented facts. Index metadata/catalog/model binding
and normalized query vectors are checked before semantic retrieval.

Status/search share the same retained CatalogProvider and joined Workers admission.
No extra generation, semaphore or runtime synchronization. All expensive work and
inference execute inside the joined blocking worker;120s cooperative deadline.
Complete CallToolResult cap512KiB includes structured and text duplication; trim
lowest-ranked results deterministically with explicit omissions, never crop facts.
If a metadata-only result cannot fit, return the ordinary output-limit failure.
Snapshot latest_known/freshness are reassessed at query time; model evidence and
index identity appear only when semantic retrieval contributed successfully.

Domain/application retain Serde-only pure types, comparisons/rank fusion and real
I/O ports. A catalog query port exposes scored lexical IDs and eligible SQLite
facts; the adapter uses its pinned semver implementation. No strategic dependencies,
schema migration or changes to the eleven existing MCP snapshots.

## Alternatives considered

- Existing lexical-first fill: suppresses semantic contribution when lexical is full.
- Semantic facts or equalizing raw scores: violates authority or misrepresents scales.
- Filtering only latest_known: incorrectly hides older compatible versions.
- Claiming complete filtered results from50 candidates: unsupported coverage.
- Implicit refresh/rebuild: changes runtime authority and reproducibility.

## Consequences

Search is reproducible over a bounded retained snapshot and transparently degrades.
Ranking/window quality, ES/EN behavior and resource performance still require M1-16
actual experiments; this algorithm is not evidence of usefulness. Inspect pagination
is a separate vertical. Product/source/distribution licenses and native host/client
qualification remain outstanding.

## Source verification

SQLite's official [FTS5 BM25/rank documentation](https://www.sqlite.org/fts5.html#the_bm25_function)
confirms lower-is-better and configurable rank; using explicit bm25 fixes the scale.
Lance8.0.0 is pinned in Cargo.lock and its cached official crate source
lance-linalg/src/distance/l2.rs sums squared differences; the existing actual
index test discriminates distances0/2 for orthogonal normalized vectors.
[Versioned crate](https://docs.rs/crate/lance-linalg/8.0.0).
