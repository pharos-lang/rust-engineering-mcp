# ADR-044 — Paged authoritative crate inspection

## Status

Accepted for implementation,2026-09-04, after M1-12 integration. Tool13 completes
the M1 tool inventory; no milestone closure or M2 scope change.

## Context

Specification34.13 requires known versions, stable/yanked/MSRV, features,
dependencies, license, repository, documentation, advisories, source and evidence.
Schema1 contains scalar crate/version facts plus bounded normalized collections;
it does not record documentation URLs, package source URLs/checksums, feature
expansions or full dependency declarations. The old internal inspect hydrates all
64 versions and their128-element collections; public retrieval needs pagination.

## Decision

Add only rust.crate.inspect with closed input: name(ASCII alnum/_/-,1..64),
section(default overview: overview/versions/features/dependencies/advisories),
version(optional exact SemVer <=128 bytes), limit(default20,1..50), offset(default0,
0..128), snapshot_fingerprint(optional sha256 identity, required for offset>0).
Version is required for features/dependencies/advisories, forbidden for versions,
optional for overview. Overview requires offset0. Version syntax is validated with
the already pinned semver1.0.28 in the SQLite adapter before querying. The flat
continuation repeats name/section/version and advances offset, retaining fingerprint;
all values remain explicit caller-selected query parameters, not authorization.
Changing generation returns snapshot_mismatch before page facts are read. A page
request with an altered name/section/version is a new explicit query over the same
snapshot, not a forged capability. No cursor signature or server-side state needed.

Every successful response carries exact snapshot fingerprint, sequence, provenance
and independently assessed freshness with latest_known semantics. A found page
includes crate scalar overview (description, declared repository, updated_at,
version_count, independent latest_known_stable) and section data. Stable is highest
known SemVer without prerelease, independently preserving yanked; absent stable is
null. Overview optionally includes exact selected version scalars. Version pages
include version/yanked/MSRV/license/published time and bounded collection counts.
Collections require exact version and expose only schema1 facts: feature names,
dependency name/requirement/kind/optional, and listed advisory IDs. Empty advisories
never assert safety or complete RustSec coverage. Repository is unverified declared
text. documentation and source are explicitly unknown/not_recorded_in_snapshot;
package source is not conflated with catalog provenance. Missing crate, missing
version, empty collection, unavailable catalog and snapshot mismatch are distinct.

SQLite provides bounded scalar/page queries through a real application port; no
full CrateRecord graph hydration. At most65 scalar versions detect the64 bound;
SemVer descending order is deterministic. Feature/advisory names sort ascending;
dependencies sort by name then kind. Count/offset/returned/next_offset are explicit;
invalid offset beyond total is invalid input, offset==total is a valid empty page.
Collections remain <=128 entries. No schema migration or lock/dependency change.

Use the existing shared CatalogProvider generation and joined worker. No model or
index availability is required to inspect SQLite. No downloads, rebuilds, project
execution or new host flags. All I/O, fact validation, schema validation and full
MCP encoding stay inside admission until completion, with120s cooperative deadline.
Complete CallToolResult is capped512KiB, including text/structured duplication.
If needed, trim whole trailing collection entries and recalculate next_offset from
actually emitted entries; never crop facts, skip records or emit non-progressing
continuation. An irreducible oversized page is an output-limit operational failure.
Cancellation/deadline cannot publish stale success.

## Alternatives considered

- Full nested record: wastes tokens and may exceed the complete MCP budget.
- Invent docs.rs/registry URLs: promotes convention to unsupported snapshot fact.
- Add schema2 solely for empty new fields: changes distribution without new data.
- Opaque signed cursors: unnecessary for explicit bounded reads without authority.
- Implicit latest version for collections: could mix different version identities.

## Consequences

M1 inspection exposes all recorded decision facts truthfully and makes absent schema
coverage explicit. Clients paginate individual collections by exact version and
snapshot identity. Future richer package metadata needs a separate persistence and
public-contract decision. This does not establish cross-platform/client, license,
distribution or utility-experiment qualification.
