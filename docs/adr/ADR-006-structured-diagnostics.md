# ADR-006 — Resultados y diagnósticos estructurados

Date: 2026-09-03

## Context

Cargo/rustc pueden fallar porque el proyecto es inválido sin que falle la llamada
MCP. La propuesta usa de forma inconsistente `success` y `passed` y no define estados
para controles omitidos.

## Decision

Usar una álgebra común de estado: `passed`, `failed`, `blocked`, `unavailable`,
`cancelled`. `failed` expresa un resultado válido del proyecto con `isError=false`.
Timeout, tooling ausente, sandbox denegado u otro fallo operacional recuperable viajan
en el mismo `OutputEnvelope` tipado con `isError=true` y `structuredContent` conforme
al schema. El envelope contiene `error_code` y `error_message` requeridos pero nulos
para resultados normales; `error_code` es un enum cerrado de fallos operativos.
Errores JSON-RPC se reservan para tool desconocida, request MCP malformada
o fallo interno que impide construir una respuesta de tool. Un quality gate requerido
solo es `passed` si todas sus etapas son `passed`; `blocked`, `unavailable` o
`cancelled` nunca cuentan como éxito.

Normalizar diagnósticos con source, severity, code opcional, mensaje, spans,
rendered opcional, suggestions y truncation metadata. Preservar evidencia cruda solo
como artifact acotado.

## Alternatives considered

- Booleano success: no distingue fallo, ausencia, denegación o cancelación.
- Tratar exit code no cero como error MCP: impide al agente reparar el proyecto.

## Consequences

Los clientes pueden razonar sin parsear texto. Todos los adapters deben mapear su
semántica a estados comunes y contract tests congelan enums y campos requeridos.

## Status

Accepted.
