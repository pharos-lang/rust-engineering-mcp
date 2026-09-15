# Changelog

## 0.8.0 — freeze de contratos (sin publicar; RC en M8-09)

### Migration notes 0.3.0 → 0.8.0

- **Inventario: 31 → 36 tools.** Se añaden cinco `rust.analyzer.*`
  (`.symbols`, `.references`, `.diagnostics`, `.actions`, `.action.apply`),
  clasificadas `preview` bajo la política de estabilidad de
  [ADR-086](docs/adr/ADR-086-deprecation-and-freeze-policy.md). Las 31 tools
  restantes quedan `stable`, condicionadas a superar la matriz de clientes
  stock M8-04 antes de RC1 (ADR-086 §1); una tool que no la supere se degrada
  a `preview` en vez de retirarse.
- **Nuevo grant de host** `--allow-analyzer-action-write WORKSPACE_ROOT`
  (requerido por `rust.analyzer.action.apply`; sin él la tool es
  `unavailable/SANDBOX_DENIED`). El flag `--rust-image` existe desde M1; lo
  nuevo en 0.8.0 es la **imagen de runtime M6** que admite
  ([ADR-085](docs/adr/ADR-085-m6-runtime-admission.md)), requerida por las
  cinco tools `rust.analyzer.*` (sin ella son `unavailable`).
- **30 contratos `stable` byte-idénticos a `0.3.0`.** El único cambio de
  schema en una tool `stable` es `rust.binary.bloat`: su `inputSchema` y su
  `outputSchema` cambian únicamente en el texto de `description` de
  `$defs/BloatProfile` (la ruta de evidencia citada en ese texto se reescribió
  por la hygiene del repo); no cambia validación, tipos, campos ni
  `annotations` (`docs/validation/M8/02-schema-diff.json`,
  `keys_changed: [inputSchema, outputSchema]`, `annotations_changed: false`).
- **Las trece tools M1 son byte-idénticas a `0.1.0`** (verificado
  `git diff v0.1.0 HEAD` sobre sus snapshots de contrato).
- **Deprecaciones anunciadas en 0.8.0: ninguna.** Por ADR-086 §4, nada que no
  se anuncie deprecado en 0.8.0 puede retirarse en 1.0.
- **Nuevo subcomando `rust-engineering-mcp contract [--json | --human]`**
  (spec §56), clase `stable` desde 0.8.0 junto con su documento en disco
  (`document_kind: rust_engineering_capabilities`, `format_version: 1`); un
  cambio de formato es minor release con migration notes. Solo `--json` (con
  `format_version: 1`) es el contrato `stable`; `--human` es una
  representación informativa del mismo documento y no forma parte del
  contrato. Publica
  `document_kind`, `format_version`, `server_version`,
  `protocol{primary_version, negotiable_versions, sdk}`,
  `tools{name → stability, annotations, input_schema_sha256,
  output_schema_sha256, description_sha256, executes_project_code,
  requires_runtime}`, `resources[]{uri_template, stability}` y `tool_count`.
  Cadena de verificación de tres eslabones: los protocol tests exigen
  igualdad servidor vivo ↔ snapshots; `tests/cli.rs` exige igualdad
  `contract --json` ↔ snapshots; la etapa `contract-freeze` del gate `core`
  exige igualdad snapshots ↔ manifiesto de freeze
  `docs/validation/M8/freeze-0.8.0.json`.
- **Clase `preview` visible en la `description`** de las cinco tools
  `rust.analyzer.*` con el prefijo `Preview (ADR-086): `.
- **Alcance de hosts para 1.0: macOS ARM64 únicamente**
  ([ADR-087](docs/adr/ADR-087-1.0-host-scope.md)); Linux/Windows x86_64 siguen
  siendo CI de portabilidad, sin artifact ni calificación.
- **Política de migración, downgrade y backup/restore**
  ([ADR-088](docs/adr/ADR-088-migration-rollback-policy.md)): ningún formato
  en disco requiere migración de bytes entre `0.3.0` y `0.8.0`; `doctor` gana
  un preflight pasivo de journals de mutación pendientes antes de un
  downgrade; backup/restore queda documentado como procedimiento operativo
  sin CLI nueva (`docs/compatibility.md` §Upgrade, rollback y backup).
- **`resources/templates/list` y `prompts/list` llevan `ttlMs`/`cacheScope`
  (V03 §3, SEP-2549).** Sin override, el SDK (`rmcp` 3.2.0) devolvía estos
  dos listados sin `ttlMs`/`cacheScope`, a diferencia de `tools/list` y
  `resources/list`; el cambio es aditivo y los 36 snapshots `*-tool.json` no
  se tocan. Las cuatro respuestas de listado llevan ahora `ttlMs: 0` /
  `cacheScope: "private"` en toda versión de protocolo soportada, incluidas
  las cuatro versiones legacy (antes solo se probaba una).
- **`doctor.mutation_journals` corrige un falso negativo de
  `downgrade_blocked` (V03 D-1, ADR-088 §3).** `MutationRecordSummary` gana
  `kind` (campo aditivo). `mutation_journals` publica `kinds{kind → pending,
  terminal}` por los seis kinds de operación, y `downgrade_blocked` pasa a
  `pending > 0 ∨ existe un registro con un kind ajeno a los cinco que
  `0.3.0` reconoce` (`manifest_patch`, `format_apply`, `fix_apply`,
  `dependency_add`, `dependency_remove`), listados en
  `downgrade_blocking_kinds`; antes, un journal `analyzer_action_apply` ya
  **committed** reportaba `downgrade_blocked: false` porque solo se contaban
  fases pendientes, pese a que `0.3.0` lo rechaza en cuanto lo lee. La nota
  de downgrade pasa a «recover, complete or prune (`mutation prune`) with
  0.8.0 before installing an older binary»; un lock ocupado por una mutación
  concurrente de `serve` (`Busy`) tiene su propia nota («journal busy»),
  distinta de un store realmente ilegible («unreadable or unknown»).
  `doctor` acepta `--state-root` solo, sin el resto de la tupla Docker,
  únicamente para esta sección (misma lectura que `mutation list
  --state-root`); `serve` sigue exigiendo la tupla completa.
