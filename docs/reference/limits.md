# Límites

Todos los límites de esta página son constantes del código (`domain`/`application`)
o flags de host acotadas; ninguno es configurable por el peer MCP. Cuando una
tool los excede, el resultado es un estado declarado (`blocked`, `unavailable`
o un campo de recorte/omisión explícito), nunca un error de infraestructura ni
un dato inventado. Ver [`reference/tools.md`](tools.md) para qué significa cada
estado por tool y [`reference/data-formats.md`](data-formats.md) para los
formatos que estos límites acotan.

## Sesión y proyectos

| Límite | Valor | Fuente |
| --- | --- | --- |
| `--root` (raíces autorizadas) | ≤ 16 | `crates/mcp-server/src/host_config.rs:39` |
| Grants de escritura por flag (`--allow-*-write`) | ≤ 16 cada uno | `crates/mcp-server/src/host_config.rs:65` |
| `--project-ttl-secs` | `1..=86400` (default `1800`) | `crates/mcp-server/src/host_config.rs:22,74` |
| Deadline de frame de salida rmcp | 1 MiB | comportamiento de la librería `rmcp`, no una constante de dominio propia; pendiente de verificar — [`docs/compatibility.md` en 51fa602e](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/compatibility.md) |

## Captura de fuente (`SourceBundle`)

Aplican a `rust.project.open`/`inspect` y a cualquier tool que capture el
workspace o el candidato de una mutación:

| Límite | Valor | Fuente |
| --- | --- | --- |
| Entradas máximas | 4096 | `crates/domain/src/source.rs:4` |
| Profundidad máxima | 32 | `crates/domain/src/source.rs:5` |
| Bytes por path | 100 | `crates/domain/src/source.rs:6` |
| Bytes por archivo | 1 MiB | `crates/domain/src/source.rs:7` |
| Bytes totales | 16 MiB | `crates/domain/src/source.rs:8` |

## Vendor Cargo offline y captura de benchmarking

| Límite | Valor | Fuente |
| --- | --- | --- |
| Vendor Cargo estándar (`--cargo-vendor-dir`) | mismas cuotas que `SourceBundle` (16 MiB / 4096 entradas / 1 MiB por archivo) | `crates/domain/src/source.rs:4-8` (reutilizadas por `cargo_vendor.rs`) |
| Captura de vendor para Criterion (`--vendor-capture`) | 512 MiB totales, 32 768 entradas, 8 MiB por archivo, 200 bytes por path, profundidad 16 | `crates/domain/src/vendor_capture.rs:45,49,54,56,58` |
| Buffer de lectura de captura | 64 KiB | `crates/domain/src/vendor_capture.rs:41` |

La captura de vendor amplía las cuotas del vendor estándar porque el cierre de
Criterion 0.8.2 (6014 archivos / ~156 MiB) no cabe en las cuotas de 16 MiB; no
son intercambiables entre tools (ver [`reference/data-formats.md`](data-formats.md)).

## Mutación (`rust.manifest.patch`, `rust.fmt.apply`, `rust.fix.apply`, `rust.dependency.add/remove`, `rust.analyzer.action.apply`)

| Límite | Valor | Fuente |
| --- | --- | --- |
| Planes pendientes por store | ≤ 4 | `crates/application/src/mutation.rs:508` |
| Bytes agregados de planes pendientes | 64 MiB | `crates/application/src/mutation.rs:508` |
| TTL de un plan (`preview`) | 600 s | `crates/application/src/mutation.rs:449` |
| Diff en la respuesta | 128 KiB (si no cabe, se rechaza antes de retener el plan) | `crates/mcp-server/src/stdio/mutation.rs:1540` |
| Manifest raíz (`rust.manifest.patch`) | 256 KiB | `crates/project-adapter/src/filesystem/macos/mutation.rs:37` (`MAX_MANIFEST_BYTES`) |
| Journals por store | ≤ 128 | `crates/project-adapter/src/filesystem/macos/mutation.rs:23` (`MAX_JOURNALS`) |
| Bytes totales del store de journals | 256 MiB | `crates/project-adapter/src/filesystem/macos/mutation.rs:24` (`MAX_STORE_BYTES`) |
| Bytes por journal | 48 MiB | `crates/project-adapter/src/filesystem/macos/mutation.rs:25` (`MAX_JOURNAL_BYTES`) |
| xattr por nombre / valor / total | 64 KiB / 1 MiB / 4 MiB | `crates/project-adapter/src/filesystem/macos/mutation.rs:34-36` |
| Timeout del worker de mutación | 240 s | `crates/mcp-server/src/stdio/mutation.rs:49` (`DEADLINE`) |

