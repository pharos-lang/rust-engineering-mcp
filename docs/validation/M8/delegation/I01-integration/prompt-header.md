# I01 — integración M8-01: tres commits en `ai/m8-stabilization`

Modelo solicitado: Claude Sonnet 5 (`claude -p --model sonnet --effort low`). Rol: integrador (solo `git add`/`git commit`; **sin editar archivos**). Orquestador: Claude Fable 5.1. No puedes lanzar subagentes. Nunca corras comandos en segundo plano. **No hagas push.**

Antes de nada: `git status --short` y `git diff --stat` para ver el árbol; `python3 -B scripts/docs-hygiene.py links-check` y `python3 -B scripts/docs-hygiene.py verify-inventories` deben estar verdes (si no, para y reporta). Luego crea exactamente estos tres commits, en este orden, con `git add` de las rutas indicadas (nada más) y el mensaje exacto. Cada mensaje termina con las dos líneas de atribución indicadas.

**Commit 1** — rutas: `docs/adr/ADR-086-deprecation-and-freeze-policy.md`, `docs/adr/README.md`, `docs/roadmap/adr-backlog-m2-m8.md`, `docs/compatibility.md`
```
docs(m8): decide D11 — ADR-086 deprecation and freeze policy (0.8 → 1.0)

Stability classes with evidence (stable/preview/experimental/internal),
"real consumer" defined (stock-client receipt or native e2e through the wire),
0.8.0 freeze with deprecations announced there, removal only in 1.0 of what was
announced, 1.x → 2.0 rule per spec §58; ADR-012 untouched. Backlog D11 → Accepted;
ADR index gains ADR-078/079/081/085/086; compatibility.md documents the policy and
the preview class of the five analyzer tools.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HSFkiUg2PJKCiP83rqANwn
```

**Commit 2** — rutas: `crates/mcp-server/src/main.rs`, `docs/tools.md`, `docs/architecture.md`, `docs/client-configuration.md`, `README.md`, `docs/roadmap/m2-m8.md`
```
fix(m8): --help lists all 36 tools; public docs describe the integrated M6 state

The --help "Available tools" literal omitted the five rust.analyzer.* tools and
the M6 runtime/grant requirements. docs/tools.md, architecture.md,
client-configuration.md and README described the checkout with 31 tools or M6
as "in development, not integrated"; they now separate release 0.3.0 (31) from
the checkout (36), document --allow-analyzer-action-write and the preview class.
m2-m8.md milestone table: M3–M6 Done with merge evidence, M7 Deferred, M8 in
progress. Verified: cargo fmt, clippy -p rust-engineering-mcp -D warnings,
cargo test --test cli (13/13), docs-hygiene links-check.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HSFkiUg2PJKCiP83rqANwn
```

**Commit 3** — rutas: `docs/validation/M8/` (todo el directorio)
```
docs(m8): M8-01 census, surface gate decisions and delegation register

36 tools (0 orphans, 2 dynamic Resources, 15 CLI commands, 10 disk formats),
11 findings dispositioned; 31 stable / 5 preview; 0 consolidations accepted with
compatibility analysis; D11 decided, D13 scope brief for the owner. Delegation
packages W01/W01b/W02/W03/W04, R01 (Gemini traceability audit) and V01/V01b
(Sonnet read-only reviews) with transcripts hashes.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01HSFkiUg2PJKCiP83rqANwn
```

Después: `git status --short` debe quedar vacío (si queda algo, repórtalo sin añadirlo) y `git log --oneline -4`. Informe: Task / Result / hashes de los tres commits / salida de `git status` / Open issues.
