# W05 — Application port and the first analyzer tool: `rust.analyzer.symbols` end to end (M6-01 closes here)

Model requested: Claude Sonnet 5 (`claude -p --model sonnet --effort high`). Role: implementation worker with disjoint file ownership. Orchestrator: Claude Fable 5.1. You may not spawn subagents. Do not commit.

## Read first

`AGENTS.md`; `docs/adr/ADR-083-analyzer-contract-and-actions.md` (§1–§4, §7 — the public contract you implement); `docs/adr/ADR-084-…` §2, §5, §8 (budgets); `docs/validation/M6/delegation/D25-D26-decision-brief.md` §1.2–1.4; `docs/validation/M6/01.md` (what the gateway proved and the measured timings); the execution adapter API you consume: `crates/execution-adapter/src/analyzer_gateway.rs` (`RustGateway::execute_analyzer`, `AnalyzerQuery`, `AnalyzerExecution`, `AnalyzerFailure`, identity constants, `APPROVED_M6_IMAGE`) and `rust_engineering_domain::analyzer`; the house pattern for a guest-backed read-only tool: `crates/application/src/check.rs` (`ProjectCheckPort`, `ProjectRegistry::check` with capture + publish), `crates/mcp-server/src/stdio/check.rs` (+ `check/schemas.rs`, `check/tests.rs`), `crates/mcp-server/src/stdio/inspection.rs` (how `RustProjectInspector` lends the gateway and how `Unavailable`/`Denied`/quarantine map to statuses), `crates/mcp-server/src/stdio.rs` (tool registration: `EngineeringServer` field, constructor around line 943, `list_tools`/`call_tool` match arms, `definition`), `crates/mcp-server/src/stdio/contract.rs`, `crates/mcp-server/tests/protocol.rs` (snapshot list at index 1..=30, `assert_output`), `scripts/release-smoke.py` (pins 31 tools + schema hashes), and how M5 documented four new tools in `docs/tools.md` ("Contratos M5"), `README.md`, `CHANGELOG.md` (`## Sin publicar`), `docs/security-model.md`, `docs/compatibility.md`, `docs/client-configuration.md`.

## Deliverables

### D1 — Application port (`crates/application/src/analyzer.rs`, + `pub mod analyzer;` in `lib.rs`)

`pub trait AnalyzerPort { fn analyze(&self, source: &SourceBundle, query: &AnalyzerQuery, control: &dyn InspectionControl) -> Result<AnalyzerExecution, InspectionError>; }` and `ProjectRegistry::analyzer_symbols(&mut self, reference, request: SymbolsRequest, port: &impl AnalyzerPort, control) -> Result<AnalyzerReport, InspectionError>` that: revalidates the project, captures the `SourceBundle` through the existing lease (`source_inner`), checks the optional `expected_project_fingerprint` (mismatch → typed `Conflict`, no data), computes the snapshot facts (`source_fingerprint` = the same digest the M2 code uses for bundles — reuse the existing helper, do not invent a second hashing), rejects a `file` not present in the bundle (`FileNotInSnapshot`) before calling the port, calls the port once, and returns a domain-only report `{ project_ref, project_identity_fingerprint, snapshot: {source_fingerprint, files, semantics: LatestKnown, atomic: false}, execution: AnalyzerExecution }`. No `rmcp`, no `serde_json`, no process APIs in application (the architecture script enforces it). Unit tests with a fake port: happy path, conflict on fingerprint, file not in snapshot, port error propagation, cancellation via `control`.

### D2 — MCP tool `rust.analyzer.symbols` (`crates/mcp-server/src/stdio/analyzer.rs` + `analyzer/schemas.rs` + `analyzer/tests.rs`)

Input (deny_unknown_fields): `project_ref` (`^prj_[0-9a-f]{32}$`), `expected_project_fingerprint: Option<String>` (`^sha256:[0-9a-f]{64}$`), `scope` tagged enum: `{ "scope": "document", "file": "<relative .rs path>" }` or `{ "scope": "workspace", "query": "<1..=128 chars, no control chars>" }`, `timeout_seconds` default 60, max 180. Annotations: `readOnlyHint=true`, `idempotentHint=true`, `destructiveHint=false`, `openWorldHint=false`. Description states: exact rust-analyzer identity, snapshot semantics `latest_known`/non-atomic, no build scripts/proc macros/check, ≤ 512 visible, Unicode-scalar 1-based positions, that it requires the host `--rust` runtime with the M6 image, and that hover/definition/rename are not offered.

