# M5 — matriz de implementación y calificación

Estado: **In progress — bloqueado en el gate `full`**, 2026-09-10. Cierre local
autorizado por [complete-m5](../prompts/complete-m5.md) y continuado por
[finish-m5-fable](../prompts/finish-m5-fable.md). Rama `ai/m5-performance`;
`main` permanece en `c6099f27415b0be3838e84d21d25eed903c8c312`. Los cortes
M5-01..04 están calificados nativamente y por clientes y `core` pasa sobre las
fuentes finales; `full` falla en `semantic` porque `lancedb 0.38.0` no compila
con `default-features = false` (véase [Bloqueo](#bloqueo-lancedb-0380)).

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
| M5-01 | Benchmark existente → dataset, archivo Criterion y logs | Calificado nativamente y por clientes; cierre conjunto bloqueado | [captura](M5-01-capture-runtime.json), [negativos](M5-01-runtime.json), [gate nativo](M5-native-gate.json), [clientes](M5-clients.json) |
| M5-02 | Dos datasets del store → compatibilidad → comparación | Calificado por clientes con IDs reales; guardas conservadas; cierre conjunto bloqueado | [clientes](M5-clients.json): Inspector y Claude Code comparan datasets propios, `inconclusive`/`insufficient_executions`, `NOT_A_DATASET`; `benchmark_compare.rs`, `performance_environment.rs` |
| M5-03 | Capability → muestras/stacks/SVG → Resource | Calificado nativamente y por clientes; cierre conjunto bloqueado | [runtime](M5-03-runtime.json): positivo, cero muestras, denegación, descendiente, cancelación y artifact precreado; [clientes](M5-clients.json): 195 muestras, 0 perdidas, dos artifacts leídos |
| M5-04 | Build → tamaño exacto y atribución estimada | Calificado nativamente y por clientes; cierre conjunto bloqueado | [runtime](M5-04-runtime.json), [clientes](M5-clients.json) (`analysis_validated`, cargo-bloat 0.12.1), [revisión](m5-delegation/closure-local-semantics/review.md) |
| M5-05 | Clientes, G1–G9 y cierre conjunto | Clientes y `core` aprobados; `full` bloqueado | [clientes](M5-clients.json), [core](M5-core-gate.json), [full fallido](m5-gate-attempts/closure-full-attempt-2/full-gate.json), [G1–G9](m5-delegation/closure-local-semantics/g1-g9-disposition.md) |

## Gate nativo independiente

[M5-native-gate.json](M5-native-gate.json) registra seis selecciones aprobadas,
cada una exacta, ignorada y serial, sobre el candidato `a2464c4`. Los commits
posteriores hasta `aa935e6` solo modifican documentación y recibos; su diff sobre
código, scripts, fixtures, manifests y AGENTS es vacío.

La captura real tiene 161361408 bytes de artifact y 156267469 bytes de archivos.
El positivo completó tres ejecuciones y produjo 90 muestras para cada uno de los
tres benchmarks, con warmup solicitado de 3000 ms y medición de 5000 ms por
benchmark y ejecución. No se deriva de ello un veredicto direccional. Los
controles de digest, solo lectura y cancelación pasaron. Los cortes que usaron
Docker terminaron con inventario propio vacío; el oráculo del snapshot pequeño
no ejecuta contenedores.

El primer intento confinado al sandbox devolvió `Unavailable` al abrir el
runtime. El reintento con acceso Docker autorizado pasó completo, sin cambios de
código ni imagen. Ambos intentos se conservan en `m5-gate-attempts/`.
El gate full posterior debe volver a acreditar su etapa M5 dentro del conjunto.

## Matriz de clientes final

[M5-clients.json](M5-clients.json) (attempt-6, HEAD `dc7ce3e`, servidor
`9aaa85f0…`): Inspector 2.5.0 convierte quince filas —ocho docker-free y siete
runtime— y lee catorce Resources en runtime; Claude Code 2.1.267
(`claude-sonnet-5`) completa el turno docker-free con los cuatro rechazos
declarados y el turno runtime con siete llamadas exactas: open, discovery, dos
mediciones propias, comparación positiva `inconclusive`/`insufficient_executions`,
`NOT_A_DATASET` contra su `criterion_archive` y lectura nativa de esa Resource,
cuyos 40960 bytes hashean al `sha256` publicado. Codex quedó fuera por decisión
del owner tras agotar su cuota (attempt-2). Los intentos 2–5 fallidos se
conservan con causa en [attempts.md](m5-clients/attempts.md); el harness y su
revisión independiente (Gemini 3.8 y Claude Sonnet 5) están dispuestos en
[closure-claude-client-harness](m5-delegation/closure-claude-client-harness/disposition.md).

## Gates conjuntos

[M5-core-gate.json](M5-core-gate.json): `core` sobre `c334498`, 23/23 pasos,
1717 tests Rust y 108 Python, 1148 fuentes inventariadas sin cambios, 25,6 min.

[full-gate.json](m5-gate-attempts/closure-full-attempt-2/full-gate.json):
`full` sobre `c334498`, 31/34 pasos aprobados en 2 h 5 min —incluidos
`docker-security`, `rust-security`, `m2-runtime`, `m3-runtime`, `m4-runtime` y
`audit-data`— y fallo en `semantic`; `catalog*`, `crate-*`, `doctor` y
`m5-runtime` no se ejecutaron. La etapa M5 dentro del gate conjunto no tiene
recibo; el gate nativo independiente sigue siendo la única suite nativa
registrada sobre este código.

## Bloqueo: `lancedb 0.38.0`

`lancedb 0.38.0` no compila con `default-features = false`: `Error::Http` está
bajo `#[cfg(feature = "remote")]` y `src/job.rs::decode` lo usa sin `cfg`
(`error[E0599]`, líneas 56 y 66). Solo `--all-features` activa la feature `local`
del adapter semántico y con ella `lancedb`, por eso `core` pasa y `semantic` no.
Activar `remote` exige cambiar `Cargo.lock` y descargar dependencias; el patch
vendor es la 0.31.0 con ediciones de manifest y `verify-vendor.py` está anclado
a esa versión; revertir la subida del owner está prohibido. Detalle y opciones
en [m5-gate-attempts](m5-gate-attempts/README.md#intento-2--2026-09-10-full-failed-en-el-paso-32-de-34).
Hasta la decisión del owner, M5 no se declara Done.

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
  por lado, el oráculo es exactamente `insufficient_executions`. El cliente
  agentic es Claude Code 2.1.267 con `claude-sonnet-5` (decisión del owner del
  2026-09-10: Codex queda fuera de este cierre). Como los artifacts están ligados
  al `ProjectRef` del proceso que los publicó, su turno runtime mide dos veces
  por sí mismo antes de comparar en positivo, obtener `NOT_A_DATASET` con un
  artifact propio de otro tipo y leerlo como Resource; el oráculo exige cada
  paso exactamente una vez, con los argumentos del plan y los IDs emitidos en
  esa misma sesión.

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

## Deuda editorial de `a2464c4`: disposición sobre el código final

La lista de «Deuda de publicación» que publicaba la matriz en `a2464c4` se
contrastó con el código de `HEAD`, cuyo diff de código respecto a `a2464c4` es
vacío. Ninguna entrada falsea una medición, altera un número de recibo ni
debilita una guarda de producto; ninguna alcanza P2 según G8. No se modifica
código en el cierre: hacerlo invalidaría los recibos nativos que acreditan
exactamente estos bytes sin una causa que lo justifique.

| Entrada | Estado en `HEAD` | Disposición |
| --- | --- | --- |
| Asociación repetición ↔ archivo ↔ logs | **Corregida.** `performance_port.rs::benchmark_archive` publica `archive.run_index` —la última repetición que exportó— y la observación `exit_run_index` —la última que corrió—; ADR-080 §4 y `docs/tools.md` los distinguen en el wire | Cerrada |
| `inconclusive_reasons` sin ordenar | **Parcial.** `IncompatibilityReason` se ordena y deduplica en dominio (`CompareError::incompatible`) y otra vez en el servidor (`report`). `InconclusiveReason` sale de `decide` en orden de evaluación de guardas: determinista, a lo sumo dos entradas (`truncated_measurement` y la guarda que decidió), sin orden lexicográfico ni de enum. El contrato publicado no promete orden | P3 abierto: `crates/domain/src/benchmark_compare.rs::decide`; ordenar o documentar en la siguiente edición del contrato |
| `provenance.run_index` = 1 | **Limitación documentada, no corregida.** `performance_port.rs::provenance` fija `1` porque una llamada publica un único dataset que agrupa todas las repeticiones; el dominio exige `1 ≤ run_index ≤ run_count` y su doc-comment describe el campo como «posición de esta ejecución», que ya no describe el v2. La repetición real la identifica `RawSample::run_index` —la captura registra 1, 2 y 3— y `run_count` declara cuántas se pidieron. Un lector no debe leer `provenance.run_index = 1` como «solo la primera repetición» | P3 abierto: retirar o redefinir el campo exige un v3 del dataset o una enmienda a ADR-073 §3; fuera del cierre |
| Comentario «paired» del bootstrap | **Parcial.** El doc-comment de `bootstrap_ratio` conserva la palabra «Paired» y declara en la misma frase que cada lado se remuestrea de forma independiente en dos etapas; el algoritmo es el descrito | P3 abierto: solo el término, en `crates/domain/src/benchmark_compare.rs` |
| `summary` constantes | **Abierta.** `rust.benchmark.compare` publica «Observed difference between two measurements…» también con `failed`/`INCOMPATIBLE_DATASETS` y `blocked`/`EVIDENCE_INCOMPLETE`; `rust.binary.bloat` publica «Exact measured file size…» también con `failed`/`OBSERVED_FAILURE` y los `blocked` de completeness. `outcome`, `error_code` y `error_message` sí describen el resultado real y son lo que los clientes evalúan | P3 abierto: `stdio/benchmark_compare.rs::encode_result` y `stdio/bloat.rs`; texto humano, no dato |
| `bool` frente a `const` | **Abierta.** `MeasuredBinary.analysis_build_symbols_forced` y `BloatAttribution.estimated` siguen como `bool` en `stdio/bloat/schemas.rs` con doc «Always true»; el valor emitido es siempre `true` | P3 abierto: endurecer el JSON Schema es aditivo y se hace con el siguiente cambio de contrato |
| Comentario del decoder | **Abierta.** `application/benchmark_compare.rs::DatasetDecoder` dice que el execution adapter aporta la implementación; la de producto es `JsonDatasetDecoder` en `mcp-server/src/stdio/benchmark_compare.rs`. El adapter solo posee el decoder de archivos Criterion | P3 abierto: comentario |

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
