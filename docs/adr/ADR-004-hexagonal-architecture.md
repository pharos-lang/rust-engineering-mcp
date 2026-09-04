# ADR-004 — Arquitectura hexagonal

Date: 2026-09-03

## Context

El dominio debe conservar significado estable aunque cambien `rmcp`, Cargo, SQLite,
LanceDB o el sandbox de plataforma.

## Decision

Separar domain, application, ports y adapters. Domain y application no importan
`rmcp`, JSON-RPC, stdio, SQLite, LanceDB ni APIs de procesos. Se crean ports solo en
fronteras de efectos o persistencia que deban probarse/sustituirse. El workspace
inicial tendrá el menor número de crates que haga comprobable esa dirección de
dependencias; no se precrearán todos los crates sugeridos por la propuesta.

## Alternatives considered

- Un único módulo acoplado: rápido al inicio, difícil de asegurar y probar.
- Un crate por concepto desde el primer día: preserva límites, pero genera ceremonia
  y tiempo de build sin evidencia de necesidad.

## Consequences

Los cortes deben implementarse verticalmente. CI añadirá checks de dependencias y
una búsqueda que impida crear procesos fuera del adapter autorizado.

## Status

Accepted.

