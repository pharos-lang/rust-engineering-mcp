# ADR-061 — Owner-bound private artifact store for quality jobs

## Status

Accepted 2026-09-06 by the M3 orchestrator after independent reviews V06/V17/V18
(ADR-063: owner provisioning authorization 2026-09-05). Stage 1 implementation
and native/runtime qualification are complete; see docs/validation/M3-matrix.md.

## Context

M3 quality jobs need bounded JUnit, reports and logs as private MCP Resources. M1's process-local `MemoryArtifactStore`, including its `rust-artifact://` URI, 256 KiB cap, one-hour retention and restart removal, remains byte-for-byte unchanged. Quality output is hostile: paths, MIME, XML/JSON/HTML/SVG, diffs and archive names are data, never authority; source and symbols can hold secrets. Bounded redaction/scanning reduce exposure but prove no secret absence.

The owner accepted M3's persistent-private-store posture. Enabling durable Stage 1 still requires the native qualification oracles below. M2's APFS state-root pattern is a precedent, but M2 journals are separate: this store never expires, evicts, migrates, scans, repairs or deletes them. Sibling namespaces share one volume, so capacity isolation must be explicit. This Proposed ADR does not authorize a tool/release, change the 18 existing contracts, or add a positive platform.

## Decision

### Boundary, layout, lock, and reuse

The fixed host-only sibling is:

```text
<state-root>/rust-mcp-quality-artifacts-v1/
  store.lock
  clock-watermark.json
  reservation/job_<32hex>.reserve
  blob/qart_<32hex>.blob
  descriptor/qart_<32hex>.json
  quarantine/
```

Names are generated canonical ASCII IDs: no guest filename/member, MIME, owner path, URI component, tool input or archive name becomes a host filename. Directories are 0700 and files 0600 under the stdio uid; blobs/transients are private regular files with link count one. This child and `rust-mcp-mutations-v1` never traverse or delete each other.

`store.lock` is M2-style exclusive non-blocking `flock`, held across admission, free-space check, reservation, publication, reconciliation and recovery inspection. Contention is a bounded busy rejection, never a wait or a second view of global quota. It coordinates only processes sharing the state root.

Extract a generic fixed-child helper `(parent, child_name)` from `crates/project-adapter/src/mutation_state.rs::prepare_mutation_state`; its M2 wrapper delegates unchanged. Add a `pub(crate)` private handle-relative primitive module: no-follow `openat`/`renameat`/`unlinkat`, private regular file/directory validation, `fsync`/`F_FULLFSYNC`, exact-capped streaming write and generic errors. Refactor both M2 `StateRoot` and the quality adapter onto it. Do not reuse `StateRoot::write_new`/`read_optional`: they are private, buffered, journal-phase-specific M2 APIs. Existing M2 mutation tests must pass unchanged and journal byte fixtures stay byte-identical.

Proposed paths are `crates/domain/src/quality_artifact.rs`, `crates/application/src/quality_artifact.rs`, and `crates/project-adapter/src/quality_artifact_store.rs` plus its private macOS module. `crates/artifact-adapter` remains the M1 memory adapter. Domain/application depend on neither rustix/APFS/Cargo/rmcp nor URI parsing.

Only macOS ARM64/APFS may qualify Stage 1. Linux and Windows return `UnsupportedPlatform` before reservation, gateway start, guest output or a filesystem fallback.

### Descriptor, job identity, and owner boundary

The v1 descriptor is strict `serde`/`schemars` Rust data in `quality_artifact.rs`, `#[serde(deny_unknown_fields)]`, committed only after its blob is complete:

```text
format_version: 1
artifact_id: qart_<128-bit random locator>
job_id: JobId = job_<128-bit random ADR-060 ID>
member_index: u16
kind: JunitXml | CoverageJson | Lcov | ArchiveBundle | MutationDiff | MutationLog | ToolLog | OtherDeclared
mime_type: closed enum
payload_format_version: closed per-kind enum
sha256, size_bytes, completeness, sensitivity
created_at_utc, expires_at_utc: observational RFC3339 instants
owner_binding: SHA-256 digest
source: ArtifactSource; runtime: ArtifactRuntime
```

There is no `qjob_` or `qjr_`; reservation filenames use ADR-060's `JobId` but are not authority. `ArtifactSource` is closed: captured-source fingerprint, closed selection and `GuestArtifactName` enum. `ArtifactRuntime` is closed: image digest, toolchain identity, closed plugin identity/version/digest and implementation digest. `GuestArtifactName` is an enum over the fixed-path table, never a guest string. `payload_format_version` is `JunitXmlV1`, `CoverageJsonV1`, `LcovV1`, `UstarV1`, `MutationDiffV1`, `Utf8LogV1`, or `DeclaredV1`; mismatched kind/version is invalid.