Output — the ADR-083 §3 envelope, as a tagged `Outcome` like `check.rs`: `status ∈ passed | failed | blocked | unavailable | cancelled`, closed `error_code`/`error_message` (`CONFLICT`, `FILE_NOT_IN_SNAPSHOT`, `ANALYZER_NOT_READY`, `ANALYZER_CRASHED`, `ANALYZER_CAPABILITY_MISMATCH`, `FRAME_LIMIT`, `MESSAGE_LIMIT`, `RESULT_LIMIT`, `TIMEOUT_INITIALIZE`, `TIMEOUT_QUERY`, `TIMEOUT_TOTAL`, `UNSUPPORTED_PROJECT_CONFIG`, `FILE_NOT_UTF8`, `SANDBOX_DENIED`, `UNSUPPORTED_PLATFORM`, `PROJECT_NOT_FOUND`, `INVALID_PROJECT`, `CANCELLED`, `OUTPUT_LIMIT_EXCEEDED`), and `data`: `project_ref`, `project_identity_fingerprint`, `snapshot {source_fingerprint, files, semantics, atomic}`, `analyzer {version, binary_sha256, image_id, config_digest, position_encoding}`, `toolchain {rust_version: "1.98.1", sysroot: "present"}`, `readiness {state: quiescent|not_ready, health: ok|warning, elapsed_ms}`, `completeness {state, omissions[{kind,count}], reasons[]}`, `limits {max_visible: 512, initialize_timeout_seconds, query_timeout_seconds, total_timeout_seconds, frame_bytes, messages}`, `session {messages_in, messages_out, bytes_in, bytes_out, duration_ms, stderr_bytes, server_requests}`, `termination`, `exit_code`, `oom_killed`, and `symbols`: for `document` scope a flat list `{depth, name, kind, detail?, deprecated, range{start{line,column},end{…}}, selection_range{…}}`; for `workspace` scope `{name, kind, container?, file, range}`; both sorted as ADR-083 §4 says; `omitted` count. Status mapping: `passed` = the analyzer answered (even if `incomplete` — completeness is data, not status); `failed` = never for this read-only tool unless the project itself makes the analyzer produce an error response (`ContentModified` → `unavailable` per ADR-084 §5); `blocked` = `CONFLICT`, `FILE_NOT_IN_SNAPSHOT`, `UNSUPPORTED_PROJECT_CONFIG`, `SANDBOX_DENIED`, `ANALYZER_CAPABILITY_MISMATCH`; `unavailable` = runtime not configured/not the M6 image/quarantined/`ANALYZER_CRASHED`/limits/timeouts as classified by `AnalyzerFailure`; `cancelled`. The response is bounded at 512 KiB with a declared trim (`RESULT_LIMIT` in `completeness.reasons`, never a truncated JSON). No stderr text, no `message` text from the analyzer ever crosses into the result.

Wire it into `EngineeringServer` exactly like `check` (field, constructor, `list_tools` definition, `call_tool` arm), lending the gateway through `RustProjectInspector` the same way `check` does (if the inspector exposes no analyzer entry point, add `pub fn with_gateway_analyzer(&self, …)` mirroring the existing lend/guard shape in `crates/execution-adapter/src/project_inspection.rs` — that file is yours for this addition only). Concurrency: joined worker via `Workers`, deadline = `timeout_seconds`, single-flight as the other guest tools.

### D3 — Contract, protocol and wire tests

- `crates/mcp-server/tests/snapshots/analyzer-symbols-tool.json`: the new tool definition; add it at index 31 in `tests/protocol.rs` and keep indices 1..=30 untouched (the 31 existing snapshots must not change by a byte).
- Protocol tests (in `tests/protocol.rs` or a new `tests/analyzer_protocol.rs`): `tools/list` in the five MCP versions includes the tool; `tools/call` without a configured runtime returns `unavailable` with `SANDBOX_DENIED`/`UNSUPPORTED_PLATFORM` as the house convention for guest tools without `--rust`; invalid arguments (unknown field, bad `project_ref`, `scope` missing, `file` with `..` or non-`.rs`, `query` with control chars, `timeout_seconds` 0/181) → `invalid_params`; unknown `project_ref` → `blocked/PROJECT_NOT_FOUND`; output validates against the tool's own `outputSchema` (reuse `assert_output`), and the text content mirrors `structuredContent`.
- Unit tests of the mapping `AnalyzerExecution`/`AnalyzerFailure` → `Outcome` for every error code above (table-driven), the 512 KiB trim, and that no `message`/stderr text can appear in the output (feed an execution with hostile strings in every free-text-looking field and assert absence).
- `scripts/release-smoke.py` and `scripts/test-release-smoke.py`: the pinned inventory becomes 32 tools and the new schema hash is pinned; do not change the other 31 hashes (if one changes, stop and report — it means an existing contract moved).

