# W06 — informe del worker

| Campo | Valor |
| --- | --- |
| Modelo | Claude Sonnet 5 (`claude -p --model sonnet --effort high --disallowedTools Agent Task`), Claude Code 2.1.268 |
| Inicio / fin (UTC) | 2026-09-12T14:18:12Z / 2026-09-12T15:34:29Z; 4570913 ms |
| Resultado | `subtype: success`, `is_error: False`, denegados 6 |
| Transcript | fuera del árbol; hashes en `transcripts.sha256` |

Verificación del orquestador tras la entrega: `cargo fmt`, `clippy --workspace
-D warnings`, analyzer (mcp-server) 34/34, protocol 53/53, application analyzer
18/18, release-smoke 9/9, arquitectura, links — todo verde.

## Informe literal del worker

## Task

W06 — implement `rust.analyzer.references` (M6-02) and `rust.analyzer.diagnostics` (M6-03) end to end, following the `rust.analyzer.symbols` shape, per the orchestrator's decisions (two-request `is_declaration` flow for references; pull-only native diagnostics with an in-band build-script oracle).

## Result

Both tools are fully implemented and wired in, tool count 32 → 34, and the 32 existing snapshots are byte-identical (verified — protocol tests pass unmodified for indices 0–31).

- **D1 (application)**: `ProjectRegistry::analyzer_references`/`analyzer_diagnostics` added; the shared capture/fingerprint/file-in-snapshot logic was refactored into private `analyzer_prelude`/`analyzer_finish` helpers, reused by `analyzer_symbols` without changing its behavior. New `AnalyzerRequestError::PositionOutOfRange`, validated against captured bytes via `LineIndex` before any port call (skipped when the file isn't UTF‑8, left to the port's own `FileNotUtf8`).
- **D2 (gateway)**: `AnalyzerQuery::References` now drives **two** `textDocument/references` requests in the same session (`includeDeclaration: true`/`false`), sharing the query-phase budget; `is_declaration` is computed by set difference and fed into the already-existing `lsp_codec::references_to_domain(..., declarations, ...)` helper (which turned out to already support this). `Diagnostics` was already fully implemented at the gateway (verified, untouched). Two new native `#[ignore]` cuts added: `m6-09-references` (`fixtures/valid-basic`) and `m6-10-diagnostics-build-script-oracle` (`fixtures/build-script`) — not run here (no Docker); `scripts/test-m6-runtime.py` discovers both automatically (verified by direct introspection, 9→11 selections).
- **D3 (tools)**: `rust.analyzer.references` and `rust.analyzer.diagnostics` added to `stdio/analyzer.rs`/`schemas.rs`, registered in `stdio.rs`, two new snapshots at indices 32/33, full failure-code mapping tables, no-peer-text tests, bootstrap-refusal tests, RESULT_LIMIT trim/fallback tests, input-validation and unknown-project-ref protocol tests, and two new `#[ignore]` end-to-end tests in `analyzer_runtime.rs`.
- **D4 (docs)**: `docs/tools.md`, `README.md`, `CHANGELOG.md`, `docs/security-model.md`, `docs/compatibility.md`, `docs/validation/M6/matrix.md`, ADR‑084 §2 phase 6, and the D25/D26 brief §2.2 all updated.

## Files changed

`crates/application/src/analyzer.rs`; `crates/execution-adapter/src/{analyzer_gateway.rs,analyzer_native.rs}`; `crates/mcp-server/src/stdio/analyzer.rs` (+`schemas.rs`, +`tests.rs`); `crates/mcp-server/src/stdio.rs`; `crates/mcp-server/tests/{protocol.rs,analyzer_runtime.rs,catalog_status.rs,crate_inspect.rs,crate_search.rs}`; two new snapshots (`analyzer-references-tool.json`, `analyzer-diagnostics-tool.json`); `scripts/{release-smoke.py,test-release-smoke.py}`; docs listed above.

## Tests executed (counts)

