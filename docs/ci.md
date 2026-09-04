# CI local, GitHub y matriz de evidencia

El gate de calificación sigue siendo local: ramas ai/, commits coherentes, merge
no-ff y evidencia post-merge. ADR-029 define la matriz inicial de M0. ADR-047 añade
GitHub CI/CD sin convertir sus runners alojados en evidencia automática de sandbox.

`.github/workflows/ci.yml` usa actions oficiales fijadas por commit, permisos
`contents: read` y cancelación por concurrencia. Aprovisiona explícitamente el
toolchain1.98.1 y dependencias locked, y ejecuta fmt/check/Clippy/tests/doctests y
fronteras arquitectónicas en Linux x86_64, macOS 26 ARM64 y Windows x86_64. Un job
Linux separado instala versiones fijadas de cargo-audit/cargo-deny y aplica
advisories/bans/sources. Los pull requests no reciben secretos ni permisos de
escritura. Este workflow puede descargar dependencias y advisory data durante su
fase explícita de aprovisionamiento; el runtime MCP no adquiere nada.

`.github/workflows/release-candidate.yml` solo admite dispatch manual desde un tag
de versión existente. Construye el perfil core para los tres runners, empaqueta
LICENSE/NOTICE, verifica SHA-256, crea attestations OIDC y abre un GitHub prerelease
en borrador. No publica en crates.io, no contiene E5/ORT/LanceDB y no convierte el
draft en release soportada. Publicarlo exige cerrar previamente las filas aplicables
de la matriz M1-17.

```text
python3 scripts/gate.py core
RUST_MCP_TEST_SOCKET=/ruta/docker.sock RUST_MCP_E5_DIR=/ruta/e5/onnx ORT_LIB_LOCATION=/ruta/ort python3 scripts/gate.py full
```

El reporte por defecto queda en `target/gate-report.json`; `--report PATH` permite
conservar un artifact de validación. Cada etapa registra comando, duración y estado.
Un error o prerequisito ausente produce exit no cero. No se aceptan Python -O ni
sustituciones del toolchain. Cargo utiliza CARGO_INCREMENTAL=0, --locked --offline.

| Entorno | Gate M0 | Alcance / pendiente |
| --- | --- | --- |
| macOS26.6.2/APFS ARM64, Rust1.98.1 | Core + full real | Único host nativo validado; E5/ORT/LanceDB incluidos |
| Docker/Linux ARM64, runc/cgroupsv2 | Security probes + CLI capabilities | M0 probes; M1 ADR-031 añade Rust1.98.1 y source transfer, sin host mounts |
| Linux ARM64/x86_64 nativo | Sin ejecutar | Requiere runner Rust1.98.1; acceso protegido de proyecto aún unavailable |
| Windows x86_64/ARM64 | Sin ejecutar | Requiere runner, harness Windows y adapter no-follow/reparse-safe |
| macOS x86_64 | Sin ejecutar | Requiere runner y native ORT/model receipt de esa plataforma |

`core` ejecuta fmt, check, Clippy, unit/integration/contract/protocol/security sin
Docker, doctests, invariantes arquitectónicas, integridad de vendor, corpus Cargo,
audit y deny. Los tests Docker ignorados se ejecutan obligatoriamente en `full`.
`full` añade probes Docker, gateway Rust real ADR-031, auditoría RustSec/SQLite
bajo network deny y el gate semántico con feature local/modelo real. Un build
sin feature local no califica M1. Los controles de arquitectura por texto son
regresiones útiles, no una prueba de ausencia de todo I/O transitivo.

Prerrequisitos explícitos: rustup (consulta de toolchain instalado), Rust/Cargo1.98.1+rustfmt+Clippy, Python3.11+, dependencias
del lock en cache, cargo-audit/cargo-deny, bases RustSec locales y suficientemente
recientes. Full requiere además Go1.27.1, Docker Desktop/buildx arrancado, cliente y
socket locales, imagen Rust aprobada de ADR-031 ya instalada, modelo E5 del recibo y ORT1.24.2 estático con hash validado. Ninguno
se instala/refresca automáticamente. `cargo fetch --locked` solo se usó durante
aprovisionamiento explícito de desarrollo antes del gate offline.

