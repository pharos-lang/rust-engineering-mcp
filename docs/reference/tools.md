# Tools

Este checkout anuncia **36 tools** por `tools/list`: 31 clase `stable` y 5
clase `preview` (las cinco `rust.analyzer.*`, ver
[§Analyzer](#analyzer-preview)). La release publicada `v0.3.0` anuncia 31
(las 18 tools M1/M2, más cuatro de testing/coverage/semver/mutation-testing,
más cinco de seguridad); las cinco `rust.analyzer.*` solo existen en el
checkout de desarrollo, nunca en una release publicada hasta que se
califiquen como `stable`. `rmcp 3.2.0` gestiona discovery, negociación y
dispatch — ver [`reference/compatibility.md`](compatibility.md) para las
versiones de protocolo negociables. La política completa de evolución de
contrato (aditivo vs. ruptura, calendario de deprecación 0.8.0 → 1.0 → 2.0,
retirada de una revisión MCP, excepción fail-closed) vive en
[`architecture/mcp-and-contracts.md#política-de-evolución-de-contratos-adr-086`](../architecture/mcp-and-contracts.md#política-de-evolución-de-contratos-adr-086).

Descripciones, schemas y annotations exactos de cada tool viven en el
documento de contrato (`rust-engineering-mcp contract --json`); esta página
resume propósito, prerrequisitos y contrato observable, sin reproducir los
JSON Schema completos.

> [!NOTE]
> Las `description` de las tools son texto de esquema congelado — sus hashes
> (`description_sha256`) forman parte del contrato verificado por
> `contract-freeze`. Algunas citan rutas de documentos históricos de
> validación por milestone; esas citas no se editan como parte de esta
> limpieza, son parte del contrato congelado, no un enlace que este
> documento mantenga vivo.

## Convenciones comunes

### Ciclo de vida de `project_ref`

`rust.project.open` es el punto de entrada de casi toda tool (excepciones:
`rust.diagnostics.explain`, `rust.catalog.status`, `rust.crate.search`,
`rust.crate.inspect` y `rust.benchmark.compare`, que no necesitan un
proyecto abierto). Devuelve un `project_ref` (`prj_<32 hex>`) que vive solo
en memoria del proceso: no sobrevive a un reinicio del servidor, expira por
inactividad (`--project-ttl-secs`, default 1800 s) y debe revalidarse antes
de cada uso. Tras un `commit` de mutación, el `project_ref` de entrada queda
invalidado — hay que reabrir el proyecto y usar el nuevo `project_ref` en
toda llamada posterior, incluidos `receipt` y `recovery`.

### Envelope de resultado

Toda tool devuelve el mismo envelope de resultado
(`status`/`summary`/`duration_ms`/`error_code`/`error_message`/`diagnostics`/
`truncation`/`data`/`evidence`); ver
[`reference/data-formats.md`](data-formats.md#envelope-de-resultado-todas-las-tools)
para la forma exacta. `error_code`/`error_message` están siempre presentes
como clave, aunque su valor sea `null`.

### `status`: qué significa cada uno

- **`passed`** — la operación se ejecutó y su evidencia es completa y
  válida según el contrato de esa tool. No implica "sin hallazgos" ni
  "óptimo": para tools de análisis, `passed` certifica que el análisis
  corrió y fue validado, no que el resultado sea deseable.
- **`failed`** — la operación se ejecutó por completo y observó un
  resultado operacional negativo declarado por la propia herramienta (una
  compilación que falla, un test que falla, un hallazgo con evidencia
  usable). `isError` es `false`: esto es un resultado válido de la tool, no
  un error de infraestructura MCP.
- **`blocked`** — la operación se rechazó antes de completarse por una
  condición explícita y nombrada (`error_code`): permiso ausente, conflicto,
  entrada inválida, límite excedido, timeout. `isError` es `true`.
- **`unavailable`** — el prerrequisito de plataforma, runtime o datos
  offline no está presente; la operación nunca llegó a intentarse en
  sustancia.
- **`cancelled`** — el peer canceló la operación; no publica `error_code`.

Un tool-level failure (`failed`) es un resultado válido de la tool, nunca un
error de infraestructura del MCP — regla explícita de `AGENTS.md`.

### Provenance y freshness: `latest_known`

Toda información derivada de un snapshot (catálogo, advisories RustSec,
capturas de proyecto) declara su semántica como `latest_known` — nunca
`latest`, salvo evidencia live explícita. Un `passed` sobre una captura
`Aging`/`Stale` no es una lectura en tiempo real del filesystem ni del
catálogo remoto; es una afirmación sobre la generación capturada, con su
freshness declarada aparte.

### Prerrequisitos de runtime

| Prerrequisito | Qué habilita | Cómo se configura |
| --- | --- | --- |
| `none` | Solo memoria/cómputo del propio servidor | Nada adicional |
| `docker-rust` | Ejecuta Cargo/binarios del proyecto en el guest aprobado | Tupla Docker completa en `serve --stdio` |
| `catalog` | Lee el catálogo local (SQLite/FTS5, opcionalmente LanceDB) | `--catalog-store`/`--catalog-trust` en `serve --stdio` |
| `docker-scanner` | Scanner de sintaxis unsafe aislado (imagen M4) | Tupla Docker + vendor Cargo |
| `analyzer` | Sesión rust-analyzer transitoria (imagen M6) | Tupla Docker con `--rust-image` = imagen M6 |

Ver [`guides/configuration.md`](../guides/configuration.md) para el detalle
de cada flag y [`reference/limits.md`](limits.md#imágenes-de-runtime-aprobadas)
para los digests exactos.

### Congelación de contrato (0.8.0)

`0.8.0` es el freeze de contrato vigente: los 36 elementos (31 `stable` + 5
`preview`) tienen sus schemas, `description` y annotations verificados
byte a byte contra `tests/baselines/contract-freeze-0.8.0.json` mediante
`scripts/contract-freeze.py verify`, invocado por
[`development/testing.md`](../development/testing.md) en cada corrida de
`gate.py core`. Un cambio de clase de estabilidad o del `tool_count` total
siempre hace fallar esa verificación.

## Tools de lectura por área

Cada tool indica: estabilidad; solo lectura o mutante (+ grant requerido);
runtime/datos prerrequisito; entradas/salidas clave; estados/errores
notables. Límites numéricos: [`reference/limits.md`](limits.md).

### Proyecto y toolchain

| Tool | Estabilidad | Tipo | Runtime |
| --- | --- | --- | --- |
| `rust.project.open` | stable | lectura | ninguno |
| `rust.project.inspect` | stable | lectura | docker-rust |
| `rust.toolchain.inspect` | stable | lectura | docker-rust |

- **`rust.project.open`** — abre un workspace por path físico ya autorizado
  vía `--root`. Entrada: `{path}`. Salida: `project_ref`, `workspace_root`,
  `fingerprint`, `validation: "structural"`. Solo valida manifests y un
  grafo acotado de miembros/path-dependencies — nunca ejecuta Cargo. Máximo
  64 referencias vivas por proceso.
- **`rust.project.inspect`** — snapshot de metadata Cargo (miembros,
  packages, targets, features, dependencias declaradas, profiles) capturado
  offline dentro del gateway. Entrada: `{project_ref}`. `blocked/SANDBOX_DENIED`
  durante bootstrap del proceso es transitorio (reintentar con nuevo request
  ID); sin runtime configurado es fallo persistente.
- **`rust.toolchain.inspect`** — inventario de rustc/Cargo/componentes
  instalados **en la imagen aprobada** (nunca del host que sirve MCP).
  Entrada: `{project_ref}`.

### Check, formato, lints y fixes

| Tool | Estabilidad | Tipo | Grant | Runtime |
| --- | --- | --- | --- | --- |
| `rust.check` | stable | lectura (ejecuta código de proyecto) | — | docker-rust |
| `rust.fmt.check` | stable | lectura | — | docker-rust |
| `rust.fmt.apply` | stable | mutante | `--allow-fmt-write` | docker-rust |
| `rust.clippy` | stable | lectura (ejecuta código de proyecto) | — | docker-rust |
| `rust.fix.apply` | stable | mutante (ejecuta código de proyecto) | `--allow-fix-write` | docker-rust |

- **`rust.check`** — `cargo check` frozen/offline sobre selección cerrada
  (`package`/`workspace`, `features`, target único instalado). Puede
  ejecutar `build.rs`/proc macros. `passed` exige `exit 0` + evidencia
  completa; un fallo de compilación es `failed`; evidencia incompleta o
  timeout son `blocked`. Publica logs como Resource (`rust-artifact://`).
- **`rust.fmt.check`** — `cargo fmt --all --check` fijo. Salida incluye
  `affected_files` y un diff de solo visualización (nunca aplicable
  directamente).
- **`rust.fmt.apply`** — ciclo `preview`→`commit`→`receipt` (ver
  [`architecture/mutation.md`](../architecture/mutation.md)). Ejecuta
  rustfmt sobre staging y revalida con `fmt.check` completo antes de
  publicar el candidato. Reemplaza solo `.rs` existentes.
- **`rust.clippy`** — cuatro perfiles cerrados (`default`, `project`,
  `strict`, `pedantic`); `strict` puede convertir warnings en `failed`.
- **`rust.fix.apply`** — mismo ciclo de mutación; comando fijo
  (workspace/todos los targets/features por defecto, frozen/offline);
  requiere `Cargo.lock` existente; revalida con `cargo check` frozen tras
  aplicar.

### Testing y cobertura

| Tool | Estabilidad | Tipo | Runtime |
| --- | --- | --- | --- |
| `rust.test` | stable | lectura (ejecuta código de proyecto) | docker-rust |
| `rust.test.nextest` | stable | lectura (ejecuta código de proyecto) | docker-rust |
| `rust.coverage` | stable | lectura (ejecuta código de proyecto) | docker-rust |
| `rust.mutation.test` | stable | lectura (ejecuta código de proyecto) | docker-rust |
| `rust.miri` | stable | lectura (ejecuta código de proyecto) | docker-rust |

- **`rust.test`** — `cargo test` frozen/JSON con `test-threads=1`.
  `build_succeeded` nullable representa la fase reportada; fallar
  compilación o tests es `failed`.
- **`rust.test.nextest`** — perfil `rust-mcp` fijo, nunca ejecuta doctests.
  Puede negociar MCP Tasks para timeouts largos; sin declaración del peer,
  `auto`/`synchronous` solo califican para `timeout_seconds <= 60`.
  Publica JUnit/stdout/stderr en el store durable M3 cuando hay
  `--state-root` calificado.
- **`rust.coverage`** — una sola fase `cargo llvm-cov --no-report`, deriva
  JSON+LCOV+HTML del mismo profdata. Un scope con denominador cero queda
  ausente de las métricas — nunca 0 % ni 100 %. HTML se publica como
  `ArchiveBundle` USTAR validado. Ver la limitación en
  [`reference/compatibility.md`](compatibility.md#limitaciones-documentadas)
  sobre `fail-under`/doctests, aún sin decidir.
- **`rust.mutation.test`** — ejecuta `cargo mutants` con baseline
  obligatorio; `missed >= 1` es `failed`. `max_mutants` (default/máx. 100)
  se comprueba en una pasada de listado antes de construir nada
  (`MUTANT_LIMIT_EXCEEDED` si excede). Los mutantes se aplican solo en una
  copia privada del sandbox; `/source` siempre read-only.
- **`rust.miri`** — modo de "integridad de clasificación": rechaza grafos
  con proc-macros, build scripts o harnesses custom antes de correr; la
  clasificación (UB observada / no soportado / fallo ordinario /
  indeterminado) nunca se deriva del stdout del programa interpretado, solo
  de la terminación del gateway y de los diagnósticos propios de Miri.

### Dependencias, manifest y diagnóstico de compilador

| Tool | Estabilidad | Tipo | Grant | Runtime |
| --- | --- | --- | --- | --- |
| `rust.dependencies.audit` | stable | lectura | — | catalog |
| `rust.dependency.add` | stable | mutante | `--allow-dependency-add` | docker-rust |
| `rust.dependency.remove` | stable | mutante | `--allow-dependency-remove` | docker-rust |
| `rust.manifest.patch` | stable | mutante | `--allow-manifest-write` | docker-rust |
| `rust.diagnostics.explain` | stable | lectura | — | docker-rust |

- **`rust.dependencies.audit`** — audita `Cargo.lock` (v4 acotado) contra un
  snapshot RustSec firmado por el operador (`--rustsec-snapshot`). Solo
  compara sources crates.io canónicos como tales; freshness `fresh`
  (≤ 24 h) permite un `passed` limpio, `aging`/`stale`/`unknown` no pasan.
- **`rust.dependency.add`** / **`rust.dependency.remove`** — ciclo de
  mutación; requieren resolución offline aprobada (vendor Cargo). `add`
  nunca reemplaza silenciosamente una definición distinta; `remove` elimina
  solo la clave seleccionada, incluida una forma dotted, sin tocar
  contenedores inline/dotted que exigirían reescribir campos vecinos
  (layout no soportado → rechazo).
- **`rust.manifest.patch`** — operaciones semánticas cerradas
  (`lint_set/remove`, `feature_set/remove`, `profile_set/remove`,
  `workspace_dependency_set/remove`) sobre el `Cargo.toml` raíz únicamente;
  nunca acepta punteros TOML/JSON arbitrarios ni tablas patch/replace.
- **`rust.diagnostics.explain`** — sin `project_ref`, deliberadamente:
  ejecuta `rustc --explain <código>` fijo sobre un `SourceBundle` vacío en
  el mismo sandbox aprobado. Un código bien formado pero ausente en esa
  versión de rustc es `unavailable`, nunca una explicación heurística
  fabricada.

### Quality gates compuestos

| Tool | Estabilidad | Tipo | Runtime |
| --- | --- | --- | --- |
| `rust.quality.gate` | stable | lectura (ejecuta código de proyecto) | docker-rust |
| `rust.quality.gate.v2` | stable | lectura (ejecuta código de proyecto) | docker-rust |

- **`rust.quality.gate`** — perfil `fast` (fmt/check/clippy strict) o
  `standard` (+ tests por defecto/audit offline) sobre una única generación
  de fuente capturada, con fila de estado por etapa.
- **`rust.quality.gate.v2`** — `profile=strict` (format/check/clippy/test/
  audit/deny/coverage) o `release` (+ SemVer contra `baseline_project_ref`
  obligatorio); `mutation` es opt-in y exige que su presupuesto derivado más
  300 s quepa en el timeout global. Solo `strict` sin mutation califica para
  sincronía ≤ 60 s; `release` y mutation exigen MCP Tasks.

### Catálogo de crates

| Tool | Estabilidad | Tipo | Runtime |
| --- | --- | --- | --- |
| `rust.catalog.status` | stable | lectura | catalog |
| `rust.crate.search` | stable | lectura | catalog |
| `rust.crate.inspect` | stable | lectura | catalog |

- **`rust.catalog.status`** — input `{}`, sin `project_ref`. Reporta
  disponibilidad/identidad/freshness de catálogo, reserva, modelo, índice
  semántico y RustSec por separado; un `passed` puede contener componentes
  `unavailable`. Carga una generación inmutable por sesión: un import/rebuild
  administrativo mientras `serve` corre es invisible hasta reiniciar.
- **`rust.crate.search`** — `mode` `lexical`/`semantic`/`hybrid` (RRF
  determinista `sum(1/(60+rank))`, nunca mezcla scores crudos). Modelo/índice
  ausente o inválido degrada a `effective_mode: "lexical"` con `fallback`
  explícito, nunca a error.
- **`rust.crate.inspect`** — paginación explícita por parámetros
  (`section`: `overview`/`versions`/`features`/`dependencies`/`advisories`).
  Un cambio de generación entre páginas produce `blocked/SNAPSHOT_MISMATCH`
  antes de leer cualquier hecho de la página nueva.

### Seguridad y supply chain

| Tool | Estabilidad | Tipo | Runtime |
| --- | --- | --- | --- |
| `rust.deny` | stable | lectura | docker-rust |
| `rust.unsafe.scan` | stable | lectura | docker-scanner |
| `rust.supply_chain.inspect` | stable | lectura | docker-rust |

- **`rust.deny`** — `cargo-deny 0.19.7` solo para `licenses`/`bans`/`sources`
  sobre metadata frozen/offline; requiere vendor, policy y snapshot RustSec
  autenticados por el host.
- **`rust.unsafe.scan`** — examina spans de sintaxis `unsafe`/`extern` en
  archivos `.rs` capturados; nunca expande macros, evalúa `cfg` ni inspecciona
  código generado. Cero hallazgos no prueba ausencia de UB (ver
  [`reference/compatibility.md`](compatibility.md#limitaciones-documentadas)).
- **`rust.supply_chain.inspect`** — compone lock/metadata, checksums,
  duplicados, auditoría RustSec, `deny` opcional y estado yanked exacto
  contra una generación autenticada del catálogo. El localizador Source/URL
  del paquete se omite completamente del output (solo clase de fuente + hash
  de bytes). Nunca computa un score de seguridad.

### SemVer

| Tool | Estabilidad | Tipo | Runtime |
| --- | --- | --- | --- |
| `rust.semver.check` | stable | lectura (ejecuta código de proyecto) | docker-rust |

- Compara dos `project_ref` locales vivos (baseline/candidato), cada uno
  con su propia evidencia `SnapshotEvidence` independiente. `cargo-semver-checks
  0.50.0` no ofrece findings machine-readable completos: los campos por
  finding son best-effort y siempre `partial`.

### Rendimiento

| Tool | Estabilidad | Tipo | Runtime |
| --- | --- | --- | --- |
| `rust.benchmark.run` | stable | lectura (ejecuta código de proyecto) | docker-rust |
| `rust.benchmark.compare` | stable | lectura (cómputo puro) | ninguno |
| `rust.profile.flamegraph` | stable | lectura (ejecuta código de proyecto) | docker-rust |
| `rust.binary.bloat` | stable | lectura (ejecuta código de proyecto) | docker-rust |

- **`rust.benchmark.run`** — mide benchmarks ya existentes del proyecto con
  Criterion 0.8.2 (único harness reconocido); nunca genera un benchmark.
  Parámetros del harness fijados por el servidor (warmup, tiempo de
  medición, tamaño de muestra). Muestras crudas nunca en la respuesta —
  solo como artifact `benchmark_dataset` (formato `...v2`, ver
  [`reference/data-formats.md`](data-formats.md)).
- **`rust.benchmark.compare`** — compara dos datasets ya publicados por el
  mismo proyecto; cómputo puro, sin ejecutar procesos. **En este build nunca
  emite dirección** (`regression`/`improvement`/`no_material_change`) — ver
  la limitación documentada en
  [`reference/compatibility.md`](compatibility.md#limitaciones-documentadas).
  Un par incompatible es `failed/INCOMPATIBLE_DATASETS`, un resultado
  observado, no un error de infraestructura.
- **`rust.profile.flamegraph`** — exige la capability de host
  `--allow-profiling user-space-sampling`; sin ella, `blocked/PROFILING_NOT_AUTHORIZED`
  antes de crear cualquier contenedor. Muestreo estrecho y fijo (solo
  espacio de usuario, solo el proceso hijo). Cero muestras es un resultado
  válido y declarado (`no_samples`), no un fallo.
- **`rust.binary.bloat`** — separa `measured` (hechos verificados por el
  propio producto: tamaño exacto, sha256, formato) de `attribution`
  (estimación de `cargo-bloat 0.12.1`). `passed` significa únicamente
  "el análisis corrió y fue validado" — nunca "el binario está optimizado"
  ni "el ranking es exhaustivo" (ver el tope `BLOAT_MAX_ROWS` en
  [`reference/limits.md`](limits.md)). Solo ELF64/AArch64 está calificado;
  WASM no soportado por el analizador, Mach-O/PE no calificados.

### Analyzer (preview)

| Tool | Estabilidad | Tipo | Grant | Runtime |
| --- | --- | --- | --- | --- |
| `rust.analyzer.symbols` | preview | lectura | — | analyzer |
| `rust.analyzer.references` | preview | lectura | — | analyzer |
| `rust.analyzer.diagnostics` | preview | lectura | — | analyzer |
| `rust.analyzer.actions` | preview | lectura | — | analyzer |
| `rust.analyzer.action.apply` | preview | mutante | `--allow-analyzer-action-write` | analyzer |

Las cinco requieren la imagen guest M6 exacta (rust-analyzer 1.98.1); una
sesión transitoria por consulta, sin pool. `rust-analyzer.toml`/
`.rust-analyzer.toml` en la captura se rechazan antes de arrancar. Hover,
go-to-definition y rename no se ofrecen (deferred, no un campo oculto).
Durante bootstrap del proceso, `blocked/SANDBOX_DENIED` es transitorio
(reintentar); tras bootstrap, `unavailable/SANDBOX_DENIED` significa runtime
no configurado o política del host.

- **`rust.analyzer.symbols`** — símbolos de documento o de workspace
  (`scope: document|workspace`). Nunca `failed`: solo lectura, así que
  `status ∈ passed|blocked|unavailable|cancelled`.
- **`rust.analyzer.references`** — dos peticiones LSP
  (`includeDeclaration: true/false`) para marcar `is_declaration` sin que
  rust-analyzer lo declare nativamente. `position` es 1-based en Unicode
  scalars; fuera de rango es `blocked/POSITION_OUT_OF_RANGE`.
- **`rust.analyzer.diagnostics`** — pull-only (`textDocument/diagnostic`),
  nunca `publishDiagnostics`. Bajo la configuración mínima actual (deuda
  trazada, ver [`reference/compatibility.md`](compatibility.md#limitaciones-documentadas)),
  solo expone diagnósticos de sintaxis — no de tipos/borrow-checker, que
  siguen siendo dominio de `rust.check`.
- **`rust.analyzer.actions`** — lista code actions de solo lectura sobre un
  rango; cada `WorkspaceEdit` se valida estructuralmente pero nunca se
  aplica. `expected_project_fingerprint` es obligatorio (no opcional como en
  las otras tres de lectura).
- **`rust.analyzer.action.apply`** — reutiliza el writer M2 verbatim (mismo
  ciclo `preview`/`commit`/`receipt`, mismo journal, mismo `MutationPlans`).
  **No verificado por compilación**: la validación es solo estructural;
  ejecuta `rust.check` después de cualquier commit. Publica
  `guarantees_not_provided` explícito
  (`compile_verification`, `os_exclusion_of_external_writers`,
  `multi_file_atomicity`, `malicious_host_protection`,
  `demonstrated_power_loss_survival`).

## Resources

Dos plantillas dinámicas, siempre anunciadas en
`resources/templates/list`, nunca en `resources/list` (que devuelve `[]`
por diseño): `rust-artifact://{project_ref}/{artifact_id}` (store M1, ver
[`reference/data-formats.md`](data-formats.md#store-de-artifacts-m1-rust-artifact-en-memoria))
y `rust-quality-artifact://{project_ref}/{quality_job_id_or_artifact_id}{?offset,length}`
(store M3+, ver [`reference/limits.md`](limits.md#store-de-artifacts-de-quality-jobs-m3-rust-mcp-quality-artifacts-v1)).
Cada lectura revalida `project_ref` vivo y propiedad del artifact; ninguna
renueva el TTL del artifact leído.

## Decisiones relacionadas

Ver [`architecture/decisions.md`](../architecture/decisions.md) para el
mapa de ADRs que fijan cada contrato de tool individual.
