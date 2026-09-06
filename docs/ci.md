# CI local, GitHub y matriz de evidencia

El gate de calificación sigue siendo local: ramas ai/, commits coherentes, merge
no-ff y evidencia post-merge. ADR-029 define la matriz inicial de M0. ADR-047 añade
GitHub CI/CD sin convertir sus runners alojados en evidencia automática de sandbox.
ADR-048 separa CI portable, host positivo y artifact distribuible.

`.github/workflows/ci.yml` usa actions oficiales fijadas por commit, permisos
`contents: read` y cancelación por concurrencia. Aprovisiona explícitamente el
toolchain1.98.1 y dependencias locked, y ejecuta fmt/check/Clippy/tests/doctests y
fronteras arquitectónicas en Linux x86_64, macOS 26 ARM64 y Windows x86_64. Un job
Linux separado instala versiones fijadas de cargo-audit/cargo-deny y aplica
advisories/bans/sources. Los pull requests no reciben secretos ni permisos de
escritura. Este workflow puede descargar dependencias y advisory data durante su
fase explícita de aprovisionamiento; el runtime MCP no adquiere nada.

`.github/workflows/sonarcloud.yml` calcula cobertura antes del análisis. Rust usa
`cargo-llvm-cov` 0.9.0 con Rust 1.98.1 y ejecuta el workspace, todos sus targets y
las dependencias fijadas por `Cargo.lock`; el resultado se entrega como LCOV.
Python usa Coverage.py 7.16.0 desde una wheel fijada por URL y SHA-256 y entrega
Cobertura XML. `scripts/test-*.py` se clasifica como código de prueba; los demás
scripts son fuentes medibles. El job ejecuta arquitectura, validación de reportes,
gate reporting, artifact/smoke, calificador Codex y exportación pública: 74 tests
Python en total. Los entrypoints que requieren un host release real permanecen
analizados por Sonar y probados por sus suites, pero se excluyen solo del porcentaje
de cobertura; su evidencia end-to-end es separada y candidate-bound.

`sonar.coverage.exclusions` nombra cada archivo individualmente —nunca un crate
entero ni un comodín— y ninguno sale del análisis: siguen midiéndose fiabilidad,
seguridad, mantenibilidad y duplicación. Solo se excluye lo que el scanner
portable no puede ejecutar, y cada grupo declara el recibo que sí prueba su
comportamiento:

1. Programas de calificación maintainer-only: `scripts/codex-model-qualifier.py`,
   `scripts/release-artifact.py`, `scripts/release-smoke.py` y
   `scripts/verify-vendor.py`. Requieren host release Darwin real, Docker/Codex o
   ambos. Recibos: [`M3-full-gate.json`](validation/M3-full-gate.json) y los
   receipts de release en `docs/validation/`.
2. Sondas M2 sobre Docker: `scripts/probe-m2-cargo-fix.py`,
   `probe-m2-fix-socket-mask.py`, `probe-m2-guest-staging.py`,
   `probe-m2-offline-registry.py`, `probe-m2-vendor-data.py` y
   `probe-m2-write-primitives.py`. Su única ruta ejecutable crea volúmenes y
   contenedores contra la imagen aprobada en un daemon local; el runner Ubuntu no
   tiene ni el socket ni la imagen. Recibos: los JSON `M2-*` que cada sonda emite
   y [`M3-rust-security.json`](validation/M3-rust-security.json).
