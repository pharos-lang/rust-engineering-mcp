# ADR-017 — LanceDB como índice derivado

Date: 2026-09-03

## Context

La búsqueda por intención necesita similitud vectorial, pero versiones, licencias,
yanked y advisories deben provenir del snapshot autoritativo.

## Decision

Usar LanceDB detrás de `SemanticIndex` solo para IDs y scores de candidatos. Cada
generación registra snapshot fingerprint, schema/index version, embedding model,
revision, dimensión y normalización. Una discrepancia, ausencia o corrupción invalida
el índice y activa fallback lexical declarado. SQLite rehidrata y filtra todos los
facts finales.

El release 0.1.0 oficial incluye soporte semántico; builds de desarrollo pueden
deshabilitarlo para aislar fallos, pero no califican como release M1. Rebuild escribe
una generación nueva y la activa atómicamente.

## Alternatives considered

- SQLite vector extension: menor stack, pero no es la decisión de producto y requiere
  reevaluar calidad/portabilidad.
- LanceDB como source of truth: mezcla facts con índice reconstruible.
- Semantic-only: pierde matches exactos y no degrada offline.

## Consequences

El grafo Arrow/LanceDB es pesado y exige benchmarks de build, startup, RAM y targets.
Resultados híbridos deben marcar qué canales contribuyeron; un score no afirma calidad.

## Status

Accepted.

Source: <https://docs.rs/lancedb/latest/lancedb/>

