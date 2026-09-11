# W02 — Write ADR-083 (D25: analyzer contract and actions) and ADR-084 (D26: rust-analyzer runtime and LSP lifecycle)

Model requested: Claude Sonnet 5 (`claude -p --model sonnet --effort medium`). Role: documentation worker. The decisions are already taken by the orchestrator (Claude Fable 5.1); you write them down in ADR form, you do not change them. If something in the brief is contradictory or missing, stop and report it under "Open issues" instead of deciding.

## Inputs (read all, in this order)

1. `docs/validation/M6/delegation/D25-D26-decision-brief.md` — the binding decisions, including §4 (closure of the `[R01]` items).
2. `docs/validation/M6/delegation/R01-ra-research/disposition.md` — what of the research is accepted and why the report is auxiliary evidence only.
3. `docs/roadmap/adr-backlog-m2-m8.md` entries D25 and D26; `docs/roadmap/m6-analyzer.md`; `docs/roadmap/m6-provisioning-request.md`.
4. Shape and tone references: `docs/adr/ADR-075-m5-runtime-provisioning.md`, `docs/adr/ADR-076-m5-performance-contracts.md`, `docs/adr/ADR-050-local-coordinated-mutation.md`, `docs/adr/ADR-052-mutation-journal-and-authorization.md`, `docs/adr/ADR-009-deny-by-default-security.md`.

## Deliverables (only these files)

1. `docs/adr/ADR-083-analyzer-contract-and-actions.md` — resolves D25. Sections: `Context`, `Decision` (tool inventory of exactly five tools; inputs; common output envelope; per-tool results and bounds; the `action_digest`/preview/commit/receipt flow reusing the M2 writer through `MutationCandidate`/`MutationPlans`/`commit_mutation` with a new `MutationKind::AnalyzerActionApply`; the host permission `--allow-analyzer-action-write <root>`; the WorkspaceEdit acceptance rules and the closed rejection reasons; positions in Unicode scalars 1-based; deferred capabilities hover/definition/rename), `Alternatives considered` (five tools vs. one multiplexed tool; actions as Resource; apply through a typed mode of an existing M2 tool; exposing hover/definition), `Consequences` (semver: 31 existing tools untouched, 5 new snapshots + wire tests; G6 note on journals with an unknown kind), `Status`: Accepted (orchestrator decision 2026-09-11 within the M6 assignment; D25 was Proposed).
2. `docs/adr/ADR-084-rust-analyzer-runtime-and-lsp-lifecycle.md` — resolves D26. Sections as above. `Decision` must cover: exact identity (rust-analyzer 1.98.1 aarch64-unknown-linux-gnu from the pinned channel manifest, xz sha256 `a0fd960a9ab36193ae9ba4310e5f780f6ca38fa86160fae739be4ac541b6d10c`; rust-src xz `5c846ebcebcc7e2e0777a4cdaa12051691593f16a7e94edbae5e6241cc62d98c`; image M6 derived from the admitted M5 digest — cite ADR-082 for provisioning and say admission is a separate ADR with native qualification); transient instance per query (chosen over a keyed pool, with the reason); the seven lifecycle phases; the new duplex stdio session in the execution adapter supervisor and the new `Phase::Analyzer` of the single gateway (no `Command` outside it; LSP codec is not a second MCP/JSON-RPC stack); the minimal client capabilities and the fixed `initializationOptions` (list every key/value from brief §4.5) with `config_digest`; the readiness oracle (`experimental/serverStatus` quiescent); pull diagnostics; utf-8 encoding negotiation and the byte→Unicode-scalar translation; capture-time rejection of `rust-analyzer.toml`/`.rust-analyzer.toml` and why (workspace config overrides client config and cannot be disabled); the expected external processes during initialize and the "any other program is a P1" rule; the budget table of brief §2.4 verbatim; SLIs; rollback; native scope macOS ARM64/APFS + guest Linux ARM64 only. `Alternatives considered`: keyed pool; host-side rust-analyzer; batch CLI (`rust-analyzer diagnostics`/`scip`); push diagnostics with a silence timer; advertising snippets/commands/resource operations. `Status`: Accepted (orchestrator decision 2026-09-11; D26 was Proposed). Include a `Sources` section listing the LSP 3.17 spec URL, the rust-analyzer book/configuration URLs, the channel manifest URL, and note that R01 findings are verification targets for the native calibration, not evidence.
3. `docs/roadmap/adr-backlog-m2-m8.md`: change D25 and D26 `Status` lines to `Decided → ADR-083` / `Decided → ADR-084` (keep the rest of each entry intact; do not touch other entries).
4. If `docs/adr/README.md` or any index lists ADRs by number, add rows for ADR-082 (title "Aprovisionamiento del runtime M6", being written by another worker — add the row only if the index already has ADR-081 and the file `docs/adr/ADR-082-m6-runtime-provisioning.md` exists at the time you run; otherwise leave a note in your report), ADR-083 and ADR-084.

## Constraints

- Language: Spanish, same register as ADR-075/076. Dates: 2026-09-11.
- Do not invent facts: every number and key comes from the brief. Where the brief says a value is fixed "subject to native calibration", say so.
- Do not edit any other file. Do not commit. Run `python3 -B scripts/docs-hygiene.py links-check` at the end and require "0 broken in living documents".

## Report (mandatory headings)

Task / Result / Files changed / Tests executed / Evidence / Risks / Decisions / Open issues.
