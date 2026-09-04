# ADR-015 — JSON-RPC versus JSON Schema

Date: 2026-09-03

## Context

JSON-RPC es el envelope MCP; JSON Schema describe argumentos y resultados. Confundir
ambos produciría protocolo duplicado o contratos incompletos.

## Decision

`rmcp` gestiona JSON-RPC, lifecycle, `tools/list`, `tools/call`, result types y
cancelación. Cada tool usa DTOs Rust con Serde/Schemars, raíz objeto por compatibilidad,
`#[serde(deny_unknown_fields)]`, invariantes en newtypes y JSON Schema 2020-12.
Aunque MCP hace `outputSchema` opcional, este producto lo exige para todas las tools.

Handlers devuelven
`Result<Json<OutputEnvelope>, Json<OutputEnvelope>>` (o el equivalente exacto
confirmado en `rmcp` 3.2.0): la rama `Ok` produce `isError=false` y la rama `Err`
produce un structured tool error con `isError=true`. Ambas ramas comparten el mismo
schema discriminado y generan `structuredContent` conforme más el espejo JSON en
TextContent. `ErrorData` se reserva para errores JSON-RPC. Contract tests verifican
schemas y wire output de ambas ramas. Los schemas se validan en runtime en el borde y
se guardan como snapshots de contrato.

## Alternatives considered

- Schemas manuales: duplican los tipos y derivan con facilidad.
- Retorno `String`: puede omitir outputSchema/structuredContent.
- Valores JSON genéricos internos: desplazan errores a runtime.

## Consequences

Cambios de DTO son cambios de contrato visibles en CI. Validación de schema no
reemplaza validación semántica ni sanitización de outputs.

## Status

Accepted.

Sources: <https://modelcontextprotocol.io/specification/2026-07-28/server/tools>,
<https://github.com/modelcontextprotocol/rust-sdk/blob/rmcp-v3.2.0/crates/rmcp/tests/test_json_schema_detection.rs>

## M0-07 implementation refinement

`stdio::contract::Contract<I,O>` owns generated schemas, startup validators,
input decoding (schema plus Serde invariants) and output validation. It requires
closed object roots, and takes the domain ToolStatus from a typed output adapter:
passed/failed map to SDK structured results; blocked/unavailable/cancelled map to
SDK structured errors. Only malformed input uses InvalidParams; output contract
violations use a fixed InternalError without reflecting the rejected value.
No new protocol parser, dependency or public tool is introduced. The existing
project.open schema snapshot is unchanged. The generic boundary has a failed-result
fixture; project.open cannot return a compilation failure and retains its narrower
outcome schema. Future M1 DTOs must model their own data/diagnostics/evidence and
retain the shared domain envelope semantics. Schemas do not replace Rust invariants
such as whitespace-only text, coordinate ordering or provenance consistency.
