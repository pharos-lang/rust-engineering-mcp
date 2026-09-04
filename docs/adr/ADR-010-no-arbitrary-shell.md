# ADR-010 — Sin shell arbitrario

Date: 2026-09-03

## Context

Un wrapper de shell convertiría el MCP en ejecución remota genérica y habilitaría
inyección por argumentos, quoting y expansión.

## Decision

No invocar `sh -c`, `bash -c`, `cmd /c`, PowerShell evaluando texto ni equivalentes.
Resolver ejecutables autorizados a paths absolutos confiables y construir argv desde
enums/newtypes validados. No aceptar trailing args arbitrarios, variables wrapper,
runner/linker aportados por el caller ni command strings en configuración de
proyecto. CI inspeccionará usos de APIs de proceso fuera del Execution Gateway.

## Alternatives considered

- Shell con escaping: las reglas varían por plataforma y siguen ampliando scope.
- Allowlist de strings completas: frágil ante flags compuestos y ejecutables hijos.

## Consequences

Algunos casos avanzados de Cargo no estarán disponibles hasta modelar parámetros
tipados. Esta limitación es intencional y reduce la superficie de ataque.

## Status

Accepted.

