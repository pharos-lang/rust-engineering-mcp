# Execution Gateway y modelo de seguridad

Este es el capítulo más grande porque concentra la mayoría de las
invariantes no negociables de seguridad de `AGENTS.md`: roots confiables,
I/O no-follow, el Execution Gateway único, deny-by-default verificable, sin
shell arbitrario, admisión por identidad de imagen Docker, cancelación y
cleanup de árboles de proceso, y el registro completo de riesgos residuales
de 1.0. La frontera de distribución (qué plataformas califican como host
positivo) se detalla también aquí porque acota directamente qué garantías
de este capítulo están realmente probadas frente a cuáles son solo la
ambición original de una ADR.

## Roots confiables y handles explícitos

**Decisión (ADR-007, refinada por ADR-024 para `rust.project.open`).** El
host o CLI confiable configura las roots permitidas **antes** de
`project.open`; el proyecto o caller solo puede restringir esa lista, nunca
ampliarla. `project.open` canonicaliza y valida el workspace dentro de esas
roots y emite un identificador opaco, aleatorio, de al menos 128 bits —
nunca derivado del path. El registro de `ProjectRef` vive por proceso,
expira por inactividad configurable y se pierde al reiniciar el servidor;
cada uso posterior revalida la root y la identidad básica del proyecto.

ADR-024 concreta esto para `rust.project.open`: el host suministra hasta 16
roots físicas absolutas vía `--root`; sin roots no hay acceso alguno;
`/` como root se rechaza explícitamente. El campo `validation:"structural"`
del resultado **no es** una certificación de que el proyecto compila ni de
que su resolución de registry es válida — es solo estructura de manifiesto.

## I/O no-follow y por qué canonicalizar-y-abrir no es una frontera

**Decisión (ADR-007/024).** Toda I/O propia del producto abre paths
**relativos a handles de directorio** con semántica no-follow/
reparse-safe equivalente. **Canonicalizar un path y después abrirlo por ese
path NO es una frontera de seguridad** — es vulnerable a TOCTOU (el archivo
puede sustituirse entre la canonicalización y la apertura); esta regla es
explícita en el propio ADR-007, no una inferencia de este documento.

En producción el adapter macOS usa `openat` con `O_NOFOLLOW_ANY` /
`O_RESOLVE_BENEATH` / `O_UNIQUE`, disponibles **solo en macOS 26+/APFS**. Un
sistema operativo que no ofrezca esta primitiva **falla cerrado antes de
cualquier I/O** — nunca anuncia contención estricta sin ella ni degrada
silenciosamente a un modo más débil.

**Riesgo residual (RR-14, ver tabla de riesgos residuales más abajo).**
Abrir un FIFO o un device node puede tener efectos secundarios incluso bajo
no-follow; la ACL no se compara explícitamente (solo se preserva vía
`CLONE_ACL`); la detección de hardlink es solo por `nlink == 1`; la captura
no es atómica frente a un proyecto que cambia bytes durante la lectura.
Aceptado y documentado; se reevalúa ante una versión mayor de macOS o de
APFS.

**Estado actual.** Vigente; evidencia
`crates/project-adapter/src/filesystem/macos/{state_primitives,source,
snapshot,quality,vendor_capture}.rs`; tests en
`crates/project-adapter/tests/{filesystem,source,host_snapshot,
catalog_store}.rs`.

## Execution Gateway único

Decisiones: ADR-008.

**Decisión.** Toda ejecución externa de proceso atraviesa un único adapter
(`execution-adapter`) que consume un `ExecutionSpec` tipado: ejecutable
absoluto verificado, argv cerrado, cwd validado, roots de lectura/escritura,
entorno reconstruido tras `env_clear`, política efectiva, requisito de
sandbox, timeout, token de cancelación y límites de streaming.
`rust.check`/`.clippy`/`.test` ejecutan `build.rs` y proc macros del
proyecto (`executes_project_code = true`) y por eso exigen opt-in de host
más sandbox suficiente antes de correr. La configuración de Cargo es
offline con aislamiento de red real; una dependencia ausente produce un
error tipado, **nunca** una descarga automática. El gateway crea y termina
árboles de procesos completos, no solo el proceso hijo directo.

`scripts/check-architecture.py` prohíbe `Command::new`,
`std::process::Command` y `tokio::process::Command` fuera de
`execution-adapter` — el mismo mecanismo que fuerza la hexagonalidad (ver
[`overview.md`](overview.md)).

**Alternativas rechazadas que siguen explicando el límite actual.**
`Command` disperso por cada adapter sería imposible de auditar; matar solo
el PID padre deja huérfanos ejecutándose; un target Cargo estándar con un
lock interno no contiene código hostil ni coordina procesos externos
concurrentes.

**Contención de proceso Unix por grupo/sesión es best-effort únicamente.**
Un descendiente que llama `setsid()` puede escapar del grupo de procesos
del padre. Que `children_contained=true` sea una garantía **fuerte** exige
namespace de PID + cgroup, más un test explícito de descendiente
daemonizado que no logre escapar — eso es exactamente lo que aporta el
gateway Docker/Linux (ver más abajo), no el propio proceso nativo macOS.

**Sobre Windows.** No existe un mecanismo de contención nativo de Windows en
este producto — ni Job Objects ni ningún otro. La CI de Windows fue retirada
el 2026-09-13 (ver más abajo); CI corre hoy sobre dos plataformas hospedadas
(`ubuntu` x86_64, `macos-26` arm64). Windows no tiene adapter de filesystem
no-follow/reparse-safe nativo ni oráculos de seguridad equivalentes a los de
macOS+Docker Linux, y su restauración como target de CI es deuda de
portabilidad, no una decisión vigente del producto.

**Estado actual.** Parcialmente vigente como regla arquitectónica (el
gateway único, sí; la contención "strong" solo está qualified en
macOS+Docker Linux, ver más abajo). Evidencia:
`crates/execution-adapter/src/{lib,capabilities}.rs`;
`crates/execution-adapter/tests/nextest_runtime.rs`.

