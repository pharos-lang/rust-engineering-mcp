# M1-07 external review

Claude Code2.1.259; explicit claude-opus-5, High, read-only/safe/restricted/no tools.
Actual reviewer model verified in modelUsage; findings evaluated by principal.

# M1-07 Review — read-only, no edits or execution performed

Scope reviewed: the 8 bounded files plus the focused host/config diff. No P0 found: **the key containment property holds** — `Input` carries only `project_ref`, the snapshot path comes exclusively from `HostAuditConfig` populated by `--rustsec-snapshot`, and no MCP-supplied string reaches `read_host_snapshot`. The lockfile confirms `rustsec 0.32.0`'s tree is `cargo-lock, cvss, fs-err, platforms, semver, serde, thiserror, toml, url` — no HTTP/git transitive, consistent with the ADR claim.

## P1

**1. No lower bound on advisory coverage: an empty (or truncated) but hash-matching snapshot yields `Passed`.** *Confirmed.*
`RustSecSnapshot::from_bytes` (`crates/catalog-adapter/src/audit.rs`) checks only `document.records.len() > MAX_RECORDS`; there is no minimum, and `records: []` builds a valid, integrity-`Verified`, fresh snapshot. `audit()` then scans, finds nothing, and returns `state: Passed` with summary *"Captured lockfile has no known vulnerabilities in the fresh verified RustSec snapshot"*. `AuditObservation` exposes no advisory-record count, so no client or operator can distinguish a 700-advisory snapshot from a 0-advisory one — `snapshot_fingerprint` only proves the file matches the flag the operator typed. Realistic trigger: a generation pipeline that writes zero/partial records and whose SHA-256 is computed from that same truncated file. Suggest a record count in the observation (and/or a configured minimum) so "no findings" is distinguishable from "no data".

**2. crates.io classification uses `SourceId`'s relaxed `PartialEq`, not the module's own `same_source`.** *Confirmed inconsistency; exploitability unverified.*
`crates/catalog-adapter/src/audit/lock.rs` introduces `same_source` with the explicit comment *"SourceId's Ord/Eq deliberately relax Git reference/revision comparison. Audit identities must retain those facts rather than inherit first-match loss"*, and uses it for identity, dedup and the library cross-check. The single most security-relevant decision then uses derived equality:
```rust
} else if identity.source.as_ref().is_some_and(|s| s == &canonical) {
    AuditSource::CratesIo
```
If `cargo_lock::SourceId`'s `PartialEq` ignores `precise` for non-git kinds (as cargo's own `SourceIdInner` does), a captured lock entry such as `registry+https://github.com/rust-lang/crates.io-index#<precise>` is classified `CratesIo`, counted in `crates_io_scanned`, and removed from `unsupported_packages` — converting an honest `Incomplete` into a `Passed` while asserting a provenance the lock did not state. The existing tests cover credential/query/kind spoofs but not a `precise` fragment on a registry source. One-line fix (`same_source(s, &canonical)`); I could not confirm cargo-lock's `PartialEq` body with tools disabled.

**3. Host-snapshot I/O errors are collapsed into `AuditSnapshotInvalid`, discarding security- and deadline-relevant attribution.** *Confirmed.*
`crates/mcp-server/src/stdio/auditing/provider.rs`:
```rust
ProjectError::Cancelled => AuditDataError::Cancelled,
_ => AuditDataError::InvalidSnapshot,
```
So `Rejected(SandboxDenied)` (symlinked snapshot path, ancestor swap, hardlink — exactly the conditions `host_snapshot.rs` tests as security rejections), `Rejected(ProjectNotFound)` (missing file), `Rejected(CommandTimeout)`, `Rejected(OutputLimitExceeded)` and `ProjectError::Internal` all surface as `Blocked / AUDIT_SNAPSHOT_INVALID / "RustSec snapshot is invalid or inaccessible"`. Dedicated codes exist and are unreachable from this path (`AuditSnapshotUnavailable`, `SandboxDenied`, `CommandTimeout`, `OutputLimitExceeded`). Compounding it, `joined_result` only re-raises `joined.interrupted` for `*Cancelled` results, so a deadline breach during the snapshot read returns `InvalidSnapshot` and the `WorkerError::TimedOut` signal is dropped. Fails closed, but it misattributes host/sandbox faults to the operator's data.

## P2

4. **`observation.state` and the tool `status` implement opposite precedence.** `audit()` in `catalog-adapter/src/audit.rs` orders `Failed` before `Incomplete`; `classify()` in `auditing.rs` orders `Incomplete` (any `issue.is_some()`, unsupported packages, or omission) before `Failed`, while `output()` still emits `Outcome::Failed` when `!findings.is_empty()`. Result: unsupported sources + a real advisory ⇒ wire `status: "failed"` with `data.observation.state: "incomplete"`. Same class of mismatch in the integrity-failed arm, which retains data whose `issue` was set to `SnapshotUnavailable` by `classify` while `error_code` is `AUDIT_INTEGRITY_FAILED`.