`owner_binding` is domain-separated SHA-256 of state-root device/inode, host uid, granted-root device/inode, and granted project workspace-root string. A state secret is deliberately dropped: same-uid access to the protected state root is already inside the local trust boundary, and a secret does not distinguish sessions or protect against that reader. Peer IDs, fingerprints, ProjectRef strings, artifact IDs and URI text are excluded.

Every read resolves the URI ProjectRef through the live registry/current host grant, derives the binding from the physical granted root and compares it. The accepted boundary is **uid + state root + host-granted root**, not peer/session isolation: a later stdio session with the same uid/state root/live grant for the same root may read retained evidence; a different uid, state root or granted root cannot. `SECURITY.md` must state this before Stage 1. A retained `qart_` locator plus a fresh live ProjectRef for that root is the re-access path; clients may construct canonical URI. Locator is not credential; revalidation is the gate. A manifest edit does not itself alter root binding, but grant/reference must remain live.

Malformed/unknown/expired/revoked/mismatched cases return the same `Resource not found` status/error variant. No resource list, count or descriptor enumeration crosses this boundary. No constant-time promise is made; 128-bit unguessable IDs reduce timing as an oracle, while status/error-variant/count/enumeration remain indistinguishable. Quality reads never call `reap_artifacts`/`retain_owners` and never renew the ProjectRef idle lease.

### Stages, quotas, and L11 divergence

- **Stage 0:** absent state root or unavailable durable store returns bounded JUnit/log evidence through unchanged M1 memory artifacts. The M3 result sets `completeness: Truncated` and an omission flag if a report cannot fit; it claims no persistence and changes no M1 schema/retention.
- **Stage 1:** with configured state root and qualified native store, durable store is default. Neither stage adds a daemon, broker, account, collector, network or installation path.

All values are **proposals pending fixture measurement**; TTL maximum is explicitly deferred for recalibration. Smaller existing limits win.

| Unit | Phase | Default | Maximum | Exceed behaviour | Oracle |
| --- | --- | ---: | ---: | --- | --- |
| bytes/artifact | admission/egress | 32 MiB | 32 MiB | reject before gateway; no descriptor | exact cap/flood |
| bytes/job | reservation | 64 MiB | 64 MiB | reject before gateway | aggregation bound |
| stored members/job | admission | 128 | 128 | reject job | 128/129 fixture |
| retained+reserved bytes/owner | admission | 128 MiB | 128 MiB | reject; no eviction | owner quota |
| retained+reserved bytes/global | admission | 256 MiB | 256 MiB | reject; no eviction | two-process quota |
| QUALITY_CONTROL_HEADROOM | fstatfs | 16 MiB | 16 MiB | reject reservation | maximal reservation |
| artifact TTL | publish/read | 1 h | measurement deferred | expire/no renewal | before/after expiry |
| raw Resource chunk | serialization | 320 KiB | 320 KiB | reject response | base64 cap |
| index members/page | Resource index | 64 | 64 | cursor/reject | 65-member page |
| cursor bytes | URI parse | 128 B | 128 B | not found | 128/129 cursor |

This deliberately diverges from L11: eviction is rejected. Saturation means reject-before-produce, never deleting promised live evidence. TTL only reclaims expired known bytes.

### State-root capacity and honest reservation

Before reserving, under `store.lock`, call `fstatfs` on the validated state-root handle and require:

```text
free_bytes >= requested_reservation + M2_RECOVERY_HEADROOM + QUALITY_CONTROL_HEADROOM
M2_RECOVERY_HEADROOM = 49 MiB = 48 MiB recovery staging + 1 MiB metadata/growth
QUALITY_CONTROL_HEADROOM = 16 MiB (proposed)
```

The shared domain constant `M2_RECOVERY_HEADROOM_BYTES` is consumed directly by
the M2 store and has a compile-time equality assertion against its 48 MiB staging
+ 1 MiB metadata derivation. A maximal quality reservation must leave it available
and an M2 commit must still succeed afterward.

After this check use pinned rustix `fallocate` best-effort on each reservation file; do not add `libc`. This is not a hard APFS guarantee: snapshots, purgeable space or other writers can consume space and rustix exposes no reliable verification result. Logical accounting, RSS, apparent free space, sparse files and Docker volumes alone are insufficient. ENOSPC/short write at every stream write, sync, rename, descriptor write or publication recheck fails closed: no descriptor, release only known reservation. If output is smaller, truncate it before descriptor commit, but retain declared quota until descriptor rename **and** descriptor-dir fsync; only then release surplus, so truncation cannot weaken publication protection.

