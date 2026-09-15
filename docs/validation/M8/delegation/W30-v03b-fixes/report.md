# W30 — correcciones V03b (P2-1, P2-2, P3s)

Worker: Claude Sonnet 5 (`claude -p --model sonnet --effort medium`), sin subagentes, sin commit.

## Cambios

**(1) P2-1 — `codex_gate` (C-2).** `scripts/test-m8-clients.py`: extraída
`codex_classification(returncode, stderr, open_observed, inspect_observed,
unknown_tool_wire_refused)` con la regla `passed` exige
`unknown_tool_wire_refused`; la comprobación por transcript del modelo se
conserva solo como campo informativo `unknown_tool_event_refused` en
`protocol_evidence` (se retiró `unknown_tool_refused`/
`unknown_tool_refused_on_wire` combinados). Docstring de `codex_classification`
deja explícito por qué el evento no cuenta. Test nuevo:
`CodexClassificationTests` en `scripts/test-m8-clients-unit.py` (3 casos:
passed solo con refutación de wire, refutación solo por evento → `partial`,
`capacity_refused` por stderr).

**(2) P2-2 — `doctor_ok` en el escenario (a) (R-1(iii)).**
`scripts/test-m8-rollback.py`: `doctor_ok` ya no arranca en `True`; ahora es
`False` en la rama `else` (fallo de `doctor` o ausencia de
`mutation_journals.downgrade_blocked`), igual que el gap de calidad en (b).
Comentario actualizado. El test existente
`test_records_a_declared_gap_instead_of_failing_when_doctor_state_root_is_unsupported`
esperaba `status == "passed"` con el gap; se renombró a
`test_fails_with_a_declared_gap_when_doctor_state_root_is_unsupported` y ahora
exige `status == "failed"` (el gap se sigue registrando en `gaps[]`).

**(3) `regression_verdict` — `insufficient_samples` como `unavailable`.**
`scripts/measure-m8-performance.py`: `unavailable_count` ahora suma también
los recibos con verdict `insufficient_samples`. Docstring actualizado. Test
nuevo `test_insufficient_samples_counts_as_unavailable_not_within` en
`scripts/test-m8-performance-unit.py` (2× `insufficient_samples` + 1× `over` →
`indeterminate`, no `not_regressed`).

**(4) Recibo de clientes — límite de `id`.** `scripts/test-m8-clients.py`
(`run_inspector`): el reporte añade `wire_confirmation: "positional"` y
`wire_confirmation_note` explicando que la confirmación de cada negativo
genérico se hace por posición en el wire, no por `id` (el proxy no lo
registra en `safe_keys`), lo que es sólido para la sesión secuencial del
Inspector pero frágil ante un cliente que intercale llamadas.

**(5) `docs/tools.md` — frase de `doctor`.** La frase de la sección
`mutation_journals` que decía «solo metadatos del journal, nunca workspace ni
source» pasa a «no lee el workspace ni el source; abre el store de journals
(crea el lock del store si no existe) con la misma lectura que `mutation
list` (solo metadatos del journal)», precisando que `--state-root` sí
escribe el lock del store si no existe.

**(6) `01-census.json` — plantilla quality.** El literal
`rust-quality-artifact://{project_ref}/{quality_job_id_or_artifact_id}?offset={n}&length={n}`
pasa a la forma RFC 6570
`rust-quality-artifact://{project_ref}/{quality_job_id_or_artifact_id}{?offset,length}`.

## Verificación

- `python3 -B scripts/test-m8-clients-unit.py` → 95 tests, OK
- `python3 -B scripts/test-m8-rollback-unit.py` → 40 tests, OK
- `python3 -B scripts/test-m8-performance-unit.py` → 79 tests, OK (más la
  comparación de ejemplo del script, sin relación con el cambio)
- `python3 -B scripts/test-gate-reporting.py` → 13 tests, OK
- `python3 -c "import json;json.load(open('docs/validation/M8/01-census.json'))"` → OK
- `python3 -B scripts/docs-hygiene.py links-check` → 2865 enlaces resueltos, 0
  rotos en documentos vivos, 5 excluidos por `.gitignore`, 459 rotos en
  registros congelados (línea base sin cambios por esta tarea)

No se tocó ningún otro archivo fuera de la lista permitida. Sin commit.
