# ADR-012 — SemVer y compatibilidad MCP

Date: 2026-09-03

## Context

El protocolo y `rmcp` cambian con rapidez. MCP 2026-07-28 usa lifecycle moderno
stateless con `server/discover`; `initialize`/`initialized` son compatibilidad legacy.

## Decision

Baseline M1: MCP 2026-07-28 y `rmcp` 3.2.0. El adapter usa negociación/capabilities
del SDK y no codifica una única versión fuera de la matriz. El contrato principal
prueba `server/discover`; legacy initialization se soporta solo en las versiones que
`rmcp` negocie y se prueba por separado.

Durante 0.x, cambios incompatibles requieren minor release, changelog y snapshots de
schema. Tool names y campos requeridos no cambian en patch. Dependencias estratégicas
se fijan en lockfile y se actualizan por PR explícita con protocol/contract tests.

## Alternatives considered

- Soportar solo legacy: nace obsoleto.
- Seguir siempre latest sin pin: introduce cambios no revisados.
- Wrapper propio de compatibilidad: duplica el SDK.

## Consequences

Debe existir `docs/compatibility.md` antes del release y una matriz de clientes
objetivo. Los tipos `non_exhaustive` de `rmcp` se manejan en el adapter.

## Status

Accepted.

Sources: <https://modelcontextprotocol.io/specification/2026-07-28/basic/versioning>,
<https://github.com/modelcontextprotocol/rust-sdk/releases/tag/rmcp-v3.2.0>

