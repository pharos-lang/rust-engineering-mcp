# W15 — disposición del orquestador (2026-09-14)

Veredicto: **aceptado con incidencia de sesión**. El worker lanzó el driver en
segundo plano pese a la regla y su turno terminó sin informe ni recibo (el
proceso murió con la sesión). El orquestador ejecutó el driver en primer plano
(`python3 -B scripts/test-m8-rollback.py --old-tag v0.3.0 --out
docs/validation/M8/03-rollback.json`, exit 0): construyó `v0.3.0` (`6ea330d`)
en un worktree/target propios sin red y produjo el recibo con `status: passed`,
4/4 escenarios verificados por el orquestador en el recibo:

| Esc. | Evidencia |
| --- | --- |
| (a) | test nativo `leaves_a_committed_analyzer_action_apply_journal_for_an_older_binary_to_reject` (0.8.0) → `v0.3.0 mutation list --json` exit 1, `status: blocked`, `error_code: recovery_required` (fail-closed por registro, journal intacto) |
| (b) | estado M3 (`quality-artifacts recover`) y catálogo (`catalog import/status`) escritos por 0.8.0 leídos por `v0.3.0` (`passed`) |
| (c) | floor avanzado por 0.8.0 → `v0.3.0 catalog import` de secuencia menor: exit 1, `CATALOG_ROLLBACK` («requires a strictly newer signed sequence»); `status` sigue `passed` |
| (d) | estado `v0.3.0` → 0.8.0 lo lee y `doctor --json` lo valida (`status: warning` pasivo por catálogo de prueba) |

`test-m8-rollback-unit.py` verde. Hashes de ambos binarios en el recibo. El
recibo no entra en `core` (construcción de un segundo binario); se cita en la
matriz como evidencia M8-03. Deuda menor: el recibo no registra `version --json`
de cada binario (solo sha256 y ruta) → se añade en una pasada posterior si se
repite en RC.
