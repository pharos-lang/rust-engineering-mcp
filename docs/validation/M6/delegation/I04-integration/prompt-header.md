You are the integration delegate for /Users/cburgosro/Projects/rust-mcp, branch ai/m6-analyzer. Make exactly ONE commit with the explicit paths below. Another worker (W04) is starting concurrently and may create/modify: `crates/execution-adapter/src/{lsp_session.rs,analyzer_gateway.rs,analyzer_native.rs,rust_gateway.rs,rust_applied.rs}`, `crates/project-adapter/**`, `crates/mcp-server/src/host_config.rs`, `docs/adr/ADR-085-*`, `docs/validation/M6/01*`, `scripts/test-m6-runtime*.py`, `scripts/gate.py`, `sonar-project.properties`, `.github/workflows/sonarcloud.yml`, `docs/ci.md`. NEVER stage those; do not abort because they appear. If any OTHER unexpected path appears, stop and report.

Run `python3 -B scripts/docs-hygiene.py links-check` (require "0 broken in living documents") and `python3 -B scripts/docs-hygiene.py verify-inventories` (require "0 failures") first.

Stage exactly: crates/domain/src/lib.rs crates/domain/src/analyzer.rs crates/execution-adapter/src/lib.rs crates/execution-adapter/src/lsp_codec.rs docs/adr/ADR-084-rust-analyzer-runtime-and-lsp-lifecycle.md docs/validation/M6/delegation/D25-D26-decision-brief.md docs/validation/M6/delegation/README.md docs/validation/M6/delegation/I03-integration docs/validation/M6/delegation/V03-review-domain-codec docs/validation/M6/delegation/W03-domain-codec docs/validation/M6/delegation/W03b-domain-codec-fixes docs/validation/M6/delegation/W04-lsp-session-gateway

IMPORTANT: `crates/execution-adapter/src/lib.rs` may by then contain extra `mod` lines added by W04 (e.g. `lsp_session`, `analyzer_gateway`, `analyzer_native`, `APPROVED_M6_IMAGE`). If `git diff crates/execution-adapter/src/lib.rs` shows anything other than the single line `pub mod lsp_codec;`, stage only that hunk (`git add -p` is not available non-interactively — instead, if the diff is not exactly that one line, DO NOT stage lib.rs, leave it out, and say so in the report; the build of the committed tree would then be incomplete, so in that case ALSO do not commit at all and report).

Commit message (subject, blank line, body, blank line, trailers):

feat(m6): analyzer domain values and bounded LSP codec (M6-01 groundwork)

Adds rust_engineering_domain::analyzer — validated file paths, TextRange over the existing 1-based Unicode-scalar Position, a LineIndex with \n as the only line break and byte/UTF-8/UTF-16 translation that rejects offsets inside a code point, symbol/diagnostic/reference/code-action values, apply_edits, closed omission and rejection enums and the M6 limits — and rust_engineering_execution::lsp_codec — an incremental base-protocol decoder bounded at 1 MiB per frame, 4096 messages and 16 MiB per session, strict JSON-RPC parsing, a correlator, the fixed initialize capabilities and initializationOptions with their config digest, and conversions to domain that accept only file:///source/ URIs and reject commands, snippets, resource operations, version mismatches and overlapping edits. Independent review V03 (Block: P0 unbounded recursion, P1 missing codeAction capability) is dispositioned and fixed in W03b; 28 domain and 45 codec tests. No process or MCP tool yet.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HSFkiUg2PJKCiP83rqANwn

Do not push, do not edit files. Report with headings: Task / Result / Files changed / Tests executed / Evidence (commit hash) / Risks / Decisions / Open issues.
