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
gate reporting, artifact/smoke, calificador Codex, exportación pública y el
resumen reproducible de presupuestos M4: 80 tests Python en total. Los entrypoints que requieren un host release real permanecen
analizados por Sonar y probados por sus suites, pero se excluyen solo del porcentaje
de cobertura; su evidencia end-to-end es separada y candidate-bound.

`sonar.coverage.exclusions` nombra cada archivo individualmente —nunca un crate
entero ni un comodín— y ninguno sale del análisis: siguen midiéndose fiabilidad,
seguridad, mantenibilidad y duplicación. Las exclusiones restantes son programas de operación/calificación, cuyo camino
end-to-end necesita un host preparado; no se afirma que sus suites unitarias
sean inejecutables. Cada grupo declara su evidencia adicional:

Los cinco módulos `*_native.rs` de `execution-adapter` están declarados
explícitamente como tests en `sonar.test.inclusions`: el compilador solo los incluye
con `cfg(test)` y contienen los oráculos nativos de Docker, incluido el test Miri
ignorado que ejecuta el gate M4. Esta clasificación evita contabilizar código de
prueba como producto sin excluirlo del análisis.

1. Programas de calificación maintainer-only: `scripts/codex-model-qualifier.py`,
   `scripts/release-artifact.py`, `scripts/release-smoke.py` y
   `scripts/verify-vendor.py`. Requieren host release Darwin real, Docker/Codex o
   ambos. Recibos: [`M3-full-gate.json`](validation/M3/full-gate.json) y los
   receipts de release en `docs/validation/`.
2. Sondas M2 sobre Docker: `scripts/probe-m2-cargo-fix.py`,
   `probe-m2-fix-socket-mask.py`, `probe-m2-guest-staging.py`,
   `probe-m2-offline-registry.py`, `probe-m2-vendor-data.py` y
   `probe-m2-write-primitives.py`. Su única ruta ejecutable crea volúmenes y
   contenedores contra la imagen aprobada en un daemon local; el runner Ubuntu no
   tiene ni el socket ni la imagen. Recibos: los JSON `M2-*` que cada sonda emite
   y [`M3-rust-security.json`](validation/M3/rust-security.json).
3. Clientes reales: `scripts/m3-inspector-session.mjs` y
   `scripts/m4-inspector-session.mjs`, que conducen sesiones MCP contra un
   servidor con runtime/store nativos. Su evidencia está en
   [`M3-runtime.json`](validation/M3/runtime.json),
   [`M3-full-gate.json`](validation/M3/full-gate.json) y
   [`M4-clients.json`](validation/M4/clients.json).

Ningún archivo Rust de producto está excluido del porcentaje de cobertura.
Los caminos que solo ejecutan los gates nativos pueden reducir la cifra portable;
ese límite de medición se conserva visible. Tener ramas que necesitan Docker o
macOS no justifica ocultar las ramas portables del mismo archivo.

La revisión de prerrequisitos M4 detectó exclusiones excesivas y una justificación
incorrecta en los anteriores grupos 3/5. Se retiraron todas las exclusiones Rust
de producto (16 rutas) y se añadieron
oráculos para impedir su reintroducción mediante exclusiones exactas o glob.
`sonar-project.properties` entra desde M4 en `gate.py::source_inventory`; cambiar
el ámbito de cobertura modifica el hash del input. Los recibos M3 históricos no
se reescriben: no incluían ese archivo y no acreditan una medición Sonar nueva.
Esta corrección no afirma ningún porcentaje ni resultado remoto nuevo; la próxima
corrida Sonar debe medir el ámbito ampliado sin reducir el umbral de calidad.

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
[recibo público](validation/M1/17-public-release.json).

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

[M1-10](validation/M1/10.md) conserva full15/15 con hashes inmutables, previo al
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
[Evidencia](validation/M1/11.md).

M1-12 añade `scripts/test-crate-search.py` como etapa17 de full:2 tests ordinarios y1 nativo ignorado ejecutado explícitamente, con E5/Lance bajo network deny.
La [evidencia M1-12](validation/M1/12.md) registra el gate focalizado; el full conjunto final sigue requerido antes de cierre/release.

## M1-13 — Inspección paginada MCP

Full incorpora `scripts/test-crate-inspect.py` como etapa18: dos tests ordinarios,
compilados con feature local y ejecutados bajo network deny macOS. Comprueban
páginas SQLite, hechos desconocidos y continuación ligada a generación; inspect
no necesita ni ejecuta embeddings para estas consultas. El [gate M1-13](validation/M1/13.md)
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