- **La plantilla `rust-quality-artifact` pasa a RFC6570 (V03 S-1, contrato
  antes del freeze).** `…/{quality_job_id_or_artifact_id}?offset={n}&length={n}`
  expandía `offset` y `length` desde la misma variable `{n}`, y no
  describía las URIs de índice (sin query); la forma correcta es
  `…/{quality_job_id_or_artifact_id}{?offset,length}`, publicada tanto por
  `resources/templates/list` (`stdio.rs`) como por `contract --json`
  `resources[]` (`capability_document.rs`), ahora construidas desde una
  única constante compartida (`resources::QUALITY_TEMPLATE_SUFFIX`) y
  verificadas por igualdad exacta entre ambas listas.

- **M6-04/M6-05: `rust.analyzer.actions` y `rust.analyzer.action.apply`**
  (rama `ai/m6-analyzer`). El inventario público pasa de 34 a 36 tools; los 34
  snapshots existentes quedan byte a byte. `rust.analyzer.actions` (solo
  lectura) lista las code actions de un rango con su `action_digest`,
  aplicabilidad (`applicable` o `rejected` con razón cerrada, título y kind) y
  `edits_summary`, aplicando las mismas reglas estructurales que el preview.
  `rust.analyzer.action.apply` aplica una acción listada por el writer M2 único
  (preview/commit/receipt, journal, idempotencia, invalidación del
  `project_ref`), con el nuevo grant de host
  `--allow-analyzer-action-write WORKSPACE_ROOT`; sin él es
  `unavailable/SANDBOX_DENIED`. El preview re-resuelve la acción sobre una
  captura nueva (`ACTION_STALE` si el digest ya no coincide o el source cambió
  antes de commit). Validación solo estructural: **no verificada por
  compilación** (decisión A del owner); `guarantees_not_provided` incluye
  `compile_verification` y la descripción pide revisar cada archivo del diff y
  ejecutar `rust.check` después. `MutationKind::AnalyzerActionApply` publica
  su propia vista de validación (`workspace_edit_structural_only`); las cinco
  tools M2 no cambian de contrato. Calificación nativa pendiente del
  orquestador.

- **M6-02/M6-03: `rust.analyzer.references` y `rust.analyzer.diagnostics`**
  (rama `ai/m6-analyzer`). El inventario público pasa de 32 a 34 tools.
  `rust.analyzer.references` busca referencias a un símbolo en una posición
  (`textDocument/references`); la misma sesión envía la petición dos veces
  — `includeDeclaration: true` y `false` — y marca `is_declaration` en toda
  ubicación presente solo en la primera respuesta, porque rust-analyzer no lo
  hace por sí solo (D25 R5, ADR-084 §2 fase 6 enmendada); cuando el llamador
  pide `include_declaration: false`, las declaraciones se retiran de
  `references` y se cuentan en `omitted_declarations`. Una `position` fuera de
  las líneas o columnas capturadas es `blocked/POSITION_OUT_OF_RANGE` antes de
  abrir sesión. `rust.analyzer.diagnostics` lee diagnósticos nativos de
  rust-analyzer (`textDocument/diagnostic`, pull, reporte `full`), distintos de
  los de `rust.check`; `message` es texto derivado del proyecto acotado a 4096
  caracteres Unicode con `message_truncated` y sustitución de caracteres de
  control, nunca el `message` de `serverStatus` ni `stderr`. Mismas
  anotaciones y mismo runtime M6 admitido que `rust.analyzer.symbols`; los 32
  snapshots existentes quedan sin cambios. Véase
  [ADR-084](docs/adr/ADR-084-rust-analyzer-runtime-and-lsp-lifecycle.md) §2
  (enmienda de la fase 6). M6 sigue en desarrollo local, sin integración
  remota, PR ni release; calificación nativa pendiente del orquestador.

- **M6-01: primera tool de análisis, `rust.analyzer.symbols`** (rama
  `ai/m6-analyzer`). El inventario público pasa de 31 a 32 tools. Lee símbolos
  de documento (`textDocument/documentSymbol`) o de workspace
  (`workspace/symbol`) con el rust-analyzer exacto (1.98.1
  `aarch64-unknown-linux-gnu`) admitido por digest dentro de la imagen guest
  M6, sobre un snapshot `latest_known`/no atómico ya capturado. Solo lectura
  (`readOnlyHint`, `idempotentHint`); nunca ejecuta build scripts, proc macros
  ni `checkOnSave`, y una captura con `rust-analyzer.toml` o
  `.rust-analyzer.toml` se rechaza antes de crear ningún contenedor. Requiere
  el runtime del host `--rust` apuntando a la imagen M6; sin él la tool es
  `unavailable`. Resultado acotado a 512 símbolos visibles y 512 KiB de
  respuesta MCP, con el recorte siempre declarado
  (`completeness.reasons: result_limit`), nunca un JSON truncado. Hover,
  go-to-definition y rename quedan Deferred (no se exponen). Nuevo puerto de
  aplicación `rust_engineering_application::analyzer` y la implementación del
  lado `RustProjectInspector` en `execution-adapter`. Contrato fijado con
  snapshot y wire tests en las cinco versiones MCP soportadas. Véanse
  [ADR-082](docs/adr/ADR-082-m6-runtime-provisioning.md),
  [ADR-083](docs/adr/ADR-083-analyzer-contract-and-actions.md),
  [ADR-084](docs/adr/ADR-084-rust-analyzer-runtime-and-lsp-lifecycle.md) y
  [ADR-085](docs/adr/ADR-085-m6-runtime-admission.md). M6 sigue en desarrollo
  local, sin integración remota, PR ni release.

- **Reordenación del repositorio sin cambios de producto** (rama
  `ai/repo-hygiene`, 2026-09-11). La evidencia de calificación pasa a un
  paquete por milestone (`docs/validation/M<n>/` con `history/inventory.json`),
  las revisiones a `docs/reviews/M<n>/`, los prompts ejecutados a
  `docs/prompts/history/` y la evidencia de 0.1.0 a `docs/release/0.1.0/`
  (incluido `PUBLICATION-SNAPSHOT.json`, antes en la raíz). Todo se movió con
  `git mv` y se verificó byte a byte; los enlaces de los documentos vivos se
  reescribieron y [`docs/validation/path-map.json`](docs/validation/path-map.json)
  resuelve las rutas anteriores que citan los recibos. Se retiraron del árbol
  el estado privado del store de los intentos de clientes (`state-*/`, 645
  archivos) y las capturas crudas de `docs/research/m1-16/measurement`
  (408 archivos) y, con el mismo patrón de inventario, los recibos superados e
  intentos fallidos, los transcripts crudos de clientes y de delegación, las
  salidas crudas detrás de recibos y las copias de entradas de revisión
  (1 437 archivos, 30 MB): el árbol conserva lo válido para la versión. Nuevo
  `scripts/docs-hygiene.py`. Los comentarios de `crates/` y `fixtures/` y el
  snapshot de contrato del tool de bloat citan las rutas nuevas; sin cambios
  funcionales en crates, `Cargo.*`, fixtures, vendor, ADRs aceptados ni
  contratos.

