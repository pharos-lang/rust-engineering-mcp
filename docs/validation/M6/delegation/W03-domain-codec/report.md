# W03 — informe del worker

| Campo | Valor |
| --- | --- |
| Modelo | Claude Sonnet 5 (`claude -p --model sonnet --effort high`), Claude Code 2.1.268 |
| Modelos observados | claude-haiku-4-5-20251001, claude-sonnet-5 |
| Inicio / fin (UTC) | 2026-09-11T22:26:48Z / 2026-09-11T23:39:39Z; 4365071 ms |
| Resultado | `subtype: success`, `is_error: False` |
| Permisos denegados | 2: `kill -9 35129 34458 37070 2>/dev/null; sleep 1; ps aux | grep -i "cargo test" | grep -v grep`; `set -e
echo "== fmt =="; cargo fmt --all -- --check
echo "== check =="; cargo check -p rust-engineer` |
| Transcript | fuera del árbol; hashes en `transcripts.sha256` |

Verificación del orquestador tras la entrega: `cargo check --workspace --all-targets --locked --offline`, `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` y `cargo fmt --all -- --check` en verde (el worker solo había compilado domain y execution-adapter; los `pub use analyzer::*` no crean ambigüedad aguas abajo).

## Informe literal del worker

All verification passes. Here is the mandatory report.

## Task

W03 — implement `crates/domain/src/analyzer.rs` (analyzer domain types, byte↔position translation) and `crates/execution-adapter/src/lsp_codec.rs` (bounded LSP base-protocol codec, typed DTO subset, hostile-peer tests), per the D25/D26 decision brief.

## Result

Both deliverables implemented, tested, and green against the specified targeted verification. No workspace-wide gate run (per instructions). No commits made.

## Files changed

- `crates/domain/src/analyzer.rs` (new) — domain types, `LineIndex`, `apply_edits`, limits, tests.
- `crates/domain/src/lib.rs` — added `mod analyzer; pub use analyzer::*;`.
- `crates/execution-adapter/src/lsp_codec.rs` (new) — `Decoder`/`encode`/`Correlator`, LSP wire DTOs, conversion helpers, tests.
- `crates/execution-adapter/src/lib.rs` — added `pub mod lsp_codec;`.
- No `Cargo.toml`, `supervisor.rs`, `rust_gateway.rs`, or MCP server changes.

## Tests executed (with counts)

- `cargo fmt --all -- --check` — clean.
- `cargo check -p rust-engineering-domain -p rust-engineering-execution --all-targets --locked --offline` — clean.
- `cargo clippy -p rust-engineering-domain -p rust-engineering-execution --all-targets --locked --offline -- -D warnings` — clean, zero warnings.
- `cargo test -p rust-engineering-domain --locked --offline analyzer` — **26/26 passed**.
- `cargo test -p rust-engineering-execution --locked --offline --lib lsp_codec` — **32/32 passed**.
- `python3 -B scripts/check-architecture.py` — PASS.
- Manual grep confirms zero `unwrap()`/`expect(`/`panic!`/`unsafe` in either delivered file (one false-positive-looking hit is the unrelated `Correlator::expect()` method name).

## Evidence

Domain tests cover every rejection named in the brief: `AnalyzerFile` path/`.rs` validation, `TextRange` inversion, `LineIndex` ASCII/BMP/astral/emoji/combining/CRLF/BOM/empty/trailing-newline/offset==len, UTF-16 surrogate-pair and UTF-8 line-relative-byte encodings (both directions), `SymbolKind`/`DiagnosticSeverity` closed ranges, `DocumentSymbol` selection/depth invariants, `AnalyzerDiagnostic` code/message/related bounds, `CodeActionKind` LSP↔MCP string round-trips, `apply_edits` (multi-edit, touching, overlap, non-UTF-8), `Completeness`'s `Complete ⇒ no limit_visible` invariant (constructor and wire), `AnalyzerIdentity` JSON round-trip.

Codec tests cover the full hostile-peer list: oversized/zero/negative/non-numeric/overflowing `Content-Length`, missing blank line, LF-only separators, 9-header and 2 KiB-header caps, short-body-then-EOF, invalid UTF-8 body, JSON array batch, 200-deep nested JSON (no panic), 4096-message flood, byte-total cap, both-result-and-error / missing-`jsonrpc` / non-integer-id malformed messages, unknown/duplicate/late correlator responses, server-request answered with `-32601` never a result, encode/decode round-trip, `config_digest` stability, `InitializeParams` fixed shape, URI scoping (including a genuine percent-encoded traversal), document-symbol depth-first flattening, cross-file related-info omission, multi-file references with declaration/external omission, and every `WorkspaceEdit` rejection (`createFile`, snippet, bare `Command`, `Command`-bearing `CodeAction`, external URI, overlap, version mismatch) plus a clean multi-edit that round-trips through `domain::apply_edits`.

## Risks

- `references_to_domain`'s `is_declaration` flag is caller-supplied (a `declarations: &[Location]` slice), because the bare LSP `textDocument/references` response carries no such flag itself; W06 (which owns the `references` tool) must decide how it identifies the declaration location before calling this.
- `diagnostics_to_domain` silently omits related-information entries that point outside the diagnosed file, since M6-01 opens only one document per query and this layer has only that file's `LineIndex`. This is documented in the function's doc comment; if a future package needs cross-file related spans it will need to pass a multi-file index map instead.
- `IncompleteReason` is deliberately minimal (4 variants named literally in the brief). The brief's "…" signals more will be needed; adding variants later is a normal, safe enum extension since nothing has shipped externally yet.

## Decisions

- `AnalyzerLimits` implemented as bare `pub const` items in `analyzer.rs` (not a wrapper struct), matching `source.rs`'s existing `SOURCE_MAX_*` convention.
- `CodeActionKind`'s wire spelling uses explicit `#[serde(rename = "...")]` per variant (`quickfix`, `refactor_extract`, …) rather than `rename_all = "snake_case"`, because the derived snake_case would produce `quick_fix` (wrong) instead of the brief's `quickfix`.
- `config_digest()` returns `Result<SourceFingerprint, AnalyzerError>` rather than a bare `SourceFingerprint` as the brief's prose suggests, to honor "no unwrap/expect/panic": the sha256-hex-to-fingerprint parse is provably infallible but only `?`-propagatable, not unwrap-able. Mirrors the existing `digest(...).parse().map_err(...)?` pattern in `execution-adapter/src/lib.rs`.
- Added `Display`/`Error` impls for `AnalyzerError` and `CodecError` (not explicitly requested) — cheap, matches `ContractError`'s existing precedent, and was needed for ergonomic `?`-based tests without `unwrap`/`expect`.
- `WorkspaceSymbol`/`SymbolInformation` collapsed to one wire struct (`SymbolInformation`) since our client capabilities never advertise `resolveSupport`, so rust-analyzer always returns the full `{uri, range}` location; documented in that struct's doc comment.
- A missing `LineIndex` entry for an otherwise in-scope file during code-action resolution maps to `ActionRejection::NotUtf8` (the brief's own terminology for "captured bytes not usable"), rather than adding an undocumented new rejection reason.

## Open issues

- None blocking. W04 (session lifecycle) and W06 (references/workspace-symbol tool) should read the two "Risks" items above before building on `references_to_domain`/`diagnostics_to_domain`.
