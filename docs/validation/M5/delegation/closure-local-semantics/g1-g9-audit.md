# Auditoría independiente G1–G9 — cierre local M5

Fecha de observación: 2026-09-10. Rama `ai/m5-performance`; HEAD observado
`9f889af216ff90e0913bfc18edba944ae68d8c7c`. Esta auditoría es read-only sobre
producto: no ejecutó Cargo ni Docker y no convierte gates pendientes en pases.

## Resultado

**NOT READY FOR DONE.** El diseño y las pruebas están implementados, pero al
momento de esta lectura faltan los recibos source-bound de runtime, clientes y
gates finales. Además permanece un P2 de seguridad que la propia disposición
llama obligatorio antes del cierre: la cobertura de `verify_applied` en M5 es un
subconjunto estricto de `rust_applied`.

## Mapa de puertas

| Gate | Evidencia que existe | Estado verificable y brecha |
| --- | --- | --- |
| **G1 — arquitectura y contrato** | ADR-073..081; tipos en `crates/domain/src/{benchmark,benchmark_compare,benchmark_run,bloat,profile,vendor_capture}.rs`; composición en `crates/application/src/`; gateway/adapters en `crates/execution-adapter` y `crates/project-adapter`; cuatro snapshots M5 y contract/protocol tests en `crates/mcp-server`. | **Implementado; gate final pendiente.** `scripts/check-architecture.py` liga la guarda estadística a `M5-02-method-simulation.json`. La invariancia heredada se comprobó separadamente abajo. Falta que `core/full` acrediten estos bytes finales. |
| **G2 — autoridad y threat model** | ADR-074/077/078; `docs/security-model.md`; gateway tipado, env cerrado, red `none`, seccomp por fase, vendor descriptor-bound y digest-bound; tests negativos y selecciones nativas en `performance_gateway.rs`, `performance_native.rs` y `vendor_capture.rs`; revisiones `docs/reviews/M5/m5-security/` y `closure-local-vendor/`. | **BLOCKED por finding existente.** `docs/security-model.md` declara que `verify_applied` no cubre toda la matriz ADR-064 y `docs/reviews/M5/m5-security/disposition.md` lo conserva como P2 sin corregir y obligatorio antes de cerrar. G8 dispone que un P2 de seguridad bloquea. |
| **G3 — lifecycle, cuotas y auditoría** | Budgets y caps cerrados en dominio/gateway; supervisor unido; cleanup/quarantine; artifacts privados con TTL/cuotas; selecciones de cancelación, descendiente, precreación, exceso y residuo en `performance_native.rs`; logs por stderr/tracing. | **Implementado; evidencia final pendiente.** Los JSON actuales bajo `docs/validation/M5-*-runtime.json` anteceden los cambios de volumen, governor, replay y semántica/logs, por lo que no acreditan los bytes actuales. Debe llegar el nuevo recibo con residue before/after y selecciones exactas. |
| **G4 — fixtures y pruebas** | Fixtures `benchmark`, `benchmark-compile-error`, `benchmark-datasets`, `profile-workload`, `bloat` y `criterion-vendor`; unit/contract/protocol/native tests; `scripts/test-m5-clients.py`, `m5-inspector-session.mjs` y 64 tests del oráculo Python pasados en esta revisión. | **BLOCKED hasta matriz final.** El `M5-clients.json` existente dice `tools_without_a_client_positive = [rust.benchmark.run, rust.benchmark.compare]` y `model_turn_completed = false` en runtime. El harness nuevo exige discovery→compare positivo→`NOT_A_DATASET`→Resource con IDs reales, pero aún necesita un receipt stock Codex/Inspector sobre esos bytes. |
| **G5 — gates y evidencia** | `scripts/gate.py` incluye fmt/check/clippy/test/doctests, arquitectura, audit/deny, helper guest clippy y `m5-runtime` en full. `scripts/test-m5-runtime.py` ejecuta cada ignored test exacto, uno por vez, y registra logs/digests. Intentos anteriores se conservan bajo `m5-gate-attempts/`. | **BLOCKED.** Core/full y runtime final están en ejecución o pendientes; ningún resultado se presupone aquí. Los intentos y receipts anteriores son historia, no acreditación del candidato actual. |
| **G6 — compatibilidad, migración y rollback** | ADR-076; formatos `benchmark-dataset.v2`, artifacts privados y receipts versionados; rechazo cerrado de formatos desconocidos; `docs/compatibility.md`; rollback por digest M4 y revocación de profiling documentados. | **Evidencia estática favorable; gate final pendiente.** M5 es aditivo y no migra catálogo/índice. La invariancia de las 27 tools heredadas está probada por diff abajo; los wire tests aún deben pasar en el gate final. |
| **G7 — operación y distribución** | `M5-provisioning.json` registra imagen, toolchain, binarios y red de provisioning; ADR-075/077 fijan aprovisionamiento/admisión por digest; README, SECURITY, `docs/tools.md`, `client-configuration.md` y `compatibility.md` describen operación, límites y revocación. | **Cierre local pendiente de gates y docs finales.** Instalar bytes empaquetados, attestations y checks live pertenecen a integración/release; no están autorizados en esta sesión y no se presentan como hechos. |
| **G8 — revisión independiente y bug bar** | Revisiones externas históricas de método y seguridad (`docs/reviews/m5-*`), revisión estadística agy, revisiones Codex de bloat/logs y fallback local final de vendor/semántica con hashes. V-01/S-01/S-02 tienen re-review; dos P3 finales están trazados. | **BLOCKED por el P2 de seguridad de `verify_applied`.** Claude Sonnet 5 no autenticó y Opus 5 no fue invocado para el delta final; esa limitación está registrada sin afirmar cuota. El fallback fue autorizado, pero no dispone el P2 anterior. |
| **G9 — DoR/DoD común** | Scope M5, cuatro contratos, método, fixtures, oráculos, rollback y documentación existen; `M5-matrix.md` conserva los cortes `In progress`. | **BLOCKED.** Depende de resolver G2/G8, recibir runtime/client/core/full finales source-bound y sincronizar matriz, handoff y tablero con esos resultados. M6 no puede comenzar por este estado. |

