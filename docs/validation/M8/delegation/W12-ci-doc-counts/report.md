# W12 — informe del worker (Claude Sonnet 5, Low; claude 2.1.268)

Invocación: `claude -p --model sonnet --effort low … < prompt-header.md`. Inicio 2026-09-14T21:19:53Z, fin 2026-09-14T21:20:41Z, exit 0, 13 turnos, 46351 ms.

---

**Files changed:** `docs/ci.md`

**Conteos antes→después:**
- Core: 26 → 28 etapas (añadidas `contract-freeze-tests`, `contract-freeze`)
- Full: 42 → 44 etapas (28 core + 14 nativas + `m5-runtime` + `m6-runtime`, sin cambios en la rama nativa)
- Etapas nativas: 14 (sin cambio, ya correctas — solo se re-verificó la lista contra `scripts/gate.py`)
- Añadida sección `## M8 — etapas de freeze de contrato` describiendo orden, verificación (hash canónico de schema/annotations/description por tool, clase `stable`/`preview`, conteo total) y el recibo `core-gate.json` (28 etapas, 2026-09-14)

**Open issues:** ninguno — `links-check` reporta 0 rotos en documentos vivos (los 459 rotos son en "frozen records", fuera de alcance).
