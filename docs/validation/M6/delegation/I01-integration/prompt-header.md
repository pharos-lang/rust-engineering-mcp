You are the integration delegate for the Rust Engineering MCP repository at /Users/cburgosro/Projects/rust-mcp, branch ai/m6-analyzer. Task: commit ONLY the M6 orchestration coordination state. Steps, in order:
(1) run `git status --short` and confirm exactly these untracked paths exist: docs/prompts/implement-m6-fable-orchestrator.md, docs/roadmap/m6-provisioning-request.md, docs/validation/M6/ — if anything else is modified or untracked, stop and report without committing;
(2) run `python3 -B scripts/docs-hygiene.py links-check` and `python3 -B scripts/docs-hygiene.py verify-inventories` and require "0 broken in living documents" and "0 failures";
(3) `git add` exactly those three paths;
(4) commit with this exact message (subject line, blank line, body, blank line, trailers):

docs(m6): open the analyzer milestone — coordination register and provisioning request

Branch ai/m6-analyzer from main 627729a. Records the live verification of the M5 closure against the current bytes, the rust-analyzer/rust-src inventory (absent from the 1.98.1 host toolchain and from the M5 guest image), the verified CLI versions and model probes, the blocked R01 Gemini research package, and the owner dossier requesting rust-analyzer 1.98.1 + rust-src 1.98.1 in a guest image derived from the admitted M5 digest. No product code.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HSFkiUg2PJKCiP83rqANwn

Do not push. Do not edit any file. Then report using exactly these headings: Task / Result / Files changed / Tests executed / Evidence (include the new commit hash from `git log -1`) / Risks / Decisions / Open issues.
