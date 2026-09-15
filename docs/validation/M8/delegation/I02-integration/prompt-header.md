# I02 — integración M8-02: commits en `ai/m8-stabilization`

Modelo solicitado: Claude Sonnet 5 (`claude -p --model sonnet --effort low`). Rol: integrador (solo `git add`/`git commit`; **sin editar archivos**). Orquestador: Claude Fable 5.1. No puedes lanzar subagentes. Nunca corras comandos en segundo plano. **No hagas push.**

Antes: `git status --short`; `python3 -B scripts/docs-hygiene.py links-check` y `verify-inventories` verdes (si no, para y reporta). Crea exactamente estos commits, en orden, con `git add` de las rutas indicadas y el mensaje exacto (termina cada uno con las dos líneas de atribución).

**Commit 1** — rutas: `docs/adr/ADR-087-1.0-host-scope.md`, `docs/adr/README.md`, `docs/roadmap/adr-backlog-m2-m8.md`, `docs/spec/rust-engineering-mcp-propuesta-v0.3.md`
```
docs(m8): decide D13 = A — ADR-087 fixes the 1.0 host scope to macOS ARM64

Owner decision (2026-09-14): 1.0 qualifies and publishes only macOS ARM64 with the
Docker Linux ARM64 gateway; Linux/Windows x86_64 stay portable CI without positive
capabilities or artifacts; the Windows CI retirement is portability debt, not a
1.0 criterion. Spec §61/§97 gain a scope clarification note (no rewrite); the 1.0
checklist item "cross-platform" closes as "resolved by approved scope change".

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HSFkiUg2PJKCiP83rqANwn
```

**Commit 2** — rutas: `crates/mcp-server/`, `Cargo.toml`, `Cargo.lock`
```
feat(m8): 0.8.0 contract freeze — stability classes, preview prefix, `contract` document

Version 0.8.0 (workspace + lock). Each tool exposes a static definition(); a
closed Stability table marks the five rust.analyzer.* tools `preview` and prefixes
their descriptions with "Preview (ADR-086): " (5 snapshots regenerated, the other
31 byte-identical). New static CLI `contract [--json | --human]` (spec §56):
document_kind/format_version/server_version, negotiable MCP revisions, and per
tool stability, annotations, canonical sha256 of inputSchema/outputSchema/
description, executes_project_code (14 tools) and requires_runtime; dynamic
Resource templates. Portable tests pin the hashes against the snapshots, the
protocol/SDK/template literals against their sources, and the 14-tool set.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HSFkiUg2PJKCiP83rqANwn
```

**Commit 3** — rutas: `scripts/contract-freeze.py`, `scripts/test-contract-freeze.py`, `scripts/gate.py`, `scripts/release-smoke.py`, `.github/workflows/sonarcloud.yml`
```
feat(m8): contract-freeze oracle — manifest generate/verify/diff and mandatory gate stage

scripts/contract-freeze.py generates the freeze manifest (canonical sha256 per tool,
recorded stability class), verifies the current snapshots against it (any stable
change, class change, count change or missing manifest fails; preview changes warn
unless --strict) and diffs snapshots against a git ref (bytes read via git show
--end-of-options, ref validated with rev-parse --verify). gate.py core gains
`contract-freeze-tests` and the unconditional `contract-freeze` stage. Hermetic
unit tests with git doubles; sonarcloud.yml runs them under coverage.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HSFkiUg2PJKCiP83rqANwn
```

**Commit 4** — rutas: `CHANGELOG.md`, `README.md`, `docs/tools.md`, `docs/compatibility.md`, `docs/client-configuration.md`, `docs/implementation-status.md`, `docs/ci.md`
```
docs(m8): 0.8.0 migration notes, error-code families, stability classes, status board

CHANGELOG "0.8.0 — freeze de contratos" with migration notes 0.3.0 → 0.8.0 (31→36
tools, new host grant, binary.bloat $defs description-only schema change, thirteen
M1 tools byte-identical to 0.1.0, zero deprecations announced, `contract` document
stable). tools.md documents the two closed error-code families and the dynamic
Resources; compatibility/client-configuration carry a single preview note; the
status board gains the M8 section and marks M7 Deferred; ci.md records the
ADR-087 Windows note and the real stage counts (28 core / 44 full) with the M8
freeze stages.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HSFkiUg2PJKCiP83rqANwn
```

**Commit 5** — rutas: `docs/validation/M8/` (todo)
```
docs(m8): M8-02 record — decisions, schema diff, freeze manifest, delegation packages

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HSFkiUg2PJKCiP83rqANwn
```

Después: `git status --short` vacío (si queda algo, repórtalo sin añadirlo) y `git log --oneline -6`. Informe: Task / Result / hashes / `git status` / Open issues.
