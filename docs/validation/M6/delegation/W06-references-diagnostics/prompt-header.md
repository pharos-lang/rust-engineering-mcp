# W06 — `rust.analyzer.references` (M6-02) and `rust.analyzer.diagnostics` (M6-03) end to end

Model requested: Claude Sonnet 5 (`claude -p --model sonnet --effort high`). Role: implementation worker with disjoint file ownership. Orchestrator: Claude Fable 5.1. You may not spawn subagents. **Never run a command in the background** (the `-p` session ends at the end of your turn and is not resumed); run everything in the foreground and finish with the report as your last message. Do not commit. Any `--root` you pass to the server in a test must be a canonicalized path (the server refuses `..`). Everything here follows the shape `rust.analyzer.symbols` already has (W05/W05b, committed): read `crates/application/src/analyzer.rs`, `crates/mcp-server/src/stdio/analyzer.rs` (+ `schemas.rs`, `tests.rs`), `crates/mcp-server/tests/analyzer_runtime.rs`, `docs/tools.md` "Contratos M6 — analyzer", and the gateway API in `crates/execution-adapter/src/analyzer_gateway.rs` (`AnalyzerQuery::{References, Diagnostics}` already exist and are calibrated) before writing anything. Binding contract: ADR-083 §2–§4; lifecycle and budgets: ADR-084.

## Orchestrator decisions you implement (do not reopen)

- **`is_declaration` for references (R5 of `docs/validation/M6/01.md`).** rust-analyzer does not flag the declaration in a `textDocument/references` answer. Decision: in the same session, send **two** `textDocument/references` requests — `includeDeclaration: true` and `includeDeclaration: false` — and mark as `is_declaration` every location present only in the first. This changes `AnalyzerQuery::References` handling in `analyzer_gateway.rs` (you own that change, additive: a second request in phase 6, both inside the query budget) and needs a one-paragraph dated amendment in ADR-084 §2 phase 6 ("una consulta; para references, dos peticiones …") and in the brief §2.2. If the client asked `include_declaration: false`, the declaration locations are removed from the visible list but still counted in `omitted_declarations`.
- **Diagnostics are pull only** (`textDocument/diagnostic`, full report), native rust-analyzer diagnostics only; the M6-03 native cut must prove the **in-band build-script oracle** promised in `01.md` R1: on `fixtures/build-script` (real `build.rs` with `include!(concat!(env!("OUT_DIR"), …))`), with build scripts disabled the analyzer cannot resolve that include and the diagnostics answer contains the corresponding unresolved diagnostic — that is deterministic proof that no build script ran, complementary to the `container top` sampling. Record the diagnostic `code` observed.
- **Tool count 32 → 34**; the 32 existing snapshots stay byte-identical.

## Deliverables

### D1 — Application (`crates/application/src/analyzer.rs`, additive)

`ProjectRegistry::analyzer_references(reference, ReferencesRequest{file, position, include_declaration, expected_project_fingerprint, timeout_seconds}, port, control)` and `analyzer_diagnostics(reference, DiagnosticsRequest{file, expected_project_fingerprint, timeout_seconds}, port, control)` sharing the capture/fingerprint/file-in-snapshot logic with `analyzer_symbols` (refactor into one private helper; the symbols behaviour must not change — its tests stay green unmodified). Positions are validated against the captured bytes **before** the port call: a `position` beyond the file's line count or column count → `AnalyzerRequestError::PositionOutOfRange` (new variant), no session started. Unit tests with the fake port for both, including the out-of-range case and `include_declaration` filtering.

### D2 — Gateway (additive, `crates/execution-adapter/src/analyzer_gateway.rs` + `analyzer_native.rs`)

- `References`: the two-request flow above; result `AnalyzerResult::References { references: Vec<domain::Reference>, omitted_external: u32, omitted_visible: u32 }` (domain types already exist; `is_declaration` computed by set difference on `(file, range)`).
- `Diagnostics`: already implemented per `01.md`? Verify; if the `Diagnostics` arm exists, add nothing but the native cut; if it is a stub, complete it (pull report → `diagnostics_to_domain`, sorted, ≤ 512, `omitted` counts).
- Two native cuts in `analyzer_native.rs` (same receipt machinery, `#[ignore]`, one at a time): `m6-09-references` on `fixtures/valid-basic` (a symbol with ≥ 2 references; assert the declaration is flagged exactly once and every range slices the symbol name out of the captured bytes) and `m6-10-diagnostics-build-script-oracle` on `fixtures/build-script` (assert ≥ 1 native diagnostic whose range is on the `include!`/`env!` line; assert `health: ok`, quiescent, cleanup). `scripts/test-m6-runtime.py` discovers them automatically (verify) — the gate becomes 12 selections. Do not run them (no Docker for you); the orchestrator runs the suite.