## M4 — calificación local completa

La calificación M4 ejecutó 19 etapas core y 33 full; ese es el conteo que
acreditan sus recibos y no se reescribe. La configuración actual de
`scripts/gate.py` ejecuta 22 core y 36 full tras las tres etapas core que añade
M5 (ver [M5](#m5--etapas-de-gate-imagen-y-fixture-de-benchmarks)). Las secciones
M1/M3 anteriores conservan sus conteos históricos. Comandos, con inputs ya
aprovisionados:

```sh
python3 -B scripts/gate.py core --report target/M4-core-gate.json
python3 -B scripts/gate.py full --report target/M4-full-gate.json
```

Full exige un host macOS ARM64, propietario único del daemon Docker,
`RUST_MCP_TEST_SOCKET`, `RUST_MCP_E5_DIR` y `ORT_LIB_LOCATION` explícitos. No
instala ni actualiza inputs. El full M4 [aprobado](validation/M4/full-gate.json)
es una ejecución monolítica 33/33 posterior a la remediación del PR, sobre 990
inputs y sin cambios de fuentes durante el gate. Los intentos anteriores y la
recuperación local de E5 permanecen documentados en
[recuperación](validation/M4/e5-local-recovery.json) y el
[driver registrado](validation/M4/full-gate-resume-driver.py). El full y los
clientes vigentes comparten los mismos 990 inputs; el core histórico conserva 987.

Las etapas adicionales incluyen imagen alterada, inventario pasivo y
`python3 -B scripts/test-m4-runtime.py`. Este último invoca 19 selecciones nativas
con `--exact --ignored --nocapture --test-threads=1`, exige exactamente un test
pasado por selección y registra fuentes/config/imágenes/logs/cleanup. Las 19
usan la imagen final M4 `25ed…`; una selección ejecuta rollback explícito a M3.
Las regresiones M3 conservan sus propios 62 casos e imagen. El
[mapa de hardening](validation/M4/hardening-map.md) enumera cada caso y límite;
[scanner](validation/M4/scanner-native.json) pasó siete oráculos y
[Miri](validation/M4/miri-native.json) 13 clasificaciones y siete admisiones.

G4 se ejecuta aparte mediante `python3 -B scripts/test-m4-clients.py --run`, con
socket explícito y `RUST_MCP_M4_CODEX_SYNC_QUALIFIED=1` sustentado en el
[presupuesto registrado](validation/M4/client-execution.json). Requiere los
clientes previamente instalados: Inspector 2.5.0 y Codex 0.153.0; el intento 6
[pasó](validation/M4/clients.json). No almacena credenciales del cliente en el
repositorio. El [handoff](validation/M4/handoff.md) distingue los resultados
locales de CI/Sonar remotos, que no se ejecutaron para este checkout.

## M5 — etapas de gate, imagen y fixture de benchmarks

### Tres etapas core nuevas

`scripts/gate.py` añade tres etapas al modo **core** —por tanto se ejecutan
también en `full`, que es core más sus etapas nativas— justo después del bloque
M4 y antes de `vendor`:

| Etapa | Modo | Comando exacto |
| --- | --- | --- |
| `m5-helper-fmt` | core (y full) | `cargo fmt --manifest-path fixtures/profile-helper/Cargo.toml --check` |
| `m5-helper-tests` | core (y full) | `cargo test --manifest-path fixtures/profile-helper/Cargo.toml --locked --offline --target-dir target/profile-helper` |
| `m5-vendor-tests` | core (y full) | `python3 -B -m unittest discover -s fixtures/criterion-vendor -p 'test_*.py'` |

Las dos últimas se declaran con `require_test_groups=True`: una etapa que no
ejecuta ningún test es un fallo, no un pase. Con ellas, el conteo de `run(` en
`scripts/gate.py` pasa a **22 etapas core** (`fmt`, `check`, `clippy`, `test`,
`doctests`, `architecture`, `gate-reporting`, `release-artifact-tests`,
`release-smoke-tests`, `codex-qualifier-tests`, `m4-client-harness-tests`,
`m4-safety-harness-tests`, `m4-helper-fmt`, `m4-helper-tests`,
`m4-provisioning-tests`, `m5-helper-fmt`, `m5-helper-tests`, `m5-vendor-tests`,
`vendor`, `cargo-fixtures`, `audit`, `deny`) y **36 en full**, que añade las 14
etapas nativas ya documentadas (`docker-security`, `rust-security`,
`m2-runtime`, `m3-runtime`, `m4-tampered-plugin`, `m4-inventory`, `m4-runtime`,
`audit-data`, `semantic`, `catalog`, `catalog-status`, `crate-search`,
`crate-inspect`, `doctor`). Ese conteo describe la configuración vigente del
script, no una ejecución acreditada: no existe todavía un recibo de gate M5.

### Etapa full para el runtime nativo M5

`full` incorpora `m5-runtime`, que ejecuta `scripts/test-m5-runtime.py` sobre
la imagen admitida. El script descubre las seis selecciones ignoradas de
`performance_native.rs` y las ejecuta una por vez con `--exact --ignored
--test-threads=1`. Comprueba admisión antes de medir y registra sources, fixtures,
logs, resultado y digest del recibo nativo por selección. No aprovisiona ni
reconstruye imágenes. El estado de calificación está en la
[matriz M5](validation/M5/matrix.md); la existencia de la etapa no constituye
por sí sola un gate aprobado.

### Imagen guest M5

La imagen se construye y se recibe con un único comando, que es el procedimiento
completo:

```sh
python3 -B scripts/build-m5-runtime.py
```

El script comprueba **antes de construir** que el tag base
`rust-engineering-runtime:1.98.1-arm64-m4-scanner` resuelve exactamente a
`sha256:25ed3626e710081a571a86a29521eaf2e890e796afd422ba5e409e0ce1891635` y
aborta si no; prepara el contexto con `provision.py` sin acceder a la red;
construye con `--network=none --pull=false`; y verifica sobre la imagen resultante
que los dos binarios existen, que **ninguno** es alcanzable por `PATH`, que los
binarios M3/M4 siguen presentes y que el contexto de construcción no dejó
residuos.

El builder importa: Docker 29.7.2 ya no ofrece el constructor clásico —queda
colgado tras el aviso de deprecación— y BuildKit resuelve un `FROM sha256:…` como
referencia **remota**, que bajo `--network=none` falla con `DeadlineExceeded`. Por
eso el `FROM` nombra la base por tag y no por digest, y por eso el script **no**
fija `DOCKER_BUILDKIT`: no queda un builder alternativo que seleccionar, y el
propio [recibo](validation/M5/provisioning.json) registra la línea
`building with "desktop-linux" instance using docker driver` de BuildKit. La
garantía de digest no se pierde: se comprueba inmediatamente antes de construir y
el id observado queda en el recibo.

El recibo se escribe en `docs/validation/M5/provisioning.json`. La ejecución del
2026-09-08 pasó con `network_used: false`, 46 archivos y 10 769 232 bytes de
contexto, y produjo la imagen `rust-engineering-runtime:1.98.1-arm64-m5` con id
`sha256:e0a5ca1661b3e49d0a3d68ee3cc0963453078d08eb7fc43c30538c16b7998aac`,
que contiene `/opt/perf/bin/cargo-bloat`
(`sha256:e3eaea0d81679b8a14b8b435f54f00c0c952d4c4c5dc9a204fdbe42b3de7326a`) y
`/opt/perf/bin/rust-mcp-profile-helper`
(`sha256:04bd5ab818204b6f91371c3d82df91ab4f55c148c0e0fc802125520acf762dc5`).
Construir esa imagen no la admite en el gateway: la admisión es una decisión
separada con su propia calificación nativa
([ADR-075](adr/ADR-075-m5-runtime-provisioning.md),
[imagen M5](../fixtures/rust-runtime/m5/README.md)).

### Fixture de benchmarks: materializar el vendor antes de compilar

`fixtures/criterion-vendor/` versiona **archivos `.crate` fijados**, no un árbol
extraído: 52 paquetes de crates.io que forman el cierre transitivo completo de
`criterion 0.8.2`. El directory source que Cargo necesita es *generado* y está en
`.gitignore`, así que `fixtures/benchmark` **no compila desde un checkout limpio**
hasta ejecutar una vez:

```text
python3 -B fixtures/criterion-vendor/materialize.py
```

El script verifica el `sha256` de cada archivo contra `INVENTORY.json` antes de
extraer nada, aplica las mismas reglas de seguridad de archivo que
`fixtures/rust-runtime/m4-scanner/provision.py`, escribe cada
`.cargo-checksum.json` y nunca accede a la red. `--verify-only` comprueba sin
escribir. La etapa `m5-vendor-tests` ejercita esas comprobaciones; no sustituye a
la materialización, que sigue siendo un paso explícito del operador.
