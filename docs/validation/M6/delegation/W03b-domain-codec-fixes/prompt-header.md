# W03b — Fix the findings of the independent review V03 on the analyzer domain and LSP codec (W03)

Model requested: Claude Sonnet 5 (`claude -p --model sonnet --effort high`). Role: implementation worker, same file ownership as W03 (`crates/domain/src/analyzer.rs`, `crates/execution-adapter/src/lsp_codec.rs`; plus the two doc edits in item 2). Orchestrator: Claude Fable 5.1. The orchestrator accepted every finding of `docs/validation/M6/delegation/V03-review-domain-codec/claude-sonnet-5-review.md`; apply them as specified below (adjustments are the orchestrator's decisions, not the reviewer's).

## Changes (all mandatory)

1. **P0 — unbounded recursion in `walk_document_symbol`.** Enforce `depth <= 32` **before** recursing, on every path (including after the 512 cap is reached): a deeper tree is not a panic and not a stack overflow but a counted omission (`truncated`/`limit_visible`) — stop descending. Use checked arithmetic for `depth` (no `+ 1` on `u8` without a bound). Add a test with ≥ 512 flat siblings followed by a 10 000-deep chain: must return `Ok` with the omission count and never recurse past 32.
2. **P1 — missing `textDocument.codeAction` client capability.** Add to `client_capabilities()`:
   `"codeAction": { "codeActionLiteralSupport": { "codeActionKind": { "valueSet": ["quickfix", "refactor", "refactor.extract", "refactor.inline", "refactor.rewrite", "source", "source.organizeImports"] } }, "isPreferredSupport": true, "dataSupport": false, "disabledSupport": false }` — still **no** `resolveSupport`. Add a test that the emitted `InitializeParams` JSON contains exactly these keys. Then amend the two documents that list the capabilities so they stay truthful: append the `codeAction` literal-support line to `docs/adr/ADR-084-rust-analyzer-runtime-and-lsp-lifecycle.md` §4 and to `docs/validation/M6/delegation/D25-D26-decision-brief.md` §4.3 (one sentence each, no other edits to those files).
3. **P2 — O(n²) header rescans.** Keep the decoder incremental: remember the header-end offset / parsed `Content-Length` between `feed()` calls so partial bodies are not rescanned from byte 0. Add a test that feeds a 1 MiB frame one byte at a time and completes within a bounded number of scans (assert via a counter exposed under `#[cfg(test)]`, or by timing-free structural assertion).
4. **P2 — `NotUtf8` used as a catch-all.** Add `domain::ActionRejection::FileNotInSnapshot` and use it when the edit's file has no `LineIndex` in `indices`; keep `NotUtf8` only for genuine UTF-8 failures. Document that callers must pass indices for **every** captured `.rs` file (multi-file actions are legitimate in M6-04). Tests for both reasons.
5. **P2 — config keys.** Keep `references.excludeTests` and `lru.capacity` (they exist in the configuration reference); add a doc comment on `initialization_options()` that says every key is verified against the real binary's `--print-config-schema` by the W04 native calibration (that is the oracle, not this comment).
6. **P3 — `Content-Length` parsing.** Accept ASCII digits only (no `+`, no whitespace, no leading zeros beyond a single `0`); tests for `+10`, ` 10`, `010`, `0x10`.
7. **P3 — `parse_body` strictness.** A message with `method` **and** (`result` or `error`) → `MalformedMessage`; unknown top-level keys → `MalformedMessage` (the only allowed keys are `jsonrpc`, `id`, `method`, `params`, `result`, `error`). Tests.
8. **P3 — edit ordering tie-break.** Sort edits by `(start, end)`; two edits with the same `start` where either is non-empty → `OverlappingRanges`; two zero-width insertions at the same position → `OverlappingRanges` as well (order would be ambiguous). Apply in both `domain::apply_edits` and `resolve_action`. Tests, including unsorted input order.
9. **P3 — per-item isolation of code actions.** Deserialize the `textDocument/codeAction` result as `Vec<serde_json::Value>` and convert each element separately so one malformed element becomes `ActionRejection::UnresolvedEdit` for that element only; fix the doc comment accordingly. Test with one bad element among good ones.
10. **P3 — poisoned decoder.** After a fatal `CodecError`, every further `feed()` returns that same error (`poisoned` flag); test.
11. **P3 — `workspace_symbols_to_domain`.** Add the conversion (`Vec<SymbolInformation>` → `Vec<domain::WorkspaceSymbol>` + omission counts for external URIs, ≤ 512 visible, deterministic order by `(file, range.start, name)`); tests.
12. **Tests the reviewer found missing.** `EditLimit` (129 edits) and `BytesLimit` (result > 512 KiB) rejections; `apply_edits` with unsorted input.
13. **Orchestrator finding — `config_digest`.** Replace `unwrap_or_default()` with error propagation (`AnalyzerError::Invalid` on serialization failure); same in `encode` if applicable (never digest or emit empty bytes silently).

## Verification (targeted)

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked --offline
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo test -p rust-engineering-domain --locked --offline analyzer
cargo test -p rust-engineering-execution --locked --offline --lib lsp_codec
python3 -B scripts/check-architecture.py
python3 -B scripts/docs-hygiene.py links-check
```

If a `cargo test` binary hangs at start (0 % CPU) on this host, delete that binary under `target/debug/deps/` and re-run; do not kill unrelated processes.

## Constraints

Same as W03: no `Cargo.toml` changes, no new dependencies, no `unwrap`/`expect`/`panic!`/`unsafe`, no `serde_json` in domain, doc comments state contracts. Do not commit.

## Report (mandatory headings)

Task / Result / Files changed / Tests executed (with counts) / Evidence / Risks / Decisions / Open issues.