Detalle del ciclo `preview`/`commit`/`receipt` y `local_coordinated`:
[`architecture/mutation.md`](../architecture/mutation.md).

## `rust.analyzer.*` (preview)

| Límite | Valor | Fuente |
| --- | --- | --- |
| Resultados visibles (símbolos/referencias/diagnósticos) | 512 | `crates/domain/src/analyzer.rs:53` |
| Profundidad de símbolo | 32 | `crates/domain/src/analyzer.rs:61` |
| Code actions listadas | 32 | `crates/domain/src/analyzer.rs:62` |
| Edits por acción aplicada | 128 | `crates/domain/src/analyzer.rs:63` |
| Frame LSP | 1 MiB | `crates/domain/src/analyzer.rs:64` |
| Mensajes por job | 4096 | `crates/domain/src/analyzer.rs:65` |
| stdout del analizador | 16 MiB | `crates/domain/src/analyzer.rs:66` |
| stderr del analizador | 1 MiB | `crates/domain/src/analyzer.rs:67` |
| Timeout de inicialización | 60 s | `crates/domain/src/analyzer.rs:68` |
| Timeout de query | 30 s | `crates/domain/src/analyzer.rs:69` |
| Timeout total de la llamada | 180 s | `crates/domain/src/analyzer.rs:70` |
| Resultado MCP completo | 512 KiB | `crates/domain/src/analyzer.rs:71` |
| `code` de diagnóstico | 128 caracteres | `crates/domain/src/analyzer.rs:532` |
| `message` de diagnóstico | 4096 caracteres | `crates/domain/src/analyzer.rs:536` |
| Entradas `related` por diagnóstico | 32 | `crates/domain/src/analyzer.rs:540` |
| Caracteres de `query` (scope workspace) | 128 | `crates/domain/src/analyzer.rs:1517` |
| Estados de progreso publicados | 32 | `crates/domain/src/analyzer.rs:1755` |

## `rust.coverage`

| Límite | Valor | Fuente |
| --- | --- | --- |
| Timeout por defecto | 300 s | `crates/domain/src/coverage.rs:8` |
| Timeout máximo | 3600 s | `crates/domain/src/coverage.rs:9` |
| Filas por archivo en el resumen | 128 | `crates/domain/src/coverage.rs:10` |

Un scope con denominador cero (sin líneas/regiones/funciones instrumentadas)
queda ausente de las métricas — nunca se reporta como 0 % ni 100 %.

## `rust.test.nextest`

| Límite | Valor | Fuente |
| --- | --- | --- |
| Timeout máximo | 3600 s | `crates/domain/src/nextest.rs:12` / `crates/application/src/nextest.rs:16` |
| Longitud de nombre de test | 512 caracteres | `crates/domain/src/nextest.rs:168` |

## `rust.mutation.test`

| Límite | Valor | Fuente |
| --- | --- | --- |
| `max_mutants` | `1..=100` (default 100) | `crates/domain/src/mutation_test.rs:21-22` |
| `mutant_timeout_seconds` | `1..=60` (default 60) | `crates/domain/src/mutation_test.rs:25-26` |
| Timeout de build derivado | `60..=300` s | `crates/domain/src/mutation_test.rs:29-30` |
| Timeout global del job | `3600` s máximo | `crates/application/src/mutation_test.rs:32` |
| Filas visibles en la respuesta | 128 | `crates/domain/src/mutation_test.rs:32` (`MUTATION_MAX_ROWS`) |
| Longitud de nombre/versión en filas | 256 / 64 caracteres | `crates/domain/src/mutation_test.rs:34,36` |

