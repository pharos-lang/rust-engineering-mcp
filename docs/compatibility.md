# Compatibilidad

## Implementación verificada

| Componente | Foundation implementada |
| --- | --- |
| Versión del paquete | `0.1.0-dev.1`; sin release |
| Toolchain fijado / MSRV inicial | Rust y Cargo `1.98.1`, edition 2024 |
| Target de validación local | `aarch64-apple-darwin` |
| SDK | `rmcp =3.2.0`, features `server`, `transport-io`, sin defaults |
| Runtime / logging | Tokio `1.53.1`, tokio-util `0.7.19`, tracing `0.1.44`, tracing-subscriber `0.3.23` |
| Dominio | Serde `1.0.229`; sin dependencia del SDK, ADR-022 |
| Linux / Windows / macOS x86_64 nativos | Sin ejecución validada; matriz CI inicial documentará esta limitación, gate requerido antes del RC M1 |
| Licencia / redistribución | Decisión pendiente; gate obligatorio antes de publicar M1 |
| Clientes de terceros | Pendientes; las pruebas actuales usan un harness independiente |
| Sandbox | Probes M0 separados; Cargo M1 habilitado solo en runtime aprobado Docker/Linux ARM64 calibrado ADR-031/032 |
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
de doce tools. El checkout anuncia trece con M1-13 implementado y gate aprobado.
Esto no acredita conformidad completa de cada revisión MCP.
No se han probado clientes Codex/Claude/otros.
Las versiones se declaran explícitamente; no se anuncia una nueva versión por
actualizar el SDK sin ampliar las pruebas.

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
posteriores. Siguen pendientes clientes reales, distribución y release.

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
