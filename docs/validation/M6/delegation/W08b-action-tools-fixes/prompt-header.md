# W08b — Close the V08 P3s on the analyzer write path (W08)

Model requested: Claude Sonnet 5 (`claude -p --model sonnet --effort high`). Role: implementation worker on the same files as W08 (working tree, uncommitted W07+W08). Orchestrator: Claude Fable 5.1 (running as Opus 4.8). You may not spawn subagents. **Never run a command in the background.** Do not run Docker-backed tests (the orchestrator re-runs the apply e2e). Do not commit. Read `docs/validation/M6/delegation/V08-review-action-tools/disposition.md` first — it is binding; the reviewer text is context.

## Changes (material V08 P3s; the two deferred-to-debt items are NOT in scope)

1. **Kind check before `plans.resolve`.** In `crates/mcp-server/src/stdio/mutation/analyzer_action.rs::Ports::commit`, verify the resolved plan's `candidate.kind == MutationKind::AnalyzerActionApply` **before** calling `plans.resolve` (which binds the idempotency key). A commit through the analyzer tool that names an M2 plan's id/digest must be refused (`PERMISSION_DENIED`) without binding the key to that foreign plan. Add a test proving the foreign plan's key is NOT consumed.
2. **Grant check before the worker.** Move the `--allow-analyzer-action-write` grant check out of the `workers.run_joined` closure so a call without the grant returns `unavailable/SANDBOX_DENIED` before admission, `try_lock`, or any store/plan state — never LOCK_BUSY/CANCELLED/TIMEOUT. Keep the bootstrap refusal where it is. Test that a no-grant call is SANDBOX_DENIED even under contention.
3. **Listing/preview parity (closes the V07 item).** In `crates/application/src/analyzer.rs`, the `analyzer_actions` listing must mark an action `rejected` (not `applicable`) in exactly the cases `analyzer_action_candidate`/preview would refuse it: edits that change no bytes (map to a rejection reason — reuse `unresolved_edit` or add nothing new to the closed vocabulary if one fits), a non-`.rs` edited path, and a bundle-level limit breach. Add the closed reason mapping (do not widen the wire `ActionRejection` enum if a member fits; if none fits, report before adding). Tests for each. The comment promising this parity must become true.
4. **Widen peer-text sanitization.** In `crates/mcp-server/src/stdio/analyzer/actions.rs::bounded_title` (and any sibling sanitizer for peer-controlled strings on applicable AND rejected actions), replace not only `char::is_control` (Cc) but also bidi controls U+202A–202E and U+2066–2069, zero-width U+200B/200C/200D/FEFF, and the line/paragraph separators U+2028/U+2029, with U+FFFD. Confirm `rust.analyzer.diagnostics`'s `code`/`message` sanitizer (W06b) already covers these; if not, align it. Keep the ≤256-scalar bound. Tests with each category.
5. **Commit/receipt timeout message.** In `worker_failure`/the timeout mapping for the apply tool, a `TimedOut` on `commit` or `receipt` must carry a message telling the caller the write may or may not have landed and to check the receipt (like the `Io` wording), not "Analyzer call exceeded its total budget". Preview keeps its message. Status stays `unavailable/TIMEOUT_TOTAL`.
6. **Audit event vocabulary.** The audit `Event` now carries new `reason`/`tool` values (analyzer codes). Bump the audit event `SCHEMA` constant/version (or, if the event vocabulary is documented as open, add a one-line note where it is defined). Whichever you choose, ensure the existing M2 audit tests and the `test-…` scripts still pass; if bumping the schema breaks a pinned value, report instead.
7. **`ACTION_STALE` / stale-ref code consistency.** Where cheap, make a stale/invalidated `project_ref` map to a single, documented code across preview/commit/receipt rather than PROJECT_NOT_FOUND vs PERMISSION_DENIED vs ACTION_STALE for the same underlying cause; if normalizing is risky, instead document the exact mapping in `docs/tools.md` (which code means what) so a caller can act on it. Prefer documentation over a risky refactor.
8. **Apply overflow code.** Apply refuses (does not trim) an over-budget preview because the exact diff is the review surface. ADR-083 §3 lists `RESULT_LIMIT`. Either use `RESULT_LIMIT` for the overflow (preferred, one closed code) or keep `LIMIT_EXCEEDED` and add a dated ADR-083 note explaining apply refuses rather than trims. Pick one, keep the code set closed, and make the snapshot/tests consistent.
9. **Tests V08 flagged missing:** (a) an M2 plan cannot be committed through the analyzer tool and an analyzer plan cannot be committed through an M2 tool (both directions; the store-level foreign-kind refusal); (b) a non-ignored test that the production `gateway.image_id()` form parses as `sha256:<64hex>` so the decoder cannot silently reject it (use the real `APPROVED_M6_IMAGE` constant, not `"sha256:m6"`); (c) a tool-level test that a grant for a different root stays PERMISSION_DENIED.

## Not in scope (deferred to M6-06 debt, per the disposition)

Audit `admitted` accuracy on post-grant denials; a per-file generated/vendored flag in the preview `files` data; the CPU-amplification `LineIndex` reuse micro-optimization.

## Verification (targeted, foreground)

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked --offline
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo test -p rust-engineering-domain --locked --offline analyzer
cargo test -p rust-engineering-application --locked --offline analyzer
cargo test -p rust-engineering-mcp --locked --offline analyzer
cargo test -p rust-engineering-mcp --locked --offline --bins
cargo test -p rust-engineering-mcp --locked --offline --test protocol --test catalog_status --test crate_inspect --test crate_search
cargo test -p rust-engineering-mcp --locked --offline --test analyzer_runtime --no-run
python3 -B scripts/check-architecture.py
python3 -B scripts/test-release-smoke.py
python3 -B scripts/docs-hygiene.py links-check
```

If any change alters a tool snapshot, regenerate it from the live server and update its hash in `release-smoke.py`; the other 34 must stay byte-identical (assert; stop and report otherwise). If a fix would change an M2 tool's snapshot/schema, STOP and report. Report: Task / Result / Files changed / Tests executed (counts) / Evidence / Risks / Decisions / Open issues.
