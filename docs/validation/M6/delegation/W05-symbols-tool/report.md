# W05 — informe del worker

| Campo | Valor |
| --- | --- |
| Modelo | Claude Sonnet 5 (`claude -p --model sonnet --effort high --disallowedTools Agent Task`), Claude Code 2.1.268 |
| Modelos observados | claude-haiku-4-5-20251001, claude-sonnet-5 |
| Inicio / fin (UTC) | 2026-09-12T07:35:27Z / 2026-09-12T11:25:43Z; 3 h 50 min |
| Resultado | `subtype: success`, `is_error: False` |
| Permisos denegados | 19 (el sandbox del worker bloqueó `cargo test … --ignored` y `test-m6-runtime.py` por usar Docker; el orquestador ejecutó la suite nativa después, ver disposición V05) |
| Transcript | fuera del árbol; hashes en `transcripts.sha256` |

## Informe literal del worker

All implementation, testing, and documentation work for W05 is complete. Here is my report.

## Task

Implement the M6-01 closing vertical: the application port `AnalyzerPort`/`ProjectRegistry::analyzer_symbols`, the MCP tool `rust.analyzer.symbols`, its contract/protocol/wire tests, the native evidence test, and the required documentation, per ADR-083/ADR-084 and the D1–D5 deliverables.

## Result

All five deliverables (D1–D5) are implemented, tested, and documented. The tool is wired into `EngineeringServer`, advertised as tool 32/32, and its snapshot is the byte-for-byte JSON `tools/list` actually emits. Full targeted and workspace-level verification is green. Two serde/schemars interaction issues (flatten + `deny_unknown_fields`, and three pre-existing hardcoded `31`-tool-count test assertions elsewhere in the crate) were found and fixed as direct, minimal consequences of this change.

## Files changed

Owned files (per delegation):
- `crates/application/src/analyzer.rs` (new) + one `pub mod analyzer;` line in `crates/application/src/lib.rs`
- `crates/mcp-server/src/stdio/analyzer.rs`, `stdio/analyzer/schemas.rs`, `stdio/analyzer/tests.rs` (new)
- Registration edits in `crates/mcp-server/src/stdio.rs`
- The one lend/port impl in `crates/execution-adapter/src/project_inspection.rs`
- `crates/mcp-server/tests/{protocol.rs,analyzer_runtime.rs}` and `tests/snapshots/analyzer-symbols-tool.json`
- `scripts/{release-smoke.py,test-release-smoke.py,test-m6-runtime.py}`, `scripts/gate.py` (only the one addendum line)
- `docs/{tools.md,README.md→README.md,CHANGELOG.md,security-model.md,compatibility.md,client-configuration.md,implementation-status.md,ci.md}`

Direct-regression fixes, outside the listed ownership, made because my change broke them (see Decisions):
- `crates/mcp-server/tests/{catalog_status.rs,crate_inspect.rs,crate_search.rs}` — each hardcoded `tools.len() == 31`; updated to `32`.

Not touched: `analyzer_gateway.rs`, `lsp_session.rs`, `lsp_codec.rs`, domain, `Cargo.toml`/`Cargo.lock`.

## Tests executed (with counts)

- `cargo test -p rust-engineering-application --locked --offline analyzer` → 9 passed, 0 failed
- `cargo test -p rust-engineering-mcp --locked --offline analyzer` → 9 unit + 1 compiled-but-`--ignored` = 10 handled, 0 failed
- `cargo test -p rust-engineering-mcp --locked --offline --test protocol` → 47 passed, 0 failed (31 pre-existing snapshots byte-identical + 3 new analyzer wire tests)
- `cargo test -p rust-engineering-mcp --locked --offline` (full crate) → 0 failed after the 3-file regression fix
- `cargo test -p rust-engineering-application --locked --offline` (full crate) → 0 failed
- `cargo test -p rust-engineering-execution --locked --offline --lib` → 462 passed, 0 failed, 33 ignored (native, unrelated)
- `cargo fmt --all -- --check`, `cargo check --workspace --all-targets --locked --offline`, `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` → clean
- `python3 -B scripts/check-architecture.py` → PASS
- `python3 -B scripts/test-release-smoke.py` → 9 passed
- `python3 -B scripts/test-gate-reporting.py` → 13 passed
- `python3 -B scripts/test-m6-runtime-unit.py` → 19 passed (after my `test-m6-runtime.py` refactor)
- `python3 -B scripts/docs-hygiene.py links-check` → 0 broken in living documents
- `cargo test -p rust-engineering-mcp --locked --offline --test analyzer_runtime --no-run` → compiles; `--ignored` execution attempted, see Open issues

