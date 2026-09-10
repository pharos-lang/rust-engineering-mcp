# Compatibilidad

## Implementación verificada

| Componente | Foundation implementada |
| --- | --- |
| Release soportada | `0.1.0` |
| Checkout de desarrollo | `0.3.0-dev`; 31 tools: 18 M1/M2, cuatro M3 y cinco M4 calificadas localmente, más cuatro M5 implementadas y sin calificar; Tasks anunciado con negociación mutua; sin commit de integración, PR, tag ni publicación |
| Toolchain fijado / MSRV inicial | Rust y Cargo `1.98.1`, edition 2024 |
| Target de validación local | `aarch64-apple-darwin` |
| SDK | `rmcp =3.2.0`, features `server`, `transport-io`, sin defaults |
| Runtime / logging | Tokio `1.53.1`, tokio-util `0.7.19`, tracing `0.1.44`, tracing-subscriber `0.3.23` |
| Dominio | Serde `1.0.229`; sin dependencia del SDK, ADR-022 |
| CI portable | Linux x86_64, macOS ARM64 y Windows x86_64; fuente/protocolo/fail-closed, no capabilities positivas |
| Host positivo local M1–M4 | macOS 26 ARM64/APFS; ejecución de proyecto en guest Docker Linux ARM64 aprobado |
| Artifact 0.1.0 publicado | Un único archive core `aarch64-apple-darwin`; checksum, SBOM/notices y provenance verificados |
| Linux / Windows / macOS x86_64 nativos | CI pública compila y prueba el código fuente; la calificación nativa del sandbox y filesystem sigue pendiente para ampliar soporte en una release futura |
| Licencia / redistribución | Código original `MIT OR Apache-2.0`; assets `local` no se redistribuyen en 0.1.0 |
| Clientes de terceros | M4: Inspector 2.5.0 con Tasks y Codex 0.153.0 stock por sincronía; [recibo](validation/M4-clients.json). M5: Inspector 2.5.0 (quince filas, catorce Resources) y Claude Code 2.1.267 `claude-sonnet-5` como cliente agentic; [recibo](validation/M5-clients.json). M1/M2 conservan sus matrices anteriores. |
| Sandbox | Probes M0 separados; ejecución M1–M4 habilitada solo en runtimes aprobados Docker/Linux ARM64 calibrados por sus ADR |
| SQLite / FTS5 | rusqlite 0.40.2, SQLite bundled 3.53.2; memoria, pruebas ARM64 macOS |
| LanceDB / embeddings | M0-09: E5/ORT y LanceDB0.31 memory://; feature local, gate macOS ARM64 |

## Matriz wire de stdio (bootstrap histórico M0)

| Protocolo | Bootstrap probado | Resultado |
| --- | --- | --- |
| `2026-07-28` | `server/discover` con metadata por request | Versiones e identidad reales, capability tools; `resultType: complete` |
| `2026-07-28` | `tools/list` sin discovery previo | `rust.project.open`, sin cursor |
| `2025-11-25` | `initialize` / `notifications/initialized` | Versión preservada, `rust.project.open` |
| `2025-06-18` | `initialize` / `notifications/initialized` | Versión preservada, `rust.project.open` |
| `2025-03-26` | `initialize` / `notifications/initialized` | Versión preservada, `rust.project.open` |
| `2024-11-05` | `initialize` / `notifications/initialized` | Versión preservada, `rust.project.open` |
| Moderna/desconocida vía `initialize` | Handshake legacy | Fallback explícito del SDK a `2025-11-25` |
| Versión inline desconocida | Metadata completa | Error `-32022`; permite request válido posterior |

La matriz original acredita bootstrap y project.open. La evidencia M1-11 cubre
las once definiciones anteriores; [M1-12](validation/M1-12.md) valida el contrato
de doce tools. La release `0.1.0` anuncia trece con M1-13 implementado y gate
aprobado. El checkout de desarrollo anuncia 27 al sumar las
cinco tools M2, las cuatro tools M3 y las cinco definiciones M4. Las 18 primeras
conservan su calificación M2, las cuatro M3 pasan su gate Docker y las cinco M4
pasaron su calificación local. Tasks se anuncia tras la calificación M3-02; cada
uso requiere que el peer declare la extensión.
Esto no acredita conformidad completa de cada revisión MCP.

## Imagen y plugins M3

