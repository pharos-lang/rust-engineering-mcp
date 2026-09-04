# ADR-016 — SQLite como catálogo autoritativo

Date: 2026-09-03

## Context

El catálogo necesita facts relacionales, transacciones, FTS y operación local sin un
servicio adicional.

## Decision

Usar SQLite mediante `rusqlite` encapsulado en `SqliteCatalogRepository`. El build
oficial usa SQLite bundled para controlar versión/features y verificar FTS5 en cada
target. Migrations son monotónicas, versionadas y transaccionales; nunca se editan
después de release. `PRAGMA user_version` se contrasta con una tabla de migrations.

Schema v1 normaliza crates, versiones, features, dependencies, yanked, rust-version,
license, repository, timestamps, advisories, snapshots, provenance y freshness. FTS5
indexa documentos derivados con rebuild determinista. Imports se construyen en una
DB de staging, pasan integridad/schema y se activan atómicamente; la DB activa se
abre read-only desde runtime cuando sea posible.

## Alternatives considered

- Servicio SQL: rompe operación local/offline simple.
- LanceDB como única DB: no es la fuente adecuada para facts/joins.
- SQLx: útil async/multibackend, pero innecesario para SQLite embebido M1.

## Consequences

El adapter debe manejar `spawn_blocking`/pool acotado para no bloquear Tokio.
Bundled incrementa build/binario, pero hace FTS5 y versiones reproducibles.

## Status

Accepted.

Sources: <https://www.sqlite.org/>, <https://www.sqlite.org/fts5.html>