3. Gateway Docker cerrado y sus puertos: `crates/execution-adapter/src/lib.rs`,
   `rust_gateway.rs`, `mutation_gateway.rs`, `mutation_test_gateway.rs`,
   `nextest_gateway.rs`, `coverage_gateway.rs`, `semver_gateway.rs`,
   `resolution_gateway.rs`, `project_inspection.rs`, `coverage_port.rs`,
   `nextest_port.rs`, `mutation_test_port.rs` y `semver_port.rs`. Construyen y
   ejecutan las fases del contenedor; los puertos reciben `&RustGateway` concreto,
   así que sin daemon no hay ruta que un test portable pueda tomar. Los parsers
   que sí son puros viven aparte (`coverage_json.rs`, `nextest_junit.rs`,
   `semver_output.rs`, `mutation_outcomes.rs`) y siguen midiéndose. Recibos:
   [`M3-runtime.json`](validation/M3-runtime.json) y
   [`M3-rust-security.json`](validation/M3-rust-security.json).
4. Store durable macOS ARM64 y su publicación:
   `crates/mcp-server/src/stdio/quality_artifacts.rs`,
   `crates/project-adapter/src/mutation_store.rs`, `mutation_port.rs`,
   `quality_artifact_store.rs`, `cargo_vendor.rs` y `filesystem.rs`. ADR-061
   califica solo macOS ARM64/APFS: fuera de ese host `NativeQualityArtifactStore`
   no tiene constructor, por lo que las rutas de publicación son inalcanzables en
   Linux. Recibos: [`M3-runtime.json`](validation/M3-runtime.json) y
   [`M3-06-rollback.json`](validation/M3-06-rollback.json).
5. Entrypoints de host: `crates/mcp-server/src/stdio.rs` —ensamblado del servidor
   sobre transporte stdio real, store nativo y runtime Docker— y
   `crates/mcp-server/src/main.rs` —dispatch de argv del binario—, más
   `scripts/m3-inspector-session.mjs`, que conduce una sesión MCP contra un
   servidor real. Recibos: [`M3-runtime.json`](validation/M3-runtime.json) y
   [`M3-full-gate.json`](validation/M3-full-gate.json).

Los módulos de herramienta (`stdio/nextest.rs`, `coverage.rs`, `semver.rs`,
`mutation.rs`, `mutation_test.rs`, `tasks.rs`, `resources.rs`) no se excluyen:
su validación de opciones, sus conversiones DTO, sus proyecciones y su gramática
de URI son puras y se prueban en el propio módulo.

El análisis Python declara las versiones compatibles 3.11, 3.12, 3.13 y 3.14.
`crates/catalog-adapter/src/schema.sql` es DDL de SQLite, no PL/SQL de Oracle;
`.sql` se retira por tanto de los sufijos del analizador PL/SQL. Un futuro archivo
`.plsql` sí activará ese analizador y deberá aportar su configuración Oracle real.

La cifra de SonarCloud representa los tests Rust portables y el control de
arquitectura Python que se ejecutan en Ubuntu. No incluye doctests ni los gates
full, Docker, macOS network-deny, E5/ORT/LanceDB, clientes reales o pruebas nativas
de otras plataformas. Esos alcances conservan su evidencia separada en este
documento y en `docs/validation/`.

`.github/workflows/release-candidate.yml` solo admite dispatch manual desde un tag
de versión existente. Para 0.1.0 debe construir únicamente core para
`aarch64-apple-darwin`, generar closure target-specific, SBOM SPDX, notices,
manifest y SHA-256, instalar/verificar el archive y probar `version`, doctor pasivo,
discovery, trece tools y denegaciones estructuradas. Después crea provenance OIDC y
un prerelease en borrador. No publica en crates.io ni contiene modelo, ORT, LanceDB,
catálogo, trust, fixtures, Docker o toolchain. El draft no es una release soportada.
Para 0.1.0, el run `33948798048` pasó y el draft se promovió solo después de
verificar la descarga, hashes, attestations y smoke independientes; véase el
[recibo público](validation/m1-17-public-release.json).

```text
python3 scripts/gate.py core
RUST_MCP_TEST_SOCKET=/ruta/docker.sock RUST_MCP_E5_DIR=/ruta/e5/onnx ORT_LIB_LOCATION=/ruta/ort python3 scripts/gate.py full
```