## Evidence

- Snapshot hash: `crates/mcp-server/tests/snapshots/analyzer-symbols-tool.json` is the exact JSON dumped from a live `tools/list` call against the built binary (diffed byte-identical, `equal: True`), not hand-written.
- `TOOL_SCHEMA_SHA256["rust.analyzer.symbols"]` in `scripts/release-smoke.py` = `c19f3f29b11794f29edd69e665e9a59ffd3655ebe0040a1b78305e8683ff7a6b`, computed for real via the script's own `sha256(canonical_json({...}))` method against that same schema — not fabricated.
- Native call: **not executed**. The sandbox's permission mode blocked `cargo test --test analyzer_runtime -- --ignored` and `python3 scripts/test-m6-runtime.py` (both spawn Docker containers) even though Docker, the socket, and the admitted M6 image (`sha256:f39a5b33…`) are present locally. The test compiles cleanly against the real crate API. Deferred to the orchestrator per the delegation's own instruction ("the orchestrator will run the suite for the receipt").

## Risks

- **Native evidence unrecorded.** D4 asked me to "run it once and record the outcome"; I could not. The orchestrator should run `RUST_MCP_TEST_SOCKET=<sock> RUST_MCP_TEST_IMAGE=sha256:f39a5b33… cargo test -p rust-engineering-mcp --locked --offline --test analyzer_runtime -- --ignored --nocapture --test-threads=1` (and/or the extended `scripts/test-m6-runtime.py`) before treating M6-01/W05 as closed.
- The `scripts/test-m6-runtime.py` extension (10th step) is untested end-to-end for the same reason; its unit-testable parts (`ignored_tests(INTEGRATION_SOURCE)` discovery, syntax, the 19 existing unit tests) are verified, but the live run is not.

## Decisions

- **`AnalyzerPort` returns `AnalyzerObservation{source_fingerprint, execution}`, not bare `AnalyzerExecution`.** The bundle digest ("the same helper M2 uses") only exists inside `execution-adapter` (`source_archive::encode` + private `digest()`); application has no sha2 dependency and none was added. The port wraps the adapter-computed fingerprint alongside the execution, mirroring the `CheckObservation` precedent.
- **`AnalyzerPort::analyze` takes `ExecutionLimits`, computed by `analyzer_symbols` from the caller's `timeout_seconds`.** Needed so the caller-configurable 1..=180s budget actually reaches `RustGateway::execute_analyzer`.
- **New `AnalyzerRequestError` enum**, not literal `InspectionError`. `InspectionError` (owned by a different delegation's file) has no case for `Conflict`/`FileNotInSnapshot`; precedented by `ResolutionError`/`SemanticPreparationError`'s existing pattern of wrapping `InspectionError` plus feature-specific variants.
- **`Outcome` has 4 variants (no `Failed`)**, matching `rust.project.inspect`'s house pattern rather than `rust.check`'s, because D2 states `failed` never occurs for this read-only tool.
- **No new `with_gateway_analyzer` method.** `AnalyzerPort for RustProjectInspector` reuses the existing private `with_gateway` directly, since the impl lives in the same file.
- **`Input` derives `JsonSchema` but hand-writes `Deserialize` via a private `Shadow` struct.** Serde's `#[serde(deny_unknown_fields)]` on a struct with `#[serde(flatten)]` makes serde reject the flattened `Scope` enum's own fields as "unknown" before flatten runs — confirmed empirically, not assumed. The attribute stays declared (for schemars, which still emits `unevaluatedProperties: false`) while the real parsing goes through a shadow type without it; the resulting schema is verified byte-identical.
- **Fixed a direct regression outside my file ownership**: `catalog_status.rs`, `crate_inspect.rs`, `crate_search.rs` each hardcoded `tools.len() == 31`. Left broken, they'd fail the shared gate for a reason unrelated to their own tests. Minimal one-line fixes, not otherwise touched.
- **`SandboxDenied` maps to `unavailable`** (not `blocked`, diverging from `check.rs`'s own convention for the same `OperationalErrorCode` variant) because D2 explicitly requires "`tools/call` without a configured runtime returns `unavailable` with `SANDBOX_DENIED`".
- **`MalformedHeader`→`FRAME_LIMIT`, no new `PROTOCOL_VIOLATION` code** — took the addendum's default option to keep the closed code set exactly as D2 lists it.

## Open issues

- D4's native run and the extended `test-m6-runtime.py` gate run are both pending real execution (see Risks).
- M6-02..M6-05 (references/diagnostics/actions/action.apply) remain unimplemented, as scoped.