## `rust.semver.check`

Timeout máximo 3600 s (`crates/application/src/semver_check.rs:20`); hasta 512
findings en la respuesta (`SEMVER_MAX_FINDINGS`,
`crates/application/src/semver_check.rs:21`).

## `rust.binary.bloat`

| Límite | Valor | Fuente |
| --- | --- | --- |
| Filas por vista (función/crate) | 256 | `crates/domain/src/bloat.rs:12` (`BLOAT_MAX_ROWS`) |

El tope de filas es cobertura del ranking, declarada aparte, y nunca decide
`status` por sí solo (ver [`reference/compatibility.md`](compatibility.md)).

## `rust.benchmark.run` / `rust.benchmark.compare`

| Límite | Valor | Fuente |
| --- | --- | --- |
| Texto de identidad (bench/package/feature) | 512 caracteres | `crates/domain/src/benchmark.rs:50` |
| Muestras por benchmark | 100 000 | `crates/domain/src/benchmark.rs:54` |
| Repeticiones (`run_count`) | ≤ 16 estructuralmente; contrato de tool acota a `1..=3` | `crates/domain/src/benchmark.rs:58`; contrato en `crates/mcp-server/src/stdio/benchmark.rs:109` (`#[schemars(range(min = 1, max = 3))]`) |
| Árbol `criterion_archive` | 32 MiB | `crates/domain/src/benchmark_run.rs:80` |
| Log de harness por stream/repetición | 256 KiB | `crates/domain/src/benchmark_run.rs:91` |
| Remuestreos de bootstrap | 10 000 | `crates/domain/src/benchmark_compare.rs:62` |
| Ejecuciones mínimas por lado para reclamar dirección | 3 | `crates/domain/src/benchmark_compare.rs:151` |
| Muestras mínimas por lado | 10 | `crates/domain/src/benchmark_compare.rs:194` |
| Tamaño de familia comparable (bootstrap) | ≤ 25 | `crates/domain/src/benchmark_compare.rs:87` |
| Umbral material | 5 % | `crates/domain/src/benchmark_compare.rs:189` |
| Nivel de confianza nominal | 0,95 | `crates/domain/src/benchmark_compare.rs:122` |
| Timeout `benchmark.compare` | ≤ 30 s | `crates/domain/src/benchmark_compare.rs` (contrato de tool) |

`METHOD_QUALIFIED_FOR_DIRECTION = false` (`crates/domain/src/benchmark_compare.rs:184`)
significa que, en este build, `rust.benchmark.compare` nunca emite
`regression`/`improvement`/`no_material_change` — solo mide (ver
[`reference/compatibility.md`](compatibility.md)).

## Store de artifacts de quality jobs (M3, `rust-mcp-quality-artifacts-v1`)

Aplica a `rust.coverage`, `rust.semver.check`, `rust.mutation.test`, `rust.deny`,
`rust.unsafe.scan`, `rust.supply_chain.inspect`, `rust.quality.gate.v2`,
`rust.benchmark.run`, `rust.profile.flamegraph`, `rust.binary.bloat` cuando
publican artifacts durables:

| Límite | Valor | Fuente |
| --- | --- | --- |
| Por artifact | 32 MiB | `crates/domain/src/quality_artifact.rs:687` |
| Por job (miembros) | 64 MiB, ≤ 128 miembros | `crates/domain/src/quality_artifact.rs:688-689` |
| Por owner | 128 MiB | `crates/domain/src/quality_artifact.rs:690` |
| Global | 256 MiB | `crates/domain/src/quality_artifact.rs:691` |
| TTL por defecto / máximo | 3600 s / 86 400 s | `crates/domain/src/quality_artifact.rs:692-693` |
| Margen de control | 16 MiB | `crates/domain/src/quality_artifact.rs:694` |
| Margen de recuperación M2 | 49 MiB | `crates/domain/src/quality_artifact.rs:699` |
| Entradas máximas del store | 4096 | `crates/domain/src/quality_artifact.rs:714` |

