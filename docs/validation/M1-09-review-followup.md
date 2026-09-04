# M1-09 — focused external follow-up

Claude Code2.1.259, explicit claude-opus-5 Medium, read-only/tools disabled.
modelUsage confirms Opus5. Supplied final source/tests/ADR, no commands executed
by reviewer. This is code review, not a runtime gate result.

## Verdict: no confirmed P0/P1 in this slice.

All four items are addressed by code that is internally consistent with ADR-040 and covered by tests. Evidence per item:

**P1-2 late publication** — Confirmed fixed.
- `crates/application/src/validation.rs` — the closure's last acts are `control.check()?` then `self.touch_authorized_reference(reference).map_err(access_error)?`; any `Err` from the closure runs the `pending` rollback + `reap_artifacts` and returns before `ProjectQualityGate` is constructed. So `Ok` implies the lease touch and log group are committed.
- `crates/mcp-server/src/stdio/quality.rs` `joined_result` — `(Ok(value), _) => Ok(value)` no longer lets a late `Joined::interrupted` rewrite a committed result; `(Err(Cancelled), Some(signal))` still maps through `worker_error`, and `(Err(CleanupUncertain|Infrastructure), Some(_))` still outranks the signal.
- Tests: `committed_publication_survives_late_cancellation_and_timeout` (asserts `is_error == Some(false)`, `status == "passed"`, all three log URIs retained) and `cleanup_uncertainty_and_infrastructure_outrank_worker_interruption` pin both directions.
- ADR-040 Decision + Consequences now state the commit point explicitly and acknowledge SDK-side suppression after commit ("Delivery is not transactional"), which matches the code rather than over-claiming.

**P1-3 capture freshness** — Confirmed fixed as a documented, conservative-enough decision.
- `publish_quality` builds `Provenance` with `created_at`/`observed_at` both `captured.created_at` (set in `capture_validation` *before* `source_inner`, so never newer than the real capture), assesses at construction, then re-assesses at the final publication instant: `Evidence::Snapshot(SnapshotEvidence::assess(snapshot.provenance().clone(), snapshot.freshness().policy().clone(), clocks.0))`.
- Audit snapshot is independently re-assessed at the same point followed by `row.classify()`, so a snapshot that ages out during log publication downgrades the stage (`audit_fresh_at_execution_is_reassessed_after_log_publication` asserts `Unavailable` + `SnapshotStale` + `assessed_at == 301`).
- `outer_capture_age_includes_execution_and_log_publication_without_changing_verdict` pins Aging/Stale outer evidence with a `Passed` verdict, matching the ADR sentence "does not promise the live files still match." That is a real residual — a `passed` gate can carry `stale` evidence — but it is stated in the ADR, visible in the wire `evidence` object, and the alternative (blocking on capture age) was explicitly rejected. Not a finding.

**P1-4 cross-stage runtime** — Confirmed fixed, checked at both layers.
- `crates/domain/src/quality.rs` `quality_runtime_matches` compares `platform`, `image_id`, `configuration_fingerprint`, `rust_version`, `cargo_version`, `declared_toolchain` and deliberately omits `execution_fingerprint`, consistent with your statement that it hashes COMMAND + limits and must differ per stage.
- Application: `match_runtime` runs on every `Ok` observation *and* on `structure.runtime` before `audit` is invoked, so a divergent runtime aborts before any capture and before correlation.
- MCP `output()` re-derives a baseline and rejects divergence independently — defense in depth against a malformed application result.
- Tests cover all six identity fields × two stages (`inconsistent_runtime_aborts_before_logs_and_audit_but_command_identity_may_differ` asserts `store.captures == 0`, no `audit` in `seen`, and lease not renewed) and the negative case (`field == 6`, differing `execution_fingerprint`, passes). `divergent_audit_runtime_is_rejected_but_command_execution_fingerprints_may_differ` mirrors it at the MCP layer.

**P2 diagnostic count** — Confirmed fixed.
- `output()` truncates to 128, adds to `diagnostics_omitted`, clears `validation_complete`, then `row.classify()` — which treats `diagnostics_omitted != 0` as `Incomplete` → `Blocked`. Schema declares `length(max = 128)`, so wire and schema agree.
- `per_stage_diagnostic_count_is_bounded_even_below_byte_budget` asserts the trim happens with the payload well under `MAX_RESULT / 4`, that `diagnostics_omitted` accumulates onto a pre-existing count, and that unaffected stages stay `passed`.

## Non-blocking observations (P2/nit, no action required for this slice)

1. `quality_runtime_matches` and the `0..7` / `0..6` test loops both enumerate `RuntimeIdentity` fields by hand. Adding a field to `RuntimeIdentity` silently widens what counts as "same runtime." Destructuring the struct in `quality_runtime_matches` would turn that into a compile error.
2. `publish_quality`: the `metadata.owner != *reference` guard returns `Err(Internal)` *before* `pending.push(metadata.id.clone())`, so that one artifact is not rolled back by ID and survives `reap_artifacts` if the forged owner is live. Only reachable via a misbehaving `ArtifactStore`, which you've stated is trusted — but moving the `push` above the owner check costs nothing.
3. Diagnostics are count-bounded, but `audit.observation.findings` / `informational` / `unsupported_packages` are bounded only by the byte budget in `trim_one`. Whether that can violate a `length(max = …)` in `super::auditing::schemas::AuditObservation` isn't visible in this slice; worth a one-line check against that schema.
4. ADR-040 has lost spaces before several numerals ("implementation2026-09-04", "Spec23.11/24/49", "deadline240s", "budget", "512KiB", "60s/300s") — cosmetic, but it's the normative document.
