# Rust Engineering MCP

[![CI](https://github.com/pharos-lang/rust-engineering-mcp/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/pharos-lang/rust-engineering-mcp/actions/workflows/ci.yml)
[![Quality Gate](https://sonarcloud.io/api/project_badges/measure?project=pharos-lang_rust-engineering-mcp&metric=alert_status)](https://sonarcloud.io/summary/new_code?id=pharos-lang_rust-engineering-mcp)
[![Security Rating](https://sonarcloud.io/api/project_badges/measure?project=pharos-lang_rust-engineering-mcp&metric=security_rating)](https://sonarcloud.io/summary/new_code?id=pharos-lang_rust-engineering-mcp)
[![Reliability Rating](https://sonarcloud.io/api/project_badges/measure?project=pharos-lang_rust-engineering-mcp&metric=reliability_rating)](https://sonarcloud.io/summary/new_code?id=pharos-lang_rust-engineering-mcp)
[![Maintainability Rating](https://sonarcloud.io/api/project_badges/measure?project=pharos-lang_rust-engineering-mcp&metric=sqale_rating)](https://sonarcloud.io/summary/new_code?id=pharos-lang_rust-engineering-mcp)
[![Coverage](https://sonarcloud.io/api/project_badges/measure?project=pharos-lang_rust-engineering-mcp&metric=coverage&branch=main)](https://sonarcloud.io/summary/new_code?id=pharos-lang_rust-engineering-mcp&branch=main)
[![Rust 1.98.1](https://img.shields.io/badge/Rust-1.98.1-000000?logo=rust)](rust-toolchain.toml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE)

Rust Engineering MCP conecta agentes compatibles con [Model Context Protocol
(MCP)](https://modelcontextprotocol.io/) con proyectos Rust locales. Expone
operaciones estructuradas para inspeccionar un workspace, ejecutar comprobaciones
de calidad dentro de un runtime controlado, consultar diagnósticos y trabajar con
un catálogo local de crates.

El servidor usa transporte MCP por `stdio`. Las tools son de solo lectura respecto
al código fuente: observan, validan y devuelven evidencia; no aplican cambios al
repositorio.

> [!IMPORTANT]
> La versión actual es `0.1.0-dev.1` y se distribuye como código fuente. Todavía no
> existe una versión binaria soportada en GitHub Releases. La ejecución completa se
> ha calificado localmente en Apple Silicon con macOS 26 y APFS; la CI comprueba que
> el código compila y pasa sus pruebas en Linux, macOS y Windows, pero eso no amplía
> las garantías del sandbox o del filesystem a esas plataformas.

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
| Seguridad | `rust.dependencies.audit` | Contrasta `Cargo.lock` con un snapshot RustSec suministrado por el host. |
| Diagnóstico | `rust.diagnostics.explain` | Obtiene la explicación de un código `rustc`, por ejemplo `E0502`. |
| Calidad | `rust.quality.gate` | Ejecuta un gate `fast` o `standard` y devuelve el estado de cada etapa. |
| Catálogo | `rust.catalog.status` | Informa disponibilidad, identidad y frescura del catálogo local. |
| Catálogo | `rust.crate.search` | Busca crates en modo léxico, semántico o híbrido. |
| Catálogo | `rust.crate.inspect` | Consulta versiones, features, dependencias y advisories registrados. |

Los contratos completos, límites y ejemplos de respuesta están en
[`docs/tools.md`](docs/tools.md).

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
| Codex | [CLI o `config.toml`](docs/client-configuration.md#codex) | APIs directas del cliente Codex 0.153.0 verificadas; uso autónomo por el modelo pendiente. |
| Claude Code | [CLI o `.mcp.json`](docs/client-configuration.md#claude-code) | Configuración documentada; calificación de este MCP pendiente. |
| Gemini CLI | [`settings.json`](docs/client-configuration.md#gemini-cli) | Configuración documentada; calificación de este MCP pendiente. |
| Cursor | [`.cursor/mcp.json`](docs/client-configuration.md#cursor) | Configuración documentada; calificación de este MCP pendiente. |
| VS Code / GitHub Copilot | [`.vscode/mcp.json`](docs/client-configuration.md#vs-code-y-github-copilot) | Configuración documentada; calificación de este MCP pendiente. |
| MCP Inspector | [Web, CLI o TUI](docs/client-configuration.md#mcp-inspector) | Inspector 2.5.0 descubrió y llamó las 13 tools; Resource read no quedó calificado. |

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
`rust.clippy`, `rust.test`, `rust.dependencies.audit`,
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
  --rust-image sha256:8fac70723a8d04b6ec9633ab721806b8a55f4f083a1b3f988c61bf6a00fa1909
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

## Licencia

Copyright © 2026 IUMotion Labs.

El proyecto se distribuye, a elección del usuario, bajo
[MIT](LICENSE-MIT) o [Apache License 2.0](LICENSE-APACHE). Los componentes y datos de
terceros conservan sus propias licencias; consulta [`NOTICE`](NOTICE). Cada
distribución binaria deberá incorporar su inventario específico de notices.
