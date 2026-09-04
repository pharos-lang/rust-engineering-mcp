# M1-08 external review

ClaudeCode2.1.259, explicit claude-sonnet-5 Medium; actualmodelUsage verified.
Read-only/restricted/no tools. Packet omitted test bodies; findings evaluated below.

## M1-08 Review — Compiler Explanations (read-only)

### P1

**`crates/execution-adapter/src/project_inspection.rs` — `explain_observation` (success-path stderr requirement)**
The success branch requires `execution.exit_code == Some(0) && !stdout.trim().is_empty() && execution.stderr.is_empty()`. Any nonempty stderr on a clean `rustc --explain` run (e.g. a deprecation/lint notice from the toolchain, locale warning, etc.) drops straight into the catch-all `Err(ExecutionError::Infrastructure)`, which `stdio/explaining.rs` maps to a hard `ErrorData::internal_error`. That means a single benign stderr line for any one diagnostic code would make that code permanently unexplainable through this tool (opaque 500-style failure, not a graceful `Unavailable`/`Blocked`). The unavailable-code branch has an equally strict exact-string match on stderr (`"error: {code} is not a valid error code"`), so the same risk applies there. Since the "new runtime test currently running" is exactly what would surface this, I'd treat this as a blocking item until that test confirms zero stray stderr bytes on success/absence across the actual 1.98.1 explain surface (not just the E0502 calibration probe and E9999 unit test). If any code does emit stderr noise, the current design has no fallback and needs a decision (tolerate/trim vs. hard-fail).

### P2 (material)

**`crates/application/src/explain.rs` — `FreshnessPolicy::new("compiler-explanation-v1", 60, 300)`**
A compiler explanation for a pinned, immutable 1.98.1 image is invariant content — it can never go "stale" the way a project snapshot can. Applying a 60s/300s fresh/stale window to this evidence produces a `freshness` field in the response that will misleadingly report content as stale after 5 minutes even though nothing about the underlying fact has changed. Worth confirming this threshold was chosen deliberately for this tool rather than copied from a project-snapshot use case; if unintentional, it undermines the "latest_known provenance/freshness" contract ADR-039 calls for.

**`crates/mcp-server/src/stdio/explaining.rs` — duplicated/dead classification inside `output()`'s `Ok(explanation)` arm**
The checks for `observed.stdout_truncated`, `observed.stderr_truncated`, `ExecutionTermination::OutputLimit`, `ExecutionTermination::TimedOut`, and `explanation.len() > MAX_EXPLANATION` are all currently unreachable, because `explain_observation` in the adapter already converts every one of those conditions into an `Err` before an `Ok(ExplainObservation)` can be constructed (it also hardcodes `stdout_truncated`/`stderr_truncated` to `false`). This isn't a vulnerability today, but it's two independent sources of truth for the same limit/timeout logic with no test currently exercising the "dead" branch — if the adapter's behavior is ever refactored to pass through a truncated-but-nonempty observation, this code silently becomes load-bearing without having been validated. Suggest either removing the redundant checks (trust the adapter's invariant, matching the `debug_assert!`-style pattern used for the final `else => internal_error` branches) or adding a unit test that actually exercises the stdio-layer path with a truncated `Ok` observation to prove it works.

**`crates/execution-adapter/src/project_inspection.rs` — exact stderr string match for "unavailable" detection**
Flagging as material rather than blocking since it's explicitly compiler-version-scoped and fails closed (mismatch → `Infrastructure` error, not a fabricated explanation), consistent with ADR-039's "do not fabricate" requirement. But it is a hard-coded dependency on rustc's exact English error text with no version guard beyond the pinned image; a future image bump (even a patch release) silently regressing this string turns *every* code lookup for genuinely-unsupported codes into `internal_error` instead of `Unavailable`. Worth a code comment or test cross-referencing the specific installed 1.98.1 string so a future toolchain bump is forced to re-verify it (this is somewhat already implied by the "actual calibration and argument/containment tests" consequence in the ADR — just confirm that's tracked).

### Not flagged (reviewed, looks sound)
- `DiagnosticCode` validation (`crates/domain/src/value.rs`) correctly rejects lowercase, full-width Unicode digits, embedded whitespace/shell metacharacters, and wrong length; tests cover the injection-shaped inputs explicitly.
- Argument construction in `rust_gateway.rs` passes `--explain <code> --color never` as a fixed argv array (early `return vec![...]`), bypassing the generic flag-merging path — no shell interpolation, no free-flag injection, matches "no free flags or host subprocess" in the ADR.
- Empty `SourceBundle::new(Vec::new())` with no project handle/lease — consistent with "no project source/handle or Resources is needed."
- `explain_diagnostic` in `application/src/explain.rs` re-verifies `observation.code == code` before trusting the port response — good defense against a misbehaving adapter.
- Limits (64KiB/stream, 30s exec, 120s joined deadline, 512KiB envelope) are all wired through and match the ADR's stated numbers.
- Cleanup-uncertainty and cancellation semantics reuse the existing quarantine/`joined_result` pattern rather than inventing new operational semantics, as required.

No P0s found. Given the review scope, the main actionable follow-up before treating M1-08 as gate-ready is confirming the P1 stderr-emptiness assumption against the real running compiler test.
