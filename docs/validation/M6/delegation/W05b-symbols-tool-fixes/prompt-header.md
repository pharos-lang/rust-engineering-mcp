# W05b — Apply the V05 dispositions to `rust.analyzer.symbols` (W05)

Model requested: Claude Sonnet 5 (`claude -p --model sonnet --effort high`). Role: implementation worker, same ownership as W05 plus `crates/execution-adapter/src/lsp_codec.rs` (additive edits only, item 4) and `crates/domain/src/analyzer.rs` (one enum variant, item 4). Orchestrator: Claude Fable 5.1. You may not spawn subagents. Do not commit. Read `docs/validation/M6/delegation/V05-review-symbols-tool/disposition.md` first — it is binding; the reviewer's text is context.

## Changes (all mandatory)

1. **Bootstrap refusal (P1 → documentation).** Keep the house behaviour (`blocked/SANDBOX_DENIED`, "requires completed discovery; retry with a new request ID", identical to `rust.check`). In `docs/tools.md` (section "Contratos M6 — analyzer") document it explicitly as distinct from `unavailable/SANDBOX_DENIED` (runtime not configured / not the M6 image / denied). Add a unit test for the bootstrap path (`ready == false`) asserting `blocked/SANDBOX_DENIED` and `data: null`.
2. **`RESULT_LIMIT` fallback (P1).** When the encoded result is still over 512 KiB after every symbol was trimmed, publish `unavailable/RESULT_LIMIT` (not `blocked`). Test it.
3. **Effective limits (P2).** `limits.total_timeout_seconds` = the caller's `timeout_seconds`; `initialize_timeout_seconds` = `min(60, total)`; `query_timeout_seconds` = `min(30, total)`; keep `max_visible`, `frame_bytes`, `messages` fixed. Make sure the same effective values are what `ExecutionLimits`/the gateway budgets actually enforce (read `analyzer_gateway.rs::AnalyzerBudgets` — if the gateway derives initialize/query from the total differently, publish what the gateway enforces and say so). Test.
4. **Peer-text bounds (P2).** In `lsp_codec.rs` conversions (document symbols, workspace symbols): a `name` or `container` longer than 256 Unicode scalars, or containing any control character (`char::is_control`), makes that entry an **omission** counted under a new `domain::OmissionKind::OversizedEntry` (add the variant; keep the enum closed and documented); `detail` longer than 1 024 scalars is truncated to 1 024 and the entry carries `detail_truncated: true` (add the field to `domain::DocumentSymbol` and the wire schema). Reflect the bounds in `schemas.rs` (`maxLength` 256 / 1 024; pattern excluding control chars where schemars allows it). Tests in the codec (oversized name, control char, long detail) and in the tool mapping.
5. **Quarantine (P2 → documentation).** No code change. One sentence in `docs/tools.md`: gateway quarantine (`CleanupUncertain`) is a JSON-RPC internal error, as for every guest tool, not a tool status.
6. **Digest outside the critical section (P3).** Compute the bundle digest before `with_gateway`.
7. **Input schema (P3).** `expected_project_fingerprint` gets `^sha256:[0-9a-f]{64}$` in the schema.
8. **Trim margin (P3).** Explain the `MAX_RESULT / 4` threshold in a comment (envelope + `text` mirror) or replace it with a measurement of the encoded `CallToolResult`; add a test proving the final encoded result never exceeds 512 KiB with 512 symbols of maximal name/detail length.
9. **README wording (orchestrator).** Restore the truth: M4 and M5 are closed and integrated in `main` (M5 published as `v0.3.0`); only M6 is in local development. "El checkout `0.3.0` descubre 32 tools" → distinguish the published 0.3.0 release (31 tools) from the development checkout (32).
10. **Snapshot and pins.** Regenerate `crates/mcp-server/tests/snapshots/analyzer-symbols-tool.json` from the real `tools/list` output (the definition changes with items 4 and 7), update the hash in `scripts/release-smoke.py`/`test-release-smoke.py`; the other 31 snapshots and hashes must stay byte-identical (assert it; stop and report otherwise).

## Verification (targeted)

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked --offline
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo test -p rust-engineering-domain --locked --offline analyzer
cargo test -p rust-engineering-execution --locked --offline --lib lsp_codec
cargo test -p rust-engineering-application --locked --offline analyzer
cargo test -p rust-engineering-mcp --locked --offline analyzer
cargo test -p rust-engineering-mcp --locked --offline --test protocol --test catalog_status --test crate_inspect --test crate_search
python3 -B scripts/check-architecture.py
python3 -B scripts/test-release-smoke.py
python3 -B scripts/docs-hygiene.py links-check
```

Do not run Docker-backed tests (the orchestrator runs the native suite). Report with headings: Task / Result / Files changed / Tests executed (counts) / Evidence (new snapshot hash) / Risks / Decisions / Open issues.

## Addendum — native run diagnosis (orchestrator, 2026-09-12 11:45 UTC)

11. **Harness defect in `crates/mcp-server/tests/analyzer_runtime.rs`.** The orchestrator ran `scripts/test-m6-runtime.py` on your tree: the nine gateway cuts passed; your tenth selection failed with `Error: Disconnected` after 1.3 s. Classification criterion written before reproducing: (H) harness — the server exits at startup or on the first frames because of the test's argv/env/handshake; (P) product — the server accepts the flow and fails during the tool call; (I) infrastructure. Reproduction with the real binary and the identical argv, but with the MCP handshake (`initialize` → `notifications/initialized` → `rust.project.open` → `rust.analyzer.symbols`): **passed** — `readiness quiescent 323 ms, health ok, completeness complete, utf-8, version 1.98.1 (48a229c 2026-09-01)`. Your test sends `tools/list` as request 1 **without** `initialize`/`initialized`, so the server closes the session and stdout reaches EOF → `Disconnected`. Verdict: (H). Fix: perform the handshake exactly as `tests/inspection_runtime.rs` does (initialize with a supported protocol version, then `notifications/initialized`), keep the rest of the assertions. Do not run it (Docker is not available to you); the orchestrator re-runs the suite.
