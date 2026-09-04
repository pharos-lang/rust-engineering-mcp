# ADR-014 — Artifacts mínimos y acotados

Date: 2026-09-03

## Context

La propuesta pide truncar outputs grandes y referir el contenido completo, pero
aplaza artifacts/resources generales a M3. Sin un mecanismo mínimo, M1 pierde
evidencia o captura output ilimitado.

## Decision

Introducir en M0/M1 un ArtifactStore mínimo para logs truncados y diffs: escritura
streaming con cap duro, directorio privado, IDs aleatorios, metadata de tamaño/hash,
TTL, cuota global/por proyecto, redacción y cleanup. El resultado parcial conserva
estado y `truncated=true`; alcanzar el límite no descarta diagnósticos ya parseados.
El contenido adicional se lee mediante Resource autenticado por el ProjectRef.

Artifacts ricos (coverage, flamegraphs, SBOM) y gestión general quedan en M3+.

## Alternatives considered

- `OUTPUT_LIMIT_EXCEEDED` sin resultado: pierde evidencia útil.
- Capturar todo y truncar después: permite agotar memoria/disco.
- Aplazar artifacts: deja inconsistente el contrato M1.

## Consequences

Esta es una divergencia acotada del roadmap, necesaria para satisfacer límites y
evidencia. Security tests verifican permisos, cuota, secretos, expiry y streaming
infinito.

## Status

Accepted.


Refinamiento explícito: [ADR-028](ADR-028-ephemeral-artifact-store.md) implementa
almacenamiento efímero en memoria para M0; Resource autenticado permanece M1.
El directorio privado aquí descrito no se declara implementado ni verificado.