Deny ejecuta advisories/bans/sources con todas las features, sin advisory ignores.
`paste`1.0.15 tiene advertencia de mantenimiento transitiva visible en cargo-audit;
versiones duplicadas son warnings. ADR-047 resolvió la licencia del código original;
la redistribución de dependencias/modelos y sus notices sigue pendiente. `deny licenses`
no forma parte del gate M0 ni se declara aprobado. Esos notices se requieren antes
de publicar binarios M1, junto al benchmark ES/EN y recibos nativos por plataforma.
Los fixtures build.rs/proc macros/libtest bajo
el sandbox Cargo se verifican en los cortes M1 y en rust-security; no se atribuyen a M0.

El gate resuelve el Cargo real1.98.1 una sola vez, fija su binario hermano rustc y
precedencia del PATH de ese toolchain para scripts hijos; valida ambas versiones.
Prerrequisitos de plataforma/full se comprueban antes de la primera etapa. El build
de la imagen probe reutiliza cache y conserva el tag local para inspección/reuso;
no se afirma clean-room ni se borra una imagen aprobada automáticamente.

El harness de desarrollo `test-execution.sh` fija el cliente aprobado de Docker
Desktop en `/Applications/Docker.app/Contents/Resources/bin/docker`, requiere el
socket explícito, crea un state-root temporal privado y resuelve el ID inmutable
de la imagen que acaba de construir. No requiere otras variables Docker ni admite
skip ante ausencia/error. La CLI de producto recibe los cuatro inputs del host;
los valores del harness describen solo esta calibración de desarrollo.

## Evolución del gate Rust por corte

La configuración actual ejecuta20 tests exactos secuenciales; los párrafos
siguientes conservan la evolución histórica y los recibos de cada incorporación.

`test-rust-execution.py` requiere el socket explícito y el image ID aprobado
compilado en el gateway. Ejecuta secuencialmente dos tests exactos: transferencia
USTAR con directorios vacíos/nombre100 bytes y calibración de seis escenarios
Rust, seguida de metadata autorizada y revocación ante recalibración cancelada.
Guarda logs y recibo en `target/rust-security/`; no instala ni selecciona otra
imagen. `test-execution.sh` conserva exclusivamente la integración de probes M0.

El stage rust-security incluye ahora cuatro tests exactos secuenciales: transferencia
benigna, seis escenarios adversos del gateway, inspección MCP real y cierre por
EOF/cancel durante calibración. Rechaza cero tests ejecutados. Conserva recibos
`target/rust-security/calibration.json` y `mcp-inspection.json`; no debe ejecutarse
en paralelo con otros jobs Docker del gateway (startup rechaza objetos existentes).

M1-02 reutiliza el test MCP real para ambas inspecciones en una sesión. El test
exacto se llama toolchain_inspect_observes_installed_runtime_with_shared_calibration;
target/rust-security/mcp-toolchain.json conserva el inventario y tres ejecuciones.
El reporte M1-01 previo es histórico; script actual exige ambos recibos.

M1-03 amplía rust-security a seis tests exactos secuenciales: añade Cargo check
con éxito/E0502/E0106, logs Resources, owner/revocación y locks frozen; además
cancelación/EOF después de observar build scripts reales activos. El recibo
`target/rust-security/mcp-check.json` exige seis logs y dos compilaciones fallidas
como resultados válidos. Ningún fixture Cargo se ejecuta fuera del gateway.

M1-04 extends `scripts/test-rust-execution.py` to seven serial exact Docker tests;
its actual fmt case checks configured style, workspace coverage, no-op override,
invalid syntax, newline-only and large diff, seven log readbacks and source
immutability. The script rejects absent execution and missing receipts.

M1-05 extends the Rust execution gate to nine exact serial tests, adding actual
Clippy build.rs/proc-macro containment and six-case MCP profiles/Resources.
The harness rejects zero executed tests and missing successful receipts.

M1-06 adds actual R2 libtest containment (including observed detached descendants),
nine MCP selection/outcome/log cases, active test cancellation/EOF and an adversarial
proc-macro forged-phase check. Run the Rust execution gate serially with the explicit
socket. Fixtures are captured bytes and must never be compiled on the host.

