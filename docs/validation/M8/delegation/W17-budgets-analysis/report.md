# W17 — informe del worker (Claude Sonnet 5, High; claude 2.1.268)

Invocación: `claude -p --model sonnet --effort high … < prompt-header.md`. Inicio 2026-09-14T23:16:07Z, fin 2026-09-14T23:27:45Z, exit 0, 73 turnos, 696656 ms. Entregable: [05-budgets-analysis.md](../../05-budgets-analysis.md).

---

0 broken links in living documents, file is untracked as expected (no commit made). The document is complete.

## Informe

**Task**: W17 — inventariar mediciones existentes relevantes a M8-05 (startup, dispatch, RSS, tamaño binario/artifact, cancel p95, cleanup) y proponer, con citas, cómo medirlas y qué presupuestos fijar antes de medir, más una propuesta de soak. Único archivo escrito: `docs/validation/M8/05-budgets-analysis.md`.

**Result**: Documento de 4 secciones (inventario §1, método por magnitud §2, presupuestos propuestos §3, soak §4). Hallazgo central: **M5 no mide nada de esto** (sus cuatro tools miden código del *peer*, no el propio servidor); las cifras reales más cercanas vienen de M3 (`02-budgets.json`: única medición de `cancel_to_cleanup_ms`=1088 ms y `eof_to_join_ms`=346 ms, N=1, con Docker), M4 (`m4-budgets.json`: N=30 cold/warm, precedente de método), recibos de release 0.1.0/0.3.0 (tamaño de binario/archive) y `release-smoke.py` (latencias de handshake/dispatch, N=1). **RSS del servidor no existe en ningún perfil** — lo único parecido es una medición de RSS de un test de journaling no relacionado (M2/07) que el propio documento excluye explícitamente del MCP.

**Conteos**: ~40 citas ruta+valor+commit/fecha; 9 magnitudes del plan cubiertas en §2; tabla de 9 presupuestos en §3 (2 con medida previa parcial —tamaño de binario/archive—, 7 marcadas «sin medida previa»); soak §4 con criterios de fallo fijados para ambos perfiles.

**Risks**: los placeholders de p95 cancel-observed (3000 ms) y cleanup (1000 ms) están basados en N=1 con demora sintética — deliberadamente holgados y marcados para recalibración obligatoria única antes del primer RC, no deben tratarse como presupuestos reales.

**Open issues**: (1) no hay evidencia de determinismo bit-a-bit del build entre entornos (CI vs. local difieren en 0.1.0 y entre builds locales en 0.3.0) — recomendado verificar antes de fijar el presupuesto de tamaño de binario; (2) el perfil `core` sin Docker no tiene hoy ninguna operación cancelable de duración no trivial, así que su soak omite el ciclo de cancelación salvo que se añada una.

Verificación: `python3 -B scripts/docs-hygiene.py links-check` → 0 rotos en documentos vivos (459 rotos preexistentes en registros congelados, fuera de alcance). No se hizo commit.
