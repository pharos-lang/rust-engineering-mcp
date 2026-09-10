# M5 — matriz de implementación y calificación

Estado: **In progress**, 2026-09-10. Cierre local autorizado por
[complete-m5](../prompts/complete-m5.md). Rama `ai/m5-performance`; `main`
permanece en `c6099f27415b0be3838e84d21d25eed903c8c312`.

El candidato se integra en commits locales y se mide desde el worktree limpio
`/private/tmp/rust-mcp-m5-closure`. La integración remota, su smoke y una release
quedan fuera de esta autorización. M6 no está iniciado.

## Contrato vigente

| Decisión | Contrato |
| --- | --- |
| [ADR-073](../adr/ADR-073-benchmark-method-and-dataset.md) | Criterion 0.8.2, dataset v2, parámetros cerrados, observación del guest y método explícito |
| [ADR-074](../adr/ADR-074-profiling-capability-and-containment.md) | Profiling con capability del host, sin privilegios adicionales |
| [ADR-075](../adr/ADR-075-m5-runtime-provisioning.md) / [077](../adr/ADR-077-m5-runtime-admission.md) | Aprovisionamiento explícito e imagen admitida por digest |
| [ADR-076](../adr/ADR-076-m5-performance-contracts.md) | Cuatro tools; los 27 contratos anteriores se conservan |
| [ADR-078](../adr/ADR-078-offline-vendor-capture.md) | Captura vendor separada; límites de SourceBundle intactos |
| [ADR-079](../adr/ADR-079-bloat-result-semantics.md) | Bloat `passed` significa análisis ejecutado y validado; ranking acotado declarado |
| [ADR-080](../adr/ADR-080-harness-logs-as-artifacts.md) | Logs privados por repetición con truncación y reemplazo declarados |
| [ADR-081](../adr/ADR-081-benchmark-statistical-requalification.md) | Criterios congelados; `METHOD_QUALIFIED_FOR_DIRECTION=false` |

## Cortes y evidencia

| ID | Corte | Estado actual | Evidencia |
| --- | --- | --- | --- |
| M5-01 | Benchmark existente → dataset, archivo Criterion y logs | Implementado; calificación final en ejecución | `performance_native::m5_benchmark_run_measures_criterion_through_a_vendor_capture` y suite descubierta por `scripts/test-m5-runtime.py` |
| M5-02 | Dos datasets del store → compatibilidad → comparación | Implementado; guardas conservadas; gate final pendiente | `benchmark_compare.rs`, fixtures de datasets reales, `performance_environment.rs`, matriz de clientes final pendiente |
| M5-03 | Capability → muestras/stacks/SVG → Resource | Recalificación por cambio de fingerprint en ejecución | Positivo, cero muestras, denegación, descendiente, cancelación y artifact precreado en `performance_native.rs` |
| M5-04 | Build → tamaño exacto y atribución estimada | ADR-079 implementado y re-revisado; recalificación pendiente | [Revisión](m5-delegation/closure-local-semantics/review.md), tests de bloat |
| M5-05 | Clientes, G1–G9 y cierre conjunto | In progress | Gates finales pendientes |

## Correcciones de cierre

- El volumen vendor de captura usa `size=512m,nr_inodes=32768`. Creación,
  fingerprint y cleanup usan las mismas opciones. El snapshot pequeño conserva
  su perfil previo; no se amplió SourceBundle.
- El replay verifica sello y SHA-256 incremental. Autentica el bloque que
  completa la longitud antes de entregarlo al supervisor; no depende de una
  lectura EOF que el supervisor no solicita. Una modificación invalida el handle.
- El governor se lee mediante un argv cerrado para los IDs CPU observados en el
  guest. Ausencia, heterogeneidad o datos inválidos conservan `None`; timeout,
  cancelación y exceso de salida abortan la operación. No se declara el governor
  del host físico.
- Los logs respetan 256 KiB después de normalizar UTF-8. Bloat alinea exit,
  terminación, report y logs con la ejecución que determina el fallo.
- Los clientes comparan IDs emitidos por dos mediciones reales. Con una ejecución
  por lado, el oráculo es exactamente `insufficient_executions`. El turno stock
  dirigido por modelo añade comparación positiva, rechazo de otro tipo de
  artifact y lectura de una Resource real.

## Revisiones y disposición

La auditoría G1–G9 recuperó el P2 histórico de `verify_applied`: M5 verifica un
subconjunto de `rust_applied` en el snapshot auditado. Se corrigió en `89ec114`
según ADR-074 §3.1 y pasó la
[re-review estática](m5-delegation/closure-applied-security/review.md).
La calificación nativa y conjunta sigue siendo obligatoria antes de Done.

[Vendor](m5-delegation/closure-local-vendor/review.md) y
[semántica/logs](m5-delegation/closure-local-semantics/review.md): re-review sin
P0–P2 abiertos. Dos P3 permanecen trazados: verificar al reutilizar un nombre
existente dentro de la API de captura (su llamador actual ya verifica), y una
fila cliente que combine reemplazo y truncación de logs (la transformación tiene
prueba Rust discriminante). No debilitan una guarda de producto.

Claude Sonnet 5 no produjo revisión autenticada; Opus 5 no fue invocado. Se usó
el fallback Sol autorizado, sin atribuirle revisión de otra familia ni afirmar
agotamiento de cuota. [Intentos](m5-delegation/closure-sonnet5-semantics/attempts.md).

## Límites que permanecen

- No se califican veredictos direccionales ni `no_material_change`: la guarda
  global continúa en `false`. Los criterios de precisión, potencia y el umbral
  material del 5 % no cambian. Se publican efecto, intervalo y razones declaradas.
- El governor desconocido no se completa con un valor supuesto. La evidencia del
  guest no demuestra condiciones de la máquina física.
- Los techos de ADR-078 son política; no prometen ingesta de toda captura situada
  exactamente en el borde. El punto de calificación es el cierre real registrado.
- Las muestras provienen del harness del proyecto y no son observaciones
  autenticadas de su comportamiento. La custodia del artifact no prueba la
  veracidad científica de una muestra.
- El target positivo sigue siendo macOS ARM64 con guest Linux ARM64 e imagen
  `sha256:e0a5ca1661b3e49d0a3d68ee3cc0963453078d08eb7fc43c30538c16b7998aac`.

## Historia preservada

Los recibos anteriores se conservan byte por byte en
[m5-closure-history](m5-closure-history/inventory.json). Acreditan sus fuentes y
contratos anteriores, no el candidato nuevo. El oráculo de que Criterion no cabe
en SourceBundle permanece válido para esa vía; no implica que la captura
independiente de ADR-078 esté bloqueada. Las calibraciones históricas y revisiones
originales permanecen en sus rutas; no se cambian números medidos para cerrar M5.
