# ADR-003 — Transporte stdio primero

Date: 2026-09-03

## Context

El MVP es local, offline-first y no necesita autenticación ni lifecycle HTTP.

## Decision

El único transporte M1 es stdio provisto por `rmcp`. stdout queda reservado a frames
MCP; logging y diagnósticos operativos salen por stderr. Streamable HTTP queda fuera
de M1 y exigirá otro ADR de auth, red y rate limits.

## Alternatives considered

- Streamable HTTP desde M0: amplía superficie de ataque y operación sin aportar al
  caso local.
- Transporte propio: incompatible con el objetivo de apoyarse en el SDK.

## Consequences

El onboarding local es simple y la superficie es menor. CI necesita un harness que
separe y compruebe stdout/stderr y cierre limpio ante EOF/cancelación.

## Status

Accepted.

Source: <https://modelcontextprotocol.io/specification/2026-07-28/basic/transports>

