# ADR-011 — Resources para contexto reusable

Date: 2026-09-03

## Context

Logs, metadata reutilizable y artifacts grandes no deben repetirse en cada resultado
ni consumir todo el contexto del agente.

## Decision

Las tools ejecutan trabajo; Resources exponen información ya calculada o artifacts.
M1 implementa el mínimo requerido para leer artifacts acotados y, si aporta valor,
catalog status/project metadata. URIs contienen IDs opacos, no paths del host. Cada
lectura revalida ProjectRef/retención y aplica límites. Prompts quedan fuera de M1.

## Alternatives considered

- Todo como tool: repite trabajo y mezcla lectura con efectos.
- Incluir payload completo siempre: simple, pero rompe límites de contexto.

## Consequences

El adapter MCP incorpora resources sin trasladarlos al dominio. Contract/protocol
tests cubren autorización, URI inválida, expiry y tamaño.

## Status

Accepted.

