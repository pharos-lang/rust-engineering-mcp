# Revisión independiente M5 — semántica de bloat y artifacts de logs

Actúa como revisor externo read-only. Estás en el repositorio
`/Users/cburgosro/Projects/rust-mcp`. No implementes, no modifiques archivos, no
hagas commits y no ejecutes comandos, compilaciones ni pruebas. Usa únicamente
Read, Glob y Grep para inspeccionar archivos. La versión y modelo se registran
fuera de este prompt.

## Objetivo y Definition of Done

Revisa si la implementación actual cumple exactamente:

1. ADR-079: `rust.binary.bloat` solo publica `passed` cuando las dos ejecuciones
   del analizador terminaron limpiamente, la salida fue parseable y el tamaño
   medido coincide; el tope de 256 filas y el recorte de respuesta se declaran
   sin convertir por sí solos un análisis válido en `blocked`.
2. ADR-080: cada repetición de `rust.benchmark.run` conserva stdout y stderr como
   artifacts privados, owner-bound, con índice 1-based, límites y truncación
   explícitos, UTF-8 realmente válido o sustitución declarada, asociación correcta
   con exit/archive, y sin contaminar stdout del protocolo.
3. Las pruebas y snapshots discriminan los fallos reales, incluidas: segunda
   ejecución de bloat fallida con JSON válido; tamaño inconsistente; logs no UTF-8;
   repetición fallida sin archive; recuperación por cliente; límites/presupuesto.

Archivos principales a leer:

- `AGENTS.md`
- `docs/adr/ADR-079-bloat-result-semantics.md`
- `docs/adr/ADR-080-harness-logs-as-artifacts.md`
- `crates/domain/src/bloat.rs`
- `crates/domain/src/benchmark_run.rs`
- `crates/execution-adapter/src/performance_port.rs`
- `crates/mcp-server/src/stdio/bloat.rs`
- `crates/mcp-server/src/stdio/benchmark.rs`
- `crates/mcp-server/src/stdio/benchmark/schemas.rs`
- `crates/mcp-server/src/stdio/quality_artifacts/performance.rs`
- `crates/mcp-server/tests/snapshots/binary-bloat-tool.json`
- `crates/mcp-server/tests/snapshots/benchmark-run-tool.json`
- `scripts/test-m5-clients-unit.py`
- `scripts/test-m5-clients.py`

Puedes seguir referencias directas dentro del repositorio solo cuando sean
necesarias para confirmar un contrato. No revises ADR-081 ni rehagas la revisión
estadística. Si encuentras relacionada la puerta direccional, limita la
comprobación a confirmar que `METHOD_QUALIFIED_FOR_DIRECTION` sigue en `false`.

## Salida obligatoria

Responde en español con estas secciones exactas:

`Task`
`Result`
`Files changed`
`Tests executed`
`Evidence`
`Risks`
`Decisions`
`Open issues`

En `Result`, emite un veredicto único: `PASS`, `PASS WITH FINDINGS` o `BLOCK`.
Cada hallazgo debe tener identificador, severidad `P0`–`P3`, archivo y líneas,
evidencia concreta, impacto y corrección recomendada. Separa defectos de código,
huecos de prueba y limitaciones de revisión. `Files changed` debe decir `None` y
`Tests executed` debe decir que no ejecutaste pruebas por la restricción read-only.
No inventes resultados dinámicos. Si no puedes acceder a un archivo o el contexto
es insuficiente, decláralo como limitación.
