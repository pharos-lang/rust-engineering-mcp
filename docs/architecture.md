# Arquitectura

## Entrada del binario

El workspace contiene el paquete binario `crates/mcp-server`.
`main.rs` valida un comando cerrado, escribe ayuda/versión o un diagnóstico fijo y
devuelve el código de salida. `serve --stdio` entra en el adapter MCP;
el arranque no ejecuta proyectos. Los workers admitidos usan la frontera única
de procesos del Execution Gateway Rust configurado explícitamente por el host.

Los tests en `crates/mcp-server/tests/cli.rs` lanzan el binario construido por Cargo
con entorno vacío. Esa API de procesos pertenece exclusivamente al harness de
pruebas; es distinta del Execution Gateway del producto. Ver [ADR-021](adr/ADR-021-minimal-bootstrap.md).

## Dominio M0-02

`crates/domain` contiene referencias/fingerprints, diagnósticos, resultados y
evaluación de freshness. Solo depende de Serde; serde_json es dependencia de
pruebas. No importa MCP/JSON-RPC, stdio, procesos Cargo, SQLite ni LanceDB.
Los [contratos](domain-contracts.md) se prueban mediante su API pública y
serialización/deserialización. `Clock` es un port real consumido por el evaluador;
el dominio no consulta el reloj del host.

## Aplicación y adapter de proyectos M0-04

`crates/application` depende solo de domain. `ProjectRegistry` consume ports reales
para validar/revalidar proyectos, generar referencias, medir TTL y comprobar
cancelación. Conserva leases opacos, sin conocer handles OS, Cargo ni MCP.
`crates/project-adapter` aporta I/O protegido, parser TOML/semver acotado, SHA-256,
entropía OS y reloj monotónico. El adapter macOS usa rustix; otros targets rechazan
la operación antes de I/O. Las fronteras siguen ADR-004 y ADR-024.
ADR-048 califica como host positivo 0.1.0 únicamente macOS26 ARM64/APFS; Linux y
Windows prueban portabilidad y rechazo fail-closed, no un adapter positivo.

`stdio/project.rs` traduce DTOs Serde/schemars al caso de uso, aplica JSON Schema
a input/output y limita a un worker sin cola. El worker posee el permiso hasta
terminar realmente. La aplicación no conoce Tokio ni el SDK. No se crean ports
para los detalles internos del protocolo ni se usa Value como modelo de dominio.

## Adapter MCP M0-03

`stdio.rs` configura un runtime Tokio, un ServerHandler de rmcp y tracing hacia
stderr. El SDK controla discovery, negociación, parsing JSON-RPC, dispatch y
notificaciones. `stdio/budget.rs` envuelve AsyncRead/AsyncWrite para limitar bytes
por línea y señalar fallos mediante el token de cancelación del SDK. No interpreta
JSON ni crea respuestas de protocolo. [ADR-023](adr/ADR-023-mcp-stdio-bootstrap.md)
documenta límites y compatibilidad.

Los tests `protocol.rs` lanzan el binario y usan fixtures JSON independientes del
SDK, con plazos y lectores acotados. No se crea un port de dominio para stdio:
es una frontera externa al dominio. La release `0.1.0` incorpora trece tools:
status tiene [evidencia M1-11](validation/M1-11.md), search tiene
[gate M1-12 aprobado](validation/M1-12.md) e inspect está conectado con
[gate M1-13 aprobado](validation/M1-13.md). El checkout añade cinco handlers M2
calificados localmente, descritos al final.

SQLite es autoritativo mediante el port CatalogRepository y snapshots en memoria
(ADR-026); LanceDB derivado y E5 local están implementados en M0-09.

El [tablero](implementation-status.md) identifica la evidencia de cada corte. Un
componente descrito como futuro no implica una capability disponible.

## Execution Gateway M0-05

Domain define escenarios, límites, resultado e identidad de ejecución. Application
define ExecutionPort, cancelación y admisión de tiers. El adapter Docker implementa
el port con argv cerrado, env_clear, cwd privado, captura concurrente acotada y
contenedores sin mounts del host. Solo supervisor.rs llama a Command::spawn.
No hay dependencia Docker, filesystem o std::process en domain/application.
La vertical es la ejecución de fixtures reales, sin habilitar nuevas tools MCP;
ver ADR-025 y validation/M0-05.md. M0-06 demuestra cada capability con oráculos activos y controles positivos.

## Calibración M0-06