5. **The snapshot is re-read (up to 8 MiB) and fully re-parsed per request.** `AuditProvider::audit` calls `read_host_snapshot` + `from_bytes` (JSON parse, up to 2048 `Advisory::from_str`, SQLite table + index build) on every tool call. Freshness comes from in-document timestamps, not file mtime, so caching by fingerprint would not weaken the "no age reset on copy" property. `Connection` is `!Sync`, so a cache needs a mutex/per-worker handle — worth deciding deliberately rather than by omission.

6. **Unconfigured audit still pays for a full capture and metadata child.** With `config.audit == None`, `application::audit` has already run `source_inner` + `inspector.inspect()` (gateway child) before the provider returns `AuditObservation::unavailable()`. The tool is also unconditionally advertised in `list_tools`. Every call is a guaranteed-`Unavailable` container execution.

7. **`paths()` is computed before the findings budget check** in `audit()` (`let (paths, paths_omitted) = graph.paths(index, control)?;` precedes the `>= MAX_FINDINGS || size + bytes > MAX_PAYLOAD` test), and each call rebuilds the full reverse-edge index (`O(V+E)`). Worst case ~131 072 invocations over a 1024-node/8192-edge graph; bounded only by the checkpoint/deadline, so it degrades into a 120 s timeout rather than a fast rejection.

8. **`issue` is single-valued and `output.issue.or(...)` drops later conditions.** An `UnsupportedSources` audit that also truncates findings reports only `unsupported_sources`; the `OutputBudget` fact survives solely as `findings_omitted`/`paths_omitted`.

9. **Truncation strategy is inconsistent between layers.** Catalog `audit()` hard-fails with `Budget` when the pre-findings `serde_json::to_vec(&output)` exceeds `MAX_PAYLOAD` (reachable with ~1000 unsupported packages), so the `unsupported_packages.pop()` branch in `encode_bounded` can never run for that case — the client gets `Blocked` with no data instead of an explicit omission count.

10. **Text validation is applied unevenly and does not cover format/bidi characters.** `valid_text` gates `package` and `title` but not `advisory.metadata.informational`, whose `Informational::Other(String)` is emitted verbatim (bounded only by `MAX_MARKDOWN`/payload). Also `char::is_control()` covers Cc only, so U+202E and other Cf codepoints pass `valid_text` into `title` — rendering-spoof surface in an MCP client, in a file that otherwise takes control-character hygiene seriously.

11. **`read_host_snapshot` acquires directory authority over the snapshot's parent** (`SecureProjects::new(&[parent])`), which for `/snapshot.json` is `/` and for a shared location is that directory — a capability distinct from `--root`, taken per call. Exposure is limited by no-follow + exact-path reopen + caller-side hash, but it is a boundary the ADR does not spell out.

12. **`document.sequence` is validated (`== 0` rejected) and then never used or exposed.** Consistent with the M1-10 deferral of durable anti-rollback, but the response carries no generation marker at all beyond the file hash; worth stating in the schema description so clients don't infer one.

13. **Capacity/operability:** `MAX_RECORDS = 2048` and `MAX_BYTES = 8 MiB` are hard ceilings whose breach blocks *all* audits (`Budget`), with no headroom signal; and the 86 400 s freshness window means every deployment goes `Unavailable` ~1 day after the operator last supplied a snapshot, with no import/sync CLI until M1-10. Both are ADR-consistent fail-closed choices, not defects — flagging as delivery consequences.

14. **Dependency diff:** the "no existing version changes" framing is accurate for versions but incomplete for features — `semver` gains `serde`/`serde_core` and `url` gains `serde_derive` in the lock diff, enabling previously-unlinked code in already-present crates. Also `lock.rs::valid_name` accepts Unicode XID names, broader than Cargo's ASCII package grammar (no injection risk — SQL is parameterized — but it admits homoglyph names into output).

## Unknowns (not assessed; files not in scope)

- `auditing/schemas.rs` is `#[allow(dead_code)]` and bound only via `#[schemars(with = ...)]`, so nothing compile-time couples it to `domain::AuditObservation`. Whether drift is caught depends on whether `Contract::encode` validates instances against the output schema — I could not see `contract.rs`. If it does not, the mirror can silently diverge. **Recommend explicit verification.**
- `rustsec::Query::crate_scope()` withdrawn/informational semantics are load-bearing (both the security and informational paths rely on it excluding withdrawn). The withdrawn test asserts only `state == Passed` and `findings.is_empty()`; a withdrawn record leaking into `informational` would still pass it.
- `super::{empty, sql, fingerprint, budget}`, `checked_path`, `SecureProjects::open_path`, `FileStamp::from_stat`, `resolve_inner`, and `Workers::run_joined` were not provided; the no-follow/regular-file/single-link/TTL-revalidation properties are asserted here only through tests.
- `main.rs` argument loop: the terminal `else` is outside the diff, so duplicate `--rustsec-snapshot` / `--rustsec-sha256` handling (both guarded by `is_none()` and falling through) is unconfirmed.
- `crates/mcp-server/src/stdio/auditing/tests.rs` and `tests/snapshots/audit-tool.json` were not supplied; the closed 8-tool contract, `deny_unknown_fields` on `Input`, and the `Outcome` tagging were reviewed only as declared here.
