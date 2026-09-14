# W11 — corregir dos pines obsoletos del conteo de tools en tests runtime (`31 → 36`)

Modelo solicitado: Claude Sonnet 5 (`claude -p --model sonnet --effort medium`). Rol: worker de implementación (edición mecánica de dos aserciones de test). Orquestador: Claude Fable 5.1 (como Opus 4.8). No puedes lanzar subagentes. **Nunca corras un comando en segundo plano. No corras Docker. No hagas commit.**

## Contexto

M6 elevó el inventario público de tools de **31 → 36** (dffa620 añadió `rust.analyzer.{symbols,references,diagnostics,actions,action.apply}`). W08 actualizó las tres aserciones `tools.len()` de `protocol.rs` y los snapshots, pero **dos** aserciones de conteo en tests runtime-only (`#[ignore]`, solo se ejecutan bajo el gate `full`) quedaron obsoletas en `31` y hacen fallar el gate:

1. `crates/mcp-server/tests/inspection_runtime/fix_mutation.rs:262` — dentro de un `assert_eq!` sobre `tools/list`, el valor esperado es `Some(31)`. Cámbialo a `Some(36)`.
2. `crates/mcp-server/tests/inspection_runtime/nextest.rs:37` — `assert_eq!(tools.len(), 31);`. Cámbialo a `assert_eq!(tools.len(), 36);`.

Ambos son el conteo total del inventario tras `tools/list`, no un request-id ni un índice. **No toques nada más.** En particular NO cambies: `protocol.rs` `tools[31]` (índice del primer tool M6, correcto), `actions.rs:1029 Some(31)` (número de *acciones* acotadas, no de tools), el comentario de `release-smoke.py` (ya narra "…36 since M6-04/M6-05"), ni `catalog-adapter/.../lock.rs:1119` (conteo de paquetes).

## Verificación (foreground, sin Docker)

Antes de terminar:
1. `grep -rnE "tools.len\(\), 31|Some\(31\)" crates/mcp-server/tests/inspection_runtime/` debe devolver **cero** líneas.
2. Confirma que no quedó ningún otro conteo de inventario obsoleto en `31` en `crates/mcp-server/tests/inspection_runtime/` (revisa cada `tools`/`tools/list` cercano).
3. `cargo check -p rust-engineering-mcp --tests --locked --offline` compila limpio.

## Entrega

Reporta: Task / Result / Files changed (las dos líneas exactas, antes→después) / Tests executed (salida de `grep` y `cargo check`) / Risks / Open issues. No hagas commit; el orquestador re-corre el gate `full` sobre los bytes finales.
