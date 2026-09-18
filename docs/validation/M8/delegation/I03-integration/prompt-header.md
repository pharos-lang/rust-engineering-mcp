# I03 — integración M8-03/04/05/07/08: commits en `ai/m8-stabilization`

Modelo solicitado: Claude Sonnet 5 (`claude -p --model sonnet --effort low`). Rol: integrador (solo `git add`/`git commit`; **sin editar archivos**). Orquestador: Claude Fable 5.1. Sin subagentes, sin segundo plano, **sin push**.

Antes: `git status --short`; `python3 -B scripts/docs-hygiene.py links-check` y `verify-inventories` verdes (si no, para y reporta). Crea exactamente estos commits, en orden, con `git add` de las rutas indicadas (nada más) y el mensaje exacto; cada mensaje termina con las dos líneas de atribución.

**Commit 1** — rutas: `crates/domain/src/mutation.rs`, `crates/project-adapter/`, `crates/mcp-server/src/doctor.rs`, `crates/mcp-server/src/host_config.rs`, `crates/mcp-server/tests/doctor.rs`, `crates/mcp-server/tests/snapshots/doctor-report.json`
```
feat(m8): doctor.mutation_journals — passive downgrade preflight by journal kind (D12)

MutationRecordSummary gains the record kind; doctor reports pending/terminal
journals per kind, unreadable-or-unknown entries, and downgrade_blocked when a
journal is pending or its kind postdates the 0.3.0 baseline (analyzer_action_apply).
Works with --state-root alone, never reads the workspace, never blocks serve.
Native fixture: destination permissions revoked after Published leave a
recoverable non-terminal journal.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HSFkiUg2PJKCiP83rqANwn
```

**Commit 2** — rutas: `crates/mcp-server/src/stdio.rs`, `crates/mcp-server/src/stdio/resources.rs`, `crates/mcp-server/src/stdio/capability_document.rs`, `crates/mcp-server/tests/protocol.rs`, `crates/mcp-server/tests/cli.rs`
```
fix(m8): resources/list, resources/templates/list and prompts/list carry ttlMs/cacheScope

Under MCP 2026-07-28 the TypeScript SDK requires ttlMs/cacheScope on every list
result; rmcp's defaults omitted them and the Inspector rejected resources/list.
The three lists are now served explicitly (empty resources/prompts, the two
dynamic Resource templates in RFC 6570 form, shared constants with the contract
document), tested across all five negotiated revisions; the 36 tool snapshots
are byte-identical.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HSFkiUg2PJKCiP83rqANwn
```

**Commit 3** — rutas: `scripts/test-m8-rollback.py`, `scripts/test-m8-rollback-unit.py`, `scripts/measure-m8-performance.py`, `scripts/soak-m8.py`, `scripts/test-m8-performance-unit.py`, `scripts/test-m8-clients.py`, `scripts/test-m8-clients-unit.py`, `scripts/m8-inspector-session.mjs`, `scripts/release-smoke.py`, `scripts/gate.py`, `.github/workflows/sonarcloud.yml`, `.github/workflows/release-candidate.yml`, `sonar-project.properties`
```
feat(m8): M8-03/04/05/07 harnesses — rollback driver, performance budgets/soak, client matrix, release rehearsal

test-m8-rollback.py builds v0.3.0 in a worktree and proves rollback/upgrade
with positive controls; measure-m8-performance.py + soak-m8.py enforce the
budgets fixed before measuring (strict 2-of-3, sample minimums, lazy project
expiry reclaimed on next open); test-m8-clients.py qualifies Inspector/Codex
(mandatory), Claude Code and Gemini CLI with wire-derived oracles; release
workflow reads the tool count from the freeze manifest; release-smoke.py had
two stale schema pins fixed by the rehearsal. New core gate stages and
SonarCloud coverage entries.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HSFkiUg2PJKCiP83rqANwn
```

**Commit 4** — rutas: `docs/adr/`, `docs/roadmap/adr-backlog-m2-m8.md`, `docs/compatibility.md`, `docs/tools.md`, `docs/security-model.md`, `SECURITY.md`, `README.md`, `CHANGELOG.md`, `docs/publication.md`
```
docs(m8): ADR-088 (D12 migrations/rollback), ADR-089 (residual risks), ADR-090 (D14 offline verification)

Upgrade/rollback/backup procedures, error-code families and dynamic Resources
in tools.md, residual risks RR-01..RR-18 in the public security model (model
review only — no human audit or pentest for the first stable macOS release),
offline verification and incident response in publication.md, changelog entries.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HSFkiUg2PJKCiP83rqANwn
```

**Commit 5** — rutas: `docs/validation/M8/` (todo)
```
docs(m8): M8-03/04/05/07/08 records, threat model, budgets, receipts and delegation packages

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HSFkiUg2PJKCiP83rqANwn
```

Después: `git status --short` debe quedar vacío (si queda algo, repórtalo sin añadirlo) y `git log --oneline -6`. Informe: Task / Result / hashes / `git status` / Open issues.