El CLI capabilities recibe solo configuración explícita del host. DockerGateway
coordina probes cerrados y normaliza eventos JSON tipados; SandboxEvidence y la
admisión viven en domain/application sin Docker/stdio. El reporte registra los
resultados de cada ejecución y su fingerprint, además de identidad de configuración
y hora de observación. No hay cache de autorización. Los perfiles de control son
privados y restringidos a Network/Filesystem; ExecutionPort siempre usa el perfil
enforced. No hay ruta desde tools MCP a perfiles de control. La ejecución de
proyectos M1 usa el gateway Rust y su calibración independiente, no los probes M0.

M0-07 centraliza validación de contratos en `stdio::contract`: inputs y outputs
con schemas cerrados, validación Serde adicional y errores fijos sin payloads.
El snapshot de `rust.project.open` y las cinco versiones MCP se conservan.

`search_catalog` (application) consulta `CatalogRepository`, obtiene facts tipados
y reevalúa SnapshotEvidence con Clock. SqliteCatalogRepository implementa el port
en un crate adapter; no se agregan SQLite, procesos ni MCP a domain/application.
Activación exclusiva por &mut reemplaza metadata y conexión a la vez, sin lectores
de generaciones retenidas. La API síncrona requiere worker acotado al conectarla
al runtime MCP en M1. Véase ADR-026 y validation/M0-08.md.

## Semantic foundation M0-09

Application define `EmbeddingProvider` y `SemanticIndex`; domain contiene identidad
y evidencia. El adapter carga exclusivamente bytes del E5 fijado, verifica cinco
hashes antes de parsing y configura ORT explícitamente sin telemetry. Un índice
LanceDB memory:// nuevo por generación guarda IDs/vectores; todos los facts se
rehidratan desde SQLite. Rebuild reemplaza metadata y tabla solo tras éxito. La
mezcla conserva resultados léxicos primero y agrega candidatos únicos; no es aún
el ranking de M1-12. El worker de búsqueda MCP de M1-12 y la CLI/persistencia de M1-10
se describen debajo. Ver ADR-027, ADR-041 y ADR-043.
Esta arquitectura `local` permanece dentro de M1 y se califica desde fuente; E5,
ORT, LanceDB y sus datos no forman parte del archive core 0.1.0.

## Artifacts efímeros

Domain define IDs/metadata y application el puerto ArtifactStore/ArtifactInput.
El adapter en memoria consume streaming acotado y devuelve vistas prestadas; la
publicación de cada draft es atómica respecto a error, con hash de contenido
redactado. No hay I/O propio ni persistencia de artifacts. M1-03 conecta logs de check con Resources y autorización de ProjectRef vivo;
los diffs y persistencia siguen fuera de este corte.

## Workers y transporte M1-01

`stdio::workers` centraliza el único worker con admisión sin cola; project.open
y project.inspect lo consumen. La vida del permiso sigue al closure bloqueante y no al future del
caller. Request/session cancellation y deadline son controles cooperativos.
`stdio::admission` envuelve Transport/Service de rmcp con leases; el SDK conserva
parsing, negociación y routing. La retención conservadora de respuestas canceladas
limita el backlog sin depender de callbacks inexistentes en rmcp3.2.0. `budget`
impone límites de bytes/deadlines. Véase ADR-030 para costes y límites operativos.

## Camino Rust M1-01

`ProjectRegistry::source` obtiene un SourceBundle propio a través del port de
source; SecureProjects lo captura relativo a sus handles originales. El adapter
de ejecución codifica USTAR, ingiere bytes por stdin acotado del supervisor y
verifica/elimina el ingester antes de Cargo. El código del proyecto recibe source
read-only y tmpfs acotados; no hay bind de paths del host. `RustCommand` define
operaciones cerradas en domain. El gateway requiere calibración activa de esta
configuración; su reporte con tiempo/fingerprints es evidencia histórica, no una
credencial importable. Ver ADR-031 y validation/M1-01-rust-gateway.md. ProjectRegistry::inspect compone captura, port de inspección y revalidación final.
RustProjectInspector inicializa/calibra de forma lazy con política del host; su parser
puro devuelve hechos tipados, no JSON al dominio. MCP publica envelope con snapshot
y schemas cerrados. run_joined conserva cleanup/error y recibe cancelación por
request, EOF o fallo de I/O. Véase ADR-032.

M1-02 comparte RustProjectInspector entre ambas tools; with_gateway conserva
mutex, calibración fallida latched y cuarentena. ToolchainInspectionPort devuelve
inventario tipado; cada observación retiene tres fingerprints. El parser es puro y
el nuevo comando InstalledComponents tiene programa/path fijos. Véase ADR-033.

