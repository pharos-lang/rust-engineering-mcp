# ADR-002 — SDK oficial `rmcp`

Date: 2026-09-03

## Context

MCP evoluciona y el servidor necesita negociación, lifecycle, tools, resources,
cancelación y transporte sin mantener un protocolo propio.

## Decision

Usar el SDK Rust oficial `rmcp`, fijado por `Cargo.lock`, exclusivamente en el
adapter MCP. M0 debe verificar que `3.2.0` siga disponible, no esté yanked y no tenga
advisories aplicables antes de fijarla, y probar contra la versión MCP negociada.
Cualquier cambio de versión exige actualizar este ADR o uno que lo sustituya, la
matriz de compatibilidad y los tests; no se adopta automáticamente `latest`. No se
crea una abstracción genérica de SDK.

Baseline verificada al decidir: `rmcp` 3.2.0 está publicado y la documentación
oficial muestra stdio, schemas derivados, structured content y cancelación.

## Alternatives considered

- Implementar JSON-RPC/MCP manualmente: control total con alto riesgo de drift.
- SDK no oficial: puede ser útil, pero añade riesgo de compatibilidad y ownership.

## Consequences

La API cambiante queda confinada al adapter. Upgrades requieren protocol/contract
tests y actualización de la matriz de compatibilidad, no cambios en el dominio.

## Status

Accepted.

Sources: <https://github.com/modelcontextprotocol/rust-sdk>,
<https://docs.rs/rmcp/3.2.0/rmcp/>
