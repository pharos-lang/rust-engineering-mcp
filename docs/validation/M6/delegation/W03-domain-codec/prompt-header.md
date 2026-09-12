# W03 — Analyzer domain types, byte↔position translation, and the bounded LSP codec with hostile-peer tests

Model requested: Claude Sonnet 5 (`claude -p --model sonnet --effort high`). Role: implementation worker with disjoint file ownership. Orchestrator: Claude Fable 5.1 (decides; does not write code). This package contains no rust-analyzer binary and no Docker: everything here is pure Rust with unit tests. The real-binary lifecycle is a later package (W04, Opus) that will consume your types and codec unchanged, so precision of contracts matters more than breadth.

## Read first

- `AGENTS.md` (architecture rules; `unwrap`/`expect`/`panic` are denied by workspace lints; `unsafe_code = "deny"`).
- `docs/validation/M6/delegation/D25-D26-decision-brief.md` — binding decisions; especially §0, §1.3–1.4, §1.6, §2.4, §2.5 and §4.
- `crates/domain/src/diagnostic.rs` (`Position`, `ByteRange`, `SourceSpan`), `crates/domain/src/source.rs` (`validate_source_path`, `SourceFile`, `SourceBundle`), `crates/domain/src/value.rs` (`ContractError`, newtype macro), `crates/domain/src/lib.rs` (module pattern; `serde_json::Value` is forbidden in domain/application by `scripts/check-architecture.py`).
- `crates/execution-adapter/src/supervisor.rs` and `lib.rs` (module conventions; `serde_json` is available in the execution adapter).

## Deliverable 1 — `crates/domain/src/analyzer.rs` (+ `mod analyzer; pub use analyzer::*;` in `crates/domain/src/lib.rs`)

Serde-only domain values with validated constructors (no I/O, no `serde_json`):

