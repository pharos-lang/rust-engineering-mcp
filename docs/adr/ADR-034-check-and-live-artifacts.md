# ADR-034 — Captured Cargo check and live artifact Resources

## Status

Accepted and implemented M1-03; current evidence in validation/M1-03.md.

## Context

rust.check can execute build.rs/proc macros. M1-01/02 already provide a calibrated
immutable Rust runtime, captured source and joined workers. ArtifactStore M0 is
bounded process-local memory, not public authorization or durable storage. Compiler
failures are valid tool results. Resource reads must not bypass live ProjectRef,
retention, owner isolation or output budgets.

## Decision

Add rust.check with project_ref and closed options: optional package, workspace,
features, all_features, no_default_features, all_targets and optional installed
target. Reject contradictory package/workspace and explicit/all features. Bound
names/features/counts; never accept arbitrary flags or args after --. Preserve
frozen/offline, jobs1 and JSON output. Only the approved installed Linux target is
accepted; no dependency/toolchain installation. A new parameterized CheckProject
command is distinct from the fixed Check calibration command. Runtime argv comes
only from validated domain values; configuration binds implementation and grammar,
execution fingerprint binds actual args. Recalibrate the changed configuration.

One joined worker owns source capture, optional calibration, execution, bounded
Cargo JSON normalization, artifact capture and final ProjectRef revalidation.
Rust compiler failures return failed/isError=false with diagnostics; blocked,
unavailable and cancelled remain operational results. No passed result unless
execution exits0 and a complete successful build-finished event was observed.
Partial evidence caused by termination/output/diagnostic budgets never claims
complete validation. For pinned Cargo1.98.1, the exact frozen-lock startup error
at /source/Cargo.lock, exit101 and no Cargo JSON observations maps to
LOCKFILE_UPDATE_REQUIRED, retaining the log and incomplete evidence. Unknown
stderr never grants authority or success; this narrow classifier is advisory
output classification, not authenticated proof of the reason a process failed.
Missing or stale locks are never generated or updated. Cleanup uncertainty outranks all tool outcomes.

Use existing diagnostic domain types with explicit bounds. Preserve grouped
multipart suggestions. Source paths are relative to captured /source; generated
and sysroot locations are not host paths or filesystem authority. Malformed or
unsupported evidence cannot become a successful compilation claim. Retain bounded
textual stdout/stderr in one labeled combined log artifact with balanced stream
budgets, UTF8-safe cuts and explicit section truncation markers; gateway truncation remains visible even when
the store itself did not truncate. Metadata/hash describe retained bytes, not an
uncapped original log. Logs AND normalized diagnostics come from a stream that project code may write;
normalization does not authenticate compiler origin or make them trusted instructions.
The observed process exit and unique terminal build-finished must still agree;
never describe the diagnostic stream as authenticated compiler evidence.

Compose the existing MemoryArtifactStore with registry authorization in application
and MCP. No disk persistence. Default limits remain256KiB/artifact,16MiB/global,
1MiB/owner,256global/64owner items, TTL3600s. Failed artifact publication does not
silently evict existing content. Add owner-bound removal of a single artifact to
rollback newly captured logs after failed final authorization; never revoke prior
artifacts merely because publication of a later job fails. Artifact retention is
checked against the same monotonic origin used by the store before publication.
Before capture/read, prune the registry and reclaim artifacts whose owners are no
longer registered, without touching live owners or renewing their leases. No
cross-project I/O sweep. ArtifactInput marks upstream truncation so retained
metadata and Resources cannot imply the combined log was complete. Artifacts are accessed by canonical opaque URI
rust-artifact://prj_<32lowerhex>/art_<32lowerhex>, no paths, queries or escaping.
Each read resolves live ProjectRef, reads matching owner/retention, copies bounded
bytes and revalidates before publishing. Inexistent/expired/wrong owner is a uniform
resource_not_found (SDK maps -32002 legacy to -32602 for2026-07-28). Artifact TTL is never renewed by reads; expose remaining
retention, not process-relative monotonic timestamps. Project expiry can revoke
access before artifact retention ends.

Resources check readiness before admission; every authorized read runs through
the shared worker. Bootstrap rejection performs no artifact access.
Return base64 blob content to bound worst-case JSON expansion; serialized complete
response still has a cap below the existing1MiB transport frame. Explicit MCP
cacheScope=private and ttlMs=0 prevent suggesting cache authority after revocation.
No subscriptions or global artifact enumeration. List resources remains empty;
returned tool artifact links are the discoverable capabilities.

MCP does not inherit host secrets into the runtime. ArtifactStore retains its
explicit literal redaction policy; do not claim detection of all project secrets.
Apply any configured redaction consistently before diagnostics and logs escape;
never expose raw failing payloads through infrastructure errors or tracing.
The CLI does not add a secret argument or read the host environment wholesale.
Enabling a nonempty redaction policy requires integrating diagnostic redaction
first; the current MCP constructor only admits the explicit empty policy.
A successful Resource read is project activity and renews its idle TTL; it never
renews artifact retention. Once an idle project expires, retrieval is denied.
The Cargo phase is bounded to30s;120s worker budget additionally covers capture,
calibration and joined cleanup. Larger projects may timeout in this first profile.

## Alternatives considered

- Host cargo check: violates the calibrated execution boundary.
- Reuse Go probes or fixed Check evidence for variable args: wrong configuration.
- Retain raw log paths/files: violates live owner authorization and no-follow I/O.
- Text Resource for arbitrary256KiB: JSON escaping can exceed frame budgets.
- Cache public/indefinitely: ProjectRef revocation invalidates authorization.
- Treat nonzero compiler exit as MCP error: compilation failure is domain evidence.
- Claim complete logs after outputlimit: the gateway terminated the producer.

## Consequences

This unit enables one Cargo operation and minimal owner-authorized Resources, not
fmt/clippy/test/audit or persistent distribution. Existing M0 contracts remain
unchanged. Closed schemas are versioned; real compiler error/success, containment,
resource ownership/expiry/budgets and cancellation gates precede Done.

## Review clarifications

- Resource admission Busy/interruption uses fixed JSON-RPC server error -32000;
  internal errors remain -32603. Discovery does not consume the worker.
- A partial check report uses failed to mean the complete-validation criterion was
  not met, with mandatory validation_complete=false and an explicit summary; this
  does not assert compilation failure. Section45 reserves OUTPUT_LIMIT_EXCEEDED
  for inability to deliver a safe partial result. The five-state contract is kept.
- Hard rollback failure outranks the earlier failed authorization because cleanup
  is then uncertain. Errors stay fixed and no untrusted details enter logging.
- SDK3.2.0 defaults list_resources to an empty result; protocol tests exercise it
  in all five versions. No parallel Resource enumerator is needed.

## Retention-capacity fallback

If a live owner's retention quota is full, preserve all previously promised logs
and still publish the completed validation/diagnostics after final live ProjectRef
revalidation. data.log is required-nullable and log_unavailable_reason explicitly
reports retention_capacity. Dropped log streams are marked truncated. Do not claim
OUTPUT_LIMIT_EXCEEDED when this safe partial report fits. No automatic eviction of
live-owner content and no silent cancellation of retention promises. This resolves
normal agent iteration hitting the storage cap without redesigning the M0 store.
A late cancellation after authorized publication can leave a log with no delivered
URI; bounded retention still applies and subsequent validation remains usable.

validation_complete concerns observed process termination and full diagnostic
normalization; later truncation/loss of retained log bytes alone does not change
that assessment. Runtime versions are facts inherited from explicitly verified
provisioning of the immutable image, not new --version observations in each check.
