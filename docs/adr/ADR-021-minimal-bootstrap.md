# ADR-021 — Bootstrap ejecutable mínimo

Date: 2026-09-03

## Context

M0-01 requiere un workspace compilable, toolchain fijado, binario mínimo, controles
de calidad y documentación inicial. ADR-004 prohíbe precrear capas vacías. El
toolchain exacto 1.97.1 estaba instalado al ejecutar M0-01 en el host macOS ARM64. La licencia sigue
pendiente de confirmación explícita del owner según el tablero inicial.

## Decision

Crear un workspace virtual con un solo paquete `crates/mcp-server`, binario
`rust-engineering-mcp`, versión de desarrollo `0.1.0-dev.1`, `publish = false`,
edition 2024 y toolchain/MSRV inicial 1.97.1. Fijar rustfmt y Clippy como componentes
del toolchain y mantener Cargo.lock. No declarar compatibilidad con Rust anterior
sin pruebas.

El corte implementa únicamente ayuda y versión de la CLI. Sin argumentos o ante
cualquier invocación no soportada, termina con código 2, diagnóstico fijo en stderr
y stdout vacío. En particular, `serve --stdio` y `--stdio` no inician un servidor.
No se parsea stdin ni se imprime un banner al iniciar. Los errores de escritura
devuelven código 1 sin panic.

No añadir dependencias hasta que exista un consumidor real: Tokio, `rmcp` y tracing
se incorporarán al corte MCP M0-03, después de la verificación exigida por ADR-002.
M0-02 introducirá el dominio y sus límites de dependencias. No crear interfaces o
adapters vacíos para anticipar esos cortes. La decisión Rust/Tokio sigue vigente.

`LICENSE` registra únicamente que la licencia no se ha elegido; no concede una
licencia ni se referencia como `license-file` en Cargo. La publicación sigue
bloqueada. Los gates de desarrollo ejecutan código propio revisado del repo; no
son garantías de sandbox del producto. Los tests de CLI pueden lanzar únicamente
el binario construido por Cargo, con entorno vacío, como harness de frontera.

## Alternatives considered

- Crear todos los crates y dependencias del MVP: amplía M0-01 sin capacidades reales.
- Servidor MCP incompleto o protocolo manual: invade M0-03 y contradice ADR-002.
- Toolchain `stable` flotante: no identifica la versión usada en el gate.
- Elegir MIT/Apache por convención: contradice la decisión pendiente del owner.

## Consequences

El bootstrap es ejecutable y comprobable sin descargas de crates. Todavía no ofrece
tools MCP, aislamiento OS ni consultas al catálogo. Solo se acredita el target
realmente probado; CI multiplataforma permanece en M0-11. La separación hexagonal
se materializa al introducir comportamiento de dominio, no mediante scaffolding.

## Status

Accepted.

Addendum 2026-09-03: el owner actualizó toolchain/MSRV a 1.98.1 en `cafe721`.
Esa es la versión vigente; los números anteriores conservan el contexto histórico
del bootstrap, no indican que deba revertirse el upgrade.

Sources: <https://doc.rust-lang.org/cargo/reference/workspaces.html>,
<https://doc.rust-lang.org/cargo/reference/manifest.html>,
<https://rust-lang.github.io/rustup/overrides.html#the-toolchain-file>
