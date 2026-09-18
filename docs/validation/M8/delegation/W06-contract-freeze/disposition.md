# W06 — disposición del orquestador (2026-09-14)

Veredicto: **aceptado con un hallazgo material del worker**. Verificado por el
orquestador: `test-contract-freeze.py` 10/10, `test-gate-reporting.py` 13/13,
`02-schema-diff.json` válido: 13 M1 `unchanged` desde `v0.1.0`; desde `v0.3.0`
5 `added`, 30 `unchanged`, 1 `changed` (`rust.binary.bloat`,
`keys_changed: [inputSchema, outputSchema]`). Etapa `contract-freeze-tests`
añadida al `core`; `contract-freeze` condicional a la existencia del manifiesto
(sin patrón `skipped` en `gate.py`; se activa cuando el orquestador genera el
manifiesto tras W05).

| ID | Sev | Hallazgo | Disposición |
| --- | --- | --- | --- |
| W06-H1 | P2 (exactitud de evidencia) | La decisión 6 de `02.md` decía que `binary.bloat` cambió solo en `description`; el cambio real está en `$defs/BloatProfile/description` de ambos schemas | **Aceptado**: `02.md` decisión 6 y el encargo W08 (migration notes) corregidos por el orquestador; la semántica de validación no cambia, pero se declara como cambio textual de schema |
| W06-R1 | P3 | `verify` no usa `snapshot_sha256` como criterio | Aceptado: intencional; el manifiesto lo conserva como evidencia y `--strict` cubre RC |
