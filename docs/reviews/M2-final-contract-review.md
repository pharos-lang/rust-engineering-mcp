# M2 — revisión final de contratos

Claude Code 2.1.260, modelo explícito `claude-sonnet-5`, effort medium, read-only,
`--tools ''`, MCP vacío, sin implementación ni commits. Inputs y SHA por archivo:
[primer paquete](M2-final-contract-inputs.json),
[recheck](M2-final-contract-recheck-inputs.json). Resultados brutos:
[primero](M2-final-contract-sonnet.json), [recheck](M2-final-contract-recheck-sonnet.json).

## Resultado

**Accepted en recheck; ningún P0/P1 pendiente en este alcance.** El primer paquete
no incluía el cableado stdio y planteó una preocupación de configuración sin vendor.
La revisión posterior verificó los cinco grants independientes, el rechazo de
replay cruzado y la independencia de receipts/recovery respecto del dataset.

| Finding inicial | Disposición y evidencia |
| --- | --- |
| P1 wiring no suministrado | Resuelto: stdio construye cinco WriteConfig desde cinco vectores diferentes; provider y kind ligan preview/commit/receipt; prueba MCP add→remove deniega. |
| P1 grant sin vendor al arrancar | Revisor acepta decisión: configuración válida para receipt/recovery; el preview que necesita datos falla con motivo explícito. Requerir vendor en startup impediría recuperación sin datos innecesarios. |
| P2 schema de manifest_path | Corregido: regex exacta de componentes portables excluye `.`/`..`; pruebas preservan hidden dirs y `...`. |
| P2 preflight léxico | Aclarado: es rechazo temprano, la autoridad real procede de handles del adapter nativo; no se acredita containment por starts_with. |

La revisión cubre contratos, wiring, application y pruebas suministradas. No sustituye
la revisión Opus de ejecución/nativo ni el gate conjunto final. Los trece snapshots
M1 permanecen intactos. El cambio posterior de versión identifica el checkout como
0.2.0-dev y no publica una release.

## Identidad observada

- Primera revisión: 194297 ms; modelos facturados/registrados: claude-haiku-4-5-20251001, claude-sonnet-5.
- Recheck: 54792 ms; modelos facturados/registrados: claude-haiku-4-5-20251001, claude-sonnet-5.
El CLI registró llamadas auxiliares Haiku en algunos runs; el dictamen principal
procede de Sonnet 5 solicitado. No se configuró modelo fallback.
