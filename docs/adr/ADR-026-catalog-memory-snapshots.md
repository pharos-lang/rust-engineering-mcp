# ADR-026 — Bounded catalog snapshots in memory

## Context

M0 needs a real SQLite repository and migrations before M1 exposes import/sync and
catalog tools. Opening SQLite by a caller path would bypass no-follow host I/O.
The foundation must not invent snapshot signing trust roots or accept arbitrary SQL.

## Decision

Pin rusqlite 0.40.2 with bundled SQLite 3.53.2, serialize, limits, hooks and a 16-statement cache.
Build normalized schema v1 and deterministic FTS5 in a bounded in-memory staging
connection. Migrations commit SQL, checksum ledger and user_version together.
Export a database image plus a versioned manifest (sequence, SHA-256, exact size).
Import only an already-owned byte slice under a trusted expected manifest, verify
size/hash before safe deserialize_read_exact, reject WAL images, validate integrity,
foreign keys, exact schema/ledger and facts. Snapshot signing/filesystem acquisition
remain M1-10; checksums here prove integrity relative to host expectation, not publisher
authenticity. Runtime opens the validated image READONLY with temp_store=MEMORY,
ATTACH disabled, defensive/trusted-schema controls, SQLite limits and fixed queries.

CatalogRepository is an application port consumed by an internal catalog use case;
it returns facts from SQLite only. Results always carry snapshot identity and
provenance/freshness reassessed with an injected clock. FTS queries treat caller
terms as quoted literals and impose query/result budgets. Activation replaces the
complete repository only after staging succeeds and a strictly newer sequence is
checked. An exclusive mutable reference serializes activation and queries, so old
in-memory generations cannot accumulate via retained readers. Async MCP callers in
M1 must use one bounded blocking worker; the foundation synchronous API does no
reactor scheduling and exposes no new MCP tool.

Initial budget: 64 MiB image, 1,000 crates, 64 versions per crate, 256-byte query,
16 terms and 50 summaries. All dimension limits also obey a combined maximum
of 100,000 version/feature/dependency/advisory entries, so the product of individual
maxima is not promised as usable capacity. Search projects name/description, a
SemVer-selected latest_known (which can be yanked/prerelease), and version count;
it caps the serialized candidate payload at 128 KiB. Full details remain inspect.
Feature names are Cargo keys, not dependency activation expressions. Inspect uses
exact canonical names; FTS case folding is lexical retrieval, not identity. This bounds inputs, not total native allocator RAM. SQLite
limits/progress interruption (10 million opcodes or 30 seconds per operation) bound work; hard OS resource isolation is not claimed.

## Alternatives considered

- Canonicalize then SQLite::open(path): violates the filesystem race boundary.
- Own SQLite VFS/unsafe allocator bridge: unnecessary complexity; rusqlite supplies
  safe owned-byte deserialization in the pinned version.
- Signed public bundles now: distribution/trust roots remain explicitly undecided.
- Unrestricted schemas/import SQL: expands attack surface and invalidates facts.

## Consequences

SQLite serialization is the M0 snapshot payload format. Durable host storage, signed
archive transport, rollback administration and CLI integration are still M1-10.
The image can be rebuilt offline from source records; FTS is derived. One active
in-memory snapshot trades bounded catalog size for simpler confinement. Larger
production catalogs require a reviewed storage/budget change before release.

## Status

Accepted for M0-08, subject to the implementation gate.

Sources: https://github.com/rusqlite/rusqlite/blob/v0.40.2/src/serialize.rs,
https://www.sqlite.org/c3ref/deserialize.html, https://www.sqlite.org/security.html,
https://www.sqlite.org/fts5.html#the_integrity_check_command.

Review refinement: format_version is also stored in the snapshot row; schema changes
including provenance representation require a new user_version/migration. Page limits
are reapplied/verified after deserialize and image length must equal page_count times
page_size. Aggregate row counts are checked before hydration. Empty catalogs are
valid; missing metadata is not. Antirollback is per active instance; durable state is
M1. Activation can transiently retain caller bytes, old database and staging/runtime
copies, plus decoded records: the image cap must not be interpreted as peak RAM.