Cuotas «reject-before-produce»: una publicación que excede la cuota se
rechaza, nunca desaloja evidencia ya publicada.

## Store de artifacts M1 (en memoria, `rust-artifact://`)

| Límite | Valor | Fuente |
| --- | --- | --- |
| Por artifact | 256 KiB | `crates/artifact-adapter/src/lib.rs:27` |
| Entrada de entrada (input) | 1 MiB | `crates/artifact-adapter/src/lib.rs:26` |
| Por owner | 1 MiB, ≤ 64 artifacts | `crates/artifact-adapter/src/lib.rs:29-30` |
| Global | 16 MiB, ≤ 256 artifacts | `crates/artifact-adapter/src/lib.rs:28,30` |
| TTL | 3600 s | `crates/artifact-adapter/src/lib.rs:32` |

Este store es process-local y solo memoria: se pierde al reiniciar el
servidor (ver [`reference/data-formats.md`](data-formats.md)).

## MCP Tasks (M3+)

| Límite | Valor | Fuente |
| --- | --- | --- |
| TTL del registro de task | 7 200 000 ms (2 h) | `crates/domain/src/job.rs:6` |
| Intervalo de poll sugerido | 1000 ms | `crates/domain/src/job.rs:7` |
| Deadline de no-entrega | 30 000 ms | `crates/domain/src/job.rs:8` |
| Resultado MCP completo por tool | 512 KiB | `crates/domain/src/job.rs:9` (`TASK_RESPONSE_MAX_BYTES`) |

Este cap de 512 KiB es el mismo presupuesto de respuesta completa
(texto + `structuredContent`) que aplican todas las tools con salida
potencialmente grande (`rust.crate.search`, `rust.crate.inspect`,
`rust.benchmark.*`, las `rust.analyzer.*`, etc.); cada tool documenta en
[`reference/tools.md`](tools.md) qué recorta primero al alcanzarlo.

## Imágenes de runtime aprobadas

Cada imagen se admite por digest exacto; ningún otro valor arranca el
runtime correspondiente:

| Imagen | Digest | Fuente |
| --- | --- | --- |
| `APPROVED_RUST_IMAGE` (M1/M3) | `sha256:384a1742ecc53cdd3a9c0bf36c6f8b66db73ddd118aeeae6e55654ea998ae36a` | `crates/execution-adapter/src/rust_gateway.rs:6` |
| `APPROVED_M4_IMAGE` | `sha256:25ed3626e710081a571a86a29521eaf2e890e796afd422ba5e409e0ce1891635` | `crates/execution-adapter/src/lib.rs:35` |
| `APPROVED_M5_IMAGE` | `sha256:e0a5ca1661b3e49d0a3d68ee3cc0963453078d08eb7fc43c30538c16b7998aac` | usada en `crates/execution-adapter/src/analyzer_native.rs:1081` |
| `APPROVED_M6_IMAGE` | `sha256:f39a5b33ee7d54243664162eb635f8ec223d512042beb7cd18ecf071046b310c` | `crates/execution-adapter/src/analyzer_gateway.rs:40` |

Detalle de qué tools requiere cada imagen y cómo se aprovisionan:
[`operations/runtime-provisioning.md`](../operations/runtime-provisioning.md).

## Limitaciones conocidas del propio catálogo de límites

- Los límites de esta página son exhaustivos para las constantes de
  `domain`/`application` localizadas al momento de escribir este documento;
  cualquier constante nueva que se añada en un corte posterior debe
  incorporarse aquí en el mismo cambio (ver `AGENTS.md`, documentación viva).
- El único límite sin constante de dominio propia localizada con `grep` es el
  deadline de frame de salida de `rmcp` (comportamiento de la librería, no del
  código de este repositorio); se cita por permalink en vez de inventar un
  file:line.