M1-07: Rust execution gate has16 exact serial Docker tests, including13 audit calls
across3 tests. `python3 scripts/test-audit-data.py` separately executes real
RustSec/SQLite under macOS network deny with positive/negative TCP/UDP IPv4/IPv6
controls and no runtime temporary files. It is a new full stage (14 total); core
remains10 stages. No gate installs or refreshes dependencies/snapshots.

M1-08 adds an exact actual compiler-explanation MCP case to the serial Rust gate
(now17tests), checking invalid inputs before work, E0502/E9999 content/runtime
evidence without project authority, and EOF cleanup. Full still14stages.

M1-09 extends the serial Rust security stage to20exact tests. Three quality-gate
cases validate fast/standard,21distinct log SHA checks, source immutability and
active libtest cancellation/EOF. Full remains14stages including real E5/LanceDB
all-features and native macOS test-process network deny. No Assets are refreshed.

## M1-10 — Etapa catalog añadida

Full añade `scripts/test-catalog.py` como etapa15. Construye el test CLI con feature
local, assets E5/ORT previamente aprobados y Cargo locked/offline; ejecuta import,
status/restart/rollback y rebuild/restore Lance real bajo network deny macOS con
controles positivos. El caso nativo ignorado en core se ejecuta explícitamente en
esta etapa. No usar el emitter de fixtures como parte del gate.

[M1-10](validation/M1-10.md) conserva full15/15 con hashes inmutables, previo al
ajuste final de observabilidad del CLI, y core540/all-features Clippy/CLI nativo5+1
posteriores. No atribuir un full anterior a bytes posteriores: ambos conjuntos de
fuentes y la revisión/disposición están registrados.
La etapa no acredita distribución, rendimiento/ES-EN, clientes reales ni runners
nativos adicionales. No instala/refresca assets. [Formato y requisitos](catalog-bundle-format.md).

## M1-11 — Estado de catálogo MCP

Full incorpora `scripts/test-catalog-status.py` como etapa16. Construye con feature
local y ejecuta dos tests ordinarios y uno nativo bajo network deny macOS, con
controles IPv4/IPv6. Verifica el contrato MCP, E5/Lance reales, generación retenida,
índice corrupto tras reinicio y disponibilidad independiente de SQLite. Core
continúa con10 etapas. No instala assets ni acredita clientes o hosts adicionales.
[Evidencia](validation/M1-11.md).

M1-12 añade `scripts/test-crate-search.py` como etapa17 de full:2 tests ordinarios y1 nativo ignorado ejecutado explícitamente, con E5/Lance bajo network deny.
La [evidencia M1-12](validation/M1-12.md) registra el gate focalizado; el full conjunto final sigue requerido antes de cierre/release.

## M1-13 — Inspección paginada MCP

Full incorpora `scripts/test-crate-inspect.py` como etapa18: dos tests ordinarios,
compilados con feature local y ejecutados bajo network deny macOS. Comprueban
páginas SQLite, hechos desconocidos y continuación ligada a generación; inspect
no necesita ni ejecuta embeddings para estas consultas. El [gate M1-13](validation/M1-13.md)
pasó: core629/10 etapas, protocolo37, Clippy all-features/all-targets y los dos tests
local-feature bajo OS network deny. Ese resultado fue focalizado; el full conjunto vigente sigue requerido antes de
cierre/release. No instala assets ni acredita otros hosts.

## M1-14 — Gate de doctor

Full incorpora scripts/test-doctor.py como etapa19; core conserva10 etapas. El script
verifica CLI ordinaria y dos casos activos secuenciales: inventario/calibración real
del runtime aprobado e interrupción SIGINT durante un job observado, esperando su
cleanup y comprobando ausencia de objetos propios. No instala herramientas, descarga
imágenes ni ejecuta proyectos del usuario.

El reporte local target/doctor-security/report.json registra status=passed,
active_cases=2 y cleanup=true, con imagen y job observado. Es evidencia focalizada
de doctor, no afirmación de full19/19 ni de nuevos runners. SIGINT fue comprobado;
el manejo implementado de SIGTERM no se presenta como un caso adicional ejecutado.
[ADR-045](adr/ADR-045-cli-doctor.md). Distribución, clientes reales y cierre M1
mantienen sus gates independientes.