| Elemento | Identidad / versión | Estado |
| --- | --- | --- |
| Guest Linux ARM64 | `sha256:384a1742ecc53cdd3a9c0bf36c6f8b66db73ddd118aeeae6e55654ea998ae36a` | Provisionada; runtime M3 calificado |
| Configuración de imagen | `sha256:7d4e58b9e29b2045c13d71542f7892ee071a6886a1b939c4cbfc3ff7ce40dc45` | Verificada |
| cargo-nextest | 0.9.143 | Provisionado |
| cargo-llvm-cov / llvm-tools-preview | 0.9.0 / 1.98.1 | Provisionado |
| cargo-semver-checks | 0.50.0 | Provisionado |
| cargo-mutants | 27.1.0, source-built | Provisionado |

Provisioning pasó 47/47 observaciones. Esa provisión por sí sola no calificó las
tools M3; sus gates posteriores aportan la evidencia de comportamiento. Tampoco
cambió las cinco versiones de protocolo.
La [guía de clientes](client-configuration.md) documenta Codex, Claude Code, Gemini
CLI, Cursor, VS Code y MCP Inspector. Inspector 2.5.0 y Codex 0.153.0 conservan
evidencia M1. En M2, Inspector verificó 18 tools/open/denegaciones y Claude Code
2.1.260 con Sonnet 5 medium completó cinco preview/commit y receipt en el intento 5,
con la regla de renovar referencias explícita en prompt v2. Los intentos 1–4
fallidos se conservan; no acredita fiabilidad general ni éxito solo por descriptions.
Una configuración documentada no equivale a calificación. Las cinco versiones de
protocolo permanecen sin cambios. Las tools 20 (`rust.coverage`), 21
(`rust.semver.check`) y 22 (`rust.mutation.test`) están implementadas y
calificadas en M3.
Los dos turnos históricos fallidos se conservan; el flujo candidato final está
registrado por separado.
Las versiones se declaran explícitamente; no se anuncia una nueva versión por
actualizar el SDK sin ampliar las pruebas.

## Compatibilidad M4 calificada localmente

La imagen Linux ARM64 M4 admitida por identidad es
`sha256:25ed3626e710081a571a86a29521eaf2e890e796afd422ba5e409e0ce1891635`.
Contiene cargo-deny 0.19.7, nightly `2026-09-07` con su sysroot Miri y el scanner
sintáctico aislado. Scanner y Miri exigen esa imagen; deny conserva compatibilidad
con la imagen M4 anterior
`sha256:95dddeb5305f10b09b441e3cc4018ebb1a8a296d365c65106327d59f933c64e7`.
La configuración unificada de las 27 tools usa la identidad final `25ed`; la
compatibilidad aislada de deny con `95dd` no sustituye esa configuración. Esta
admisión no sustituye el runtime M3 para las 22 tools anteriores ni amplía la
calificación más allá de macOS ARM64/APFS con guest Docker Linux ARM64.

`rust.deny`, `rust.unsafe.scan`, `rust.supply_chain.inspect`,
`rust.quality.gate.v2` y `rust.miri` aparecen en `tools/list`. El
[core](validation/M4-core-gate.json), el [full](validation/M4-full-gate.json), el
[runtime](validation/M4-runtime.json) y los [clientes](validation/M4-clients.json)
pasaron localmente. No forman parte de una release y el hito no se declara Done
antes de la confirmación final de evidencia.

| Tool | Timeout por defecto | Máximo | Síncrono | Requisitos de host adicionales |
| --- | ---: | ---: | --- | --- |
| `rust.deny` | 120 s | 120 s | Hasta 60 s | Imagen M4 final, vendor Cargo, policy, RustSec y store durable |
| `rust.unsafe.scan` | 120 s | 120 s | Hasta 60 s | Imagen M4 final, vendor Cargo y store durable |
| `rust.supply_chain.inspect` | 120 s | 120 s | Hasta 60 s | Imagen M4 final, vendor/policy/RustSec para evidencia completa, catálogo local y store durable |
| `rust.quality.gate.v2` | 300 s | 3600 s | Hasta 60 s, solo `strict` sin mutation | Imagen M4 final y store durable; vendor, policy y RustSec completan sus etapas; `release` y mutation requieren Tasks |
| `rust.miri` | 300 s | 1800 s | Hasta 60 s | Imagen M4 final, vendor Cargo y store durable |

Con Tasks negociado, `auto|task` materializa el job. Sin Tasks, `auto` solo usa
sincronía cuando el timeout explícito es como máximo 60 s; con los defaults largos
devuelve `TASKS_REQUIRED`. `task` sin negociación y `synchronous` por encima del
límite devuelven `-32602`. Gate v2 `release` y cualquier selección mutation no
tienen fallback síncrono.

