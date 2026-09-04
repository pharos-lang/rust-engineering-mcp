# ADR-008 — Execution Gateway único

Date: 2026-09-03

## Context

Cargo puede lanzar rustc, build scripts, proc macros, linkers, tests y procesos
hijos. Validar solo el comando superior no controla el grafo de ejecución.

## Decision

Toda ejecución externa pasa por un único adapter que consume `ExecutionSpec` tipado:
executable absoluto y verificado, argv cerrado, cwd validado, roots de lectura y
escritura, entorno construido tras `env_clear`, policy efectiva, sandbox requerido,
timeout, cancellation token y límites de streaming.

`check`, Clippy y test se marcan `executes_project_code=true` porque pueden ejecutar
build scripts/proc macros. Requieren opt-in del host y sandbox suficiente. En modo
offline, Cargo usa configuración cooperativa offline/frozen y aislamiento real de
red; falta de dependencias es error tipado, nunca motivo para descargar.

El gateway crea y termina árboles de procesos con adapter OS. Process group/session
en Unix es solo best-effort porque un descendiente puede desacoplarse con `setsid`;
no habilita `children_contained=true` por sí solo. La garantía fuerte exige un
mecanismo que impida escape (por ejemplo PID namespace/cgroup apropiado) y una prueba
con descendiente daemonizado. Windows usa Job Object kill-on-close y prueba el escape
equivalente. El target/temp son aislados para código no confiable.

## Alternatives considered

- `Command` disperso por adapter: hace imposible auditar policy y cleanup.
- Matar solo el PID padre: deja descendientes.
- Target estándar con lock interno: mejora cache, pero no contiene código hostil ni
  coordina procesos externos.

## Consequences

La seguridad llega antes de Cargo tools. Cada garantía se expone como capability
concreta y se prueba por plataforma. Puede aumentar tiempo y uso de disco.

## Status

Accepted.
