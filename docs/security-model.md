# Modelo de seguridad

## Evidencia actual

M0-01 no ofrece ejecución de herramientas ni acceso a proyectos. El parser consume
como máximo el comando y dos argumentos adicionales para rechazar invocaciones no
soportadas; usa argumentos OS sin asumir UTF-8. Su salida de error es constante.
Los tests comprueban rechazo y separación stdout/stderr. No prueban sandbox.

M0-02 añade validación de contratos sin I/O. Los constructores y la deserialización
rechazan referencias/digests mal formados, spans invertidos, errores parciales y
freshness inconsistente. Ninguna prueba autentica handles, hashes o fuente de
metadata. Una evidencia deserializada conserva la evaluación en `assessed_at`;
la aplicación futura debe reevaluarla con su reloj/policy antes de usarla como
freshness actual. `network_used` histórico no produce `live`.

Las colecciones/textos son valores de dominio, sin un presupuesto operativo global.
Los adapters posteriores deben imponer límites antes/durante parsing y streaming,
y redactar errores de Serde que puedan incluir contenido no confiable. Truncation
declara evidencia recortada; no implementa esos límites.

## Frontera stdio M0-03

El lector rechaza líneas mayores de 1 MiB (LF excluido, CR incluido) antes de
entregarlas completas a rmcp; conserva un contador entre reads y usa chunks de
hasta 8 KiB. EOF con línea parcial falla. Una violación puede descartar otras
líneas del mismo chunk: al fallar se cierra toda la sesión, sin garantía de
respuesta para requests pendientes. El writer señala errores incluso después
de bootstrap y cancela el servicio aunque stdin permanezca abierto.

El runtime termina con cleanup acotado y espera al worker del gateway para
verificar la eliminación de contenedores y volúmenes de ejecución M1.
stdout solo lleva protocolo; tracing solo habilita mensajes propios fijos a stderr,
sin leer RUST_LOG ni formatear errores del SDK con datos externos. Los errores MCP
siguen siendo responsabilidad del SDK y pueden citar campos del request según
el protocolo (por ejemplo requestedVersion). No son logs del servidor.

El límite de línea no sustituye admisión global. ADR-030 incorpora antes de jobs
costosos un worker sin cola,16 solicitudes/notificaciones/envíos pendientes y
deadline absoluto de I/O10s. Las cancelaciones conocidas se propagan al gateway;
un ID desconocido no altera la sesión. Los tests M1 verifican cancelación/EOF y
cleanup del árbol; rmcp conserva la gestión del protocolo y del primer request inline.

## Frontera de proyectos M0-04

Las roots del host se abren con handles y flags no-follow en cada componente.
No se usa canonicalize para autorizar I/O productivo. La lectura de cada path
completo parte de la root original, con BENEATH y sin bajar a un handle descendiente
que un writer pueda mover. Se rechazan symlinks incluso internos, hardlinks,
lectura de archivos no regulares y cambios observados de root/proyecto/manifest.
El fstat de tipo ocurre después del open no bloqueante: la apertura de FIFO puede
ser observable y una device node preexistente puede invocar su driver. Las roots
soportadas no deben contener device nodes; creación privilegiada de dispositivos
/mounts está fuera de esta frontera. APFS no implica NODEV; no se anuncia ausencia
de efectos al abrir objetos especiales. ADR-024 registra esta limitación. Los tests
incluyen sustituciones y carreras reales; no prueban snapshots atómicos o ausencia
universal de ABA. El hash es identidad de manifests, no identidad de ejecución.

Solo macOS 26+ / APFS tiene adapter habilitado. Linux/Windows rechazan; no se acredita
containment de junctions. El host aporta paths físicos (aliases `/tmp`/`/var` fallan).
Las restricciones y límites precisos, incluida cancelación cooperativa, se describen
en [ADR-024](adr/ADR-024-project-open.md). Ni el request, ni Cargo.toml ni el entorno
amplían roots. Los permisos del host siguen siendo relevantes; no hay proceso
sandboxed. Las referencias expiran y el registro comprueba identidad antes de uso.

## Decisiones vigentes y ejecución pendiente

- [ADR-007](adr/ADR-007-explicit-project-handles.md): roots preautorizados por el host,
  handles opacos y acceso relativo no-follow/reparse-safe; canonicalizar no evita TOCTOU.
- [ADR-008](adr/ADR-008-execution-gateway.md): un único gateway, argumentos tipados,
  entorno con `env_clear`, timeouts, cancelación y cleanup del árbol completo.