## El gateway Docker/Linux como frontera concreta

Decisiones: ADR-025, ADR-031.

**Decisión.** En macOS, `sandbox-exec` por sí solo no puede satisfacer las
garantías de ADR-009 y los grupos de proceso nativos no contienen un
descendiente que llame `setsid`. La solución es un motor Docker Linux local
(**sin pulls, sin auto-arranque del daemon, sin herencia del entorno del
host**): una imagen local inmutable, un contenedor con su propio PID
namespace y cgroup por cada ejecución, rootfs solo lectura, sin mounts de
host, sin capabilities Linux, `no-new-privileges`, y una allowlist seccomp
que niega explícitamente `socket`/`unshare`/`mount`/`ptrace`/`bpf`.
**`network=none` por sí solo NO es suficiente** — se declara explícitamente
insuficiente como única garantía; la combinación completa (namespaces +
seccomp + cgroups + rootfs read-only) es la frontera real. Timeout,
cancelación o desbordamiento de recursos eliminan el **contenedor entero**,
no solo el proceso Cargo dentro de él.

ADR-031 añade el runtime Rust aprobado y el "source transfer" acotado: la
captura de source vía los handles no-follow existentes produce un
`SourceBundle`, empaquetado como USTAR acotado, cargado en un volumen Docker
gestionado con directorios root-owned de solo lectura; un contenedor
UID0/caps0 hace solo la ingesta del tar; Cargo corre después como no-root,
sin capabilities, `network=none`, con lockfile congelado (`frozen`). ADR-031
tiene una enmienda de identidad de imagen (M3) ya reflejada en el código —
vigente, no sustituida.

**Estado actual.** Vigente; confirmado como frontera de ejecución real de
1.0 por ADR-087 (ver más abajo, "Frontera de distribución"). Evidencia:
`crates/execution-adapter/src/{rust_gateway,security_gateway,
security_native,capabilities,source_archive}.rs`;
`crates/execution-adapter/src/security_gateway.rs` (13 tests),
`security_native_adversarial.rs`, `tests/{gateway,m4_portable_boundaries,
m4_privacy_runtime}.rs`.

## Deny-by-default verificable e invariantes de aislamiento

Decisiones: ADR-009.

**Decisión.** Las capacidades de sandbox se modelan por separado —
filesystem, red, hijos, entorno, CPU, memoria, PID, aislamiento de disco —
**nunca como un booleano único**. Los perfiles nombrados son
`strict`/`restricted`/`none`; cada tool declara sus efectos y requisitos.
Cuando falta una garantía requerida, la operación **falla cerrada con
`SANDBOX_DENIED`; nunca se degrada silenciosamente** a una garantía más
débil. Ningún tool que compile o ejecute código no confiable del proyecto
corre bajo `none`. La configuración de seguridad es monótona: defaults <
usuario/host confiable < CLI confiable, y la configuración del propio
proyecto **solo puede restringir**, nunca ampliar, lo que el host ya
concedió. Una clave de seguridad desconocida en cualquier configuración es
un error, no un valor ignorado. Existe una matriz normativa cerrada
tool × capability que fija qué necesita cada tool.

**Invariantes de aislamiento** (`crates/domain/src/execution.rs`):
- `offline_cooperative` y `network_isolated` son estados **distintos**: el
  primero describe "no se hace I/O de red por diseño del comando"; el
  segundo es una afirmación de que el sandbox del sistema operativo
  **aplica** esa ausencia de red, no solo que el comando eligió no usarla.
- "Fuertemente contenido" (`children_contained=true` en su forma fuerte)
  tiene **una sola definición**: namespace de PID + cgroup, verificado por
  un test de descendiente daemonizado que no logra escapar. No existe una
  variante "fuertemente contenido" más débil en ningún adapter del
  producto.
- `restricted` sigue exigiendo aislamiento de red real aplicado por el
  sistema operativo — no es un nivel "menos estricto que `strict` pero sin
  sandbox".
- Una petición que requiere `network_isolated` sin que el sandbox del
  sistema operativo pueda aplicarlo se **rechaza**, nunca se degrada a
  ejecutar sin esa garantía y reportarlo después. El host puede escoger
  explícitamente otra policy que sí permita red cuando la operación lo
  admita, pero eso es una elección explícita del host, nunca una
  degradación silenciosa de una petición que pedía aislamiento.

**Sobre `allow_project_code`: no existe como flag de host.** El código de
`crates/application/src/execution.rs` define `admit_execution(tier,
executes_project_code, allow_project_code, evidence,
expected_configuration)` con el comentario "used by future tool adapters
after selecting a concrete, verified configuration" — es una función de
admisión genérica pensada para un probe de capability, **no** está cableada
hoy a un flag de CLI del host llamado `allow_project_code`. El opt-in real
para ejecutar código de proyecto hoy es la propia configuración del runtime
Docker (imagen aprobada + tier `Strict`) descrita arriba, no un booleano de
configuración separado. No documentar `allow_project_code` como un flag de
host existente.

**Alternativas rechazadas que siguen explicando el límite actual.** Un
booleano único de "sandbox sí/no" no permitiría declarar qué capacidad
concreta falta; degradar silenciosamente cuando falta una garantía
ocultaría al agente que el resultado es menos confiable de lo que pide.

**Limitación de alcance de plataforma.** La ambición multiplataforma del
propio ADR-009 no se cumple: la matriz `strict`/`restricted` solo está
realmente qualified en macOS ARM64 + gateway Docker Linux ARM64 (ver
"Frontera de distribución" más abajo). Linux x86_64 y Windows x86_64 no
tienen un adapter de filesystem no-follow/reparse-safe nativo ni oráculos de
seguridad equivalentes.