- `AnalyzerFile(String)`: relative POSIX path validated by `validate_source_path` plus `.rs` suffix; `as_str()`.
- `TextRange { start: Position, end: Position }` with `start <= end`; reuse `Position` (1-based line, 1-based Unicode-scalar column).
- `LineIndex`: built from `&[u8]` (must be valid UTF-8 → `AnalyzerError::NotUtf8` otherwise); newline is `\n` only (`\r` stays inside the line, exactly like rust-analyzer's `LineIndex`). Methods: `position_from_byte_offset(usize) -> Result<Position, AnalyzerError>` (rejects offsets past the end or inside a code point → `OffsetInsideCodePoint`), `byte_offset_from_position(Position) -> Result<usize, AnalyzerError>` (rejects out-of-range), `position_from_utf16(line0: u32, col16: u32) -> Result<Position, AnalyzerError>` and `position_from_utf8(line0: u32, col8: u32)` (LSP zero-based line/character to public `Position`; utf-8 character = byte offset within the line, utf-16 = code units; both reject offsets inside a code point or surrogate pair), and the inverse `utf8_from_position`. Exhaustive tests: ASCII, BMP (`é`), astral (`𝄞`, emoji), combining sequences, CRLF, BOM at start, empty file, trailing newline vs none, offset == len (allowed as end position).
- `SymbolKind`: closed enum with the 26 LSP `SymbolKind` values, `TryFrom<u32>` (LSP numbers 1..=26) returning `AnalyzerError::UnknownSymbolKind` otherwise; serde `snake_case`.
- `DocumentSymbol { name: NonEmptyText, kind, detail: Option<String>, deprecated: bool, range: TextRange, selection_range: TextRange, depth: u8 }` with `selection_range` inside `range` and `depth <= 32` validated.
- `WorkspaceSymbol { name, kind, container: Option<String>, file: AnalyzerFile, range: TextRange }`.
- `Reference { file: AnalyzerFile, range: TextRange, is_declaration: bool }`.
- `DiagnosticSeverity { Error, Warning, Information, Hint }` with `TryFrom<u32>` (1..=4).
- `AnalyzerDiagnostic { file, range, severity, code: Option<String> (≤128 chars), message: NonEmptyText (≤4096 chars), related: Vec<RelatedInformation{file, range, message}> (≤32) }`.
- `CodeActionKind { QuickFix, Refactor, RefactorExtract, RefactorInline, RefactorRewrite, Source, SourceOrganizeImports }` with `from_lsp(&str) -> Option<Self>` (exact strings `quickfix`, `refactor`, `refactor.extract`, `refactor.inline`, `refactor.rewrite`, `source`, `source.organizeImports`) and `to_lsp()`.
- `ActionRejection { Command, Snippet, ResourceOperation, ExternalUri, VersionMismatch, OverlappingRanges, EditLimit, BytesLimit, NotUtf8, UnresolvedEdit }`.
- `TextEdit { file: AnalyzerFile, range: TextRange, new_text: String }` and `pub fn apply_edits(before: &[u8], edits: &[(TextRange, &str)]) -> Result<Vec<u8>, AnalyzerError>`: edits are sorted by start, must not overlap (touching allowed: `a.end == b.start`), applied against the `LineIndex` of `before`; result bytes; errors `OverlappingRanges`, out-of-range. Property-style tests with several edits, empty replacements, insertions at end, and the overlap rejection.
- `OmissionKind { ExternalUri, SysrootLocation, DependencyLocation, LimitVisible, NotUtf8File, UnresolvablePosition }`, `Omission { kind, count: u32 }`, `Completeness { state: CompletenessState (Complete|Incomplete), omissions: Vec<Omission>, reasons: Vec<IncompleteReason> }` where `IncompleteReason { AnalyzerNotReady, LimitVisible, SysrootWarning, Timeout, ... }` — keep the enum closed and documented; constructor `Completeness::complete()` and `Completeness::incomplete(reasons)`; invariant: `Complete` ⇒ no `LimitVisible` omission.
- `AnalyzerLimits` as `pub const`s: `MAX_VISIBLE_RESULTS = 512`, `MAX_ACTIONS = 32`, `MAX_EDITS = 128`, `MAX_FRAME_BYTES = 1 MiB`, `MAX_MESSAGES_PER_JOB = 4096`, `MAX_STDOUT_BYTES = 16 MiB`, `MAX_STDERR_BYTES = 1 MiB`, `INITIALIZE_TIMEOUT_SECONDS = 60`, `QUERY_TIMEOUT_SECONDS = 30`, `TOTAL_TIMEOUT_SECONDS = 180`, `MAX_RESULT_BYTES = 512 KiB`.
- `AnalyzerIdentity { version: NonEmptyText, binary_sha256: SourceFingerprint (reuse the existing `sha256:` fingerprint newtype), image_id: NonEmptyText, config_digest: SourceFingerprint, position_encoding: PositionEncoding (Utf8|Utf16) }`.
- `AnalyzerError` closed enum (`NotUtf8`, `OffsetInsideCodePoint`, `OutOfRange`, `InvalidRange`, `OverlappingRanges`, `UnknownSymbolKind`, `UnknownSeverity`, `Invalid`, `LimitExceeded`).

Tests live in the same crate (`#[cfg(test)] mod tests` or `crates/domain/tests/analyzer.rs` — your choice, but discriminating: every rejection above has a test).

## Deliverable 2 — `crates/execution-adapter/src/lsp_codec.rs` (+ `pub mod lsp_codec;` in `crates/execution-adapter/src/lib.rs`)

A bounded, incremental LSP base-protocol codec plus typed DTOs for exactly the subset M6 uses. Pure functions over bytes; no process, no I/O, no async.

- Framing: `Decoder::new(limits)` with `feed(&mut self, bytes: &[u8]) -> Result<Vec<RawMessage>, CodecError>` that accumulates and yields complete frames. Headers: `Content-Length` mandatory, decimal ≤ `MAX_FRAME_BYTES` (1 MiB), optional `Content-Type`; at most 8 header lines, ≤ 1 KiB of header bytes, header names ASCII, `\r\n` separators only; anything else → `CodecError::MalformedHeader` (fatal: the session must be killed, so make the error kinds carry `is_fatal()`). Body is parsed with `serde_json::from_slice` into a typed `RawMessage` enum `{ Request{id, method, params}, Response{id, result|error}, Notification{method, params} }` (params/result as `serde_json::Value` is allowed here); a JSON array (batch) → `CodecError::BatchRejected`; a message with both `result` and `error`, missing `jsonrpc: "2.0"`, non-string/non-integer id → `MalformedMessage`. Counters: total messages (≤ 4096 → `MessageLimit`), total body bytes (≤ 16 MiB → `ByteLimit`).
- `encode(message: &OutgoingMessage) -> Vec<u8>` producing `Content-Length: N\r\n\r\n<json>`; `OutgoingMessage` covers `Request{id: RequestId, method, params}`, `Notification{method, params}`, and `Response` for answering server→client requests with error `-32601` (`method not found`) — never a result.
- `Correlator`: allocates monotonically increasing integer ids; `expect(id)` registers an outstanding request; `accept(response) -> Result<Matched, CodecError>` rejects unknown ids (`UnknownResponseId`), duplicates (`DuplicateResponse`), and responses to ids already timed out (`LateResponse` — non-fatal, counted).
- Typed DTOs (serde, `deny_unknown_fields` **off** for server-provided structs because rust-analyzer adds fields; **on** for what we send): `InitializeParams` builder producing exactly the brief §4.3 client capabilities and §4.5 `initializationOptions` (write `pub fn initialization_options() -> serde_json::Value` and `pub fn config_digest() -> SourceFingerprint` = sha256 of the canonical (sorted-key, compact) JSON — `sha2` is a workspace dependency), `InitializeResult` reading `capabilities.positionEncoding: Option<String>`, `ServerStatusParams { health: String, quiescent: bool, message: Option<String> }`, `DidOpenTextDocumentParams`, `DocumentSymbolParams`, LSP `DocumentSymbol` (hierarchical, with `children`), `SymbolInformation`, `WorkspaceSymbolParams`, `WorkspaceSymbol`/`SymbolInformation` union, `ReferenceParams`, `Location`, `DocumentDiagnosticParams`, `DocumentDiagnosticReport` (only the `full` kind is accepted; `unchanged` → error), LSP `Diagnostic` with `relatedInformation`, `CodeActionParams`, `CodeActionOrCommand` (a bare `Command` object or a `CodeAction` with a `command` field must be detectable → `ActionRejection::Command`), `WorkspaceEdit` with both `changes` and `documentChanges` (`TextDocumentEdit` with `textDocument.version: Option<i64>`, plus the resource-operation variants `create`/`rename`/`delete` detectable → `ActionRejection::ResourceOperation`), `TextEdit`/`AnnotatedTextEdit`/`SnippetTextEdit` (`insertTextFormat == 2` → `ActionRejection::Snippet`), `ShutdownParams` (none), `ExitParams` (none), LSP `ResponseError { code: i64, message, data }` with constants `-32800`, `-32801`, `-32601`.
- Conversion helpers to domain (`rust_engineering_domain::analyzer`): `fn lsp_uri_to_file(uri: &str) -> Result<AnalyzerFile, UriRejection>` accepting **only** `file:///source/<rel>` (percent-decoding, no `..`, no `//`, rejecting anything else with `UriRejection::External` — `/opt/rust/...` sysroot paths are `External` too, the caller counts them as omissions), `fn lsp_range_to_text_range(range, &LineIndex, PositionEncoding) -> Result<TextRange, AnalyzerError>`, `fn document_symbols_to_domain(Vec<DocumentSymbol>, &LineIndex, encoding) -> Result<(Vec<domain::DocumentSymbol>, usize truncated), _>` flattening depth-first with `depth` and a hard cap of 512 visible, `fn diagnostics_to_domain(...)`, `fn references_to_domain(...)` (external URIs omitted and counted), `fn code_actions_to_candidates(...) -> Vec<Result<ResolvedAction, ActionRejection>>` where `ResolvedAction { title, kind, is_preferred, edits: Vec<domain::TextEdit>, versions: BTreeMap<AnalyzerFile, Option<i64>> }` and every rejection reason of the brief is exercised.

Hostile-peer tests (bytes in, errors out; at least these): Content-Length larger than 1 MiB; Content-Length `0`, negative, non-numeric, overflowing `u64`; missing blank line; `\n`-only separators; 9 headers; 2 KiB header; body shorter than declared then EOF; body with invalid UTF-8; JSON array batch; deeply nested JSON (serde_json's recursion limit must surface as `MalformedMessage`, not a panic); response with unknown id; duplicate response; response after timeout; request from server (`window/workDoneProgress/create`) → the encoder can answer `-32601`; notification flood past 4096 messages; a `WorkspaceEdit` with `createFile`, with a snippet edit, with a `command`, with an external URI, with overlapping edits, with a version mismatch (expect `VersionMismatch` when the edit's version ≠ 1), and one clean multi-edit that converts to domain edits and round-trips through `apply_edits`.

## Verification you must run (targeted only — no workspace-wide test, no gate)

```text
cargo fmt --all -- --check
cargo check -p rust-engineering-domain -p rust-engineering-execution --all-targets --locked --offline
cargo clippy -p rust-engineering-domain -p rust-engineering-execution --all-targets --locked --offline -- -D warnings
cargo test -p rust-engineering-domain --locked --offline analyzer
cargo test -p rust-engineering-execution --locked --offline --lib lsp_codec
python3 -B scripts/check-architecture.py
```

## Constraints

- Own only: `crates/domain/src/analyzer.rs`, the one `mod`/`pub use` line in `crates/domain/src/lib.rs`, optionally `crates/domain/tests/analyzer.rs`, `crates/execution-adapter/src/lsp_codec.rs`, the one `pub mod lsp_codec;` line in `crates/execution-adapter/src/lib.rs`. Nothing else — no `Cargo.toml` changes, no new dependencies, no changes to `supervisor.rs`/`rust_gateway.rs`, no MCP server code.
- No `unwrap`/`expect`/`panic!`/`unsafe`; no `serde_json` in domain; doc comments explain contracts (what is rejected and why), not narration.
- Do not commit.

## Report (mandatory headings)

Task / Result / Files changed / Tests executed (with counts) / Evidence / Risks / Decisions / Open issues.
