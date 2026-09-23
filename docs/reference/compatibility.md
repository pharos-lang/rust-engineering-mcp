# Compatibilidad

## Versión y contrato

El checkout actual es `0.9.0-rc.1` (release candidate de desarrollo, sin tag
ni publicación — ver
[`operations/release-verification.md`](../operations/release-verification.md)).
Las releases publicadas son `v0.1.0` (13 tools) y `v0.3.0` (31 tools); el
checkout actual anuncia **36 tools**: 31 clase `stable` y 5 clase `preview`
(`rust.analyzer.symbols`, `rust.analyzer.references`,
`rust.analyzer.diagnostics`, `rust.analyzer.actions`,
`rust.analyzer.action.apply`). `0.8.0` es el freeze de contrato vigente para toda la serie 0.8/0.9.

## Congelación de contrato 0.8.0 y verificación

Desde `0.8.0`, ningún elemento `stable` cambia de nombre, campos requeridos
ni semántica en un patch; una deprecación se anuncia desde `0.8.0` con
reemplazo y migración probada, y permanece funcional durante toda esa serie
— ver la política de estabilidad más abajo. El subcomando `contract [--json]`
y su documento en disco (`document_kind: rust_engineering_capabilities`,
`format_version: 1`) son ellos mismos contrato `stable`: publican
`protocol{primary_version, negotiable_versions, sdk}`,
`tools{name → stability, annotations, input_schema_sha256,
output_schema_sha256, description_sha256, executes_project_code,
requires_runtime}`, `resources[]` y `tool_count`.

La verificación de que el contrato vivo coincide con el freeze tiene tres
eslabones, cada uno comprobado por su propio test:

1. Los tests de protocolo exigen igualdad exacta entre el servidor vivo (por
   el wire MCP) y los snapshots de contrato
   (`crates/mcp-server/tests/snapshots/*-tool.json`).
2. `tests/cli.rs` exige igualdad entre `contract --json` y esos mismos
   snapshots.