La configuración completa usa los pares `--cargo-vendor-dir` /
`--cargo-vendor-tree-sha256`, `--security-policy` /
`--security-policy-sha256` y `--rustsec-snapshot` / `--rustsec-sha256`. Supply
chain completa sus hechos de catálogo solo con `--catalog-store` y
`--catalog-trust`. `security-runtime inventory --json` informa pasivamente la
imagen, cargo-deny, helper, nightly y sysroot compilados; no inspecciona ni instala
componentes.

La disponibilidad parcial de inputs en supply chain conserva facts ya acreditados
y marca las fuentes ausentes; no se interpreta como pass. En las otras rutas, un
runtime equivocado, vendor/policy/RustSec ausente, evidencia truncada, timeout o
cleanup incierto bloquea la publicación de un resultado limpio. Ninguno de estos
contratos descarga toolchains, plugins, advisories, vendor o catálogos durante
`serve --stdio`.

Los límites semánticos también forman parte de la compatibilidad: unsafe scan no
expande macros, evalúa `cfg` ni inspecciona código generado; Miri rechaza proc
macros, build scripts y harnesses personalizados, y una pasada limpia solo cubre
las pruebas seleccionadas. Supply chain entrega facts y freshness, no un score o
certificación. Gate v2 conserva `rust.quality.gate` y sus contratos M1 sin cambios.
Los HTML de coverage y diffs de mutation privados pueden retener source autorizado;
esta matriz no acredita redacción universal de secretos escritos en el proyecto.
[Decisiones M4](adr/ADR-067-security-policy-and-quality-contracts.md) y
[runtime admitido](adr/ADR-068-m4-runtime-admission.md).

## Compatibilidad de MCP Tasks en M3-02

El camino Tasks está implementado sobre rmcp 3.2.0 para las cinco versiones
negociadas. La matriz de producto prueba ambos lados del switch: sin anuncio,
`tasks/*` es `-32601`; anunciado sin declaración del peer, rmcp devuelve `-32021`
con `requiredCapabilities`; con declaración mutua, el task es observable. En
`2026-07-28` la declaración viaja en `_meta`; las cuatro versiones legacy la
conservan en el handshake de sesión.

El switch de producción está encendido después de G4. Inspector 2.5.0 declaró la
extensión y completó create/poll/cancel; Codex CLI/app-server 0.153.0 no la declaró
y pasó discovery, llamadas y Resources por el camino síncrono soportado. Para todo
cliente que no declare Tasks, la compatibilidad admitida sigue siendo el modo
síncrono calificado y `TASKS_REQUIRED` para jobs largos; el nombre del cliente
nunca habilita autoridad ni una excepción de protocolo. [Matriz](validation/M3-02.md).

El cliente moderno envía `params._meta` con
`io.modelcontextprotocol/protocolVersion: "2026-07-28"` y
`io.modelcontextprotocol/clientCapabilities: {}` en cada request. `clientInfo` es
opcional. Una clave requerida ausente o de tipo inválido produce `-32602`;
si ocurre en el primer request cierra con exit 1, y tras bootstrap es recuperable.
Discovery incluye identidad en `result._meta["io.modelcontextprotocol/serverInfo"]`.
Los resultados legacy no requieren `resultType` y el SDK lo omite.

## Framing y cierre

`serve --stdio` con opciones host `--root`/`--project-ttl-secs` inicia el servidor. UTF-8/JSON por líneas LF o CRLF,
con 1 MiB máximo antes de LF; CR cuenta. EOF limpio antes o después de bootstrap
termina con exit 0. Exceso de bytes, línea incompleta al EOF, fallo de I/O o
bootstrap rechazado terminan con exit 1 y diagnóstico fijo en stderr.

Se conserva el comportamiento de rmcp 3.2.0: sintaxis JSON inválida se ignora;
una forma de mensaje inválida produce `-32600` sin ID, y el siguiente frame válido
puede procesarse. No se promete `-32700`. Las tools y métodos desconocidos devuelven `-32601`.

