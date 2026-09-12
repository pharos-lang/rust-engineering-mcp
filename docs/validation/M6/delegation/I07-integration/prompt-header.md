You are the integration delegate for /Users/cburgosro/Projects/rust-mcp, branch ai/m6-analyzer. No other worker is running. Make exactly TWO commits. First run `python3 -B scripts/docs-hygiene.py links-check` (require "0 broken in living documents") and `python3 -B scripts/docs-hygiene.py verify-inventories` (require "0 failures"). Then `git status --short`: every modified/untracked path must be in one of the two lists below. If any other unexpected path appears (tmp_scratch, a stray target/ dir under fixtures, etc.), stop and report without committing.

Commit 1 (the M6-02/M6-03 tools + their evidence) — stage exactly:
crates/application/src/analyzer.rs crates/domain/src/analyzer.rs crates/execution-adapter/src/analyzer_gateway.rs crates/execution-adapter/src/analyzer_native.rs crates/execution-adapter/src/lsp_codec.rs crates/mcp-server/src/stdio.rs crates/mcp-server/src/stdio/analyzer.rs crates/mcp-server/src/stdio/analyzer/schemas.rs crates/mcp-server/src/stdio/analyzer/tests.rs crates/mcp-server/tests/analyzer_runtime.rs crates/mcp-server/tests/protocol.rs crates/mcp-server/tests/catalog_status.rs crates/mcp-server/tests/crate_inspect.rs crates/mcp-server/tests/crate_search.rs crates/mcp-server/tests/snapshots/analyzer-references-tool.json crates/mcp-server/tests/snapshots/analyzer-diagnostics-tool.json fixtures/analyzer-references scripts/release-smoke.py scripts/test-release-smoke.py docs/adr/ADR-084-rust-analyzer-runtime-and-lsp-lifecycle.md docs/tools.md README.md CHANGELOG.md docs/security-model.md docs/compatibility.md docs/validation/M6/01.md docs/validation/M6/01-calibration.json
Message:

feat(m6): rust.analyzer.references (M6-02) and rust.analyzer.diagnostics (M6-03), tools 33-34

references drives two textDocument/references requests in one session (includeDeclaration true/false) and marks the declaration by set difference over bounded location vectors; the tool filters and counts omitted_declarations. diagnostics is pull-only, native rust-analyzer diagnostics; peer message/code are control-sanitized and bounded, over-limit or empty entries are truncated or omitted and declared in completeness — never a call failure (V06 P1 fix in lsp_codec::diagnostics_to_domain). Under the safe minimal config (checkOnSave off, procMacro off, diagnostics.experimental off) diagnostics is syntax-only; type/borrow/lint/unresolved errors are rust.check's domain (owner Option A, ADR-084 §3 amendment); diagnostics quality is tracked debt. The no-build-script proof is by symbols (the include! stays unexpanded, GENERATED absent), complementing the process-tree cut. Inventory is 34 tools; the 32 existing snapshots are byte-identical. Native calibration over these exact bytes: 11/11 cuts pass (docs/validation/M6/01-calibration.json, gate passed), including m6-09-references and m6-10-no-build-script. Independent review V06 (Opus, Block: P1 + 5 P2) dispositioned and fixed in W06b/W06c/W06d.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HSFkiUg2PJKCiP83rqANwn

Commit 2 (records) — stage exactly:
docs/validation/M6/matrix.md docs/validation/M6/delegation/README.md docs/validation/M6/delegation/D25-D26-decision-brief.md docs/validation/M6/delegation/V06-review-references-diagnostics docs/validation/M6/delegation/W06-references-diagnostics docs/validation/M6/delegation/W06b-references-diagnostics-fixes docs/validation/M6/delegation/W06c-references-fixture docs/validation/M6/delegation/W06d-diagnostics-oracle-and-docs docs/validation/M6/delegation/I06-integration
Message:

docs(m6): M6-02/03 matrix, debt entry, and the V06/W06/W06b/W06c/W06d/I06 delegation records

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HSFkiUg2PJKCiP83rqANwn

Do not push, do not edit files, do not run cargo. Report with headings: Task / Result / Files changed / Tests executed / Evidence (both hashes) / Risks / Decisions / Open issues.