**Estado actual.** Parcialmente vigente (la regla, sí; el alcance de
plataforma, no). Evidencia: `crates/application/src/{profile,lib,
quality}.rs`; `OperationalErrorCode::SandboxDenied` en múltiples módulos
`stdio`; tests bajo `crates/mcp-server/tests/` y
`crates/execution-adapter/tests/nextest_runtime.rs`.

## Sin shell arbitrario

Decisiones: ADR-010.

**Decisión.** Nunca `sh -c`, `bash -c`, `cmd /c` ni PowerShell con entrada
del usuario. Los ejecutables se resuelven a paths absolutos confiables; el
argv se construye desde enums y newtypes validados, nunca desde
concatenación de strings; no hay flags arbitrarios finales, variables de
entorno wrapper suministradas por el caller, runner/linker del caller, ni
cadenas de comando en la configuración de proyecto. CI inspecciona el uso
de APIs de proceso fuera del gateway con el mismo mecanismo que fuerza
ADR-008.

**Estado actual.** Vigente; evidencia `scripts/check-architecture.py` (ban
regex) — sin invocaciones reales de shell en código de producto, sin test
Rust dedicado más allá del gate de arquitectura.

## Las tools M1 individuales sobre el gateway

**Decisión (ADR-034/035/036/037).**
- `rust.check` — opciones cerradas, `frozen`/offline, `jobs=1`; solo
  `exit 0` con un evento `build-finished` completo cuenta como `passed`;
  artifacts vía `rust-artifact://` opaco.
- `rust.fmt.check` — solo `project_ref` como input; comando fijo
  `cargo fmt --all --check`; el diff nunca se aplica como edición (eso
  vive en `rust.fmt.apply`, M2, ver [`mutation.md`](mutation.md)).
- `rust.clippy` — 5 selecciones cerradas; `lint_profile` es un enum
  `default/project/strict/pedantic`; `pedantic` **global** está
  explícitamente prohibido (el agente no puede forzarlo sobre todo el
  workspace).
- `rust.test` — grammar cerrada; `test_filter` ASCII acotado; timeout en
  el rango 1..60 s; el JSON de Cargo se parsea solo hasta el primer evento
  `build-finished` completo — el resto de la cola es harness no confiable
  pero se retiene, no se descarta.

Las cuatro comparten el mismo `MemoryArtifactStore` (ADR-028, ver
[`jobs-and-artifacts.md`](jobs-and-artifacts.md)) con autorización de
registro por `ProjectRef`. `rust.test.nextest` (ADR-064, M3) **coexiste**
con `rust.test` — no lo sustituye; ambos contratos siguen vigentes.

**Estado actual.** Vigente; evidencia `crates/mcp-server/src/stdio/
{check,format,clippy,testing}.rs`; snapshots
`{check,format,clippy,test}-tool.json`.

## Staging y fix de mutación dentro del mismo gateway

Decisiones: ADR-053, ADR-056.

**Decisión.** El gateway único se **extiende**, nunca se duplica, con fases
tipadas para M2 (mutación, ver [`mutation.md`](mutation.md)): un volumen
tmpfs Docker acotado con opciones exactas de montaje; un guardián confiable
(`sleep 900`) que mantiene el mount vivo entre las fases de una misma
operación; un ciclo de vida estrictamente ordenado; ingest/export solo vía
USTAR con validación en memoria. `cargo fix` corre bajo un perfil seccomp
**dedicado solo a esa fase**, que añade loopback TCP acotado
(`SCMP_CMP_MASKED_EQ`) sin ampliar los perfiles M1/fmt/ingest/exporter. El
éxito exige `exit 0` + JSON válido + un `cargo check` independiente
posterior que confirme que el resultado sigue compilando.

**Estado actual.** Vigente; evidencia
`crates/execution-adapter/src/{mutation_gateway,rust_applied}.rs`, perfiles
`seccomp-rust-fix.json`/`seccomp-socket.json`; tests
`tests/inspection_runtime/{fix_mutation,fix_hostile}.rs`.

## Extensiones de seccomp para quality y coverage

Decisiones: ADR-064, ADR-065.

**Decisión.** El perfil seccomp de calidad (M3) es **byte-idéntico** al
perfil M1 más exactamente una regla (`socketpair(AF_UNIX, SOCK_STREAM
masked, 0)`), y solo se aplica en la fase `TestNextest`. Un tercer volumen
tmpfs por-job (`/work/coverage-target`, 512 MiB/65536 inodos) se añade para
`rust.coverage`, con una matriz de acceso cerrada por fase: ejecutable solo
en `run`/`report`, solo lectura para el guardián, ausente por completo de
los exporters. Ambos cambios alteran el fingerprint de configuración del
gateway y exigen un rerun completo del gate para volver a calificarse.

**Estado actual.** Vigente; evidencia `crates/execution-adapter/src/
{seccomp-rust-quality.json, coverage_gateway.rs, rust_gateway.rs}`.

## Admisión por identidad inmutable

Decisiones: ADR-068, ADR-077, ADR-085.

**Decisión.** El gateway admite un conjunto **cerrado y no descubrible** de
digests SHA-256 de imagen — **nunca** tags mutables. Cada milestone (M4/M5/
M6) admite exactamente los digests nuevos que necesita, sin reemplazar ni
ampliar la admisión de milestones anteriores. La CLI/host debe seleccionar
la identidad exacta; observación pasiva de inventario **nunca** sustituye
la calibración real del gateway. Un rollback es simplemente repuntar el
gateway al digest anterior — no hay migración de estado que ejecutar.

**Patrón repetido.** Cada provisioning-ADR (adquisición/build de la imagen —
ADR-063/066/075/082, ver [`operations/runtime-provisioning.md`](../operations/runtime-provisioning.md))
va seguido de un admission-ADR **separado** (ADR-068/077/085) que es el
único que autoriza al gateway a invocar esa imagen. El caveat
`approved_for_gateway=false` de un provisioning-ADR no debe leerse como
vigente sin revisar su admission-ADR correspondiente.

