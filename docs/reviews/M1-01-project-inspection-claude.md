# M1-01 project inspection — external review

## Scope and invocation

Claude Code2.1.259, verified locally with --version/--help. Read-only packet,
no tools, no MCP connectors, no session persistence, safe/restricted modes,
permission-mode dontAsk. Explicit Sonnet5 Medium reviewed metadata parser and
schema mirrors; explicit Opus5 High reviews cancellation/cleanup composition.
Main Technical Owner retains security, public contract and dispositions.

## Sonnet5 Medium

Observed modelUsage: claude-sonnet-5; auxiliary Haiku CLI telemetry is not a
substitute reviewer. The response did not establish a P0/P1 defect; several
speculations were explicitly retracted by the reviewer itself.

- Nullable rename/inherits schema bounds: accepted as contract precision;
  added bounds also to relative_path, target_condition, MSRV and toolchain.
  Tightened activation/profile-override counts to parser limits.
- Empty relative_directory suggestion: rejected as redundant. Existing
  validate_source_path rejects an empty string before directory/manifest lookup.
- Claim that ProfileValue kind/value mismatch could pass anyOf: rejected.
  Discriminant const values bind each schema variant. Added discriminating tests
  for boolean5, integertrue and textfalse; all rejected by the actual validator.
- Origin identity intentionally identifies origin, not a dependency edge;
  target_condition is separately preserved. No edge-identity claim exists.
- Unit/null and MSRV prerelease concerns were withdrawn; bounded parser tests
  already discriminate omissions and unsupported values.
- Closed profile key matching precedes value validation; no unknown boolean key
  becomes accepted through the final boolean arm.

## Opus5 High

Observed modelUsage: claude-opus-5 (auxiliary CLI Haiku telemetry also recorded).

- P1-1 cancelled admission permits: rejected as a change request to reviewed
  ADR-030 policy, not a newly discovered leak. The16 retained slots are intentional,
  documented and tested. Releasing permit while keeping unbounded tombstones or
  relying on a callback for suppressed responses weakens the bounded design.
  Reconnect after exhausting the documented session budget remains required.
- P1-2 repeated failed calibration: accepted. RustGateway::calibrate can return
  Denied on failed containment markers. RustProjectInspector now latches every
  non-cancellation calibration error, preserving the first failure and refusing
  later calibration/execution in that session. Clean cancellation can retry.
  A discriminating unit test verifies attempts remain one after six hard error
  categories and cancellation can proceed to a single successful calibration.
- P1-3 hardcoded versions: rejected as missing context. RustGateway::new rejects
  any image except APPROVED_RUST_IMAGE before Docker commands. The explicit runtime
  provisioning/verification receipt binds Rust/Cargo1.98.1 to that immutable ID.
  Added a local comment linking the fact to that constructor invariant.
- P2-1 identity discarded: no defect. resolve_inner rejects a changed identity and
  drops its lease; application tests cover changed fingerprint and expired TTL.
  Added a comment at final revalidation and explained conservative pre-capture time.
- P2-2 unavailable policy retry wording: accepted wording fix; policy absence
  remains SANDBOX_DENIED because the host has not authorized execution, not proof
  that a binary is missing. Only bootstrap rejection instructs discovery/retry;
  permanent host policy/calibration denial no longer promises a retry will fix it.
- P2-3 gateway quarantine: accepted explicit check before calibration/execution,
  although the gateway already checks its own quarantine before execution.
- P2-4 readiness durability: protocol tests cover first inspect rejection and
  post-discovery normal validation across the pinned SDK lifecycle. Actual runtime
  test issues inspect immediately after discovery/open and requires success.
- P2-5 registry lock: all acquisitions are within the same shared worker; neither
  project.open nor inspection locks it on the current-thread reactor. Actual
  tools/list responsiveness during observed calibration covers this boundary.
  Joined deadlines intentionally depend on cooperative gateway control boundaries;
  teardown grace and existing real timeout/cancel/overflow tests preserve the limit.

No P0 found. Findings are dispositions by the Technical Owner, not an assertion
that the external reviewer independently verified the subsequent fixes.
