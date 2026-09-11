# Revisión local fallback — ADR-079, ADR-080 y G4

## Task

Revisión independiente de la semántica de éxito de `rust.binary.bloat`, la
publicación de logs de harness y el flujo de cliente stock dirigido por modelo.
El alcance exacto está congelado en `reviewed-files.sha256`.

## Result

**PASS WITH P3 FOLLOW-UP.** Los dos P2 originales quedaron corregidos y el flujo
runtime de Codex ahora demuestra una secuencia G4 con resultados observables e
identidades emitidas por la misma sesión. No quedan findings P0-P2 en este paquete.

## Files changed

La corrección G4 modificó `scripts/test-m5-clients.py` y
`scripts/test-m5-clients-unit.py`. Esta revisión además actualizó este recibo y
su manifiesto. No modificó código Rust ni documentación normativa.

## Tests executed

- `python3 -m py_compile scripts/test-m5-clients.py scripts/test-m5-clients-unit.py`
- `python3 scripts/test-m5-clients-unit.py` — 64 tests, OK.
- `git diff --check -- scripts/test-m5-clients.py scripts/test-m5-clients-unit.py crates/execution-adapter/src/performance_port.rs`

No se ejecutó Cargo, Docker ni la matriz de clientes.

## Evidence

### S-01 — RESOLVED — techo después de normalizar UTF-8

`crates/execution-adapter/src/performance_port.rs:418-455` normaliza los bytes
arbitrarios y aplica un segundo corte en frontera UTF-8. La prueba de líneas
1473-1483 usa exactamente 256 KiB de `0xff`, exige reemplazo y truncación, y
comprueba que el artifact final sea UTF-8 y no exceda 256 KiB.

### S-02 — RESOLVED — una sola captura explica el fallo bloat

`bloat_observation` (`performance_port.rs:1032-1060`) deriva exit, termination,
report, stdout, stderr y ambos flags de `failed_capture`. La prueba de líneas
2100-2151 hace fallar la vista crates con stderr distinto, conserva JSON válido
y exige que todos los campos publicados describan esa segunda ejecución.

### S-03 — P3 — falta una prueba cliente de reemplazo más truncación

Los tests Rust ya discriminan el caso y el contrato limita el artifact. La
matriz cliente no construye un payload que lleve simultáneamente `replaced` y
`truncated` a través de schema y lectura como Resource. Es una mejora de defensa
en profundidad y no bloquea el contrato cubierto en la frontera que transforma
los bytes.

### S-04 — RESOLVED — turno runtime dirigido por modelo para G4

`docs/roadmap/m2-m8.md:127-128` exige cliente stock dirigido por modelo con
discovery, positivo, fallo y Resource. El flujo anterior solo dirigía por modelo
las negativas Docker-free y aceptaba la mera presencia de nombres de tools.

`scripts/test-m5-clients.py` ahora conserva los dos dataset IDs y un
`criterion_archive` reales emitidos por la sesión, y el turno runtime ejecuta
exactamente `list_mcp_resources` → comparación positiva → comparación bloqueada
`NOT_A_DATASET` con el artifact real de tipo incorrecto → `read_mcp_resource`.
`validate_runtime_model_flow` exige argumentos exactos, lifecycle, status,
error_code, orden, ausencia de retry y contenido ligado a la URI. Los tests
unitarios rechazan omisión, retry, otro MCP e identidad sustituida.

## Risks

El nuevo turno dirigido por modelo aún necesita ejecutarse en la matriz runtime
final. Los unit tests prueban el oráculo y su cierre, no sustituyen la evidencia
del cliente stock.

## Decisions

- `METHOD_QUALIFIED_FOR_DIRECTION` sigue en `false`; la comparación runtime de
  una ejecución por lado termina antes por `insufficient_executions` y no finge
  haber ejercitado el guard global.
- El fallo dirigido por modelo reutiliza un `criterion_archive` emitido por la
  misma sesión como candidato de comparación. No inventa un ID ni repite una
  medición costosa.
- Los logs mantienen asociación 1-based por repetición y artifacts privados.

## Open issues

- Ejecutar la matriz runtime final y conservar sus eventos/receipt.
- Considerar S-03 como hardening posterior.
- La revisión externa Claude Sonnet 5 no se ejecutó: el CLI no pudo autenticarse
  en el primer intento y el reintento escalado fue interrumpido sin salida. No se
  verificó disponibilidad del modelo ni se observó agotamiento de cuota.
