# W06b — Apply the V06 dispositions to `rust.analyzer.references`/`.diagnostics` (W06)

Model requested: Claude Sonnet 5 (`claude -p --model sonnet --effort high`). Role: implementation worker, same ownership as W06 **plus** `crates/execution-adapter/src/lsp_codec.rs` and `crates/domain/src/analyzer.rs` (additive only). Orchestrator: Claude Fable 5.1 (running as Opus 4.8). You may not spawn subagents. **Never run a command in the background** (the `-p` session ends at the end of your turn and is not resumed); run everything in the foreground and end with the report as your last message. Do not run Docker-backed tests. Do not commit. Read `docs/validation/M6/delegation/V06-review-references-diagnostics/disposition.md` first — it is binding; the reviewer text is context.

## Changes (all mandatory; numbering follows the disposition)

1. **P1 — diagnostics must never hard-fail on peer content.** In `crates/execution-adapter/src/lsp_codec.rs::diagnostics_to_domain`, make every `domain::AnalyzerDiagnostic` construction succeed for any peer input by fitting the content to the domain bounds **before** construction: truncate `message` to `MAX_DIAGNOSTIC_MESSAGE_CHARS` (4096) with a truncation signal, truncate `code` to `MAX_DIAGNOSTIC_CODE_CHARS` (128), cap `related` at `MAX_RELATED_INFORMATION` (32) counting the dropped ones, and an **empty** `message` entry is **omitted and counted** (reuse `domain::OmissionKind::OversizedEntry`). Return the omission/truncation counts so the tool marks `completeness`. A diagnostic whose primary range fails to map (encoding/position) stays an error of the whole call only if it is the *only* signal — prefer: skip that entry and count it, never `ANALYZER_CRASHED`. After this, `analyzer.rs::diagnostics_failure_code`'s `ProtocolViolation` arm must no longer be reachable from over-limit content (keep it for genuine protocol violations: batch, malformed frame). Add a `domain::AnalyzerDiagnostic` truncation flag (`message_truncated`) if you need to surface truncation on the wire (see item 8). Tests in the codec: message 5000 chars → truncated+flagged, not error; empty message → omitted+counted; code 200 chars → truncated; 40 related → 32 kept + 8 counted; a 500-diagnostic answer → 512 cap already exists, keep it.

2. **P2 — sanitize `code`.** In `crates/mcp-server/src/stdio/analyzer.rs::wire_diagnostic`, run the same control-character sanitization on `code` that `bounded_message` runs on `message` (replace controls except `\n`/`\t`). Test with a `code` containing `\u{1b}`/`\u{0}`.

3. **P2 — cap `related`.** Covered by item 1 (cap at 32 in the codec). Ensure the wire schema's `maxItems: 32` can never be violated by construction; the tool's `related` collection is bounded upstream. Test that a domain diagnostic with 32 related converts and validates against its own output schema.

4. **P2 — bound the reference vectors before the set difference.** In `crates/execution-adapter/src/analyzer_gateway.rs::answer_references`/`request_references`, cap each decoded `Vec<Location>` to a fixed ceiling (`MAX_VISIBLE_RESULTS * 2` = 1024) **before** the O(n·m) difference; count the excess as an omission. Document that the difference runs on bounded input. Test the bound.

5. **P2 — really test `answer_references`.** Add a test that drives `answer_references` (or the smallest seam that runs its real body) with a fake two-response peer: response 1 (`includeDeclaration:true`) = {A_decl, B_use, C_use}, response 2 (`false`) = {B_use, C_use}; assert `is_declaration` true only for A, the shared query budget is consumed across both, and a timeout on the **second** request classifies as `TIMEOUT_QUERY` (not a silent drop). The test must fail if `answer_references` were deleted or the difference inverted.

6. **P2 — the build-script oracle must discriminate.** In `analyzer_native.rs::m6-10` and the `analyzer_runtime.rs` diagnostics e2e test on `fixtures/build-script`: assert on the **specific** unresolved diagnostic, not merely "a diagnostic on line 1" or the constant `source=="rust-analyzer"`. Assert the diagnostic's `code` is the exact rust-analyzer code for an unresolved include/macro/extern-crate caused by the missing `OUT_DIR` (record the observed `code` in the receipt and assert it; the orchestrator will confirm the exact string on the calibration run — write the assertion against a small closed set `{"unresolved-macro-call","unresolved-extern-crate","unresolved-import","macro-error"}` and record which one fired), and/or that its `message`/related references the `include!`/`env!`/`OUT_DIR`. Add the `completeness.state == complete` assertion. If `fixtures/build-script/src/lib.rs` is one line, either add a second line so `line==1` is meaningful, or drop the line check entirely and rely on the code/message discriminator. Do not weaken to pass — if the real diagnostic turns out different, stop and report so the orchestrator sees it on the calibration run.