## 0.3.0 — 2026-09-11

- Publicada la release estable `v0.3.0` desde el commit de `main`
  `6ea330debc27a2cf1564fbbc258d4b358f5b0f1a`. El workflow tag-bound `34577174517` reconstruyó, instaló y probó el
  archive core macOS ARM64 (`c6cc45e3ca17444f…`, 221 paquetes, 31 tools),
  verificó sus tres attestations OIDC y creó el draft, promocionado después de
  una descarga, verificación de attestations y smoke independientes desde este
  host.

Primera release desde `0.1.0`. Incluye M2, M3, M4 y M5 calificados localmente;
la sección `0.2.0-dev` de abajo describe los cambios de M2 que nunca se
publicaron por separado y forman parte de esta versión.

### M5 — cuatro tools de rendimiento implementadas y calificadas localmente

- **Captura de vendor offline, un contrato separado de `SourceBundle`**
  ([ADR-078](docs/adr/ADR-078-offline-vendor-capture.md)). El cierre de
  `criterion 0.8.2` rompe cuatro límites de `SourceBundle` a la vez y trece de
  sus rutas no rompen ninguno: rompen la gramática, porque llevan paréntesis.
  Ninguna cuota mueve esas trece, así que el contrato nuevo decide también sobre
  el alfabeto. Se implementa con los límites que la tabla del ADR fija —buffer de
  lectura 64 KiB, 512 MiB totales, 32 768 entradas, 8 MiB por archivo, 200 bytes
  por ruta, profundidad 16—, alfabeto ampliado solo a `()+,=@[]{}~` y el espacio,
  y con cada rechazo (byte de control, byte no ASCII, `\`, `:`, componente
  vacío, `.`, `..`, ruta absoluta) fijado por su propia prueba. Captura y
  verificación son incrementales y no residencian el árbol ni el artifact; la
  identidad es el digest, viaja en la provenance y una captura cuyo digest no es
  el declarado se rechaza; enlaces y entradas no regulares se rechazan en vez de
  saltarse; un árbol que cambia durante la captura la hace fallar; una captura
  cancelada no deja residuo ni artifact a medias. Se aprovisiona con
  `cargo-vendor capture --directory DIR --into DIR` y se declara al servidor con
  `--vendor-capture PATH --vendor-capture-tree-sha256 sha256:<64-hex>`.
  `rust.benchmark.run` la resuelve además del `CargoVendorSnapshot` de siempre,
  que sigue funcionando sin cambios para todos los flujos que ya lo usan;
  `SourceBundle`, `validate_source_path` y las cuotas de ADR-055 quedan
  intactos. La ruta de captura y su ingesta guest deben recalificarse antes de
  que M5-01 pueda recibir evidencia final.

- Implementadas `rust.benchmark.run`, `rust.benchmark.compare`,
  `rust.profile.flamegraph` y `rust.binary.bloat`, en ese orden después de las 27
  definiciones existentes; el inventario público pasa de 27 a 31. Los 27
  snapshots anteriores se conservan byte a byte bajo un test de invariancia y se
  añaden cuatro nuevos. Las cuatro son `read_only`, no escriben el checkout y no
  admiten MCP Tasks: `task` devuelve `TASKS_REQUIRED` como resultado declarado.
  Contratos en [ADR-076](docs/adr/ADR-076-m5-performance-contracts.md).
- `rust.benchmark.run` mide benchmarks Criterion 0.8.2 que el proyecto ya tiene y
  no genera ninguno. Warmup 3 s, tiempo de medición 5 s y `--sample-size 30` los
  fija el servidor como argv cerrado y viajan en la provenance; el proyecto no
  los alcanza. Un harness distinto o una versión no aprobada son resultados
  observados sin dataset. Las muestras crudas no viajan en la respuesta.
- **Los logs del harness se publican como artifacts, uno por repetición y por
  stream.** El schema congelado, `docs/tools.md` y ADR-076 decían que los logs
  «quedan en el artifact de criterion»; no quedaban en ninguna parte —ese payload
  es un export USTAR de `CRITERION_HOME` sin log alguno— y el adapter capturaba
  `stdout`/`stderr` y los descartaba. Un `OBSERVED_FAILURE` dirigía al llamador a
  un archivo que no puede contener el error del compilador, y
  `harness_unrecognized` no publicaba artifact alguno.
  [ADR-080](docs/adr/ADR-080-harness-logs-as-artifacts.md) implementa la
  capacidad en vez de borrar la promesa: `harness_stdout` y `harness_stderr`
  privados, owner-bound, con TTL y sensibilidad `source_derived`, acotados en
  256 KiB por stream y por repetición, con el recorte declarado
  (`completeness: truncated` y `size_bytes` = lo que sobrevivió). No se
  concatenan entre repeticiones y nunca salen por el `stdout` del servidor. Los
  bytes inválidos se sustituyen para que el payload sea UTF-8 válido; esa
  sustitución se declara independientemente del recorte, por stream y
  repetición. La cuota se comprueba al publicar después de ejecutar, por lo que
  no promete admitir el trabajo antes de iniciarlo. La respuesta admite hasta
  ocho artifacts en vez de dos.
- **Se corrige la asociación repetición ↔ archivo ↔ logs.** La regla publicada
  decía que el árbol retenido es «la última repetición que exportó uno, la misma
  cuyo exit y logs reporta la respuesta», y era falsa en un caso alcanzable: el
  archivo se elegía con `rfind` sobre las repeticiones que exportaron algo
  mientras el exit venía de la última sin más, así que con la tercera fallando
  sin exportar el archivo era de la segunda y nada en la respuesta permitía
  detectarlo. Ahora cada artifact lleva su `run_index`, la observación lleva
  `exit_run_index`, cada repetición lleva su fila en `observation.logs`, y los
  tres textos publicados describen lo que el código hace.
- `rust.benchmark.compare` publica el método congelado con cada informe: mediana
  del tiempo por iteración, bootstrap percentil de 10 000 remuestreos con semilla
  fija, confianza 0,95 **nominal** con Bonferroni cuando la familia es mayor que
  uno, umbral
  material del 5 %, outliers contados por vallas de Tukey y nunca eliminados, y
  un minimum detectable ratio que no se iguala al umbral. Cada comparación
  publica también cuántas ejecuciones independientes agrupó cada lado. Cuatro veredictos:
  `regression`, `improvement`, `no_material_change` e `inconclusive`. Un par
  incompatible es `status = failed` con `INCOMPATIBLE_DATASETS` y la lista
  completa de razones, con las dos provenances comparadas para que el llamador
  vea *qué* difería; no es un error de infraestructura. El resultado describe una
  medición y nunca una causa ([ADR-073](docs/adr/ADR-073-benchmark-method-and-dataset.md)).
- **La unidad de remuestreo es la ejecución, no la muestra.** Una revisión
  independiente demostró, sobre las capturas reales del propio proyecto, que un
  bootstrap dentro de una sola ejecución produce `improvement` para código que no
  cambió. El método pasa a `benchmark-comparison.v2` con bootstrap por
  conglomerados; el dataset pasa a `benchmark-dataset.v2` con `run_index` por
  muestra, y un payload v1 ya no deserializa. Se añaden cuatro negativas
  estructurales, todas antes de mirar el intervalo: menos de tres ejecuciones por
  lado (`insufficient_executions`), dispersión degenerada, familia mayor de la
  que 10 000 remuestreos resuelven, y un campo de hardware no observable en los
  dos lados.
- **El umbral de ejecuciones sube de dos a tres por lado y la cobertura entregada
  se declara.** La etapa externa del bootstrap por conglomerados subestima el
  error estándar por `sqrt(k/(k−1))` —1,41× con `k = 2`, 1,22× con `k = 3`— sin
  corrección `t_{k−1}` en los percentiles, y `run_count` admite `1..=3`, así que
  `k = 2` era alcanzable: una re-revisión independiente midió 27 de 1000
  comparaciones de código idéntico emitiendo dirección ahí. El mínimo pasa a las
  tres ejecuciones que el protocolo ya ejecuta por defecto, lo que elimina esa
  fila; la razón se renombra a `insufficient_executions` porque también se emite
  con dos ejecuciones, que no son «una sola». El `confidence_level: 0.95` se
  mantiene y se declara como nominal: el intervalo entregado es **más estrecho**
  —más confiado— que ese nivel, con cobertura medida en 0,84–0,89 bajo un nulo
  gaussiano con tres ejecuciones. La magnitud depende del modelo de deriva; el
  mecanismo no.
- **La dirección sigue descalificada independientemente de la observación de
  governor.** El gateway observa, dentro del guest Linux, los IDs de CPU
  visibles y el `scaling_governor` de cada uno; solo publica un valor si todos
  son observables y uniformes. No infiere el host físico ni macOS, y ausencia,
  exit no cero limpio, heterogeneidad o un conjunto incompleto producen `None`.
  Timeout, cancelación, output limit o truncamiento siguen el lifecycle
  fail-closed y son error operativo, no `None`. `cpu_model` también requiere
  consenso de valores guest válidos. Aun así
  `METHOD_QUALIFIED_FOR_DIRECTION=false` bloquea siempre `regression`,
  `improvement` y `no_material_change`; esa puerta estadística es independiente
  del hardware y requiere su propia recalificación.
- `rust.profile.flamegraph` exige la capability positiva del host
  `--allow-profiling user-space-sampling`; sin ella responde `blocked` con
  `PROFILING_NOT_AUTHORIZED` antes de crear contenedor alguno. El muestreo es solo
  de espacio de usuario sobre el proceso hijo y sus hilos, con un perfil seccomp
  que es el de calidad más una sola syscall (`perf_event_open`), sin
  `--cap-add`, sin contenedor privilegiado, sin `sudo` y sin tocar
  `perf_event_paranoid`. Cero muestras es un resultado válido y declarado
  ([ADR-074](docs/adr/ADR-074-profiling-capability-and-containment.md)).
- `rust.binary.bloat` separa el tamaño exacto que mide el producto (bytes y
  `sha256`) de la atribución estimada de `cargo-bloat`, marcada como estimación en
  el propio DTO. El archivo medido es un build de análisis: el analizador fuerza
  `strip=false` para leer símbolos, así que no es byte a byte el que enviaría un
  proyecto que pide stripping, y el DTO lo declara siempre. Un desacuerdo de
  tamaño se publica como `size_mismatch`, nunca fundido con la medición exacta.
- Añadidos artifact kinds nuevos en el store durable privado —
  `benchmark_dataset`, `criterion_archive`, `collapsed_stacks`, `flamegraph_svg`
  y `bloat_json` —, el mime `image/svg+xml` y sus versiones de payload. Los logs
  del harness reutilizan el `tool_log`/`utf8-log.v1` ya existente, sin variante
  nueva en el store; el DTO de la tool es el que los separa en `harness_stdout` y
  `harness_stderr`. Ninguna
  variante nueva aparece en el schema público de una tool anterior. El dataset usa
  el formato versionado `rust-engineering-mcp.benchmark-dataset.v2`
  (`format_version = 2`) con las muestras crudas —cada una con el `run_index` de
  la ejecución que la produjo— y una provenance completa; un lector que no
  reconozca exactamente ese identificador falla cerrado y nunca migra medidas. Techos: SVG ≤ 8 MiB, bloat ≤ 4 MiB, muestras ≤ 32 MiB y
  resultado MCP completo ≤ 512 KiB.
- Provisionada una imagen guest derivada por digest de la imagen M4, que añade
  exactamente `cargo-bloat 0.12.1` (MIT, con su cierre de veinte paquetes
  verificados contra el lockfile publicado) y `rust-mcp-profile-helper`,
  construido desde `fixtures/profile-helper/`. Ninguno es alcanzable por `PATH`;
  el gateway los invoca por ruta absoluta y la construcción corre con
  `--network=none` ([ADR-075](docs/adr/ADR-075-m5-runtime-provisioning.md)).
  [ADR-077](docs/adr/ADR-077-m5-runtime-admission.md) añade exactamente el digest
  `sha256:e0a5ca1661b3e49d0a3d68ee3cc0963453078d08eb7fc43c30538c16b7998aac` a la
  lista cerrada de admisión, y el puerto de performance exige esa imagen y solo
  esa. Las tres imágenes anteriores conservan su admisión y su alcance.
- **Limitación histórica M5-01, sustituida por ADR-078**: antes de la captura de
  vendor, `rust.benchmark.run` no podía alcanzar su positivo a través del
  contrato de `SourceBundle`. El cierre de Criterion
  0.8.2 son 6 014 archivos y 156 267 469 bytes, con cuatro archivos por encima
  del límite de 1 MiB por archivo, y un `SourceBundle` admite 4 096 entradas,
  16 MiB en total y 1 MiB por archivo. **Los límites no se subieron**: pertenecen
  al contrato de datos offline calificado en M2/M4 y ampliarlos habría debilitado
  una frontera de seguridad sin decisión ni recalificación. Detalle y opciones
  para el owner en [M5-01-blocker.json](docs/validation/M5/01-blocker.json).
  ADR-078 no amplía esos límites: introduce una captura separada cuya ruta
  completa está en recalificación.
- La matriz de clientes M5 usa Inspector 2.5.0 como cliente determinista y
  **Claude Code 2.1.267 (`claude-sonnet-5`) como cliente agentic** en lugar de
  Codex, por decisión del owner del 2026-09-10. El turno runtime dirigido por
  modelo mide dos veces por sí mismo —los artifacts están ligados al `ProjectRef`
  del proceso que los publica—, compara en positivo, obtiene `NOT_A_DATASET`
  con su propio `criterion_archive` y lee esa Resource, cuyo contenido debe
  hashear al artifact publicado. El harness fue revisado por Gemini 3.8 y
  Claude Sonnet 5; el driver Inspector aplica ahora su timeout por llamada.
  [Recibo](docs/validation/M5/clients.json).
- **`lancedb` vuelve a `=0.31.0` / Lance 8** (opción 2a, decisión del owner del
  2026-09-10) conforme a [ADR-027](docs/adr/ADR-027-semantic-offline-foundation.md):
  la 0.38.0 no compilaba con `default-features = false` y Lance 11 exigía un
  spill store en disco incompatible con `memory://` y con el gate semántico. Se
  conservan `fastembed 6.0.3`, `jsonschema 0.55.1` y `tokio-rustls 0.26.5`; el
  lock se regeneró offline desde el lock anterior a la subida. La actualización
  general de paquetería queda como
  [tarea post-M8](docs/roadmap/m8-stabilization.md#tarea-post-m8--actualización-de-paquetería-decisión-del-owner-2026-09-10).
  Todos los recibos M5 se recapturan sobre el nuevo lock.
- Estado: M5 **Done local**. Suite nativa 6/6
  ([gate nativo](docs/validation/M5/native-gate.json)), matriz de clientes
  ([recibo](docs/validation/M5/clients.json)), `core` 23/23
  ([recibo](docs/validation/M5/core-gate.json)) y `full` 38/38
  ([recibo](docs/validation/M5/full-gate.json)) sobre el lock con `lancedb
  0.31.0`. `BenchmarkExit` y `BloatExit` conservan `CALIBRATED = false`; la
  guarda direccional sigue en `false`. Sin integración remota, PR, tag, release
  ni cambio de versión.

### M4 — 27 tools implementadas y calificadas localmente

- Implementados `rust.deny`, `rust.unsafe.scan`, `rust.supply_chain.inspect`,
  `rust.quality.gate.v2` y `rust.miri`, en ese orden después de las 22
  definiciones existentes. Los 23 snapshots anteriores permanecen preservados y
  se añadieron cinco nuevos. El [core](docs/validation/M4/core-gate.json), el
  [full](docs/validation/M4/full-gate.json), el [runtime](docs/validation/M4/runtime.json)
  y los [clientes](docs/validation/M4/clients.json) pasaron localmente. La
  [confirmación final](docs/reviews/M4/m4-final-evidence/review.md) acepta el cierre
  local de M4. La implementación `07814664379628f00857feca13148b507de687b9`
  está en el [PR #15](https://github.com/pharos-lang/rust-engineering-mcp/pull/15);
  no hay nueva release ni tag.
- El core pasó 19 etapas (1220 tests Rust, un doctest y 11 tests del helper); el
  full pasó 33 con inventario fuente idéntico. Tras encontrar un directorio E5
  temporal vacío, la reanudación conservó 27 etapas aprobadas y ejecutó seis
  frescas usando assets existentes reverificados, sin descarga ni cambios de
  código. El [fallo original](docs/validation/M4/history/inventory.json)
  permanece preservado. El runtime final pasó 19/19 sobre `25ed…`, con scanner
  7/7 y Miri 13 clasificaciones más 7 admisiones.
- La [revisión final de código Opus](docs/reviews/M4/m4-final-closure/review.md) no
  encontró P0, P1 ni un P2 nuevo. El P2 anterior de freshness nativa ya tiene
  los casos renovados y quedó cerrado en la confirmación final. M4 está Done local.
- Admitido por identidad el runtime Linux ARM64
  `sha256:25ed3626e710081a571a86a29521eaf2e890e796afd422ba5e409e0ce1891635`
  con cargo-deny 0.19.7, nightly/Miri fijado y scanner sintáctico aislado. La
  admisión del runtime no equivale a publicación de las tools ni amplía la matriz
  de plataformas.
- Los cinco contratos conservan MCP Tasks para sus defaults y jobs largos. Una
  selección de hasta 60 s admite el camino síncrono calificado; para
  `rust.quality.gate.v2` se limita a `strict` sin mutation. Deny, scanner y supply
  chain aceptan 1..120 s con 120 s por defecto; Miri acepta 1..1800 s con 300 s
  por defecto; quality gate v2 acepta 1..3600 s con 300 s por defecto. Un timeout
  síncrono mayor de 60 s es inválido; `release` y mutation requieren Tasks.
- `rust.quality.gate.v2` añade perfiles cerrados `strict` y `release`, conserva
  `rust.quality.gate` sin cambios y permite mutation solo como selección explícita.
  El timeout debe cubrir el presupuesto derivado de mutation más 300 s para las
  demás etapas.
- La policy de seguridad, el vendor Cargo y RustSec siguen siendo inputs locales
  autenticados por el host. El runtime MCP no instala, descarga ni actualiza esos
  datos. Evidencia parcial, datos ausentes, timeout, cancelación o cleanup incierto
  nunca producen un pass.
- `security-runtime inventory [--json]` expone pasivamente la imagen, cargo-deny,
  helper, nightly y sysroot compilados. No inspecciona instalaciones. Supply chain
  usa únicamente la generación local de catálogo configurada por
  `--catalog-store` y `--catalog-trust`; no sincroniza durante `serve`.

### M3 — calidad y seguridad

- Añadidas `rust.test.nextest`, `rust.coverage`, `rust.semver.check` y
  `rust.mutation.test`; las 18 snapshots de tools preexistentes son byte-identical
  a `main` y el snapshot de mutation cambió deliberadamente durante las
  correcciones de seguridad. El gate Docker M3 pasa 62/62 (nextest 19, Tasks 7,
  coverage 8, SemVer 18 y mutation 10) y el gate Rust de seguridad 20/20.
- Añadidos el lifecycle acotado de jobs, el store privado persistente de artifacts
  y la CLI `quality-artifacts recover|prune`, con Resources bajo el esquema
  `rust-quality-artifact://`.
- Provisionada una nueva imagen guest inmutable con plugins versionados y hashes
  fijados; el runtime no instala ni descarga plugins.
- Adoptado el perfil seccomp quality con la delta mínima de `socketpair` requerida
  por Tokio ([ADR-064](docs/adr/ADR-064-quality-job-seccomp-profile.md)) y un
  volumen ejecutable dedicado solo para las fases run/report de coverage
  ([ADR-065](docs/adr/ADR-065-coverage-target-volume.md)).
- Decisiones: lifecycle y Tasks negociadas en el job executor ([ADR-060](docs/adr/ADR-060-bounded-job-execution-and-mcp-tasks.md)); store privado y
  límites de retención ([ADR-061](docs/adr/ADR-061-private-quality-artifact-store.md));
  contabilidad de coverage y baselines SemVer ([ADR-062](docs/adr/ADR-062-coverage-accounting-and-semver-baselines.md)); provisioning autorizado
  ([ADR-063](docs/adr/ADR-063-m3-guest-plugin-provisioning.md)). Tasks está
  implementado, calificado y anunciado tras G4, aunque su uso exige declaración
  mutua de la extensión. El gate full local pasa 25/25 y M3-06 queda calificado;
  el cierre del milestone sigue pendiente de la aceptación formal de ADR-064/065
  y de un re-review independiente.

## 0.2.0-dev — incluido en 0.3.0, nunca publicado por separado

### M2 calificado localmente — mutación segura

- Los planes terminales liberan cuota; los retries exactos se resuelven desde el
  journal con permisos vigentes, incluso tras reinicio (ADR-059).

- Eventos locales M2 por stderr sin source, rutas ni credenciales.
- Admisión del journal con reservas de staging y crecimiento de metadata. La
  corrupción persistente tiene un procedimiento documentado para continuar en
  workspace y state root nuevos, conservando los originales.
- El checkout registra cinco tools opt-in adicionales: `rust.manifest.patch`,
  `rust.fmt.apply`, `rust.fix.apply`, `rust.dependency.add` y
  `rust.dependency.remove`; las trece tools publicadas en `0.1.0` se conservan.
- ADR-050 adopta `local_coordinated`, con preview/commit/receipt, planes ligados a
  la operación, cinco grants independientes, journal durable y recuperación
  conservadora. No promete CAS, exclusión de escritores externos ni atomicidad
  visible multiarchivo.
- Manifest patch incorpora ediciones tipadas set/remove para lints, features,
  profiles y workspace dependencies. Add/remove exige un manifest miembro y datos
  Cargo vendorizados aprobados cuando cambia la resolución.
- La policy `preserve_presence` actualiza un lock existente y no publica el lock
  transitorio usado al validar un proyecto que carecía de él.
- Fmt y fix solo reemplazan archivos Rust existentes. Fix usa un perfil aislado
  dedicado con `network=none` y TCP loopback interno para la coordinación de Cargo;
  el candidato se comprueba después de aplicar fixes.

La calificación conjunta M2 está completada: [full y clientes](docs/validation/M2/07.md).
No se ha publicado otra release.

## 0.1.0 — 2026-09-05

- Publicada la release estable `v0.1.0` desde el commit público
  `452acdbf3a634d2cc0b9d153db09718237625b9d`. El workflow tag-bound
  `33948798048` reconstruyó, instaló y probó el archive core macOS ARM64,
  verificó sus tres attestations OIDC y creó el draft promocionado después de una
  descarga y smoke independientes.
- SonarCloud queda verde sobre `main`: cobertura total 71,4 %, 0 issues nuevos
  abiertos y, en el cambio de cierre, cobertura de código nuevo 85,1 % con ratings
  A de reliability, security y maintainability.

- ADR-048 fija la frontera candidata de 0.1.0: un único archive core para
  `aarch64-apple-darwin`, sin modelo, ORT, LanceDB, catálogo, trust, fixtures,
  Docker ni toolchain. El cierre M1 sigue siendo compuesto y exige además un full
  gate source-bound del perfil `local` en macOS26 ARM64/APFS con el gateway Docker
  Linux ARM64. Linux y Windows conservan únicamente CI portable/fail-closed.
- IUMotion Labs no publicará un catálogo oficial en 0.1.0. La fixture y su clave
  pública continúan siendo material de prueba; esta release no necesita ni crea
  una clave Ed25519 de producción.

- SonarCloud ahora importa cobertura real: LCOV de los tests Rust portables y
  Cobertura XML del control de arquitectura Python. El workflow rechaza reportes
  ausentes o vacíos, declara las versiones Python compatibles, evita clasificar el
  schema SQLite como PL/SQL y documenta las pruebas especializadas que quedan fuera
  de esta métrica.

- README reorganizado como guía pública de instalación, configuración y uso. La
  nueva guía de clientes documenta Codex, Claude Code, Gemini CLI, Cursor, VS Code
  y MCP Inspector, distinguiendo configuración disponible de compatibilidad
  calificada.

- Original project code is now dual-licensed under `MIT OR Apache-2.0`, copyright
  IUMotion Labs. The public source channel is
  `pharos-lang/rust-engineering-mcp`. Pinned GitHub Actions provide portable CI and
  a manual, OIDC-attested core-artifact workflow. The macOS ARM64 binary is now
  published through GitHub Releases; crates.io remains disabled.

- M1-17 qualification is complete. MCP Inspector 2.5.0 repeated discovery, positive and fail-closed
  paths on the final core binary. Stock Codex 0.153.0 with `gpt-5.6-sol` completed
  the model-directed E0502-to-green repair and missing-runtime phases under a
  schema-v4 closed controller; 39 controller tests and independent Opus 5 reviews
  report no open P0/P1. Protected PRs #8/#9, final public CI, tag, attestations,
  downloaded-asset smoke and GitHub Release are recorded in the closure receipt.

- M1-16: completed the frozen 24-run paired utility pilot and hidden oracles.
  Both arms passed all 12 first/final candidates; there was no discordant pair or
  observed success advantage. The saturated endpoint has zero discriminating power
  and is not equivalence evidence. The MCP arm used more interactions, elapsed time
  and tokens; no causal, population or product-value claim follows.

- M1-16 retrieval benchmark: one bounded native run over 8 queries and a closed
  15-crate projection observed Hit@5 0.125 lexical versus 1.0 semantic/hybrid,
  warm medians 0.476 versus 4.040/4.041 ms and sampled peak RSS 1,641,632 KiB.
  This separate descriptive benchmark does not establish general IR superiority,
  multilingual coverage, statistical significance, agent utility or causality.

- Prerrequisito M1-01: worker compartido sin cola, cancelación y drenaje al cierre;
  admisión de mensajes SDK y envíos acotada, deadlines de frames/escrituras y cap
  de salida. Retención conservadora de cancelaciones en rmcp3.2.0 (ADR-030).
  Se conserva el único contrato operativo rust.project.open.

- M0-08: SQLite bundled/FTS5, schema v1 y migraciones atómicas, snapshots
  verificados en memoria y consultas internas con provenance/freshness.

- M0-07: frontera de contratos tipada y reusable, validación dual schema/Serde,
  mapping de estados MCP y pruebas de errores sin reflexión; schema público intacto.

- M0-06: CLI capabilities con calibración activa, controles positivos, evidencia
  de kernel y tiers vinculados a configuración; scope exclusivo de probes confiables.
- M0-05: gateway Docker/Linux para probes cerrados, entorno reconstruido,
  presupuestos de salida/wall-time, cancelación, cleanup y fingerprint efectivo.
- M0-04: `rust.project.open`, roots explícitas del host, registro opaco con TTL y
  revalidación, manifests estructurales acotados y fingerprint de identidad.
  I/O protegido macOS 26+/APFS, fail-closed en otros adapters, schemas Rust y
  respuestas estructuradas/texto equivalentes; sin ejecutar Cargo. ADR-024.

- M0-03: MCP stdio con rmcp 3.2.0; discovery 2026-07-28 y cuatro versiones legacy,
  tools/list vacío, límites de entrada, cierre ante errores de I/O y logs solo stderr.
- M0-01: workspace mínimo y CLI sin dependencias externas; upgrade posterior del
  toolchain/MSRV a Rust 1.98.1 por el owner.
- M0-02: dominio separado con referencias/fingerprints validados, resultados y
  errores tipados, diagnósticos multipartes y provenance/freshness coherentes.
- Serde 1.0.229 para contratos base; serde_json 1.0.151 también usado por rmcp. Validación al
  deserializar, rechazo de campos desconocidos y Clock inyectable.
- CLI de ayuda y versión; rechazo explícito de modos no implementados con stdout vacío.
- Lints compartidos, rustfmt/Clippy configurados y tests del binario real.
- Documentación inicial y estrategia de modelos/revisión en AGENTS.md.

No se ha publicado ninguna release binaria. El código fuente sí es público. M0 está cerrada; los cortes M1 y su evidencia
se registran abajo y en el tablero. El gateway de probes M0 no acredita Cargo;
M1 usa un runtime Rust aprobado y calibrado por separado.

### M0-09 — Semantic foundation

- E5 local verificado, ORT sin telemetry y LanceDB memory:// por generación.
- Identidad completa, rebuild atómico y fallback léxico con facts desde SQLite.
- Gate real de inferencia/red, recibo de modelo y verificación de vendor manifest-only.

M0-10 incorpora el [corpus Rust](fixtures/README.md): fixtures compilables revisados,
diagnósticos deterministas y un adversario fuente excluido del harness del host.

### M0-10a — ArtifactStore mínimo

- Streaming en memoria con cap duro, redacción entre chunks, cuotas y TTL.
- IDs aleatorios, hash de bytes almacenados, aislamiento por owner y rollback.

### M0-11 — CI local

- Gate core/full con reportes, toolchain fijo y preflight fail-closed.
- Audit/deny, integrity receipts y matriz explícita, sin workflows remotos.

### M0-12 — Foundation cerrada

- Gate completo12 etapas:185 tests Rust distintos, corpus11 Cargo+1 input de auditoría,
  Docker real y E5/LanceDB local; evidencia y hashes de código conservados.
- Revisión independiente Opus5 High resuelta; restricciones de features reforzadas.
- Tablero actualizado y prompt para iniciar M1-01 con prerrequisitos explícitos.

M1 prerequisite: explicitly approved Rust/Cargo1.98.1 Linux ARM64 provisioning
fixture and immutable local runtime receipt; no additional operative MCP tool.

M1-01 Rust gateway prerequisite: bounded USTAR/source-volume transfer, closed
commands, applied-config verification, independent Rust seccomp profile and six
actual build-script/proc-macro/resource/descendant calibration scenarios. No new
operative MCP tool; integration and external review are tracked separately.

M1-01 project.inspect: metadata declarada capturada, provenance/freshness,
identidades de source/runtime y ProjectRef revalidado al finalizar. Workers joined,
readiness durante bootstrap y cancelación inmediata al cierre del transporte;
shutdown Rust240s acotado, sin confundir handler terminado con cleanup verificado.
Contrato/CLI/protocolo validados; gate core y Rust/MCP real aprobados, ver tablero.

M1-02: rust.toolchain.inspect observa versiones/host/canal y componentes instalados
mediante tres comandos cerrados en el gateway compartido; sin rustup/red/instalación.
Inventario tipado, fingerprints por ejecución y snapshot con ProjectRef revalidado.

### M1-03 — Cargo check y Resources

- Opciones Cargo cerradas, diagnósticos JSON normalizados con sugerencias multipart
  y resultado de compilación válido aunque falle; evidencia parcial explícita.
- Logs combinados acotados en memoria, URI opaca, autorización ProjectRef vivo,
  TTL de artifact sin renovación y lectura Resources privada sin caché.
- Rollback individual de artifacts nuevos sin expulsar logs anteriores. ADR-034.

## M1-04 — Formatting check

- `rust.fmt.check`: configured workspace formatting through the approved read-only
  captured gateway; bounded relative affected files and whole small display diff.
- Shared validation publication preserves live Resources authorization, quotas and
  freshness. No source editing, new dependencies or runtime downloads.

## M1-05 — Clippy

- Closed default/strict/pedantic/project lint profiles with structured findings and
  live-authorized logs; warning vs deny behavior explicit, no fix/source writes.
- Shared Cargo result normalization preserves check semantics; Clippy lint-family
  tags include child suggestions without claiming authenticated compiler origin.

## M1-06 — Cargo test

- Closed package/filter/features/target/timeout, actual contained test execution,
  compilation-phase evidence and bounded raw harness Resources.
- Ambiguous Cargo events after build-finished force incomplete evidence; no
  inferred test counts. Actual libtest descendants cover timeout/cancel/overflow
  plus responsive MCP discovery, backpressure and joined EOF cleanup.

## M1-07 — Local RustSec audit

- Host-expected bounded snapshots through no-follow handles; authoritative SQLite
  advisory selection and RustSec0.32.0 matching with Git/HTTP features disabled.
- Same captured lock/metadata generation, source-aware bounded paths, explicit
  stale/unknown/unsupported coverage and no false clean pass. No runtime refresh.

M1-08 / ADR-039: `rust.diagnostics.explain` accepts only an ASCII `E0000`-shaped
code and obtains bounded text from the approved installed rustc through the same
calibrated, network-denied gateway and joined workers. No project_ref, project source,
resource URI or host rustc execution is needed. Unknown codes return unavailable;
no heuristic explanation substitutes for compiler evidence. Returned text includes
content SHA, immutable runtime identity and latest_known artifact provenance/freshness.
No toolchain/image/model acquisition or native-platform qualification is implied.

M1-09 / ADR-040: `rust.quality.gate` composes fast(fmt/check/strict Clippy) or
standard(+default30s tests/offline audit) over one captured source generation, with
per-stage status, selection, repair detail and runtime evidence. One240s joined
worker; ordinary failures continue, interruption/uncertain cleanup aborts. Logs are
published as a bounded authorized group with final retention/ProjectRef checks;
rollback removes only new IDs, preserving earlier live logs. Omitted nonempty
streams make the quality verdict conservative even when command execution completed.
MCP body/envelope budgets retain stage rows and explicit omissions. No downloads,
source edits, global catalog import or new platform support. M1-10..17 remain pending.

M1-01..09: current integral gate14/14, core498,20 actual Rust gateway tests and
real E5/LanceDB/SQLite network-denied execution. Opus5 quality review resolved with
focused follow-up. Local-only integration; remaining M1/release work stays pending.

## M1-10 — Catalog acquisition and persistence

- Explicit CLI import/local-mirror sync/allowlisted HTTPS sync/status/rebuild, with
  JSON report v1; no new MCP tools or runtime acquisition.
- Domain-separated Ed25519 canonical manifests, bounded Zstd/USTAR, authenticated
  SQLite/RustSec bytes and native semantic restore bound to model/catalog identity.
- Private APFS handle I/O, protected trust file/ancestors, exclusive store lease
  and independently reserved durable sequence floor with exact-container recovery.
- Full15/15 on immutable pre-observability source; final core540, all-features
  Clippy and native CLI5+1 after reviewed floor/status/key-rotation refinements.
  [Separate source/gate receipts and review disposition](docs/validation/M1/10.md).

See [format and limits](docs/catalog-bundle-format.md). Publisher, license and
release remain unapproved; the fixture signing seed is public test data only.

## M1-11 — Read-only catalog status

- Eleventh tool, `rust.catalog.status`: closed empty input, verified component
  identities, current freshness and observable pending sequence reservation.
- Explicit host catalog/trust configuration; lazy read-only session generation,
  retained SQLite/E5/Lance handles, and independent per-call RustSec observation.
- Shared joined admission; 120s cooperative deadline and 128KiB complete result.
  Runtime acquisition remains disabled; no whole-server OS network claim.
- Gate/review recorded in [M1-11](docs/validation/M1/11.md); no M1 closure.
  [ADR-042](docs/adr/ADR-042-catalog-runtime-status.md).

## M1-12 — Bounded crate search

- Gate passed: core603 tests/10 stages, protocol35, all-features/all-targets
  Clippy, and native2 ordinary +1 explicitly run ignored E5/Lance test under
  network deny. Sonnet5 Medium review: no confirmed actionable defect.
- Twelfth tool: lexical, semantic and hybrid retrieval; SQLite version selection
  applies yanked/prerelease/MSRV filters before the result limit.
- BM25 and squared-L2 channel evidence plus deterministic RRF60 fusion; explicit
  lexical fallback, 50 candidates/channel and bounded-window accounting.
- Shared retained catalog/provider and joined worker include JSON validation,
  encoding and suffix trimming under the 512KiB complete-result budget.
- No acquisition authority, platform expansion, ranking-quality claim or M1 closure.
  [ADR-043](docs/adr/ADR-043-catalog-search-modes.md);
  [M1-12 validation](docs/validation/M1/12.md).

## M1-13 — Paged crate inspection

- Thirteenth tool with closed section/version/page input and snapshot-bound
  continuation; existing twelve tool contracts are preserved.
- SQLite scalar and collection pages expose recorded facts, explicit unknown
  documentation/source, missing crate/version outcomes and snapshot mismatch.
- Joined validation/encoding retain the shared worker; complete responses have a
  512KiB budget and preserve whole entries with progressing continuation.
- Gate passed: core629 tests/10 stages, protocol37, all-features/all-targets
  Clippy, and two local-feature tests under OS network deny, without embedding
  inference. Sonnet5 Medium review: no confirmed actionable finding.
  [ADR-044](docs/adr/ADR-044-paged-crate-inspection.md);
  [validation](docs/validation/M1/13.md). No M1 or release closure.

## M1-14 — CLI y doctor

- Doctor humano/JSON format_version1, configuración compartida con serve y checks
  tipados de catálogo, modelo, índice, RustSec, roots y runtime.
- Modo pasivo sin subprocesses; modo activo explícito mediante calibración e
  inventario del gateway Rust aprobado, sin proyecto del usuario.
- Version añade JSON de build; capabilities conserva JSON por defecto y añade
  --human. No nuevas tools MCP ni adquisiciones automáticas.
- Cancelación SIGINT/SIGTERM/SIGHUP con worker unido y cleanup; reportes limitados a128KiB.
  Warning sale0, diagnóstico fallido1 y sintaxis inválida2.
- Gate activo de doctor aprobado: calibración, SIGINT observado y cleanup de
  objetos propios. Full incorpora doctor como etapa19; este resultado focalizado
  no equivale al full conjunto ni al cierre M1.

## M1-15 — Candidatos locales

Preparados candidatos release core/local macOS arm64 con hashes, linkage, archivos de avisos y smoke de instalación offline. Doctor activo verificado en ambos ejecutables; en ese corte aún no había publicación ni licencia aprobada.
