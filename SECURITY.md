# Seguridad

## Estado actual

El binario acepta ayuda, versión y `serve --stdio` con roots/TTL fijados por el
host. `rust.project.open` lee manifests dentro de capacidades de directorio y
registra referencias de proceso; no ejecuta Cargo/procesos ni consulta catálogos
o sockets. El acceso inicial exige macOS 26+ / APFS y flags de kernel verificados;
otros adapters fallan cerrados. Esto no constituye un sandbox OS de ejecución.
Los argumentos rechazados y errores operativos usan diagnósticos fijos.

El adapter limita líneas de entrada a 1 MiB antes de LF, rechaza EOF parcial y
cancela la sesión ante errores de I/O. Solo logs propios salen por tracing a
stderr; `RUST_LOG` no habilita logs del SDK con payloads del peer. rmcp mantiene
el parsing, negociación y errores de protocolo. Hay un worker de project.open, límites de manifests y deadline cooperativo de
10 s; no hay presupuesto global de requests/salida ni deadline para clientes
lentos. Kernel I/O y primer request inline tienen las limitaciones de ADR-024.

El dominio M0-02 valida formatos e invariantes tanto en constructores como al
deserializar. Un `ProjectRef` sintácticamente válido no acredita autoridad ni
entropía; un digest no prueba integridad. Provenance y freshness permanecen juntas
en evidencia snapshot. La validación de metadata no autentica su origen. Los límites
de project.open y el saneamiento de errores se aplican en sus adapters;
los errores de formato externos no deben exponerse sin redacción.

La release binaria 0.1.0 publica únicamente el core sobre macOS26 ARM64/APFS; ADR-048
limita a ese entorno el soporte positivo. El único perfil
positivo de ejecución añade el gateway guest Docker Linux ARM64 aprobado. El
archive no incluye Docker, toolchain, modelo, ORT, LanceDB, catálogo ni trust.
No utilizarlo para ejecutar o validar repositorios no confiables. Los gates locales
son herramientas del desarrollo del servidor, no capacidades del producto.

## Requisitos para habilitar ejecución

El [modelo de seguridad](docs/security-model.md) y ADR-007..010 exigen roots confiables,
I/O relativo no-follow, Execution Gateway único, entorno reconstruido, aislamiento
real, control del árbol de procesos y límites. Cargo check, Clippy, tests, build
scripts y proc macros pueden ejecutar código no confiable.

No se anunciará una garantía antes de demostrarla mediante pruebas en la plataforma
correspondiente. Una opción offline de Cargo no demuestra aislamiento de red.

## Reportar un problema

