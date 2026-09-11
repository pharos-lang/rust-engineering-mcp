# M1-09 — Principal Engineer disposition

2026-09-04. External Opus5 High review in M1-09-review.md is an independent
read-only assessment of its supplied packet, not execution evidence. The initial
packet predates the block-trimming optimization and completion of runtime tests.

- P1-1: not confirmed in the actual architecture. Workers::new has one permit;
  admit uses try_acquire_owned before entering closures. All project/publication/
  Resources handlers acquire Registry then Store, after admission, without await.
  Resource requests during a gate are rejected as Busy, not queued on a mutex.
  This serialization is deliberate ADR-030 backpressure; no lock inversion found.
- P1-2: fixed for quality. The final application control check before the single
  lease touch defines commitment. A successful application result survives a signal
  observed only after it returns; earlier interruption rolls back. Hard cleanup/
  infrastructure errors still dominate signals. SDK cancellation or broken transport
  can suppress a committed response; delivery cannot be transactional. Such logs
  remain owner-authorized and bounded by TTL/quota, as with any lost response. This
  residual is explicit in ADR-040; no claimed rollback after successful delivery.
- P1-3: fixed. Both created_at and observed_at now denote capture; freshness is
  reassessed after all log publication at the final wall clock. Aging/Stale is visible
  even if the captured generation passed; latest_known never promises current files.
- P1-4: defense added in application and MCP, including audit metadata: runtime
  platform/image/configuration/Rust/Cargo/declaration must agree. Execution fingerprints
  intentionally differ because rust_gateway hashes command/limits/source/scope;
  requiring equality as suggested would reject every real multi-command gate. The
  audit metadata source digest is checked against the common digest before correlation;
  the top-level digest binds all stages, without a redundant new audit DTO field.
- P1-5: worst-case lease-headroom rejection rejected. It would reject valid fast
  executions on live short leases, with no safety gain. Revalidation before each
  stage and at commitment fails closed; expiry aborts with no stage payload and
  rolls back new logs. No speculative TTL extension is permitted. Tests exercise it.

Material P2 disposition:
- Artifact expiry shares the existing opaque NotFound authorization mapping. The
  quality error text describes current project/runtime policy, not a claim that the
  physical project vanished. No unauthorized logs or renewed lease survive failure.
- Closed selections are covered by application option assertions and actual runtime
  cases; emitted labels deliberately name these fixed profile contracts. No user
  flag path exists. Defaults are verified by tests, not assumed from host Cargo.
- MemoryArtifactStore is the trusted hashing adapter: Sha256::digest(content), private
  immutable entry bytes, not untrusted input metadata. Real runtime Resource tests
  recompute hashes. Adding hashing to application would duplicate the real adapter
  boundary without authenticating a malicious implementation of the entire port.
- Omitted nonempty log streams propagate stdout/stderr flags into top-level truncation
  and downgrade the stage; each stage additionally names retention_capacity. There
  is no invisible omission of a nonempty log behind a clean passed gate.
- Abort paths have no stage data, now explicitly stated in ADR-040. Completed ordinary
  failures preserve subsequent stage outcomes; cancellation/uncertain cleanup cannot
  publish partial success. Busy keeps existing SANDBOX_DENIED compatibility.
- Diagnostic count defense added at MCP even below the byte budget: retain at most128,
  increment omitted count and block clean pass. Real execution parser already bounds128.
- Unsupported packages are complete in the current audit port: graph max1024 packages,
  full unsupported clone, no silent port truncation. The MCP omission counter records
  only its later removals; domain normalize checks package accounting.
- readOnlyHint matches rust.test and means no source writes; descriptions explicitly
  warn that code executes in the ephemeral sandbox. No assertion of side-effect-free
  guest execution or external client approval behavior.
- std::time::Instant measures observation duration only, not policy or authorization;
  security/freshness clocks remain injected. No timing port needed for this telemetry.
- Block trimming replaces the old one-item loop (focal suite2.12s vs old38s). Diff
  omission addition is bounded by five stage rows. Bootstrap override has no data;
  no stale derived counters exist there. No unrelated refactor required.

Post-fix focal evidence: application8/8 (including14 runtime scenarios and aging/stale
capture), domain3/3 in core, MCP12/12 +1 ignored snapshot emitter, Clippy-Dwarnings.
Current full gate and final targeted external follow-up are recorded in M1-09.md.

Focused Opus5 Medium follow-up confirms the four corrections and no P0/P1 in
that slice (M1-09-review-followup.md). Non-blocking observations disposition:
manual RuntimeIdentity field comparison is explicit and covered for all current
fields; future schema changes require review. Wrong-owner ArtifactStore metadata
violates a trusted port contract and is rejected; moving pending.push alone cannot
remove a different owner's artifact and must not authorize deleting live foreign
objects. The real store creates owner-bound entries and cannot emit this state.
Audit schema has no collection max-length constraints for these three arrays;
real port limits1024 packages/128 findings plus MCP bytes remain authoritative.
ADR spacing corrected. No security gate waived or current runtime changed.
