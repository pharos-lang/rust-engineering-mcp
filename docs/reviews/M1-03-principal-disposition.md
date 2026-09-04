# M1-03 — Principal disposition of independent review

Claude Code2.1.259, explicit claude-opus-5/high, read-only/no tools. Actual modelUsage
confirmed Opus5; auxiliary Haiku telemetry is not a replacement reviewer. Initial
findings preserved in M1-03-opus-initial.md; they are historical, not final status.

| Finding | Disposition and evidence |
| --- | --- |
| P1-1 expired owner quota | Fixed. prune + retain_owners before capture/read and after known revocation; no cross-project filesystem sweep. Live owners preserved; expired owners reclaim capacity in application/artifact tests. |
| P1-2 stdout starves stderr | Fixed. Balanced UTF8-safe sections, markers, upstream truncation propagated through ArtifactInput to metadata and Resource. Both large streams retained within256KiB; hashes cover stored bytes. |
| P1-3 hardcoded runtime versions | No current wrong version: image ID was explicitly provisioned/verified1.98.1. Strengthened approved image/version tuple and bound project_inspection implementation to configuration fingerprint. Actual M1-02 command observations and new real check receipt confirm current image. |
| P1-4 unauthenticated diagnostic stream | Accepted limitation and fixed trust wording in ADR, tools, MCP description and diagnostics schema. Project code may write Cargo output; normalization is not authentication. Source integrity evidence refers to captured source. |
| P2-1 30s Cargo | Intentional first bounded profile, now explicit.120s worker includes calibration/capture/cleanup; larger projects may timeout. No arbitrary peer timeout in check. |
| P2-2 features schema | Fixed per-item Feature schema mirror (128 bytes/closed ASCII grammar); domain still checks conflicts/duplicates. Snapshot updated. |
| P2-3 Resource Busy internal | Fixed -32000 with static busy/retry message; actual running-check test verifies prompt busy response and discovery responsiveness. Internal remains -32603. |
| P2-4 bootstrap wording | ADR aligned: readiness guard before admission does no artifact I/O, authorized reads always joined. |
| P2-5 fallback loses URI | Minimal Data is bounded by internal types/constants and below8KiB in production. Diagnostic reduction preserves log URI; adversarial serialization test verifies. Last fallback is defensive for contract/invariant violation; no concrete reachable oversized Data from current adapter. |
| P2-6 incomplete failed | Explicit ADR decision: failed complete-validation criterion with mandatory validation_complete=false and summary, not assertion of compiler failure. Spec45/67 forbids OUTPUT_LIMIT_EXCEEDED if safe partial/artifact is deliverable; no new status or misleading error cause added. |
| P2-7 future nonempty redaction | Current constructor only empty literalpolicy. ADR requires diagnostic redaction before enabling nonempty policy; no configurable path currently exists. No current leak of configured secret. |
| P2-8 rollback failure masks cause | Intentional safety precedence: hard cleanup failure outranks authorization/cancellation. Fixed errors avoid untrusted payload logging. Fault tests cover rollback failure. |
| P2-9 repeated256KiB ceilings | Deliberate independent boundary budgets in application/MCP/store. Raising a storage limit must not silently raise public authorization/output budgets; mismatch fails closed. |
| P2-10 read renews project idle TTL | Documented as activity; artifactTTL never renews. Tests prove failed reads do not renew and expiredrefs deny. |
| P2-11 default resources/list | Not a bug. Read SDK3.2.0 ServerHandler default and actual protocol tests in all5versions return empty array. |

Additional principal fixes: real E0106 empty span labels normalize to None without
truncation (discriminating unit + actual runtime). Exact frozen lock startup
classification for absent/stale Cargo.lock, exit101 and no JSON, preserves log and
returns blocked/LOCKFILE_UPDATE_REQUIRED without host/source mutation. Unknown or
near-match stderr cannot confer authority or passed status. Pure classifier negative
cases and real frozen checks cover this path.

Initial review could not inspect full invariants. SourceBundle sorts paths; real
multi-file workspace spans pass. MemoryArtifactStore expiry uses expires > now;
application rejects zero remaining retention. Actual args/source changes alter
execution fingerprints and six changed-configuration Docker tests passed. No native
Linux/Windows/x86_64, client interoperability, license or utility benchmark claim.

## Focused follow-up disposition

Opus5 Medium (model verified) confirmed balanced streams, trust wording and
version tuple. Follow-up live-owner quota concern is fixed without deleting live
logs: quota fallback retains result/diagnostics with required-nullable log and
retention_capacity reason. Tests prove passed/failed isError=false, final live
authorization on quota and old-log preservation. This is a new application policy,
not eviction or redesign of M0 capture semantics.

Post-admission Resource cancellation/deadline now maps -32000 as documented; hard
internal remains -32603. ADR clarifies validation completeness versus later log
loss, inherited image-version facts, and single-admission lock assumptions. Late
cancellation after authorized publication cannot guarantee URI delivery; content
stays bounded by retention and no longer blocks subsequent validation when full.

The defensive oversized-text observation was useful: lossy UTF8 can expand bounded
gateway bytes. Check now truncates expanded text at UTF8 boundaries with visible
flags before parsing, preserving a safe partial report; a discriminating test
covers invalid-byte expansion. Complete core and six Docker tests rerun after all
corrections. No confirmed unresolved P0/P1 remains in the current bounded profile;
capacity/timeout/platform/unauthenticated-output limits remain explicit.