Las notificaciones de cancelación para IDs desconocidos no responden ni alteran
la sesión. project.open aplica checkpoints y un deadline cooperativo. rmcp usa
cancelación cooperativa y ejecuta el primer request inline; project.inspect rechaza ese primer job costoso hasta completar discovery. ADR-030 añade un worker sin cola,16 peticiones admitidas,16 notificaciones y16
send futures; frames de salida1MiB y deadlines totales10s para frames parciales y
escrituras. Idle entre frames completos no expira. Una petición cancelada cuya
respuesta rmcp suprime conserva su slot hasta cerrar la sesión: tras16 de esas
cancelaciones, otra petición provoca cierre y exige reconectar/reabrir proyectos.
IDs duplicados mientras están pendientes y sobrecarga cierran la sesión. No se
recicla un slot por el solo hecho de recibir cancelación. Shutdown espera hasta12s sin runtime Rust,240s con configuración explícita;
exceder ese plazo es fallo, no evidencia de cleanup. run_joined espera el cierre
real del gateway aunque rmcp suprima o abandone la respuesta. EOF y errores de
transporte cancelan el worker; la sesión solo termina limpia sin panic/cuarentena. Ver
[ADR-023](adr/ADR-023-mcp-stdio-bootstrap.md) y [evidencia](validation/M0-03.md).

## Acceso a proyectos

macOS 26+ / APFS: adapter no-follow/BENEATH habilitado tras probe. Host validado:
aarch64-apple-darwin, macOS 26.6.2, kernel 25.6.0. Otros OS/FS: no se habilita
acceso. En macOS, roots inválidas, FS no soportado o probe fallido abortan
el arranque con exit 1; en otros OS la tool responde unavailable sin I/O.
Las pruebas de junctions en Windows quedan pendientes de un adapter propio.
CLI sin roots conserva deny-by-default. Límites y subconjunto de Cargo:
[ADR-024](adr/ADR-024-project-open.md). Dependencias fijadas: rustix 1.1.4 (solo macOS),
toml 0.9.12, semver 1.0.28, sha2 0.11.0, getrandom 0.4.3, schemars 1.2.2 y
jsonschema 0.53.0 sin resolvers HTTP/file. El runtime no descarga schemas.

## Aprovisionamiento

El toolchain exacto debe estar instalado para build offline; rustup puede intentar
provisionarlo si falta. También se requiere cache de las dependencias de Cargo.lock.
La provisión es una operación explícita de desarrollo, no del runtime MCP.
El modo serve --stdio no proporciona aislamiento OS de red por sí mismo.
La CLI capabilities delega probes activos al gateway Docker explícito del host.

