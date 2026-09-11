# Rust Engineering MCP

[![CI](https://github.com/pharos-lang/rust-engineering-mcp/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/pharos-lang/rust-engineering-mcp/actions/workflows/ci.yml)
[![Quality Gate](https://sonarcloud.io/api/project_badges/measure?project=pharos-lang_rust-engineering-mcp&metric=alert_status)](https://sonarcloud.io/summary/new_code?id=pharos-lang_rust-engineering-mcp)
[![Security Rating](https://sonarcloud.io/api/project_badges/measure?project=pharos-lang_rust-engineering-mcp&metric=security_rating)](https://sonarcloud.io/summary/new_code?id=pharos-lang_rust-engineering-mcp)
[![Reliability Rating](https://sonarcloud.io/api/project_badges/measure?project=pharos-lang_rust-engineering-mcp&metric=reliability_rating)](https://sonarcloud.io/summary/new_code?id=pharos-lang_rust-engineering-mcp)
[![Maintainability Rating](https://sonarcloud.io/api/project_badges/measure?project=pharos-lang_rust-engineering-mcp&metric=sqale_rating)](https://sonarcloud.io/summary/new_code?id=pharos-lang_rust-engineering-mcp)
[![Coverage](https://sonarcloud.io/api/project_badges/measure?project=pharos-lang_rust-engineering-mcp&metric=coverage)](https://sonarcloud.io/summary/new_code?id=pharos-lang_rust-engineering-mcp)
[![Rust 1.98.1](https://img.shields.io/badge/Rust-1.98.1-000000?logo=rust)](rust-toolchain.toml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE)

Rust Engineering MCP conecta agentes compatibles con [Model Context Protocol
(MCP)](https://modelcontextprotocol.io/) con proyectos Rust locales. Expone
operaciones estructuradas para inspeccionar un workspace, ejecutar comprobaciones
de calidad dentro de un runtime controlado, consultar diagnósticos y trabajar con
un catálogo local de crates.

El servidor usa transporte MCP por `stdio`. Las trece tools de la release
`0.1.0` observan y validan sin modificar el source. El checkout `0.3.0`
registra 31 tools: las 18 de M1/M2, las cuatro tools de calidad M3, cinco tools
M4 y cuatro tools de rendimiento M5. M4 y M5 están cerrados localmente, sin
integración remota ni release. El checkout no forma una release.

> [!IMPORTANT]
> La versión estable actual es `0.3.0`. GitHub Releases publica un único binario core
> soportado para Apple Silicon (`aarch64-apple-darwin`); la ejecución completa se ha
> calificado localmente en macOS 26 y APFS. La CI compila y prueba el código en Linux,
> macOS y Windows, pero eso no amplía las garantías del sandbox o del filesystem ni
> anuncia binarios para esas otras plataformas.

## Funcionalidades

| Área | Tool | Uso |
| --- | --- | --- |
| Proyecto | `rust.project.open` | Abre una raíz previamente autorizada y devuelve un `project_ref` temporal. |
| Proyecto | `rust.project.inspect` | Inspecciona packages, targets, features, perfiles y dependencias declaradas. |
| Toolchain | `rust.toolchain.inspect` | Informa las versiones, targets y componentes instalados en el runtime aprobado. |
| Calidad | `rust.check` | Ejecuta `cargo check` con opciones tipadas y diagnósticos estructurados. |
| Calidad | `rust.fmt.check` | Comprueba formato sin modificar archivos. |
| Calidad | `rust.clippy` | Ejecuta Clippy con perfiles cerrados. |
| Calidad | `rust.test` | Ejecuta tests acotados y conserva el resultado del harness. |
| Calidad (M3, desarrollo) | `rust.test.nextest` | Tool 19; modo síncrono con la regla qualified-short (hasta 60 s). Calificada en Docker: 19/19 selecciones. |
| Calidad (M3, desarrollo) | `rust.coverage` | Tool 20; cobertura tipada y acotada. Calificada en Docker: 8/8 selecciones. |
| Calidad (M3, desarrollo) | `rust.semver.check` | Tool 21; comparación SemVer contra baseline. Calificada en Docker: 18/18 selecciones. |
| Calidad (M3, desarrollo) | `rust.mutation.test` | Tool 22; mutation testing con bundle acotado. Calificada en Docker: 10/10 selecciones. |
| Seguridad (M4, desarrollo) | `rust.deny` | Tool 23; audit y cargo-deny offline sobre una captura y policy del host. |
| Seguridad (M4, desarrollo) | `rust.unsafe.scan` | Tool 24; inventario sintáctico acotado de construcciones unsafe. |
| Supply chain (M4, desarrollo) | `rust.supply_chain.inspect` | Tool 25; facts de resolución, audit, deny y catálogo con provenance explícita. |
| Calidad (M4, desarrollo) | `rust.quality.gate.v2` | Tool 26; gate `strict` o `release` sobre una captura compartida. |
| Seguridad (M4, desarrollo) | `rust.miri` | Tool 27; evidencia tipada de Miri sobre tests seleccionados. |
| Rendimiento (M5, desarrollo) | `rust.benchmark.run` | Tool 28; mide los benchmarks Criterion que el proyecto ya tiene y publica las muestras crudas como dataset privado, junto al árbol de salida del harness y al `stdout`/`stderr` de cada repetición como artifacts propios. Calificada localmente. |
| Rendimiento (M5, desarrollo) | `rust.benchmark.compare` | Tool 29; compara dos datasets propios con un método estadístico congelado. No ejecuta nada. |
| Rendimiento (M5, desarrollo) | `rust.profile.flamegraph` | Tool 30; muestreo en CPU de un binario del proyecto; exige la capability de profiling del host. |
| Rendimiento (M5, desarrollo) | `rust.binary.bloat` | Tool 31; tamaño exacto del binario más la atribución estimada del analizador fijado. |
| Seguridad | `rust.dependencies.audit` | Contrasta `Cargo.lock` con un snapshot RustSec suministrado por el host. |
| Diagnóstico | `rust.diagnostics.explain` | Obtiene la explicación de un código `rustc`, por ejemplo `E0502`. |
| Calidad | `rust.quality.gate` | Ejecuta un gate `fast` o `standard` y devuelve el estado de cada etapa. |
| Catálogo | `rust.catalog.status` | Informa disponibilidad, identidad y frescura del catálogo local. |
| Catálogo | `rust.crate.search` | Busca crates en modo léxico, semántico o híbrido. |
| Catálogo | `rust.crate.inspect` | Consulta versiones, features, dependencias y advisories registrados. |
| Mutación (desarrollo) | `rust.manifest.patch` | Previsualiza, confirma y recupera ediciones TOML semánticas cerradas. |
| Mutación (desarrollo) | `rust.fmt.apply` | Aplica el candidato exacto producido y verificado por rustfmt. |
| Mutación (desarrollo) | `rust.fix.apply` | Aplica un candidato de `cargo fix` y lo comprueba en el sandbox. |
| Mutación (desarrollo) | `rust.dependency.add` | Añade una dependencia crates.io a un package miembro con resolución offline aprobada. |
| Mutación (desarrollo) | `rust.dependency.remove` | Elimina una dependencia seleccionada y resuelve offline el candidato. |

Los contratos completos, límites y ejemplos de respuesta están en
[`docs/tools.md`](docs/tools.md).

Las cinco tools M4 forman parte de `tools/list` y están calificadas localmente en
macOS ARM64 con el runtime Docker Linux ARM64 fijado. Sus timeouts por defecto son
120 s para deny/unsafe/supply y 300 s para gate v2/Miri, por lo que requieren MCP
Tasks. Una selección de hasta 60 segundos puede usar el camino síncrono; en
`rust.quality.gate.v2` se limita a `strict` sin mutation. `release` y mutation
requieren Tasks. Consulta su [alcance y límites](docs/tools.md#contratos-m4-calificados-localmente)
y el [handoff de evidencia](docs/validation/M4-handoff.md).

Las cuatro tools M5 miden rendimiento y tamaño sin modificar el checkout.
`rust.benchmark.run`, `rust.profile.flamegraph` y `rust.binary.bloat` exigen la
imagen guest M5 y datos offline autenticados por el host. Para Criterion,
`rust.benchmark.run` usa la captura de vendor de ADR-078; profile y bloat usan el
`CargoVendorSnapshot` configurado por el host;
`rust.profile.flamegraph` exige además `--allow-profiling user-space-sampling`;
`rust.benchmark.compare` no ejecuta nada y opera sobre dos datasets que un `run`
previo del mismo proyecto ya publicó. Ninguna admite MCP Tasks: en las tres que
aceptan `execution_mode`, `task` devuelve `TASKS_REQUIRED` como resultado
declarado; `rust.benchmark.compare` no tiene modo de ejecución.

> [!WARNING]
> M5 está **calificado localmente** (sin integración remota, PR, tag ni release):
> suite nativa 6/6 ([gate nativo](docs/validation/M5-native-gate.json)), matriz
> de clientes ([recibo](docs/validation/M5-clients.json)) y gates `core`/`full`
> sobre las fuentes finales. `rust.benchmark.compare` no emite veredictos
> direccionales: publica efecto, intervalo y razones declaradas. Estado y límites
> en la [matriz M5](docs/validation/M5-matrix.md).
> Sus contratos completos están en [`docs/tools.md`](docs/tools.md#contratos-m5--medición-de-rendimiento).

Los Resources normalizados no sustituyen una revisión de privacidad. Los HTML de
cobertura y diffs de mutation autorizados pueden contener source del proyecto,
incluidos secretos presentes en esos archivos; se almacenan como artifacts
privados y no se promete redacción universal del source autorizado.

Las mutaciones máximas pueden consumir memoria considerable: el ciclo nativo de
128 archivos/16 MiB midió aproximadamente 932 MiB de RSS después de optimizar el
journal; no es un límite del proceso MCP completo. Consulta los [límites y la
medición](docs/validation/M2/matrix.md). M2 emite eventos operativos acotados por
stderr, sin source, rutas ni credenciales y sin colector adicional; su retención
la controla el host. stdout queda reservado al protocolo.
El store privado admite hasta 128 journals/256 MiB; la admisión reserva 48 MiB
para recovery y 1 MiB para crecimiento, dejando hasta 207 MiB retenidos. Un journal
corrupto puede bloquear ese store. La [guía de recuperación](docs/client-configuration.md#planes-receipts-y-recovery)
explica cómo conservar la evidencia y continuar en copias y estado nuevos.

## Requisitos

Para compilar el servidor:

- Git;
- Rust y Cargo `1.98.1`;
- las dependencias fijadas por `Cargo.lock`.

Para abrir proyectos en el entorno actualmente calificado se necesita macOS 26 o
posterior, Apple Silicon y un volumen APFS. Las tools que ejecutan Cargo requieren,
además, Docker y la imagen Linux ARM64 exacta aprobada por el proyecto. El catálogo
es opcional y requiere que el host proporcione sus archivos locales de datos y
confianza.

Consulta la [matriz de compatibilidad](docs/compatibility.md) antes de usar el
servidor en otro sistema operativo o filesystem.

## Instalar la release macOS ARM64

Descarga el archive y `SHA256SUMS` desde la
[release v0.3.0](https://github.com/pharos-lang/rust-engineering-mcp/releases/tag/v0.3.0),
verifica los bytes y extráelos en un directorio nuevo:

```bash
shasum -a 256 -c SHA256SUMS
tar -xzf rust-engineering-mcp-v0.3.0-aarch64-apple-darwin.tar.gz
cd rust-engineering-mcp-v0.3.0-aarch64-apple-darwin
./rust-engineering-mcp version --json
./rust-engineering-mcp doctor --json
```

La release también adjunta un receipt de smoke y attestations verificables con
`gh attestation verify --repo pharos-lang/rust-engineering-mcp <asset>`.

## Compilar desde el código fuente

```bash
git clone https://github.com/pharos-lang/rust-engineering-mcp.git
cd rust-engineering-mcp
cargo build --release --locked -p rust-engineering-mcp
```

El binario queda en:

```text
target/release/rust-engineering-mcp
```

Comprueba el binario y su configuración pasiva:

```bash
./target/release/rust-engineering-mcp version --json
./target/release/rust-engineering-mcp doctor --json
```

`doctor` no instala, descarga ni repara componentes. Devuelve `warning` cuando una
capacidad opcional no está configurada. Usa `--help` para consultar todos los
comandos y opciones disponibles.

## Iniciar el servidor

La configuración mínima autoriza una o más raíces físicas. Usa siempre rutas
absolutas:

```bash
./target/release/rust-engineering-mcp serve --stdio \
  --root /ruta/absoluta/al/proyecto
```

Se pueden repetir hasta 16 argumentos `--root`. Sin roots, ninguna tool puede abrir
proyectos. Un `project_ref` pertenece al proceso actual, caduca por inactividad y
deja de ser válido al reiniciar el servidor.

Esta configuración mínima permite abrir proyectos, pero las operaciones que ejecutan
Rust fallarán de forma cerrada hasta que el host configure el runtime aprobado.

## Conectar un agente

Rust Engineering MCP puede configurarse en clientes que admitan servidores MCP
locales mediante `stdio`. La tabla distingue entre compatibilidad verificada por el
proyecto y configuraciones basadas en el soporte `stdio` documentado por cada
cliente.

| Cliente | Configuración | Evidencia actual |
| --- | --- | --- |
| Codex | [CLI o `config.toml`](docs/client-configuration.md#codex) | Codex 0.153.0 stock calificó el camino síncrono M4 para las cinco tools y un turno model-directed con las cinco en `passed`; el cliente no declaró Tasks. [Recibo M4](docs/validation/M4-clients.json). |
| Claude Code | [CLI o `.mcp.json`](docs/client-configuration.md#claude-code) | M2: Claude Code 2.1.260, Sonnet 5 medium, cinco preview/commit y receipt final; [PASS intento 5](docs/validation/M2/clients.json), con renovación de referencias explícita en el prompt. M5: Claude Code 2.1.267 (`claude-sonnet-5`) como cliente agentic restringido a MCP: cuatro rechazos declarados en docker-free y, en runtime, dos mediciones propias, comparación `inconclusive`, rechazo `NOT_A_DATASET` y lectura nativa de una Resource ligada por hash; [recibo M5](docs/validation/M5-clients.json). |
| Gemini CLI | [`settings.json`](docs/client-configuration.md#gemini-cli) | Configuración documentada; calificación de este MCP pendiente. |
| Cursor | [`.cursor/mcp.json`](docs/client-configuration.md#cursor) | Configuración documentada; calificación de este MCP pendiente. |
| VS Code / GitHub Copilot | [`.vscode/mcp.json`](docs/client-configuration.md#vs-code-y-github-copilot) | Configuración documentada; calificación de este MCP pendiente. |
| MCP Inspector | [Web, CLI o TUI](docs/client-configuration.md#mcp-inspector) | Inspector 2.5.0 calificó 27 tools y, para M4, positivos, negativos, cancelación Tasks y cinco Resources. [Recibo M4](docs/validation/M4-clients.json). |

La [guía de configuración por cliente](docs/client-configuration.md) contiene los
archivos completos, comandos de verificación y enlaces a la documentación oficial.
Estos son los dos casos de inicio rápido más habituales.

### Codex

Codex puede registrar el servidor directamente desde la CLI:

```bash
codex mcp add rust-engineering -- \
  /ruta/absoluta/rust-engineering-mcp \
  serve --stdio \
  --root /ruta/absoluta/al/proyecto
```

También puedes añadirlo en `~/.codex/config.toml` o en `.codex/config.toml` de un
proyecto confiable:

```toml
[mcp_servers.rust_engineering]
command = "/ruta/absoluta/rust-engineering-mcp"
args = ["serve", "--stdio", "--root", "/ruta/absoluta/al/proyecto"]
startup_timeout_sec = 45
tool_timeout_sec = 300
default_tools_approval_mode = "prompt"
```

Reinicia el cliente después de guardar la configuración y comprueba la conexión con
`codex mcp list` o `/mcp`. La [documentación oficial de Codex sobre
MCP](https://developers.openai.com/codex/mcp/) describe las demás opciones de
configuración.

### Claude Code

Registra el servidor en el proyecto actual desde la CLI de Claude Code:

```bash
claude mcp add --scope project rust-engineering -- \
  /ruta/absoluta/rust-engineering-mcp \
  serve --stdio \
  --root /ruta/absoluta/al/proyecto
```

Ejecuta `claude mcp get rust-engineering` o abre `/mcp` para revisar el estado. Los
servidores compartidos mediante `.mcp.json` requieren la aprobación del usuario en
un workspace confiable.

## Habilitar ejecución Rust

`rust.project.inspect`, `rust.toolchain.inspect`, `rust.check`, `rust.fmt.check`,
`rust.clippy`, `rust.test`, `rust.test.nextest`, `rust.dependencies.audit`,
`rust.diagnostics.explain` y `rust.quality.gate` usan un runtime Docker aprobado.
Cargo puede ejecutar `build.rs`, proc macros y código de tests, por lo que conviene
mantener aprobación interactiva en el cliente MCP.

La configuración del host utiliza el grupo completo de flags siguiente:

```bash
./target/release/rust-engineering-mcp serve --stdio \
  --root /ruta/absoluta/al/proyecto \
  --docker /ruta/absoluta/al/cliente/docker \
  --docker-socket /ruta/absoluta/docker.sock \
  --state-root /ruta/absoluta/a/estado-privado \
  --rust-image sha256:384a1742ecc53cdd3a9c0bf36c6f8b66db73ddd118aeeae6e55654ea998ae36a
```

El servidor acepta únicamente esa identidad de imagen. La imagen no está publicada
en un registry; las instrucciones y recibos para construir y verificar el fixture
están en [`fixtures/rust-runtime/README.md`](fixtures/rust-runtime/README.md). El
runtime no descarga ni aprovisiona imágenes durante una sesión MCP.

Para `rust.dependencies.audit`, añade juntos un snapshot RustSec local y su hash:

```text
--rustsec-snapshot /ruta/absoluta/rustsec.json
--rustsec-sha256 sha256:<64-hex>
```

Las tools M4 reutilizan el runtime y aceptan un vendor Cargo
offline autenticado. `rust.deny` requiere además el snapshot RustSec y una policy
del host; supply chain y gate v2 consumen esos mismos inputs cuando están
configurados y marcan incompleta su evidencia requerida cuando faltan:

```text
--cargo-vendor-dir /ruta/absoluta/al/vendor
--cargo-vendor-tree-sha256 sha256:<64-hex>
--security-policy /ruta/absoluta/security-policy.json
--security-policy-sha256 sha256:<64-hex>
```

Cada opción forma un par obligatorio. El vendor y la policy deben quedar fuera de
las roots autorizadas del proyecto; el runtime relee y verifica sus fingerprints.
La imagen M4 admitida por identidad inmutable es
`sha256:25ed3626e710081a571a86a29521eaf2e890e796afd422ba5e409e0ce1891635`.
La calificación local de las cinco tools usa esa imagen; no amplía la release
estable ni la matriz más allá de macOS ARM64 con guest Docker Linux ARM64.

### Habilitar las tools M5

Las tools de rendimiento exigen la imagen guest M5 **y solo esa**; cualquier otro
digest devuelve `unavailable` antes de crear contenedor alguno:

```text
--rust-image sha256:e0a5ca1661b3e49d0a3d68ee3cc0963453078d08eb7fc43c30538c16b7998aac
```

`rust.benchmark.run` resuelve su harness **offline**, así que necesita datos de
vendor autenticados por el host. Admite dos formas y ninguna descarga nada:

- el par de vendor Cargo ya documentado arriba (`--cargo-vendor-dir` y
  `--cargo-vendor-tree-sha256`), suficiente para un harness pequeño;
- una **captura de vendor** ([ADR-078](docs/adr/ADR-078-offline-vendor-capture.md)),
  para un cierre grande como el de `criterion`, que no cabe en el contrato
  `SourceBundle` ni por cuotas ni por alfabeto de rutas:

```text
--vendor-capture /ruta/absoluta/al/artifact
--vendor-capture-tree-sha256 sha256:<64-hex>
```

También es un par obligatorio, también debe quedar fuera de las roots del
proyecto, y el servidor **no captura nunca por su cuenta**: relee el artifact,
recalcula su digest de forma incremental y lo rechaza si no coincide con el
declarado. Cuando ambos están configurados manda la captura. Sin ninguno de los
dos, la tool responde que faltan datos offline en lugar de descargar o sustituir
el harness. `rust.profile.flamegraph` y `rust.binary.bloat` siguen usando el
árbol `--cargo-vendor-dir` para construir el binario que miden.

El profiling exige además una concesión explícita del host, con un único valor
admitido:

```text
--allow-profiling user-space-sampling
```

Concede exactamente el muestreo de espacio de usuario sobre el proceso hijo que
lanza el perfilador y sus hilos, con una sola syscall añadida al perfil seccomp.
No añade capabilities Linux, no usa contenedores privilegiados, no ejecuta `sudo`
y no toca `perf_event_paranoid`. Cualquier otro valor, repetir la opción o
usarla sin el grupo Docker completo hace inválida la invocación de `serve`. Sin
la concesión, `rust.profile.flamegraph` responde `blocked` con
`PROFILING_NOT_AUTHORIZED` antes de crear ningún contenedor. La capability es por
servidor y se retira quitando la bandera y reiniciando; ninguna otra tool cambia
de comportamiento por concederla. Detalles en la
[guía por cliente](docs/client-configuration.md#configurar-las-tools-m5).

## Configurar el catálogo local

El servidor no descarga ni actualiza catálogos durante una sesión MCP. Si ya tienes
un catálogo firmado y un archivo de confianza, añade:

```text
--catalog-store /ruta/absoluta/al/store
--catalog-trust /ruta/absoluta/trust.json
```

La búsqueda semántica requiere compilar el binario con `--features local` y añadir
el modelo y el índice:

```text
--catalog-model-dir /ruta/absoluta/al/modelo-e5
--catalog-index-store /ruta/absoluta/al/indice-lance
```

Sin modelo o índice, la búsqueda puede usar el modo léxico cuando SQLite esté
disponible. La administración del catálogo se realiza fuera del runtime MCP con los
comandos `catalog status`, `catalog import`, `catalog sync` y
`catalog rebuild-index`. Consulta el [formato de bundles](docs/catalog-bundle-format.md)
y la [referencia CLI](docs/tools.md#cli-de-catálogo-m1-10-no-tool-mcp).

## Flujo recomendado para un agente

1. Llama `rust.project.open` con la ruta absoluta autorizada.
2. Conserva el `project_ref` devuelto para las llamadas siguientes.
3. Usa `rust.project.inspect` antes de seleccionar packages, targets o features.
4. Ejecuta la comprobación más pequeña que responda la pregunta: formato, check,
   Clippy, test o audit.
5. Usa `rust.quality.gate` cuando necesites una evaluación compuesta.
6. Lee los Resources devueltos cuando una tool publique logs acotados.
7. Reabre el proyecto si cambió el código o caducó la referencia.

Ejemplos de solicitudes para un agente:

```text
Abre /ruta/absoluta/al/proyecto e inspecciona sus packages y features.

Ejecuta rust.check sobre el workspace abierto y resume los diagnósticos con sus spans.

Comprueba formato y Clippy estricto sin modificar ningún archivo.

Ejecuta el quality gate standard y enumera las etapas fallidas o bloqueadas.

Busca crates de serialización compatibles con Rust 1.98.1 en el catálogo local.
```

## Seguridad

- Autoriza únicamente roots necesarias y usa rutas absolutas.
- No uses el servidor con repositorios no confiables en esta versión de desarrollo.
- Mantén confirmación interactiva para tools que ejecutan Cargo.
- No interpretes `--offline` como aislamiento de red; el gateway exige controles del
  sandbox y falla cerrado si no puede verificarlos.
- `stdout` está reservado al protocolo MCP; los logs operativos se escriben en
  `stderr`.
- El servidor no hereda automáticamente todo el entorno ni instala componentes.

Lee el [modelo de seguridad](docs/security-model.md) y la [política para reportar
vulnerabilidades](SECURITY.md) antes de habilitar ejecución.

## Solución de problemas

| Síntoma | Qué revisar |
| --- | --- |
| El cliente no inicia el servidor | Ejecuta `rust-engineering-mcp version --json`, usa una ruta absoluta al binario y revisa `stderr`. |
| `project.open` devuelve `unavailable` | Comprueba macOS/APFS, que la root fue autorizada y que no contiene symlinks en la ruta física. |
| Una tool devuelve `SANDBOX_DENIED` | Verifica que se proporcionó el grupo completo de flags Docker y la imagen aprobada. |
| `rust.dependencies.audit` no está disponible | Proporciona juntos el snapshot RustSec y el SHA-256 esperado. |
| El catálogo aparece `not_configured` | Proporciona juntos `--catalog-store` y `--catalog-trust`. |
| La búsqueda semántica se degrada a léxica | Compila con `--features local` y revisa modelo e índice con `doctor --json`. |
| Un `project_ref` dejó de funcionar | Reabre el proyecto; las referencias caducan y no sobreviven al proceso. |
| El cliente parece recibir texto que no es MCP | No redirijas logs a `stdout` ni inicies el binario mediante scripts que impriman allí. |

## Documentación

- [Configuración de clientes MCP](docs/client-configuration.md)
- [Tools y CLI](docs/tools.md)
- [Compatibilidad](docs/compatibility.md)
- [Modelo de seguridad](docs/security-model.md)
- [Configuración de CI](docs/ci.md)
- [Estado verificable del proyecto](docs/implementation-status.md)
- [Arquitectura](docs/architecture.md)
- [ADRs](docs/adr/README.md)
- [Changelog](CHANGELOG.md)

Los documentos de arquitectura, decisiones y estado conservan los detalles internos
de implementación y planificación. Este README se limita a la instalación, operación
y uso público del MCP.

## Contribuir

Consulta [`CONTRIBUTING.md`](CONTRIBUTING.md). Antes de abrir un PR, ejecuta al
menos:

```bash
cargo fmt --all --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
```

Los gates completos y sus prerrequisitos están documentados en [`docs/ci.md`](docs/ci.md).
Core solo no cierra M0 ni califica una distribución M1. ADR-048 exige además el
full gate source-bound con feature `local`, E5/ORT/LanceDB y el gateway Docker
Linux ARM64 en el host positivo macOS 26 ARM64/APFS. La CI portable en Linux,
macOS y Windows acredita fuente, protocolo y comportamiento fail-closed; no amplía
las capabilities nativas ni promete artifacts para esos targets.

## Licencia

Copyright © 2026 IUMotion Labs.

El proyecto se distribuye, a elección del usuario, bajo
[MIT](LICENSE-MIT) o [Apache License 2.0](LICENSE-APACHE). Los componentes y datos de
terceros conservan sus propias licencias; consulta [`NOTICE`](NOTICE). Cada
distribución binaria deberá incorporar su inventario específico de notices.

## Escritura local M2 en desarrollo

[ADR-050](docs/adr/ADR-050-local-coordinated-mutation.md) fija el modo
`local_coordinated`: preview devuelve el diff y un plan acotado; commit revalida
la generación completa y publica el candidato exacto; receipt permite observar o
recuperar la operación durable. Los locks coordinan procesos que comparten
`--state-root`, pero no bloquean IDE, Git u otros escritores del mismo usuario. No
hay CAS ni atomicidad visible para una publicación de varios archivos.

El checkout de desarrollo descubre 31 tools: conserva las trece de M1, añade
`rust.manifest.patch`, `rust.fmt.apply`, `rust.fix.apply`,
`rust.dependency.add` y `rust.dependency.remove`, e integra el contrato M3-01 de
`rust.test.nextest`, las otras tres tools M3, las cinco tools M4 calificadas
localmente y las cuatro tools M5 calificadas localmente. Cada tool de escritura exige su grant de host:
`--allow-manifest-write`, `--allow-fmt-write`, `--allow-fix-write`,
`--allow-dependency-add` o `--allow-dependency-remove`, seguido de la raíz del
workspace. Un grant no autoriza planes ni receipts de otra operación.

`manifest.patch` admite operaciones cerradas set/remove para lints, features,
profiles incorporados y workspace dependencies. `rust.dependency.add/remove` seleccionan un
`Cargo.toml` relativo que Cargo debe corroborar como package miembro. Los cambios
que alteran resolución requieren el par opcional `--cargo-vendor-dir PATH` y
`--cargo-vendor-tree-sha256 sha256:DIGEST`; lints, profiles, fmt y fix no lo
requieren. La policy `preserve_presence` actualiza el `Cargo.lock` raíz si ya
existía y excluye del candidato el lock transitorio si no existía.

Todo candidato se produce o valida con la imagen Docker ya configurada para M1,
sin ejecutar Cargo del host ni descargar datos durante una llamada MCP. El perfil
dedicado de fix conserva `network=none` y permite TCP loopback solo dentro de su
namespace para la coordinación interna de Cargo; build scripts y proc macros pueden
influir en los cambios `.rs`, por lo que se debe revisar el diff exacto. La
calificación local M2 está completada con [evidencia reproducible](docs/validation/M2/07.md). El paquete del checkout informa
`0.3.0`.

`rust.test.nextest` usa el perfil quality dedicado, no ejecuta doctests y publica
JUnit/stdout/stderr como Resources privadas. M3-02 habilitó el anuncio de MCP Tasks
después de la puerta G4: el uso asíncrono sólo se materializa cuando el peer también
declara `io.modelcontextprotocol/tasks`. Sin esa declaración, `auto` y
`synchronous` sólo se admiten para una selección calificada con
`timeout_seconds <= 60`; un `auto` más largo devuelve `TASKS_REQUIRED` antes de
admisión y `task` se rechaza. Véanse los
[documentos de validación M3](docs/validation/M3-matrix.md).

La CLI de desarrollo `cargo-vendor inspect --directory /ruta/vendor --json`
verifica un directory source preparado mediante
`cargo vendor --locked --versioned-dirs /ruta/vendor`. Devuelve el fingerprint y
los paquetes verificados, sin ejecutar Cargo ni descargar datos.

`cargo-vendor capture --directory /ruta/vendor --into /ruta/capturas --json`
produce en cambio una captura ADR-078: lee el árbol de forma incremental, escribe
un artifact inmutable cuyo nombre es su propio digest y devuelve ese digest, que
es el valor de `--vendor-capture-tree-sha256`. Rechaza symlinks, hard links y
cualquier entrada que no sea archivo o directorio regular, falla si el árbol
cambia durante la captura y no deja residuo si se cancela. Tampoco descarga nada. El operador
ejecuta ambos comandos de preparación fuera del runtime MCP; el servidor no hereda
`CARGO_HOME`, no instala herramientas y no descarga crates. Esta fuente es opcional y no forma
parte de la instalación de M1.

## M3 — calidad avanzada

El checkout `0.3.0` descubre 31 tools.
`rust.test.nextest`, `rust.coverage`,
`rust.semver.check` y `rust.mutation.test` están implementadas y calificadas en el
gate Docker M3: 62/62 selecciones (nextest 19, Tasks 7, coverage 8,
SemVer 18 y mutation 10), más 20/20 controles de seguridad. Los detalles
y hashes están en la [matriz y recibos M3](docs/validation/M3-matrix.md).
Las 18 snapshots preexistentes son byte-identical a `main`; el snapshot de
mutation cambió deliberadamente durante las correcciones de seguridad.

El host habilita la imagen guest M3 mediante provisioning explícito y autorizado
por el owner de `fixtures/rust-runtime` con `--plugins`. Las versiones de plugins
y sus hashes quedan fijados en el recibo de provisioning; la imagen seleccionada
es el nuevo ID inmutable
`sha256:384a1742ecc53cdd3a9c0bf36c6f8b66db73ddd118aeeae6e55654ea998ae36a`.
Incluye nextest 0.9.143, llvm-cov 0.9.0 con llvm-tools 1.98.1, semver-checks
0.50.0 y mutants 27.1.0 construido desde fuente. El runtime MCP no instala ni
actualiza plugins.

El store privado persistente de calidad usa el mismo `--state-root`, bajo
`rust-mcp-quality-artifacts-v1`. Su TTL predeterminado es 1 h; los límites son
32 MiB por artifact, 64 MiB por job, 128 MiB por owner, 256 MiB global y 128
miembros por job. `quality-artifacts recover|prune` está disponible para el
operador local. En macOS ARM64/APFS el store está calificado; Linux y Windows
fallan cerrados.

Las lecturas Stage 1 mediante Resources y la recuperación/prune usan el store
privado persistente. Para M3, macOS ARM64/APFS es el único host positivo; Linux y
Windows fallan cerrados. Tasks está implementado, calificado y anunciado; cada
operación asíncrona sigue requiriendo que el peer declare la extensión. Inspector
2.5.0 completó el lifecycle Tasks; Codex app-server 0.153.0 no declaró la extensión
y quedó en su ruta síncrona calificada.

El límite de cuatro planes aplica a propuestas pendientes: los planes terminales
dejan capacidad para nuevas propuestas en la siguiente admisión. Un commit con
plan ausente/expirado solo puede repetir un journal existente con ID, digest y key
exactos, bajo grant vivo e identidad física original. No inicia efectos nuevos sin
preview vigente. Prune retira ese replay; un receipt terminal describe historia,
no el source actual. Véase [ADR-059](docs/adr/ADR-059-terminal-plan-retirement-and-durable-replay.md).

Para consultar los requisitos compilados del runtime opcional de seguridad:

```sh
rust-engineering-mcp security-runtime inventory --json
```

El inventario incluye imagen, cargo-deny, helper, nightly y sysroot con sus hashes.
`installation_observed=false` indica que el comando no inspecciona ni instala
componentes. La operación real exige la configuración explícita descrita arriba
y calibración del gateway; el reporte `doctor` existente conserva su formato.
