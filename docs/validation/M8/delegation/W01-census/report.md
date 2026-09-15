# W01 — informe del worker (Claude Sonnet 5, High; claude 2.1.268)

Invocación: `claude -p --model sonnet --effort high --permission-mode acceptEdits --disallowedTools Agent Task --allowedTools <lista> --no-session-persistence --output-format json < prompt-header.md`. Inicio 2026-09-14T13:44:33Z, fin 2026-09-14T14:08:39Z, exit 0, 234 turnos, 1444534 ms, modelos ['claude-haiku-4-5-20251001', 'claude-sonnet-5'], permission_denials: 14 (Bash fuera del allowlist: `awk`, `mkdir`, redirecciones a `/tmp`, bucles `for`, `git check-ignore`; el worker reformuló cada uno con herramientas permitidas y completó el censo).

---

## Informe final

**Task**: M8-01 — censo de invocaciones reales, contratos/errores/CLI/Resources/formatos, clasificación propuesta stable/preview/internal, gate de superficie, y reconciliación de `docs/roadmap/m2-m8.md`.

**Result**: Entregados los tres artefactos exactos pedidos. Censo completo y verificado live: 36/36 tools coinciden byte a byte entre `01-census.json` y la respuesta real `tools/list` (392 529 bytes) del binario contra un root temporal bajo `/private/tmp`. Huérfanos = 0. 9 findings (0 P0/P1, 4 P2, 5 P3), 4 de ellos `blocks_freeze: true`.

**Files changed**:
- `docs/validation/M8/01-census.json` (nuevo, 134 126 bytes)
- `docs/validation/M8/01-census.md` (nuevo, 317 líneas)
- `docs/roadmap/m2-m8.md` (editado: solo las 4 filas M3/M4/M5/M6 de la tabla de hitos, de "Planned" a "Done" con evidencia enlazada)

**Comandos ejecutados**: JSON-RPC de 3 líneas (`initialize`/`notifications/initialized`/`tools/list`) contra el binario con `--root /private/tmp/m8-census-root.v52Agq`; `--help`, `version --json`, `doctor --json`; múltiples scripts Python de medición sobre snapshots/ADRs/tests/receipts (en `target/m8-census/`, gitignored, no forma parte del entregable); `python3 -c "import json;json.load(...)"` (válido); `python3 -B scripts/docs-hygiene.py links-check` (0 rotos en mis archivos; 1 roto preexistente pertenece a `W02-adr-d11/report.md`, de otro worker); `git log --oneline --merges` y `git rev-parse` para los commits de cierre M3-M6.

**Conteos**: tools=36, resources=2 (ambos dinámicos, no enumerables), cli_commands=15, disk_formats=10, orphans=0, findings=9 (P2=4, P3=5, P0/P1=0).

**Top-10 propuestas del gate de superficie**: (1) `rust.quality.gate`↔`rust.quality.gate.v2` — candidato más fuerte, no fusionar antes de 0.8 freeze; (2) `rust.dependencies.audit`+`rust.deny`↔`rust.supply_chain.inspect` — ADR-071 confirma que este último ya los compone; (3) `rust.dependency.add`↔`remove`; (4) `rust.fmt.check`↔`rust.fmt.apply` (alto riesgo, cambia `readOnlyHint` de un contrato congelado); (5) `rust.test`↔`rust.test.nextest` (rust.test es una de las trece M1 congeladas, sin fusión posible); (6-8) las tres consultas LSP de solo lectura de M6 (baja prioridad); (9) ningún tool individual excede 5% de `tools/list`; (10) el coste agregado de 392 KiB se explica por `$defs` de envelope duplicados (hasta 31/36 tools redefinen `Data`), no por outliers.

**Risks**: F1/F2 (conteos de tools desactualizados en `docs/tools.md` y `--help`) y F5 (dos casings incompatibles de `error_code`) bloquean el freeze 0.8 y deben resolverse en M8-02.

**Open issues**: F4 (línea resumen de `m2-m8.md:12` quedó inconsistente con la tabla corregida, fuera del alcance textual autorizado); F3 (ADR-078/079/081/085 sin indexar); F7 (journal de mutación M2 sin `format_version` en archivo — insumo directo para D12); F8 (conteos de etapas de `docs/ci.md` no reconciliables por lectura estática de `scripts/`, fuera de mi alcance).

No se hizo commit.
