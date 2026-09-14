You are the integration delegate for /Users/cburgosro/Projects/rust-mcp, branch ai/m6-analyzer. Make exactly TWO commits, adding only the explicit paths listed. Another worker (W03) is concurrently editing `crates/domain/src/lib.rs`, `crates/domain/src/analyzer.rs`, `crates/domain/tests/analyzer.rs`, `crates/execution-adapter/src/lib.rs`, `crates/execution-adapter/src/lsp_codec.rs` and `docs/validation/M6/delegation/W03-domain-codec/`: NEVER stage those paths, and do not abort because they appear in `git status`. If any OTHER unexpected modified/untracked path appears, stop and report without committing.

Before committing run `python3 -B scripts/docs-hygiene.py links-check` (require "0 broken in living documents") and `python3 -B scripts/docs-hygiene.py verify-inventories` (require "0 failures").

Commit 1 — stage exactly: fixtures/rust-runtime/m6 scripts/build-m6-runtime.py scripts/test-m6-provisioning.py scripts/gate.py .github/workflows/sonarcloud.yml sonar-project.properties docs/ci.md docs/adr/ADR-082-m6-runtime-provisioning.md docs/validation/M6/provisioning.json docs/roadmap/m6-provisioning-request.md
Message (subject, blank line, body, blank line, trailers):

feat(m6): provision the M6 guest image with rust-analyzer 1.98.1 and rust-src 1.98.1

Owner-authorized (2026-09-11, option A+B+C of docs/roadmap/m6-provisioning-request.md). fixtures/rust-runtime/m6 downloads the pinned channel manifest and the two tarballs once, verifies every sha256 against the published manifest, validates tar members and size bounds, and builds rust-engineering-runtime:1.98.1-arm64-m6 from the admitted M5 digest with --network=none. rust-analyzer lives at /opt/analyzer/bin off PATH; its RUNPATH dependency on librustc_driver/libLLVM is satisfied by symlinks into /opt/rust/lib. ADR-082 records the decision; the receipt is docs/validation/M6/provisioning.json (image sha256:f39a5b33…, rust-analyzer 1.98.1 (48a229c 2026-09-01)). Adds the two gate stages, the SonarCloud coverage wiring and the ci.md section (25 core / 40 full stages). The image is not admitted in the gateway yet.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HSFkiUg2PJKCiP83rqANwn

Commit 2 — stage exactly: docs/adr/ADR-083-analyzer-contract-and-actions.md docs/adr/ADR-084-rust-analyzer-runtime-and-lsp-lifecycle.md docs/adr/README.md docs/roadmap/adr-backlog-m2-m8.md docs/validation/M6/delegation/README.md docs/validation/M6/delegation/D25-D26-decision-brief.md docs/validation/M6/delegation/R01-ra-research docs/validation/M6/delegation/V01-review-provisioning docs/validation/M6/delegation/W01-provisioning docs/validation/M6/delegation/W01b-provisioning-fixes docs/validation/M6/delegation/W02-adrs
Message:

docs(m6): decide D25/D26 (ADR-083, ADR-084) and record the R01, W01, V01, W01b and W02 delegations

The orchestrator's decision brief fixes the five analyzer tools, the M2-writer reuse for action apply, the transient-per-query LSP lifecycle, the fixed initializationOptions, the minimal client capabilities, the capture-time rejection of rust-analyzer.toml and the budgets; ADR-083/084 write them down and the backlog marks D25/D26 Decided. The R01 Gemini research is archived with its disposition (fabricated tarball hashes in Q1 → auxiliary evidence only). The V01 independent review of the provisioning diff (Block: 2 P2, 6 P3) and its W01b fixes are recorded with their dispositions.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HSFkiUg2PJKCiP83rqANwn

Do not push, do not edit files. Report with headings: Task / Result / Files changed / Tests executed / Evidence (both commit hashes) / Risks / Decisions / Open issues.