- [ADR-009](adr/ADR-009-deny-by-default-security.md): capabilities independientes;
  `strict` y `restricted` fallan cerrados cuando falta una garantía requerida.
- [ADR-010](adr/ADR-010-no-arbitrary-shell.md): sin shell ni flags arbitrarios.
- [ADR-018](adr/ADR-018-offline-catalog-sync.md): sincronización/importación explícita
  fuera del runtime, nunca descarga oculta durante tools MCP.

Cargo check, Clippy y test pueden ejecutar código del proyecto. Un process group
no garantiza containment de descendientes desacoplados, y el modo offline de Cargo
no bloquea sockets. Estas limitaciones no se presentarán como seguridad implementada.

Los procesos del harness incluyen el binario propio, mkfifo para un fixture controlado
y Cargo sobre fixtures revisados, incluido un build.rs benigno que escribe en OUT_DIR;
el adversario de security queda excluido y no se ejecutan repositorios externos. Cualquier
gate de desarrollo ejecuta código del repositorio bajo la autoridad del host.

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

## Contratos M0-07 y catálogo M0-08

M0-07 centraliza validación de contratos en `stdio::contract`: inputs y outputs
con schemas cerrados, validación Serde adicional y errores fijos sin payloads.
El snapshot de `rust.project.open` y las cinco versiones MCP se conservan.

El catálogo M0-08 recibe bytes ya adquiridos por el host y un manifest esperado.
Verifica hash/tamaño, schema, integridad SQLite/FTS5 y facts antes de activación.
Opera en memoria sin abrir paths ni aceptar SQL del caller; checksum no autentica
al publisher. M1-10 incorpora verificación firmada y adquisición filesystem/CLI
por separado, descritas debajo y verificadas en su recibo M1-10.
Límites de imagen/consultas y progress handler no equivalen a un límite OS de RAM.

## Modelo e índice locales

M0-09 no acepta paths/URLs del modelo ni URI del índice: recibe bytes con tamaños
y hashes fijados y crea LanceDB solo en memoria. ORT se configura por el host sin
telemetry antes de cargar; configuración global previa ajena causa fallo cerrado.
El gate macOS calibra red denegada y ausencia de spill temporal. No demuestra el
tier strict del producto ni límites duros de memoria/CPU nativa. La procedencia
del loader es verificación local; el recibo separado conserva la descarga previa.
El fallback conserva evidencia de SQLite y declara el error semántico.

## ArtifactStore mínimo M0

ADR-028 sustituye el directorio privado propuesto por memoria efímera: no se afirma
un gate de permisos de disco. Cada consulta requiere ProjectRef owner+ArtifactId
opaco; M1 debe validar además que el proyecto siga autorizado/vivo. Cuotas de
bytes/cantidad globales y por owner, TTL monotónico y cleanup en operaciones.
Una regresión del reloj limpia y bloquea el store. Redacción byte a byte de todos
los patrones literales solapados, incluso entre chunks y prefijos truncados.
Input/output tienen presupuestos independientes; alcanzar exactamente el cap
marca truncación conservadora sin leer otro byte. No hay borrado seguro de RAM.

## Admisión MCP M1-01

ADR-030 añade admisión sobre mensajes ya parseados por rmcp, sin otro parser RPC.
Un worker sin cola y permisos hasta terminación real;16 requests,16 notifications,
16 sends pendientes. Los requests cancelados sin respuesta conservan el permiso
hasta teardown porque rmcp3.2.0 no notifica su consumo. Agotar esa cuota requiere
reconectar. Rechazo por sobrecarga/ID pendiente duplicado cierra la sesión; jamás
se bloquea receive esperando un permiso. Input/output1MiB por frame y deadlines
absolutos10s. La serialización y RSS nativo requieren además límites de DTOs.
No se habilita Cargo ni se afirma kill-tree por estas pruebas de workers.

El camino Rust ADR-031 exige imagen aprobada por ID y calibración propia. Ingiere
solo USTAR generado desde SourceBundle validado, mediante tar fijo UID0/caps0;
elimina/verifica ese escritor antes de ejecutar Cargo UID65534/caps0 con source
read-only, /work exec512MiB y /tmp noexec64MiB. La red tiene enforcement seccomp y
network=none. Volúmenes local no demuestran cuota contra un extractor comprometido;
el ingester confiable no se presenta como sandbox estricto de código de proyecto.
Timeout/cancel/overflow terminan contenedores y verifican ausencia antes de borrar
el volumen; cleanup incierto pone el gateway en cuarentena. La evidencia y límites
están en [M1-01](validation/M1-01-rust-gateway.md).

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