API contrastada con el [SDK fijado](https://docs.rs/rmcp/3.2.0/rmcp/),
[versioning oficial](https://modelcontextprotocol.io/specification/2026-07-28/basic/versioning)
y [stdio oficial](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/stdio).

## Gateway y capabilities M0-05/06

Únicamente probes Go confiables en Docker/Linux ARM64, sin Cargo ni mounts del host.
La CLI capabilities exige binario/socket/state-root/imagen explícitos y calibra su
configuración actual. `strict_available`/`restricted_available` solo cubren
`trusted_probe_image_only`; `project_code_available=false`. El camino Rust aprobado
de ADR-031 tiene transferencia y calibración separadas (ver más abajo). La frontera, controles
positivos y límites están en [security model](security-model.md#gateway-m0-05),
[ADR-025](adr/ADR-025-container-execution-gateway.md) y [evidencia M0-06](validation/M0-06.md).

## Contratos M0-07

M0-07 centraliza validación de contratos en `stdio::contract`: inputs y outputs
con schemas cerrados, validación Serde adicional y errores fijos sin payloads.
El snapshot de `rust.project.open` y las cinco versiones MCP se conservan.

## Semantic foundation

M0-09 fija fastembed6.0.2, ort2.0.0-rc.13/API24, ORT estático1.24.2 local y
lancedb0.31.0/Lance8.0.0, con el cambio exclusivo de manifests ADR-027. La feature
`local` es explícita y obligatoria para el gate semántico y futura distribución M1.
El gate nativo actual cubre macOS ARM64; Linux/Windows nativos siguen sin evidencia.
`tinyvec`1.12.0 evita el fallo alloc observado en1.13.0. `paste`1.0.15 tiene una
advertencia de mantenimiento RustSec2024-0436 registrada, no un ignore de seguridad.

ArtifactStore M0 es process-local memory-only, sin claims de persistencia entre
reinicios. Su API no depende de filesystem; evidencia de ejecución actual macOS.
Resource MCP y autorización viva de ProjectRef se integrarán en M1 (ADR-028).

La matriz CI inicial y límites de certificación están en [CI local](ci.md). Core
solo no cierra M0 ni califica M1; full requiere Docker real y semántica local.

El primer project.open moderno conserva la ventana de bootstrap M0: rmcp aún no
recibe notificaciones de cancelación mientras ejecuta esa primera validación,
hasta su retorno o deadline cooperativo10s. Los jobs costosos y Resources se rechazan durante bootstrap; ADR-030/034
conectan readiness y workers compartidos. El SDK3.2.0 no admite batches JSON-RPC:
las cinco modalidades negociadas los rechazan con Invalid Request y la sesión
puede continuar. No se afirma conformidad completa legacy. En upgrades del SDK,
revisar todos los métodos de Service delegados y el flush por mensaje de
AsyncRwTransport/SinkExt::send. Los deadlines son por fase; no un deadline único
para todo el teardown. Un timeout de escritura puede dejar un frame incompleto
antes del cierre de la conexión; no se promete una respuesta RPC en ese caso.

## Calibración Rust ADR-031

Runtime Linux/aarch641.98.1 aprobado, Docker29.7.2/runc1.3.6/cgroupsv2 observado
desde macOS26.6.2 ARM64. Se prueba el camino de fuente en volumen administrado,
build.rs y proc macro reales, denegación de sockets, límites efectivos y cleanup
de descendientes con setsid/doble fork. El perfil permite IPC privado SEQPACKET,
con socket/bind/connect/listen denegados. Esta evidencia no acredita Linux/Windows
nativo, x86_64 ni otra imagen/configuración. [Recibo](validation/M1-01-rust-gateway.md).

M1-02 publica toolchain del guest aprobado, incluyendo installed_targets. La
selección del proyecto solo admite1.98.1; no instala rustup ni componentes. Los
registros verbose/manifiesto se validan contra la imagen exacta. Otros runtimes
requieren nueva aprobación/calibración; no se extrapola a toolchains del host.

M1-03 anuncia Resources sin listado/subscripción. rmcp3.2.0 aplica SEP-2164:
Resource not found conserva -32002 en las cuatro versiones legacy y se normaliza
a -32602 en2026-07-28. Mensaje fijo y ausencia de data permanecen iguales para URI
inválida, owner distinto y referencia expirada. No se reimplementa este mapping.

M1-04 adds the fifth current tool, `rust.fmt.check`, on the same five negotiated
MCP versions. Its real-version oracle is approved Linux ARM64 rustfmt1.9.0 with
Rust/Cargo1.98.1. This does not qualify native Linux/Windows/x86_64 or third-party
clients. Stable configured formatting with skip attributes is the declared scope.

M1-05 añade rust.clippy con cuatro perfiles cerrados en las cinco versiones wire.
Clippy0.1.98 está en la imagen aprobada1.98.1; sus fixtures build.rs/proc macro
se verifican mediante el gateway real, sin ampliar plataformas calificadas.

M1-06 adds rust.test in the same five wire versions. Stable Cargo JSON covers
compilation only; bounded human harness output is retained without fabricated
counts. Custom harnesses rejecting fixed test-threads/color arguments can fail.
Only approved Linux ARM64 execution is qualified; no native/third-party expansion.

M1-07 adds audit in five wire versions. Lock support is a strict v4 subset with
unambiguous complete source identity, reachable workspace graph and bounded paths.
It reports captured-state facts, not active-feature resolution. Physical snapshot
paths require the same macOS26+/APFS capabilities; no new native platform claims.

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
source edits, global catalog import or new platform support. At the M1-09 baseline, M1-10..17 remained pending; current M1-10 evidence is below.

## M1-10 — CLI y persistencia en desarrollo

Report CLI format_version1 es independiente de MCP: ese corte conservó diez tools
y las cinco modalidades wire. Bundle v1/schema1, Ed25519 ring0.17.14, zstd0.13.3 y
HTTPS reqwest0.12.28 están fijados; no hay endpoint/publisher por defecto. El store
y lectores de trust/model/bundle requieren macOS26+/APFS y paths protegidos.
Linux/Windows/x86_64 nativos siguen sin calificación; input exFAT tampoco se admite.

Feature local restaura objetos nativos Lance8 con E5/ORT verificados y metadata
ligada al catálogo/modelo; build core no acredita ese camino. Report semantic
availability solo es true tras validación nativa. Bundle hash identifica el
contenedor exacto, no una serialización canónica del archivo comprimido.
[Formato/flags](catalog-bundle-format.md), incluido floor independiente y requisitos
0600/0700. [M1-10](validation/M1-10.md) distingue full15/15 y la fuente anterior
al ajuste final del CLI de los gates core540/Clippy all-features/CLI nativo5+1
posteriores. En ese corte seguían pendientes clientes reales, distribución y
release; M1-17 los calificó después sin cambiar este contrato de catálogo.

## M1-11 — Contrato de estado

`rust.catalog.status` añade la undécima definición sobre el SDK/modos existentes;
no añade versiones wire, plataformas ni clientes calificados. Usa input `{}`, schema
cerrado y el envelope estructurado habitual, con128KiB de presupuesto total y120s
cooperativos. Host flags de catálogo/trust forman un par; modelo es opcional e
índice externo requiere modelo. [Contrato](tools.md#rustcatalogstatus).

Core puede observar SQLite; semántica configurada requiere build `local` y assets
verificados. La generación se carga lazy y se conserva hasta reiniciar, mientras
RustSec de audit se relee por llamada. El I/O protegido mantiene macOS26+/APFS;
`runtime_api_disabled` no cambia la calificación de sandbox OS ni la matriz nativa.

## M1-12 — Búsqueda

La duodécima definición conserva las once anteriores y los modos SDK existentes;
no amplía versiones wire ni calificación nativa/clientes. `local` permite E5/Lance
verificados; core conserva lexical y fallback explícito. Status/search comparten
la misma generación y no observan imports nuevos hasta reiniciar sesión.

El límite de resultado de search es512KiB completo, distinto de128KiB de status;
ambos conservan deadline120s cooperativo joined. RRF/ventanas/filtros son contratos,
no resultados experimentales de calidad ES/EN ni de performance. Los gates de
release siguen pendientes. [Contrato](tools.md#rustcratesearch),
[evidencia M1-12](validation/M1-12.md).

## M1-13 — Inspección

La decimotercera definición conserva los doce contratos previos y la negociación
SDK existente. Core consulta SQLite sin modelo/índice; el gate local-feature no
implica que inspect ejecute embeddings. Comparte la generación de status/search;
importar otro snapshot requiere reiniciar para observarlo y continuar con su identidad.
[Gate M1-13 aprobado](validation/M1-13.md): core629/10 etapas, protocolo37,
Clippy all-features/all-targets y dos tests local-feature bajo OS network deny. No amplía calificación de clientes, plataformas, distribución ni release.
[Contrato](tools.md#rustcrateinspect), [evidencia M1-13](validation/M1-13.md).

## M1-14 — Contratos CLI de diagnóstico

Version conserva su salida humana y añade --json format_version1 con hechos de build.
Doctor introduce JSON format_version1 y rendering humano: passed/warning exit0,
failed exit1, sintaxis inválida exit2. Capabilities mantiene JSON por defecto y acepta
--json explícito o --human; sigue siendo una operación activa del probe image,
distinta de la calibración Rust de doctor --active. No cambian versiones MCP ni tools.

El gate doctor verificó calibración, SIGINT y cleanup en el runtime Linux ARM64
aprobado desde el host macOS existente. No acredita runners nativos Linux/Windows,
clientes MCP ni capacidades filesystem adicionales. Los adapters sin soporte fallan
cerrados; compilación del target, inventario del contenedor y evidencia nativa son
hechos diferentes. [ADR-045](adr/ADR-045-cli-doctor.md).

## M1-15 — Candidatos locales

Candidatos release macOS arm64 ejecutados desde instalación privada: core/local version y doctor activo. Firma ad hoc verificada localmente; no notarización ni evidencia de otros hosts. Véase [candidatos](release/offline-candidates.md).

La release final sustituye esos candidatos como canal soportado: `v0.1.0` publica
solo core macOS ARM64 y usa provenance OIDC, checksum y smoke sobre los bytes
descargados. Véase el [recibo público](validation/m1-17-public-release.json).

## Escritura local M2 en desarrollo

Las cinco tools M2 usan el mismo binario y el mismo grupo Docker/imagen de M1; no
añaden daemon, UID, servicio ni tool instalada. Exigen grants independientes y un
state root privado compartido por las instancias que escriban el workspace. La
calificación host positiva permanece limitada a macOS 26 ARM64/APFS. Linux,
Windows, macOS x86_64 y filesystems distintos no tienen adapter positivo para
project I/O, captura vendor o publicación.

Las operaciones con resolución requieren opcionalmente un directory source Cargo
preparado por el operador y configurado con path más fingerprint. El runtime no
descarga crates ni hereda Cargo host. Fmt/fix no requieren vendor; fix exige lock
existente y permite loopback únicamente dentro de su namespace Docker aislado,
manteniendo `network=none`. Esto no acredita aislamiento nativo Docker en Linux o
Windows ni convierte el soporte de compilación CI en soporte de escritura.

El modo `local_coordinated` presupone que el host mantiene estables roots/state y
evita escritores simultáneos durante commit. No ofrece CAS, exclusión OS de otros
programas ni atomicidad visible multiarchivo. `preserve_presence` mantiene la
presencia o ausencia inicial de Cargo.lock. La [calificación conjunta](validation/M2-07.md)
M2 está completada sobre los bytes que entonces se identificaban como `0.2.0-dev`;
el checkout actual es `0.3.0-dev` y la release soportada continúa siendo `0.1.0`
con 13 tools.

M2 ADR-059 conserva schemas y formato de journal: libera planes terminales y
permite commit replay exacto desde el journal con ID/digest/key y autoridad viva
incluso tras TTL/reinicio. No permite iniciar efectos nuevos sin preview vigente.

## Rendimiento M5 en desarrollo

Las cuatro definiciones M5 están implementadas, calificadas nativamente y por
clientes, y **sin Done**: la [matriz M5](validation/M5-matrix.md) mantiene M5-05
`In progress` con el gate `full` bloqueado por `lancedb 0.38.0`. `tools/list` devuelve 31 definiciones: las 27 anteriores
intactas byte a byte en sus snapshots y las cuatro nuevas. Los recibos históricos
no califican los contratos M5 finales; la matriz identifica la evidencia pendiente.
Las cinco versiones de protocolo no cambian. Nada de esta sección forma parte de la
release `0.1.0`.

### Artifact kinds, MIME y versiones de payload nuevos

El store privado de calidad (ADR-061) recibe cinco kinds nuevos con su MIME y su
versión de payload. **Ninguno aparece en el schema público de una tool anterior**:
los DTO M1–M4 declaran su propio enum cerrado por tool mediante
`#[schemars(with = …)]`, de modo que el schema publicado de `rust.test.nextest`,
`rust.coverage`, `rust.semver.check`, `rust.mutation.test` y las cinco M4 queda
byte a byte igual, bajo un test de invariancia sobre los 27 snapshots previos
([ADR-076](adr/ADR-076-m5-performance-contracts.md) §1/§2).

| Artifact kind | MIME | Versión de payload | Origen |
| --- | --- | --- | --- |
| `benchmark_dataset` | `application_json` | `benchmark_dataset_v2` | `rust.benchmark.run` |
| `criterion_archive` | `application_x_tar` | `ustar_v1` | `rust.benchmark.run` |
| `collapsed_stacks` | `text_plain` | `collapsed_stacks_v1` | `rust.profile.flamegraph` |
| `flamegraph_svg` | `image_svg_xml` | `flamegraph_svg_v1` | `rust.profile.flamegraph` |
| `bloat_json` | `application_json` | `bloat_json_v1` | `rust.binary.bloat` |

`image_svg_xml` es el único valor nuevo de `QualityMimeType`; `ustar_v1` ya
existía y se reutiliza sin cambio. `GuestArtifactName` y `PluginIdentity` reciben
las variantes correspondientes (`Criterion`, `ProfileHelper`, `Bloat`). Estos
enums son internos al store durable: son la identidad con la que el store valida
un descriptor, no una ampliación de un contrato ya publicado.
Los logs de benchmark reutilizan el artifact privado `tool_log`/`utf8-log.v1`;
no están dentro de `criterion_archive`. Se asocian al `run_index` y stream,
declaran sustitución UTF-8 separada de truncamiento y se publican tras ejecutar,
sujetos a la cuota del store.

### Formatos versionados con ciclo propio

Los bytes de los artifacts declaran su propio identificador, **independiente del
SemVer del servidor y del contrato de las tools** (G6):

| Identificador | Contenido |
| --- | --- |
| `rust-engineering-mcp.benchmark-dataset.v2` | Dataset de benchmark: identidad, muestras crudas con su `run_index`, `sampling_mode`, parámetros solicitados y provenance |
| `rust-engineering-mcp.benchmark-comparison.v2` | Método de comparación congelado: estadístico, remuestreos por conglomerados sobre ejecuciones, semilla, confianza, corrección por multiplicidad, umbral y política de outliers |
| `rust-engineering-mcp.collapsed-stacks.v1` | Stacks colapsados |
| `rust-engineering-mcp.flamegraph-svg.v1` | Flame graph renderizado por el producto |
| `rust-engineering-mcp.bloat-report.v1` | Reporte de tamaño y atribución |

**Regla de migración.** Un lector que no reconozca exactamente el identificador y
su `format_version` **falla cerrado**: no coerciona, no migra y no reinterpreta.
Migrar significa conservar las muestras crudas o repetir la ejecución; **nunca
transformar mediciones incompatibles en equivalentes**
([ADR-073](adr/ADR-073-benchmark-method-and-dataset.md) §3). La comparación
rechaza el par, enumerando todas las razones, si difieren formato, unidad,
harness, versión de harness, `rust_version`, `cargo_version`, digest de imagen,
plataforma, arquitectura, selección, cuotas o modelo de CPU, o si el modelo de
CPU es desconocido en cualquiera de los dos lados; un campo de hardware que el
runtime no puede observar se serializa ausente y bloquea la comparación en vez de
rellenarse con un valor plausible (ADR-073 §3/§5). Un par incompatible es un
resultado observado (`INCOMPATIBLE_DATASETS`), no un error de infraestructura.
El governor, cuando existe, describe únicamente un consenso de las CPUs visibles
del guest Linux. No identifica el host físico; sysfs ausente, exit no cero limpio,
conjunto incompleto o heterogeneidad se serializan como desconocidos. Timeout,
cancelación, output limit o truncamiento son errores operativos unidos, no
hardware desconocido. `cpu_model` también exige consenso de valores guest válidos.
Aun con consenso,
`METHOD_QUALIFIED_FOR_DIRECTION=false` mantiene cerrada toda dirección hasta la
recalificación estadística.

### Imagen guest M5

| Elemento | Identidad / versión | Estado |
| --- | --- | --- |
| Guest Linux ARM64 M5 | `sha256:e0a5ca1661b3e49d0a3d68ee3cc0963453078d08eb7fc43c30538c16b7998aac` (`rust-engineering-runtime:1.98.1-arm64-m5`) | Construida y con [recibo](validation/M5-provisioning.json); admitida por digest en el gateway ([ADR-077](adr/ADR-077-m5-runtime-admission.md)); seis selecciones nativas aprobadas ([gate nativo](validation/M5-native-gate.json)) |
| Base | `sha256:25ed3626e710081a571a86a29521eaf2e890e796afd422ba5e409e0ce1891635` | Imagen M4 aprobada, intacta y verificada por digest antes de construir |
| `cargo-bloat` | 0.12.1, MIT, en `/opt/perf/bin` | Provisionado, fuera del `PATH` del contenedor de trabajo |
| `rust-mcp-profile-helper` | Construido desde `fixtures/profile-helper` | Provisionado, fuera del `PATH` del contenedor de trabajo |
| Perfil seccomp de profiling | `seccomp-rust-profile.json` = perfil quality + `perf_event_open` | [Prueba de capability](validation/M5-profiling-capability-probe.json) pasada sobre la imagen M4 |

La imagen no cambia toolchain, plugins M3, binarios M4, usuario, `WORKDIR` ni
`PATH`, y el gateway invoca ambos binarios por ruta absoluta. La imagen M4
permanece aprobada mientras M5 no califique; revocar M5 es volver a apuntar el
gateway a ese digest, sin estado que migrar
([ADR-075](adr/ADR-075-m5-runtime-provisioning.md) §2/§4). M5 no sustituye ni
amplía las tres imágenes anteriores: las tools M1–M4 conservan su calificación
contra sus propios digests y un host configurado con la imagen M4 recibe
`unavailable` en las cuatro tools nuevas. El puerto de performance exige el
digest M5 y solo ese, porque las versiones del analizador y del helper que el
resultado declara son propiedades de esa identidad.

### Frontera de target

El positivo de análisis de tamaño está calibrado sobre **ELF64/AArch64** en el
guest Linux ARM64: la
[calibración](validation/M5-04-bloat-calibration.json) registra `ELF64`,
`AArch64` y `DYN (Position-Independent Executable file)`. Ese positivo **no
califica Mach-O ni PE**, y **WASM no está soportado por el analizador**. Solo se
calibraron los exits 0 (`passed`) y 1 (`analysis_failed`); los demás siguen sin
calibrar. `--profile release-lto` es inusable con Cargo 1.98.1 —el analizador
deriva `CARGO_PROFILE_RELEASE_LTO` a partir del nombre del perfil y Cargo lo
rechaza con `invalid type: Option value, expected a boolean or string`—, así que
LTO se expresa con `--release` más una variable de entorno propiedad del
producto. El positivo de profiling también se califica únicamente en el guest
Linux ARM64; cambiar el target exige D13 y un oráculo nativo nuevo
([ADR-074](adr/ADR-074-profiling-capability-and-containment.md) §6).
