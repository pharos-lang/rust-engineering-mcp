# W07 — informe del worker (Claude Sonnet 5, Medium; claude 2.1.268)

Invocación: `claude -p --model sonnet --effort medium --permission-mode acceptEdits --disallowedTools Agent Task --allowedTools <lista> --no-session-persistence --output-format json < prompt-header.md`. Inicio 2026-09-14T16:01:27Z, fin 2026-09-14T16:03:19Z, exit 0, 36 turnos, 110739 ms, permission_denials: 0.

---

## Task / Result / Files changed

**Task:** D13 = A — redactar ADR-087 (alcance de hosts 1.0) y aclaraciones en spec/README/compatibility/ci.

**Result:** Completado, sin commit.

**Files changed:**
- `docs/adr/ADR-087-1.0-host-scope.md` (nuevo, 104 líneas) — Context/Decision (4 puntos)/Alternatives (B, C)/Consequences/Status Accepted/Sources.
- `docs/adr/README.md` — una viñeta añadida al final del índice.
- `docs/roadmap/adr-backlog-m2-m8.md:136` — `Status: Accepted, [ADR-087](../../../../adr/ADR-087-1.0-host-scope.md), 2026-09-14.`
- `docs/spec/rust-engineering-mcp-propuesta-v0.3.md` — nota al final de §61 y párrafo tras la lista de criterios de §97 (junto a `cross-platform`).
- `README.md` — reescrito el bloque `[!IMPORTANT]` de plataformas para enlazar ADR-087 y `docs/compatibility.md`.
- `docs/compatibility.md` — filas «Target de validación local» y «Linux / macOS x86_64 nativos» actualizadas con enlace a ADR-087.
- `docs/ci.md:119` — añadida la aclaración de deuda de portabilidad con enlace a ADR-087 en la fila de Windows retirado.

**Texto exacto añadido a la spec:**

§61 (tras el párrafo de ADR-048, antes de «Artefactos:»):
> Aclaración de alcance 1.0 (ADR-087, 2026-09-14): la matriz anterior sigue siendo aspiracional; 1.0 califica y publica únicamente macOS ARM64; Linux/Windows conservan CI de portabilidad sin capabilities positivas ni artifacts.

§97 (tras la lista de criterios «1.0.0», antes del separador):
> `cross-platform` se resuelve por cambio de alcance aprobado por el owner, no por la matriz aspiracional de §61: 1.0 califica y publica únicamente macOS ARM64 con gateway Docker Linux ARM64; Linux/Windows x86_64 conservan CI de portabilidad/fail-closed sin capabilities positivas ni artifact ([ADR-087](../../../../adr/ADR-087-1.0-host-scope.md)).

**Verificación:** `python3 -B scripts/docs-hygiene.py links-check` → 2590 links resueltos, 0 rotos en documentos vivos por mis cambios (los 2 rotos reportados están en `docs/validation/M8/delegation/W07-d13-scope/prompt-header.md` y `W08-freeze-version/prompt-header.md`, archivos preexistentes fuera de mi alcance de edición).

**Risks:** Ninguno identificado; los cambios son aditivos y no tocan ADR-047/048 ni `docs/publication.md`, tal como exige la decisión.

**Open issues:** `docs/validation/M8/delegation/README.md` seguía modificado desde antes de esta sesión (fuera de mis archivos permitidos) — no lo toqué; lo dejo para que el orquestador lo revise.
