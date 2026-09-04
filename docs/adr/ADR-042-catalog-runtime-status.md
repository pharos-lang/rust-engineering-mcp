# ADR-042 — Read-only catalog runtime and component status

## Status

Accepted for implementation,2026-09-04; M1-11 remains in progress until gate/review.
M1-10 is integrated; M1-12/13 query contracts remain separate vertical decisions.

## Context

Catalog administration is authenticated and durable, but runtime still has ten tools.
The status tool must describe verified context, not merely existing filenames, and
must preserve ADR-030 admission while loading SQLite/E5/native objects. Existing
dependency audit has a separate explicit host snapshot source that must not silently
change merely because a catalog bundle includes RustSec bytes.

## Decision

Add only rust.catalog.status in this vertical, empty closed input and the existing
structured output envelope. Component state is either verified available with typed
identity/evidence or unavailable with a fixed reason. Successful availability
inspection may report unavailable components; it does not claim usable facts.
Snapshots retain source provenance and are reassessed using the current clock.

The host supplies --catalog-store and --catalog-trust together, optionally
--catalog-model-dir and --catalog-index-store (external index requires model).
Tools never accept filesystem paths or refresh/download options. Lazy load only
after protocol bootstrap in the shared joined blocking worker. Retain one immutable
catalog/model/index generation per session, including unavailable initial load;
restart to observe admin imports/rebuilds. Cancellation/deadline signals never
release admission while native work remains, and late success is discarded.

Runtime uses protected read-only no-follow owned readers, never CatalogStore::open
or its lock/staging cleanup. Read floor/active/floor with bounded consistency retry;
share the exact typed sequence-floor validator with CLI. An active older than a
reserved floor is an explicit pending reservation, not a silent rollback. Floor
identity remains observable when active data is missing. No unverified payload is
parsed as SQLite/native data; local derived bytes require protected host ownership.
All existing trusted-state/ACL/platform limits of ADR-041 continue to apply.

SQLite is authoritative. Model availability requires fixed E5 hash validation and
successful local provider initialization; index availability requires native restore,
exact model/catalog identity and complete crate-name coverage. Missing or invalid
semantics leave valid SQLite available. The provider retains native instances for
later query verticals instead of discarding validated handles.

RustSec status observes the existing --rustsec-snapshot/hash source used by audit,
through its existing owned acquisition/parser, separately on each status call.
Bundled RustSec presence is separately labeled as a catalog payload; it does not
silently become the audit source. Identity, sequence, record count and freshness
are reported for the actual configured audit snapshot. Existing ten tools retain
all contracts and source-selection behavior.

Network reports acquisition_allowed=false and enforcement=runtime_api_disabled:
this describes the runtime's absent acquisition authority, not an unmeasured OS
network sandbox for the entire server. Tests under enforced OS network deny retain
positive controls. CLI HTTPS remains a separate explicit operation.

Status has a120-second cooperative joined deadline and a128KiB complete encoded
MCP result budget, with ordinary bootstrap/admission/cancel/timeout errors. Domain
and application retain only Serde/pure types and effect ports; MCP schemas remain
in the adapter with contract snapshots. No generic JSON internal model is added.

## Alternatives considered

- Reusing the admin lease: status would mutate staging and block future imports.
- File-presence checks: falsely claim valid model/index/advisory context.
- Native work on Tokio reactor or detached capacity: violates admission/cancel bounds.
- Silent runtime refresh or implicit bundled audit source: changes authority and facts.
- Declaring OS network isolation from absent HTTP calls: unsupported enforcement claim.

## Consequences

One more tool is advertised, eleven total. Cold status can be expensive when the
host opts into local semantics; instances are retained after validation. Performance,
query ranking, inspect pagination, client qualification and cross-platform evidence
remain separate requirements. Status can expose a pending reservation or explicit
component failure without downloading, modifying the store or granting project I/O.

Freshness uses the existing M1 age classification (one day fresh, seven days stale)
for each source independently. Classification never expires a validated pinned
model. Index evidence is its exact catalog fingerprint and full model identity;
those source components provide the assessed provenance/freshness. Source provenance
may record network acquisition by its publisher; `acquisition_allowed=false` describes
this runtime authority, not how the publisher originally obtained its snapshot.
RustSec preserves its existing positive-u64 sequence contract and 1..=2048 parser
record bound; catalog floor sequences separately fit SQLite signed integers.
