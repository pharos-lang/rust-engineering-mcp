# ADR-018 — Sincronización separada y snapshots seguros

Date: 2026-09-03

## Context

Consultas MCP con red oculta rompen reproducibilidad y air-gap. A la vez, importar un
archive no confiable introduce traversal, decompression bombs y rollback attacks.

## Decision

El runtime MCP nunca sincroniza, descarga advisories/modelos ni hace live fallback.
`catalog sync` es un modo CLI explícito con policy/host allowlist propia. `catalog
import` acepta un manifest canónico firmado con versión, snapshot ID monotónico,
hashes, tamaños y modelo; verifica contra trust roots configuradas por el host.

La importación impone límites comprimidos/descomprimidos y de archivos, rechaza paths
absolutos, `..`, symlinks, hardlinks y devices, extrae a staging privado, verifica
cada hash/SQLite/index y activa atómicamente con rollback. Snapshots anteriores se
rechazan salvo flag administrativo explícito. No se inventa una clave de firma antes
de definir la distribución.

## Alternatives considered

- Actualizar durante `crate.search`: datos frescos con comportamiento oculto.
- Confiar solo en TLS/checksum: no autentica publisher ni evita rollback.
- Extraer y validar después: expone filesystem antes de verificar.

## Consequences

M1 puede operar completamente offline. La política exacta de trust roots bloquea la
publicación de snapshots oficiales, no el desarrollo del formato/importer.

## Status

Accepted.