**Riesgo de operación real (ADR-085).** La admisión de la imagen M6 es
**global** en la lista cerrada de `RustGateway::new` — un host con la
imagen M6 puede técnicamente invocar tools M1-M5 sobre ella, pero esa
combinación se declara explícitamente **NO qualified**. Asumir "imagen más
nueva = superset de capacidad qualified" es un error real que un operador
podría cometer.

**Estado actual.** Vigente; evidencia
`crates/execution-adapter/src/{security_inventory,deny_json,
miri_admission,miri_native,unsafe_native,performance_port,
analyzer_gateway}.rs`; [`docs/validation/M4/{base-calibration,
deny-adversarial,scanner-native,miri-native,runtime,full-gate}.json`](https://github.com/pharos-lang/rust-engineering-mcp/tree/51fa602e/docs/validation/M4),
[`docs/validation/M5/{00-admission-runtime,native-gate}.json`](https://github.com/pharos-lang/rust-engineering-mcp/tree/51fa602e/docs/validation/M5),
`tests/data/m5-runtime-provisioning.json`,
`tests/data/m6-runtime-provisioning.json` y [`docs/validation/M6/01-calibration.json`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/validation/M6/01-calibration.json).

## Scanner de sintaxis `unsafe` aislado

Decisiones: ADR-069.

**Decisión.** Un subproceso helper secuencial, **uno por archivo** — nunca
uno para todo el proyecto a la vez, para no agotar la pila con un archivo
hostil individual. Es una fase tipada del gateway, sin paths, flags ni
shell provenientes del cliente. Cap de 2 s por hijo, deadline global entre
1 y 120 s. El host valida toda la salida del parser; **nunca** confía en
texto de error libre que el propio parser hostil podría fabricar. Un
presupuesto de 25 s se reserva para control, lanzamiento y cleanup, para
que estos nunca hagan exceder el deadline global. Los cuerpos de macro se
tratan como opacos — nunca se expanden. **Zero findings nunca prueba
seguridad** — la cobertura parcial del escáner se declara explícitamente,
nunca se oculta.

**Estado actual.** Vigente (v3 gobierna sobre las enmiendas v1/v2 previas
del mismo documento); evidencia
`crates/execution-adapter/src/unsafe_native.rs`,
`crates/mcp-server/src/stdio/unsafe_scan.rs`;
[`docs/validation/M4/scanner-native.json`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/validation/M4/scanner-native.json) (7 casos, incluido agotamiento de
budget), `runtime.json` (19/19).

**Limitación documental.** [`docs/adr/README.md`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/README.md) marca ADR-069 como
"pendiente" — está desactualizado; el cuerpo y el código muestran
implementación completa (ver [`decisions.md`](decisions.md)).

## Captura de vendor a gran escala, separada de `SourceBundle`

Decisiones: ADR-078.

**Decisión.** El árbol vendor (dependencias resueltas offline vía `cargo
vendor`, ver [`mutation.md`](mutation.md)) se captura — **nunca se monta
read-write** — y se autentica en el host antes de que el guest lo vea;
queda content-addressed por digest e inmutable. Tiene sus propias cuotas,
**mayores** que las de `SourceBundle` (512 MiB / 32768 entradas / 8 MiB por
archivo / profundidad 16) — las cuotas de `SourceBundle` **no se suben**
para dar cabida a este caso; el volumen tmpfs se dimensiona desde los
límites de este contrato específico, no desde `VOLUME_OPTIONS` compartido
con otras fases.

**Limitación declarada por el propio ADR.** Solo ~156 MB de captura de
vendor está empíricamente qualified, pese a que el contrato permite hasta
512 MiB — el ADR es explícito en que "este documento no afirma que funcione"
para capturas mayores a lo medido.

**Estado actual.** Vigente (como plenamente enmendado); evidencia
`crates/domain/src/vendor_capture.rs`,
`crates/project-adapter/src/{vendor_capture,filesystem/macos/
vendor_capture}.rs`; [`docs/validation/M5/01-vendor-capture-measurements.json`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/validation/M5/01-vendor-capture-measurements.json).

## Frontera de distribución: por qué "vigente" casi siempre significa "macOS ARM64"

Varias de las decisiones anteriores tienen una ambición multiplataforma en
su ADR original que **no** corresponde al estado calificado real. La
frontera efectiva, confirmada para 1.0 por ADR-087 sin sustituir ADR-048
(0.1.0), es: **macOS 26 ARM64/APFS como único host nativo positivo, con el
gateway Docker Linux ARM64 fijo para toda ejecución de código de
proyecto**. Linux x86_64 es hoy el único target de CI adicional de
portabilidad/fail-closed — sin adapter de filesystem no-follow nativo, sin
oráculos de seguridad G4 nativos, sin artifact de release. Windows x86_64
tuvo ese mismo rol hasta que su CI fue retirada el 2026-09-13 por una
regresión de stdio previa a `initialize` (ver ADR-003 en
[`mcp-and-contracts.md`](mcp-and-contracts.md#stdio-como-único-transporte-y-su-presupuesto));
su restauración es deuda de portabilidad, **nunca** un criterio de 1.0.
Linux ARM64 y macOS x86_64 no se anuncian ni se testean en absoluto. La matriz aspiracional de cinco
triples de la especificación original se mantiene en el texto histórico
como una aspiración admitida-incumplida, no se borra ni se reescribe como
si se hubiera cumplido. El detalle de release/publicación de esta frontera
vive en [`reference/compatibility.md`](../reference/compatibility.md) y en
[`operations/release-verification.md`](../operations/release-verification.md).

CI, además, corre hoy sobre **dos** plataformas hospedadas (`ubuntu`
x86_64, `macos-26` arm64) — Windows fue retirada, no es una tercera
plataforma activa.

## Registro de riesgos residuales de 1.0

Decisiones: ADR-089.

Estos son los diecinueve riesgos que 1.0 acepta para su único host
positivo (macOS ARM64 + gateway Docker Linux ARM64). Aceptar un riesgo
**no** amplía ninguna capability ni convierte una limitación en garantía;
una condición de reevaluación cumplida **suspende** la aceptación de ese
riesgo específico hasta un nuevo ADR. La auditoría independiente citada es
explícitamente una **revisión de modelo** (Claude Opus 5, solo lectura),
**nunca** una auditoría humana ni un pentest (RR-01). Tabla completa,
reproducida sin abreviar desde el registro normativo
([ADR-089](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-089-residual-risk-register.md)):

| ID | Riesgo aceptado | Severidad | Alcance | Mitigación existente | Condición de reevaluación |
| --- | --- | --- | --- | --- | --- |
| RR-01 | La auditoría independiente es una revisión de modelo (Opus 5 High, read-only), no una auditoría humana ni un pentest; puede omitir clases de fallo que un pentest encontraría | Media | Todo el producto 1.0 | Threat model con citas, oráculos nativos por hito, revisiones independientes previas (matrices M1–M8) | Antes de anunciar un host distinto de macOS ARM64, un transporte remoto (M7) o un catálogo/modelo oficial (D15/D16); ante un P0/P1 reportado tras la release; si el owner contrata una auditoría humana |
| RR-02 | Linux x86_64 y Windows x86_64 no calificados; Linux ARM64 y macOS x86_64 no anunciados | Baja | Hosts no macOS | Adapters fallan cerrados; CI de portabilidad; sin artifact | Subprograma D13 con adapter no-follow/reparse-safe, oráculos G4 nativos, host real y ADR sucesor de ADR-087 |
| RR-03 | Deuda M6 de las 5 tools analyzer `preview`: `SANDBOX_DENIED` mezcla rechazo permanente y capacidad transitoria; diagnósticos solo de sintaxis; assists no deterministas; precisión de `admitted` en el audit tras denegación post-grant; apply no verificado por compilación; e2e `analyzer_runtime.rs` desgateado; ausencia de procesos por muestreo | Media | `rust.analyzer.*` | Clase `preview`; 12 cortes nativos M6; writer M2; `ACTION_STALE` | Antes de promover cualquiera de las cinco a `stable`, o de habilitar diagnósticos semánticos (Opción B de ADR-084) |
| RR-04 | `rust.dependencies.audit` degrada (no bloquea) ante snapshot RustSec stale o de edad desconocida | Media | Audit y gates que lo componen | `AuditIssue::SnapshotStale`/`SnapshotUnknownAge` explícitos; freshness y provenance en el resultado | Siguiente major que permita cambiar el contrato M1, o evidencia de un cliente calificado que trate un audit stale como limpio |
| RR-05 | Kernel LinuxKit, runc, Docker Desktop y el daemon están en la TCB; un 0-day de esas piezas escaparía del guest. Deltas de seccomp aceptados: `socketpair` AF_UNIX (quality), `socket` AF_INET loopback con `--network=none` (fix), `perf_event_open` en una fase (profiling) | Media | Toda ejecución de código del proyecto | Seccomp deny-default por fase, sin red, `--cap-drop=ALL`, `no-new-privileges`, rootfs read-only, límites de PIDs/memoria/CPU, cuarentena | Advisory que afecte a runc, seccomp, Docker Desktop o el kernel del guest; cambio de imagen aprobada o de perfil (exige recalibración) |
| RR-06 | Deadlines cooperativos y sin límite duro de RSS/CPU en el proceso host (catálogo, ORT, RustSec, parsers) | Baja | Proceso servidor | Caps de bytes/entradas/resultados, workers unidos, admisión de 16 | Transporte remoto o multi-tenant; medición M8-05 o soak M8-09 fuera de budget |
| RR-07 | Sin detección universal de secretos: source concedido y sus secretos se retienen en artifacts, logs y diffs; `assert_no_credentials` detecta por nombre de archivo, no por contenido; sin secret scanning en CI | Media | Artifacts, logs, evidencia de validación | Redacción literal, normalización M4, canarios, `.gitignore`, eventos sin paths | Publicar evidencia sobre repositorios de terceros; habilitar remoto o telemetría |
| RR-08 | Otros procesos del mismo uid y un host malicioso quedan fuera de la frontera; ACLs no inspeccionadas; el dueño que restaura o borra todo el estado reinicia el floor | Media | State root, trust, artifacts, checkout | `0700`/`0600`, uid efectivo, `nlink == 1`, binding owner, floor separado | M7 (multi-tenant/remoto) o cambio del modelo de permisos/ACL de macOS |
| RR-09 | Mutación `local_coordinated`: sin CAS ni exclusión OS de editores, sin atomicidad multiarchivo visible, power loss no demostrado (solo ENOSPC inyectado); un journal corrupto bloquea el store compartido | Media | 6 tools de escritura | Journal versionado, revalidación de identidad y bytes, recovery explícito, `doctor.mutation_journals` | Nuevo adapter de host, pérdida de datos reportada o cambio de semántica de APFS |
| RR-10 | Dependencia comprometida sin advisory publicado no se detecta; `build.rs` de dependencias corren en CI y en el host de desarrollo; `paste 1.0.15` unmaintained. El job `build` de `release-candidate.yml` concede `id-token`/`attestations: write` al mismo job que compila `build.rs` de dependencias antes de atestar | Media | Build, CI y binario; publicación | `cargo audit`/`deny`, pins `=`, `--locked`, vendor por SHA-256, imágenes por digest, CODEOWNERS; `persist-credentials: false` en todos los checkouts | Tarea post-M8 de paquetería; advisory RUSTSEC nuevo; cambio de `deny.toml`; separar el job de attestation de la compilación (descartado antes de RC1: no ejecutable sin más riesgo que beneficio) |
| RR-11 | La firma Ed25519 autentica al publisher que eligió el host, no la corrección de los facts; sin trust root ni catálogo oficial; el E5 fijado no se audita | Baja | Catálogo y búsqueda semántica | Firma antes del parsing, floor, SHA-256 E5, SQLite autoritativo, LanceDB derivado | Decisiones D15/D16 de distribución de catálogo o modelo |
| RR-12 | La attestation OIDC acredita el workflow, no reproducibilidad; `SONAR_TOKEN` es secreto de larga vida de un tercero. Re-observar branch protection, ejecutar la provenance 0.8.x y verificar D14 sobre assets reales son ítems **bloqueantes de RC1**, no riesgo aceptado | Media | Artifacts y repositorio público | OIDC sin clave, `gh attestation verify` con signer exacto, permisos mínimos, acciones por SHA, guard de forks, CODEOWNERS, solo draft | Cualquier cambio de workflows o de protección |
| RR-13 | Resultados producidos por código del proyecto (tests, lints, benchmarks, harness) no están autenticados | Baja | Tools que ejecutan código | Clasificación conservadora, tests de forgery, tamaño solicitado vs. observado | Si un contrato empezara a afirmar autenticidad de esos resultados |
| RR-14 | Límites del filesystem macOS: FIFO/device-node open con posibles efectos, ACL conservada sin comparar, hardlinks detectados solo por `nlink`, captura no atómica | Baja | Captura y writer | `O_NOFOLLOW_ANY`/`O_RESOLVE_BENEATH`, rechazo de links, detección de cambios | Nueva versión mayor de macOS o de APFS |
| RR-15 | Sin revocación en caliente de grants; un `kill -9` del servidor deja contenedores/volúmenes etiquetados | Baja | Grants y jobs | Reinicio, cleanup unido, cuarentena, etiquetas | Transporte remoto; residuos observados en soak M8-09 |
| RR-16 | Retención y borrado de journals, backups y logs dependen del operador; sin borrado seguro en disco ni en RAM | Baja | State root, backups, stderr | TTL y cuotas de artifacts, `prune` explícito, permisos privados | Requisito de cumplimiento/privacidad o transporte remoto |
| RR-17 | Brechas de oráculo en gates obligatorios: rollback con dos binarios fuera de `core`/`full`, e2e del analyzer desgateado, power loss sin oráculo | Baja | Calificación | Oráculos manuales con recibo (`03-rollback.json`, cortes M6) | Cierre M8-09: dos RC consecutivos |
| RR-18 | `rmcp` 3.2.0 forma parte de la TCB del protocolo: puede citar campos del request en errores y retiene permisos de cancelaciones suprimidas hasta reconectar | Baja | stdio | Pin `=`, admisión propia, tests en cinco revisiones MCP | Cualquier subida de `rmcp` |
| RR-19 | Sin firma de código ni notarización macOS en el binario publicado (añadido por la auditoría independiente V04) | Baja | Archive de release | Integridad por `SHA256SUMS` y attestation OIDC, no por Gatekeeper | Distribución por un canal que active Gatekeeper (p. ej. Homebrew o un instalador) fuera del archive/attestation actual |

**Nota de precedencia documental.** [`docs/adr/README.md`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/README.md) resume esta lista
como "RR-01…RR-18"; está desactualizado — el cuerpo del ADR y
[`docs/security-model.md`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/security-model.md) llegan hasta RR-19 (añadido por la auditoría
independiente V04, 2026-09-15). No usar el índice como fuente de conteo.

**Estado de release, para no sobre-afirmar.** 1.0 **no está lista**:
[`docs/validation/M8/checklist-1.0.md`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/validation/M8/checklist-1.0.md) tiene las filas 6, 7, 10 y 11
abiertas. M8 (contract freeze 0.8.0, migraciones/rollback, budgets, threat
model, matriz de clientes) está integrado, pero el cierre M8-09/1.0 sigue
pendiente. `v0.9.0-rc.1` **no** es un tag ni una release publicada — es
solo la versión de trabajo del checkout actual. El binario publicado no
tiene firma de código ni notarización (RR-19, riesgo aceptado); la única
provenance de release es `SHA256SUMS` más la attestation de build OIDC de
GitHub. RR-12 sigue solo **parcialmente** cerrado.

## El threat model M8-08: fronteras, controles y oráculos

El [threat model M8-08](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/validation/M8/08-threat-model.md) es la entrada
que revisa la auditoría independiente citada en RR-01 — no es en sí mismo
una auditoría. Cubre el producto en su host positivo único (macOS ARM64 +
gateway Docker Linux ARM64), transporte stdio local; HTTP/remoto queda
fuera de alcance por estar diferido (M7). Identifica **8 fronteras de
confianza** y evalúa **53 controles** contra ellas, cada uno con un oráculo
de una de cuatro clases: **N** (nativo: test real sobre APFS/Docker/imagen
aprobada), **U** (unit/contract/protocol sin frontera de SO), **H**
(evidencia histórica o de configuración, no re-ejecutada en este corte) o
**—** (sin oráculo, propiedad explícitamente no garantizada).

### Fronteras de confianza

| ID | Frontera | Lado confiable | Lado no confiable |
| --- | --- | --- | --- |
| B1 | Host / operador → servidor | argv validado de `serve`, Docker CLI por ruta/socket explícitos | Configuración parcial, rutas dentro de roots, daemon/VM Docker |
| B2 | Cliente MCP / agente → servidor | Admisión, schemas cerrados, grants del host | Todo el tráfico JSON-RPC |
| B3 | Proyecto hostil → captura, Cargo, rust-analyzer | Captura no-follow y bytes propios, argv cerrado | `build.rs`, proc macros, tests, benches, config de proyecto, peer LSP |
| B4 | Guest Docker → host | Flags del contenedor, seccomp por fase, cleanup unido | Todo proceso dentro del contenedor |
| B5 | Catálogo / bundle firmado → store | Trust del host, floor persistido | Bytes del bundle, SQLite, red de sync |
| B6 | Modelo E5 / ORT / LanceDB → proceso | Hashes fijados en código, `memory://` | Bytes del modelo, índice importado |
| B7 | Evidencia / receipts / artifacts / journals | Store owner-bound, formatos versionados | Objetos en disco, locators, logs, evidencia de clientes |
| B8 | Pipeline de publicación GitHub / OIDC | Workflow en tag, OIDC, branch protection | Acciones de terceros, PRs de forks, tokens |

### B1 — Host / operador

| Entrada hostil / abuso | Control existente | Oráculo | Residual |
| --- | --- | --- | --- |
| Grants de escritura sin runtime, root de escritura fuera de las de lectura, o journal dentro de una root | `serve` se invalida (`host_config.rs`: escritura exige grupo Docker y root ⊂ lectura; `rust-mcp-mutations-v1` siempre fuera de toda root) | U | Baja |
| Imagen no aprobada o tag mutable | Lista cerrada de digests; comprobación por llamada en cada gateway M4-M6; digest del ejecutable Docker fijo | N (`m5-runtime`, `m6-runtime`, `m4-tampered-plugin`) | Baja; un daemon hostil queda fuera (RR-05) |
| Profiling concedido sin gateway o con valor abierto | Valor cerrado `user-space-sampling`; sin grupo `--rust-*` la invocación es inválida | N (recibo M5-03) | Baja; sin revocación en caliente (RR-15) |
| Política de seguridad, vendor o captura dentro de una root del proyecto | Rechazo de configuración; digest esperado de la política de seguridad | U | Baja |
| State root, journal o trust con permisos laxos | Directorios `0700`/archivos `0600`, uid efectivo, `nlink == 1` | N + U | Media: ACLs no inspeccionadas (RR-08) |
| Daemon Docker, VM de Docker Desktop o socket del host comprometidos | Solo reducción (`env_clear`, `--config` propio); sin garantía frente a un daemon hostil | — | Media (RR-05) |

### B2 — Cliente MCP / agente

| Entrada hostil / abuso | Control existente | Oráculo | Residual |
| --- | --- | --- | --- |
| Líneas gigantes, EOF parcial, clientes lentos, flood | Línea ≤ 1 MiB, deadline de frame 10 s; 16 requests/notifications/sends | U | Baja: cancelaciones suprimidas por rmcp retienen permisos hasta reconectar (RR-18) |
| Ampliar autoridad por argumentos, URI, `taskId`, fingerprint o annotations | Autoridad solo del host; errores uniformes (`task unavailable`, Resources opacos, mutaciones cerradas y denegadas por defecto) | U | Baja |
| Cancel/EOF para dejar trabajo huérfano o recuperar capacidad antes del cleanup | Worker unido hasta cleanup | N | Baja |
| Commit o replay con plan ajeno, expirado o de otro kind | Replay solo con ID/digest/key exactos (ADR-059, ver [`mutation.md`](mutation.md)) | N | Baja |
| Flags o argv arbitrarios | Schemas cerrados congelados; argv del perfilador sin paths del peer | U | Baja |
| Agotar recursos lanzando jobs o artifacts | Un worker único; cuatro planes/64 MiB; cuotas del store de artifacts | N | Baja |

### B3 — Proyecto hostil (build.rs, proc macros, tests, benches, rust-analyzer)

| Entrada hostil / abuso | Control existente | Oráculo | Residual |
| --- | --- | --- | --- |
| `build.rs`/proc macro con red, lectura del host o escape durante check/Clippy | Todo corre en el guest (B4); calibración de contención real | N | Media solo por kernel/runc (RR-05) |
| Tests que dejan descendientes desacoplados | Contención y limpieza verificada por calibración y fixture `leaky` | N | Baja |
| Proc macro que falsifica eventos de Cargo o salida de herramienta | La forgery no oculta un fallo de Cargo posterior | N | Baja (RR-13) |
| `cargo fix` con proc macro que muta el manifest o escribe fuera | Staging guest y publisher host acotado (ADR-053/054); perfil seccomp dedicado añade solo loopback | N | Baja: TCP loopback interno del namespace (RR-05) |
| `.cargo/config*` que redirige sources, linker o rustflags | Predicado de rechazo aplicado antes del volumen; Miri rechaza configuración de fuente | N + U | Baja |
| `rust-analyzer.toml` que reactiva build scripts u `overrideCommand` | Rechazo en captura y defensa en gateway | N | Baja |
| Peer LSP hostil: frame gigante, crash, nunca quiescent, cancelación | Codec acotado y deadlines (ADR-084); `env_clear` | N | Baja; ausencia de procesos por muestreo (RR-03) |
| Flood de salida, JUnit/tar/HTML hostiles, symlink en ruta fija | Límites por-tool en nextest/coverage/mutation runtime | N | Baja |
| Symlink/hardlink o sustitución del checkout durante la captura | `O_NOFOLLOW_ANY`/`O_RESOLVE_BENEATH` en cada open | N | Baja: FIFO/device-node y captura no atómica (RR-14) |
| Binario perfilado que pre-crea artifacts o deja nietos | Vaciado de PID namespace, `O_EXCL`, reconciliación de manifest (ADR-074) | N | Baja |
| Benchmark que falsea sus propios números | Ninguno por diseño; origen no autenticado declarado | — | Baja (RR-13) |
| Secretos del host filtrados a HTML, diffs, logs o diagnósticos | Captura solo de roots concedidas; normalización M4 | N | Media: los secretos del propio source sí se retienen (RR-07) |

### B4 — Guest Docker → host

| Entrada hostil / abuso | Control existente | Oráculo | Residual |
| --- | --- | --- | --- |
| Syscalls de escape (mount, unshare, ptrace, bpf…) | Los seis perfiles seccomp tienen `defaultAction: SCMP_ACT_ERRNO`; ninguno lista esas syscalls | N | Media (RR-05) |
| Red desde el guest | `--network=none`; perfiles rust/quality/profile no permiten `socket` (solo `socketpair` AF_UNIX en quality) | N + U | Baja |
| Fork bomb, memoria, CPU | `--pids-limit=128 --cpus=1 --memory=1g --memory-swap=1g`; tmpfs `/work` 512 MiB y `/tmp` noexec 64 MiB | N | Baja |
| Escalada de privilegios | `--cap-drop=ALL`, `no-new-privileges`, `--read-only`, `--ipc=private`, `--cgroupns=private`; usuario no-root por fase | N | Media (RR-05) |
| Cleanup incierto para reutilizar un gateway sucio | `rm --force` + verificación de ausencia; si no, cuarentena; ejecución bloqueada mientras esté en cuarentena | N | Baja; `kill -9` del servidor deja objetos (RR-15) |
| Ampliación por profiling | Solo la fase `ProfileRun` usa el perfil profile, que añade solo `perf_event_open` | N + U | Baja |
| Vulnerabilidad de kernel LinuxKit, runc o Docker Desktop | Ningún control propio | — | Media (RR-05) |

### B5 — Catálogo / bundle firmado

| Entrada hostil / abuso | Control existente | Oráculo | Residual |
| --- | --- | --- | --- |
| Bundle falsificado o alterado | Firma Ed25519 con prefijo de dominio verificada **antes** del parsing JSON; versión cerrada | U | Baja |
| Rollback a una secuencia anterior | `SequenceFloor::permits`; floor reservado antes de activar | N | Baja: el dueño que borra todo el estado lo resetea (RR-08) |
| SQLite hostil (esquema, `user_version`, triggers) | Validación de esquema y ledger; sin SQL del caller | U | Baja |
| Descarga oculta o red durante tools | Sync solo por CLI, `https_only`, `no_proxy`, sin redirects, hostname canónico | N | Baja |
| Trust sustituido o permisivo | `0600` bajo padre `0700` | U | Media: ACLs (RR-08) |
| Clave de publisher comprometida o publisher malicioso elegido por el host | Ninguno técnico: la firma autentica al publisher que el host eligió, no el contenido; sin catálogo ni clave de producción oficial | — | Baja (RR-11) |

### B6 — Modelo E5 / ORT / LanceDB

| Entrada hostil / abuso | Control existente | Oráculo | Residual |
| --- | --- | --- | --- |
| Modelo o tokenizer sustituidos | Revisión, tamaños y SHA-256 fijados en código, verificados antes de parsear | N + U | Baja: el upstream fijado no se audita (RR-11) |
| ORT dinámico o descargado | `download-binaries` y `load-dynamic` prohibidos en `deny.toml`; un único `libonnxruntime.a` exigido | U | Baja |
| Índice LanceDB envenenado | Derivado y en `memory://`; SQLite rehidrata y filtra cada candidato | N | Baja |
| Telemetría o red desde ORT | ORT configurado sin telemetría | N | Baja: el deny es calibración del gate, no enforcement del producto |

### B7 — Evidencia, receipts, artifacts y journals

| Entrada hostil / abuso | Control existente | Oráculo | Residual |
| --- | --- | --- | --- |
| Locator de artifact o Task usado como credencial | Binding uid + state root + root concedida; masking de IDs; objetos desconocidos a cuarentena | N | Media: mismo uid (RR-08) |
| Journal corrupto, de versión futura o de un binario más nuevo | Sniff de formato y `RecoveryRequired` antes de efectos; preflight de `doctor` | N | Media: journal corrupto bloquea el store (RR-09) |
| Regresión de reloj para extender TTL | Watermark durable | N | Baja |
| Eventos de auditoría con paths, diffs o credenciales | Registro de campos cerrados, sin paths | U | Baja |
| Credenciales de clientes versionadas en evidencia | `.gitignore`; `assert_no_credentials` en el arnés de tests | U | Media: detección por nombre de archivo, no por contenido (RR-07) |
| Rutas home locales en la exportación pública | Sustitución por `<LOCAL_HOME>` | U | Baja |

### B8 — Pipeline de publicación GitHub / OIDC

| Entrada hostil / abuso | Control existente | Oráculo | Residual |
| --- | --- | --- | --- |
| Release desde una rama o tag no estable | El workflow exige un tag `vX.Y.Z`/`vX.Y.Z-rc.N`; la versión se deriva del tag | H | Baja |
| Robo de clave de firma | No existe clave privada en el repositorio: solo OIDC, verificación con signer workflow exacto, permisos mínimos globales | H | Media: la attestation acredita el workflow, no la reproducibilidad (RR-12) |
| Acción de terceros comprometida | Acciones fijadas por SHA; Dependabot para `github-actions` | — | Media (RR-12) |
| PR de fork que exfiltra `SONAR_TOKEN` | Evento `pull_request` (no `pull_request_target`) y guard de fork; herramientas Python con hashes | — | Media: token de larga vida de un tercero (RR-12) |
| Merge sin revisión o force-push | `CODEOWNERS`; protección observada (`strict`, `enforce_admins`, sin force-push ni borrado) | H | Media (RR-12) |
| Smoke del archive con inventario erróneo | `release-smoke.py` fija los 36 tools y sus SHA-256 de schema; `tool_count` se deriva de `tests/baselines/contract-freeze-0.8.0.json`, no de un literal (issue cerrado en M8-07) | H | Baja |

**Conteo (§8 del threat model).** 8 fronteras (B1–B8); 53 controles
evaluados — 31 con oráculo nativo, 12 con oráculo unit/contract/protocol, 4
con evidencia histórica/de configuración y 6 sin oráculo (propiedad no
garantizada); 19 riesgos residuales (RR-01…RR-19).

**Temas obligatorios que el propio threat model resuelve explícitamente**
(dependencias comprometidas, secretos en source/evidencia, poisoning de
catálogo/modelo, escapes de containment, credenciales de publicación) están
detallados en la sección 4 de
[`docs/validation/M8/08-threat-model.md`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/validation/M8/08-threat-model.md)
y remiten a las mismas RR-05/07/08/10/11/12 de la tabla anterior — no se
duplican aquí para evitar una tercera copia del mismo contenido.