M1-03: ProjectCheckPort retorna CheckObservation tipado; application compone captura,
evidencia, ArtifactStore y autorización final. Parser Cargo puro en execution; SDK/
schemas/Resource URI solo en MCP. Logs se retienen bajo un único ArtifactClock
clonable; todos los locks registry→store y gateway ocurren en el worker compartido.
Domain/application siguen sin Cargo CLI, rmcp ni bases de datos.

M1-04 adds ProjectFormatPort and a typed formatting report. Application validation
capture/publication is shared narrowly with check; final ProjectRef authorization,
TTL, quota fallback and logs remain identical. Cargo-fmt parsing stays in execution
adapter, while rmcp and display budgets stay in MCP. See ADR-035.

M1-05 adds distinct ProjectClippyPort/ClippyOptions while reusing captured-validation
publication. A private execution-adapter Cargo normalization function serves check
and Clippy. Domain/application still contain no Cargo CLI, database or rmcp API.

M1-06 adds ProjectTestPort, validated TestOptions and TestObservation with nullable
reported build phase. Shared capture/publication remains project-bound; execution
adapter handles the Cargo/harness boundary, MCP uses the same joined single worker.
No Cargo CLI/rmcp/database types enter domain/application.

M1-07 combines ProjectInspectionPort and DependencyAuditPort over one SourceBundle.
A separate bounded SQLite RustSec database holds canonical advisory facts; official
matching consumes those selected rows. The M0 catalog schema is unchanged. Domain
and application have no RustSec/SQL/Cargo/rmcp dependency. Host I/O, checksum and
transport are adapters; this is not the future authenticated catalog importer.

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

## Catálogo administrado M1-10

`catalog_cli` compone adquisición explícita del host, `bundle` en catalog-adapter,
`catalog_store` en project-adapter y `catalog_semantic`/semantic-adapter. El verifier
consume bytes propios, autentica manifest/payloads y abre SQLite; el store solo usa
nombres fijos relativos a handles APFS. El floor independiente se reserva antes de
la generación activa, bajo la misma lease exclusiva. No se añaden dependencias
SQLite/rmcp/filesystem al dominio/aplicación ni tools públicas.

La CLI HTTPS usa un adapter separado de red con allowlist explícita; ningún handler
MCP lo llama. El índice exporta objetos Lance8 reales y restaura en un registry
con proveedor memory solamente, validando modelo/catálogo/esquema/filas antes de
aceptarlo. SQLite conserva los facts. Ese corte ADR-027/041 no añadió tools;
M1-11 conecta solo su estado al runtime. [Formato y límites](catalog-bundle-format.md).

## Contexto runtime M1-11

`CatalogStatusPort` devuelve observaciones tipadas; aplicación valida parentesco
catálogo/modelo/índice y evalúa freshness con Clock. `stdio::catalog` conserva schemas
y envelope fuera del dominio, con input vacío y presupuesto128KiB del resultado MCP
completo. [Gate/revisión M1-11](validation/M1-11.md).

El provider carga lazy tras bootstrap en el worker bloqueante joined compartido.
Retiene una generación inmutable y sus handles SQLite/modelo/Lance, incluida una
primera observación indisponible; no vuelve a cargar hasta reiniciar sesión. El
lector propio read-only observa floor/active/floor con retry acotado y comparte el
validador de floor con CLI; no usa la lease de administración. RustSec se observa por
separado desde la fuente de audit en cada llamada. [ADR-042](adr/ADR-042-catalog-runtime-status.md).

## Recuperación M1-12

`CatalogSearchRepository` separa candidatos léxicos con score de facts elegibles
seleccionados por SQLite/semver. Aplicación verifica canales, conserva sus ranks y
fusiona hybrid con RRF60; dominio sigue limitado a tipos/comparaciones puros.
`stdio::crate_search` adapta el contrato y recorta solo el sufijo de resultados.

Status y search comparten la misma instancia `CatalogProvider`, generación lazy y
admisión joined; no se introduce cache de catálogo ni permiso de ejecución paralelo.
El worker mantiene inferencia, consultas, validación JSON y encoding hasta terminar.
[ADR-043](adr/ADR-043-catalog-search-modes.md); [gate aprobado](validation/M1-12.md).

## Inspección autoritativa M1-13