M1-04 uses a closed FormatCheck command and fixed RUSTFMT executable under the
existing network-denied, read-only-source profile. Stable style config and skip
attributes are honored; disable_all_formatting is forced false. Unknown warnings,
malformed/truncated output or parser failure never yield a complete pass. No
JSON/unstable emit mode, arbitrary flags, source edits or new authority (ADR-035).

Clippy has closed profiles and runs with frozen/offline JSON under the existing
read-only source sandbox. Compile-time project code remains untrusted. Lint-family
classification is evidence normalization, not authentication. ADR-036 requires
actual Clippy build.rs/proc-macro containment checks before Done.

M1-06 R2 uses the same strict effects policy, with actual libtest containment and
detached test processes observed before destructive controls. The closed timeout
is1..60s/default30s for gateway work; cleanup has independent joined budgets.
readOnlyHint describes enforced host effects, not code trust. Cargo-looking tail
events invalidate reported phase completeness; retained log headings are unescaped
human presentation, not authenticated stream framing.

M1-07 keeps metadata under the restricted external effects matrix and performs
bounded RustSec/SQLite work on owned data in-process. It does not claim whole-MCP
OS isolation. An explicit macOS test-process deny-network gate verifies TCP/UDP
IPv4/IPv6 denial and real RustSec matching/SQLite without temporary files; this
scope is separate from Docker metadata containment. No HTTP/Git RustSec features.

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

## M1-10 — Bundle, trust y estado durable

La firma Ed25519 con prefijo de dominio se verifica antes del JSON/payload parsing;
la descompresión y lectura USTAR previas están acotadas. Nombres de archive nunca
se usan para escribir. Publisher/channel provienen de trust propio0600 bajo padre
0700 y ancestros controlados; el host debe impedir grants ACL adicionales. Firma
no significa aprobación editorial/licencia. Las claves fixture son públicas.

Store APFS no-follow con lease exclusiva y staging fijo; floor separado reserva
secuencia/hash antes de active. Recuperación permite bytes reservados exactos o
secuencia mayor y falla cerrada ante floor inválido. Key rotation no reinicia el
floor. Los checksums locales no protegen contra el owner que elimina/restaura todo
el estado. Status puede limpiar staging bajo lock; no prometer lectura sin efectos.

Restore nativo valida identidad completa E5/catálogo, objetos, tabla y cobertura;
ningún path del artifact se abre como URI filesystem de Lance. La CLI valida índices
importados antes de activar; derived inválido no cambia facts SQLite. Modelo/ORT
requieren assets explícitos. Sync HTTPS limitado es la única nueva operación de
red y permanece fuera de tools/runtime. [Presupuestos y CLI](catalog-bundle-format.md)
detalla80MiB bundle,16MiB index y plazos cooperativos, sin límite RSS/CPU nativo
duro. [Evidencia M1-10](validation/M1-10.md).

## Contexto runtime M1-11

[Evidencia M1-11](validation/M1-11.md). El reader read-only no adquiere lease,
crea locks, reserva floor ni borra staging. Lectura floor/active/floor con retry
acotado evita presentar una mezcla durante administración concurrente. Un active
verificado anterior al floor se declara con reserva pendiente; floor inválido o
identidad distinta falla cerrado. La primera generación observada se conserva por
sesión, sin refresh implícito. Fallos del índice/modelo dejan SQLite verificado
disponible; RustSec de audit se lee separadamente cada vez.

El límite128KiB cubre el resultado MCP codificado completo. El deadline120s es
cooperativo: no interrumpe por fuerza parsers/inferencia nativos, y su capacidad
permanece ocupada hasta el retorno real. Cancelación/timeout no publican éxito
tardío. `runtime_api_disabled` describe ausencia de adquisición en esta API, no
bloqueo OS de red del proceso completo. [ADR-042](adr/ADR-042-catalog-runtime-status.md).

## Búsqueda M1-12

Canales acotados a50 candidatos; unión/hidratación hasta100 IDs. SQLite valida los
facts y selecciona versiones elegibles antes de limitar la página; IDs desconocidos,
duplicados o distancias inválidas invalidan el canal semántico. Fallback conserva
filtros y no oculta cancelación ni errores SQLite. Los scores e IDs de advisories no
son evidencia de calidad ni de ausencia de vulnerabilidades.

