You are the integration delegate for /Users/cburgosro/Projects/rust-mcp, branch ai/m6-analyzer. No other worker is running now. Make exactly TWO commits.

First run `python3 -B scripts/docs-hygiene.py links-check` (require "0 broken in living documents") and `python3 -B scripts/docs-hygiene.py verify-inventories` (require "0 failures"). Then `git status --short`: every modified/untracked path must be in one of the two lists below; if any other path appears, stop and report without committing.

Commit 1 — stage exactly: crates/domain/src/analyzer.rs crates/execution-adapter/src/lib.rs crates/execution-adapter/src/lsp_codec.rs crates/execution-adapter/src/lsp_session.rs crates/execution-adapter/src/analyzer_gateway.rs crates/execution-adapter/src/analyzer_native.rs crates/execution-adapter/src/rust_applied.rs crates/execution-adapter/src/rust_gateway.rs crates/mcp-server/src/host_config.rs crates/project-adapter/src/filesystem/macos/source.rs crates/project-adapter/tests/source.rs scripts/test-m6-runtime.py scripts/test-m6-runtime-unit.py scripts/gate.py sonar-project.properties .github/workflows/sonarcloud.yml docs/ci.md docs/adr/ADR-084-rust-analyzer-runtime-and-lsp-lifecycle.md docs/adr/ADR-085-m6-runtime-admission.md docs/validation/M6/01.md docs/validation/M6/01-calibration.json docs/validation/M6/01-config-schema.json docs/validation/M6/delegation/D25-D26-decision-brief.md
Message:

feat(m6): duplex LSP session, Phase::Analyzer, M6 image admission and native calibration (M6-01 gateway)

Adds lsp_session (bounded duplex stdio peer: 1 MiB frames, 4096 messages, 16 MiB/1 MiB stdout/stderr never published, deadline and cancellation on every wait, shutdown→exit→grace→kill, kill/reap evidence), analyzer_gateway with Phase::Analyzer (rust-analyzer 1.98.1 at /opt/analyzer/bin inside the existing volume/ingest/container flow, initialize with the fixed capabilities and 17-key initializationOptions, utf-8 asserted, experimental/serverStatus quiescent as readiness, didOpen version 1, one query, joined cleanup and quarantine), admission of the M6 image sha256:f39a5b33… (ADR-085, global list like ADR-077; M1–M5 stay qualified against their own digests), capture-time rejection of rust-analyzer.toml at any depth, and nine #[ignore] native calibration cuts driven by scripts/test-m6-runtime.py (full-gate stage m6-runtime). Calibration against the real binary: 9/9 (docs/validation/M6/01.md, receipt 01-calibration.json sha256 7b87268d…, config schema archived), health ok, quiescent 353–385 ms, only the admitted argv observed in the guest. ADR-084 §3 amended after the first run: cargo.sysrootQueryMetadata does not exist in the binary and cargo.autoreload=false produced a permanent warning; config_digest is now sha256:a2592cfc…. Independent review V04 (Opus, Block: 4 P2 / 8 P3, no containment finding) dispositioned and fixed in W04b.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HSFkiUg2PJKCiP83rqANwn

Commit 2 — stage exactly: docs/validation/M6/matrix.md docs/validation/M6/delegation/README.md docs/validation/M6/delegation/I04-integration docs/validation/M6/delegation/V04-review-lsp-session-gateway docs/validation/M6/delegation/W04-lsp-session-gateway docs/validation/M6/delegation/W04b-lsp-session-gateway-fixes docs/validation/M6/delegation/W05-symbols-tool
Message:

docs(m6): matrix, and the W04/V04/W04b/I04 delegation records

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HSFkiUg2PJKCiP83rqANwn

Do not push, do not edit files, do not run cargo. Report with headings: Task / Result / Files changed / Tests executed / Evidence (both hashes) / Risks / Decisions / Open issues.
