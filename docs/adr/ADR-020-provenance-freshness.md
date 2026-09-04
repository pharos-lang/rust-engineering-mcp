# ADR-020 — Provenance y freshness obligatorios

Date: 2026-09-03

## Context

Un snapshot local puede estar correcto y a la vez obsoleto. Presentar
`latest_known` como live induce decisiones falsas.

## Decision

Toda salida basada en catálogo, advisories, modelo o artifact mutable incluye un
tipo no opcional `Provenance` con source kind, snapshot/model ID, created/observed at,
integrity status y `network_used`; y `Freshness` con `live`, `fresh`, `aging`, `stale`
o `unknown`, edad y threshold/policy aplicada.

Usar `latest_known` para snapshots. `latest_live` solo existe cuando una operación
CLI/open-world explícita consultó y registró la fuente, nunca en runtime M1. Clock es
un port inyectable. Un quality gate no pasa audit si freshness viola su policy; search
puede devolver datos stale con warning y filtros declarados.

## Alternatives considered

- Timestamps sueltos: obligan a cada consumidor a interpretar edad.
- Freshness opcional: permite omitir el dato precisamente cuando más importa.

## Consequences

Los contratos son algo mayores pero verificables. Unit tests controlan umbrales y
reloj; contract tests impiden eliminar provenance accidentalmente.

## Status

Accepted.