El port de inspección paginada consulta escalares y colecciones acotadas en SQLite,
sin hidratar el grafo completo de versiones. Comparte CatalogProvider y generación
inmutable con status/search; no depende del índice/modelo para obtener facts.
Domain representa lookup, página, counts y metadatos no registrados como tipos;
el adapter MCP proyecta schema y codifica dentro del worker joined existente.
La continuación repite parámetros explícitos y fingerprint, sin estado de cursor.
[ADR-044](adr/ADR-044-paged-crate-inspection.md), [gate aprobado](validation/M1-13.md).

## M1-14 — Composición de doctor

El parser host_config es compartido por serve y doctor; las opciones siguen siendo
política explícita del host. Doctor compone CatalogProvider + catalog_context y
SecureProjects para observaciones locales, sin llamar al administrador CatalogStore.
Solo --active construye RustProjectInspector y usa ToolchainInspectionPort con un
SourceBundle fijo en memoria. Reutiliza calibración, comandos tipados y gateway;
no introduce ports, ejecutores o dependencias de protocolo en domain/application.

Doctor_run registra señales antes del trabajo, posee el control y espera el worker
bloqueante hasta cleanup. El mismo Report tipado alimenta JSON/humano y códigos de
salida; version informa build facts y capabilities añade rendering humano conservando
su contrato JSON. No cambia las trece tools MCP. [ADR-045](adr/ADR-045-cli-doctor.md).

## Frontera de distribución 0.1.0

ADR-048 no cambia la arquitectura ni el contrato. Se distribuye un único archive
core `aarch64-apple-darwin` que conserva discovery, las trece definiciones y caminos
SQLite lexical/degradados. No contiene modelo, ORT, LanceDB, catálogo, trust,
fixtures, Docker ni toolchain. El cierre arquitectónico es compuesto: artifact
core verificado más full gate source-bound del perfil `local` en el host positivo,
con ejecución de proyecto dentro del guest Docker Linux ARM64 aprobado.

No existe catálogo oficial 0.1.0 ni clave Ed25519 de producción. Import, firmas,
antirollback, cambio de trust por el host y revocación por retirada de trust siguen
implementados para catálogos aportados explícitamente por el host.

## Escritura local M2 en desarrollo

El adapter MCP expone cinco handlers tipados sobre un mismo caso de uso de mutación:
manifest patch, fmt, fix y dependency add/remove. Domain liga candidato, clase de
operación, fingerprints, validación y receipts; application conserva los planes
compartidos (cuatro/64 MiB), verifica autoridad antes de capturar y antes de
publicar, y mantiene resolución/editor/inspector tras ports. Los DTOs rmcp, JSON
Schema y presupuestos del envelope permanecen en el adapter MCP.

Los productores de candidatos usan el Execution Gateway Docker ya existente.
Source, staging y el directory source Cargo aprobado viajan como archivos propios;
no hay bind writable del workspace ni ejecución de Cargo host. Fmt y fix exportan
solo reemplazos `.rs` existentes. Fix selecciona un perfil seccomp dedicado con
TCP loopback interno y `network=none`; los demás perfiles no reciben esa capacidad.
La resolución ejecuta metadata offline sobre staging escribible y metadata frozen
sobre el resultado, con vendor read-only y configuración source replacement fija.

El publisher nativo aplica los bytes exactos mediante handles no-follow, locks y
journal. `local_coordinated` detecta cambios observados, pero no ofrece exclusión
OS ante escritores externos, CAS ni una transacción visible multiarchivo. La policy
`preserve_presence` incluye el lock raíz actualizado si existía y elimina del
candidato un lock creado solo para validar. Esta arquitectura está integrada en el
checkout `0.2.0-dev` de 18 tools, con [calificación M2](validation/M2-07.md); la release
`0.1.0` conserva 13.

M2 usa [eventos locales de terminación](adr/ADR-058-local-mutation-observability.md)
por tracing/stderr. La retención de planes se consulta sin modificarla; la CLI
conserva la autoridad del operador sobre journals. No se añade collector, servidor
de métricas ni dependencia del dominio hacia tracing. Corrupción del store puede
exigir la [remediación conservadora](client-configuration.md#planes-receipts-y-recovery)
en identidades físicas nuevas, sin borrar evidencia original.

ADR-059 libera la cuota de planes terminales mediante una marca atómica y poda
antes de la próxima admisión. El port existente del publisher agrega replay de
ID/digest/key exclusivamente sobre journal existente, bajo autoridad viva; no se
añade cache de tombstones ni otro store. TTL limita empezar cambios nuevos.