7. **P3 — remove the dead `include_declaration` gateway path.** The gateway always fetches both requests, so `AnalyzerQuery::References`'s `include_declaration` is unused there. Remove it from the gateway query (and the dead `request_for` References arm); keep `include_declaration` in the tool→application request where the filtering actually happens. Fix `crates/application/src/analyzer.rs`'s `references_include_declaration_is_threaded...` test to assert the real behaviour (the tool removes declarations and counts them), not a dead thread. If `AnalyzerQuery::References` is a domain type, this is an additive-compatible field removal on an unshipped enum — fine.

8. **P3 — `related` truncation flag.** Add `message_truncated` to the `RelatedInformation` wire struct and set it when a related message is truncated (mirrors the parent diagnostic); update the snapshot.

9. **P3 — `omitted` totals.** In `analyzer.rs`'s `references_execution_outcome`/`diagnostics_execution_outcome` (and the symbols one if it shares the helper), make `omitted` the sum of **all** `completeness.omissions[].count`, not only `LimitVisible`. The per-kind breakdown stays in `completeness.omissions[]`. Keep `rust.analyzer.symbols`'s observable output unchanged if its omissions are only `LimitVisible` today (verify: for symbols the total equals the old value, so no snapshot churn there).

10. **P3 — native file-aware slicing.** In `m6-09-references`, index each reference's range into the bytes of `reference.file`, not always `src/lib.rs`.

11. **P3 — utf-8 confirm.** Confirm `convert()` and `answer_references` both use `PositionEncoding::Utf8` consistently; no change if they match, note it in the report.

12. **P3 — docs.** In `docs/tools.md`: (a) references consumes two LSP messages and shares the query budget, so it can be less available than symbols at the same `timeout_seconds`; (b) the bootstrap `blocked/SANDBOX_DENIED` vs runtime `unavailable/SANDBOX_DENIED` distinction for all three analyzer tools; (c) diagnostics message/code are bounded and control-sanitized, over-limit entries truncated/omitted and declared in `completeness`, never a call failure.

13. **P3 — `OUTPUT_LIMIT` comment** in `tests/protocol.rs`: record the measured high-water mark of the 34-tool `tools/list` output next to the 6 MiB ceiling.

14. **P3 — `#[allow]` scope.** Narrow the `#[allow(clippy::expect_used, clippy::unwrap_used)]` so it does not blanket the M6-01 symbols tests (apply per-test or to the new block only).

15. **Snapshots + pins.** Regenerate `analyzer-references-tool.json` and `analyzer-diagnostics-tool.json` from the real `tools/list` (they change with items 2/3/8), update their two hashes in `scripts/release-smoke.py`/`test-release-smoke.py`; the other 32 snapshots and hashes stay byte-identical (assert; stop and report otherwise).

## Deferred (do NOT do): the ~1400-line triplication refactor (P3) — the orchestrator tracked it as debt for M6-06.

## Verification (targeted, foreground)

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked --offline
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo test -p rust-engineering-domain --locked --offline analyzer
cargo test -p rust-engineering-execution --locked --offline --lib lsp_codec
cargo test -p rust-engineering-execution --locked --offline --lib analyzer_gateway
cargo test -p rust-engineering-application --locked --offline analyzer
cargo test -p rust-engineering-mcp --locked --offline analyzer
cargo test -p rust-engineering-mcp --locked --offline --test protocol --test catalog_status --test crate_inspect --test crate_search
cargo test -p rust-engineering-mcp --locked --offline --test analyzer_runtime --no-run
python3 -B scripts/check-architecture.py
python3 -B scripts/test-release-smoke.py
python3 -B scripts/test-m6-runtime-unit.py
python3 -B scripts/docs-hygiene.py links-check
```

## Constraints

No new dependencies, no `Cargo.toml` changes, no `unwrap`/`expect`/`panic!`/`unsafe` (except inside `#[cfg(test)]` as the codebase already does). Domain/application stay free of `serde_json`/`rmcp`/process APIs. Report with headings: Task / Result / Files changed / Tests executed (counts) / Evidence (both new snapshot hashes) / Risks / Decisions / Open issues.
