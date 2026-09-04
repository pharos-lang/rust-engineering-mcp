# ADR-009 — Seguridad deny-by-default verificable

Date: 2026-09-03

## Context

`network=false` o `CARGO_NET_OFFLINE` no impiden que código hostil abra sockets. La
fuerza de filesystem, red, procesos y resource limits varía por sistema operativo.

## Decision

Modelar capacidades separadas, no un booleano `sandbox`: filesystem isolation,
network isolation, child containment, env isolation, CPU, memory, PID y disk limits.
Perfiles: `strict` (todas las garantías requeridas demostradas), `restricted`
(subconjunto explícito) y `none`.

Cada tool declara efectos/requisitos. Si falta una garantía requerida, falla cerrado
con `SANDBOX_DENIED`; no se rebaja silenciosamente. Ninguna tool que compile o ejecute
código no confiable corre en `none`. `offline_cooperative` y `network_isolated` son
estados distintos y ambos se reportan.

La configuración es monotónica: defaults < user/host confiable < CLI confiable; la
config del proyecto solo puede restringir. Claves desconocidas de seguridad son
errores. El default es `allow_project_code=false`.

Matriz normativa M1 (`R` = read root, `W` = write root; cualquier requisito ausente
produce `SANDBOX_DENIED`):

| Tool | Código del proyecto | R autorizadas | W autorizadas | Red | Entorno | Hijos | Recursos | Tier mínimo |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `rust.project.open` | no | root host candidato | ninguna | no usa | proceso MCP | ninguno | presupuesto servidor | `none`, con I/O no-follow |
| `rust.project.inspect` | no | project + toolchain/cache aprobados | temp/target solo si Cargo lo exige | isolated | allowlist | strongly contained | wall/output/PID | `restricted` |
| `rust.toolchain.inspect` | no | toolchain aprobada | temp controlado | isolated | allowlist | strongly contained | wall/output/PID | `restricted` |
| `rust.check` | sí, indirecto | project + toolchain/cache aprobados | target/temp aislados | isolated | allowlist | strongly contained | wall/output/CPU/RAM/PID/disk | `strict` |
| `rust.fmt.check` | no | project + rustfmt aprobados | temp aislado; source read-only | isolated | allowlist | strongly contained | wall/output/PID | `restricted` |
| `rust.clippy` | sí, indirecto | project + toolchain/cache aprobados | target/temp aislados | isolated | allowlist | strongly contained | wall/output/CPU/RAM/PID/disk | `strict` |
| `rust.test` | sí, directo | project + toolchain/cache aprobados | target/temp aislados | isolated | allowlist | strongly contained | wall/output/CPU/RAM/PID/disk | `strict` |
| `rust.dependencies.audit` | no | project lockfile + RustSec snapshot + tool | temp aislado | isolated | allowlist | strongly contained | wall/output/PID | `restricted` |
| `rust.diagnostics.explain` | no | rustc aprobado | ninguna | isolated | allowlist | strongly contained | wall/output/PID | `restricted` |
| `rust.quality.gate` | unión de etapas | unión de etapas | unión de etapas | requisito más fuerte | requisito más fuerte | requisito más fuerte | requisito más fuerte | `strict` para `fast`/`standard` M1 |
| `rust.catalog.status` | no | catalog config/manifest | ninguna | no usa | proceso MCP | ninguno | presupuesto servidor | `none` |
| `rust.crate.search` | no | SQLite/LanceDB/model | ninguna | no usa | proceso MCP | ninguno | wall/CPU/RAM del servidor | `none` |
| `rust.crate.inspect` | no | SQLite/manifest | ninguna | no usa | proceso MCP | ninguno | presupuesto servidor | `none` |

`strongly contained` tiene una sola definición: ningún descendiente puede escapar al
kill/cleanup y el fixture daemonizado de ADR-008 lo demuestra; no existe una variante
débil anunciable. `restricted` sigue exigiendo aislamiento real de red para cualquier
proceso externo, entorno reconstruido, filesystem acotado y strong child containment;
se diferencia de `strict`
porque no ejecuta código del proyecto y no exige todos los límites fuertes. Operar
una tool in-process en `none` no le concede filesystem general: conserva roots y
APIs seguras del proceso MCP.

## Alternatives considered

- Best effort cross-platform: mayor disponibilidad, pero promesas de seguridad
  engañosas.
- Linux-only: simplifica strict, pero contradice el objetivo multiplataforma.

## Consequences

La matriz puede exponer menos tools en ciertos hosts hasta implementar adapters.
Nunca se anuncia soporte strict sin tests reales de red, escape, secretos, procesos
hijos, wall time, output streaming, CPU, RAM, PIDs y disco en esa plataforma, cada
uno con un oráculo que pruebe enforcement y cleanup.

## Status

Accepted.

M1-07 refinement (ADR-038): audit uses the existing isolated metadata child for
workspace roots, then bounded RustSec/SQLite matching on owned bytes in-process.
The restricted external-process row still applies to metadata; no network-deny
requirement is weakened and no whole-process OS sandbox is claimed for the matcher.