### D3 — Tools (`crates/mcp-server/src/stdio/analyzer.rs` + `schemas.rs` + `tests.rs`, same module)

- `rust.analyzer.references`: input `{project_ref, expected_project_fingerprint?, file, position{line,column}, include_declaration = true, timeout_seconds}`; output envelope identical to symbols with `references: [{file, range, is_declaration}]`, `omitted`, `omitted_declarations`. Position semantics: 1-based line, 1-based Unicode-scalar column (document it; reject `line`/`column` = 0 in the schema with `minimum: 1`).
- `rust.analyzer.diagnostics`: input `{project_ref, expected_project_fingerprint?, file, timeout_seconds}`; output envelope with `diagnostics: [{file, range, severity, code?, source: "rust-analyzer", message, related: [{file, range, message}]}]`, `omitted`. **`message` is project-derived diagnostic text** (allowed to cross, like symbol names) but bounded: ≤ 4 096 scalars, control chars other than `\n`/`\t` replaced, truncation flagged `message_truncated`; `code` ≤ 128. Never the serverStatus `message` or stderr.
- Registration in `stdio.rs` (fields, constructor, `list_tools`, `call_tool`), snapshots `analyzer-references-tool.json` and `analyzer-diagnostics-tool.json` at indices 32 and 33 in `tests/protocol.rs`, wire tests in the five MCP versions, invalid-argument tests, mapping tests (table-driven, every code), no-peer-text test, bootstrap test, `RESULT_LIMIT` trim/fallback tests. `release-smoke.py`/`test-release-smoke.py`: 34 tools, two new hashes, the other 32 unchanged (assert; stop and report otherwise). The three `tools.len()` regression tests → 34.
- Extend `tests/analyzer_runtime.rs` with two more `#[ignore]` end-to-end tests (references on `valid-basic`, diagnostics on `build-script`) using the same handshake helper; `scripts/test-m6-runtime.py` must discover them (it already discovers that file).

### D4 — Docs (same commit): `docs/tools.md` (two contracts in the M6 section, inventory 34), `README.md` (34 tools; one sentence each), `CHANGELOG.md` `## Sin publicar`, `docs/security-model.md` (the build-script in-band oracle; diagnostic text boundary), `docs/compatibility.md` (34), `docs/validation/M6/matrix.md` rows M6-02/M6-03 → "Entregado; calificación nativa pendiente del orquestador", ADR-084 §2 amendment and brief §2.2 amendment (references: two requests).

## Verification (targeted)

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked --offline
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo test -p rust-engineering-application --locked --offline analyzer
cargo test -p rust-engineering-execution --locked --offline --lib analyzer_gateway
cargo test -p rust-engineering-execution --locked --offline --lib lsp_codec
cargo test -p rust-engineering-mcp --locked --offline analyzer
cargo test -p rust-engineering-mcp --locked --offline --test protocol --test catalog_status --test crate_inspect --test crate_search
cargo test -p rust-engineering-mcp --locked --offline --test analyzer_runtime --no-run
python3 -B scripts/check-architecture.py
python3 -B scripts/test-release-smoke.py
python3 -B scripts/test-m6-runtime-unit.py
python3 -B scripts/docs-hygiene.py links-check
```

## Ownership

`crates/application/src/analyzer.rs`; `crates/execution-adapter/src/{analyzer_gateway.rs,analyzer_native.rs}` (additive); `crates/mcp-server/src/stdio/analyzer.rs` and `stdio/analyzer/**`; registration edits in `crates/mcp-server/src/stdio.rs`; `crates/mcp-server/tests/{protocol.rs,analyzer_runtime.rs,catalog_status.rs,crate_inspect.rs,crate_search.rs}` (count lines only in the last three); the two new snapshots; `scripts/{release-smoke.py,test-release-smoke.py}`; the documents in D4. No `Cargo.toml`, no dependencies, no changes to `lsp_session.rs`/`lsp_codec.rs`/domain unless strictly additive and reported. No `unwrap`/`expect`/`panic!`/`unsafe`.

## Report (mandatory headings)

Task / Result / Files changed / Tests executed (counts) / Evidence (snapshot hashes) / Risks / Decisions / Open issues.
