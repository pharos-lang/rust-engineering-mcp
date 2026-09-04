# ADR-038 — Owned RustSec snapshots and bounded dependency audit

## Status
Accepted for implementation2026-09-04; M1-07 requires current gates.

## Context
Spec23.8 permits cargo-audit or equivalent RustSec library;34.6 requires RustSec
snapshot and SQLite correlation. Existing SQLite only stores advisory IDs, which
cannot establish vulnerable ranges. The approved image has no cargo-audit. Runtime
must neither refresh nor acquire advisories, and Database::open(path) would bypass
handle-relative no-follow I/O. RustSec0.32.0 has no public owned-byte Database builder.

## Decision
Pin rustsec0.32.0 without default/git/HTTP/binary-scanning features in catalog adapter.
Use Advisory::from_str and the official Query matcher over records selected from
bounded authoritative in-memory SQLite. No RustSec path APIs, network or child
processes. Capture Cargo.lock from the same immutable SourceBundle passed to metadata; obtain workspace roots
through existing frozen metadata via the single calibrated Execution Gateway.
Application combines two ports; domain/application contain no Cargo, RustSec or SQL.
All work is in the existing joined bounded worker; cancellation checked between
records, packages and graph steps; final ProjectRef revalidation before publication.

Trusted host explicitly supplies a snapshot file and its expected SHA-256 through
closed serve flags. Snapshot v1 contains sequence, source identity, source-created
and source-observed times (nullable), and path/Markdown records. The safe filesystem
adapter reads relative to no-follow APFS directory handles with regular-file/single-
link/stable-stamp checks. Bound bytes before parsing/hash verification. No default
home-directory lookup, Git fetch, refresh, install or arbitrary paths from MCP.
Checksum verifies host-expected integrity, never publisher authenticity. Missing or
stale/unknown-age snapshot cannot produce a clean passed audit. Freshness is assessed
at request time, never reset merely because a file was copied or loaded.

Validate each crates/<package>/<RUSTSEC-id>.md against parsed package/id/collection;
fill an absent collection with Crates only after exact path validation, and reject
contradictory collections, mismatches, duplicates, placeholders and unsupported
source identities. SQLite selection plus official matching is the correlation;
do not re-check IDs derived from the same record as independent corroboration.
SQLite retains exact validated advisory facts with fixed schema/queries; RustSec
provides affected-version matching, patched/unaffected requirements and CVSS severity.
Withdrawn records cannot report active vulnerabilities; informational records are
reported separately. Match only explicit crates.io package sources as such; path,
Git and other registries never become crates.io through absent source metadata.
Expose skipped-source coverage and avoid asserting complete vulnerability coverage
for unsupported resolved sources. Cargo.lock is resolved-state evidence, not a claim
that every listed optional/target dependency was built. No network feature resolution.

Return advisory identity, package/version, patched/unaffected requirements, optional
severity, workspace root and bounded relevant paths through the lock graph. Resolve
edges with full package identity including source, rejecting ambiguous/inconsistent
locks. Each workspace member must match exactly one local lock node; otherwise
block. Metadata --no-deps establishes roots, not general manifest/lock resolution
synchronization. Report only the captured lock generation and its fingerprint.
No recursive unbounded tree rendering. Size/record/package/edge/path/matcher
budgets and response omission make incompleteness explicit; incomplete never passes.
No-finding fresh complete scans pass; findings fail; missing data unavailable;
invalid lock/snapshot or exceeded safety budget blocked; cancellation suppresses
responses under existing MCP policy. Retain project and RustSec snapshot evidence
with latest_known semantics. Signed distribution, durable antirollback, CLI import/
sync and global catalog lifecycle remain M1-10, not claimed implemented here.

## Alternatives considered
Installing cargo-audit into the approved image creates a new runtime provisioning
boundary and is unnecessary for a pure-data operation. RustSec Database::open would
reopen paths outside the capability adapter. Handwritten vulnerable-range evaluation
risks diverging from RustSec semantics. IDs alone cannot prove vulnerability facts.
Treating local Git metadata or a content checksum as publisher authentication is false.