3. La etapa `contract-freeze` de `scripts/gate.py core` ejecuta
   `scripts/contract-freeze.py verify`, que exige igualdad entre los
   snapshots y el manifiesto de freeze
   `tests/baselines/contract-freeze-0.8.0.json` — ver
   [`development/testing.md`](../development/testing.md#scriptsgatepy-corefull).

Un cambio de clase de estabilidad o del `tool_count` total siempre rompe
esta cadena, con o sin `--strict`; ver
[`reference/tools.md`](tools.md) para el contrato por tool.

## SemVer del servidor

Durante la serie `0.x`, un cambio incompatible de contrato exige release
minor + changelog + notas de migración; nombres de tool y campos requeridos
nunca cambian en un patch. No existe un parámetro `version` por tool — una
deprecación se señala con el prefijo `Deprecated since 0.8.0 — use …` en la
`description` de la tool, en esta página y en el changelog.

## Protocolo MCP

SDK `rmcp 3.2.0`. Versión primaria negociada: `2026-07-28`. Versiones
negociables: `2024-11-05`, `2025-03-26`, `2025-06-18`, `2025-11-25`,
`2026-07-28` — el adapter usa la negociación/capacidades del propio SDK, sin
hardcodear una versión. `resources/list`, `resources/templates/list` y
`prompts/list` devuelven `ttlMs: 0`/`cacheScope: "private"` en toda revisión
negociada. `resources/templates/list` anuncia dos plantillas
(`rust-artifact://{project_ref}/{artifact_id}` y
`rust-quality-artifact://{project_ref}/{quality_job_id_or_artifact_id}{?offset,length}`);
`resources/list` y `prompts/list` devuelven siempre `[]` por diseño — no hay
objetos estáticos que listar.

## Plataformas

| Plataforma | Estado |
| --- | --- |
| macOS ARM64 (`aarch64-apple-darwin`) + APFS | **Único host positivo qualified**, para 0.1.0, 1.0 y todo lo intermedio. Adapter de filesystem no-follow/BENEATH; ejecución de código de proyecto siempre delegada al gateway Docker/Linux ARM64 |
| Guest Docker Linux ARM64 (por digest exacto) | Único motor de ejecución de código de proyecto; sin runtime `container` de Apple, sin ejecución nativa |
| Linux x86_64 | Target de CI de portabilidad/fuente y fail-closed únicamente — **no** es target de capability. Sin adapter de filesystem propio, sin sandbox nativo |
| macOS x86_64 | Ni se anuncia ni se testea |
| Linux ARM64 (nativo, fuera del guest) | Ni se anuncia ni se testea |
| Windows x86_64 | CI retirada explícitamente (regresión de stdio pre-`initialize`); restaurarla es deuda de portabilidad, nunca un criterio de release. Sin adapter de filesystem, sin sandbox |

Este alcance de host para 1.0 confirma, sin sustituir, el mismo boundary que
fijó la release `0.1.0`: exactamente una familia de host positiva. Una
matriz más amplia (Linux x86_64 nativo con adapter `openat2`/`RESOLVE_*`
propio, o una tercera familia) fue considerada y rechazada por escala de
trabajo sin cronograma firme — no es una limitación pendiente de corrección
inminente, es una decisión de alcance vigente.

## Imágenes de runtime aprobadas

Cada imagen se admite por digest SHA-256 exacto; ver
[`reference/limits.md`](limits.md#imágenes-de-runtime-aprobadas) para la
tabla completa. `serve` nunca acepta un digest distinto de los aprobados ni
degrada silenciosamente a una imagen "menor": una tool cuya imagen requerida
no está configurada responde `unavailable`, nunca ejecuta sobre una imagen
equivocada. Provisión y actualización de estas imágenes:
[`operations/runtime-provisioning.md`](../operations/runtime-provisioning.md).

## Clientes qualified

Resumen de evidencia real de calificación por cliente (MCP Inspector,
Codex, Claude Code, Gemini CLI, Cursor, VS Code) y su configuración exacta:
[`guides/clients.md`](../guides/clients.md). Un snippet de configuración
documentado no equivale por sí solo a calificación: solo las filas con
evidencia de invocación real (Inspector, Codex, Claude Code) cuentan como
consumidor real para la política de estabilidad de abajo.

## Clases de estabilidad y política de deprecación

Cuatro clases por elemento del contrato: `stable`, `preview`,
`experimental` (sin uso hoy) e `internal` (no anunciado en `tools/list`).
"Consumidor real" significa una invocación registrada de un cliente stock
o un test end-to-end genuino por el wire MCP — **un test unitario, de
contrato o de snapshot no cuenta**; un elemento sin esa evidencia no puede
ser `stable`, y todo elemento `stable` debe ejercitarse por un cliente real
antes de un release candidate o se degrada a `preview`. Bajo esta política,
las cinco tools `rust.analyzer.*` son `preview`; las 31 restantes son
`stable`.

Esta sección cubre solo las clases de estabilidad y la regla base de
ruptura 0.x. El resto de la política de ADR-086 — aditivo vs. ruptura real,
el calendario de deprecación 0.8.0 → 1.0 → 2.0, la retirada de una revisión
MCP, la excepción fail-closed de seguridad y la exigencia de snapshots — vive
en
[`architecture/mcp-and-contracts.md#política-de-evolución-de-contratos-adr-086`](../architecture/mcp-and-contracts.md#política-de-evolución-de-contratos-adr-086).

## Documentación de estabilidad

- [`reference/tools.md`](tools.md) — estabilidad, prerrequisitos y
  contrato por tool.
- [`reference/cli.md`](cli.md) — superficie de subcomandos.
- [`reference/limits.md`](limits.md) — límites numéricos.
- [`reference/data-formats.md`](data-formats.md) — formatos en disco y su
  versionado.

## Limitaciones documentadas

Estas limitaciones son vigentes en este checkout — no son errores a
corregir en el corto plazo, son decisiones de alcance o carencias de
evidencia que un usuario debe conocer antes de depender de la
funcionalidad correspondiente.

**Medición, no veredicto — `rust.benchmark.compare`.** En el runtime
actual, `rust.benchmark.compare` **nunca** emite un veredicto direccional
(`regression`/`improvement`/`no_material_change`): un guard de código
(`METHOD_QUALIFIED_FOR_DIRECTION = false`,
`crates/domain/src/benchmark_compare.rs:184`) fuerza `inconclusive` para
toda comparación, porque el drift de medición entre corridas del host
excede lo que el método actual puede tolerar al umbral configurado. La
tool sí mide y publica el efecto observado (`effect_ratio` y su intervalo);
lo que no hace es clasificarlo como regresión o mejora. Ver
[`reference/limits.md`](limits.md#rustbenchmarkrun--rustbenchmarkcompare).

**Deuda de contrato — tools `rust.analyzer.*` en preview.** Las cinco
tools del analyzer permanecen en clase `preview` mientras no completen la
matriz de clientes reales que exige la política de estabilidad; no ofrecen
hover, go-to-definition ni rename (deliberadamente diferido, nunca un campo
oculto); sus diagnósticos son solo de sintaxis bajo la configuración mínima
actual (los diagnósticos experimentales de tipos/borrow-checker quedan como
deuda trazada, no habilitados, porque inundan de falsos positivos sobre
macros de la librería estándar).

**Captura de vendor calificada solo parcialmente — `rust.benchmark.run`.**
El contrato de captura de vendor permite hasta 512 MiB, pero solo ~156 MiB
(el cierre real de Criterion 0.8.2) está empíricamente qualified; capturas
mayores no tienen evidencia de funcionar, aunque el contrato las admita.

**Garantías no provistas por la mutación — `local_coordinated`.** El modo
de escritura no ofrece CAS, ni exclusión a nivel de sistema operativo frente
a otros programas (editor, Git, otro proceso del mismo usuario), ni
atomicidad multiarchivo visible para otros lectores, ni protección frente a
un host malicioso. La supervivencia a una pérdida de energía real
**no está demostrada** — solo se probó inyección de `ENOSPC`, no un corte
de energía real. Un journal corrupto o de formato desconocido bloquea el
store compartido completo hasta remediarlo. Detalle:
[`architecture/mutation.md`](../architecture/mutation.md) y
[`operations/backup-and-recovery.md`](../operations/backup-and-recovery.md).

**Sin certificación de seguridad universal.** Cero hallazgos de
`rust.unsafe.scan` nunca prueba ausencia de undefined behavior o de fallos
de memory safety; un run limpio de `rust.miri` certifica solo los tests y la
configuración seleccionados, nunca ausencia universal de UB; `rust.dependencies.audit`,
`rust.deny` y `rust.supply_chain.inspect` entregan hechos con provenance y
freshness, nunca un score de seguridad ni una certificación legal.

**Sin migración/backup dedicados.** No existe un subcomando de backup ni de
migración de datos; el procedimiento es operativo (parar el servidor, copiar
árboles de archivos, validar con `doctor`). Ver
[`operations/backup-and-recovery.md`](../operations/backup-and-recovery.md).

**Ausencias de la especificación original, vigentes.** Ninguna de las
siguientes existe en este checkout, y no deben asumirse disponibles: metadata
de `cargo-binstall`; modos de red restringida "mirror corporativo" o
"air-gapped firmado" del catálogo más allá del modo online-controlado con
allowlist (no reverificados de forma independiente en este corte); una tool
dedicada a consultar documentación local o remota de crates
(`rust.crate.inspect`/`.search` cubren metadata, no contenido de
documentación); un transporte remoto (HTTP, autenticación, rate limits) —
formalmente diferido, sin trabajo iniciado; contadores de métricas estándar
por tool (recuento de ejecuciones, duración, cache-hit); un enum de perfil de
rendimiento genérico (`dev`/`ci`/`release`/`bench`/`size`) —
`rust.binary.bloat` cubre el caso de uso de tamaño de forma concreta en su
lugar; `cargo-semver-checks` en el pipeline de release (la estabilidad de
contrato se rastrea manualmente vía diff de freeze, no con esa herramienta).

### No son limitaciones: sustituidas deliberadamente

Tres elementos de la especificación original **no** son trabajo pendiente,
sino decisiones de arquitectura ya tomadas en su lugar: distribución
secundaria vía `cargo install`/crates.io (`publish = false` deliberado; el
canal es GitHub Releases), un formato de configuración de host en TOML
(sustituido por flags explícitas de `serve --stdio`, ver
[`guides/configuration.md`](../guides/configuration.md)) y un layout local
fijo tipo `~/.rust-engineering-mcp/{catalog,vectors,...}` (sustituido por
rutas explícitas que el operador elige por flag — `--state-root`,
`--catalog-store`, etc., ver
[`operations/catalog-maintenance.md`](../operations/catalog-maintenance.md)).
No los trates como carencias a corregir.

## Decisiones relacionadas

Ver [`architecture/decisions.md`](../architecture/decisions.md) para el
mapa de ADRs que fijan el alcance de host, la política de deprecación/freeze
y el modelo de confianza de mutación.