### Time, publication, recovery, migration

In-session deadlines are monotonic. Durable wall clock/RFC3339 only limit expiry across restart. In session, expiry is the earlier monotonic or wall-clock deadline: wall clock can shorten, never lengthen, TTL. A value earlier than durable watermark is a quality-store-only clock regression and fails closed.

The reservation file is the only partial destination. Egress exact-caps/hashes it,
revalidates private regular identity, syncs and renames to blob; then strict temp
descriptor is synced/revalidated, renamed into `descriptor/`, and that directory
synced. The final descriptor is commit marker. A per-member failure publishes no
descriptor for that member, marks its omission, and continues the bounded batch;
already committed descriptors are returned. Finalization releases the remaining
claim, and a failed release invokes reconciliation before any locator is returned.

Reconciliation trusts known v1 descriptor/blob pairs only when strict schema, binding/timestamps, private identities/link counts, size and SHA-256 validate. It may discard only recognizably named known-private uncommitted reservation/temp and release matching reservation. Unknown version/name, malformed descriptor, link, ownership/mode change, hash mismatch, ambiguity or clock anomaly quarantines, not guesses/overwrites/unlinks/serves. The block affects only quality artifacts: M1 Resources, M2 commit/receipt and the existing non-quality tools continue.

M3-01 implements host-operator CLI stubs `rust-engineering-mcp quality-artifacts recover --state-root PATH` and `quality-artifacts prune --state-root PATH`. The local operator with that state root may delete only validated quarantined/expired **quality** objects, never M2 state, and never repair unknown bytes. No daemon/automatic remediation results.

Format bumps use new siblings (`rust-mcp-quality-artifacts-v2`), never in-place reinterpretation. Migration validates source, reserves side-by-side space, verifies new bytes then publishes versioned manifest. Unknown version fails closed. Rollback/uninstall preserves directories and rejects incompatible readers; it never deletes another owner's objects or M2 state.

### Resources, fixed egress, and ArchiveBundle

M1 URI/MIME/cap/TTL/restart/error semantics stay unchanged. M3 canonical URIs:

```text
rust-quality-artifact://prj_<32hex>/job_<32hex>?cursor=<1..128 opaque bytes>
rust-quality-artifact://prj_<32hex>/qart_<32hex>?offset=<decimal>&length=<decimal>
```

Job index has at most 64 descriptor rows; member URI returns blob chunk. Query order, decimal grammar and cursor/limit bounds are closed; unknown/duplicate/fragment/escaping/overlong forms are not found. `resources/list` does not enumerate quality objects; reads never start work, egress, repair or TTL renewal.

`length <= 320 KiB = 327,680 raw bytes`; base64 is `4 * ceil(327,680 / 3) = 436,908` bytes, leaving 87,380 below the 512 KiB complete response cap. Serialize complete typed response and reject over 512 KiB. Responses are private/no-cache, descriptor-derived, and have no host path.

`GuestArtifactEgress` is a typed existing-channel port, not shell/archive extractor/guest-selected host write. Its `GuestArtifactName` enum maps a single-file kind only to exact fixed guest paths: JUnit XML, coverage JSON, LCOV, outcomes JSON, diff text or log. It accepts regular files only and rejects links/devices/FIFOs/sockets.

Multi-file coverage HTML/SVG and mutants reports are one `ArchiveBundle`. A fixed
approved guest program emits a bounded USTAR stream. The shared closed validator in
`mutation_outcomes.rs` enforces total size and entry count, empty USTAR prefix and
linkname, guest uid/gid, regular files/directories only, and rejects links, devices,
extensions and `..`. The archive is stored and counted as one descriptor/member;
its internal entries remain independently bounded and are never addressable Resource
members. It never extracts to host paths and is never previewed.

### Content, sensitivity, observability

Parsers have independent byte/token/depth/line/field caps and report Partial, Invalid or Unavailable, never false clean. JUnit streaming rejects DTD/entity/external-entity; JSON streams bounded tokens/depth (no unbounded DOM); LCOV is bounded line parsing with opaque source paths. HTML/SVG is octet-stream attachment; optional preview is inert bounded summary rejecting scripts/event handlers/`javascript:`/all remote URLs, else no preview. Diff/log preview escapes hostile text and never follows/applies embedded path/link.

Operational retention uses ordinary host quality grant. PotentiallySensitive needs explicit retention grant. SourceDerived/SymbolDerived retention and every export beyond private Resource need additional host permission. `SecretSuspected` refuses retention unless host grants PotentiallySensitive retention; no scan match proves nothing. Peer/client/tool input cannot relax permissions.

