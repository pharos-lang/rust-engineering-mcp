# ADR-001 — Rust y Tokio como plataforma

Date: 2026-09-03

## Context

El servidor integra procesos Cargo, I/O asíncrono, parsing de datos no confiables y
distribución multiplataforma. La especificación exige Rust y Tokio.

## Decision

Implementar el producto en Rust estable con Tokio para I/O, procesos y cancelación.
Fijar el toolchain en el repositorio y mantener `Cargo.lock` para binarios. El core
usa tipos concretos y errores tipados; `unsafe` propio requiere justificación y test.

## Alternatives considered

- TypeScript o Python: ecosistema MCP accesible, pero peor ajuste para distribución
  autocontenida, control de procesos y tipos compartidos con Cargo.
- Rust síncrono: menor complejidad inicial, pero no satisface bien stdio concurrente,
  cancelación ni streaming de procesos.

## Consequences

La integración con Cargo y la distribución son naturales. Aumentan el tiempo de
compilación y la disciplina requerida sobre features/dependencias. Rust no sustituye
el sandbox del sistema operativo.

## Status

Accepted.