La tool no abre nuevos paths ni sincroniza assets; comparte estado retenido y
admisión de status. Encoding y validación siguen dentro del worker joined120s;
cap512KiB completo y omisiones explícitas impiden recortar silenciosamente facts.
Estos presupuestos no son límites duros de RAM/CPU nativas ni nueva política OS.
[ADR-043](adr/ADR-043-catalog-search-modes.md) y [evidencia M1-12](validation/M1-12.md).

## Inspección de facts M1-13

La identidad de snapshot se compara antes de leer la página; no depende de modelo
ni índice y no otorga permisos nuevos. El adapter limita consultas a escalares y
páginas del catálogo retenido, preservando hechos y ausencia explícita de datos.
La continuación puede cambiar parámetros porque cada página es una consulta nueva;
no permite cambiar la generación sin detectar mismatch. Recorte de salida conserva
entradas completas y progreso, bajo512KiB del resultado duplicado y120s cooperativos
del mismo worker. Estos límites no añaden containment OS ni deadlines nativos duros.
[ADR-044](adr/ADR-044-paged-crate-inspection.md), [gate aprobado](validation/M1-13.md).

## M1-14 — Diagnóstico sin autoridad adicional

Doctor pasivo no lanza subprocesses, no busca herramientas en PATH ni toma la lease
administrativa del catálogo. Lee inputs configurados con los mismos adapters seguros;
puede validar/cargar assets nativos locales. Root accesible no implica una auditoría
completa de ACL ni soporte seguro en otros OS. --active autoriza expresamente la
calibración existente y observaciones tipadas del runtime aprobado, nunca source
seleccionado por un proyecto. Host tools permanecen sin comprobar.

Las señales se registran antes de iniciar el worker; SIGINT/SIGTERM/SIGHUP activan cancelación
y se espera cleanup. Los deadlines120s/900s son cooperativos, no preempción nativa.
Reportes humano/JSON se limitan a128KiB y usan razones/acciones cerradas, sin paths ni
errores arbitrarios del host. Cleanup incierto conserva fallo; warning/exit0 no afirma
containment o readiness universal. No descargas ni reparación automática.
[ADR-045](adr/ADR-045-cli-doctor.md).

## M1-15 — Candidatos locales

La preparación M1-15 verifica archivos confiables generados localmente; no introduce un instalador genérico. Catálogos entran por el importer autenticado existente. Trust seed42 sigue siendo fixture pública; instalación privada y firma ad hoc no otorgan identidad de publisher.

## Escritura local M2 en desarrollo

[ADR-050](adr/ADR-050-local-coordinated-mutation.md) fija edición local
coordinada para M2: permiso de escritura del host configurado una vez, preview/diff,
precondiciones, locks entre instancias MCP, journal y recuperación conservadora.
No exige servicios privilegiados, sudo, cuentas ni cambios de ownership del proyecto.
El host mantiene estables las roots y evita editar simultáneamente los archivos
afectados durante el commit breve. El MCP no bloquea al IDE ni promete CAS o atomicidad
visible multiarchivo. Conflictos observados detienen la operación; los posteriores
pueden requerir recuperación conservando evidencia y sin pisar bytes desconocidos.
La rama de desarrollo incorpora `rust.manifest.patch` para lints del `Cargo.toml`
raíz e integra `rust.fmt.apply` para archivos Rust existentes. Ambos usan
preview/commit/receipt, con grants separados `--allow-manifest-write WORKSPACE_ROOT`
y `--allow-fmt-write WORKSPACE_ROOT`. La calificación conjunta sigue en curso;
estas capacidades no forman parte de la release `0.1.0`. Las trece tools M1 y su
sandbox se conservan. Fix, dependencias y las demás familias de patch siguen
pendientes; el tablero enlaza la evidencia sin anunciar M2 terminado.
# Metadata y namespace del writer M2

En el writer experimental macOS/APFS, ACL se conserva mediante CLONE_ACL del
kernel; no se compara independientemente. UID/GID, modo, file flags y xattrs sí
se verifican. La exclusión de hardlinks depende de nlink, no de una garantía
acreditada a O_UNIQUE. El host y el IDE deben dejar intactos los nombres reservados
`.rust-mcp-mut-*`, también después de una interrupción: la verificación previa
no convierte unlink/rename en compare-and-swap. Conservar journal y temporales si
hay `recovery_required`; no usar git clean ni borrar evidencia para desbloquearlo.