Emit bounded tracing/stderr for admission/lock/reservation/egress/publication/read/expiry/reconciliation/uncertain cleanup: closed reason/status, valid opaque ID, counts/bytes/duration only; never paths/URI/source/bytes/text/secrets/peer inputs. No collector, daemon, telemetry or durable audit log.

### M3-01 test and oracle list

Skip, fallback or unmeasured budget is not pass.

| Case | Negative oracle | Positive control |
| --- | --- | --- |
| XML billion laughs | reject DTD/entity before expansion; no descriptor | 2-pass/1-fail JUnit exact |
| Deep JSON | Invalid/Partial, no stack exhaustion | below-limit stable metric/hash |
| External URI HTML | never fetch, no preview | static inert summary |
| HTML script | no execution/active preview | benign octet-stream only |
| Output flood | stop/no descriptor/accounting preserved | exact cap hash/size |
| ENOSPC mid-stream | no descriptor; release known reservation | later within-budget reservation works |
| Two different roots | same not-found/no index leak | each root reads own |
| Same root, two sessions | fresh reference reads same locator; different root not found | proves accepted boundary |
| TTL | expired unreadable/reclaim only known bytes | pre-expiry no lease/TTL renewal |
| Quota/eviction | reject before gateway/displace nothing | exact budget publishes |
| Two processes | contender busy/no double accounting | later admission after unlock |
| M2 headroom | max quality retains 49 MiB; M2 commit succeeds | capacity coupling proven |
| M2 regression | all mutation tests/journal bytes unchanged | shared primitive identical bytes |
| M1 regression | URI/cap/TTL/restart byte-identical | Stage 0 only M3 omission metadata |
| Guest symlink | reject link/device/FIFO/dir without traversal | regular fixed path streams |
| ArchiveBundle | reject link/`..`/oversize/nonregular without extraction | bounded regular tar canonicalizes |
| Crash blob to descriptor | no blob served | full pair reads exact hash |
| Crash descriptor to dir fsync | quarantine unless durable completion proven | fully synced pair survives |
| Restart/clock | corruption/version/hash/regression blocks quality only | valid v1 + fresh ref reads |
| Platform | Linux/Windows UnsupportedPlatform before reserve/guest/output | macOS ARM64/APFS reaches Stage 1 |

## Alternatives considered

- Enlarging/persisting M1 changes frozen Resource semantics.
- RSS/free-space alone, sparse files or Docker claims cannot protect M2 headroom.
- Eviction can remove promised evidence; reject-before-produce is chosen.
- Guest-selected archives/paths make hostile path interpretation authority.
- Broker/daemon/collector/database/new account violates ADR-050 local coordination.
- Session-bound persistence makes post-restart artifacts unreachable; owner accepted uid + state-root + root boundary.
- In-place migration/uninstall cleanup risks data loss and false compatibility.

## Consequences

M3 obtains versioned durable evidence with M1 compatibility, M2 capacity protection, honest APFS reservation, closed ArchiveBundle ingress and fail-closed recovery. It adds native qualification, disk use, permissions, recovery CLI and fault tests; it does not itself implement a tool/protocol/parser/host option/release/platform/network. Stage 1 documentation must state same-uid/state-root/granted-root re-access, retention, CLI and uninstall bytes; Stage 0 keeps M3-01 useful pre-qualification.

## Open issues

- State secret is intentionally dropped: same-uid protected-state access is already the boundary and secret did not separate sessions; document this in SECURITY.
- `SecretSuspected` is decided: refuse retention without PotentiallySensitive grant; scanning remains bounded and non-guaranteed.
- Closed `ArtifactSource`/`ArtifactRuntime` field shapes are proposed above and must be finalized in M3-01 DTO/schema design; no free-form strings.
- TTL maximum is measurement-deferred; fixtures must set/reaffirm host cap, not infer it from ProjectRef or ADR-060 retention.

## Sources

- `AGENTS.md`; M3 roadmap; D17; traceability C11/L11.
- ADR-014/028/031/050/052/053/054/058/059/060/062.
- M1 artifact/application/MCP resource modules.

## Implementation notes

The M2 `StateRoot` refactor was deferred; the shared primitive serves the quality
store. The implementation exposes `UnsupportedStateRoot`, writes the `.trunc`
marker before publication, uses the 49-byte cursor grammar, and is gated to
`macOS+aarch64`.
- `mutation_state.rs`, macOS `mutation.rs`, and `mutation_archive.rs`.
- Security model, client configuration, tools, architecture and M2-07 evidence.
