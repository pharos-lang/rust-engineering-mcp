You are the integration delegate for /Users/cburgosro/Projects/rust-mcp, branch ai/m6-analyzer. No other worker is running. Make exactly TWO commits. First run `python3 -B scripts/docs-hygiene.py links-check` (require "0 broken in living documents") and `python3 -B scripts/docs-hygiene.py verify-inventories` (require "0 failures"). Then `git status --short`: every modified/untracked path must appear in one of the two lists below EXCEPT `docs/validation/M6/delegation/W06-references-diagnostics/` which is next-cut planning and must be LEFT UNSTAGED. If any other unexpected path appears (e.g. tmp_scratch, a stray file), stop and report without committing.

Commit 1 (the M6-01 tool + its evidence) — stage exactly:
crates/application/src/analyzer.rs crates/application/src/lib.rs crates/domain/src/analyzer.rs crates/execution-adapter/src/lsp_codec.rs crates/execution-adapter/src/project_inspection.rs crates/mcp-server/src/stdio.rs crates/mcp-server/src/stdio/analyzer.rs crates/mcp-server/src/stdio/analyzer crates/mcp-server/tests/protocol.rs crates/mcp-server/tests/analyzer_runtime.rs crates/mcp-server/tests/catalog_status.rs crates/mcp-server/tests/crate_inspect.rs crates/mcp-server/tests/crate_search.rs crates/mcp-server/tests/snapshots/analyzer-symbols-tool.json scripts/gate.py scripts/release-smoke.py scripts/test-release-smoke.py scripts/test-m6-runtime.py docs/tools.md README.md CHANGELOG.md docs/security-model.md docs/compatibility.md docs/client-configuration.md docs/implementation-status.md docs/ci.md docs/validation/M6/01.md docs/validation/M6/01-calibration.json docs/validation/M6/02.md
Message:

feat(m6): the rust.analyzer.symbols tool, closing M6-01 (tool 32)

Adds the application port rust_engineering_application::analyzer and the MCP tool rust.analyzer.symbols (document and workspace scope) over the M6 gateway: capture, snapshot fingerprint, expected-fingerprint conflict, file-in-snapshot check, the ADR-083 §3 envelope (status/closed error codes, snapshot/analyzer/toolchain/readiness/completeness/limits/session facts, Unicode-scalar 1-based positions, ≤512 visible, 512 KiB bound with a declared trim, no analyzer stderr/message text on the wire). Peer symbol name/container are bounded (256 scalars, control chars → OversizedEntry omission), detail truncated at 1024. The inventory is now 32 tools; the 31 existing definitions are byte-identical and their snapshot hashes unchanged. Native evidence: the nine analyzer_native calibration cuts, re-run over these exact bytes, pass 9/9 (docs/validation/M6/01-calibration.json, gate status passed); the end-to-end product path was reproduced by hand (docs/validation/M6/02.md). The flaky analyzer_runtime cargo wrapper is de-gated pending W05f hardening. Independent review V05 (Block: 2 P1, 3 P2, 3 P3) dispositioned and fixed in W05b–W05e.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HSFkiUg2PJKCiP83rqANwn

Commit 2 (records) — stage exactly:
docs/validation/M6/matrix.md docs/validation/M6/delegation/README.md docs/validation/M6/delegation/W05-symbols-tool docs/validation/M6/delegation/W05b-symbols-tool-fixes docs/validation/M6/delegation/W05e-native-evidence-and-harness docs/validation/M6/delegation/V05-review-symbols-tool docs/validation/M6/delegation/I05-integration
Message:

docs(m6): M6-01 matrix update and the W05/V05/W05b/W05e/I05 delegation records

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HSFkiUg2PJKCiP83rqANwn

Do not push, do not edit files, do not run cargo. Report with headings: Task / Result / Files changed / Tests executed / Evidence (both hashes) / Risks / Decisions / Open issues.
