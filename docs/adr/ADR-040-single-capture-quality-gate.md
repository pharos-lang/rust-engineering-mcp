# ADR-040 — single-capture quality gate and grouped publication

## Status
Accepted and validated 2026-09-04; core498 and integral full14/14. Evidence: ../validation/M1-09.md.

## Context
Spec 23.11/24/49 requires fast(fmt/check/clippy) and standard(+test/audit), preserving
repair detail and all required outcomes. Calling existing MCP tools or registry
wrappers would recapture files and renew authorization separately. Existing ports
already accept owned SourceBundle; shared workers prohibit nested admitted jobs.

## Decision
One joined worker, one live ProjectRef, one source capture. Call existing execution
ports directly in fixed order fmt/check/clippy/test/audit. Standard metadata and
RustSec correlation consume exactly that same bundle. Compare all returned source
fingerprints and runtime platform/image/configuration/toolchain identity; inconsistency
is infrastructure, never a combined snapshot. Execution fingerprints differ by command
and are retained individually, not compared for equality. Audit metadata is checked
against the common source fingerprint before correlation. Revalidate
project between stages without touching lease. Continue after ordinary validation
or operational failures; infrastructure, uncertain cleanup, cancellation or global
deadline observed before publication commitment abort without publishing new logs.
Abort paths return no stage data. Completed gates preserve every stage's status and
repair facts in the final result. Global deadline 240s includes initial calibration; each
command keeps its existing 30s work budget; cleanup is still joined independently.

Profiles have closed options: fmt --all; check Cargo defaults; Clippy Strict (-D
warnings), Cargo default members; standard test Cargo defaults with 30s, then audit.
No free flags, selectors, downloads or lock modifications. This is not all-target/
all-feature/all-workspace test coverage. Each stage records the applied selection.
Only every required complete passed result produces overall passed. Aggregate
precedence is blocked, unavailable, failed, passed; cancellation is suppressed by
existing MCP policy after worker completion. A successful application return is a
committed publication: a later worker signal does not replace it with a timeout or
cancelled payload. The final control check immediately before the single lease touch
is the commit point; earlier interruption rolls back. SDK cancellation/transport loss
can still suppress a committed response, so already committed logs may remain until
their bounded TTL, just as after any lost response. Delivery is not transactional.
Individual statuses are never collapsed.

Publish zero to four optional log artifacts as one bounded group. Extend existing
artifact authorization helper with a no-touch internal path while preserving public
read semantics. Stage only new IDs; verify owner/id/bytes/metadata/retention and final
live project identity without renewing. Recheck all retained lifetimes at one final
artifact-clock instant and then touch project lease once. On any failure, attempt
rollback of every newly staged ID, preserving earlier live-owner logs; rollback
failure is infrastructure. Quota may omit optional logs explicitly, not authorization.
No new ArtifactStore persistence, root policy or Resources URI design.

Outer evidence uses the common source fingerprint when any stage observed execution;
created_at and observed_at are the capture instant, assessed_at is final publication,
using the existing 60s/300s freshness policy. A passed gate describes that captured
generation even when its age is Aging/Stale; it does not promise the live files
still match. All-operational failures may carry local evidence and no source fingerprint instead
of fabricating capture identity. Runtime/evidence remains per-stage. Audit normalization
is shared pure domain policy, re-assessed at final publication; stale, unknown,
unverified or incomplete audit never passes. MCP bounds total detail to 512KiB with
explicit omissions and conservative status downgrade. Raw streams only through
existing live/opaque/retained Resources, never duplicated into structured gate data.

## Alternatives considered
Nested MCP calls duplicate admission and parsing. Existing registry wrappers capture
multiple generations and publish independently. Dropping stage detail prevents repair.
Using one log with forgeable stage delimiters weakens independent stage evidence.
Per-stage project touch can extend authority after an eventually failed batch.

## Consequences
Application orchestration remains independent of rmcp/Cargo/SQLite. The grouped
publication extends an existing capability boundary and preserves single-tool public
contracts; focused regression and adversarial rollback tests are mandatory. No M0
redesign or M2 scope. Runtime image/toolchain/policies remain approved; full gate
requires original Docker/E5/ONNX assets and actual semantic network-denied execution.

Grouped log omission preserves the execution's validation_complete assessment but
sets stream omission flags, as single-tool publication does. Quality-gate verdicts
are conservative: any omitted nonempty log stream prevents a clean overall pass;
stage execution facts and explicit retention-capacity reason remain visible. Empty
logs need not be retained to establish complete execution evidence.

The shared Workers permit admits exactly one job without a queue, including Resource
reads. All handlers acquire Registry then Store after admission; no second worker
waits on these guards. Busy preserves the existing conservative SANDBOX_DENIED
contract. A lease is checked before each stage and at publication; there is no
worst-case headroom rejection, which would reject otherwise valid short runs. A
lease expiring during work fails closed and rolls back; callers can rediscover.
