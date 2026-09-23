# Rust Engineering MCP

[![CI](https://github.com/pharos-lang/rust-engineering-mcp/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/pharos-lang/rust-engineering-mcp/actions/workflows/ci.yml)
[![Quality Gate](https://sonarcloud.io/api/project_badges/measure?project=pharos-lang_rust-engineering-mcp&metric=alert_status)](https://sonarcloud.io/summary/new_code?id=pharos-lang_rust-engineering-mcp)
[![Security Rating](https://sonarcloud.io/api/project_badges/measure?project=pharos-lang_rust-engineering-mcp&metric=security_rating)](https://sonarcloud.io/summary/new_code?id=pharos-lang_rust-engineering-mcp)
[![Reliability Rating](https://sonarcloud.io/api/project_badges/measure?project=pharos-lang_rust-engineering-mcp&metric=reliability_rating)](https://sonarcloud.io/summary/new_code?id=pharos-lang_rust-engineering-mcp)
[![Maintainability Rating](https://sonarcloud.io/api/project_badges/measure?project=pharos-lang_rust-engineering-mcp&metric=sqale_rating)](https://sonarcloud.io/summary/new_code?id=pharos-lang_rust-engineering-mcp)
[![Coverage](https://sonarcloud.io/api/project_badges/measure?project=pharos-lang_rust-engineering-mcp&metric=coverage)](https://sonarcloud.io/summary/new_code?id=pharos-lang_rust-engineering-mcp)
[![Rust 1.98.1](https://img.shields.io/badge/Rust-1.98.1-000000?logo=rust)](rust-toolchain.toml)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE)
[![M8ven Score](https://m8ven.ai/badge/mcp/pharos-lang/rust-engineering-mcp)](https://m8ven.ai/mcp/pharos-lang/rust-engineering-mcp)

Rust Engineering MCP conecta agentes compatibles con [Model Context Protocol
(MCP)](https://modelcontextprotocol.io/) con proyectos Rust locales. Expone
transporte `stdio` (JSON-RPC vía `rmcp`) y operaciones estructuradas para abrir
un workspace, ejecutar comprobaciones de calidad dentro de un runtime Docker
controlado, consultar diagnósticos, editar el manifest con permiso explícito y
buscar en un catálogo local de crates.

## Capacidades

La **release publicada más reciente, `v0.3.0`**, expone 31 tools estables:
apertura e inspección de proyecto, toolchain, `check`/`fmt`/`clippy`/`test`,
audit RustSec, quality gate, catálogo local, las cinco tools de escritura
(`fmt.apply`, `fix.apply`, `dependency.add/remove`, `manifest.patch`), nextest,
coverage, semver-check, mutation testing, y las tools de seguridad y
rendimiento avanzadas (`deny`, `unsafe.scan`, `supply_chain.inspect`,
`quality.gate.v2`, `miri`, `benchmark.run/compare`, `profile.flamegraph`,
`binary.bloat`). El checkout de desarrollo (`0.9.0-rc.1`, sin tag ni release
todavía) añade cinco tools de análisis en clase **preview**
(`rust.analyzer.symbols/references/diagnostics/actions/action.apply`), para un
total de 36. El catálogo completo, con contratos, límites y ejemplos, está en
[`docs/reference/tools.md`](docs/reference/tools.md); la CLI del binario
(`doctor`, `catalog`, `mutation`, `contract`, etc.) está en
[`docs/reference/cli.md`](docs/reference/cli.md).

## Requisitos y frontera de soporte

- Binario host: macOS con Apple Silicon (`aarch64-apple-darwin`) sobre un
  volumen **APFS**. Es la única plataforma con capacidades positivas hoy;
  Linux y Windows fallan cerrado en las rutas de mutación y ejecución. CI
  hoy corre sobre dos plataformas hospedadas (`ubuntu` x86_64, `macos-26`
  arm64) como evidencia de fuente/protocolo; la CI de Windows fue retirada
  el 2026-09-13 y su restauración no es un requisito de soporte.
- Runtime/engine: las tools que ejecutan Cargo (`check`, `clippy`, `test`,
  `quality.gate`, mutación, seguridad, rendimiento y analyzer) requieren Docker
  con una imagen guest Linux ARM64 admitida por digest exacto — ver
  [Operación del runtime](docs/operations/runtime-provisioning.md).
- Datos opcionales: catálogo de crates (SQLite + FTS5, LanceDB derivado bajo
  el feature `local`), snapshot de advisories RustSec y vendor Cargo offline;
  ninguno se descarga durante una sesión MCP — ver
  [Mantenimiento del catálogo](docs/operations/catalog-maintenance.md).
- No disponible: transporte remoto/HTTP, sandbox nativo Linux/Windows y
  runtime unificado multiplataforma (planes sin ejecutar).

## Instalar

**Release soportada:** `v0.3.0` publica exactamente tres assets: el archivo
core para `aarch64-apple-darwin`, `SHA256SUMS` y `release-smoke-receipt.json`.
El SBOM SPDX y las notices de terceros van dentro del archive (no son assets
separados); la procedencia se verifica en línea con `gh attestation verify`
contra el registro de atestaciones de GitHub.

```bash
curl -LO https://github.com/pharos-lang/rust-engineering-mcp/releases/download/v0.3.0/rust-engineering-mcp-v0.3.0-aarch64-apple-darwin.tar.gz
curl -LO https://github.com/pharos-lang/rust-engineering-mcp/releases/download/v0.3.0/SHA256SUMS
shasum -a 256 -c SHA256SUMS
tar -xzf rust-engineering-mcp-v0.3.0-aarch64-apple-darwin.tar.gz
```

Esa release expone 31 tools (no las 5 preview del checkout). Detalle completo
de verificación (incluida `gh attestation verify`) y compilación desde fuente
con el toolchain fijado `1.98.1`: [`docs/guides/installation.md`](docs/guides/installation.md).

## Conectar un cliente

Ejemplo mínimo para **Claude Code** (evidencia real de calificación en
[`docs/guides/clients.md`](docs/guides/clients.md)):

```bash
claude mcp add --scope project rust-engineering -- \
  /ruta/absoluta/rust-engineering-mcp \
  serve --stdio \
  --root /ruta/absoluta/al/proyecto
```

Sustituye `/ruta/absoluta/rust-engineering-mcp` por la ruta absoluta al binario
instalado o compilado, y `/ruta/absoluta/al/proyecto` por la raíz física que
autorizas a abrir. Esta configuración **no concede escritura**: sin flags
`--allow-*-write` ni grupo `--docker`, el servidor solo abre y valida
proyectos. Otros clientes (Codex, MCP Inspector, Gemini CLI, Cursor, VS Code)
y la configuración avanzada están en
[`docs/guides/clients.md`](docs/guides/clients.md) y
[`docs/guides/configuration.md`](docs/guides/configuration.md).

## Primera comprobación y uso

```bash
/ruta/absoluta/rust-engineering-mcp doctor --json
```

`doctor` es un diagnóstico pasivo: reporta capacidades configuradas sin
instalar ni reparar nada. Con el servidor conectado, el flujo mínimo de un
agente es:

1. `rust.project.open` con la ruta absoluta autorizada → devuelve `project_ref`.
2. Guardar ese `project_ref` para las llamadas siguientes.
3. Ejecutar una operación, por ejemplo `rust.check` sobre el `project_ref`.

El paso 3 **ejecuta Cargo dentro del runtime Docker aprobado**; sin ese
runtime configurado la tool responde `blocked`/`SANDBOX_DENIED` (abrir un
proyecto sin Docker sigue funcionando, pero no basta para validar código). Ver
el paso exacto para provisionar la imagen guest en
[`docs/operations/runtime-provisioning.md`](docs/operations/runtime-provisioning.md).

## Advertencias operativas

- **No uses el servidor con repositorios no confiables en esta versión de
  desarrollo.** Autoriza únicamente las roots que necesitas, con rutas
  absolutas; el audit de seguridad es una revisión de modelo, no un
  pentest, y el kernel/`runc`/Docker Desktop siguen en el TCB. Ver
  [`SECURITY.md`](SECURITY.md).
- `cargo check`/Clippy/test/mutación pueden ejecutar `build.rs` y proc macros:
  no son operaciones "de solo lectura" en el sentido de código inerte.
  Mantén aprobación interactiva del cliente.
- Ninguna tool de escritura actúa sin el grant explícito del host
  (`--allow-manifest-write`, `--allow-fmt-write`, `--allow-fix-write`,
  `--allow-dependency-add`, `--allow-dependency-remove`,
  `--allow-analyzer-action-write`); no hay escritura por defecto.
- El catálogo, RustSec y los datos de vendor son siempre offline y los aporta
  el operador; el runtime MCP nunca sincroniza ni descarga nada por sí mismo.
- Soporte limitado a macOS ARM64/APFS con guest Docker Linux ARM64; las tools
  `analyzer.*` son **preview** con deuda de contrato conocida.

Modelo de seguridad completo: [`docs/architecture/execution-and-security.md`](docs/architecture/execution-and-security.md)
y [`SECURITY.md`](SECURITY.md).

## Documentación

- [Instalación detallada](docs/guides/installation.md)
- [Configuración de host y flags](docs/guides/configuration.md)
- [Clientes MCP](docs/guides/clients.md)
- [Flujos de trabajo típicos](docs/guides/workflows.md)
- [Solución de problemas](docs/guides/troubleshooting.md)
- [Tools y CLI](docs/reference/tools.md) · [Compatibilidad](docs/reference/compatibility.md)
- [Arquitectura](docs/architecture/overview.md)
- [Seguridad](SECURITY.md) · [Contribuir](CONTRIBUTING.md) · [Changelog](CHANGELOG.md)
- Índice completo: [`docs/README.md`](docs/README.md)

## Licencia

Copyright © 2026 IUMotion Labs. Distribuido, a elección del usuario, bajo
[MIT](LICENSE-MIT) o [Apache License 2.0](LICENSE-APACHE). Componentes y datos
de terceros conservan sus propias licencias; consulta `NOTICE`.