## Consequences
ADR-009 restricted effects remain for the metadata child: isolated network/env/fs,
strong cleanup and budgets. RustSec matching itself is in-process owned-data work,
like SQLite, and does not claim OS isolation of the whole MCP process. This does not
weaken network deny on any external operation. Native platform support is unchanged.
Dependency acquisition is an explicit development action from allowed crates.io;
lock/checksums/features and offline validation are recorded, never runtime provisioning.
Local snapshots remain bounded and host-attested; full global catalog import and
signed distribution are not part of this vertical.

## Sources
https://raw.githubusercontent.com/RustSec/rustsec/rustsec/v0.32.0/rustsec/src/advisory.rs
https://raw.githubusercontent.com/RustSec/rustsec/rustsec/v0.32.0/rustsec/src/database/query.rs
https://raw.githubusercontent.com/RustSec/rustsec/rustsec/v0.32.0/rustsec/src/database/entries.rs
https://raw.githubusercontent.com/RustSec/rustsec/rustsec/v0.32.0/rustsec/Cargo.toml

Implementation bounds: snapshot8MiB/2048records/64KiB Markdown and128records per
package; SQLite16MiB, fixed prepared selects, query_only after staging. Lock v4 only,
1MiB/1024packages/8192edges; raw typed unique resolution precedes cargo-lock11.0.1,
whose abbreviation resolver otherwise chooses a first match. Cross-check full
kind/URL/precise identity; every node reachable from a workspace root. Reject
legacy metadata/replace/unused patches and ambiguous or orphan nodes. This is an
explicit conservative captured-lock subset, not general Cargo resolution validation.
Paths are one shortest representative per reachable workspace root, at most8 roots
and32packages/path, using bounded iterative BFS even through cycles. Omission
counts reachable roots not retained; alternative routes are not enumerated.
At most128 findings/informational entries and256KiB adapter payload; omissions or
unsupported non-workspace sources make coverage incomplete. Freshness policy is
fresh24h/aging through7days/stale later; only Fresh with both known, nonfuture
source timestamps permits clean passed. Stale/unknown overall Unavailable retains
historical findings. Informational records alone do not fail a complete scan.
Sequence is validated transport metadata, not rollback enforcement. Expected hash
is fixed by host configuration for each process; no runtime activation or refresh.
Development pins cargo-lock11.0.1 to the audited API, with no dependency-tree feature;
the bounded source-aware graph is intentionally narrower than its renderer.

Review refinements: reject zero-record documents; expose snapshot_record_count and
snapshot_sequence alongside the fingerprint. A positive count describes the host-
selected dataset, not completeness against the publisher's global collection.
Compare registry kind/URL/precise explicitly: cargo-lock marks parsed registry URLs
as "locked", whereas absent advisory source uses the canonical default. Reject
non-ASCII package names and control/bidirectional-format characters in display
metadata; informational labels are bounded to128bytes. Cache paths once per package
and stop path construction when the128-finding retention budget is exhausted.

The configured snapshot's parent directory is a separate trusted host read authority,
not a ProjectRef root; the exact leaf is read through that handle. Existing capability
policy rejects filesystem-root authority (for example /snapshot.json). Missing files
remain unavailable; containment denial, deadline, budget, unsupported OS and internal
errors retain their distinct classifications. Snapshot bytes are re-read/revalidated
and SQLite rebuilt per request under bounded worker limits; there is no retained
activation/cache or implicit refresh. Project capture/metadata precedes advisory
availability assessment, including an unconfigured snapshot; this preserves live
project authorization but may spend a bounded metadata job before Unavailable.

A usable fresh snapshot with known vulnerabilities is Failed even if coverage is
incomplete; stale/unknown snapshots remain Unavailable with historical findings.
The issue field is the first blocking reason; explicit coverage and omission fields
retain additional limitations. The adapter256KiB hard cap and MCP512KiB envelope
cap are independent defenses; oversized base metadata may block without partial data.

Noncanonical explicit advisory origins reject the entire document, rather than
silently skipping records under a complete scan. Explicit canonical advisory parsing
is regression-tested for the locked marker. Byte-budget exhaustion stops further
path construction; all additional matches are counted as omitted.