El reporte por defecto queda en `target/gate-report.json`; `--report PATH` permite
conservar un artifact de validación. El schema v2 registra inicio/fin UTC, comando,
duración, estado y conteos directos por etapa. Los reportes históricos anteriores
conservan sus timestamps/conteos derivados y no se reescriben.
Un error o prerequisito ausente produce exit no cero. No se aceptan Python -O ni
sustituciones del toolchain. Cargo utiliza CARGO_INCREMENTAL=0, --locked --offline.

| Entorno | Evidencia 0.1.0 | Alcance |
| --- | --- | --- |
| macOS26.6.2/APFS ARM64, Rust1.98.1 | Host positivo core + full `local`; único artifact 0.1.0 publicado | E5/ORT/LanceDB solo en full desde fuente |
| Docker/Linux ARM64, runc/cgroupsv2 | Guest de ejecución aprobado | No es host/artifact Linux nativo |
| Linux x86_64 | CI portable/fail-closed | Sin capability positiva ni artifact 0.1.0 |
| Windows x86_64 | CI portable/fail-closed | Sin adapter reparse-safe positivo ni artifact 0.1.0 |
| Linux ARM64, macOS x86_64, Windows ARM64 | No anunciados | Fuera de artifacts 0.1.0 |

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
la redistribución de modelo/ORT/LanceDB sigue fuera de 0.1.0. `deny licenses` no
forma parte del gate M0 ni sustituye el closure legal. El archive core necesita
inventario y notices exactos para su target; los assets excluidos conservan sus
limitaciones para quien construya `local`. El benchmark acotado ya existe, sin
afirmar superioridad general ni utilidad de agente.
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
Guarda logs y recibo vigente en `target/m3-rust-security/`; no instala ni selecciona otra
imagen. `test-execution.sh` conserva exclusivamente la integración de probes M0.

El stage rust-security incluye ahora cuatro tests exactos secuenciales: transferencia
benigna, seis escenarios adversos del gateway, inspección MCP real y cierre por
EOF/cancel durante calibración. Rechaza cero tests ejecutados. Conserva recibos
`target/m3-rust-security/calibration.json` y `mcp-inspection.json`; no debe ejecutarse
en paralelo con otros jobs Docker del gateway (startup rechaza objetos existentes).

M1-02 reutiliza el test MCP real para ambas inspecciones en una sesión. El test
exacto se llama toolchain_inspect_observes_installed_runtime_with_shared_calibration;
target/m3-rust-security/mcp-toolchain.json conserva el inventario y tres ejecuciones.
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

M3-01 añade `scripts/test-m3-runtime.py` como stage full después de M2. Ejecuta
19 selecciones exactas ignoradas de `nextest_runtime` y del módulo MCP
`inspection_runtime::nextest`
con selección exacta, un único test pasado y `--test-threads=1`; hashea fuentes,
config e imagen y persiste estado running/final en `target/m3-runtime/receipt.json`.
El gate Rust existente también emite un recibo source-bound bajo la imagen P02 en
`target/m3-rust-security/receipt.json`. En la evidencia actual rust-security pasa
20/20 y el UnixStream interno de Tokio funciona únicamente mediante el perfil
separado de ADR-064. Los controles negativos
mantienen AF_INET/AF_INET6/connect/pathname-Unix denegados; el gate final M3-01
pasó 19/19 y queda registrado en `validation/M3-01-runtime.json`.

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

## Cierre compuesto ADR-048

El artifact core por sí solo no califica M1. El candidato final debe combinar su
receipt de archive/SBOM/notices/install/smoke con un full gate v2 source-bound del
perfil `local`, Inspector y stock Codex dirigido por modelo sobre los mismos bytes,
reviews finales y la evidencia pública de PR, CI, tag, attestation y release. IUMotion
Labs no publica catálogo oficial 0.1.0 y no se aprovisiona clave Ed25519 de producción.