### D4 — Native end-to-end evidence (one `#[ignore]` test, not a suite)

Add `crates/mcp-server/tests/analyzer_runtime.rs` with one ignored test that starts the real server binary with `--rust` pointing at the M6 image and socket (pattern: `tests/inspection_runtime.rs`), opens `fixtures/valid-basic`, calls `rust.analyzer.symbols` with `scope: document`, `file: src/lib.rs`, and asserts `passed`, `readiness.state == "quiescent"`, `analyzer.version == "rust-analyzer 1.98.1 (48a229c 2026-09-01)"`, `position_encoding == "utf-8"`, at least one symbol whose `selection_range` slices its own name out of the fixture bytes, and `completeness.state == "complete"`. Then a second call with `scope: workspace`, `query: "answer"` (or whatever the fixture defines) that returns that symbol. Add the selection to `scripts/test-m6-runtime.py` as a tenth step (it already drives `#[ignore]` selections; extend its discovery to this integration test file, keeping the receipt honest). Run it once and record the outcome in your report; copy nothing into `docs/validation` — the orchestrator will run the suite for the receipt.

### D5 — Documentation (same commit as the code, AGENTS.md rule)

- `docs/tools.md`: new section "Contratos M6 — analyzer" with `rust.analyzer.symbols` documented like the M5 tools (inputs, outputs, statuses/codes, limits, snapshot semantics, what it never does); the inventory sentence becomes 32 tools.
- `README.md`: tool inventory 31 → 32, one paragraph on the analyzer (requires `--rust` with the M6 image; no build scripts/proc macros/check-on-save; `rust-analyzer.toml` projects are refused), host flag unchanged for reads.
- `CHANGELOG.md` `## Sin publicar`: entry for ADR-082/083/084/085 and the tool.
- `docs/security-model.md`: threat-model rows for the analyzer (hostile LSP peer, hostile `rust-analyzer.toml`, never-ready/crash, external URIs, frame/message limits, no peer text in results) citing ADR-084 and `docs/validation/M6/01.md`.
- `docs/compatibility.md`: guest image M6 row (`sha256:f39a5b33…`, tag, receipt, admission ADR-085, "M1–M5 tools remain qualified against their own digests; running them on the M6 image is not qualified by their suites"), tool count 32, the `--print-config-schema` archive.
- `docs/client-configuration.md`: how to point `--rust` at the M6 image for the analyzer.
- `docs/implementation-status.md`: M6 row → In progress with M6-01 evidence links.

## Verification (targeted)

```text
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked --offline
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo test -p rust-engineering-application --locked --offline analyzer
cargo test -p rust-engineering-mcp --locked --offline analyzer
cargo test -p rust-engineering-mcp --locked --offline --test protocol
python3 -B scripts/check-architecture.py
python3 -B scripts/test-release-smoke.py
python3 -B scripts/test-gate-reporting.py
python3 -B scripts/docs-hygiene.py links-check
RUST_MCP_TEST_SOCKET=… RUST_MCP_TEST_IMAGE=sha256:f39a5b33… cargo test -p rust-engineering-mcp --locked --offline --test analyzer_runtime -- --ignored --nocapture --test-threads=1
```

## Ownership (touch nothing else)

`crates/application/src/analyzer.rs` (+ one `pub mod` line in `lib.rs`), `crates/mcp-server/src/stdio/analyzer.rs` and `stdio/analyzer/**`, the registration edits in `crates/mcp-server/src/stdio.rs`, the one lend method in `crates/execution-adapter/src/project_inspection.rs`, `crates/mcp-server/tests/{protocol.rs,analyzer_runtime.rs}` and `tests/snapshots/analyzer-symbols-tool.json`, `scripts/release-smoke.py`, `scripts/test-release-smoke.py`, `scripts/test-m6-runtime.py` (discovery extension only), and the documents in D5. No `Cargo.toml` changes, no new dependencies, no changes to `analyzer_gateway.rs`/`lsp_session.rs`/`lsp_codec.rs`/domain (if you need something there, report it; do not patch around it). No `unwrap`/`expect`/`panic!`/`unsafe`.

## Report (mandatory headings)

Task / Result / Files changed / Tests executed (with counts) / Evidence (snapshot hash, native call output summary) / Risks / Decisions / Open issues.