Reportar vulnerabilidades mediante
[GitHub private vulnerability reporting](https://github.com/pharos-lang/rust-engineering-mcp/security/advisories/new),
con versión o commit, plataforma, pasos de reproducción e impacto. No abrir una
incidencia pública antes de coordinar la corrección y no incluir secretos reales en
fixtures o logs. No existe todavía una versión binaria soportada; el código público
permanece en desarrollo.

## Gateway M0-05

El gateway M0 separado del servidor ejecuta probes locales confiables en Docker/Linux
arm64 con cgroups v2. No admite Cargo, programas arbitrarios ni mounts del host.
El cliente Docker, daemon/VM, imagen inmutable y rutas de control son TCB del host;
no se hereda el entorno ni el contexto Docker. Estado propio macOS/APFS no-follow;
otros hosts fallan cerrados. Los presupuestos de ejecución excluyen preparación y
cleanup, que tienen plazos propios de control. Daemon/host no disponible impide
certificar cleanup: se devuelve CleanupUncertain y se bloquea la instancia.
M0-06 acredita capabilities del fixture mediante una operación CLI explícita. En
ese corte histórico solo operaba rust.project.open; los probes no acreditan Cargo.
La evidencia Rust1.98.1 de M1 es independiente y usa el runtime aprobado ADR-031.

## Detección activa M0-06

`capabilities --docker PATH --docker-socket PATH --state-root PATH --probe-image sha256:ID`
produce JSON con status verified/degraded/unavailable, timestamp observado,
identidad de engine/configuración/imagen y evidencia por probe. Exit0 exige todas
las garantías; exit1 indica degradación o indisponibilidad; uso inválido es exit2.
`strict_available`/`restricted_available` están acotados a
`scope=trusted_probe_image_only`; `project_code_available=false` siempre.
No importa reportes previos ni hace descargas. Un engine diferente, capability
faltante o evidencia de otra configuración no habilita ejecución.

Los controles positivos solo crean sockets (sin tráfico) y escriben un canario
sintético de la imagen. Ninguno tiene mounts del host. El reporte prueba denegación
de socket IPv4/IPv6 TCP/UDP usada por DNS/loopback y UNIX/NETLINK; no afirma haber realizado
consultas DNS o conexiones externas. La ausencia del canario host es observación
auxiliar: la frontera es el namespace de mounts verificado sin binds/volúmenes,
rootfs read-only y seccomp sin mount/unshare/setns. La carrera de symlinks prueba
protección del canario read-only dentro del guest, no transferencia de proyectos.
macOS/APFS + Docker Linux arm64 es la única combinación validada. El camino
Rust aprobado se valida separadamente mediante ADR-031; no amplía la autoridad
del reporte de probes M0.

El catálogo M0-08 recibe bytes ya adquiridos por el host y un manifest esperado.
Verifica hash/tamaño, schema, integridad SQLite/FTS5 y facts antes de activación.
Opera en memoria sin abrir paths ni aceptar SQL del caller; checksum no autentica
al publisher. Import firmado y adquisición filesystem/CLI pertenecen a M1-10.
Límites de imagen/consultas y progress handler no equivalen a un límite OS de RAM.

El adapter semántico M0-09 carga solo el E5 exacto verificado desde bytes y un
índice LanceDB memory:// reconstruible. Su prueba offline macOS no sustituye los
tiers de sandbox ni demuestra límites duros de recursos nativos. El lock conserva
la advertencia de mantenimiento de paste1.0.15 (RUSTSEC-2024-0436); el cambio
manifest-only de LanceDB elimina dependencias de tests ajenas al runtime.

Artifacts M0 se almacenan solo en memoria (ADR-028). Sus cuotas limitan contenido
retenido y cantidad, no RSS total; la redacción cubre patrones literales provistos
por el host, no toda PII ni borrado garantizado de RAM. Fuente síncrona confiable y
no bloqueante; timeout/cancelación del productor permanecen en el gateway.

La admisión MCP M1-01 conserva leases hasta completar dispatch/envío y retiene
cancelaciones suprimidas por el SDK hasta cerrar la sesión (máximo16 slots).
Sobrecarga, IDs pendientes duplicados, output mayor de1MiB y deadlines de10s
cierran/cancelan la sesión. El worker conserva su permiso hasta terminar realmente;
un timeout del caller no certifica finalización. Ver ADR-030.

El camino Rust ADR-031 exige imagen aprobada por ID y calibración propia. Ingiere
solo USTAR generado desde SourceBundle validado, mediante tar fijo UID0/caps0;
elimina/verifica ese escritor antes de ejecutar Cargo UID65534/caps0 con source
read-only, /work exec512MiB y /tmp noexec64MiB. La red tiene enforcement seccomp y
network=none. Volúmenes local no demuestran cuota contra un extractor comprometido;
el ingester confiable no se presenta como sandbox estricto de código de proyecto.
Timeout/cancel/overflow terminan contenedores y verifican ausencia antes de borrar
el volumen; cleanup incierto pone el gateway en cuarentena. La evidencia y límites
están en [M1-01](docs/validation/M1-01-rust-gateway.md).

Inspección MCP ADR-032: fuente capturada por handles originales, runtime explícito
calibrado de forma lazy y metadata tipada/budgeted. Rechazo durante bootstrap;
worker joined retiene capacidad hasta cleanup, incluso si el SDK abandona el handler.
EOF/fallo de I/O cancela trabajos. Panic o cleanup incierto impide declarar cierre
limpio. Revalidación final de ProjectRef no convierte el source capturado en una
snapshot atómica ni autentica imports de catálogo.

M1-02 no amplía roots ni permite comandos arbitrarios. El único comando nuevo lee
un path fijo del runtime inmutable; no sigue paths aportados por proyecto/peer.
Targets instalados provienen del manifiesto observado, no de supported-targets.

M1-03 parametriza Cargo check únicamente mediante CheckOptions validados. La nueva
configuración se recalibra; los probes M0 no acreditan este camino. Resources usa
URI opaca con owner, revalidación live y retención antes de entregar bytes; ninguna
URI autoriza I/O. Caché privada/TTL0, base64 y presupuesto completo512KiB. Logs
previos sobreviven al rollback individual de una publicación fallida. El store es
memoria efímera y redacción literal explícita vacía; no detecta secretos arbitrarios.

M1-04 formatting uses the same calibrated network-denied Linux ARM64 containment.
The fixed cargo-fmt check never writes captured or host source. Project formatting
configuration/skip attributes affect coverage; whole-project disable is overridden.
Display diffs are untrusted text, never executable edit instructions (ADR-035).

M1-05 Clippy can execute build.rs/proc macros. It uses the same calibrated gateway
and approved image; actual hostile fixtures must pass through Clippy as well as
check. Lint allows remain project-controlled; passing is not a security attestation.

M1-06 executes selected test binaries and doctests (R2) in the same approved
strict profile. Dedicated actual libtest fixtures verify containment and observe
detached descendants before timeout/cancel/overflow; MCP cancel/EOF observes live
tests. Additional Cargo-looking events after build-finished invalidate phase
completeness. Producer identity and human log section delimiters are unauthenticated.

M1-07 host snapshot integrity is SHA-256 relative to explicit host expectation,
not publisher authentication. Pure owned-byte RustSec/SQLite matching runs in the
joined worker; metadata still uses the calibrated network-denied gateway. Snapshot
paths never come from MCP arguments. Ambiguous lock identities and no-follow
failures are rejected. Signed import/distribution and durable rollback remain M1-10.

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

## Adquisición explícita M1-10

Solo la CLI adquiere bundles; runtime y tools no reciben autoridad de descarga.
Trust exige archivo propio0600, padre0700 y ancestros protegidos contra reemplazo;
los controles POSIX no inspeccionan ACLs: el host debe excluir grants adicionales.
Solo macOS26+/APFS está habilitado. La firma autentica al publisher elegido por el
host. IUMotion Labs no distribuye un catálogo oficial en 0.1.0: la fixture pública
no es trust root, no se empaqueta trust y no existe clave Ed25519 de producción que
custodiar para esta release.

El floor separado se reserva antes de activar; conservarlo permite reparar active
inválido/ausente con los bytes reservados exactos o una secuencia superior. No borrar
estado para recuperar. Durabilidad incierta exige reread; status también toma lock
y puede limpiar staging. El owner conserva la responsabilidad por el store entero.

HTTPS sync es opt-in del host, con hostname exacto, sin proxies/redirects/retries;
no equivale a network deny. Límites de bytes y deadlines cooperativos de lectura,
descompresión/rebuild no garantizan RSS/CPU nativo ni interrupción de kernel I/O.
Ver [contrato y recuperación](docs/catalog-bundle-format.md) y [evidencia M1-10](docs/validation/M1-10.md).

## Estado de catálogo M1-11

La undécima tool recibe solo `{}`; sus paths y trust provienen del host. Su lector
protegido no toma la lease administrativa, no crea locks ni limpia staging. Conserva
una generación por sesión y verifica firma/floor antes de SQLite; E5 y objetos Lance
requieren identidad/hash y cobertura de nombres. Fallos semánticos no invalidan
facts SQLite verificados. El host sigue siendo responsable de ACLs y estado durable.

`network.acquisition_allowed=false` con `enforcement=runtime_api_disabled` expresa
falta de autoridad de adquisición, no aislamiento OS de todo el servidor. La lectura
RustSec configurada para audit sigue independiente y se repite por llamada. Deadline
cooperativo120s y worker joined conservan admisión hasta terminar trabajo nativo;
el éxito tardío se descarta. [Detalles](docs/security-model.md#contexto-runtime-m1-11).

## Búsqueda de catálogo M1-12

La duodécima tool recibe query/modo/filtros cerrados, sin SQL, opciones FTS, paths
ni refresh. Texto con apariencia de operadores se trata como términos literales.
Índice/modelo no aportan facts: SQLite rehidrata y filtra cada candidato; identidad
semántica inválida causa fallback explícito, sin convertir cancelación en éxito.
Advisory IDs son los listados por el snapshot, no una auditoría completa.

Validación JSON y encoding/recorte permanecen dentro del mismo worker joined.
Su120s cooperativo y cap512KiB completo no prometen interrupción dura de código
nativo ni aislamiento OS adicional. [Contrato](docs/tools.md#rustcratesearch) y
[evidencia M1-12](docs/validation/M1-12.md).

## Inspección paginada M1-13

Los parámetros name/section/version/offset seleccionan una consulta explícita;
el fingerprint fija la generación y no concede autoridad ni actúa como cursor
opaco. Un mismatch bloquea la página antes de leer sus facts. La tool usa SQLite
retenido, sin requerir semántica ni adquirir assets. Repository es texto declarado;
source/documentation unknown e IDs de advisories no acreditan seguridad.
El worker conserva admisión durante I/O, validación y encoding;120s cooperativos
y512KiB del resultado MCP completo mantienen las limitaciones existentes.
[Contrato](docs/tools.md#rustcrateinspect), [gate aprobado](docs/validation/M1-13.md).

Doctor pasivo no ejecuta probes ni adquiere administración del catálogo. El modo
--active calibra el runtime aprobado y espera cleanup ante SIGINT/TERM/HUP;
una salida bloqueada puede fallar después de ese cleanup. Véase ADR-045.

## Frontera del artifact 0.1.0

El único archive publicado contiene el ejecutable core macOS ARM64, licencias del
producto, closure de dependencias target-specific, SBOM SPDX, notices, manifest y
hashes. GitHub OIDC acredita provenance de esos bytes, no un catálogo ni assets
excluidos. La instalación debe verificar el archive y ejecutar `version`, doctor
pasivo, discovery, el inventario de trece tools y denegaciones esperadas. El core
no demuestra por sí solo el perfil M1 completo: esa evidencia procede del full gate
`local` source-bound con assets exactos y del gateway aprobado.

## Escritura local M2 en desarrollo

[ADR-050](docs/adr/ADR-050-local-coordinated-mutation.md) define
`local_coordinated`. Los cinco grants de escritura son independientes y provienen
solo del host; `project_ref`, el request y el contenido del proyecto no amplían
autoridad. Preview no publica source. Para efectos nuevos, commit exige el plan exacto no
vencido, vuelve a comprobar identidad y bytes completos y usa publicación no-follow
más journal. Con plan ausente o expirado, solo se admite replay de un journal
existente ligado a ID/digest/key exactos y autoridad viva (ADR-059); puede recuperar
o migrar ese registro, nunca crear una operación nueva. Los
locks coordinan instancias con el mismo state root, pero no excluyen IDE, Git u
otros procesos del usuario. No se garantiza CAS, atomicidad visible multiarchivo,
protección frente a un host malicioso ni supervivencia demostrada a power loss.

El workspace nunca se monta con escritura en Docker. Source y vendor se capturan
como bytes propios; los procesos de ingest terminan antes del mutador y vendor se
monta read-only. Solo el mutador acotado puede escribir staging y solo el publisher
host aplica después los paths y bytes autorizados.
Fmt y fix pueden reemplazar como máximo 128 archivos `.rs` existentes. Fix ejecuta
build scripts y proc macros: esos componentes pueden influir en cualquier cambio
`.rs` permitido. Su perfil conserva `network=none`, pero permite sockets TCP
loopback dentro del namespace para el protocolo interno de Cargo; no se debe
describir como denegación absoluta de sockets. Esta excepción no se aplica a M1,
fmt, ingest, export ni resolución.

Las mutaciones que cambian resolución requieren un directory source aprobado por
path y SHA-256. El servidor lo captura con I/O no-follow; no hereda `CARGO_HOME`,
configuración, credenciales ni red del host. Cargo usa source replacement fijo,
HOME/CARGO_HOME efímeros y ejecución offline con red aislada. La policy
`preserve_presence` publica un lock actualizado solo cuando ya existía. Datos
ausentes, corruptos o cambiados bloquean la operación completa; no hay fallback a
descarga ni a edición solo del manifest.

La [calificación local M2](docs/validation/M2-07.md) está completada. El checkout se identifica como `0.3.0-dev`,
pero estas cinco tools no forman parte de la release estable `0.1.0`.

Los journals M2 parciales/corruptos pueden bloquear nuevas mutaciones del store
compartido. Se conserva la evidencia; el [procedimiento de recuperación](docs/client-configuration.md#planes-receipts-y-recovery)
requiere copias físicas y estado nuevos si la reconciliación no converge. Esta
limitación de disponibilidad no se presenta como recuperación automática universal.
Los eventos M2 por stderr no contienen código, rutas ni credenciales; el host
controla la retención de esos logs.

## Frontera M3 de jobs y artifacts persistentes

Un permiso de worker equivale a un permiso de job: la ejecución se puede consultar
o cancelar mientras el job está vivo, y los errores se enmascaran entre estados
no distinguibles. La expiración usa deadline monotónico durante la sesión y TTL
de reloj persistente al reconciliar; leer no renueva el TTL.

El store privado de calidad liga cada artifact al uid, al state root protegido y
al workspace root concedido por el host. Por tanto, el mismo uid con el mismo
state root y la misma root concedida por el host puede releer evidencia retenida,
incluso tras reinicio; no es aislamiento entre peers del mismo usuario. TTL y
cuotas aplican a esa evidencia y el host controla su retención. La imagen M3 añade
plugins solo mediante provisioning explícito autorizado; el runtime no instala ni
descarga.

El perfil seccomp quality conserva íntegramente el perfil Rust anterior y añade
exactamente una regla: `socketpair(arg0 == AF_UNIX, arg1 & 0x0f == SOCK_STREAM,
arg2 == 0)`. Es la forma anónima de stream que Tokio necesita; los perfiles M1 y
M2 no cambian, y AF_INET/AF_INET6 y la red Docker siguen denegados.
Los identificadores de Task y los locators de artifacts nunca son autoridad: la
autoridad proviene del owner, la root concedida y los grants del host según
[ADR-060](docs/adr/ADR-060-bounded-job-execution-and-mcp-tasks.md) y ADR-061.
La imagen, reporting y alcance de seguridad M1/M2 permanecen sin ampliación.

Coverage usa además un tmpfs nombrado por job, acotado y ejecutable únicamente en
sus fases `CoverageRun` y `CoverageReport`: el volumen de reports permanece
`noexec`, pero esas fases deben escribir profraw/profdata en el volumen dedicado.
Existe precisamente para cruzar esas fases en contenedores separados sin hacer
ejecutable el volumen de artifacts. Se elimina con cleanup.

La fuente host se monta read-only y el verifier comprueba los mounts aplicados;
la inmutabilidad se acredita con el canary del gate
`host_source_and_canary_are_unchanged_after_every_mutation_run`. El campo por
respuesta `source_unchanged` se eliminó precisamente porque el gateway no podía
hacerlo fallar: era una afirmación tautológica, no una observación. Tasks está
implementado, calificado y anunciado después de G4; el camino asíncrono sólo queda
habilitado para un peer que también declare `io.modelcontextprotocol/tasks`.