- `cargo fmt --all -- --check` — clean
- `cargo check --workspace --all-targets --locked --offline` — ok
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` — clean
- `cargo test -p rust-engineering-application analyzer` — 18/18 (16 new/refactored + 2 pre-existing unrelated `bloat` tests matched by substring)
- `cargo test -p rust-engineering-execution --lib analyzer_gateway` — 19/19 (17 pre-existing unmodified + 2 new)
- `cargo test -p rust-engineering-execution --lib lsp_codec` — 48/48, untouched
- `cargo test -p rust-engineering-mcp analyzer` — 34/34 (14 pre-existing symbols + 20 new references/diagnostics)
- `cargo test -p rust-engineering-mcp --test protocol --test catalog_status --test crate_inspect --test crate_search` — 59/59 (53+2+2+2)
- `cargo test -p rust-engineering-mcp --test analyzer_runtime --no-run` — compiles
- `python3 -B scripts/check-architecture.py` — PASS
- `python3 -B scripts/test-release-smoke.py` — 9/9
- `python3 -B scripts/test-m6-runtime-unit.py` — 19/19
- `python3 -B scripts/docs-hygiene.py links-check` — 0 broken in living documents (unchanged baseline)

## Evidence (snapshot hashes)

- `analyzer-references-tool.json`: `73b29c0d44700b65a167319798b30832090ef3928879ec4c7d550b8ebd681918`
- `analyzer-diagnostics-tool.json`: `19f3945b85f5cb5127e9a78dde77cfe39f21f0b0d42206bd831207ab57242b02`
- `TOOL_SCHEMA_SHA256` (release-smoke.py, `sha256(canonical_json({inputSchema,outputSchema}))`): references `e44acc84db266c56e86dcc7047df7b31ada99570eed992cac55b2e7bb3eb0d14`, diagnostics `6772c1df9ca83fe2a2beb9bbba062f19c2a8439892a6dbc0817497218af6b736`
- Native cut discovery verified by direct script introspection: 11 `#[ignore]`d tests in `analyzer_native.rs` (9 pre-existing + `m6-09-references`, `m6-10-diagnostics-build-script-oracle`), all mapped to a `Cut::open` cut name.

## Risks

- Native cuts `m6-09`/`m6-10` and the two `analyzer_runtime.rs` end-to-end tests are unrun here (no Docker) — genuine correctness rests on your native pass.
- Pre-existing gap, not introduced by W06: `lsp_codec::diagnostics_to_domain` hard-fails the *entire* diagnostics call (`ProtocolViolation`) if a single native diagnostic's `message`/`code` exceeds the domain's construction limits, rather than omitting/truncating that one entry. `rust.analyzer.diagnostics`'s wire-layer `bounded_message` sanitizes/truncates defensively, but can't reach a message that already failed conversion upstream. Fixing the root cause needs `lsp_codec.rs`, which W06 doesn't own.
- `OUTPUT_LIMIT` in `tests/protocol.rs` raised 2 MiB → 6 MiB (a test-harness ceiling on cumulative stdout across one test's repeated `tools/list` calls) — needed once 34 tools' schemas are discovered repeatedly in `metadata_errors_after_bootstrap_are_recoverable`; not a product change.

## Decisions

- **Domain left untouched.** `AnalyzerResult::References(Vec<Reference>)` keeps its existing tuple shape; I did not introduce the `omitted_external`/`omitted_visible` split D2's prose describes. D3's actual wire contract only asks for `omitted` + `omitted_declarations`, so I implemented exactly that, computed at the MCP tool layer.
- **`include_declaration` filtering happens in the tool layer**, not gateway/application: the gateway always fetches the full `is_declaration`-tagged set (both requests, always); the tool removes declarations from the visible list and counts `omitted_declarations` when the caller asked `false`. D1's "include_declaration filtering" unit test instead verifies the flag threads correctly into the `AnalyzerQuery` the fake port receives.
- **`request_for`/`convert` in `analyzer_gateway.rs` kept unmodified** for References (their existing unit tests stay green, unmodified); the real two-request runtime path is a new, separate `answer_references`/`request_references`/`reference_params` set of functions invoked directly from `protocol()`.
- **References "oversized" test replaced** with a "worst-case 512, still fits, no trim" test: unlike `DocumentSymbol` (free-form `detail` up to 1024 chars), `Reference` is fixed-shape and small, so the byte-budget trim loop can't be triggered at the real ≤512-entry invariant without violating the `maxItems: 512` wire schema.

## Open issues

- The brief says "the gate becomes 12 selections"; the actual discovery mechanism (`scripts/test-m6-runtime.py`, dynamic, not hardcoded) yields 11 (9 existing + 2 new). Flagging for the orchestrator rather than forcing a number.
- Native calibration of M6-02/M6-03 (both cuts, plus the two `analyzer_runtime.rs` tests) is pending the orchestrator's Docker-equipped run.
