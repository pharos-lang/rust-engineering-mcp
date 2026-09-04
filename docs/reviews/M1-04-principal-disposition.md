# M1-04 principal disposition

2026-09-04. External Sonnet5 Medium read-only plus principal source/diff review.
No confirmed unresolved P0/P1 findings in the bounded supported profile.

- P1 pinned grammar evidence: valid review-time gap, resolved by actual approved
  rustfmt1.9.0 Docker seven-case runtime test, including newline-only output and
  all workspace members. Receipt is tracked with current configuration identity.
- P1 CapturedPath charset: not a defect. domain/src/source.rs:17 validates only
  ASCII alphanumeric plus `._/-`, rejects dot/dotdot/empty components, max100bytes.
  SourceFile private fields/new guarantee it; output schema is a superset of valid
  captured names. Contract.encode validates its output. No relaxation required.
- P2 incomplete vs failed: intentional ADR-034/035 five-state contract;
  validation_complete and explicit summary distinguish absence of a complete
  assessment. Partial safe reports must not be discarded. Documentation and tests
  make this visible; future quality gate must require completed passing stages.
- P2 calibration coupling: reviewer misread configuration_fingerprint's serialized
  command table as sequential command execution. FormatCheck is fingerprinted,
  not run in calibration. Calibration still runs actual adverse Cargo check
  scenarios. The approved immutable runtime is shared intentionally.
- P2 duplicate bounds: deliberate independent protocol boundary defense; schema
  maxLength counts characters while internal budget counts bytes.
- P2 diagnostic trim: current formatter yields no diagnostics, but the shared
  typed observation can carry them and MCP boundary tests explicitly exercise
  trimming/downgrade. Keep defensive handling; no new parser claim.

The final core gate also exposed the historical shape-fixture missing RUSTFMT.
The test adaptation adds that exact fixed variable, preserving historical bytes;
production verification remains exact env comparison. Current Docker gates test
real applied configuration. No silent model, runtime, dependency or scope change.