## Invariancia de las 27 tools heredadas

La base observada es `main = origin/main = c6099f27415b0be3838e84d21d25eed903c8c312`.
El árbol base contiene 28 snapshots: 27 tools más `doctor-report.json`; el
candidato contiene 32. `git diff --name-status main...HEAD --
crates/mcp-server/tests/snapshots` muestra exactamente cuatro archivos `A`:

- `benchmark-run-tool.json`
- `benchmark-compare-tool.json`
- `profile-flamegraph-tool.json`
- `binary-bloat-tool.json`

`git diff --exit-code main...HEAD -- $(git ls-tree -r --name-only main
crates/mcp-server/tests/snapshots)` terminó con exit 0 y diff vacío. Por tanto,
los 28 artifacts heredados —incluidos los 27 schemas de tools— son byte a byte
idénticos a M4. `protocol.rs` y el harness M5 esperan 31 tools y las cinco
versiones wire heredadas; su ejecución pertenece al gate final pendiente.

## Cómo queda ligada la evidencia a fuentes

- `scripts/gate.py` captura `source_inventory` al inicio y exige igualdad al
  final antes de publicar `passed`.
- `scripts/test-m5-runtime.py` hashea todos los `.rs`, manifests, lockfile,
  scripts, ADR/receipt de admisión, seccomp y fixtures; exige exactamente un test
  ejecutado por selección y registra el digest del receipt nativo tras cada paso.
- `scripts/test-m5-clients.py` registra hashes del server, inventario de fuentes
  y scripts de cliente, y vuelve a compararlos antes de publicar `passed`.

Un recibo final solo debe aceptarse si esos checks refieren el candidato que se
cierre; los receipts actuales de runtime/clientes no cumplen esa función después
de los cambios de cierre.

## Blockers reales

1. Resolver el P2 de seguridad `verify_applied` o emitir una disposición nueva
   coherente con la regla G8; hoy la propia documentación exige corregirlo antes
   de cerrar.
2. Obtener el receipt runtime final con todos los cortes y residue limpio.
3. Obtener la matriz Inspector + Codex runtime final, incluido el turno dirigido
   por modelo y Resource real.
4. Obtener `core` y `full` source-bound sobre los mismos bytes finales.
5. Actualizar matriz, handoff y `implementation-status.md` solo después de leer
   esos receipts.

La integración remota, PR/push/merge, smoke remoto, tag y release no están
autorizados. Siguen pendientes como evidencia de integración/release y esta
auditoría no los sustituye ni los declara fallidos.
