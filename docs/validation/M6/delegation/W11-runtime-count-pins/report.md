# W11 — pines obsoletos de conteo de tools en tests runtime (`31 → 36`)

| Campo | Valor |
| --- | --- |
| Modelo | Claude Sonnet 5 (`claude -p --model sonnet --effort medium --disallowedTools Agent Task`) |
| CLI | `claude` 2.1.267 |
| Inicio / fin (UTC) | 2026-09-13T01:08:01Z / 2026-09-13T01:14:01Z |
| Resultado | `subtype: success`, `is_error: False` |
| Origen | El gate `full` (1ª pasada) falló en `m2-runtime` grupo 5 (`fix_mutation_runtime::`): `assert Some(36)==Some(31)`. Dos aserciones de conteo de inventario en tests `#[ignore]` (solo bajo `full`) quedaron en 31 tras la subida a 36 tools de dffa620; W08 actualizó las tres de `protocol.rs` pero no estas dos runtime-only. |

## Cambios (dos líneas, mecánicos)

- `crates/mcp-server/tests/inspection_runtime/fix_mutation.rs:262` — `Some(31)` → `Some(36)` (corre bajo `m2-runtime`).
- `crates/mcp-server/tests/inspection_runtime/nextest.rs:37` — `assert_eq!(tools.len(), 31)` → `36` (`#[ignore]` "run serially by M3 gate" → corre bajo `m3-runtime`).

## Verificación (orquestador)

- `git diff` = exactamente las dos líneas anteriores.
- `grep -rnE "tools.len\(\), 31|Some\(31\)" crates/mcp-server/tests/inspection_runtime/` → **0 líneas**.
- No stale en otras formas: `protocol.rs tools[31]` (índice del primer tool M6), `actions.rs:1029 Some(31)` (acciones acotadas), `release-smoke.py` comentario (ya narra "…36 since M6-04/05"), `lock.rs:1119` (paquetes) — todos correctos, no tocados.
- `cargo check -p rust-engineering-mcp --tests --locked --offline` → limpio (worker).

## Clasificación §4 del fallo original

Categoría **(B) actualización esperada de pin**, no regresión del writer M2: el conteo 31→36 es consecuencia directa del inventario ya aprobado y enviado (dffa620); ningún invariante de escritura/commit/receipt de M2 se rompió. El gate re-corre `full` sobre los bytes corregidos.
