# M1-11 — Principal review disposition

Read-only Sonnet5 High review, Claude Code2.1.259; exact model confirmed by result
modelUsage. [Original findings](M1-11-review-sonnet.md). Reviewer was given a bounded
packet, not execution authority; unshown context is checked below against source.

- P1 RustSec record-count bound: rejected premise. `catalog-adapter/src/audit.rs`
  defines MAX_RECORDS2048; from_bytes rejects empty records and >2048 before building
  RustSecSnapshot. Runtime maps those parser failures to unavailable, so neither
  claimed full-tool outage is reachable with that provider. Existing parser bounds
  remain unchanged. Principal cross-check did find a separate real mismatch: the
  audit snapshot allows any positive u64 sequence, while status applied the catalog's
  signed-i64 floor bound. Fixed status validation; u64::MAX regression test added.
- P2 network_used: rejected recommendation. This bit is source provenance, including
  a publisher's original acquisition. It does not grant runtime network authority.
  Rejecting true would discard valid signed history. ADR042 makes that distinction
  explicit; a domain test preserves publisher network provenance. OS deny is tested
  independently, with positive controls, by the native gate.
- P2 RustSec read errors: accepted test gap. Added actual missing-file, symlink and
  8MiB+1 boundary cases through the provider; Missing/Denied/Budget leave catalog
  available. Unsupported-platform status still derives from the existing fail-closed
  platform adapter; no native Linux/Windows execution is claimed.
- P2 semantic coverage: completed after the initial packet. Added core-feature
  disabled test and actual E5/Lance CLI/MCP gate under network deny. Tests cover
  model absence, native corruption, old intact index against a new signed catalog,
  exact model/catalog/count binding, immutable session cache and restart behavior.
  Existing native persistence tests cover crate membership/schema validation.
- P3 sentinel Budget: intentional internal short-circuit for failed control between
  protected reads. A final check preserves actual cancellation/deadline; it is not
  intended to expose Budget for that path. Genuine read-size/count budgets still
  yield component Budget. No swallowed cancellation or released admission.
- P3 index evidence: index metadata binds the exact catalog and full model identity;
  their component evidence supplies freshness. Documented explicitly in ADR042.
- P3 shared freshness thresholds: intentional existing M1 classification; freshness
  does not expire or prohibit use of a pinned model. Current E5 has unknown creation
  age, honestly preserved. No made-up live timestamp.
- P3 duplicated bootstrap construction: harmless common adapter envelope pattern;
  final construction provides a specific retry hint. Existing contract tests verify it.

Principal integration review confirms shared single worker, bootstrap gating,
read-only provider lifetime, host flag pairs, eleven definitions, unchanged ten old
snapshots and CLI floor byte compatibility. No M1 milestone closure is inferred.

[Sonnet5 Medium follow-up](M1-11-review-followup.md) confirms the source disposition
and finds no remaining actionable code defect. It requested actual final run
evidence; this is now attached: [native transcript](M1-11-native.txt),
[core report](M1-11-core.json), [hashes and final counts](M1-11-summary.json).
The complete final core and native extended-index tests passed; reviewer evidence
limitations are retained rather than represented as reviewer execution.
