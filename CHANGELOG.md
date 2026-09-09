# Changelog

## 0.3.0-dev — Unreleased

### M5 — cuatro tools de rendimiento, implementadas y sin calificar por completo

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
- **En este runtime `rust.benchmark.compare` no emite dirección alguna**, y son
  dos razones independientes: el governor de CPU es ilegible dentro del
  contenedor, y la deriva medida entre ejecuciones del mismo código en el host
  calificado (6,1–28,7 %, las seis medidas de los dos lados) supera el umbral
  material del 5 %. La tool mide y publica
  el efecto —un cambio de fuente del +25 % se mide como +24,4 %—; lo que no hace
  es llamarlo regresión.
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
  y `bloat_json` —, el mime `image/svg+xml` y sus versiones de payload. Ninguna
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
- **Limitación M5-01**: `rust.benchmark.run` no puede alcanzar hoy su positivo a
  través del contrato de vendor offline del producto. El cierre de Criterion
  0.8.2 son 6 014 archivos y 156 267 469 bytes, con cuatro archivos por encima
  del límite de 1 MiB por archivo, y un `SourceBundle` admite 4 096 entradas,
  16 MiB en total y 1 MiB por archivo. **Los límites no se subieron**: pertenecen
  al contrato de datos offline calificado en M2/M4 y ampliarlos habría debilitado
  una frontera de seguridad sin decisión ni recalificación. Detalle y opciones
  para el owner en [M5-01-blocker.json](docs/validation/M5-01-blocker.json). Sus
  selecciones negativas y de control sí están calificadas.
- Estado: M5 **no está Done**. `rust.profile.flamegraph` y `rust.binary.bloat`
  están calificados nativamente, `rust.benchmark.compare` está probado sobre
  datasets reales del guest, el positivo de `rust.benchmark.run` está bloqueado, y
  el gate conjunto y la matriz de clientes M5 no se han ejecutado
  ([matriz](docs/validation/M5-matrix.md),
  [handoff](docs/validation/M5-handoff.md)). `BenchmarkExit` y `BloatExit`
  conservan `CALIBRATED = false`. No hay release, tag ni cambio de versión.

### M4 — 27 tools implementadas y calificadas localmente

- Implementados `rust.deny`, `rust.unsafe.scan`, `rust.supply_chain.inspect`,
  `rust.quality.gate.v2` y `rust.miri`, en ese orden después de las 22
  definiciones existentes. Los 23 snapshots anteriores permanecen preservados y
  se añadieron cinco nuevos. El [core](docs/validation/M4-core-gate.json), el
  [full](docs/validation/M4-full-gate.json), el [runtime](docs/validation/M4-runtime.json)
  y los [clientes](docs/validation/M4-clients.json) pasaron localmente. La
  [confirmación final](docs/reviews/m4-final-evidence/review.md) acepta el cierre
  local de M4. La implementación `07814664379628f00857feca13148b507de687b9`
  está en el [PR #15](https://github.com/pharos-lang/rust-engineering-mcp/pull/15);
  no hay nueva release ni tag.
- El core pasó 19 etapas (1220 tests Rust, un doctest y 11 tests del helper); el
  full pasó 33 con inventario fuente idéntico. Tras encontrar un directorio E5
  temporal vacío, la reanudación conservó 27 etapas aprobadas y ejecutó seis
  frescas usando assets existentes reverificados, sin descarga ni cambios de
  código. El [fallo original](docs/validation/M4-hardening-attempts/full-attempt-2/receipt.json)
  permanece preservado. El runtime final pasó 19/19 sobre `25ed…`, con scanner
  7/7 y Miri 13 clasificaciones más 7 admisiones.
- La [revisión final de código Opus](docs/reviews/m4-final-closure/review.md) no
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

## 0.2.0-dev — Unreleased

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

La calificación conjunta M2 está completada: [full y clientes](docs/validation/M2-07.md).
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
  [Separate source/gate receipts and review disposition](docs/validation/M1-10.md).

See [format and limits](docs/catalog-bundle-format.md). Publisher, license and
release remain unapproved; the fixture signing seed is public test data only.

## M1-11 — Read-only catalog status

- Eleventh tool, `rust.catalog.status`: closed empty input, verified component
  identities, current freshness and observable pending sequence reservation.
- Explicit host catalog/trust configuration; lazy read-only session generation,
  retained SQLite/E5/Lance handles, and independent per-call RustSec observation.
- Shared joined admission; 120s cooperative deadline and 128KiB complete result.
  Runtime acquisition remains disabled; no whole-server OS network claim.
- Gate/review recorded in [M1-11](docs/validation/M1-11.md); no M1 closure.
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
  [M1-12 validation](docs/validation/M1-12.md).

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
  [validation](docs/validation/M1-13.md). No M1 or release closure.

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
